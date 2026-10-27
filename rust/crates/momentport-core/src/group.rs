//! 分组。直接移植 Swift 版 `BurstGrouper.group`：
//! 1. 子文件夹优先：每个含 ≥2 张照片的子文件夹算一组；
//! 2. 平铺文件夹：按拍摄时间排序，相邻间隔 ≤ gap 归一组，> gap 切开；
//! 3. 只有 1 张的“组”不算连拍，列为单张，不写 BurstUUID。
//!
//! 调用方传入的 `photos` 应已按（拍摄时间，文件名）排好序；
//! 本函数在桶内再次按同一规则排序，保证结果与输入顺序无关。

use crate::scan::PhotoFile;
use crate::uuid::new_uuid_upper;

#[derive(Debug, Clone)]
pub struct BurstGroup {
    pub photos: Vec<PhotoFile>,
    pub burst_uuid: String,
    pub key_index: usize,
    pub source_description: String,
}

impl BurstGroup {
    /// 关键帧（key_index 越界时钳制到最后一帧，与 Swift 版一致）。
    pub fn key_photo(&self) -> Option<&PhotoFile> {
        if self.photos.is_empty() {
            return None;
        }
        let idx = self.key_index.min(self.photos.len() - 1);
        Some(&self.photos[idx])
    }
}

#[derive(Debug, Default)]
pub struct Grouping {
    pub groups: Vec<BurstGroup>,
    pub singles: Vec<PhotoFile>,
}

fn sorted(mut photos: Vec<PhotoFile>) -> Vec<PhotoFile> {
    photos.sort_by(|a, b| {
        a.capture_ms
            .cmp(&b.capture_ms)
            .then_with(|| a.file_name.cmp(&b.file_name))
            .then_with(|| a.path.cmp(&b.path))
    });
    photos
}

pub fn group_photos(photos: &[PhotoFile], gap_seconds: f64, key_index: usize) -> Grouping {
    // 先按“所在子文件夹”分桶；顶层文件归为 "" 桶；桶顺序 = 桶内首张照片出现顺序。
    let mut buckets: Vec<(String, Vec<PhotoFile>)> = Vec::new();
    for p in photos {
        if let Some(b) = buckets.iter_mut().find(|(k, _)| *k == p.dir_key) {
            b.1.push(p.clone());
        } else {
            buckets.push((p.dir_key.clone(), vec![p.clone()]));
        }
    }
    let mut out = Grouping::default();
    for (key, bucket) in buckets {
        let bucket = sorted(bucket);
        if !key.is_empty() {
            if bucket.len() >= 2 {
                out.groups.push(BurstGroup {
                    photos: bucket,
                    burst_uuid: new_uuid_upper(),
                    key_index,
                    source_description: format!("子文件夹：{}", key),
                });
            } else {
                out.singles.extend(bucket);
            }
            continue;
        }
        // 顶层平铺：按时间间隔切分。
        let gap_ms = gap_seconds * 1000.0;
        let mut current: Vec<PhotoFile> = Vec::new();
        let flush = |current: &mut Vec<PhotoFile>, out: &mut Grouping, gap_seconds: f64| {
            if current.len() >= 2 {
                out.groups.push(BurstGroup {
                    photos: std::mem::take(current),
                    burst_uuid: new_uuid_upper(),
                    key_index,
                    source_description: format!(
                        "按时间自动分组（间隔 ≤ {} 秒）",
                        gap_seconds
                    ),
                });
            } else {
                out.singles.extend(std::mem::take(current));
            }
        };
        for p in bucket {
            if let Some(last) = current.last() {
                if (p.capture_ms - last.capture_ms) as f64 > gap_ms {
                    flush(&mut current, &mut out, gap_seconds);
                }
            }
            current.push(p);
        }
        flush(&mut current, &mut out, gap_seconds);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn photo(name: &str, dir: &str, ms: i64) -> PhotoFile {
        PhotoFile {
            path: PathBuf::from(format!("{}/{}", dir, name)),
            file_name: name.to_string(),
            dir_key: dir.to_string(),
            capture_ms: ms,
            has_exif_date: true,
        }
    }

    #[test]
    fn subfolder_priority_and_flat_split() {
        let photos = vec![
            photo("b1.jpg", "A", 1000),
            photo("b2.jpg", "A", 1500),
            photo("f1.jpg", "", 10000),
            photo("f2.jpg", "", 10500),
            photo("f3.jpg", "", 20000), // 与 f2 间隔 9500ms > 2000ms，切开
            photo("solo.jpg", "", 40000),
        ];
        let g = group_photos(&photos, 2.0, 0);
        assert_eq!(g.groups.len(), 2);
        assert_eq!(g.groups[0].photos.len(), 2); // 子文件夹组
        assert_eq!(g.groups[0].source_description, "子文件夹：A");
        assert_eq!(g.groups[1].photos.len(), 2); // 平铺 f1+f2
        assert_eq!(g.singles.len(), 2); // f3 与 solo 各自单张
        assert!(g.groups.iter().all(|gr| !gr.burst_uuid.is_empty()));
        assert_ne!(g.groups[0].burst_uuid, g.groups[1].burst_uuid);
    }

    #[test]
    fn gap_boundary_is_inclusive() {
        // 间隔恰好 = gap 时不切分（Swift 版是 > gap 才切）。
        let photos = vec![
            photo("a.jpg", "", 0),
            photo("b.jpg", "", 2000),
        ];
        let g = group_photos(&photos, 2.0, 0);
        assert_eq!(g.groups.len(), 1);
        assert_eq!(g.singles.len(), 0);
    }

    #[test]
    fn single_subfolder_is_not_a_burst() {
        let photos = vec![photo("only.jpg", "A", 0)];
        let g = group_photos(&photos, 2.0, 0);
        assert_eq!(g.groups.len(), 0);
        assert_eq!(g.singles.len(), 1);
    }
}
