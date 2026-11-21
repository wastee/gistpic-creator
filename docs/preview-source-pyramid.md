# 预览源分辨率瓦片金字塔设计

本文只设计预览的像素缓存和渲染，不改导出、复制、裁剪、排版、缝隙、圆角、缩放交互。目标是把预览图块从“按当前显示尺寸生成”改成“按图片源分辨率分层生成”，让换排版、换裁剪、加图片、缩放都只改变取样和摆放，不再重建整张作品的像素。

## 1. 结论

采用每张图片一套源分辨率瓦片金字塔：

- 第 0 层是原图分辨率，第 k 层是原图按 2 的 k 次方缩小的分辨率，最粗的顶层（最长边不超过 512）常驻内存，作为整张图的完整底图。
- 图块按“图片 + 源版本 + 源分辨率层 + 图块坐标”缓存。排版、裁剪带、缝隙、圆角、画布缩放都不进入缓存键。
- 预览按当前显示比例选出目标层；从顶层（最粗）到目标层逐层画，目标层没准备好的地方由更粗的层盖住，目标层只画已经生成的块。
- 顶层永远完整，因此图片块内部不会再露出作品底色；换排版、加图、缩放都不会让已有图片的像素层失效。
- 只缓存顶层、目标层和少量相邻层的可见图块，外加一个按预算淘汰的源图解码缓存；不常驻所有层，不生成比源分辨率更大的图块。

这个设计把完整底图从临时缩略图补丁变成金字塔的正式一层，也把布局变化重算像素从根上去掉。

## 2. 目标和约束

### 2.1 功能需求

预览必须正确表现：

- 横排、竖排、循环排版、字幕模式；
- 每张图的裁剪带、手动裁剪；
- 缝隙宽度 0 到 100、缝隙颜色、缝隙颜色淡入；
- 作品外沿、图块圆角、图块之间的 1 物理像素取整误差；
- 拖动排序、多选、裁剪拖动、缩略图导航；
- 手动缩放 5% 到 400%、适应窗口、1:1；
- 系统缩放因子 1.0、1.5、2.0；
- 新增、删除、清空、替换图片；
- 复制和导出结果与预览一致。

### 2.2 现有预览的约束

- 屏幕按物理像素采样，图块是 512×512 物理像素。
- GPUI 用 img 元素逐块贴图，没有自定义着色器和 GPU mipmap 接口。
- 图块、缩略图、源图解码都在后台线程生成，UI 线程不能做整图解码或缩放。
- 导出路径用整张原图合成，预览不能改变原图或裁剪范围。

### 2.3 设计目标

1. 图片块内部任何位置都不能露出作品底色；这由结构保证，不靠刚好补上。
2. 换排版、换裁剪、换顺序、加图片时，已有图片的像素层不重建。
3. 缩放时只换取样层，不重算同一份源像素。
4. 100% 以上不生成比源图更大的图块。
5. 内存有上限，跟窗口大小、图片数量、源图缓存预算相关。
6. 源图只解码一次，除非被缓存淘汰后再次需要。
7. 保留现有的 1 物理像素贴边补偿、图块 1 像素重叠、圆角和裁剪行为。
8. 导出路径零改动。

### 2.4 不做什么

- 不引入 GPU mipmap、虚拟纹理、自定义渲染管线；GPUI 的逐块贴图继续用。
- 不把整张作品合成成一张常驻大图；那样内存和刷新成本都不可控。
- 不在 UI 线程同步解码或缩放。
- 不改导出、复制、裁剪算法和排版算法。
- 不要求所有层、所有图块都缓存；只缓存需要的可见部分。

## 3. 现有实现的问题

- src/app/tiles.rs 的 TileSpec 把“图片 + 当前显示尺寸 + 当前系统缩放后的物理尺寸”绑在一起；显示尺寸一变，same_content 为假，TileGrid::sync_with 换代号，整张图的图块重新生成。
- sync_with 在换代时把当前代次降为兜底代次，但当前缓存只保留屏幕内的块；缩放或滚动让可见范围变大时，原本在屏幕外、已经释放的区域没有兜底块。
- src/ui/canvas.rs 第 681 行只有在 tile_views 一块都没有时才用缩略图或底色垫底；只要还剩一块，缺的区域就是透明的，直接露出作品底色的白色。
- src/app.rs 的 build_tile_images 每次生成任务都重新 load_rgb 解码整张源图；换排版、换缩放都要重复解码和缩放。
- 100% 以上仍然按显示物理尺寸生成图块；400% 时生成的是源图放大后的图块，像素量是源图的 16 倍，既费内存也费 CPU，却没有增加任何真实细节。
- 缩略图固定 256 宽，只在完全没有图块时使用；它不是一个正式的完整分辨率层。

