# MomentPort

**MomentPort 是“其他设备照片、视频导入苹果相册”的通用工具。v1 先只做第一个功能：荣耀连拍 → 苹果连拍。**

以后要加的功能（其他品牌照片、视频、实况等）都往这个工程里加，不用再起新名字、新工程。

## 先说最重要的结论（不要跳过）

苹果**没有公开**“把外部照片写成连拍”的文件格式，PhotoKit 也**没有**公开 API 可以直接设置 `burstIdentifier`。

所以本程序能做到、也只承诺做到的是：

1. 把荣耀导出后散开的 JPEG，按组写上 iPhone 连拍在文件里用的分组标识；
2. 像素不重新编码，原图的 Make / Model / 拍摄时间 / GPS / 方向原样保留；
3. 导入相册后，**用 PhotoKit 把刚导入的资产读回来，告诉你到底成组没有**——成组了就是成组了，没成组程序会直说，不会把“已导入”说成“已成组”。

如果你在某台 Mac / 某个相册版本上导入后验证说“未被识别为连拍”，那就是这个版本导入器不认文件层标识，程序没有办法绕过它去改相册数据库——**故意不写相册数据库**，那会损坏图库、影响 iCloud 同步，得不偿失。兜底办法见下面“如果没成组”。

## 它到底写了什么

iPhone 连拍的分组标识，在 ExifTool 的 Apple MakerNote 表里是：

- `0x000b BurstUUID`：同一组连拍共享同一个值；
- `0x0015 ImageUniqueID`：每帧各不相同；
- `0x0001 MakerNoteVersion`：本程序写 15（对应 macOS 12 / iOS 15 时代常见的版本，不是声称照片就是 iPhone 拍的）。

程序的做法（`MomentPort/AppleMakerNote.swift` + `MomentPort/JpegExif.swift`）：

- 只改 JPEG 的 EXIF 段：解析原 TIFF（IFD0 / ExifIFD / GPS / Interop / 简单 JPEG 缩略图 IFD1），把原有条目原样重建，只替换 / 新增 MakerNote 条目（`0x927C`）；
- 像素扫描数据（SOS 之后）原样复制，**不解码、不重新编码**；
- **不改 Make / Model**：荣耀的照片仍写 HONOR / 机型，不冒充苹果设备；
- 不改拍摄时间：分组和排序都以原 `DateTimeOriginal + SubSecTimeOriginal` 为准；
- 每组一个 `BurstManifest.json`：记录 BurstUUID、帧数、关键帧、每帧的 ImageUniqueID 和来源文件名，便于复查。

## 荣耀连拍是怎么分组的

荣耀图库里的“连拍组”在导出 / 另存后就不再是一个文件，导出来就是一堆单独的 JPEG，组信息丢了。程序按下面优先级把组找回来：

1. **子文件夹优先（最可靠）**：输入文件夹下每个含 ≥2 张照片的子文件夹算一组。建议在荣耀图库里每组“另存为”到不同文件夹，再把这些文件夹一起交给程序。
2. **平铺文件夹按时间切**：按拍摄时间排序，相邻两张间隔 ≤ 2 秒归一组（界面里可改 0.5–10 秒）。荣耀连拍帧间隔通常远小于 1 秒，两次按住快门之间通常不止 2 秒。
3. 只有 1 张的“组”不算连拍，列为未成组，**不写 BurstUUID**，转换时跳过。

分组错了怎么办：先把照片按组分到不同子文件夹，再重新选输入文件夹——这比在界面里拖拽合组更可靠。

## 使用步骤

### 准备

1. 在荣耀手机图库里打开连拍，选要保留的帧，“另存为 / 保存”到手机存储（或直接用数据线把 DCIM 里的连拍 JPEG 拷到 Mac）。
2. 建议按组分文件夹，例如：`荣耀连拍/生日蛋糕/`、`荣耀连拍/球场/`。

