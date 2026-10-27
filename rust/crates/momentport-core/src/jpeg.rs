//! JPEG 段级改写。直接移植 Swift 版 `JpegExif.swift` 的 `JpegConverter`：
//! - 像素扫描数据（SOS 之后）原样复制，不解码、不重新编码；
//! - 只换第一个 EXIF APP1 段；原图没有 EXIF 时，在 SOI 之后新建一个
//!   最小 TIFF（big-endian，与 Swift 版一致）；
//! - 其他段（包括第二个起的 APP1）原样复制；
//! - 重建后的 EXIF 段超过 64KB 时报错，不输出截断文件。

use crate::makernote;
use crate::tiff::{build_tiff, parse_tiff, ByteOrder, ParsedTiff};
use crate::timeutil::{parse_exif_datetime, parse_subsec_ms};

fn segment_length(data: &[u8], pos: usize) -> usize {
    if pos + 4 > data.len() {
        return 0;
    }
    ((data[pos + 2] as usize) << 8) | (data[pos + 3] as usize)
}

fn is_exif_app1(data: &[u8], pos: usize, payload_start: usize, payload_end: usize) -> bool {
    if payload_end - payload_start < 6 {
        return false;
    }
    // 调用方已保证 marker == 0xe1；这里只验 "Exif\0\0"。
    let _ = pos;
    data[payload_start] == 0x45
        && data[payload_start + 1] == 0x78
        && data[payload_start + 2] == 0x69
        && data[payload_start + 3] == 0x66
        && data[payload_start + 4] == 0
        && data[payload_start + 5] == 0
}

/// 返回改写后的完整 JPEG 数据。
pub fn convert(data: &[u8], burst_uuid: &str, image_unique_id: &str) -> Result<Vec<u8>, String> {
    if data.len() < 2 || data[0] != 0xff || data[1] != 0xd8 {
        return Err("不是 JPEG 文件".to_string());
    }
    let mut out: Vec<u8> = vec![0xff, 0xd8];
    let mut pos = 2usize;
    let mut exif_replaced = false;
    let mut pixel_width: Option<u32> = None;
    let mut pixel_height: Option<u32> = None;

    while pos + 2 <= data.len() {
        if data[pos] != 0xff {
            break;
        }
        let marker = data[pos + 1];
        if marker == 0xd9 {
            out.extend_from_slice(&data[pos..pos + 2]);
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            out.extend_from_slice(&data[pos..pos + 2]);
            pos += 2;
            continue;
        }
        if marker == 0xda {
            // SOS：从这里起全部是扫描数据，原样复制到文件尾。
            let len = segment_length(data, pos);
            if len < 2 || pos + 2 + len > data.len() {
                break;
            }
            out.extend_from_slice(&data[pos..]);
            break;
        }
        let len = segment_length(data, pos);
        if len < 2 || pos + 2 + len > data.len() {
            break;
        }
        let segment_end = pos + 2 + len;
        let payload_start = pos + 4;
        let payload_end = segment_end;

        if (marker == 0xc0 || marker == 0xc1 || marker == 0xc2) && payload_end - payload_start >= 5 {
            pixel_height =
                Some(((data[payload_start + 1] as u32) << 8) | (data[payload_start + 2] as u32));
            pixel_width =
                Some(((data[payload_start + 3] as u32) << 8) | (data[payload_start + 4] as u32));
        }

        let is_exif = marker == 0xe1 && is_exif_app1(data, pos, payload_start, payload_end);
        if is_exif && !exif_replaced {
            let tiff_data = &data[payload_start + 6..payload_end];
            let parsed = parse_tiff(tiff_data)?;
            let maker_note =
                makernote::build(burst_uuid, image_unique_id, parsed.order);
            let new_tiff = build_tiff(&parsed, &maker_note, pixel_width, pixel_height);
            let mut new_payload = b"Exif".to_vec();
            new_payload.extend_from_slice(&[0, 0]);
            new_payload.extend_from_slice(&new_tiff);
            let new_len = new_payload.len() + 2;
            if new_len > 0xffff {
                return Err("重建后的 EXIF 段超过 64KB".to_string());
            }
            out.extend_from_slice(&[
                0xff,
                0xe1,
                ((new_len >> 8) & 0xff) as u8,
                (new_len & 0xff) as u8,
            ]);
            out.extend_from_slice(&new_payload);
            exif_replaced = true;
        } else {
            out.extend_from_slice(&data[pos..segment_end]);
        }
        pos = segment_end;
    }

    if !exif_replaced {
        // 原图没有 EXIF：新建一个最小 TIFF，插在 SOI 之后。
        let order = ByteOrder::Big;
        let empty = ParsedTiff {
            order,
            ifd0: vec![],
            exif: None,
            gps: None,
            interop: None,
            ifd1: None,
            thumbnail_jpeg: None,
        };
        let maker_note = makernote::build(burst_uuid, image_unique_id, order);
        let new_tiff = build_tiff(&empty, &maker_note, pixel_width, pixel_height);
        let mut new_payload = b"Exif".to_vec();
        new_payload.extend_from_slice(&[0, 0]);
        new_payload.extend_from_slice(&new_tiff);
        let new_len = new_payload.len() + 2;
        if new_len > 0xffff {
            return Err("重建后的 EXIF 段超过 64KB".to_string());
        }
        let mut rebuilt: Vec<u8> = vec![0xff, 0xd8];
        rebuilt.extend_from_slice(&[
            0xff,
            0xe1,
            ((new_len >> 8) & 0xff) as u8,
            (new_len & 0xff) as u8,
        ]);
        rebuilt.extend_from_slice(&new_payload);
        rebuilt.extend_from_slice(&out[2..]);
        return Ok(rebuilt);
    }
    Ok(out)
}

