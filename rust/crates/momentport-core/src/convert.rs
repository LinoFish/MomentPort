//! 转换引擎：只做三件事——复制、写 BurstUUID、写清单。
//! 与 Swift 版 `MomentPortEngine` 对齐，并按任务要求扩展：
//! - 不重新编码像素，不改 Make / Model，不改拍摄时间与 GPS；
//! - 不删除原图（输出到另一个文件夹，原文件夹原封不动）；
//! - 转换后回读校验 BurstUUID / PhotoIdentifier 再计成功，否则记入 skipped；
//! - 同组内重名加帧序号后缀；
//! - live 配对视频原样拷到同一输出文件夹、保持同名，不写 BurstUUID，
//!   不声称已转成苹果实况；
//! - 单张默认只记录不写 BurstUUID；`--copy-singles` 时原样拷到 Singles/。

use crate::group::Grouping;
use crate::jpeg;
use crate::manifest::{
    burst_manifest_json, report_json, singles_manifest_json, FrameInfo, LivePairInfo,
    ReportGroup, SkippedInfo,
};
use crate::scan::{pair_for_photo, LivePair, UnprocessedFile};
use crate::uuid::new_uuid_upper;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GroupResult {
    pub folder_name: String,
    pub burst_uuid: String,
    pub output_files: Vec<PathBuf>,
    pub skipped: Vec<SkippedInfo>,
}

#[derive(Debug, Default)]
pub struct ConvertOutcome {
    pub groups: Vec<GroupResult>,
    pub singles_copied: bool,
    pub report_path: PathBuf,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入失败：{}", e))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("写入失败：{}", e))?;
    Ok(())
}

fn unique_dest(folder: &Path, file_name: &str, frame_index: usize) -> PathBuf {
    let dest = folder.join(file_name);
    if !dest.exists() {
        return dest;
    }
    // 同组内重名：加帧序号后缀（与 Swift 版一致：stem_02.ext）。
    let (stem, ext) = match file_name.rfind('.') {
        Some(i) if i > 0 => (&file_name[..i], &file_name[i + 1..]),
        _ => (file_name, ""),
    };
    if ext.is_empty() {
        folder.join(format!("{}_{:02}", stem, frame_index + 1))
    } else {
        folder.join(format!("{}_{:02}.{}", stem, frame_index + 1, ext))
    }
}

