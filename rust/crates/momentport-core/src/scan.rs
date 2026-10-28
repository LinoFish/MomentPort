//! 输入扫描。口径与 Swift 版 `BurstGrouper.scan` 对齐，并按任务要求扩展：
//! - 只读顶层与一层子文件夹（避免把整个图库误扫进来）；
//! - 大写扩展名（.JPG）与小写同等处理；
//! - 隐藏文件（`.` 开头）跳过并记原因；
//! - 缺 EXIF 时间用文件修改时间兜底（`has_exif_date == false` 时即为兜底）；
//! - HEIC / 视频 / 其他格式不静默丢弃，记入 `unprocessed` 并写清原因；
//! - 同名 `.jpg` + `.mov`（同目录、同词干，扩展名大小写不敏感）标为 live 配对。

use crate::honor::{self, HonorBurst};
use crate::jpeg::exif_capture_time;
use crate::timeutil::system_time_ms;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PhotoFile {
    pub path: PathBuf,
    pub file_name: String,
    /// 所在子文件夹名；顶层文件为 ""（与 Swift 版桶 key 一致）。
    pub dir_key: String,
    /// 拍摄时间 epoch 毫秒（EXIF 时间或文件修改时间兜底）。
    pub capture_ms: i64,
    pub has_exif_date: bool,
    /// 仅样片证实的命名标识；无可验证专用元数据时不推测私有 tag。
    pub honor_burst: Option<HonorBurst>,
}

