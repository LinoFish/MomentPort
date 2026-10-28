//! 样片证实的荣耀文件名协议；未发现可用的专用连拍元数据，不推测 MakerNote。
use crate::timeutil::parse_exif_datetime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HonorBurst {
    pub group_id: String,
    pub frame_index: u16,
    pub is_cover: bool,
}

/// 严格匹配 IMG_YYYYMMDD_HHMMSS_BURSTddd[_COVER].JPG/JPEG。
/// Make 缺失时允许仅文件名识别；明确为其他厂商时不采用此规则。
pub fn from_filename(name: &str, make: Option<&str>) -> Option<HonorBurst> {
    if let Some(make) = make.filter(|m| !m.trim().is_empty()) {
        if !make.trim().eq_ignore_ascii_case("HONOR") {
            return None;
        }
    }
    let name = name.to_ascii_uppercase();
    let (stem, ext) = name.rsplit_once('.')?;
    if !matches!(ext, "JPG" | "JPEG") {
        return None;
    }
    let (id, frame) = stem.strip_prefix("IMG_")?.split_once("_BURST")?;
    let b = id.as_bytes();
    if b.len() != 15
        || b[8] != b'_'
        || !b
            .iter()
            .enumerate()
            .all(|(i, b)| i == 8 || b.is_ascii_digit())
    {
        return None;
    }
    let date = format!(
        "{}:{}:{} {}:{}:{}",
        &id[..4],
        &id[4..6],
        &id[6..8],
        &id[9..11],
        &id[11..13],
        &id[13..15]
    );
    parse_exif_datetime(&date)?;
    let (frame, is_cover) = match frame.strip_suffix("_COVER") {
        Some(frame) => (frame, true),
        None => (frame, false),
    };
    if frame.len() != 3 || !frame.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frame_index = frame.parse().ok()?;
    if frame_index == 0 {
        return None;
    }
    Some(HonorBurst {
        group_id: format!("IMG_{}", id),
        frame_index,
        is_cover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_filename_and_make_guard() {
        let burst = from_filename("IMG_20230108_205027_BURST090.JPG", Some("HONOR")).unwrap();
        assert_eq!(burst.frame_index, 90);
        assert!(!burst.is_cover);
        assert!(
            from_filename("img_20230108_205027_burst002_cover.jpeg", None)
                .unwrap()
                .is_cover
        );
        assert!(from_filename("IMG_20230108_205027_BURST002.JPG", Some("Apple")).is_none());
        for name in [
            "IMG_20230108_205027.JPG",
            "holiday_BURST001.JPG",
            "IMG_20230230_205027_BURST001.JPG",
            "IMG_20230108_255027_BURST001.JPG",
            "IMG_20230108_205027_BURST000.JPG",
            "IMG_20230108_205027_BURST01.JPG",
            "IMG_20230108_205027_BURST001_COVER_copy.JPG",
            "IMG_20230108_205027_BURST001.PNG",
            "IMG_20230108_205027_BURST001_COVER.JPG.bak",
            "IMG_中文日期_205027_BURST001.JPG",
        ] {
            assert!(from_filename(name, None).is_none(), "{}", name);
        }
    }
}
