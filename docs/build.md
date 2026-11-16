# 构建与打包

## 环境要求

- Rust 1.90 或更新版本（工程使用 edition 2024；系统托盘依赖要求 1.90，实测 Rust 1.98 可以构建）
- 首次构建需要联网下载依赖（字幕检测模型已经放在 `assets/text-detection.onnx`，不需要额外下载）

```bash
cargo build --release
```

## Linux

运行需要：

- 支持 Vulkan 的显卡驱动（`vulkaninfo` 能正常输出）
- Wayland 或 X11 桌面
- 常见运行库：`libxkbcommon`、`libxkbcommon-x11`、`libwayland-client`、`libxcb`

```bash
# Arch
sudo pacman -S vulkan-icd-loader libxkbcommon libxkbcommon-x11 wayland libxcb

# Debian / Ubuntu
sudo apt install libvulkan1 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libxcb1
```

系统托盘走 D-Bus 的 StatusNotifierItem，用纯 Rust 实现，不需要 GTK 或 libappindicator 系统库。

如果启动时提示 `Unable to init GPU context`，说明当前环境没有可用的 Vulkan 设备，程序依赖显卡渲染，无法在没有 GPU 的环境运行。

### 托盘图标和 AppIndicator 扩展

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

### 桌面图标

仓库里有一份桌面文件 `packaging/linux/gistpic-creator.desktop`，图标用 `assets/icon-256.png`。

窗口和任务栏的图标由桌面环境按 `app_id`（`com.gistpic.creator`）去匹配 `.desktop` 文件里的 `Icon` 字段，程序自己设不了，所以必须把桌面文件和图标装到系统里。

其他发行版用便携压缩包时，把三个文件手动装到用户目录：

```bash
mkdir -p ~/.local/bin ~/.local/share/icons/hicolor/256x256/apps ~/.local/share/applications
install -m755 gistpic-creator ~/.local/bin/
install -m644 gistpic-creator.png ~/.local/share/icons/hicolor/256x256/apps/
install -m644 gistpic-creator.desktop ~/.local/share/applications/
```

托盘图标不用这一步，程序启动后直接显示新图标。

### Arch Linux 安装包

用项目里的部署脚本打成本地包，一步装好：

```bash
./scripts/build-arch.sh
sudo pacman -U dist/archlinux/gistpic-creator-bin-*.pkg.tar.zst
```

脚本先跑 `cargo build --release`，把可执行文件、桌面文件、图标和文档放到 `dist/archlinux/`，按 `Cargo.toml` 里的版本号填好 `PKGBUILD`，再调用 `makepkg` 打成 `.pkg.tar.zst`。

包名是 `gistpic-creator-bin`：装的是预编译好的二进制，按 Arch 的命名约定带 `-bin` 后缀。装完后桌面菜单里能搜到，窗口和任务栏也会用上图标。

`makepkg` 只有 Arch 上有。在别的发行版上跑这个脚本，只会生成 `PKGBUILD`，不产出安装包。

## macOS

需要 Xcode 命令行工具（GPUI 用 Metal 渲染）：

```bash
xcode-select --install
./scripts/build-macos.sh
```

产物是 `dist/省流图创造机.app`。没有开发者证书签名的 app，第一次打开需要右键 →「打开」，或者在「系统设置 → 隐私与安全性」里允许。

## Windows

需要 MSVC 工具链：安装 Visual Studio 生成工具，勾选「使用 C++ 的桌面开发」，然后：

```powershell
./scripts/build-windows.ps1
```

产物是 `dist\gistpic-creator-windows-x64.zip`，解压后双击 `gistpic-creator.exe`。

exe 文件本身的图标由 `build.rs` 在编译时从 `assets/icon.ico` 写进去，资源管理器、任务栏和 Alt+Tab 显示的都是它。这一步只在目标平台是 Windows 时执行，其他平台编译时跳过。

## 打便携包

| 平台 | 命令 | 产物 |
| --- | --- | --- |
| Linux（便携包） | `./scripts/build-linux.sh` | `dist/gistpic-creator-linux-x86_64.tar.gz` |
| Arch Linux（本地安装包） | `./scripts/build-arch.sh` | `dist/archlinux/gistpic-creator-bin-<版本>-x86_64.pkg.tar.zst`，同目录还有 `PKGBUILD` |
| Windows | `./scripts/build-windows.ps1` | `dist\gistpic-creator-windows-x64.zip` |
| macOS | `./scripts/build-macos.sh` | `dist/省流图创造机.app`、`dist/gistpic-creator-macos.zip` |

## 自动构建

仓库带 GitHub Actions 配置（`.github/workflows/ci.yml`）：推送到 `main` 分支或打 `v*` 标签时自动跑测试，并在 Linux、Windows、macOS 三个平台各构建一份制品，另外在 Arch 容器里打一份安装包。所有制品都能在 Actions 页面下载。

推送分支只在 Actions 页面留制品，不发版。要让用户能直接下载，得打一个 `v*` 标签：

```bash
git tag v1.0.3
git push origin v1.0.3
```

打完标签，四个平台构建全部通过后会建一个同名的 GitHub Release，把便携包和 Arch 安装包一起挂上去。同一个标签重复推时，旧 Release 会先删掉再重建。Arch 的 `-debug-` 空壳包不会被上传。

## 不打开界面检查输出

可以用两个示例脚本直接跑一次拼接或排版，结果写到 `dist` 目录：

```bash
cargo run --release --example stitch_samples   # 字幕模式
cargo run --release --example layout_samples   # 两横一竖排版
```

这两个脚本要读 `示例图片/` 里的素材，素材不进仓库，得自己放。

还有一个查显卡的脚本：

```bash
cargo run --release --example gpu_probe
```