### 在 macOS 12 上构建

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
3. 点“导入到相册并验证”。程序按 PhotoKit 正常流程导入（关键帧先导入，只是尽力让它当代表图，苹果没有承诺导入顺序决定代表图），然后回查每帧的 `burstIdentifier` / `representsBurst`。
4. 看日志，不要只看相册：日志会写“已识别为连拍”还是“已导入但未被识别为连拍”。再到相册的“媒体类型 > 连拍”里核对；打开连拍点“选出最佳”，可以改代表图。

## 不用相册也能先验文件

`Tools/verify_burst.py` 只用 Python 标准库，不依赖 exiftool：

```sh
python3 Tools/verify_burst.py 输出文件夹/Burst_01 原图文件夹
```

它检查三件事：同组 BurstUUID 相同、每帧 ImageUniqueID 各不相同、转换后 SOS 扫描数据与原图完全相同。

**这只证明文件写对了，不证明相册一定成组。** 最终结论以程序里“导入并验证”的 PhotoKit 回查为准。

## 如果没成组

先不要删输出文件夹，按顺序排查：

1. 确认导入的是**转换后的输出文件夹**，不是原图；
2. 确认日志里到底是“未被识别”还是导入本身失败；
3. 兜底浏览方式：在相册里为该组建一个同名相簿（例如 `Burst_01`），把这组照片放进去——浏览体验接近成组，但它不是系统连拍，没有“选出最佳 / 只保留选中帧”的连拍操作；
4. 保留 `BurstManifest.json`：换一台 Mac / 换一个相册版本后，可以拿同一批输出文件重新导入验证，不用重新转换。

## 限制（v1，写在前面）

- 只处理 JPEG（`.jpg` / `.jpeg`）。荣耀连拍如果存成 HEIC 或其他格式，程序会跳过并在日志里写明原因，不会假装转了。
- 视频、其他品牌照片、实况照片互转是后续功能，v1 没有实现，不要按已实现来用。
- 复杂缩略图（带 StripOffsets 的 IFD1）重建时会丢弃缩略图 IFD，主图不受影响，相册会重新生成缩略图。
- 不写、不改相册的 SQLite 数据库，不改 iCloud 状态。
- 关键帧选择只影响导入顺序和 manifest 记录，代表图的最终决定权在相册里。

## 文件清单

```text
MomentPort.xcodeproj/project.pbxproj   Xcode 工程（macOS 12、Swift 5）
MomentPort/
  App.swift                 SwiftUI 入口
  ContentView.swift         四步界面：选文件夹 → 分组 → 转换 → 导入验证
  Models.swift              PhotoFile / BurstGroup / Manifest / 验证结果
  BurstGrouper.swift        子文件夹优先 + 时间间隔分组，读 EXIF 拍摄时间
  JpegExif.swift            JPEG 段级改写 + TIFF 解析/重建（像素不动）
  AppleMakerNote.swift      Apple MakerNote：BurstUUID / ImageUniqueID
  MomentPortEngine.swift    转换引擎：复制、写标识、写完读回校验、写 manifest
  PhotoImporter.swift       PhotoKit 导入 + burstIdentifier 回查
  Info.plist                照片权限说明等
Tools/
  verify_burst.py           不依赖 exiftool 的文件层校验脚本
```

## 本地验证记录（截至交付时）

这台开发机是 Linux，没有 Swift / Xcode，所以**没有在 macOS 12 上编译，也没有在真机相册上导入验证**——这一步必须在你的 Mac 上做，README 没有把它说成已完成。

已在 Linux 上用与 Swift 代码相同布局算法的 Python 镜像做过合成 JPEG 验证：

- 原图：HONOR、Magic6 Pro、含 GPS、含长字符串 EXIF、含 JPEG 缩略图、无 BurstUUID；
- 转换后：`verify_burst.py` 读回 BurstUUID 与送入值一致，ImageUniqueID 唯一；
- SOS 扫描数据与原图完全相同（像素未重编码）；
- Make / Model / DateTimeOriginal 在转换后 TIFF 里原样保留。

也就是说：文件层逻辑已验证，**相册层成组与否仍待你在 macOS 12 + 你的相册版本上用程序里的“导入并验证”确认**。
