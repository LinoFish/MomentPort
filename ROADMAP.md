# MomentPort 路线图

## v1（已提交）

荣耀连拍 JPEG → 苹果连拍文件层标识：同一组共享 BurstUUID、每帧唯一的
ImageUniqueID，像素不重新编码，Make / Model / 拍摄时间 / GPS 原样保留。
导入后是否成组，必须用 PhotoKit 回查 burstIdentifier 才能下结论，
程序不写相册数据库、不冒充苹果设备。

## v1.1（进行中，2026-10-27 定向）

Dong 的电脑装不了 Xcode、只能用命令行，因此底层改用 Rust：

- Rust 转换核心：分组、JPEG EXIF 段级改写、Manifest、跳过原因、Live 配对识别，
  是唯一的转换事实来源，不再另维护一个 Python 转换核心。
- 命令行：`cargo build --release` 出单文件二进制，不依赖 Xcode 工程。
- macOS 桥接层（薄层）：ImageIO 读拍摄时间、PhotoKit 导入并回查是 macOS 专属能力，
  与跨平台的 Rust 文件层分开。纯命令行调 PhotoKit 的签名与权限门槛，
  必须在 Dong 的 Mac 上实测后才能下结论，之前不声称已实现。
- 仍在后续范围、不在本版承诺的：视频连拍/实况的真正转换、相册一定成组。

## 更远的

其他设备照片、视频导入苹果相册的通用工具：按品牌与格式逐步加，
每个新格式先写清“能做什么、不能做什么、怎么验证”，再推代码。
