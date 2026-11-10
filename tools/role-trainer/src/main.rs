//! 角色分类器训练工具。
//!
//! 数据来自 `examples/export_candidate_dataset.rs` 生成的 `manifest.csv`。
//! 使用 Rust 的 Candle 训练一个小型卷积网络，输出应用使用的二进制权重文件。
//!
//! 用法：
//! `cargo run --manifest-path tools/role-trainer/Cargo.toml --release -- \
//!    --manifest data/candidate-dataset/manifest.csv \
//!    --output assets/text-block-classifier.bin --epochs 30 --seed 42`

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{
    AdamW, Conv2d, Conv2dConfig, Linear, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap,
    conv2d, linear, ops,
};
const INPUT_WIDTH: usize = 96;
const INPUT_HEIGHT: usize = 48;
const GEOMETRY_LEN: usize = 8;
const CLASS_COUNT: usize = 4;
const CLASS_SUBTITLE: usize = 0;
const MODEL_MAGIC: &[u8; 8] = b"GPRC0001";
const MODEL_VERSION: u32 = 1;

fn main() {
    if let Err(err) = run() {
        eprintln!("训练失败：{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse()?;
    let mut samples = load_samples(&args.manifest)?;
    if samples.is_empty() {
        bail!("清单里没有带标签的候选块");
    }
    if samples.len() < 3 {
        bail!("至少需要 3 个带标签的候选块才能切分训练、验证和测试集");
    }
    let (train, validation, test) = split_samples(&mut samples, args.seed);
    let train_counts = class_counts(&train);
    let class_weights = class_weights(&train_counts);

    println!(
        "样本：训练 {}，验证 {}，测试 {}；类别计数 {:?}",
        train.len(),
        validation.len(),
        test.len(),
        train_counts
    );

    let device = Device::Cpu;
    let varmap = VarMap::new();
    let vb = VarBuilder::from_varmap(&varmap, DType::F32, &device);
    let model = Net::new(&vb)?;
    let mut optimizer = AdamW::new(
        varmap.all_vars(),
        ParamsAdamW {
            lr: args.learning_rate,
            ..Default::default()
        },
    )?;

    let mut best_threshold = 0.5f32;
    let mut best_f1 = -1.0f32;
    for epoch in 1..=args.epochs {
        train_epoch(
            &model,
            &mut optimizer,
            &train,
            &class_weights,
            args.batch_size,
            args.seed.wrapping_add(epoch as u64),
            &device,
        )?;
        let probs = predict(&model, &validation, &device)?;
        let (threshold, precision, recall, f1) = choose_threshold(&probs, &validation);
        if f1 > best_f1 {
            best_f1 = f1;
            best_threshold = threshold;
        }
        println!(
            "epoch {epoch:02}/{}: val precision={precision:.4}, recall={recall:.4}, f1={f1:.4}, threshold={threshold:.2}",
            args.epochs
        );
    }

    let test_probs = predict(&model, &test, &device)?;
    let (test_precision, test_recall, test_f1) =
        metrics_at_threshold(&test_probs, &test, best_threshold);
    println!(
        "测试：precision={test_precision:.4}, recall={test_recall:.4}, f1={test_f1:.4}, threshold={best_threshold:.2}"
    );
    if test_precision < args.min_precision || test_recall < args.min_recall {
        bail!(
            "未达到发布门槛：需要 precision >= {:.4}、recall >= {:.4}，实际 precision={test_precision:.4}、recall={test_recall:.4}",
            args.min_precision,
            args.min_recall
        );
    }

    save_model(&args.output, &varmap, best_threshold)?;
    println!("模型已写入 {}", args.output.display());
    Ok(())
}

#[derive(Clone)]
struct Sample {
    image: Vec<f32>,
    geometry: [f32; 8],
    label: usize,
    source: String,
}

fn load_samples(manifest: &Path) -> Result<Vec<Sample>> {
    let text = std::fs::read_to_string(manifest)
        .with_context(|| format!("读取清单 {} 失败", manifest.display()))?;
    let base = manifest.parent().unwrap_or_else(|| Path::new("."));
    let mut samples = Vec::new();
    let mut skipped_unlabeled = 0usize;
    for (line_index, line) in text.lines().enumerate() {
        if line_index == 0 || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 11 {
            bail!("清单第 {} 行列数不足", line_index + 1);
        }
        let label_text = fields[2].trim();
        if label_text.is_empty() {
            skipped_unlabeled += 1;
            continue;
        }
        let label = match label_text {
            "subtitle" => CLASS_SUBTITLE,
            "watermark" => 1,
            "danmaku" => 2,
            "other" => 3,
            other => bail!("清单第 {} 行未知标签：{other}", line_index + 1),
        };
        let crop_path = base.join(fields[0]);
        let image = load_image(&crop_path)
            .with_context(|| format!("读取候选图 {} 失败", crop_path.display()))?;
        let geometry = parse_geometry(&fields[3..11])
            .with_context(|| format!("清单第 {} 行几何特征无效", line_index + 1))?;
        samples.push(Sample {
            image,
            geometry,
            label,
            source: fields[1].to_string(),
        });
    }
    if skipped_unlabeled > 0 {
        println!("跳过 {skipped_unlabeled} 个未标注候选块");
    }
    Ok(samples)
}

fn load_image(path: &Path) -> Result<Vec<f32>> {
    let image = image::open(path)?.to_rgb8();
    if image.width() != INPUT_WIDTH as u32 || image.height() != INPUT_HEIGHT as u32 {
        bail!(
            "候选图尺寸应为 {INPUT_WIDTH}×{INPUT_HEIGHT}，实际 {}×{}",
            image.width(),
            image.height()
        );
    }
    let mut data = vec![0f32; 3 * INPUT_HEIGHT * INPUT_WIDTH];
    for (x, y, pixel) in image.enumerate_pixels() {
        let offset = y as usize * INPUT_WIDTH + x as usize;
        data[offset] = pixel[0] as f32 / 255.0;
        data[INPUT_HEIGHT * INPUT_WIDTH + offset] = pixel[1] as f32 / 255.0;
        data[2 * INPUT_HEIGHT * INPUT_WIDTH + offset] = pixel[2] as f32 / 255.0;
    }
    Ok(data)
}

fn parse_geometry(fields: &[&str]) -> Result<[f32; 8]> {
    if fields.len() != GEOMETRY_LEN {
        bail!("需要 {GEOMETRY_LEN} 个几何特征");
    }
    let mut geometry = [0f32; GEOMETRY_LEN];
    for (index, field) in fields.iter().enumerate() {
        geometry[index] = field.parse::<f32>()?.clamp(0.0, 1.0);
    }
    Ok(geometry)
}

fn split_samples(samples: &mut [Sample], seed: u64) -> (Vec<Sample>, Vec<Sample>, Vec<Sample>) {
    let mut sources: Vec<String> = samples.iter().map(|sample| sample.source.clone()).collect();
    sources.sort();
    sources.dedup();
    shuffle(&mut sources, seed);

    let train_end = (sources.len() as f32 * 0.70).round() as usize;
    let validation_end = (sources.len() as f32 * 0.85).round() as usize;
    let source_role = |source: &str| {
        let position = sources
            .iter()
            .position(|candidate| candidate == source)
            .unwrap_or(0);
        if position < train_end {
            0
        } else if position < validation_end {
            1
        } else {
            2
        }
    };

    let mut train = Vec::new();
    let mut validation = Vec::new();
    let mut test = Vec::new();
    for sample in samples.iter() {
        match source_role(&sample.source) {
            0 => train.push(sample.clone()),
            1 => validation.push(sample.clone()),
            _ => test.push(sample.clone()),
        }
    }

    // 数据很少时按来源切分可能得到空训练集、验证集或测试集；用固定规则补足，保证流程可运行。
    if train.is_empty() || validation.is_empty() || test.is_empty() {
        let mut all = samples.to_vec();
        shuffle(&mut all, seed);
        let train_end = (all.len() as f32 * 0.70).round() as usize;
        let validation_end = (all.len() as f32 * 0.85).round() as usize;
        let train_end = train_end.clamp(1, all.len().saturating_sub(2).max(1));
        let validation_end = validation_end.clamp(
            train_end + 1,
            all.len().saturating_sub(1).max(train_end + 1),
        );
        train = all[..train_end].to_vec();
        validation = all[train_end..validation_end.min(all.len())].to_vec();
        test = all[validation_end.min(all.len())..].to_vec();
    }
    (train, validation, test)
}

fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed.wrapping_add(0x9e3779b97f4a7c15);
    for index in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let swap = (state as usize) % (index + 1);
        items.swap(index, swap);
    }
}