## 4. 总体结构

每张图片对应一个 ImagePyramid：

- levels：从第 0 层（原图）到第 K 层（顶层）的元数据；
- top：顶层的一张完整 RenderImage，常驻；
- lod：这张图当前选择的显示层；
- 全局 TileCache：按“图片 + 源版本 + 层 + 列 + 行”存图块，带 LRU 和内存预算；
- 全局 SourceCache：按源版本缓存解码后的 RGB 像素，带独立预算；
- TileScheduler：按优先级批量生成顶层、目标层和预取层图块，每个任务只解码一次源图；
- 渲染计划 BlockPlan：每帧给出从顶层到目标层的若干层，层内只包含已经生成的可见图块；缺的块由更粗的层盖住。

数据流：

~~~
源图文件或剪贴板
  │ 解码一次（SourceCache）
  ▼
源分辨率层级（只生成需要的图块和顶层）
  │
  ├─ 顶层完整纹理（常驻，导航/占位/兜底）
  ├─ 中间层可见图块（按需生成，LRU）
  └─ 目标层可见图块（按需生成，LRU）
  │
  ▼
BlockPlan：从粗到细的层列表
  │
  ▼
render_block：逐层贴图，块容器裁切，缝隙底色只在块外可见
~~~

## 5. 数据模型

### 5.1 源版本

~~~
struct SourceRevision {
    item: ItemId,
    fingerprint: u64,
    width: u32,
    height: u32,
}
~~~

源路径、剪贴板字节、图片像素、尺寸任一变化都产生新的 revision。金字塔、图块缓存、源缓存、生成任务都以 revision 为键；旧 revision 的迟到结果直接丢弃。

### 5.2 层级

~~~
struct LevelMeta {
    index: u32,
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
}

struct ImagePyramid {
    revision: SourceRevision,
    levels: Vec<LevelMeta>,
    top: Option<Arc<RenderImage>>,
    lod: LodState,
}
~~~

### 5.3 图块

~~~
struct TileKey {
    revision: SourceRevision,
    level: u32,
    col: u32,
    row: u32,
}

struct TileEntry {
    image: Arc<RenderImage>,
    bytes: usize,
    last_used: u64,
    pinned: bool,
}
~~~

TileKey 不含显示尺寸、裁剪带、排版、缝隙、圆角。显示尺寸变化不会让 key 失效。

### 5.4 显示层选择状态

~~~
struct LodState {
    current: u32,
    base: u32,
    last_switch_frame: u64,
}
~~~

current 是带迟滞的显示层；base 是理论层。分开是为了避免缩放在层级边界附近来回抖动。

### 5.5 渲染计划

~~~
struct BlockPlan {
    target_level: u32,
    layers: Vec<LevelLayer>,
    missing_target: Vec<TileKey>,
    fallback_surface: bool,
}

struct LevelLayer {
    level: u32,
    tiles: Vec<TileView>,
}
~~~

## 6. 层级几何

### 6.1 层数和尺寸

源图尺寸记为 Sw 和 Sh，顶层最长边上限 TOP_MAX = 512。

~~~
K = 最小的非负整数 k，使 max(ceil(Sw / 2^k), ceil(Sh / 2^k)) <= TOP_MAX
L_k = (ceil(Sw / 2^k), ceil(Sh / 2^k))，k = 0..=K
~~~

- 第 0 层是原图分辨率，保留最多细节。
- 第 K 层是顶层，最长边不超过 512，常驻，作为完整底图、导航图、拖动小图和占位图。
- 源图本身不超过 512 时，K=0，顶层就是原图。

示例：2000×1000，TOP_MAX=512，K=2：

~~~
L0 = 2000×1000
L1 = 1000×500
L2 = 500×250   // 顶层
~~~

### 6.2 源坐标、层坐标、显示坐标

层 k 的像素宽高为 Lw×Lh。层像素 x_l 对应源坐标：