impl PhotoFile {
    /// 去扩展名的词干（小写），用于 live 配对匹配。
    pub fn stem_lower(&self) -> String {
        let name = &self.file_name;
        match name.rfind('.') {
            Some(i) if i > 0 => name[..i].to_lowercase(),
            _ => name.to_lowercase(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VideoFile {
    pub path: PathBuf,
    pub file_name: String,
    pub dir_key: String,
}

impl VideoFile {
    pub fn stem_lower(&self) -> String {
        let name = &self.file_name;
        match name.rfind('.') {
            Some(i) if i > 0 => name[..i].to_lowercase(),
            _ => name.to_lowercase(),
        }
    }
}

/// 一对 live 配对：照片 + 同名视频（视频不写 BurstUUID，只同文件夹复制）。
#[derive(Debug, Clone)]
pub struct LivePair {
    pub photo_file_name: String,
    pub photo_dir_key: String,
    pub video_file_name: String,
    pub video_path: PathBuf,
}

/// 未处理文件：原因必须写清，绝不静默丢弃。
#[derive(Debug, Clone)]
pub struct UnprocessedFile {
    pub file_name: String,
    pub dir_key: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub photos: Vec<PhotoFile>,
    pub live_pairs: Vec<LivePair>,
    pub unprocessed: Vec<UnprocessedFile>,
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn extension_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

fn unsupported_reason(ext: &str) -> String {
    match ext {
        "heic" | "heif" => {
            "未处理：HEIC/HEIF 不在本版转换范围（未写 BurstUUID），需后续版本支持".to_string()
        }
        "mov" | "mp4" | "m4v" => {
            "未处理：视频连拍/实况转换不在本版范围（未写 BurstUUID）；若与同名照片配对则按 live 配对原样复制".to_string()
        }
        "" => "未处理：无扩展名，无法识别为 JPEG".to_string(),
        other => format!("未处理：不支持的格式 .{}（本版只转换 JPEG）", other),
    }
}

fn capture_ms_for(path: &Path, bytes: &[u8]) -> (i64, bool) {
    let (ms, has) = exif_capture_time(bytes);
    if let Some(ms) = ms {
        return (ms, has);
    }
    // 兜底：文件修改时间；读不到则用当前时间（与 Swift 版 fallback 思路一致，
    // 但会如实记录 has_exif_date=false；调用方日志可据此区分）。
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(system_time_ms)
        .unwrap_or_else(|_| system_time_ms(std::time::SystemTime::now()));
    (mtime, false)
}

fn process_file(path: &Path, file_name: &str, dir_key: &str, out: &mut ScanResult, videos: &mut Vec<VideoFile>) {
    let ext = extension_lower(file_name);
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let bytes = std::fs::read(path).unwrap_or_default();
            let (capture_ms, has_exif_date) = capture_ms_for(path, &bytes);
            let make = crate::jpeg::exif_make(&bytes);
            out.photos.push(PhotoFile {
                path: path.to_path_buf(),
                file_name: file_name.to_string(),
                dir_key: dir_key.to_string(),
                capture_ms,
                has_exif_date,
                honor_burst: honor::from_filename(file_name, make.as_deref()),
            });
        }
        "mov" | "mp4" | "m4v" => {
            videos.push(VideoFile {
                path: path.to_path_buf(),
                file_name: file_name.to_string(),
                dir_key: dir_key.to_string(),
            });
        }
        _ => {
            out.unprocessed.push(UnprocessedFile {
                file_name: file_name.to_string(),
                dir_key: dir_key.to_string(),
                reason: unsupported_reason(&ext),
            });
        }
    }
}

/// 扫描输入文件夹（顶层 + 一层子文件夹）。返回已按（拍摄时间，文件名）排好序的照片。
pub fn scan_dir(input: &Path) -> Result<ScanResult, String> {
    if !input.is_dir() {
        return Err(format!("输入文件夹不存在或不是文件夹：{}", input.display()));
    }
    let mut result = ScanResult::default();
    let mut videos: Vec<VideoFile> = Vec::new();

    let mut top_entries: Vec<PathBuf> = Vec::new();
    let rd = std::fs::read_dir(input).map_err(|e| format!("读取输入文件夹失败：{}", e))?;
    for e in rd.flatten() {
        top_entries.push(e.path());
    }
    top_entries.sort();

    for entry in top_entries {
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_hidden(&name) {
            result.unprocessed.push(UnprocessedFile {
                file_name: name,
                dir_key: String::new(),
                reason: "跳过：隐藏文件（. 开头），未处理".to_string(),
            });
            continue;
        }
        if entry.is_dir() {
            let dir_key = name.clone();
            let mut sub_entries: Vec<PathBuf> = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&entry) {
                for e in rd.flatten() {
                    sub_entries.push(e.path());
                }
            }
            sub_entries.sort();
            for sub in sub_entries {
                let sub_name = sub
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if is_hidden(&sub_name) {
                    result.unprocessed.push(UnprocessedFile {
                        file_name: sub_name,
                        dir_key: dir_key.clone(),
                        reason: "跳过：隐藏文件（. 开头），未处理".to_string(),
                    });
                    continue;
                }
                if sub.is_dir() {
                    result.unprocessed.push(UnprocessedFile {
                        file_name: sub_name,
                        dir_key: dir_key.clone(),
                        reason: "跳过：超出扫描深度（只读顶层与一层子文件夹），未处理".to_string(),
                    });
                    continue;
                }
                process_file(&sub, &sub_name, &dir_key, &mut result, &mut videos);
            }
        } else if entry.is_file() {
            process_file(&entry, &name, "", &mut result, &mut videos);
        }
    }

    // Live 配对：同目录、同词干的照片与视频配对；配对的视频不再算未处理。
    let mut paired_video_idx: Vec<usize> = Vec::new();
    for photo in &result.photos {
        for (i, v) in videos.iter().enumerate() {
            if paired_video_idx.contains(&i) {
                continue;
            }
            if v.dir_key == photo.dir_key && v.stem_lower() == photo.stem_lower() {
                result.live_pairs.push(LivePair {
                    photo_file_name: photo.file_name.clone(),
                    photo_dir_key: photo.dir_key.clone(),
                    video_file_name: v.file_name.clone(),
                    video_path: v.path.clone(),
                });
                paired_video_idx.push(i);
                break;
            }
        }
    }
    for (i, v) in videos.iter().enumerate() {
        if !paired_video_idx.contains(&i) {
            result.unprocessed.push(UnprocessedFile {
                file_name: v.file_name.clone(),
                dir_key: v.dir_key.clone(),
                reason: "未处理：视频连拍/实况转换不在本版范围（未写 BurstUUID）".to_string(),
            });
        }
    }

    result.photos.sort_by(|a, b| {
        a.capture_ms
            .cmp(&b.capture_ms)
            .then_with(|| a.file_name.cmp(&b.file_name))
            .then_with(|| a.path.cmp(&b.path))
    });
    result.live_pairs.sort_by(|a, b| {
        a.photo_dir_key
            .cmp(&b.photo_dir_key)
            .then_with(|| a.photo_file_name.cmp(&b.photo_file_name))
    });
    result.unprocessed.sort_by(|a, b| {
        a.dir_key
            .cmp(&b.dir_key)
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    Ok(result)
}

/// 查找某张照片的 live 配对（有则返回视频文件名与路径）。
pub fn pair_for_photo<'a>(pairs: &'a [LivePair], photo: &PhotoFile) -> Option<&'a LivePair> {
    pairs.iter().find(|p| {
        p.photo_dir_key == photo.dir_key && p.photo_file_name == photo.file_name
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    #[test]
    fn scan_handles_uppercase_hidden_and_live_pair() {
        let base = std::env::temp_dir().join(format!(
            "momentport-scan-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        // 最小 JPEG（无 EXIF，靠 mtime 兜底）：SOI + SOF0 + SOS + EOI。
        let jpg = vec![
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x02, 0x00, 0x03, 0x01, 0x01,
            0x11, 0x00, 0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00, 0x11,
            0xff, 0xd9,
        ];
        write_file(&base.join("A.JPG"), &jpg);
        write_file(&base.join("A.MOV"), b"fake mov");
        write_file(&base.join(".hidden.jpg"), &jpg);
        write_file(&base.join("note.heic"), b"fake heic");
        write_file(&base.join("clip.mov"), b"unpaired mov");
        let r = scan_dir(&base).unwrap();
        assert_eq!(r.photos.len(), 1);
        assert!(!r.photos[0].has_exif_date); // 无 EXIF → mtime 兜底
        assert_eq!(r.live_pairs.len(), 1);
        assert_eq!(r.live_pairs[0].video_file_name, "A.MOV");
        // 未处理：隐藏文件 + HEIC + 未配对视频 = 3 条，且都不静默。
        assert_eq!(r.unprocessed.len(), 3);
        assert!(r
            .unprocessed
            .iter()
            .any(|u| u.file_name == ".hidden.jpg"));
        assert!(r.unprocessed.iter().any(|u| u.file_name == "note.heic"));
        assert!(r.unprocessed.iter().any(|u| u.file_name == "clip.mov"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_honor_without_metadata_and_reject_conflicting_make() {
        use crate::tiff::{build_tiff, ByteOrder, ParsedTiff, TiffEntry};
        let base = std::env::temp_dir().join(format!("momentport-honor-scan-{}", crate::uuid::new_uuid_upper()));
        let name = "IMG_20230108_205027_BURST002.JPG";
        // 文件名足够：完全没有 EXIF 时也识别，不依赖 mtime 作为连拍 ID。
        write_file(&base.join("bare").join(name), &[0xff, 0xd8, 0xff, 0xd9]);
        for (dir, make) in [("honor", "HONOR"), ("other", "Apple")] {
            let parsed = ParsedTiff {
                order: ByteOrder::Big,
                ifd0: vec![TiffEntry::ascii(0x010f, make)],
                ..Default::default()
            };
            let mut payload = b"Exif\0\0".to_vec();
            payload.extend(build_tiff(&parsed, b"AF_S\0", None, None));
            let mut jpg = vec![0xff, 0xd8, 0xff, 0xe1];
            jpg.extend(((payload.len() + 2) as u16).to_be_bytes());
            jpg.extend(payload);
            jpg.extend([0xff, 0xd9]);
            write_file(&base.join(dir).join(name), &jpg);
        }
        let scan = scan_dir(&base).unwrap();
        assert_eq!(scan.photos.len(), 3);
        for p in scan.photos {
            assert_eq!(p.honor_burst.is_some(), p.dir_key != "other");
            assert!(!p.has_exif_date);
        }
        std::fs::remove_dir_all(base).unwrap();
    }
}
