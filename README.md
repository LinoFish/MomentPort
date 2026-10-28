# MomentPort

**MomentPort 是“其他设备照片、视频导入苹果相册”的通用工具。v1 先只做第一个功能：其他设备连拍 → 苹果连拍文件层标识，以荣耀连拍为例。**

分组、EXIF 改写、Manifest 这些逻辑本来就与品牌无关：荣耀样片按已验证的文件名模式识别连拍；其他 JPEG 散图保留子文件夹和拍摄时间的启发式分组。以后要加的功能（其他品牌照片、视频、实况等）都往这个工程里加，不用再起新名字、新工程。

**v1.2（SwiftUI 界面 + Rust 后端）：界面是 SwiftUI，转换的唯一事实来源仍是 `rust/` 下的 Rust 实现**，界面经 `MomentPort/RustBackend.swift` 用进程调用 Rust 二进制（Bundle Resources → 自选路径 → PATH），预览跑 `--list-json`、转换后读 `MomentPortReport.json` 回填结果。原来 v1 的 Swift 转换引擎文件（`BurstGrouper` / `JpegExif` / `AppleMakerNote` / `MomentPortEngine`）已标注弃用、界面不再调用，仅保留以免破坏 Xcode 工程引用；不要在那里改转换逻辑。之前一度起步的 Python 转换 CLI 已被 Rust 版取代，不再同时维护两个转换实现（`Tools/verify_burst.py` 只保留作文件层校验脚本）。路线见 `ROADMAP.md`。

打包不用 Xcode IDE：`sh scripts/build-app.sh [输出 .app 路径]`（默认项目根目录 `MomentPort.app`）。脚本先 `cargo build --release`，把 Rust 二进制拷进 `.app/Contents/Resources/`，再用 Swift 5 模式、`-O` 和 macOS 12.0 目标编译 SwiftUI，展开并校验 Info.plist，分别签名内置 CLI 与 App，最后严格验证签名。全部成功后才替换旧 App；失败会清理暂存目录并保留旧产物。

Swift 模块缓存默认放在 `${TMPDIR:-/tmp}/momentport-swift-module-cache`，避免沙箱内无法写入用户 Library 默认缓存。可用 `SWIFT_MODULE_CACHE_PATH` / `CLANG_MODULE_CACHE_PATH` 指定其他可写缓存；Rust 输出目录尊重 `CARGO_TARGET_DIR`。首次使用当前工具链和 SDK 组合时，可能提示缺少兼容的预编译标准库并花几分钟生成系统模块缓存，后续构建会复用。产物架构与构建机一致（当前验证为 Intel `x86_64`），不是 Universal App；临时签名不等于 Developer ID 签名或公证。

## 先说最重要的结论（不要跳过）

苹果**没有公开**“把外部照片写成连拍”的文件格式，PhotoKit 也**没有**公开 API 可以直接设置 `burstIdentifier`。

所以本程序能做到、也只承诺做到的是：

1. 把荣耀导出后散开的 JPEG，按组写上 iPhone 连拍在文件里用的分组标识；
2. 像素不重新编码，原图的 Make / Model / 拍摄时间 / GPS / 方向原样保留；
3. 导入相册后，**用 PhotoKit 把刚导入的资产读回来，告诉你到底成组没有**——成组了就是成组了，没成组程序会直说，不会把“已导入”说成“已成组”。

2026-10-28 对照新增原生样片发现：旧版本写入的 MakerNote 缺失 Apple 签名头，自己的读回校验也沿用了同一错误。旧文件校验通过不能证明系统可读，更不能据此认定是相册版本的问题。现已修正原生布局并通过系统 ImageIO 读取；**用户已于 2026-10-28 确认本次 Apple Photos 实际验证通过**。这一验收仅覆盖本次用户测试环境与文件，不代表所有系统版本、设备或导入途径均已验证。未修改相册数据库。详细证据和复验步骤见 [验证记录](macos/APPLE_BURST_VALIDATION.md)。

## 它到底写了什么

iPhone 连拍的分组标识，在 ExifTool 的 Apple MakerNote 表里是：

