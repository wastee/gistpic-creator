# 第三方组件与许可

本程序包含或依赖以下第三方内容。分发本程序（包括二进制）时请一并保留本文件。

## 字幕文字检测模型

- 文件：`assets/text-detection.onnx`（编译时内嵌进可执行文件）
- 来源：`robertknight/ocrs-models` 仓库中的 `text-detection.onnx`
  - https://huggingface.co/robertknight/ocrs-models
  - https://github.com/robertknight/ocrs-models
- 用途：只做文字区域检测（找出文字在图片里的位置），不做文字识别
- 许可证：**CC-BY-SA-4.0**（Creative Commons 署名-相同方式共享 4.0 国际）
  - 许可证全文：https://creativecommons.org/licenses/by-sa/4.0/legalcode
  - 中文说明：https://creativecommons.org/licenses/by-sa/4.0/deed.zh
- 该模型训练时使用了 HierText 数据集（Google），按其原始许可分发
- 署明：模型由 Robert Knight 为 ocrs 项目整理与转换

因为模型被内嵌进可执行文件，分发本程序的二进制属于分发该模型的副本，需要遵守 CC-BY-SA-4.0：保留署名、注明许可、并以相同许可分发模型部分（本程序的源代码使用 GPL-3.0 许可，见 LICENSE）。

## Rust 依赖

主要依赖及其许可证：

| 依赖 | 许可证 | 用途 |
| --- | --- | --- |
| gpui（`vendor/gpui` 本地补丁） | Apache-2.0 | 界面框架与 GPU 渲染；补丁增加窗口显示/隐藏和 Linux 图片剪贴板写入 |
| ocrs | MIT OR Apache-2.0 | 文字检测推理封装 |
| rten | MIT OR Apache-2.0 | ONNX 模型推理 |
| image | MIT OR Apache-2.0 | 图片解码、缩放、编码 |
| chrono | MIT OR Apache-2.0 | 导出文件名的时间戳 |
| dirs | MIT OR Apache-2.0 | 默认保存目录 |
| option-ext | MPL-2.0 | 默认保存目录（经 `dirs` → `dirs-sys` 引入，见下节） |
| thiserror | MIT OR Apache-2.0 | 错误类型 |
| log / env_logger | MIT OR Apache-2.0 | 日志 |
| tray-icon | MIT OR Apache-2.0 | 系统托盘图标与菜单 |
| muda | MIT OR Apache-2.0 | 托盘菜单 |
| ksni | Unlicense | Linux 托盘（StatusNotifierItem） |
| zbus | MIT | Linux 托盘宿主检测 |
| objc2 | MIT | macOS 调用 AppKit |
| objc2-app-kit | Zlib OR Apache-2.0 OR MIT | macOS 恢复最小化窗口 |
| candle | MIT OR Apache-2.0 | 开发训练工具训练文字块角色分类器；不随桌面应用发布 |

## MPL-2.0 组件

其余依赖都是 MIT、Apache-2.0、BSD、Zlib、Unicode-3.0、CC0、0BSD、Unlicense 这类宽松许可，
只有 `option-ext` 使用 MPL-2.0，它的义务和上面那些不一样，单独说明。

- 包名：`option-ext` 0.2.0
- 许可证：Mozilla Public License 2.0，全文见 https://mozilla.org/MPL/2.0/
- 源码：https://github.com/soc/option-ext
- 引入路径：`dirs` → `dirs-sys` → `option-ext`，用来给 `Option` 补几个方法
- 用途：取得默认保存目录，会被静态链接进可执行文件

MPL-2.0 的 copyleft 只覆盖它自己的源文件，不要求整个程序换成 MPL-2.0。但分发二进制时有两件事要做：

1. `option-ext` 这部分源码要能拿到，并且以 MPL-2.0 提供。源码位置见上面两行，也可以用 `cargo vendor` 导出全部依赖源码。
2. MPL-2.0 允许和 GPL 代码放进同一个可执行文件（Mozilla 官方 FAQ 第 14 条），条件是这份源码同时也可以按 GPL 使用。本程序整体按 GPL-3.0 发布，满足这一条。

完整的依赖树与各自的许可证可以用以下命令查看：

```bash
cargo tree
cargo metadata --format-version 1
```

Rust 生态里大部分 crate 使用 MIT 或 Apache-2.0 双许可，具体以各 crate 的 `LICENSE` 文件为准。
