# 本地 GPUI 补丁

这个目录是 `gpui 0.2.2` 的源码副本，增加窗口运行时显示/隐藏能力和 Linux 图片
剪贴板写入。根 `Cargo.toml` 通过 `[patch.crates-io]` 使用本地版本。

## 改动

- `src/platform.rs`
  - `PlatformWindow` 增加默认空实现的 `set_visible(&self, visible: bool)`。
- `src/window.rs`
  - 增加公开的 `Window::set_visible(visible: bool)`，不主动改变焦点。
  - `FileDropEvent::Entered` 用新路径覆盖残留的 `active_drag`：上一次拖放漏掉
    `Exited` 时，旧路径会把新一批文件顶掉，松手后一个都拿不到。应用内部正在拖图
    （`active_drag` 不是 `ExternalPaths`）时不覆盖，避免松手丢图。
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
- `src/platform/linux/wayland/client.rs`（文件拖放）
  - `Enter` 时同步登记当次拖动（窗口、数据 offer、位置、代次），文件列表仍在后台
    异步读取。`Drop` 早于文件列表就绪时只记 `dropped_early`，等读取完成的任务
    先派发 `Entered` 再补发 `Submit`，修掉「Drop 早于 Entered」的丢文件竞态。
  - 代次用于让 `Leave`/新的 `Enter` 之后迟到的读取结果直接退出。
- `src/platform/linux/x11/client.rs`（文件拖放）
  - `XdndDrop` 时如果 `XDND_DATA`（文件列表）还没取回，记录 `drop_pending`；
    `SelectionNotify` 到达后先派发 `Entered` 再补发 `Submit`。
  - 修掉「Submit 早于 SelectionNotify」导致拖入文件丢失。
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
  - `start_window_move` 改为空实现：无头测试没有真实窗口可移动，原先的
    `unimplemented!()` 会让“空白处左键拖动窗口”这类测试直接 panic。
    是否真的请求了移动，由应用层自己的测试计数断言。
- `src/platform/windows/util.rs`（光标映射）
  - `load_cursor` 增加 `OpenHand` / `ClosedHand` 两个分支，都映射到系统自带的
    四向箭头 `IDC_SIZEALL`。Windows 没有手形光标，也不像 Linux 那样能按名字去
    光标主题里加载 `openhand` / `closedhand`，改之前这两种样式会落到默认箭头，
    界面上看不出“这张图可以拖动”。
- `src/window.rs`、`src/app/test_context.rs`（无头测试读光标）
  - 增加 `Window::cursor_style()` 和 `VisualTestContext::cursor_style()`，返回当前
    鼠标位置下生效的光标样式。
  - 为什么需要：测试里 `TestPlatform::set_cursor_style` 写进 `active_cursor` 的
    那条路径，被 `Window::reset_cursor_style` 里的 `is_window_hovered()` 挡住，
    而 `TestWindow::is_hovered()` 恒返回 false，于是无头测试永远读不到光标。新接口
    直接按 `Frame::cursor_style(self)` 算一遍（和 `reset_cursor_style` 用的是同一
    份数据），不需要窗口真的被鼠标悬停，也不需要测试窗口假装悬停。
  - 生产行为不变：只是把 `reset_cursor_style` 里原来内联的那段读取提成公开只读
    方法，推送给平台的那一步照旧只在窗口被悬停时执行。
- `src/app.rs`
  - `App::shutdown` 在没有注册退出回调（`on_app_quit`）时跳过等待。
    带 `test-support` 功能编译的真实程序会使用测试版的 `block_internal`，
    它在非测试调度器上直接返回 `Err`，导致每次正常退出都误报
    `timed out waiting on app_will_quit`。没有回调时本来就没有东西要等。
- `src/taffy.rs`
  - 给两个浮点字面量补 `f32` 后缀，消除当前 Rust 的
    `float_literal_f32_fallback` 警告。

