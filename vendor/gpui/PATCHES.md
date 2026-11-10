# 本地 GPUI 补丁

这个目录是 `gpui 0.2.2` 的源码副本，增加窗口运行时显示/隐藏能力和 Linux 图片
剪贴板写入。根 `Cargo.toml` 通过 `[patch.crates-io]` 使用本地版本。

## 改动

- `src/platform.rs`
  - `PlatformWindow` 增加默认空实现的 `set_visible(&self, visible: bool)`。
- `src/window.rs`
  - 增加公开的 `Window::set_visible(visible: bool)`，不主动改变焦点。
- `src/platform/linux/wayland/window.rs`
  - 窗口状态记录 `visible`、`title`、`window_min_size`、`parent`。
  - 隐藏时先提交空缓冲区（`attach(None, 0, 0)` + `commit`）取消映射，再销毁
    `xdg_toplevel`、装饰对象和 `xdg_surface`；保留 `wl_surface` 和 Blade 渲染器。
  - 显示时在同一个 `wl_surface` 上重新创建 `xdg_surface`、`xdg_toplevel` 和装饰，
    设置标题/应用 ID/最小尺寸/父窗口，提交空缓冲区等待新的
    `xdg_surface.configure`；收到 configure 后再绘制新帧。
  - 只销毁 `xdg_toplevel`、保留旧 `xdg_surface` 时，合成器不会重新发初始 configure；
    因此隐藏必须把 `xdg_surface` 一起销毁。
  - 隐藏期间跳过 `frame`、`draw`、`completed_frame`，避免向隐藏表面提交缓冲区。
- `src/platform/linux/wayland/clipboard.rs`、`src/platform/linux/wayland/client.rs`
  - 写入剪贴板时按 `ClipboardItem` 的实际条目声明文本或图片 MIME 类型，图片使用
    `ImageFormat::mime_type()`（例如 `image/png`）。
  - 其他程序请求 MIME 时，`send` / `send_primary` 返回对应条目的字节；图片不再被丢掉。
  - 写剪贴板优先使用键盘事件序列号，没有键盘序列号时回退到鼠标按下序列号，
    这样点工具栏按钮复制也能被 Wayland 合成器接受。
- `src/platform/linux/x11/window.rs`
  - 记录 `visible`，用 X11 的 `unmap_window` / `map_window` 隐藏和显示。
- `src/platform/linux/x11/clipboard.rs`、`src/platform/linux/x11/client.rs`
  - `set_image` 使用图片自己的格式原子，不再把 JPEG/WebP 等字节错误声明成 PNG。
  - 写入剪贴板和主选区时先找图片条目，有图片就写图片，没有图片再写文字。
  - 大于 X11 最大请求长度的数据改用 INCR 分块传输，避免大 PNG 被服务器拒绝；
    小文本仍走原来的单次属性写入。
- `src/platform/windows/window.rs`
  - 用 `ShowWindowAsync` 的 `SW_HIDE` / `SW_SHOWNOACTIVATE` 隐藏和显示。
- `src/platform/mac/window.rs`
  - 用 `orderOut:` / `orderFront:` 隐藏和显示。
- `src/platform/test/window.rs`、`src/app/test_context.rs`
  - 测试平台记录可见状态，并提供 `TestAppContext::is_window_visible`。
- `src/taffy.rs`
  - 给两个浮点字面量补 `f32` 后缀，消除当前 Rust 的
    `float_literal_f32_fallback` 警告。

## 参考

- gpui-ce 的窗口可见性实现：
  <https://github.com/gpui-ce/gpui-ce/pull/239>
- 相关限制讨论：
  <https://github.com/gpui-ce/gpui-ce/issues/115>

## 边界

- 窗口显示/隐藏不处理 Wayland 桌面拒绝程序主动激活窗口的问题。
- Linux 图片剪贴板只写应用实际提供的图片格式，不做图片转码；本程序复制时统一提供 PNG。
- 如果后续升级 `gpui`，优先确认上游是否已经提供等价的 `set_visible` 和 Linux 图片剪贴板，
  再删除这份本地副本。