- `0x000b BurstUUID`：同一组连拍共享同一个值；
- `0x002b PhotoIdentifier`：每帧各不相同；新增原生样片使用此字段，未含旧实现假定的 `0x0015`。清单保留 `imageUniqueID` JSON 键以兼容 Swift，值对应 PhotoIdentifier；
- `0x0001 MakerNoteVersion`：按样片写 signed LONG（类型 9）、值 17。此值不代表设备品牌或系统版本；
- 完整 `Apple iOS\0\0\x01MM` 14 字节头，IFD 在 14 开始；MakerNote 使用独立大端，偏移相对整个 MakerNote 的起点，字符串 count 包含 NUL。外层 TIFF 可为大端或小端。

程序的做法（v1.2 起是 `rust/crates/momentport-core/`，旧 Swift 版是 `MomentPort/AppleMakerNote.swift` + `MomentPort/JpegExif.swift`，已弃用）：

- 只改 JPEG 的 EXIF 段：解析原 TIFF（IFD0 / ExifIFD / GPS / Interop / 简单 JPEG 缩略图 IFD1），把原有条目原样重建，只替换 / 新增 MakerNote 条目（`0x927C`）；
- 像素扫描数据（SOS 之后）原样复制，**不解码、不重新编码**；
- **不改 Make / Model**：荣耀的照片仍写 HONOR / 机型，不冒充苹果设备；
- 不改拍摄时间：分组和排序都以原 `DateTimeOriginal + SubSecTimeOriginal` 为准；
- 每组一个 `BurstManifest.json`：记录 BurstUUID、帧数、关键帧、每帧的 PhotoIdentifier（JSON 键仍为 imageUniqueID）和来源文件名，便于复查。

## 荣耀连拍是怎么分组的

按以下优先级识别（SwiftUI 预览和转换均调用同一个 Rust 后端）：

1. **荣耀文件名标识优先**：严格匹配 `IMG_YYYYMMDD_HHMMSS_BURSTddd[_COVER].JPG/JPEG`（大小写不限、日期有效、三位序号非零）。同目录、同日期时间前缀为一组；不同目录隔离。同一个子文件夹可以包含多组，帧缺失或跨度超过 `--gap` 不会拆散已识别组。EXIF Make 明确为其他厂商时不采用这条规则；Make 缺失允许仅按文件名识别。这是荣耀样片的命名模式，不能仅凭它证明设备品牌。
2. **剩余照片按子文件夹分组**：每个子文件夹的剩余照片 ≥2 张时算一组。
3. **剩余顶层照片按时间切分**：按拍摄时间排序，相邻间隔 ≤ gap（默认 2 秒）归一组。此项与子文件夹规则仍是启发式推测，可能需要人工检查。
4. **只有一帧不算连拍**：已识别 ID 仅剩一帧（含孤立封面）也列为单张，不回流到子文件夹/时间分组，不写 BurstUUID。

荣耀组按数字帧序号排序；缺少 `001` 或序号不连续均可。有 `_COVER` 默认选为关键帧（它本身也保留为一帧）；缺封面选现存最小序号帧。多个封面标记时按上述排序选第一个。CLI 显式 `--key-index` / `--key-indexes` 或界面点选会覆盖自动选择，越界钳制到最后一帧。未识别照片沿用拍摄时间排序、默认首帧。

分组不符时可先按组分到不同子文件夹。文件名已丢失且没有可信连拍 ID 的照片，仍只能使用上述启发式规则；本版不猜测未知私有 tag。

### 真实样片证据（2026-10-28）

只读检查 `样片/Honor/1`–`4` 共 **46 张 JPEG、5 个命名前缀**：目录 1 为 3 帧、无封面（序号 002/005/090）；目录 2 为 25 帧、有封面；目录 3 为 4 帧、有封面；目录 4 混放两组各 7 帧，一组有封面、一组无封面。

所有样片的 EXIF Make/Model 为 `HONOR` / `LSA-AN00`，ExifIFD `0x927c` MakerNote 仅为 5 字节 `AF_S\0`；未观察到可用的专用连拍 ID 或封面 tag，不把这个共有值解释成连拍语义。JPEG 头部仅有 EXIF APP1、DQT、SOF0、DHT，未见 XMP APP1；最终 EOI 后无附加数据。可验证的连拍/封面标识来自文件名；EXIF 日期与 `0x9291 SubSecTimeOriginal` 用于拍摄时间（原代码误用 `0xa357`，已修正）。这些结论限于本批文件，不代表所有荣耀机型都没有专用元数据。