fn class_counts(samples: &[Sample]) -> [usize; CLASS_COUNT] {
    let mut counts = [0usize; CLASS_COUNT];
    for sample in samples {
        counts[sample.label] += 1;
    }
    counts
}

fn class_weights(counts: &[usize; CLASS_COUNT]) -> [f32; CLASS_COUNT] {
    let total: usize = counts.iter().sum();
    let mut weights = [1f32; CLASS_COUNT];
    for (index, count) in counts.iter().enumerate() {
        weights[index] = (total as f32 / (CLASS_COUNT as f32 * (*count).max(1) as f32)).max(0.1);
    }
    weights
}

struct Net {
    conv1: Conv2d,
    conv2: Conv2d,
    conv3: Conv2d,
    conv4: Conv2d,
    fc_image: Linear,
    fc_geometry: Linear,
    fc1: Linear,
    fc2: Linear,
}

impl Net {
    fn new(vb: &VarBuilder) -> Result<Self> {
        let conv_config = Conv2dConfig {
            padding: 1,
            ..Default::default()
        };
        Ok(Self {
            conv1: conv2d(3, 24, 3, conv_config, vb.pp("conv1"))?,
            conv2: conv2d(24, 48, 3, conv_config, vb.pp("conv2"))?,
            conv3: conv2d(48, 96, 3, conv_config, vb.pp("conv3"))?,
            conv4: conv2d(96, 128, 3, conv_config, vb.pp("conv4"))?,
            fc_image: linear(128, 64, vb.pp("fc_image"))?,
            fc_geometry: linear(GEOMETRY_LEN, 16, vb.pp("fc_geometry"))?,
            fc1: linear(64 + 16, 32, vb.pp("fc1"))?,
            fc2: linear(32, CLASS_COUNT, vb.pp("fc2"))?,
        })
    }