~~~
x_s = x_l * Sw / Lw
~~~

显示坐标，相对整张图片左上角的逻辑像素：

~~~
x_d = x_s * (full_display_width / Sw)
    = x_l * (full_display_width / Lw)
~~~

其中 full_display_width 来自 render_block 现有的：

~~~
scale_x = entry.display_width / band.width()
full_width = item.width * scale_x
~~~

即整张原图在屏幕上的逻辑宽高，不受裁剪带影响。裁剪带只负责用 block 容器裁切，不参与像素层映射。

物理像素密度：

~~~
D = full_width * device_scale
~~~

### 6.3 图块网格

每层独立切 512×512 的层像素图块：

~~~
tiles_x = ceil(Lw / 512)
tiles_y = ceil(Lh / 512)
core = (col*512, row*512, min(512, Lw - col*512), min(512, Lh - row*512))
~~~

每块图块在四条边上多取 B = 1 个层像素，作为采样边界：

~~~
texture_rect = core 向外扩 1 个层像素，再夹到 [0, Lw]×[0, Lh]
texture_size = texture_rect.width × texture_rect.height
~~~

图块纹理里存的是 texture_rect 对应的层图像内容，不是整层。相邻图块的纹理在接缝处重叠 2 个层像素；贴到屏幕后重叠 1 到 2 个显示像素，和现在的 TILE_OVERLAP 思路一致，但单位从目标物理像素改成了层像素。

### 6.4 选层

给定整张图在屏幕上的物理宽度 D，选最粗的、像素宽高都不小于 D 的层：

~~~
k = 0
while k < K and L_{k+1}.width >= D_width and L_{k+1}.height >= D_height:
    k += 1
target_base = k
~~~

- D 大于等于原图宽高时，k=0，使用原图，不再生成更大的层。
- D 小于等于顶层宽高时，k=K，直接使用顶层。
- 其余情况 Lw/D 落在 1 到 2 之间：纹理像素至少和显示像素一样多，最多两倍，既不模糊也不过度生成。

示例：2000×1000 的图，D=1000 时选 L1（1000×500）；D=600 时选 L1（1000×500，1.67 倍）；D=500 时选 L2（500×250，顶层）。

### 6.5 迟滞和预取

直接在层级边界切换会抖动：D 在 Lw 附近来回一点，层就在 k 和 k-1 之间反复。

设当前层 current，当前层的像素密度比：

~~~
r = min(Lw_current / D_width, Lh_current / D_height)
~~~

规则：

- r 小于 0.98 且 current 大于 0：当前层已经不够清晰，切到更细的一层，current 减 1；
- r 大于 2.04 且 current 小于 K：当前层过度清晰，切到更粗的一层，current 加 1；
- 其它情况保持 current；
- 初次进入、窗口尺寸大幅变化或 current 不在合法范围时，直接取 target_base。

预取：

- 当 r 小于 1.25 且 current 大于 0 时，提前请求 current 减 1 层的可见图块，用户继续放大时目标层已经准备好。
- 用户快速缩放跨过多层时，只保留最终 current 的目标层；中间层不逐个生成，顶层和目标层优先。
- 目标层切换后，旧层图块保留在缓存里，由 LRU 决定去留；它们不会让新层失效。

### 6.6 顶层

- 顶层是一张完整纹理，不是图块网格，大小不超过 512×512 长边；它的宽高使用第 K 层的实际尺寸 L_K，不固定按 512 宽生成，竖长图的顶层高度也不会超过 512。
- 顶层在图片加入后最高优先级生成，生成后常驻，不参与 LRU 淘汰。
- 顶层在渲染计划里当成“只有一块的层”处理：这一层的 TileView 直接用整张顶层纹理，TileView 的矩形是整张图在屏幕上的 full_width×full_height。目标层等于顶层时，画面就是这一层；目标层比顶层细时，顶层作为最粗的底层先画。源图本身不超过 512 时 K=0，这一层就是原图，不会再生成同尺寸的图块。
- 顶层同时作为导航器缩略图、拖动小图、选中卡片的预览图、图片块在目标层未就绪时的完整底图。
- 不再保留单独的 256 宽 thumb_cache；顶层就是完整低分辨率层。这样缩略图和底图是同一份像素，不会出现两套缓存不同步。

## 7. 渲染计划

