# 省流图创造机（GistPic Creator）

把视频画面里的字幕裁掉、从上到下拼成一张竖长图；也可以把若干图片自动排成 16:9、9:16 等比例的长图。

界面用 [GPUI](https://gpui.rs)（Zed 编辑器的界面框架）实现，支持 Linux / macOS / Windows。

## 功能

- **字幕模式**
  - 粘贴、拖入或打开多张截图，自动在整张画面里找字幕，并优先考虑画面下半
  - 找得不准时，可以直接在成品图上拖动上下两条裁剪线，手动框选要保留的部分
  - 手动框选可以只改一张，也可以应用到选中的几张或全部图片；不同分辨率的图片按画面高度的百分比对齐
  - 没有手动框选时，第一张保留画面主体：字幕在画面下方时保留上半部分，字幕在上方时保留下半部分
  - 没有手动框选的其余图片只保留字幕那一条，上下各留一点出血位（默认是字幕文字高度的 25%，可在底部胶囊的留白滑条按 5% 一档调整）
  - 多行字幕只保留第一行，可以在右键卡片里切换“保留双行字幕”
  - 自动区分弹幕和字幕：弹幕字号更小、位置随机，不会被当成字幕
  - 按顺序从上到下拼成一张长图
- **排版模式**
  - 不裁字幕，图片不会被裁剪或拉伸
  - 按图片宽高自动分组（比如两张横图排一列、两张竖图排一列），不要求按粘贴顺序
  - 画布紧贴排好的图片，不补边；选 16:9 / 9:16 / 4:3 / 1:1 时，目标比例只用来决定怎么分组、怎么排最接近
  - 想严格按粘贴顺序排，用横向排列（统一高度）或竖向排列（统一宽度）
- **通用**
  - 两种模式共用同一份图片列表
  - 整个窗口就是一张画板：没有顶栏、没有右侧抽屉、没有固定状态栏，也没有双击弹出的放大窗口
  - 鼠标停在图片上或按住上下边缘拖动时，会就地浮出这张图被裁掉的上下区域，拖动裁剪线可以自由裁剪，靠近单行字幕、双行字幕、整张画面时自动吸附
  - 拖动两张图之间的接缝，实时调整全局图片间距（0 到 40 像素）；间距大于 0 时单击接缝，在暖白、炭黑、暖灰三种缝隙颜色之间循环
  - 在画布空白处按住拖出选框，可以一次圈选多张图片；多选后拖动任意一张的裁剪线，所有选中的图片按同一比例同步调整
  - 按住图片中间拖动可以调整顺序；拖动时底部胶囊变成珊瑚红色的删除区，拖进去松手即删除
  - 底部只有一条悬浮胶囊：模式切换、留白滑条或排版比例、添加、撤销、复制、导出都放在这里
  - 在图片或画布上点右键，会弹出温润圆角的小卡片，承载单图兜底操作和全局设置
  - 撤销 / 重做、清空、删除当前选中的图片
  - `Ctrl/Cmd + 滚轮`缩放画布，`Ctrl/Cmd + + / - / 0`也能调；缩放不是 100% 时画布右下角出现比例胶囊，点击回到 100%
  - 导出 PNG 文件，或直接复制图片到剪贴板
  - 跟随系统浅色 / 深色外观
  - 系统托盘图标：Windows / macOS / Linux 都支持，点图标或菜单把窗口叫回来
  - 关闭按钮行为可选：默认隐藏窗口到托盘让程序继续运行，也可以改成直接退出
  - 界面设置、窗口大小位置下次启动自动恢复

## 语言

桌面界面支持简体中文和英文，默认简体中文。在画布空白处或底部胶囊上点右键，卡片里可以随时切换语言；窗口标题、托盘菜单、胶囊提示和导出文件名会一起切换。

## 使用

1. 启动程序：Linux / macOS 用 `cargo run --release`，Windows 双击 `gistpic-creator.exe`
2. 把图片放进列表：截图后按 `Ctrl+V`（macOS 是 `Cmd+V`），或把图片文件拖进窗口，或点空状态的大卡片、在画布上点右键选“打开图片”（`Ctrl/Cmd + O`）
3. 画布上直接看拼好的成品。鼠标停在某张图上，会浮出这张图被裁掉的上下部分；按住上边线或下边线拖动，靠近单行字幕、双行字幕、整张画面时会自动吸附，也可以停在任意位置
4. 改多张：在画布空白处按住拖出选框，或按住 `Ctrl`（macOS 是 `Cmd`）点击多选；再拖动任意一张的裁剪线，选中的图片会同步变化
5. 调间距和颜色：按住两张图之间的接缝上下拖动，间距在 0 到 40 像素之间变化；间距大于 0 时单击接缝，循环切换纯白、炭黑、暖灰三种缝隙颜色
6. 调整顺序或删除：按住图片中间拖动，拖到别的图上调整顺序；拖动时底部胶囊会变成珊瑚红色的“松手删除”区，拖进去松手即删除
7. 底部胶囊里切换字幕模式 / 排版模式；字幕模式调留白滑条，排版模式选 16:9、9:16、4:3、1:1、横排、竖排，也可以拖画布右下角的圆角手柄
8. 点胶囊里的“复制”把长图放进剪贴板，或点“导出”保存文件。正在识别、复制成功、出错等提示会在胶囊上方轻轻浮现，过一会儿自动消失

### 界面手势一览

| 做什么 | 怎么做 |
| --- | --- |
| 选中一张 | 左键单击图片 |
| 多选 / 取消多选 | `Ctrl/Cmd` + 左键单击，或在画布空白处拉框 |
| 选中一段 | `Shift` + 左键单击 |
| 调整顺序 | 按住图片中间拖到另一张图上，松手插到那张前面 |
| 原地调裁剪 | 鼠标停在图片上，按住上边线或下边线拖动；靠近档位自动吸附 |
| 同步裁剪 | 先多选，再拖任意一张的裁剪线 |
| 调图片间距 | 按住两张图之间的接缝上下拖动 |
| 换缝隙颜色 | 间距大于 0 时，单击接缝 |
| 删除图片 | 按住图片拖到底部胶囊的删除区，松手；或按 `Delete` |
| 缩放画布 | `Ctrl/Cmd` + 滚轮，或 `Ctrl/Cmd + + / - / 0` |
| 回到 100% | 点击画布右下角的比例胶囊 |
| 换排版比例 | 点胶囊里的比例片，或拖画布右下角的圆角手柄 |
| 图片右键卡片 | 在图片上点右键：保留单行 / 双行 / 整张画面、同步裁剪、恢复自动结果、上移 / 下移 / 删除 |
| 全局右键卡片 | 在画布空白处或胶囊上点右键：打开图片、粘贴、清空、中英文切换、关闭到托盘、退出 |

没找到字幕的图片默认输出整张画面，不再弹窗询问；识别失败的图片也会保留整张画面。右键卡片里的“整张画面”开关反映当前输出范围，手动框选只改这张图的输出范围，不会重新跑字幕识别。

### 微信小程序版

仓库里还有一份微信小程序实现，放在 `miniprogram/`，和桌面版共用同一套检测模型，但代码是独立的小程序 JavaScript，不依赖 Rust。

- 支持字幕模式和排版模式，图片列表可以触控拖动排序，字幕上下边界可以直接用手指拖。
- 自动识别用微信原生推理接口在手机端跑 `assets/text-detection.onnx`，需要真机验证；开发者工具不支持这个接口。
- 没有配置模型地址或识别失败时，自动退回手动框选，不影响导出。
- 小程序不能读取剪贴板图片，输入方式只有相册、拍照和聊天文件；一次最多 20 张图；导出最长边限制 4096 像素。

完整的导入、模型配置、测试和发布步骤见 [miniprogram/README.md](miniprogram/README.md)。

### 系统托盘与关闭按钮

程序运行时会在系统托盘显示一个图标（Linux 需要桌面有托盘宿主，见下文），图标上画的是播放键和两条字幕横条。

- **Windows / macOS**：左键点图标直接把窗口叫到前面；右键弹出菜单。
- **Linux**：图标由桌面显示，左键行为由桌面决定（GNOME 上通常是弹出菜单）；菜单里始终可以点“打开主窗口”。
- 菜单内容只有两项：“打开主窗口”和“退出”。
- 退出：按 `Ctrl/Cmd + Q`，或点窗口自带的关闭按钮（关掉“隐藏到托盘”时），或托盘菜单里的“退出”。
- 窗口隐藏后的恢复：
  - Windows / macOS / Linux：点托盘图标或菜单里的“打开主窗口”会重新显示窗口。
  - Linux Wayland 下窗口会重新显示，但桌面可能拒绝程序主动把窗口提到最前面；需要时用桌面自带的任务栏或窗口列表。
- 在画布空白处或底部胶囊上点右键，卡片里有开关“关闭窗口时隐藏到托盘”：
  - 开着（默认）：点窗口关闭按钮会隐藏窗口，程序继续在托盘运行。
  - 关掉：点窗口关闭按钮直接退出程序。
  - 这个开关会记住，下次启动保持上次的选择。

Linux 上托盘图标依赖桌面提供 StatusNotifierItem 宿主。GNOME 默认没有，需要安装并启用 AppIndicator 扩展：

```bash
# 先找扩展名字（不同发行版名字可能不同）
gnome-extensions list | grep -i appindicator
# 用上面找到的名字启用
gnome-extensions enable <扩展名字>
# 确认宿主服务存在，返回 (true,) 就表示有
gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
  --method org.freedesktop.DBus.NameHasOwner org.kde.StatusNotifierWatcher
```

如果宿主不存在，程序会在胶囊上方的提示气泡里说明原因；托盘图标不会显示，程序仍在运行，可以用桌面的窗口列表（GNOME 按 Super 键打开活动概览）打开窗口。

启用扩展后，桌面会启动托盘宿主，本程序会自动重新注册图标；如果当前桌面要重新登录才会加载扩展，重新登录后再启动本程序。

### 快捷键

| 操作 | 快捷键 |
| --- | --- |
| 粘贴图片 | `Ctrl/Cmd + V` |
| 打开图片 | `Ctrl/Cmd + O` |
| 导出 | `Ctrl/Cmd + E` |
| 复制图片到剪贴板 | `Ctrl/Cmd + Shift + C` |
| 撤销 | `Ctrl/Cmd + Z` |
| 重做 | `Ctrl/Cmd + Shift + Z` |
| 删除选中的图片 | `Delete` / `Backspace` |
| 缩放画布 | `Ctrl/Cmd + +` / `Ctrl/Cmd + -` / `Ctrl/Cmd + 0` |
| 缩放画布（鼠标） | `Ctrl/Cmd + 滚轮` |
| 关掉右键卡片 | `Esc` |
| 退出程序 | `Ctrl/Cmd + Q` |

## 构建

环境要求：

- Rust 1.90 或更新版本（工程使用 edition 2024；系统托盘依赖要求 1.90，实测 Rust 1.98 可以构建）
- 首次构建需要联网下载依赖（字幕检测模型已经放在 `assets/text-detection.onnx`，不需要额外下载）

```bash
cargo build --release
```

### Linux

运行需要：

- 支持 Vulkan 的显卡驱动（`vulkaninfo` 能正常输出）
- Wayland 或 X11 桌面
- 常见运行库：`libxkbcommon`、`libwayland-client`、`libxcb`

系统托盘走 D-Bus 的 StatusNotifierItem，用纯 Rust 实现，不需要 GTK 或 libappindicator 系统库。GNOME 需要额外启用 AppIndicator 扩展，见上文“系统托盘与关闭按钮”。

Arch：

```bash
sudo pacman -S vulkan-icd-loader libxkbcommon wayland libxcb
```

Debian / Ubuntu：

```bash
sudo apt install libvulkan1 libxkbcommon0 libwayland-client0 libxcb1
```

如果启动时提示 `Unable to init GPU context`，说明当前环境没有可用的 Vulkan 设备，程序依赖显卡渲染，无法在没有 GPU 的环境运行。

### macOS

需要 Xcode 命令行工具（GPUI 用 Metal 渲染）：

```bash
xcode-select --install
./scripts/build-macos.sh
```

产物是 `dist/省流图创造机.app`。没有开发者证书签名的 app，第一次打开需要右键 →“打开”，或者在“系统设置 → 隐私与安全性”里允许。

### Windows

需要 MSVC 工具链：安装 Visual Studio 生成工具，勾选“使用 C++ 的桌面开发”，然后：

```powershell
./scripts/build-windows.ps1
```

产物是 `dist\gistpic-creator-windows-x64.zip`，解压后双击 `gistpic-creator.exe`。

### 打便携包

| 平台 | 命令 | 产物 |
| --- | --- | --- |
| Linux | `./scripts/build-linux.sh` | `dist/gistpic-creator-linux-x86_64.tar.gz` |
| Windows | `./scripts/build-windows.ps1` | `dist\gistpic-creator-windows-x64.zip` |
| macOS | `./scripts/build-macos.sh` | `dist/省流图创造机.app`、`dist/gistpic-creator-macos.zip` |

## 不打开界面检查输出

可以用两个示例脚本直接跑一次拼接或排版，结果写到 `dist` 目录：

```bash
cargo run --release --example stitch_samples   # 字幕模式
cargo run --release --example layout_samples   # 两横一竖排版
```

## 开发日志和启动测量

- debug 构建自动包含启动阶段日志、开发过程日志和 `--startup-bench` 测量参数。
- release 构建默认不包含这些内容，只保留用户需要看到的警告和错误；日志默认级别为 `warn`。
- 需要在 release 构建上复测启动时，临时加 `dev-tools` 功能：

```bash
cargo build --release --features dev-tools
./target/release/gistpic-creator --startup-bench
```

测量模式会等到第一帧画完再退出，不启动托盘。这里的“启动用时”是从进程开始运行算到第一帧
提交，不是只算进程启动。日志里的 `GPUI 平台初始化` 包含 Blade 的 Vulkan 图形设备初始化，
通常占启动时间的大头；字体配置和字幕模型加载通常只占几毫秒。想单独确认显卡初始化耗时，可以跑：

```bash
cargo run --release --example gpu_probe
```

## 测试

```bash
cargo test
```

小程序版的纯逻辑测试用 Node，不需要微信开发者工具：

```bash
node --test tests/miniprogram/
```

覆盖内容：

- 字幕分类：从弹幕、水印、中部误检里挑出真正字幕；多行字幕分组；整张图候选和下半优先打分
- 裁剪规则：出血位、第一张保留画面、手动框选覆盖自动结果、字幕在上方 / 下方的处理
- 排版算法：两横一竖自动分列、两横两竖自动分组、四张横图排成 16:9、横排 / 竖排、画布紧贴内容、不裁剪不拉伸
- 合成与导出：统一宽度、缝隙颜色、PNG 编解码、预览图尺寸
- 真实截图：`示例图片/示例_*` 目录里的 9 张图逐张校验检测结果
- 端到端：真实截图 → 检测 → 裁剪 → 合成
- 托盘图标：生成的 RGBA 数据尺寸正确、角落透明、模板图单色、彩色图有蓝色底色和白色图形
- 无头界面测试：用 GPUI 的测试平台创建窗口、粘贴真实截图、等检测完成、拖动框选横线、应用到多张图片、撤销重做、没找到字幕时默认保留整张画面、切换排版比例、复制图片、真正导出 PNG 文件（不需要显卡）

Linux 上复制图片后，可以用系统工具确认剪贴板内容：

```bash
# Wayland（需要 wl-clipboard）
wl-paste --type image/png > /tmp/gistpic-clipboard.png
file /tmp/gistpic-clipboard.png

# X11（需要 xclip）
xclip -selection clipboard -t image/png -o > /tmp/gistpic-clipboard.png
file /tmp/gistpic-clipboard.png
```

## 已知限制

- 只处理静态图片，不处理视频
- 只判断“文字在哪”，不识别文字内容
- 图片很多、分辨率很高时，导出会占用较多内存（输出图是整张解码后的位图）
- 画面里的其他大号文字（大字标题、字幕组水印等）可能被误判成字幕；自动结果会优先看下半画面，但不保证每次都对，可在图上拖动上下裁剪线手动框选
- 排版选固定比例时，因为图片尺寸各不相同且不裁剪，输出比例只能尽量接近目标比例，不会强行补边或裁剪
- 剪贴板图片使用常见的 `image/png` 等 MIME 类型；截图工具或浏览器按这些格式放进剪贴板即可，X11 和 Wayland 都支持读取和写入
- Linux 的保存对话框依赖 xdg-desktop-portal，常见桌面环境默认就有
- Linux 的托盘图标依赖桌面提供 StatusNotifierItem 宿主；GNOME 需要启用 AppIndicator 扩展，宿主不存在时图标不显示（程序会在胶囊上方提示）
- Linux Wayland 下，桌面可能拒绝程序主动激活窗口；托盘里的“打开主窗口”会把隐藏的窗口重新显示出来，但不保证自动提到最前面，可用桌面任务栏或窗口列表切换
- 没有做单实例限制：同时开两个程序会出现两个托盘图标
- 界面设置保存在用户配置目录下的 `gistpic-creator/state.json`。文件内容有问题时程序会用默认值启动并保留原文件，不会覆盖
- 字幕检测模型加载通常只有几毫秒；启动慢时优先看显卡初始化，而不是模型加载

## 目录结构

```
src/
  main.rs            程序入口，创建窗口、注册快捷键
  lib.rs
  model.rs           项目、图片、设置、排版目标等数据结构
  platform.rs        平台相关常量与工具
  i18n.rs            中英文界面文案
  imaging/
    decode.rs        图片解码
    detect.rs        字幕检测（内嵌 ONNX 检测模型）
    classify.rs      合并重复行、分组、打分，挑出字幕
    crop.rs          裁剪带推导（出血位、第一张保留画面、手动框选）
    layout.rs        16:9 / 9:16 等排版算法
    render.rs        裁剪、预览图、拼接、排版合成、PNG 编码
  app.rs             主状态与业务逻辑（导入、检测调度、撤销、导出触发）
  config.rs          记住界面设置和窗口位置
  app/
    actions.rs       快捷键动作定义
    keymap.rs        快捷键绑定
    undo.rs          撤销 / 重做栈
    export.rs        导出 PNG、复制到剪贴板
  tray/
    mod.rs           托盘图标、菜单、事件处理
    icon.rs          程序内生成的托盘图标
    linux.rs         Linux 托盘宿主检测
    macos.rs         macOS 从最小化恢复窗口
  ui/
    theme.rs         配色与阴影
    tokens.rs        尺寸、字号、圆角、间距常量
    widgets.rs       菜单行、开关、拖动时跟随鼠标的小标签
    icon.rs          用路径画的界面图标
    canvas.rs        全屏画布：长图、排版、选框、接缝
    canvas/
      geometry.rs    画布布局、图块与接缝的几何计算
    ghost_crop.rs    原地幽灵透视与磁吸档位
    capsule.rs       底部悬浮胶囊与状态气泡
    context_menu.rs  图片 / 画布右键卡片
vendor/
  gpui/               本地 GPUI 补丁，只增加窗口显示/隐藏
assets/
  text-detection.onnx  内嵌的字幕文字检测模型
miniprogram/         微信小程序版（字幕模式、排版模式、触控拖动、端侧自动识别）
tests/               各模块单元测试与真实截图测试
  miniprogram/       小程序纯逻辑 Node 测试
scripts/             三个平台的打包脚本
```

## 许可

本项目代码使用 MIT 许可证。内嵌的字幕检测模型来自 `robertknight/ocrs-models`，使用 CC-BY-SA-4.0 许可，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