    fn forward(&self, image: &Tensor, geometry: &Tensor) -> Result<Tensor> {
        let mut x = self.conv1.forward(image)?.relu()?;
        x = x.max_pool2d(2)?;
        x = self.conv2.forward(&x)?.relu()?;
        x = x.max_pool2d(2)?;
        x = self.conv3.forward(&x)?.relu()?;
        x = x.max_pool2d(2)?;
        x = self.conv4.forward(&x)?.relu()?;
        let x = x.mean_keepdim(D::Minus1)?.mean_keepdim(D::Minus2)?;
        let x = x.flatten_from(1)?;
        let x = self.fc_image.forward(&x)?.relu()?;
        let geometry = self.fc_geometry.forward(geometry)?.relu()?;
        let joined = Tensor::cat(&[x, geometry], 1)?;
        let joined = self.fc1.forward(&joined)?.relu()?;
        Ok(self.fc2.forward(&joined)?)
    }
}

fn train_epoch(
    model: &Net,
    optimizer: &mut AdamW,
    samples: &[Sample],
    class_weights: &[f32; CLASS_COUNT],
    batch_size: usize,
    seed: u64,
    device: &Device,
) -> Result<()> {
    let mut indices: Vec<usize> = (0..samples.len()).collect();
    shuffle(&mut indices, seed);
    for batch_start in (0..indices.len()).step_by(batch_size.max(1)) {
        let batch_indices = &indices[batch_start..(batch_start + batch_size).min(indices.len())];
        let (images, geometry, labels) = batch_tensors(samples, batch_indices, device)?;
        let logits = model.forward(&images, &geometry)?;
        let log_probs = ops::log_softmax(&logits, 1)?;
        let picked = log_probs
            .gather(&labels.unsqueeze(1)?, 1)?
            .squeeze(1)?
            .neg()?;
        let weight_tensor = Tensor::from_vec(class_weights.to_vec(), (CLASS_COUNT,), device)?;
        let weights = weight_tensor.gather(&labels, 0)?;
        let loss = (picked * weights)?.mean_all()?;
        optimizer.backward_step(&loss)?;
    }
    Ok(())
}

