# macOS 桥接层说明（MomentPort v1.1）

> 先说诚实底线（不要跳过）：苹果**没有公开**“写 BurstUUID 就一定成组”的保证，
> PhotoKit **没有**公开 API 可以直接设置 `burstIdentifier`；`burstIdentifier` /
> `representsBurst` 都是只读属性。本文件不写一个假的 PhotoKit 桥接实现来冒充；
> 下面分层写清楚：哪一层已实现、哪一层必须在 Dong 的 Mac 上实测后才能下结论。

## 三层分工

### 第 1 层：Rust 核心（文件层，跨平台）——已实现

位置：`../rust/`（`momentport-core` 库 + `momentport-cli` 二进制，仅标准库）。

- 分组：子文件夹优先、平铺按拍摄时间切分（默认间隔 2.0 秒）；
- JPEG 只改 EXIF 段、像素 SOS 原样复制；Apple MakerNote 写 BurstUUID（0x000b，
  同组同一大写连字符 UUID）、每帧不同 ImageUniqueID（0x0015）、版本 15（0x0001）；
- 输出 `Burst_01/`、`Burst_02/`… + `BurstManifest.json` + 顶层 `MomentPortReport.json`；
- 转换后回读校验 BurstUUID / ImageUniqueID 再计成功；原图文件夹原封不动；
- 绝不写/改相册 SQLite；绝不把 Make / Model 改成 Apple。

这一层可在 Linux 上 `cargo test`、跑 CLI 全流程；本机 Linux 已实测的部分见任务回报，
Mac 上只需 `cargo build --release` 即可得到同一份核心（见文末验证命令）。

### 第 2 层：macOS 专属能力——必须分层，不能混进 Rust 核心

| 能力 | 正确归属 | 说明 |
| --- | --- | --- |
| 读 EXIF 拍摄时间 | ImageIO（`CGImageSourceCopyPropertiesAtIndex`） | 现有 Swift 版 `BurstGrouper.readPhotoInfo` 就是这么读的；Rust 核心在 Linux 上只能直接解析 JPEG EXIF TIFF，行为等价但时区口径不同：Rust 把无时区的 EXIF 墙上时间按 UTC 换算，只用于排序与切分；混用 EXIF 时间与文件修改时间兜底时，macOS 侧如需绝对本地时间，应以 ImageIO 的绝对时间为准。 |
| 导入相册并回查是否成组 | PhotoKit（`PHPhotoLibrary` / `PHAsset`） | 唯一能回答“相册到底成组没有”的办法：按正常流程导入转换后的文件，再把刚导入的资产读回来，看 `burstIdentifier` / `representsBurst`。现有 Swift 版 `PhotoImporter.importAndVerify` 就是这个做法：关键帧先导入只是尽力让它当代表图，苹果没有承诺导入顺序决定代表图。 |
| 把照片加进 Photos（兜底） | AppleScript（见 `import_to_photos.applescript`） | 只把照片加进 Photos，**不声称成组**。成组与否仍须用 PhotoKit 回查；AppleScript 导入后如需确认，同样要走 PhotoKit 回查那一步。 |

### 第 3 层：优先方案与待实测项——不要提前下结论

优先方案是：**Rust 核心 + 极小 macOS 桥接**，二选一：

1. **objc2 / FFI 调 PhotoKit**：写一个极小的 macOS 专属桥接程序（Swift 或 Rust + objc2），
   只做两件事——按正常流程导入 + 回查 `burstIdentifier` / `representsBurst`。
   它不改相册数据库、不设 burstIdentifier（设不了），只报告回查事实。
2. **AppleScript 调 Photos 导入作兜底**：`import_to_photos.applescript`，只导入不验证。

**纯命令行 PhotoKit 授权在 macOS 上有额外门槛，必须在 Dong 的 Mac 上实测后才能下结论：**

- 命令行程序触发的照片图库授权提示、签名（是否需要 Developer ID / 本地签名）、
  TCC（透明度、同意与控制）记录方式，都与 Xcode App 不同；
- 首次在命令行跑导入时，系统是否弹授权、授权后 TCC 是否持久、沙箱/未签名是否被拦截，
  都要实测，不能凭推测写进文档；
- 在实测结论出来之前，**不要声称已复用 PhotoKit**，也不要声称“命令行导入已验证”。

## AppleScript 兜底导入脚本用法

```sh
osascript macos/import_to_photos.applescript "/path/to/输出文件夹/Burst_01"
```

- 脚本只做：把该文件夹里的 JPEG 加进 Photos（`import` 命令，`skip check duplicates` 设为 false，
  即不跳过重复项，由用户在 Photos 里自行处理重复）；
- 脚本**不**写 BurstUUID、**不**改文件、**不**回查 burstIdentifier、**不**声称成组；
- 导入后请用 PhotoKit 回查（现有 Swift 版 `PhotoImporter`，或待实测的极小桥接）确认，
  再到 Photos 的“媒体类型 > 连拍”里核对；没成组时不要删输出文件夹，兜底浏览方式是
  在 Photos 里为该组建同名相簿（见主 README“如果没成组”一节）。

## Mac 上待验证的命令

```sh
# 1. 构建 Rust 核心（需先装 Rust，见 https://rustup.rs）
cd rust
cargo build --release
cargo test

# 2. 跑 CLI（示例路径按实测替换）
./target/release/momentport-cli <输入文件夹> <输出文件夹> --gap 2.0
./target/release/momentport-cli <输入文件夹> <输出文件夹> --dry-run

# 3. 文件层校验（仍用保留的 Tools/verify_burst.py，不改它）
python3 ../Tools/verify_burst.py <输出文件夹>/Burst_01 <原图文件夹>

# 4. AppleScript 兜底导入（只导入，不声称成组）
osascript ../macos/import_to_photos.applescript "<输出文件夹>/Burst_01"

# 5. PhotoKit 回查（待在 Dong 的 Mac 上实测命令行授权后再定最终命令）：
#    现有 Swift 工程的 PhotoImporter.importAndVerify 是已实现的回查做法；
#    命令行桥接的授权/签名/TCC 结论出来前，不写死一条“已验证”的命令。
```

## 已知限制（v1.1 Rust 核心，写在前面）

- 只转换 JPEG（`.jpg` / `.jpeg`，大小写不敏感）；HEIC / 视频等不静默丢弃，
  记入日志与 `MomentPortReport.json` 的未处理原因；
- 视频连拍 / 实况转换不在本版范围；`.mov` 只在与同名照片配对时原样复制到
  同一输出文件夹，**不**写 BurstUUID，**不**声称已转成苹果实况；
- 复杂缩略图（带 StripOffsets 的 IFD1）重建时会丢弃缩略图 IFD，主图不受影响，
  相册会重新生成缩略图（与 Swift 版一致）；
- 扫描只读顶层与一层子文件夹；隐藏文件跳过并记原因；
- 缺 EXIF 时间用文件修改时间兜底，并在日志里标注 `mtime兜底`；
- 关键帧选择只影响 manifest 记录（CLI 本身不导入相册，导入顺序的代表图
  尽力而为部分属于 PhotoKit 桥接层，苹果没有承诺导入顺序决定代表图）。