本次验证：Rust `cargo test` **28 项通过**（原 22 项 + 6 项荣耀回归），`cargo build --release` 成功。真实样片以原目录、临时符号链接平铺、临时排除封面三种输入验证，均为 5 组；原目录帧数为 3/25/4/7/7，排除封面后为 3/24/3/6/7。46 帧副本转换成功、0 跳过，5 组均通过 `Tools/verify_burst.py`（同组 UUID 一致、逐帧 ID 唯一、SOS 与原图一致），封面/缺封面的 manifest 关键帧与预览一致。App bundle 内置 CLI 与独立 CLI 的预览 JSON 完全一致；原始输入 48 个文件（含辅助文件）的路径及 SHA-256 前后完全一致。该自动回归轮次未向个人照片图库导入、未验证 PhotoKit 成组；后续用户已确认 Photos 实际验证通过（见验证记录）。

Swift 验证：用项目原有 `RustBackend.swift`、`Models.swift` 及其依赖编译临时命令行驱动，实际执行 `preview` → `toGroups` → `convert`，46 张真实样片的 JSON 解码、5 组映射、转换报告/清单和关键帧均通过；未调用 PhotoKit。荣耀识别会话当时的完整 App 构建在指定临时模块缓存后，仍停在未修改的 `ContentView.swift:177`（`body` 表达式类型检查超时）；该会话未完成完整 SwiftUI App 构建。后续修复与实际 App 验证见下一节。本次只在 Swift 文件中同步了说明注释，没有修改旧 Swift 转换/分组逻辑。

> 以下旧轮次的 UUID / 文件层“通过”沿用了旧格式，不能用于证明 Apple 兼容；本次修复与系统读取证据见 [Apple 连拍验证记录](macos/APPLE_BURST_VALIDATION.md)。

### 完整 App 构建修复与验证（2026-10-28，macOS）

- 环境：macOS 12.7.6、Command Line Tools Swift 5.7.2、macOS SDK 13.1，无需 Xcode IDE。保存的原始 `ContentView` 再次复现第 177 行类型检查超时。顶层 `VStack` 的 ViewBuilder 有 23 个直接子项，超过此 SDK `buildBlock` 最多 10 项的重载上限，并叠加嵌套帧列表的类型推断。
- 修复：按现有区域拆成独立 `@ViewBuilder` 属性，连拍组提取为接收 `Binding<BurstGroup>` 的小 View；不增加布局容器，不改文案、步骤、帧条/Picker 的关键帧选择语义，也不改 Rust 转换或 PhotoKit 导入逻辑。打包脚本补齐可写缓存、最低系统版本、暂存发布与签名校验。
- 完整执行 `sh scripts/build-app.sh` 成功；生成项目根目录 `MomentPort.app`，内含 SwiftUI 主程序、展开后的 Info.plist、可执行 Rust 后端与签名资源。主程序和 CLI 均为 `x86_64` Mach-O、最低 macOS 12.0，动态依赖均为系统库；App 及 CLI 均通过 `codesign --verify --strict`，App 同时通过 `--deep`。这是本机可用的 ad-hoc 签名构建，未公证。
- 启动冒烟：启动实际 App 8 秒，确认 `isFinishedLaunching=true`、一个可见的 900×648 窗口、进程持续运行且启动日志为空，随后关闭本次测试进程；未点击按钮，未请求照片图库权限或导入照片。
- 回归：Rust 28 项测试通过；以打包后的 CLI 运行实际 `RustBackend.preview` / `toGroups` / `convert`，46 张样片仍为 3/25/4/7/7 帧，预览与清单关键帧一致；五组逐一通过 `verify_burst.py` 的 UUID、逐帧唯一 ID 和 SOS 原样校验。内置 CLI 与独立 release CLI 预览 JSON 一致，48 个原始文件的路径及 SHA-256 与上次会话和本次运行前均一致。
- 打包可靠性：模拟 Swift 编译失败时，不生成新半成品，已有产物保持原样，暂存目录清理。未验证完整交互流程或 PhotoKit 在个人图库中的成组效果；本次没有访问个人照片图库。

## 命令行用法（不用 Xcode，v1.1 主推）