fn batch_tensors(
    samples: &[Sample],
    indices: &[usize],
    device: &Device,
) -> Result<(Tensor, Tensor, Tensor)> {
    let mut images = Vec::with_capacity(indices.len() * 3 * INPUT_HEIGHT * INPUT_WIDTH);
    let mut geometry = Vec::with_capacity(indices.len() * GEOMETRY_LEN);
    let mut labels = Vec::with_capacity(indices.len());
    for index in indices {
        let sample = &samples[*index];
        images.extend_from_slice(&sample.image);
        geometry.extend_from_slice(&sample.geometry);
        labels.push(sample.label as u32);
    }
    let image_tensor = Tensor::from_vec(
        images,
        (indices.len(), 3, INPUT_HEIGHT, INPUT_WIDTH),
        device,
    )?;
    let geometry_tensor = Tensor::from_vec(geometry, (indices.len(), GEOMETRY_LEN), device)?;
    let label_tensor = Tensor::from_vec(labels, (indices.len(),), device)?;
    Ok((image_tensor, geometry_tensor, label_tensor))
}

fn predict(model: &Net, samples: &[Sample], device: &Device) -> Result<Vec<[f32; CLASS_COUNT]>> {
    let indices: Vec<usize> = (0..samples.len()).collect();
    let (images, geometry, _) = batch_tensors(samples, &indices, device)?;
    let logits = model.forward(&images, &geometry)?.to_dtype(DType::F32)?;
    let probs = ops::softmax(&logits, 1)?.to_vec2::<f32>()?;
    let mut result = Vec::with_capacity(probs.len());
    for row in probs {
        result.push([row[0], row[1], row[2], row[3]]);
    }
    Ok(result)
}

fn choose_threshold(
    probabilities: &[[f32; CLASS_COUNT]],
    samples: &[Sample],
) -> (f32, f32, f32, f32) {
    let mut best = (0.5f32, 0f32, 0f32, -1f32);
    let mut threshold = 0.05f32;
    while threshold <= 0.95 {
        let (precision, recall, f1) = metrics_at_threshold(probabilities, samples, threshold);
        if f1 > best.3 {
            best = (threshold, precision, recall, f1);
        }
        threshold += 0.05;
    }
    best
}