- `src/platform/blade/blade_atlas.rs`（图集回收）
  - 新增待释放队列 `pending_frees` 和上一帧的 `SyncPoint`：`PlatformAtlas::remove`
    不再当场销毁整张图集，先把图块记账，等 GPU 画完这一帧再还。
  - 新增 `reclaim_freed_tiles`：在 `before_frame` 里用 `wait_for(..., 0)` 查一次
    上一帧画完没有（不阻塞界面），画完了才把图块还给 `etagere` 分配器，并把引用
    计数归零的图集销毁、索引放进 `free_list` 复用。
  - 为什么改：原来的 `remove` 直接销毁整张 1024×1024 图集，从不调用
    `etagere::BucketedAtlasAllocator::deallocate`，而 `allocate` 只复用现有图集。
    一张图集里只要还有一个图块在用，它整张就不能销毁；于是每次图块换批都要新建
    一张图集。截图里 46 秒内建了 57 张 1024×1024 的 `Bgra8Unorm` 图集，显存涨到
    138.9 MB，最终 `ERROR_DEVICE_LOST`。
  - `flush_initializations` 和 `flush` 在取图集时容忍空槽位：同一帧里刚建好又被
    回收的图集不能再拿去 `init_texture` / 拷贝数据。
- `src/platform/blade/blade_renderer.rs`（等 GPU 有上限、清理时不碰已死的 GPU）
  - `wait_for_gpu` 改成返回是否等到；超时后最多再等 `GPU_WAIT_RETRIES`（1）轮。
    原来的 `while !self.gpu.wait_for(...) {}` 没有任何上限，一个死循环。
  - `destroy` 开头先看 `std::thread::panicking()`：线程已经在展开（别处 panic 了，
    正在清理）就直接返回，不碰 GPU；没在展开才等 GPU，等不到也直接返回。
  - `update_drawable_size_impl` 等不到 GPU 就保持原尺寸返回。
  - 为什么改：这张截图里 GPU 已经设备丢失，`wait_for` 永远等不到结果，进程卡在
    窗口析构里，用户只能强制结束程序。更致命的是第二次 panic：第一次 panic 发生在
    `submit` 持有队列锁的时候，锁被毒化，之后任何 `gpu.wait_for` 都在取锁那一行
    panic；在析构里再 panic 会被编译器转成 `panic_in_cleanup`，进程直接 abort。
    文件末尾那三行 `panic in a destructor during cleanup` /
    `thread caused non-unwinding panic. aborting.` 就是这么来的。

## 参考

- `etagere` 的回收语义：`deallocate` 之后空间要等同组图块全部释放才回收，所以还图块比销毁图集更省
- gpui-ce 的窗口可见性实现：
  <https://github.com/gpui-ce/gpui-ce/pull/239>
- 相关限制讨论：
  <https://github.com/gpui-ce/gpui-ce/issues/115>

## 边界

- 窗口显示/隐藏不处理 Wayland 桌面拒绝程序主动激活窗口的问题。
- Linux 图片剪贴板只写应用实际提供的图片格式，不做图片转码；本程序复制时统一提供 PNG。
- 如果后续升级 `gpui`，优先确认上游是否已经提供等价的 `set_visible`、Linux 图片剪贴板
  和图集回收，再删除这份本地副本。
- 图集回收只处理本程序真正用到的路径：图块被 `remove` 之后延迟一帧再还。没有做
  「图集碎片整理」，也没改 `allocate` 的挑选顺序，图集数量在碎片多时仍会缓慢增长，
  只是不再每换一批图块就新建一张。
- 等 GPU 只在窗口关闭、改尺寸和每帧末尾这三处生效，超时判定沿用 blade 自己的
  `MAX_FRAME_TIME_MS`（10 秒）。显卡真的没响应时程序会放弃等待并继续走关闭流程，
  不再无限卡住，但也不会尝试恢复显卡。