### 7.1 每帧计算

对每个可见图片块：

1. 用 render_block 现有的 full_width、full_height、device_scale 算出 D 和可见矩形；
2. 更新 LodState，得到 target = lod.current；
3. 从 target 到 K 逐层收集可见范围内已经生成的图块；
4. 目标层缺的块放进 missing_target，交给调度器；目标层就是顶层时，missing_target 只表示顶层请求，不再生成同尺寸图块；
5. 如果顶层还没有生成，fallback_surface 为真；不管目标层有没有部分块，块底层都要先铺占位底色。

伪代码：

~~~
fn plan_block(pyramid, block, viewport, cache) -> BlockPlan {
    let d = block.full_display_width * block.device_scale;
    let target = pyramid.lod.update(d);

    let mut layers = Vec::new();
    for level in (target..=pyramid.top_level()).rev() {
        let ready: Vec<_> = if level == pyramid.top_level() {
            // 顶层是完整纹理，不查图块缓存，直接作为这一层的唯一一块。
            pyramid.top.clone().map(|image| vec![TileView {
                key: TileKey::top(pyramid.revision),
                image,
                rect: block.full_display_rect(),
            }]).unwrap_or_default()
        } else {
            visible_tiles_at_level(pyramid, block, viewport, level)
                .into_iter()
                .filter_map(|key| cache.get(key).map(|image| TileView { key, image, ... }))
                .collect()
        };
        if !ready.is_empty() {
            layers.push(LevelLayer { level, tiles: ready });
        }
    }

    let missing = target_tiles(pyramid, block, viewport)
        .filter(|key| !cache.contains(key))
        .collect();

    BlockPlan {
        target_level: target,
        layers,
        missing_target: missing,
        // 顶层是唯一保证完整覆盖的层。顶层没到之前，即使目标层
        // 已经有部分块，底层也必须铺占位底色，不能只靠这些块。
        fallback_surface: pyramid.top.is_none(),
    }
}
~~~

layers 从粗到细排列。渲染时先画粗层，再画细层；同一层内按行优先画。

### 7.2 为什么不会露白

- 顶层是一张完整图片，永远覆盖整张图；只要它存在，块内任何位置至少有顶层像素。
- 中间层和目标层只画已经生成的块，不会留下声明了要画但没有纹理的空洞。
- 目标层缺失的区域由更粗的层覆盖；最坏情况退到顶层。
- 顶层还没生成时，块底层先用现有的 theme.surface 占位底色，再画已经就绪的其它层；占位色不是白色，也不会透出缝隙色。
- 顶层一旦生成，顶层就作为最粗的完整层替换占位底色，块内部的覆盖从这一层开始由结构保证。
- 块容器继续使用 overflow_hidden 和现有圆角，图片层被裁在块内，块外的缝隙底色照旧可见。

这和“图块没到就用缩略图垫底”的区别是：顶层是金字塔的第一层，是完整图片的正式表示；不是某个显示尺寸的临时补丁，也不会因为换排版、加图、缩放而失效。

### 7.3 图块绘制

每个 TileView 的屏幕矩形：

~~~
x = image_left + texture_rect.x * full_width / Lw
y = image_top  + texture_rect.y * full_height / Lh
w = texture_rect.width  * full_width / Lw
h = texture_rect.height * full_height / Lh
~~~

- image_left 和 image_top 来自现有 render_block 的裁剪带偏移；
- 纹理本身包含 1 个层像素的边界；纹理矩形比 core 大，贴到屏幕后相邻块自然重叠；
- 图块按行优先、从粗层到细层绘制；后画的细层盖住粗层；
- 块右沿或下沿贴作品边时，继续沿用 1.0.10 的 edge_cover：块框、顶层图片、目标层最右或最下块多画 1 个物理像素，由 base 的 overflow_hidden 裁掉。金字塔不改变这个取整补偿。

### 7.4 缩略图、导航和拖动

- 导航器、拖动小图、选中卡片预览统一用 pyramid.top；
- 目标层没有就绪时，选中卡片的图片区域也用顶层，不出现空洞；
- 删除图片或清空作品时，ImagePyramid、顶层、所有层图块、源缓存条目一起失效，迟到的生成结果按 revision 和 epoch 丢弃。

## 8. 图块生成

### 8.1 源缓存