fn metrics_at_threshold(
    probabilities: &[[f32; CLASS_COUNT]],
    samples: &[Sample],
    threshold: f32,
) -> (f32, f32, f32) {
    let mut true_positive = 0usize;
    let mut false_positive = 0usize;
    let mut false_negative = 0usize;
    for (probability, sample) in probabilities.iter().zip(samples.iter()) {
        let predicted = probability[CLASS_SUBTITLE] >= threshold;
        let actual = sample.label == CLASS_SUBTITLE;
        match (predicted, actual) {
            (true, true) => true_positive += 1,
            (true, false) => false_positive += 1,
            (false, true) => false_negative += 1,
            (false, false) => {}
        }
    }
    let precision = if true_positive + false_positive == 0 {
        0.0
    } else {
        true_positive as f32 / (true_positive + false_positive) as f32
    };
    let recall = if true_positive + false_negative == 0 {
        0.0
    } else {
        true_positive as f32 / (true_positive + false_negative) as f32
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    (precision, recall, f1)
}

fn save_model(path: &Path, varmap: &VarMap, threshold: f32) -> Result<()> {
    let names = [
        "conv1.weight",
        "conv1.bias",
        "conv2.weight",
        "conv2.bias",
        "conv3.weight",
        "conv3.bias",
        "conv4.weight",
        "conv4.bias",
        "fc_image.weight",
        "fc_image.bias",
        "fc_geometry.weight",
        "fc_geometry.bias",
        "fc1.weight",
        "fc1.bias",
        "fc2.weight",
        "fc2.bias",
    ];
    let data = varmap.data().lock().unwrap();
    let mut weights = Vec::new();
    for name in names {
        let var = data
            .get(name)
            .with_context(|| format!("模型缺少权重 {name}"))?;
        let values = var.as_tensor().flatten_all()?.to_vec1::<f32>()?;
        weights.extend_from_slice(&values);
    }
    drop(data);

    let mut output = Vec::new();
    output.extend_from_slice(MODEL_MAGIC);
    output.extend_from_slice(&MODEL_VERSION.to_le_bytes());
    output.extend_from_slice(&(INPUT_WIDTH as u32).to_le_bytes());
    output.extend_from_slice(&(INPUT_HEIGHT as u32).to_le_bytes());
    output.extend_from_slice(&(GEOMETRY_LEN as u32).to_le_bytes());
    output.extend_from_slice(&(CLASS_COUNT as u32).to_le_bytes());
    output.extend_from_slice(&(weights.len() as u32).to_le_bytes());
    output.extend_from_slice(&threshold.to_le_bytes());
    output.extend_from_slice(&fnv1a(&weights).to_le_bytes());
    for value in weights {
        output.extend_from_slice(&value.to_le_bytes());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, output)?;
    Ok(())
}

fn fnv1a(values: &[f32]) -> u32 {
    let mut hash = 2_166_136_261u32;
    for value in values {
        for byte in value.to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

struct Args {
    manifest: PathBuf,
    output: PathBuf,
    epochs: usize,
    batch_size: usize,
    learning_rate: f64,
    seed: u64,
    min_precision: f32,
    min_recall: f32,
}

impl Args {
    fn parse() -> Result<Self> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut manifest = None;
        let mut output = None;
        let mut epochs = 30usize;
        let mut batch_size = 64usize;
        let mut learning_rate = 0.001f64;
        let mut seed = 42u64;
        let mut min_precision = 0.99f32;
        let mut min_recall = 0.95f32;
        let mut index = 0;
        while index < args.len() {
            let value = args.get(index + 1);
            match args[index].as_str() {
                "--manifest" => {
                    manifest = value.map(PathBuf::from);
                    index += 2;
                }
                "--output" => {
                    output = value.map(PathBuf::from);
                    index += 2;
                }
                "--epochs" => {
                    epochs = value.context("--epochs 缺少值")?.parse()?;
                    index += 2;
                }
                "--batch-size" => {
                    batch_size = value.context("--batch-size 缺少值")?.parse()?;
                    index += 2;
                }
                "--learning-rate" => {
                    learning_rate = value.context("--learning-rate 缺少值")?.parse()?;
                    index += 2;
                }
                "--seed" => {
                    seed = value.context("--seed 缺少值")?.parse()?;
                    index += 2;
                }
                "--min-precision" => {
                    min_precision = value.context("--min-precision 缺少值")?.parse()?;
                    index += 2;
                }
                "--min-recall" => {
                    min_recall = value.context("--min-recall 缺少值")?.parse()?;
                    index += 2;
                }
                other => bail!("未知参数：{other}"),
            }
        }
        Ok(Self {
            manifest: manifest.context("缺少 --manifest")?,
            output: output.context("缺少 --output")?,
            epochs,
            batch_size,
            learning_rate,
            seed,
            min_precision,
            min_recall,
        })
    }
}