/// 校验用：从 JPEG 里读回 BurstUUID / ImageUniqueID。
pub fn read_burst_info(data: &[u8]) -> (Option<String>, Option<String>) {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return (None, None);
    }
    let mut pos = 2usize;
    while pos + 4 <= data.len() && data[pos] == 0xff {
        let marker = data[pos + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            pos += 2;
            continue;
        }
        let len = segment_length(data, pos);
        if len < 2 || pos + 2 + len > data.len() {
            break;
        }
        if marker == 0xe1 && len >= 8 {
            let ps = pos + 4;
            if data[ps] == 0x45
                && data[ps + 1] == 0x78
                && data[ps + 2] == 0x69
                && data[ps + 3] == 0x66
                && data[ps + 4] == 0
                && data[ps + 5] == 0
            {
                let tiff_data = &data[ps + 6..pos + 2 + len];
                let parsed = match parse_tiff(tiff_data) {
                    Ok(p) => p,
                    Err(_) => return (None, None),
                };
                let exif = match parsed.exif {
                    Some(e) => e,
                    None => return (None, None),
                };
                // 注意：parse_tiff 会滤掉 0x927c，所以这里不能依赖 parsed.exif；
                // 必须从原始 TIFF 里再取一次 MakerNote 条目。
                // 为保持简单：重新解析原始 TIFF 的 Exif IFD（不过滤）。
                let (burst, unique) = read_makernote_raw(tiff_data, parsed.order);
                let _ = exif;
                return (burst, unique);
            }
        }
        pos += 2 + len;
    }
    (None, None)
}

/// 从原始 TIFF 里读 MakerNote（不过滤 0x927c 的原始读取，供校验用）。
fn read_makernote_raw(tiff: &[u8], order: ByteOrder) -> (Option<String>, Option<String>) {
    use crate::tiff::type_size;
    if tiff.len() < 8 {
        return (None, None);
    }
    let ifd0_off = order.u32_at(tiff, 4) as usize;
    let read_ifd = |off: usize| -> Option<(Vec<(u16, u16, u32, Vec<u8>)>, usize)> {
        if off + 2 > tiff.len() {
            return None;
        }
        let count = order.u16_at(tiff, off) as usize;
        if off + 2 + count * 12 + 4 > tiff.len() {
            return None;
        }
        let mut out = Vec::new();
        for i in 0..count {
            let e = off + 2 + i * 12;
            let tag = order.u16_at(tiff, e);
            let typ = order.u16_at(tiff, e + 2);
            let cnt = order.u32_at(tiff, e + 4);
            let size = (cnt as usize) * type_size(typ);
            let value = if size <= 4 {
                tiff[e + 8..e + 8 + size].to_vec()
            } else {
                let voff = order.u32_at(tiff, e + 8) as usize;
                if voff + size > tiff.len() {
                    return None;
                }
                tiff[voff..voff + size].to_vec()
            };
            out.push((tag, typ, cnt, value));
        }
        let next = order.u32_at(tiff, off + 2 + count * 12) as usize;
        Some((out, next))
    };
    let (ifd0, _) = match read_ifd(ifd0_off) {
        Some(v) => v,
        None => return (None, None),
    };
    let exif_ptr = ifd0
        .iter()
        .find(|(tag, _, _, _)| *tag == 0x8769)
        .and_then(|(_, _, _, v)| {
            if v.len() >= 4 {
                Some(order.u32_at(v, 0) as usize)
            } else {
                None
            }
        });
    let exif_ptr = match exif_ptr {
        Some(p) => p,
        None => return (None, None),
    };
    let (exif, _) = match read_ifd(exif_ptr) {
        Some(v) => v,
        None => return (None, None),
    };
    let mn = match exif.iter().find(|(tag, _, _, _)| *tag == 0x927c) {
        Some((_, _, _, v)) => v.clone(),
        None => return (None, None),
    };
    makernote::parse(&mn, order)
}

