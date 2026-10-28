# Apple 连拍兼容修复验证（2026-10-28）

已修复原生 MakerNote 布局，并完成系统 ImageIO 读取、像素与元数据回归及完整 App 构建。**用户已于 2026-10-28 确认 Apple Photos 实际验证通过，完成本次验收**。该结论来自用户实际操作确认，限于本次测试环境与文件；未获得全部系统版本、设备或导入途径的覆盖证据，也不将文件层或 ImageIO 校验冒充 PhotoKit 实测。

## 原生样片证据与根因

只读检查 `样片/apple/IMG_3672.JPG` 至 `IMG_3675.JPG`：4 张实际 JPEG，EXIF 外层为 MM，Make/Model 为 Apple / iPhone 12。四帧共享同一 BurstUUID，逐帧 PhotoIdentifier 不同。原生 MakerNote 1551–1563 字节，包含 46–47 项。

| 项目 | 原生样片 | 旧输出 | 修复后输出 |
| --- | --- | --- | --- |
| MakerNote 头 | `Apple iOS\0\0\x01MM`，14 字节 | 缺失 | 与样片相同 |
| IFD 起点 | MakerNote +14 | +0 | +14 |
| 内部字节序 | 头部声明 MM | 盲从外层 TIFF | 独立 MM |
| 外置值偏移 | 相对 MakerNote 首字节 | 相对无头 IFD | 相对完整 MakerNote 首字节 |
| `0x0001` | 类型 9，count 1，值 17 | 类型 4，值 15 | 类型 9，count 1，值 17 |
| `0x000b` | ASCII，count 37（含 NUL），组共享 | 自检可读，系统不可读 | 同类型/长度/组共享 |
| 每帧 ID | `0x0020` 与 `0x002b` 各自逐帧不同；无 `0x0015` | 自建 `0x0015` | 写 `0x002b PhotoIdentifier` |
| 系统 ImageIO MakerApple | 可读取 1/11/43 | 字典不存在 | 可读取 1/11/43 |

**已证实的失败原因是旧输出不符合系统可解析的 Apple MakerNote 布局**。旧 Rust 读回及 Python 校验器均使用相同的错误假设，形成自洽的错误验证。系统 ImageIO 对旧输出读不到 MakerApple、对原生和修复输出可读，构成独立对照。不能仅凭这一修复推断 Photos 导入器必然成组。

逐段检查：每张原生照片只有一个 EXIF APP1，未发现 XMP APP1；另有 ICC_PROFILE APP2（550 字节负载）、AROT APP10（764 字节负载）、DQT/DRI/SOF0/DHT 和像素扫描，EOI 后无附加数据。不把 ICC、AROT 或未知 MakerNote 字段解释成连拍组、帧序号或封面标志，也不复制这些设备特有数据到荣耀照片。现有转换器保留非 EXIF 段和全部 SOS 后字节。

这 4 帧不能确定原生封面标志、帧序号字段或完整选片语义。没有猜写未知字段。荣耀封面/序号继续来自已验证文件名，关键帧选择只影响 manifest 与导入顺序；系统代表图仍需相册确认。样片的 `0x0014=1` 也未被当作连拍类型抄写。

ExifTool 的[MakerNotes 实现](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/MakerNotes.pm)独立确认 Apple 签名、+14 的 IFD 和 note-relative 偏移；[Apple tag 实现](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Apple.pm)确认 signed MakerNoteVersion、BurstUUID、PhotoIdentifier 与 ImageCaptureRequestID 的名称/类型。本机未安装 ExifTool，未声称运行过它；实际交叉验证使用原始字节解析及系统 ImageIO。所有样片都只在本机读取，没有上传。

## 实现

