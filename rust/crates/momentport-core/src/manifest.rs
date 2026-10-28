//! Manifest / 报告 JSON（手动拼接，仅标准库）。
//!
//! `BurstManifest.json` 的字段名与 Swift 版 `BurstManifest` 对齐
//! （burstUUID / frameCount / keyFrameFileName / keyFrameIndex /
//! sourceDescription / frames[{fileName, sourceFileName, captureTimeISO,
//! imageUniqueID, isKeyFrame}]），并在同文件里追加本版扩展字段
//! `livePairs` 与 `skipped`，旧读取方忽略扩展字段即可。
//! 顶层另写 `MomentPortReport.json`，汇总所有组、单张、live 配对与
//! 未处理原因——未处理文件绝不只出现在终端日志里。

use crate::group::BurstGroup;
use crate::json::quoted;
use crate::scan::{LivePair, UnprocessedFile};
use crate::timeutil::format_iso_ms;

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub file_name: String,
    pub source_file_name: String,
    pub capture_ms: i64,
    pub image_unique_id: String,
    pub is_key_frame: bool,
}

#[derive(Debug, Clone)]
pub struct LivePairInfo {
    pub photo_file_name: String,
    pub video_file_name: String,
    pub video_copied: bool,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct SkippedInfo {
    pub file_name: String,
    pub reason: String,
}

pub fn burst_manifest_json(
    group: &BurstGroup,
    frames: &[FrameInfo],
    live_pairs: &[LivePairInfo],
    skipped: &[SkippedInfo],
) -> String {
    let key_name = group
        .key_photo()
        .map(|p| p.file_name.clone())
        .unwrap_or_default();
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"burstUUID\": {},\n", quoted(&group.burst_uuid)));
    s.push_str(&format!("  \"frameCount\": {},\n", frames.len()));
    s.push_str("  \"frames\": [");
    if frames.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, f) in frames.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"captureTimeISO\": {},\n", quoted(&format_iso_ms(f.capture_ms))));
            s.push_str(&format!("      \"fileName\": {},\n", quoted(&f.file_name)));
            s.push_str(&format!("      \"imageUniqueID\": {},\n", quoted(&f.image_unique_id)));
            s.push_str(&format!("      \"isKeyFrame\": {},\n", f.is_key_frame));
            s.push_str(&format!("      \"sourceFileName\": {}\n", quoted(&f.source_file_name)));
            s.push_str(if i + 1 == frames.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
    }
    s.push_str(&format!("  \"keyFrameFileName\": {},\n", quoted(&key_name)));
    s.push_str(&format!("  \"keyFrameIndex\": {},\n", group.key_index));
    s.push_str("  \"livePairs\": [");
    if live_pairs.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, p) in live_pairs.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"note\": {},\n", quoted(&p.note)));
            s.push_str(&format!("      \"photoFileName\": {},\n", quoted(&p.photo_file_name)));
            s.push_str(&format!("      \"videoCopied\": {},\n", p.video_copied));
            s.push_str(&format!("      \"videoFileName\": {}\n", quoted(&p.video_file_name)));
            s.push_str(if i + 1 == live_pairs.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
    }
    s.push_str("  \"skipped\": [");
    if skipped.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, k) in skipped.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"fileName\": {},\n", quoted(&k.file_name)));
            s.push_str(&format!("      \"reason\": {}\n", quoted(&k.reason)));
            s.push_str(if i + 1 == skipped.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
    }
    s.push_str(&format!("  \"sourceDescription\": {}\n", quoted(&group.source_description)));
    s.push_str("}\n");
    s
}

/// 单张清单（`Singles/SinglesManifest.json`）：明确写“未写 BurstUUID”。
pub fn singles_manifest_json(
    singles: &[(String, String, Option<LivePair>)],
    copied: bool,
) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"burstUUID\": null,\n");
    s.push_str(&format!(
        "  \"note\": {},\n",
        quoted("单张不是连拍，未写 BurstUUID；--copy-singles 时仅原样复制，不改 EXIF")
    ));
    s.push_str(&format!("  \"singlesCopied\": {},\n", copied));
    s.push_str("  \"singles\": [");
    if singles.is_empty() {
        s.push_str("]\n");
    } else {
        s.push('\n');
        for (i, (name, dir, pair)) in singles.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"dirKey\": {},\n", quoted(dir)));
            s.push_str(&format!("      \"fileName\": {},\n", quoted(name)));
            match pair {
                Some(p) => s.push_str(&format!(
                    "      \"liveVideoFileName\": {}\n",
                    quoted(&p.video_file_name)
                )),
                None => s.push_str("      \"liveVideoFileName\": null\n"),
            }
            s.push_str(if i + 1 == singles.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ]\n");
    }
    s.push_str("}\n");
    s
}

