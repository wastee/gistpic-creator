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

微信小程序版也使用同一个模型文件：小程序不内嵌模型，首次启用自动识别时从配置的地址下载到用户手机本地，再交给微信原生推理接口。分发小程序或提供模型下载地址时，同样需要保留上述署名和 CC-BY-SA-4.0 许可说明；小程序“关于与开源许可”页已经写了这些信息。

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
| thiserror | MIT OR Apache-2.0 | 错误类型 |
| log / env_logger | MIT OR Apache-2.0 | 日志 |
| tray-icon | MIT OR Apache-2.0 | 系统托盘图标与菜单 |
| muda | MIT OR Apache-2.0 | 托盘菜单 |
| ksni | Unlicense | Linux 托盘（StatusNotifierItem） |
| zbus | MIT | Linux 托盘宿主检测 |
| objc2 | MIT | macOS 调用 AppKit |
| objc2-app-kit | Zlib OR Apache-2.0 OR MIT | macOS 恢复最小化窗口 |
| candle | MIT OR Apache-2.0 | 开发训练工具训练文字块角色分类器；不随桌面应用发布 |

完整的依赖树与各自的许可证可以用以下命令查看：

```bash
cargo tree
cargo metadata --format-version 1
```

Rust 生态里大部分 crate 使用 MIT 或 Apache-2.0 双许可，具体以各 crate 的 `LICENSE` 文件为准。