- Rust MakerNote 写入/读取改用上述布局；无头旧格式不再通过自检，读取可接受头部独立声明的 MM/II。新增 PhotoIdentifier 对应原生 `0x002b`，manifest 的 `imageUniqueID` 键保留以兼容现有 Swift JSON。
- Python 文件校验器同步拒绝旧布局；找不到原图时明确失败，不跳过像素验证。
- PhotoImporter 仍使用公开的 `PHAssetCreationRequest.addResource`；不设置只读的 burstIdentifier，不调用私有导入接口，不改图库数据库。设置 `includeAllBurstAssets=true`；只有每帧均有同一非空 ID、数量完整、恰一代表图且 burst 查询包含所有导入资产，才确认成组。修正界面仅凭 `representsBurst` 显示成功的问题。
- PhotoKit 的 [`burstIdentifier`](https://developer.apple.com/documentation/photos/phasset/burstidentifier) 为只读；文件标记的可读性不构成任意分组写入的公开保证。App 按钮访问系统图库，界面明确提示与 Photos 中其他打开图库的区别。

## 回归验证与复现

```sh
MACOSX_DEPLOYMENT_TARGET=12.0 cargo test --manifest-path rust/Cargo.toml
MACOSX_DEPLOYMENT_TARGET=12.0 cargo test --manifest-path rust/Cargo.toml --test native_samples -- --ignored
sh scripts/build-app.sh
python3 Tools/regress_private_samples.py MomentPort.app/Contents/Resources/momentport-cli
swiftc -module-cache-path /tmp/momentport-swift-module-cache Tools/verify_imageio.swift -o /tmp/verify-imageio
/tmp/verify-imageio --native 样片/apple
# <本轮输出> 由 regress_private_samples.py 最后一行打印：
/tmp/verify-imageio 样片/Honor <本轮输出>/all-46
/tmp/verify-imageio 样片/Honor <本轮输出>/no-cover-43
/tmp/verify-imageio <本轮输出>/byte-order-input <本轮输出>/byte-order-4
swiftc -module-cache-path /tmp/momentport-swift-module-cache MomentPort/Models.swift Tools/verify_import_facts.swift -o /tmp/verify-import-facts
/tmp/verify-import-facts
```

真实样片测试是显式 opt-in，不把个人照片字节嵌入代码、测试包或 App。仓库不提供样片；默认常规测试跳过这两项。可用 `MOMENTPORT_PRIVATE_SAMPLES` 指定含 `apple/` 与 `Honor/` 的本机目录，缺少样片时不要运行 `--ignored` 或私有回归脚本。提交前另将 Rust 源码复制到不含 `样片/` 的临时目录重新运行常规测试：28 项通过，私有样片 2 项明确 ignored；另单独运行真实样片 2 项通过；导入判定 9 个用例包含部分 ID 缺失、缺帧、多 ID、无代表图、多代表图、单帧/空组等拒绝情况。

真实荣耀仍为 46 帧、5 组（3/25/4/7/7）；排除封面为 43 帧（3/24/3/6/7），默认关键帧正确。完整转换无跳过，SOS 原样；系统 ImageIO 核对 MakerApple 1/11/43、组 ID 共享、逐帧 ID 唯一，并逐帧比较 TIFF、EXIF、GPS、方向、尺寸、色彩描述和解码像素。另用实际 JPEG 像素构造外层 II/MM 各两帧输入，均可被 ImageIO 读取正确字段且像素不变。旧输出作为负例被同一系统校验明确拒绝。

本轮观察到外部将荣耀第二个 7 帧组从目录 4 调整到目录 5；本会话没有移动、修改或恢复样片。50 张原始 JPEG 的文件名/内容 SHA-256 与最初快照一致（目录位置不作为内容一致的依据）；按调整后的稳定目录再次运行的原始文件快照前后完全一致。测试兼容当前样片实际目录，未写死目录数。

## 实际相册验收及少量复验

用户最终确认本次 Apple Photos 实际验证通过。以下保留自动化验证边界和可选手工复验步骤，不表示用户验收仍未完成。

自动化轮次未建立能够验证隔离目标的图库自动化路径；受限环境的 System Events 只读探测返回 -10822。公开 PhotoKit shared 入口不能选择任意辅助图库，因此没有调用它进行自动导入，也没有访问、导入、删除或改动个人系统图库。没有通过私有 API 或直接读写相册数据库来代替验证。

可按 Apple 官方的[新建辅助图库流程](https://support.apple.com/guide/photos/create-additional-photo-libraries-pht6d60b524/mac)手工复验：按住 Option 启动 Photos，新建一个测试图库，保持它为非系统图库、不启用 iCloud；先手工导入原生 4 帧作阳性对照，确认出现在“连拍”中；再分别导入下方 6 帧测试文件中两个 Burst 文件夹里的 JPEG，观察是否各为 3 帧连拍。不要在辅助图库测试时点 App 的 PhotoKit 导入按钮，它会访问系统图库。

若原生对照也不成组，应先检查导出或导入途径；若原生成组而新版输出未成组，说明文件解析修复还不足以满足该 Photos 导入器，后续需要继续保留这组对照研究。此时应按该环境的失败结果继续排查；本次用户验收不能保证其他环境的结果。

当前 App 为 macOS 12+、x86_64、ad-hoc 签名，未公证。最终产物与本轮验证输出链接在会话交付说明中。

## 本次交付与记录边界

源码、测试、文档和构建脚本提交到仓库；样片、App、压缩包、日志及结构化测试输出仅留本地，均不提交。旧输出需要使用修复后的版本重新转换。

前序自动化回归报告中的 `photosGroupingVerified: false` 仅说明该脚本未执行 Photos 导入；不回写历史输出，也不以该字段否定后续用户确认的实际验收。完整 App 及内置 CLI 已通过构建和严格签名校验，并完成启动冒烟；用户最终确认 Photos 验证通过。