Dong 的电脑装不了 Xcode，所以 v1.1 的主推路径是 Rust 命令行，不依赖 `.xcodeproj`：

```sh
cd rust
cargo build --release
cargo test
./target/release/momentport-cli <输入文件夹> <输出文件夹> --gap 2.0
```

常用选项（输入、输出文件夹必填，缺失时退出码 2）：

```sh
# 只列分组，不写任何文件（也不会创建输出文件夹）
./target/release/momentport-cli <输入文件夹> <输出文件夹> --dry-run
./target/release/momentport-cli <输入文件夹> <输出文件夹> --list

# 机器可读预览（供 SwiftUI 界面解析）：只列分组不写文件，输出文件夹可省略
./target/release/momentport-cli <输入文件夹> --list-json --gap 2.0

# 改切分间隔、改关键帧、把单张也原样拷出来
./target/release/momentport-cli <输入文件夹> <输出文件夹> --gap 3.0 --key-index 1 --copy-singles

# 每组不同的关键帧：按组顺序给，缺省的组用 --key-index 的值
./target/release/momentport-cli <输入文件夹> <输出文件夹> --key-indexes 1,0
```

参数说明：`--gap` 默认 2.0 秒（平铺文件夹里相邻间隔 > gap 切分，≤ gap 归一组）；`--key-index` 未指定时自动选荣耀封面或首帧，显式指定时覆盖自动选择（越界钳制到最后一帧）；`--key-indexes` 按组顺序覆盖关键帧，缺省组保留全局选择或自动选择，越界同样钳制；`--copy-singles` 默认关闭（单张只记录，不写 BurstUUID；打开后原样拷到 `Singles/`，仍不写 BurstUUID）。

`--list-json` 字段对照（改 Rust 那边必须同步改 `MomentPort/RustBackend.swift`，否则界面解码失败）：`gap` / `groups[{sourceDescription, keyIndex, photos[{fileName, dirKey, captureMs, captureISO, hasExifDate, liveVideoFileName}]}]` / `singles[同 photo]` / `livePairs[{photoFileName, photoDirKey, videoFileName}]` / `unprocessed[{fileName, dirKey, reason}]`。注意：预览里没有 `burstUUID`——Rust 每次分组都会新生成 UUID，预览那次的 UUID 不是转换那次的，界面不在预览里展示它；真正的 burstUUID 只在转换报告里读到。

输出结构（原图文件夹原封不动）：

```text
输出文件夹/
  Burst_01/  Burst_02/  …   每组一个子文件夹，保留原文件名 + BurstManifest.json
  Singles/                  仅 --copy-singles 时有：单张原样复制 + SinglesManifest.json
  MomentPortReport.json     汇总：组、单张、live 配对、未处理原因
```

稳健性与明确处理（与 Swift 版口径一致，另见下文“限制”）：大写扩展名（`.JPG`）与小写同等处理；`.` 开头隐藏文件跳过并记原因；缺 EXIF 时间用文件修改时间兜底并在日志里标 `mtime兜底`；HEIC/视频等不静默丢弃，日志与报告里写清未处理原因，视频注明“未写 BurstUUID，视频连拍/实况转换不在本版范围”；同名 `.jpg` + `.mov` 标为 live 配对，两者拷到同一输出文件夹、保持同名，`.mov` 不写 BurstUUID，**不声称已转成苹果实况**；扫描只读顶层与一层子文件夹。

macOS 专属能力分层见 `macos/BRIDGE.md`：Rust 核心只负责文件层；读 EXIF 时间可用 ImageIO、导入相册并回查 `burstIdentifier` 必须走 PhotoKit，那是一层薄桥接。**纯命令行调 PhotoKit 的签名/权限（TCC）门槛必须在 Dong 的 Mac 上实测后才能下结论**，本版不声称已复用 PhotoKit；`macos/import_to_photos.applescript` 只是把照片加进 Photos 的兜底，不声称成组、不回查。

## 使用步骤

### 准备

1. 在荣耀手机图库里打开连拍，选要保留的帧，“另存为 / 保存”到手机存储（或直接用数据线把 DCIM 里的连拍 JPEG 拷到 Mac）。
2. 建议按组分文件夹，例如：`荣耀连拍/生日蛋糕/`、`荣耀连拍/球场/`。