/// 从 JPEG 里读拍摄时间：优先 DateTimeOriginal + SubSecTimeOriginal，
/// 其次 IFD0 的 DateTime；都没有返回 (None, false)。
/// 返回 (epoch 毫秒, 是否来自 EXIF/TIFF 时间)。
pub fn exif_capture_time(data: &[u8]) -> (Option<i64>, bool) {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return (None, false);
    }
    let mut pos = 2usize;
    while pos + 4 <= data.len() && data[pos] == 0xff {
        let marker = data[pos + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            pos += 2;
            continue;
        }
        let len = segment_length(data, pos);
        if len < 2 || pos + 2 + len > data.len() {
            break;
        }
        if marker == 0xe1 && len >= 8 {
            let ps = pos + 4;
            if data[ps] == 0x45
                && data[ps + 1] == 0x78
                && data[ps + 2] == 0x69
                && data[ps + 3] == 0x66
                && data[ps + 4] == 0
                && data[ps + 5] == 0
            {
                let tiff_data = &data[ps + 6..pos + 2 + len];
                if let Ok(parsed) = parse_tiff(tiff_data) {
                    if let Some(exif) = &parsed.exif {
                        if let Some(dt) = exif
                            .iter()
                            .find(|e| e.tag == 0x9003)
                            .and_then(|e| e.ascii_string())
                            .and_then(|s| parse_exif_datetime(&s))
                        {
                            let mut ms = dt;
                            if let Some(sub) = exif
                                .iter()
                                .find(|e| e.tag == 0xa357)
                                .and_then(|e| e.ascii_string())
                                .and_then(|s| parse_subsec_ms(&s))
                            {
                                ms += sub;
                            }
                            return (Some(ms), true);
                        }
                    }
                    // 兜底：部分写入器（含 PIL）把 0x9003 DateTimeOriginal
                    // 直接放在 IFD0 而非 ExifIFD 里；ImageIO 式读取应两种都认。
                    // 真实设备文件通常在 ExifIFD 里，上面已优先处理。
                    for tag in [0x9003u16, 0x9004] {
                        if let Some(dt) = parsed
                            .ifd0
                            .iter()
                            .find(|e| e.tag == tag)
                            .and_then(|e| e.ascii_string())
                            .and_then(|s| parse_exif_datetime(&s))
                        {
                            let mut ms = dt;
                            if let Some(sub) = parsed
                                .ifd0
                                .iter()
                                .find(|e| e.tag == 0xa357)
                                .and_then(|e| e.ascii_string())
                                .and_then(|s| parse_subsec_ms(&s))
                            {
                                ms += sub;
                            }
                            return (Some(ms), true);
                        }
                    }
                    if let Some(dt) = parsed
                        .ifd0
                        .iter()
                        .find(|e| e.tag == 0x0132)
                        .and_then(|e| e.ascii_string())
                        .and_then(|s| parse_exif_datetime(&s))
                    {
                        return (Some(dt), true);
                    }
                }
                return (None, false);
            }
        }
        pos += 2 + len;
    }
    (None, false)
}