~~~
struct SourceCache {
    entries: LruMap<SourceRevision, Arc<RgbImage>>,
    budget_bytes: usize,
    pending: HashSet<SourceRevision>,
}
~~~

- SourceImage::File：后台读取并解码一次，存 Arc；
- SourceImage::Clipboard：解码一次，存 Arc；
- SourceImage::Pixels(Arc)：直接共享，不 clone；
- 生成任务持有 Arc，任务结束释放；缓存按预算 LRU 淘汰；
- 同一个 revision 的并发请求合并成一个解码任务，避免多线程同时解码同一张大图。

### 8.2 顶层生成

- 用源图缩到顶层尺寸；使用现有的 preview_image 和 thumbnail 路径，专门做大幅缩小，速度快；
- 顶层是单个 RenderImage，生成后立即通知 UI；优先级高于所有目标层图块；
- 顶层生成失败时记录错误，图片块用 theme.surface 占位，不显示作品底色。

### 8.3 层图块生成

输入：revision、层 k、图块 core 矩形、源图 Arc。

步骤：

1. texture_rect 等于 core 向外扩 1 个层像素，再夹到层尺寸内；
2. 把 texture_rect 映射回源坐标：

~~~
sx0 = floor(texture_rect.x * Sw / Lw)
sy0 = floor(texture_rect.y * Sh / Lh)
sx1 = ceil(texture_rect.right * Sw / Lw)
sy1 = ceil(texture_rect.bottom * Sh / Lh)
~~~

3. 从源图裁出 [sx0,sx1)×[sy0,sy1)，缩放到 texture_rect.width×texture_rect.height；
4. 转成 BGRA，构造 RenderImage。

这样每块图块只读自己覆盖的源区域，不生成整层中间图；同一层的所有图块使用同一个层坐标到源坐标的映射，接缝处的采样相位一致，再加上 1 像素边界，不会出现接缝色差或细线。

### 8.4 缩放滤镜

- 顶层沿用 thumbnail，大幅缩小快且质量足够；
- 缩放倍数接近 2 倍时，用面积平均或 Triangle，采样范围正好覆盖一个输出像素；
- 缩放倍数大于 2 倍时，用现有的 thumbnail 路径（专门做大幅缩小，比 Lanczos3 快，质量在这个尺寸上看不出差别），或者先做 2 倍面积平均再做一次缩小；
- 不在每块上使用高开销的 Lanczos3；
- 同一层内所有图块使用同一种滤镜和同一映射，保证接缝一致；
- 对截图里的文字，优先保证不糊、不抖动，不追求照片级锐化。

### 8.5 边界块

- 层边缘的图块 core 被夹到层尺寸；texture_rect 也夹到层尺寸，纹理可能小于 core 加 2 像素；
- 贴图时纹理矩形按实际 texture_rect 算；块容器裁切；不会越界；
- 顶层整图只有一块，边缘按图片边界夹紧；顶层不参与图块缓存，也不生成同尺寸的重复图块。

## 9. 调度和并发

### 9.1 请求优先级

从高到低：

1. 当前可见图片的顶层（没有顶层就没有完整底图）；
2. 当前可见图片的目标层可见图块；
3. 目标层相邻块，预取一屏边缘外的一圈；
4. 预取层，用户可能继续放大；
5. 其它可见图片的目标层；
6. 非可见图片不生成。

### 9.2 批量生成

- 同一个 revision 和层级的缺块合并成一个任务，任务内只解码一次源图，逐块生成，逐块交回；
- 一个图片同一时刻最多一个生成任务；新请求合并到队列，不并发解码同一张图；
- 全局限制同时解码的图片数量，例如 2 张，避免多张大图同时解码把内存打满；
- UI 线程每帧最多接收固定数量新纹理，例如 8 块，其余下一帧，避免卡顿；粗层和顶层保证画面不空。

### 9.3 取消和迟到结果

- 每个任务带 revision 和 session_epoch；
- 任务返回时，如果 revision 已变、图片已删除、作品已清空，结果全部进 images_to_drop，由窗口释放 GPU 纹理；
- 图块已不在当前缓存预算内时，迟到结果也直接释放；
- 生成中的图块算 pinned，不参与 LRU 淘汰。

### 9.4 冻结