### 在 macOS 12 上构建（v1 界面版，v1.1 命令行不需要这一步）

本工程面向 **macOS 12、Xcode 13、Swift 5**：

```sh
open MomentPort.xcodeproj
```

在 Xcode 里点 Run 即可。首次“导入到相册并验证”时，系统会问是否允许访问照片图库，点允许。

说明：

- 工程没有开 App Sandbox（`ENABLE_APP_SANDBOX = NO`），因为它要读用户任意选的文件夹、写输出文件夹并调用 PhotoKit；这是照片工具的常见做法，不联网、不上传。
- Bundle ID：`com.linofish.MomentPort`。

### 在程序里走四步

1. 点“选择荣耀照片文件夹”，程序自动分组；检查组数、帧数、时间范围对不对，必要时调间隔、改关键帧。
2. 点“选择输出文件夹”，点“转换”。原文件夹原封不动，输出在另一个文件夹，每组一个 `Burst_01`、`Burst_02`……子文件夹，里面是写了 BurstUUID 的副本 + `BurstManifest.json`。
3. 点“导入到相册并验证”。此按钮通过 PhotoKit 导入**系统照片图库**，不跟随 Photos 窗口选择的其他图库。隔离测试时应在 Photos 中手动导入文件。程序按 PhotoKit 正常流程导入（关键帧先导入，只是尽力让它当代表图，苹果没有承诺导入顺序决定代表图），然后包含全部连拍资产回查每帧的 `burstIdentifier` / `representsBurst`，并用 `fetchAssets(withBurstIdentifier:)` 确认组内包含全部新导入资产。所有帧均有同一非空组 ID、数量正确且恰有一张代表图才显示“已识别”。
4. 看日志，不要只看相册：日志会写“已识别为连拍”还是“已导入但未被识别为连拍”。再到相册的“媒体类型 > 连拍”里核对；打开连拍点“选出最佳”，可以改代表图。

## 不用相册也能先验文件

`Tools/verify_burst.py` 只用 Python 标准库，不依赖 exiftool：

```sh
python3 Tools/verify_burst.py 输出文件夹/Burst_01 原图文件夹
```

它检查带 Apple 头的 MakerNote、同组 BurstUUID、每帧 PhotoIdentifier 唯一，以及转换后 SOS 扫描数据与原图相同。旧版无头部的自定义 IFD 会失败。系统读取与实际成组是另外的验证层，命令见 [验证记录](macos/APPLE_BURST_VALIDATION.md)。

**这只是文件层结构校验，不证明相册一定成组。** 最终结论以程序里“导入并验证”的 PhotoKit 回查为准。

## 如果没成组

先不要删输出文件夹，按顺序排查：

1. 确认导入的是**转换后的输出文件夹**，不是原图；
2. 确认日志里到底是“未被识别”还是导入本身失败；
3. 兜底浏览方式：在相册里为该组建一个同名相簿（例如 `Burst_01`），把这组照片放进去——浏览体验接近成组，但它不是系统连拍，没有“选出最佳 / 只保留选中帧”的连拍操作；
4. 旧版本的无头 MakerNote 输出需要用新版重新转换。保留 `BurstManifest.json`，用少量新版输出和原生样片在隔离图库中对照复验，不要反复批量导入个人图库。

## 限制（v1 / v1.1，写在前面）

- 只处理 JPEG（`.jpg` / `.jpeg`，大小写不限）。连拍如果存成 HEIC 或其他格式，程序会跳过并在日志/报告里写明原因，不会假装转了。
- 视频、实况照片的真正转换是后续功能，v1.1 没有实现，不要按已实现来用。**Live 配对只是识别 + 同拷**：同名 `.jpg` + `.mov` 会被标注并拷到同一文件夹，`.mov` 不写 BurstUUID，不等于已转成苹果实况。
- 扫描只读顶层与一层子文件夹；更深的嵌套不会被当成组，请先把组文件夹放到顶层下一层。
- 复杂缩略图（带 StripOffsets 的 IFD1）重建时会丢弃缩略图 IFD，主图不受影响，相册会重新生成缩略图。
- 不写、不改相册的 SQLite 数据库，不改 iCloud 状态。
- 关键帧选择只影响导入顺序和 manifest 记录，代表图的最终决定权在相册里。

