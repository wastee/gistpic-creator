# 角色分类器训练工具

这个工具训练一个轻量文字块分类器，判断候选块是字幕、水印、弹幕还是其他文字。它是开发工具，不随桌面应用发布。

## 当前状态

训练数据和模型权重尚未提供，应用还没有接入分类器。桌面版目前仍是 Release 1 的几何规则版本。完成数据标注、训练并通过验收门槛后，才把 `assets/text-block-classifier.bin` 接入应用。

## 数据格式

先用桌面版仓库里的导出工具生成候选块：

```bash
cargo run --release --features dev-tools --example export_candidate_dataset -- \
  --input data/screenshots --out data/candidate-dataset
```

`data/screenshots` 按视频来源分目录，例如：

```text
data/screenshots/
  视频A/
    0001.png
    0002.png
  视频B/
    0001.png
```

输出：

```text
data/candidate-dataset/
  crops/crop_000000.png
  crops/crop_000001.png
  manifest.csv
```

`manifest.csv` 的列：

```text
crop,source,label,cx,cy,w,h,lines,max_line_h,top_gap,bottom_gap
```

`label` 需要人工填写，取值：

```text
subtitle
watermark
danmaku
other
```

训练工具按 `source` 切分训练集、验证集、测试集，避免同一视频同时出现在训练和测试里。

## 训练

```bash
cargo run --release --manifest-path tools/role-trainer/Cargo.toml -- \
  --manifest data/candidate-dataset/manifest.csv \
  --output assets/text-block-classifier.bin \
  --epochs 30 \
  --seed 42
```

默认发布门槛：

- 测试集字幕精确率 ≥ 0.99；
- 测试集字幕召回率 ≥ 0.95。

未达到门槛时训练命令返回失败，不会写出可供应用使用的模型。确需调整门槛时用 `--min-precision` 和 `--min-recall`，但降低门槛不能算作达到验收要求。

## 模型文件

训练工具输出 `assets/text-block-classifier.bin`，内容依次为：

- 8 字节魔术字 `GPRC0001`；
- 版本号 `u32`；
- 输入宽、高 `u32`；
- 几何特征数量 `u32`；
- 分类数量 `u32`；
- 权重数量 `u32`；
- 字幕概率阈值 `f32`；
- 权重 FNV-1a 校验和 `u32`；
- 权重按固定顺序排列的 `f32` 小端字节。

权重顺序固定为：

```text
conv1.weight, conv1.bias
conv2.weight, conv2.bias
conv3.weight, conv3.bias
conv4.weight, conv4.bias
fc_image.weight, fc_image.bias
fc_geometry.weight, fc_geometry.bias
fc1.weight, fc1.bias
fc2.weight, fc2.bias
```

应用侧必须用同一结构、同一预处理和同一几何特征顺序做推理，否则模型输出无效。

## 数据处理约定

- 原始截图和候选块不提交到仓库。
- 只使用自有或已获授权的截图。
- 训练、验证、测试按视频来源切分，测试集冻结。
- 训练集需要包含大量水印、弹幕、标题、招牌等困难负例，尤其是容易和字幕混淆的样本。
