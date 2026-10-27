//! MomentPort v1.1 Rust 核心（仅标准库，无外部 crate）。
//!
//! 与 Swift 版行为对齐的要点（先读 `MomentPort/*.swift` 再改这里）：
//! - JPEG 只改 EXIF 段，像素 SOS 原样复制，不重新编码；
//! - Apple MakerNote 是类 IFD 结构：UInt16 计数 + 12 字节条目 + UInt32 next=0，
//!   字符串偏移相对 MakerNote 起点，ASCII 含结尾 NUL，字节序沿用原 TIFF；
//!   写 BurstUUID（tag 0x000b，同组同一大写连字符 UUID）、每帧不同
//!   ImageUniqueID（tag 0x0015）、MakerNote 版本 15（tag 0x0001）；
//! - 分组：子文件夹优先（每子文件夹 ≥2 张为一组）、平铺文件夹按拍摄时间
//!   排序、间隔 > 阈值（默认 2.0 秒）切分；单张不是连拍、不写 BurstUUID；
//! - 输出为 Burst_01/、Burst_02/…并保留原文件名、写 BurstManifest.json；
//! - 转换后回读校验 BurstUUID / ImageUniqueID 再计成功；
//! - 原图文件夹原封不动；绝不写/改相册 SQLite；绝不把 Make / Model 改成 Apple；
//! - 诚实底线：苹果没有公开“写 BurstUUID 就一定成组”的保证，PhotoKit
//!   没有 burstIdentifier setter；绝不把“已导入”说成“已成组”。
//!
//! macOS 专属能力不在本核心里：读 EXIF 时间可用 ImageIO、导入相册并
//! 回查 burstIdentifier 必须走 PhotoKit，见 `macos/BRIDGE.md`。

pub mod convert;
pub mod group;
pub mod jpeg;
pub mod json;
pub mod makernote;
pub mod manifest;
pub mod scan;
pub mod tiff;
pub mod timeutil;
pub mod uuid;

pub use convert::{convert_all, ConvertOutcome, GroupResult};
pub use group::{group_photos, BurstGroup, Grouping};
pub use scan::{pair_for_photo, scan_dir, LivePair, PhotoFile, ScanResult, UnprocessedFile};