## 文件清单

```text
MomentPort.xcodeproj/project.pbxproj   Xcode 工程（macOS 12、Swift 5）
MomentPort/
  App.swift                 SwiftUI 入口
  ContentView.swift         四步界面（v1.2）：选文件夹 → 分组 → 转换 → 导入验证，走 RustBackend
  DesignSystem.swift        v1.2 设计系统：品牌色、间距、按钮、帧条、状态条
  RustBackend.swift         v1.2 唯一接线层：Process 调 Rust 二进制，--list-json 预览、读报告回填
  Models.swift              PhotoFile / BurstGroup / Manifest / 验证结果
  BurstGrouper.swift        【已弃用】v1 Swift 分组，界面不再调用
  JpegExif.swift            【已弃用】v1 Swift JPEG 改写，界面不再调用
  AppleMakerNote.swift      【已弃用】v1 Swift MakerNote，界面不再调用
  MomentPortEngine.swift    【已弃用】v1 Swift 转换引擎；Result 类型仍被导入清单复用，逻辑不再调用
  PhotoImporter.swift       PhotoKit 导入 + burstIdentifier 回查（仍是导入验证的唯一入口）
  Info.plist                照片权限说明等
scripts/
  build-app.sh              v1.2 打包：cargo + swiftc 成 .app，不用 Xcode IDE
Tools/
  verify_burst.py           不依赖 exiftool 的文件层校验脚本（只校验，不转换）
rust/                       v1.1 转换核心（唯一事实来源，零外部 crate）
  Cargo.toml                工作区
  crates/momentport-core/   库：分组、JPEG EXIF 段级改写、MakerNote、Manifest
  crates/momentport-cli/    二进制 momentport-cli：cargo build --release 出单文件
macos/
  BRIDGE.md                 macOS 能力分层：文件层 / ImageIO / PhotoKit 回查 / 兜底
  import_to_photos.applescript  只把照片加进 Photos 的兜底，不声称成组
ROADMAP.md                  路线图：v1、v1.1 Rust 转向、更远的通用导入
```

## 早期本地验证记录（Linux，历史记录）

以下为早期会话记录；当前 macOS 完整 App 构建与启动结果见上面的“完整 App 构建修复与验证”。

当时的开发机是 Linux，没有 Swift / Xcode，所以**没有在 macOS 12 上编译，也没有在真机相册上导入验证**——这是当时的验证边界；后续 macOS 构建及用户 Photos 验收见当前验证记录。

已在 Linux 上用与 Swift 代码相同布局算法的 Python 镜像做过合成 JPEG 验证：

- 原图：HONOR、Magic6 Pro、含 GPS、含长字符串 EXIF、含 JPEG 缩略图、无 BurstUUID；
- 转换后：`verify_burst.py` 读回 BurstUUID 与送入值一致，ImageUniqueID 唯一；
- SOS 扫描数据与原图完全相同（像素未重编码）；
- Make / Model / DateTimeOriginal 在转换后 TIFF 里原样保留。

以上仅记录早期文件层验证；当前验收状态以 [Apple 连拍验证记录](macos/APPLE_BURST_VALIDATION.md) 为准。

## v1.1 Rust 验证记录（协调员亲自重跑，2026-10-27，Linux）