- 拖动裁剪线、拖动接缝、框选等交互期间，可以暂停新请求；已就绪的层继续画，顶层保证不露白；
- 拖动排序会让图片平移、循环排版会改尺寸；金字塔的像素层不受影响，只改变摆放和目标层。排序拖动期间可以继续请求目标层的新可见块，但不必重建已就绪的层。

## 10. 缓存和内存

### 10.1 缓存分层

- 顶层：每张图片一张，常驻，不淘汰；
- 层图块：按 revision、层、列、行存，LRU；当前帧使用中的块和生成中的块 pin 住；
- 源图：按 revision 存，LRU；只服务生成任务，不参与渲染 pin；
- 元数据：ImagePyramid、层尺寸、LOD 状态，很小。

### 10.2 预算

建议默认：

~~~
GPU 图块预算：256 MB
源图解码预算：256 MB
顶层总计：按图片数量，单张最多约 1 MB
~~~

- 512×512 RGBA 图块约 1 MB；256 MB 约 256 块，足够 4K 窗口的多层可见块；
- 源图预算按 Sw*Sh*3 计；一张 3840×2160 的截图约 25 MB；同时解码两张约 50 MB；
- 顶层 50 张图约 50 MB；图片数量特别多时，可以按预算把非可见顶层降级，但本项目的图片数量通常不需要；
- 预算不是硬性拒绝：当前帧可见层全部 pin 住时允许短暂超出，下一帧再按 LRU 回收。

### 10.3 淘汰

- 淘汰顺序：非可见层的旧图块，非可见图片的中间层，离目标层最远的层，非当前帧的图块；
- 顶层、当前帧使用的图块、生成中的图块不淘汰；
- 淘汰时把 Arc 放进 images_to_drop，由 UI 线程调用 window.drop_image；不能直接在后台线程 drop GPU 纹理；
- 源图缓存淘汰后，目标层图块仍在；再次需要生成新图块时重新解码源图。

### 10.4 内存对比

| 项目 | 旧实现 | 新设计 |
|---|---|---|
| 图块键 | 图片加当前显示物理尺寸 | 图片加源分辨率层加块坐标 |
| 换排版 | 整张图重生成 | 像素层复用，只改摆放和目标层 |
| 加图片 | 只加新图 | 只加新图顶层和目标层 |
| 100% 以上 | 生成放大图块，400% 时 16 倍像素 | 最多到原图层，不生成放大图块 |
| 完整底图 | 256 缩略图，只在一块的图块都没有时用 | 512 顶层，永远完整 |
| 源图解码 | 每次生成任务解码一次 | 按 revision 解码一次，预算缓存 |
| 内存上限 | 每张图可见图块加缩略图 | 顶层加可见层图块加源图预算 |

## 11. 接入现有代码

### 11.1 模块

- 新增 src/app/pyramid.rs：层几何、选层、LOD、BlockPlan、纯函数和单元测试；
- 新增 src/app/tile_cache.rs：TileKey、TileEntry、LRU、预算、pin、释放队列；
- 新增 src/app/source_cache.rs：源图解码缓存、pending 合并、revision；
- 修改 src/app/tiles.rs：删除 TileSpec 和 TileGrid 的显示尺寸逻辑；保留并迁移 visible_tiles、TileView、TilePlan 的思路，改成层坐标版本；
- 修改 src/app.rs：AppRoot 字段、tile_views_for、spawn_tile_build、build_tile_images、thumbnail_for；
- 修改 src/ui/canvas.rs：render_block 的 base 层、render_card 的预览图；
- 修改 src/ui/canvas/geometry.rs：不改变布局；只确认 full_width、full_height、image_left、image_top 继续使用。

### 11.2 AppRoot 字段变化

~~~
+ pyramids: HashMap<ItemId, ImagePyramid>
+ tile_cache: TileCache
+ source_cache: SourceCache
+ tile_scheduler: TileScheduler
- thumb_cache: HashMap<ItemId, Arc<RenderImage>>
- thumb_pending / thumb_failed
~~~

### 11.3 render_block 变化

当前：

~~~
TileSpec::for_image(...)
tile_views_for(...)
if tile_views.is_empty() { thumbnail or surface } else { for tile in tile_views }
~~~

改为：

~~~
let plan = self.plan_block(item, entry, viewport, freeze, cx);
...
if plan.fallback_surface { base.child(div().absolute().inset_0().bg(theme.surface)); }
for layer in &plan.layers {
    for tile in &layer.tiles {
        base.child(img(tile.image).absolute().left(...).top(...).w(...).h(...))
    }
}
~~~