/// 执行转换。`output` 不存在时创建；原图文件夹只读，绝不写入。
pub fn convert_all(
    output: &Path,
    grouping: &Grouping,
    live_pairs: &[LivePair],
    unprocessed: &[UnprocessedFile],
    copy_singles: bool,
) -> Result<ConvertOutcome, String> {
    std::fs::create_dir_all(output).map_err(|e| format!("创建输出文件夹失败：{}", e))?;
    let mut outcome = ConvertOutcome::default();
    let mut report_groups: Vec<ReportGroup> = Vec::new();
    let mut report_live: Vec<LivePairInfo> = Vec::new();

    for (index, group) in grouping.groups.iter().enumerate() {
        let folder_name = format!("Burst_{:02}", index + 1);
        let folder = output.join(&folder_name);
        std::fs::create_dir_all(&folder).map_err(|e| format!("创建输出文件夹失败：{}", e))?;
        let mut outputs: Vec<PathBuf> = Vec::new();
        let mut frames: Vec<FrameInfo> = Vec::new();
        let mut skipped: Vec<SkippedInfo> = Vec::new();
        let mut group_live: Vec<LivePairInfo> = Vec::new();
        let mut copied_videos: Vec<String> = Vec::new();

        for (frame_index, photo) in group.photos.iter().enumerate() {
            let data = match std::fs::read(&photo.path) {
                Ok(d) => d,
                Err(e) => {
                    skipped.push(SkippedInfo {
                        file_name: photo.file_name.clone(),
                        reason: format!("读取失败：{}", e),
                    });
                    continue;
                }
            };
            let image_unique_id = new_uuid_upper();
            let converted = match jpeg::convert(&data, &group.burst_uuid, &image_unique_id) {
                Ok(c) => c,
                Err(e) => {
                    skipped.push(SkippedInfo {
                        file_name: photo.file_name.clone(),
                        reason: format!("不是可改写的 JPEG：{}", e),
                    });
                    continue;
                }
            };
            // 校验：刚写完立刻读回，BurstUUID 必须与本组一致，否则不算成功。
            let (back_burst, back_unique) = jpeg::read_burst_info(&converted);
            if back_burst.as_deref() != Some(group.burst_uuid.as_str())
                || back_unique.as_deref() != Some(image_unique_id.as_str())
            {
                skipped.push(SkippedInfo {
                    file_name: photo.file_name.clone(),
                    reason: "写完后读回校验失败，已跳过该帧".to_string(),
                });
                continue;
            }
            let dest = unique_dest(&folder, &photo.file_name, frame_index);
            if let Err(e) = write_atomic(&dest, &converted) {
                skipped.push(SkippedInfo {
                    file_name: photo.file_name.clone(),
                    reason: e,
                });
                continue;
            }
            let dest_name = dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| photo.file_name.clone());
            outputs.push(dest);
            frames.push(FrameInfo {
                file_name: dest_name,
                source_file_name: photo.file_name.clone(),
                capture_ms: photo.capture_ms,
                image_unique_id,
                is_key_frame: frame_index == group.key_index,
            });
            // live 配对视频：原样拷到同一文件夹、保持同名，不写 BurstUUID。
            if let Some(pair) = pair_for_photo(live_pairs, photo) {
                if !copied_videos.contains(&pair.video_file_name) {
                    let vdest = folder.join(&pair.video_file_name);
                    match std::fs::copy(&pair.video_path, &vdest) {
                        Ok(_) => {
                            copied_videos.push(pair.video_file_name.clone());
                            group_live.push(LivePairInfo {
                                photo_file_name: photo.file_name.clone(),
                                video_file_name: pair.video_file_name.clone(),
                                video_copied: true,
                                note: "live 配对视频原样复制到同一文件夹，未写 BurstUUID，未转成苹果实况".to_string(),
                            });
                        }
                        Err(e) => {
                            group_live.push(LivePairInfo {
                                photo_file_name: photo.file_name.clone(),
                                video_file_name: pair.video_file_name.clone(),
                                video_copied: false,
                                note: format!("live 配对视频复制失败：{}", e),
                            });
                        }
                    }
                }
            }
        }
        let manifest = burst_manifest_json(group, &frames, &group_live, &skipped);
        write_atomic(&folder.join("BurstManifest.json"), manifest.as_bytes())?;
        report_live.extend(group_live);
        report_groups.push(ReportGroup {
            folder_name: folder_name.clone(),
            burst_uuid: group.burst_uuid.clone(),
            frame_count: frames.len(),
            key_frame_file_name: group
                .key_photo()
                .map(|p| p.file_name.clone())
                .unwrap_or_default(),
            source_description: group.source_description.clone(),
        });
        outcome.groups.push(GroupResult {
            folder_name,
            burst_uuid: group.burst_uuid.clone(),
            output_files: outputs,
            skipped,
        });
    }

    // 单张：默认只记录；--copy-singles 时原样拷到 Singles/（不写 BurstUUID）。
    let singles_info: Vec<(String, String, Option<LivePair>)> = grouping
        .singles
        .iter()
        .map(|p| {
            (
                p.file_name.clone(),
                p.dir_key.clone(),
                pair_for_photo(live_pairs, p).cloned(),
            )
        })
        .collect();
    if copy_singles && !grouping.singles.is_empty() {
        let singles_dir = output.join("Singles");
        std::fs::create_dir_all(&singles_dir).map_err(|e| format!("创建输出文件夹失败：{}", e))?;
        for photo in &grouping.singles {
            let dest = unique_dest(&singles_dir, &photo.file_name, 0);
            if std::fs::copy(&photo.path, &dest).is_ok() {
                if let Some(pair) = pair_for_photo(live_pairs, photo) {
                    let _ = std::fs::copy(&pair.video_path, singles_dir.join(&pair.video_file_name));
                    report_live.push(LivePairInfo {
                        photo_file_name: photo.file_name.clone(),
                        video_file_name: pair.video_file_name.clone(),
                        video_copied: true,
                        note: "单张的 live 配对视频随单张原样复制到 Singles/，未写 BurstUUID，未转成苹果实况".to_string(),
                    });
                }
            }
        }
        write_atomic(
            &singles_dir.join("SinglesManifest.json"),
            singles_manifest_json(&singles_info, true).as_bytes(),
        )?;
        outcome.singles_copied = true;
    } else if !grouping.singles.is_empty() {
        // 未复制时，live 配对仍要在报告里出现（未复制状态）。
        for photo in &grouping.singles {
            if let Some(pair) = pair_for_photo(live_pairs, photo) {
                if !report_live.iter().any(|r| {
                    r.photo_file_name == photo.file_name && r.video_file_name == pair.video_file_name
                }) {
                    report_live.push(LivePairInfo {
                        photo_file_name: photo.file_name.clone(),
                        video_file_name: pair.video_file_name.clone(),
                        video_copied: false,
                        note: "单张未复制（未选 --copy-singles），live 配对视频未复制，未写 BurstUUID".to_string(),
                    });
                }
            }
        }
    }

    let report = report_json(
        &report_groups,
        &singles_info,
        outcome.singles_copied,
        &report_live,
        unprocessed,
    );
    let report_path = output.join("MomentPortReport.json");
    write_atomic(&report_path, report.as_bytes())?;
    outcome.report_path = report_path;
    Ok(outcome)
}