- 工具链：工作区内 rustup，`cargo/rustc 1.99.1`，未改系统目录；`cargo test` **19 passed，0 failed**（含分组 gap 边界、MakerNote 双字节序往返、JPEG SOS 原样、TIFF 往返、UUID 形状、扫描大小写/隐藏/live 配对）。
- `cargo build --release` 成功，单文件 `momentport-cli` 约 761 KB。
- 协调员独立合成测试集（不用子代理的测试集）：子文件夹组 3 帧（含大写 `.JPG`）、平铺时间切分 f1+f2、缺 EXIF 时间用 mtime 兜底 n1+n2、live 子文件夹 2 帧 + 同名 `.mov`、单张 f3 与 solo、未处理 3 项（隐藏文件、`.heic`、未配对 `.mov`）。结果：4 组共 9 帧全部成功 0 跳过；`--dry-run` 未创建输出文件夹；`--copy-singles` 下单张原样进 `Singles/`；`verify_burst.py` 逐组通过（同组 BurstUUID 唯一 1 种、ImageUniqueID 各不相同、SOS 与原图一致）；live 的 `.mov` 字节与原文件一致；原图文件夹 SHA-256 对比**全部未动**。
- 协调员测试中发现并已修复一个真实问题：部分写入器（含 PIL）把 `0x9003 DateTimeOriginal` 直接放在 IFD0 而非 ExifIFD 里，Rust 原版只在 ExifIFD 里找，会误退回 mtime 兜底、导致平铺组错合。已补 IFD0 兜底并加单测（`exif_time_in_ifd0_is_recognized`），修复后上述分组才全部正确。真实设备文件通常在 ExifIFD 里，修复前也不会在真机文件上全军覆没，但 ImageIO 式读取本应两种位置都认。
- **未验证、不要按已验证来用**：没有在 macOS 上编译运行之外的验证（本机无 Mac），没有在相册里成组，没有复用 PhotoKit；AppleScript 兜底未经 macOS 实测；Rust 把无时区 EXIF 墙上时间按 UTC 换算仅用于排序切分，macOS 侧如需绝对本地时间应以 ImageIO 为准。

## v1.2 验证记录（2026-10-28，Linux；Swift 部分未编译，见下）

已做：
- Rust：`cargo test` 22 通过（核心 19 + CLI 新增 3：--list-json 不要求输出文件夹、--key-indexes 按组顺序解析、非法取值报错）。
- 合成集：子文件夹组 2 帧（含大写 `.JPG`）、平铺组 2 帧、单张 2 张（含 1 对 live 配对）、未处理 1 项（HEIC）。`--list-json` 经 `python3 -m json.tool` 校验为合法 JSON，组顺序关键帧为 `1,0`（与传入的 `--key-indexes 1,0` 一致）；转换后 `verify_burst.py` 两组均通过（同组 BurstUUID 唯一 1 种、ImageUniqueID 各不相同、SOS 与原图一致）。
- 字段对照：Rust `--list-json` 键名与 `RustBackend.swift` 的 `Preview` / `PreviewGroup` / `PreviewPhoto` / `PreviewLivePair` / `PreviewUnprocessed` 逐一核对一致；转换报告键名与 `Report` / `ReportGroup`、清单键名与 `BurstManifest` 逐一核对一致（报告其余字段 Swift 会忽略未知键）。
- Swift 文件只做了括号配对与“界面不再调用旧引擎”的 grep 检查，未做编译。

未做（原因）：
- Swift 编译与界面实测：本机是 Linux，没有 Swift / Xcode，`scripts/build-app.sh` 只在 macOS 上能跑到 swiftc 那步；ContentView 的新布局、帧条点选、错误条、导入分开呈现均未在 Mac 上实测，**不要按已验证来用**。
- PhotoKit 回查：仍待 Dong 在 Mac 上导入后验证，与 v1 / v1.1 相同。
- 推送 GitHub：本地改完即停，未推；推送要等 Dong 能及时批准的时间窗口再做，避免上次审批超时过期。

## 测试与私有样片

常规测试不需要任何照片或视频：

```sh
MACOSX_DEPLOYMENT_TARGET=12.0 cargo test --manifest-path rust/Cargo.toml
swiftc -module-cache-path /tmp/momentport-swift-module-cache MomentPort/Models.swift Tools/verify_import_facts.swift -o /tmp/verify-import-facts
/tmp/verify-import-facts
```

真实样片不随仓库发布。`native_samples` 的两个测试默认标记为 ignored；仅在本机备好私有样片后，显式运行 `cargo test --manifest-path rust/Cargo.toml --test native_samples -- --ignored`。默认查找根目录 `样片/`，也可用环境变量 `MOMENTPORT_PRIVATE_SAMPLES` 指向本机目录（需含 `apple/` 与 `Honor/`）；不要将个人绝对路径写进代码。`Tools/regress_private_samples.py` 使用同一环境变量，亦须手动运行，输出仅写入被忽略的 `rust/target/`。完整步骤见验证记录。

`.gitignore` 排除私有 `样片/`、App bundle、Rust 构建/回归输出、暂存打包目录和 macOS 元数据；合法项目资源不按图片扩展名一概忽略。提交时仍须逐文件审查，禁止加入其他位置的个人媒体或数据。