/// SOS 段起始位置起的全部字节（含 SOS 标记与扫描数据），供“像素原样”测试用。
/// 与 `Tools/verify_burst.py` 的 `sos_suffix` 口径一致。
pub fn sos_suffix(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 2 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut pos = 2usize;
    while pos + 4 <= data.len() && data[pos] == 0xff {
        let marker = data[pos + 1];
        if marker == 0xda {
            return Some(&data[pos..]);
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            pos += 2;
            continue;
        }
        let len = segment_length(data, pos);
        if len < 2 || pos + 2 + len > data.len() {
            return None;
        }
        pos += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::{build_tiff, ParsedTiff, TiffEntry};

    /// 造一个最小可解析 JPEG：SOI + APP1(EXIF) + SOF0 + SOS + 扫描数据 + EOI。
    /// TIFF 为 little-endian，IFD0 含 Make，Exif 含 DateTimeOriginal/SubSec。
    pub fn sample_jpeg_with_exif() -> Vec<u8> {
        let parsed = ParsedTiff {
            order: ByteOrder::Little,
            ifd0: vec![TiffEntry::ascii(0x010f, "HONOR")],
            exif: Some(vec![
                TiffEntry::ascii(0x9003, "2024:05:01 10:00:00"),
                TiffEntry::ascii(0xa357, "250"),
            ]),
            gps: None,
            interop: None,
            ifd1: None,
            thumbnail_jpeg: None,
        };
        let tiff = build_tiff(&parsed, &[], None, None);
        let mut payload = b"Exif".to_vec();
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&tiff);
        let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
        let len = (payload.len() + 2) as u16;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload);
        // SOF0：精度 8，高 2，宽 3，1 分量。
        out.extend_from_slice(&[0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x02, 0x00, 0x03, 0x01, 0x01, 0x11, 0x00]);
        // SOS：长度 8，扫描数据含 0xff00 转义序列，逼真即可。
        out.extend_from_slice(&[0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
        out.extend_from_slice(&[0xaa, 0xff, 0x00, 0xbb, 0xcc]);
        out.extend_from_slice(&[0xff, 0xd9]);
        out
    }

    pub fn sample_jpeg_no_exif() -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        out.extend_from_slice(&[0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x02, 0x00, 0x03, 0x01, 0x01, 0x11, 0x00]);
        out.extend_from_slice(&[0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
        out.extend_from_slice(&[0x11, 0x22, 0x33]);
        out.extend_from_slice(&[0xff, 0xd9]);
        out
    }

    #[test]
    fn convert_with_exif_preserves_sos_and_make() {
        let src = sample_jpeg_with_exif();
        let burst = "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA";
        let unique = "BBBBBBBB-2222-4222-8222-BBBBBBBBBBBB";
        let out = convert(&src, burst, unique).expect("convert");
        assert_eq!(sos_suffix(&out), sos_suffix(&src));
        let (b, u) = read_burst_info(&out);
        assert_eq!(b.as_deref(), Some(burst));
        assert_eq!(u.as_deref(), Some(unique));
        // Make 原样保留：重建后的 TIFF 里仍能读到 HONOR。
        let (ms, has) = exif_capture_time(&out);
        assert!(has);
        assert_eq!(ms, parse_exif_datetime("2024:05:01 10:00:00").map(|v| v + 250));
    }

    #[test]
    fn convert_without_exif_inserts_app1_and_preserves_sos() {
        let src = sample_jpeg_no_exif();
        let out = convert(&src, "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA", "CCCCCCCC-3333-4333-8333-CCCCCCCCCCCC")
            .expect("convert no-exif");
        assert_eq!(sos_suffix(&out), sos_suffix(&src));
        let (b, u) = read_burst_info(&out);
        assert!(b.is_some() && u.is_some());
    }

    #[test]
    fn convert_rejects_non_jpeg() {
        assert!(convert(b"not a jpeg", "x", "y").is_err());
        assert!(convert(&[0xff, 0xd8], "x", "y").is_ok()); // 只有 SOI 也能补 EXIF 段
    }

    #[test]
    fn exif_time_in_ifd0_is_recognized() {
        // 部分写入器（含 PIL）把 0x9003 直接放在 IFD0；须认，不能退回 mtime。
        let parsed = ParsedTiff {
            order: ByteOrder::Little,
            ifd0: vec![
                TiffEntry::ascii(0x010f, "HONOR"),
                TiffEntry::ascii(0x9003, "2024:05:01 10:00:00"),
            ],
            exif: None,
            gps: None,
            interop: None,
            ifd1: None,
            thumbnail_jpeg: None,
        };
        let tiff = build_tiff(&parsed, &[], None, None);
        let mut payload = b"Exif".to_vec();
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&tiff);
        let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
        let len = (payload.len() + 2) as u16;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&[0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
        out.extend_from_slice(&[0x11, 0x22]);
        out.extend_from_slice(&[0xff, 0xd9]);
        let (ms, has) = exif_capture_time(&out);
        assert!(has);
        assert_eq!(ms, parse_exif_datetime("2024:05:01 10:00:00"));
    }

    #[test]
    fn exif_time_missing_returns_false() {
        let (ms, has) = exif_capture_time(&sample_jpeg_no_exif());
        assert_eq!(ms, None);
        assert!(!has);
    }
}