pub struct ReportGroup {
    pub folder_name: String,
    pub burst_uuid: String,
    pub frame_count: usize,
    pub key_frame_file_name: String,
    pub source_description: String,
}

/// 顶层报告 JSON：组摘要 + 单张 + live 配对 + 未处理原因 + 诚实声明。
pub fn report_json(
    groups: &[ReportGroup],
    singles: &[(String, String, Option<LivePair>)],
    singles_copied: bool,
    live_pairs: &[LivePairInfo],
    unprocessed: &[UnprocessedFile],
) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!(
        "  \"honestyNote\": {},\n",
        quoted("文件层 BurstUUID 不等于相册已成组；苹果没有公开“写 BurstUUID 就一定成组”的保证，PhotoKit 没有 burstIdentifier setter；成组与否必须在 macOS 上导入后用 PhotoKit 回查 burstIdentifier / representsBurst")
    ));
    s.push_str("  \"groups\": [");
    if groups.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, g) in groups.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"burstUUID\": {},\n", quoted(&g.burst_uuid)));
            s.push_str(&format!("      \"folderName\": {},\n", quoted(&g.folder_name)));
            s.push_str(&format!("      \"frameCount\": {},\n", g.frame_count));
            s.push_str(&format!("      \"keyFrameFileName\": {},\n", quoted(&g.key_frame_file_name)));
            s.push_str(&format!("      \"sourceDescription\": {}\n", quoted(&g.source_description)));
            s.push_str(if i + 1 == groups.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
    }
    s.push_str("  \"livePairs\": [");
    if live_pairs.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, p) in live_pairs.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"note\": {},\n", quoted(&p.note)));
            s.push_str(&format!("      \"photoFileName\": {},\n", quoted(&p.photo_file_name)));
            s.push_str(&format!("      \"videoCopied\": {},\n", p.video_copied));
            s.push_str(&format!("      \"videoFileName\": {}\n", quoted(&p.video_file_name)));
            s.push_str(if i + 1 == live_pairs.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
    }
    s.push_str(&format!("  \"singlesCopied\": {},\n", singles_copied));
    s.push_str("  \"singles\": [");
    if singles.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, (name, dir, pair)) in singles.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"dirKey\": {},\n", quoted(dir)));
            s.push_str(&format!("      \"fileName\": {},\n", quoted(name)));
            match pair {
                Some(p) => s.push_str(&format!(
                    "      \"liveVideoFileName\": {},\n",
                    quoted(&p.video_file_name)
                )),
                None => s.push_str("      \"liveVideoFileName\": null,\n"),
            }
            s.push_str("      \"note\": \"单张不是连拍，未写 BurstUUID\"\n");
            s.push_str(if i + 1 == singles.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
    }
    s.push_str("  \"unprocessed\": [");
    if unprocessed.is_empty() {
        s.push_str("]\n");
    } else {
        s.push('\n');
        for (i, u) in unprocessed.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"dirKey\": {},\n", quoted(&u.dir_key)));
            s.push_str(&format!("      \"fileName\": {},\n", quoted(&u.file_name)));
            s.push_str(&format!("      \"reason\": {}\n", quoted(&u.reason)));
            s.push_str(if i + 1 == unprocessed.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ]\n");
    }
    s.push_str("}\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::PhotoFile;
    use std::path::PathBuf;

    #[test]
    fn manifest_json_is_wellformed_text() {
        let group = BurstGroup {
            photos: vec![PhotoFile {
                path: PathBuf::from("a.jpg"),
                file_name: "a.jpg".to_string(),
                dir_key: String::new(),
                capture_ms: 0,
                has_exif_date: true,
                honor_burst: None,
            }],
            burst_uuid: "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA".to_string(),
            key_index: 0,
            source_description: "子文件夹：A".to_string(),
        };
        let json = burst_manifest_json(
            &group,
            &[FrameInfo {
                file_name: "a.jpg".to_string(),
                source_file_name: "a.jpg".to_string(),
                capture_ms: 0,
                image_unique_id: "BBBBBBBB-2222-4222-8222-BBBBBBBBBBBB".to_string(),
                is_key_frame: true,
            }],
            &[],
            &[],
        );
        assert!(json.contains("\"burstUUID\": \"AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA\""));
        assert!(json.contains("\"frameCount\": 1"));
        assert!(json.contains("\"isKeyFrame\": true"));
        assert!(json.trim_end().ends_with('}'));
    }
}