- image_left、image_top、full_width、full_height、scale_x、scale_y 的计算不变；
- cover_right、cover_bottom、edge_cover 的 1 物理像素补偿保留；
- 图块显示矩形改用层坐标映射；不再用 TileEntry::display_rect 的旧显示尺寸归一化。

### 11.4 选择卡片和导航

- selection::render_card 当前接收 tile_views 和 thumbnail；改为接收 BlockPlan 的可见层和 pyramid.top；
- 导航器对每张图取 pyramid.top；不再依赖 thumb_cache；
- 拖动小图、选中卡片、导航器三处共用同一张顶层纹理，避免三套缩略图。

### 11.5 源图替换和删除

- 图片删除：从 pyramids、tile_cache、source_cache 移除；正在生成的任务按 revision 丢弃；
- 清空作品：所有金字塔、图块缓存、源缓存清空；epoch 增加；
- 源图替换或重新导入：新 revision；旧金字塔整体失效；旧图块进释放队列。

### 11.6 导出

src/app/export.rs、src/imaging/render.rs、src/imaging/layout.rs 不改。预览金字塔只读 SourceImage 和 Item 的尺寸、指纹，不修改它们。

## 12. 正确性边界

| 场景 | 行为 |
|---|---|
| 缩放 5% | 选顶层或接近顶层；顶层完整，不露白 |
| 缩放 400% | 选第 0 层，不生成放大图块；像素放大显示 |
| 换排版 | 层键不变；只改摆放和 LOD；已有像素复用 |
| 加图片 | 只生成新图顶层和目标层；旧图不动 |
| 拖裁剪线 | 层键不变；容器裁切；不重生成 |
| 字幕模式和排版模式切换 | 同一张图的全图显示尺寸可能变；LOD 可能变；像素层复用 |
| 缝隙 0 到 100、任意颜色 | 像素层不受影响；块外缝隙底色照旧 |
| 作品外沿取整 | 保留 1.0.10 的 1 物理像素补偿 |
| 选中卡片、导航、拖动 | 用顶层；不额外生成缩略图 |
| 删除或清空 | 金字塔、图块、源缓存、GPU 纹理全部失效 |
| 源图尺寸和记录不一致 | 生成任务报错；不显示错误像素 |
| 颜色 | 源图 RGB 转 BGRA；合成和预览同一颜色空间，不额外调色 |

## 13. 测试和验收

### 13.1 单元测试

- 层尺寸：对多种源尺寸和 TOP_MAX 验证 K、L_k、tiles_x、tiles_y；
- 选层：对 5% 到 400% 的显示宽度，验证目标层宽高大于等于显示物理宽高；100% 以上选第 0 层；顶层在最低缩放被选中；
- 迟滞：在层级边界附近反复加减 1%，层不来回抖动；
- 映射：源、层、显示坐标双向一致；裁剪带偏移不影响层键；
- 图块矩形：边缘块、内部块、1 像素边界、顶层单块；
- 接缝：用渐变或棋盘源图生成相邻层图块，验证重叠区域像素一致、显示矩形有重叠、无缝隙；
- 缓存：LRU 顺序、预算、pin、顶层不淘汰、迟到结果拒绝、revision 变化清空；
- 源缓存：并发请求合并、同一 revision 只解码一次、淘汰后重新解码。

### 13.2 无头 UI 测试

- 白块回归一：顶层已经就绪，强制目标层缺块，断言 BlockPlan.layers 含顶层，且 base 的可见区域被覆盖；用现有 debug_bounds 和新的层调试选择器测量；
- 白块回归二：顶层还没就绪、目标层只有部分块，断言行首先画 theme.surface 占位底色，部分目标块不会让缺口露出作品底色；
- 白块回归三：目标层就绪后，顶层仍然保留并参与计划，换排版/缩放时不会因为没有旧显示尺寸的整套图块而露白；
- 排版复用：换横排、竖排、循环、字幕，断言同一图片的层键不变、生成任务数不增加，除新目标层可见块；
- 加图复用：加入新图片，断言旧图片的 ImagePyramid 和图块缓存不重建，新图片顶层优先出现；
- 缩放复用：从 100% 到 200% 再回 100%，断言目标层图块从缓存直接命中，不重复生成；
- 外沿：保留现有两条外沿测试，并增加顶层和目标层外沿覆盖断言；
- 导出：现有 tests/render.rs 全部不变。

### 13.3 验收标准

1. 任意缩放、排版、裁剪、缝隙、圆角、加图或删图下，图片块内部不出现作品底色；目标层缺块时看到的是更粗的层或占位底色，不是白块；
2. 换排版、排序、加图不重建已有图片的像素层；同一张图的源图只解码一次，缓存命中时不再解码；
3. 100% 以上不生成比源图更大的图块；400% 时只画原图层；
4. 顶层常驻，导航、拖动、卡片、预览共用同一份；不再有独立 256 缩略图缓存；
5. 内存不随所有层总和增长；有预算和 LRU；顶层不淘汰；GPU 纹理通过 window.drop_image 释放；
6. 导出和复制结果与改动前完全一致；
7. 5% 到 400% 缩放、横竖排、循环、字幕、裁剪拖动、排序拖动都不出现白块和明显接缝。

## 14. 分阶段落地

### 阶段 1：纯算法层

- 新增 pyramid.rs：层尺寸、选层、迟滞、映射、BlockPlan 纯函数；
- 单元测试；不接 UI、不改现有行为；
- 产出：层几何和选层逻辑可独立验证。

### 阶段 2：顶层和源缓存

- 新增 SourceCache、顶层生成；图片加入后先出顶层；
- 预览 base 用顶层作为完整底图；旧图块路径暂时保留，用于对比；
- 导航、拖动、卡片切到顶层；
- 验收：新图、换排版、缩放不露白；顶层只生成一次。

### 阶段 3：层图块和目标层

- 新增 TileCache，按 revision、层、列、行存；批量生成；LOD 目标层；BlockPlan；
- render_block 改为从粗到细逐层贴图；删除 TileSpec 和 TileGrid 的显示尺寸换代逻辑；
- 验收：换排版不重生成像素；100% 以上不生成放大图块；无白块。

### 阶段 4：调度、缓存预算和预取

- 优先级队列、解码并发限制、UI 每帧上传上限、LRU、预算、pin、预取；
- 验收：大图和多图下内存有界；拖动和缩放不卡；顶层优先。

### 阶段 5：清理和文档

- 删除旧 thumb_cache、TileSpec、旧测试辅助；更新文档和测试选择器；
- 全量测试、导出测试、手工验证 5% 到 400%。

## 15. 风险和取舍

1. 源缓存内存：一张大截图解码后几十 MB。用独立预算、LRU、同时解码数量限制控制；源图被淘汰后从顶层和图块继续显示，缺块再解码。
2. 顶层内存：每张图一张不超过 512 长边的纹理，数量多时约 1 MB 一张；本项目图片数量通常几十张以内，可接受。数量特别多时可按可见性降级，但不建议一开始做。
3. 纹理数量：每层可见约 20 到 60 块，2 到 3 层同时画，draw call 增加；GPUI 逐块 img 下可接受；如果实测卡顿，再考虑图集或自定义绘制。
4. 层级切换 pop：迟滞和预取减少；跨层切换时旧层保留，必要时可做短交叉淡入，但不作为第一版必需。
5. 接缝：边界 1 层像素加现有 1 物理像素补偿；用接缝像素测试锁定。
6. 实现复杂度：比缩略图垫底大，但把缓存键从显示尺寸改成源分辨率，后续缩放、排版、加图都不用再打补丁。
7. 旧测试迁移：TileSpec 和 TileGrid 的测试要改写成层版本；属于一次性成本。

## 16. 关键决策摘要

- 缓存单位：图片加源分辨率层加图块坐标，不含显示尺寸、排版、裁剪。
- 完整底图：顶层，不超过 512 长边，常驻，不是缩略图补丁。
- 显示：从顶层到目标层逐层覆盖，只画已就绪块；顶层保证不露白。
- 目标层：最粗的、像素不少于显示物理像素的层；100% 以上用原图层。
- 生成：每层每块从源图裁剪缩放，1 层像素边界；每张图每批只解码一次源图。
- 内存：顶层加可见层图块加源图预算；LRU；GPU 纹理统一由 UI 线程释放。
- 导出：完全不动。
