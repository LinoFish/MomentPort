//! TIFF 解析与重建。直接移植 Swift 版 `JpegExif.swift` 的语义：
//! - 条目值以原始字节保存，重建时不再解释语义，原照片的曝光、GPS、
//!   方向等信息原样保留；
//! - IFD0 去掉子 IFD 指针（0x8769、0x8825），Exif 去掉 0xa005、0x927c，
//!   重建时重新计算指针；
//! - 复杂缩略图（带 StripOffsets 的 IFD1）丢弃，只保留简单 JPEG 缩略图。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    pub fn bytes16(self, v: u16) -> [u8; 2] {
        match self {
            ByteOrder::Little => [(v & 0xff) as u8, ((v >> 8) & 0xff) as u8],
            ByteOrder::Big => [((v >> 8) & 0xff) as u8, (v & 0xff) as u8],
        }
    }
    pub fn bytes32(self, v: u32) -> [u8; 4] {
        match self {
            ByteOrder::Little => [
                (v & 0xff) as u8,
                ((v >> 8) & 0xff) as u8,
                ((v >> 16) & 0xff) as u8,
                ((v >> 24) & 0xff) as u8,
            ],
            ByteOrder::Big => [
                ((v >> 24) & 0xff) as u8,
                ((v >> 16) & 0xff) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
            ],
        }
    }
    pub fn u16_at(self, data: &[u8], off: usize) -> u16 {
        if off + 2 > data.len() {
            return 0;
        }
        match self {
            ByteOrder::Little => (data[off] as u16) | ((data[off + 1] as u16) << 8),
            ByteOrder::Big => ((data[off] as u16) << 8) | (data[off + 1] as u16),
        }
    }
    pub fn u32_at(self, data: &[u8], off: usize) -> u32 {
        if off + 4 > data.len() {
            return 0;
        }
        match self {
            ByteOrder::Little => {
                (data[off] as u32)
                    | ((data[off + 1] as u32) << 8)
                    | ((data[off + 2] as u32) << 16)
                    | ((data[off + 3] as u32) << 24)
            }
            ByteOrder::Big => {
                ((data[off] as u32) << 24)
                    | ((data[off + 1] as u32) << 16)
                    | ((data[off + 2] as u32) << 8)
                    | (data[off + 3] as u32)
            }
        }
    }
}

pub fn type_size(t: u16) -> usize {
    match t {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

/// 一个 TIFF 条目：值以原始字节保存，长度应为 count * type_size。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiffEntry {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    pub value: Vec<u8>,
}

impl TiffEntry {
    pub fn ascii(tag: u16, text: &str) -> TiffEntry {
        let mut v = text.as_bytes().to_vec();
        v.push(0);
        TiffEntry {
            tag,
            typ: 2,
            count: v.len() as u32,
            value: v,
        }
    }
    /// LONG 类型条目的值（4 字节，按给定字节序编码）。
    pub fn long_value(&self, order: ByteOrder) -> u32 {
        if self.value.len() < 4 {
            return 0;
        }
        order.u32_at(&self.value, 0)
    }
    /// ASCII 条目解码：去掉结尾 NUL 之前的内容；非 ASCII 返回 None。
    pub fn ascii_string(&self) -> Option<String> {
        if self.typ != 2 {
            return None;
        }
        let end = self.value.iter().position(|&b| b == 0).unwrap_or(self.value.len());
        Some(String::from_utf8_lossy(&self.value[..end]).into_owned())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParsedTiff {
    pub order: ByteOrder,
    pub ifd0: Vec<TiffEntry>,
    pub exif: Option<Vec<TiffEntry>>,
    pub gps: Option<Vec<TiffEntry>>,
    pub interop: Option<Vec<TiffEntry>>,
    pub ifd1: Option<Vec<TiffEntry>>,
    pub thumbnail_jpeg: Option<Vec<u8>>,
}

impl Default for ByteOrder {
    fn default() -> Self {
        ByteOrder::Big
    }
}

fn parse_ifd(tiff: &[u8], offset: usize, order: ByteOrder) -> Result<(Vec<TiffEntry>, usize), String> {
    if offset + 2 > tiff.len() {
        return Err("IFD 偏移超出范围".to_string());
    }
    let count = order.u16_at(tiff, offset) as usize;
    if offset + 2 + count * 12 + 4 > tiff.len() {
        return Err("IFD 条目超出范围".to_string());
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let eoff = offset + 2 + i * 12;
        let tag = order.u16_at(tiff, eoff);
        let typ = order.u16_at(tiff, eoff + 2);
        let cnt = order.u32_at(tiff, eoff + 4);
        let size = (cnt as usize) * type_size(typ);
        let value = if size <= 4 {
            tiff[eoff + 8..eoff + 8 + size].to_vec()
        } else {
            let voff = order.u32_at(tiff, eoff + 8) as usize;
            if voff + size > tiff.len() {
                return Err(format!("条目 0x{:04x} 的值超出范围", tag));
            }
            tiff[voff..voff + size].to_vec()
        };
        entries.push(TiffEntry {
            tag,
            typ,
            count: cnt,
            value,
        });
    }
    let next = order.u32_at(tiff, offset + 2 + count * 12) as usize;
    Ok((entries, next))
}

pub fn parse_tiff(tiff: &[u8]) -> Result<ParsedTiff, String> {
    if tiff.len() < 8 {
        return Err("TIFF 太短".to_string());
    }
    let order = if tiff[0] == 0x49 && tiff[1] == 0x49 {
        ByteOrder::Little
    } else if tiff[0] == 0x4d && tiff[1] == 0x4d {
        ByteOrder::Big
    } else {
        return Err("未知的字节序标记".to_string());
    };
    if order.u16_at(tiff, 2) != 42 {
        return Err("TIFF 魔数不是 42".to_string());
    }
    let ifd0_offset = order.u32_at(tiff, 4) as usize;
    let (ifd0, next_offset) = parse_ifd(tiff, ifd0_offset, order)?;

    let mut exif: Option<Vec<TiffEntry>> = None;
    let mut gps: Option<Vec<TiffEntry>> = None;
    let mut interop: Option<Vec<TiffEntry>> = None;
    if let Some(e) = ifd0.iter().find(|e| e.tag == 0x8769) {
        let ptr = e.long_value(order) as usize;
        let (entries, _) = parse_ifd(tiff, ptr, order)?;
        if let Some(ip) = entries.iter().find(|e| e.tag == 0xa005) {
            let ioff = ip.long_value(order) as usize;
            let (ientries, _) = parse_ifd(tiff, ioff, order)?;
            interop = Some(ientries);
        }
        exif = Some(entries);
    }
    if let Some(g) = ifd0.iter().find(|e| e.tag == 0x8825) {
        let ptr = g.long_value(order) as usize;
        let (entries, _) = parse_ifd(tiff, ptr, order)?;
        gps = Some(entries);
    }

    let mut ifd1: Option<Vec<TiffEntry>> = None;
    let mut thumbnail: Option<Vec<u8>> = None;
    if next_offset != 0 {
        let (entries, _) = parse_ifd(tiff, next_offset, order)?;
        let has_strip = entries.iter().any(|e| e.tag == 0x0111 || e.tag == 0x0117);
        if !has_strip {
            if let (Some(fmt), Some(len_e)) = (
                entries.iter().find(|e| e.tag == 0x0201),
                entries.iter().find(|e| e.tag == 0x0202),
            ) {
                let toff = fmt.long_value(order) as usize;
                let tlen = len_e.long_value(order) as usize;
                if toff + tlen <= tiff.len() && tlen > 0 {
                    thumbnail = Some(tiff[toff..toff + tlen].to_vec());
                    ifd1 = Some(
                        entries
                            .iter()
                            .filter(|e| e.tag != 0x0201 && e.tag != 0x0202)
                            .cloned()
                            .collect(),
                    );
                }
            }
        }
    }

    let clean_ifd0: Vec<TiffEntry> = ifd0
        .into_iter()
        .filter(|e| e.tag != 0x8769 && e.tag != 0x8825)
        .collect();
    let clean_exif = exif.map(|v| {
        v.into_iter()
            .filter(|e| e.tag != 0xa005 && e.tag != 0x927c)
            .collect()
    });
    Ok(ParsedTiff {
        order,
        ifd0: clean_ifd0,
        exif: clean_exif,
        gps,
        interop,
        ifd1,
        thumbnail_jpeg: thumbnail,
    })
}

fn ifd_size(n: usize) -> usize {
    2 + n * 12 + 4
}

fn values_size(entries: &[TiffEntry]) -> usize {
    entries
        .iter()
        .map(|e| if e.value.len() > 4 { (e.value.len() + 1) / 2 * 2 } else { 0 })
        .sum()
}

fn with_pointer(entries: &[TiffEntry], tag: u16, offset: usize, order: ByteOrder) -> Vec<TiffEntry> {
    entries
        .iter()
        .map(|e| {
            if e.tag == tag {
                TiffEntry {
                    tag: e.tag,
                    typ: e.typ,
                    count: e.count,
                    value: order.bytes32(offset as u32).to_vec(),
                }
            } else {
                e.clone()
            }
        })
        .collect()
}

fn write_ifd(
    out: &mut Vec<u8>,
    order: ByteOrder,
    entries: &[TiffEntry],
    next_offset: usize,
    value_start: usize,
) {
    out.extend_from_slice(&order.bytes16(entries.len() as u16));
    let mut value_cursor = value_start;
    let mut pending: Vec<(usize, Vec<u8>)> = Vec::new();
    for e in entries {
        out.extend_from_slice(&order.bytes16(e.tag));
        out.extend_from_slice(&order.bytes16(e.typ));
        out.extend_from_slice(&order.bytes32(e.count));
        if e.value.len() <= 4 {
            let mut field = e.value.clone();
            while field.len() < 4 {
                field.push(0);
            }
            out.extend_from_slice(&field);
        } else {
            out.extend_from_slice(&order.bytes32(value_cursor as u32));
            pending.push((value_cursor, e.value.clone()));
            value_cursor += (e.value.len() + 1) / 2 * 2;
        }
    }
    out.extend_from_slice(&order.bytes32(next_offset as u32));
    for (offset, value) in pending {
        if out.len() < offset {
            out.resize(offset, 0);
        }
        out.extend_from_slice(&value);
        if value.len() % 2 == 1 {
            out.push(0);
        }
    }
}

/// 重建 TIFF：只替换/新增 MakerNote 条目（0x927C），其余条目原样。
/// 与 Swift 版 `TIFFBuilder.build` 的布局一致：
/// Header(8) | IFD0 | IFD0值 | Exif | Exif值 | GPS | GPS值 | Interop | Interop值 | IFD1 | IFD1值 | 缩略图
pub fn build_tiff(
    parsed: &ParsedTiff,
    maker_note: &[u8],
    pixel_width: Option<u32>,
    pixel_height: Option<u32>,
) -> Vec<u8> {
    let order = parsed.order;
    let mut ifd0 = parsed.ifd0.clone();
    let mut exif = parsed.exif.clone().unwrap_or_default();
    let gps = parsed.gps.clone();
    let interop = parsed.interop.clone();
    let mut ifd1 = parsed.ifd1.clone();

    exif.push(TiffEntry {
        tag: 0x927c,
        typ: 7,
        count: maker_note.len() as u32,
        value: maker_note.to_vec(),
    });
    if parsed.exif.is_none() {
        if let (Some(w), Some(h)) = (pixel_width, pixel_height) {
            exif.push(TiffEntry {
                tag: 0xa002,
                typ: 4,
                count: 1,
                value: order.bytes32(w).to_vec(),
            });
            exif.push(TiffEntry {
                tag: 0xa003,
                typ: 4,
                count: 1,
                value: order.bytes32(h).to_vec(),
            });
        }
    }
    ifd0.push(TiffEntry {
        tag: 0x8769,
        typ: 4,
        count: 1,
        value: vec![0, 0, 0, 0],
    });
    if gps.is_some() {
        ifd0.push(TiffEntry {
            tag: 0x8825,
            typ: 4,
            count: 1,
            value: vec![0, 0, 0, 0],
        });
    }
    if interop.is_some() {
        exif.push(TiffEntry {
            tag: 0xa005,
            typ: 4,
            count: 1,
            value: vec![0, 0, 0, 0],
        });
    }
    if parsed.thumbnail_jpeg.is_some() && ifd1.is_some() {
        if let Some(f1) = &mut ifd1 {
            f1.push(TiffEntry {
                tag: 0x0201,
                typ: 4,
                count: 1,
                value: vec![0, 0, 0, 0],
            });
            f1.push(TiffEntry {
                tag: 0x0202,
                typ: 4,
                count: 1,
                value: vec![0, 0, 0, 0],
            });
        }
    }
    ifd0.sort_by_key(|e| e.tag);
    exif.sort_by_key(|e| e.tag);
    let mut gps_sorted = gps.map(|mut g| {
        g.sort_by_key(|e| e.tag);
        g
    });
    let mut interop_sorted = interop.map(|mut it| {
        it.sort_by_key(|e| e.tag);
        it
    });
    if let Some(f1) = &mut ifd1 {
        f1.sort_by_key(|e| e.tag);
    }

    let ifd0_offset = 8usize;
    let ifd0_values_offset = ifd0_offset + ifd_size(ifd0.len());
    let exif_offset = ifd0_values_offset + values_size(&ifd0);
    let exif_values_offset = exif_offset + ifd_size(exif.len());
    let mut cursor = exif_values_offset + values_size(&exif);
    let mut gps_offset = 0usize;
    let mut gps_values_offset = 0usize;
    if let Some(g) = &gps_sorted {
        gps_offset = cursor;
        gps_values_offset = gps_offset + ifd_size(g.len());
        cursor = gps_values_offset + values_size(g);
    }
    let mut interop_offset = 0usize;
    let mut interop_values_offset = 0usize;
    if let Some(it) = &interop_sorted {
        interop_offset = cursor;
        interop_values_offset = interop_offset + ifd_size(it.len());
        cursor = interop_values_offset + values_size(it);
    }
    let mut ifd1_offset = 0usize;
    let mut ifd1_values_offset = 0usize;
    if let Some(f1) = &ifd1 {
        ifd1_offset = cursor;
        ifd1_values_offset = ifd1_offset + ifd_size(f1.len());
        cursor = ifd1_values_offset + values_size(f1);
    }
    let mut thumb_offset = 0usize;
    if let Some(thumb) = &parsed.thumbnail_jpeg {
        thumb_offset = cursor;
        cursor += (thumb.len() + 1) / 2 * 2;
    }
    let _ = cursor;

    ifd0 = with_pointer(&ifd0, 0x8769, exif_offset, order);
    if gps_sorted.is_some() {
        ifd0 = with_pointer(&ifd0, 0x8825, gps_offset, order);
    }
    exif = with_pointer(&exif, 0xa005, interop_offset, order);
    if parsed.thumbnail_jpeg.is_some() {
        if let Some(f1) = &ifd1 {
            let thumb_len = parsed.thumbnail_jpeg.as_ref().map(|t| t.len()).unwrap_or(0);
            let f1 = with_pointer(f1, 0x0201, thumb_offset, order);
            let f1 = with_pointer(&f1, 0x0202, thumb_len, order);
            ifd1 = Some(f1);
        }
    }
    // 消除未使用变量警告（gps/interop 在上面已用过，保留绑定以便阅读）。
    let _ = (&mut gps_sorted, &mut interop_sorted);

    let mut out: Vec<u8> = Vec::new();
    match order {
        ByteOrder::Little => out.extend_from_slice(&[0x49, 0x49]),
        ByteOrder::Big => out.extend_from_slice(&[0x4d, 0x4d]),
    }
    out.extend_from_slice(&order.bytes16(42));
    out.extend_from_slice(&order.bytes32(ifd0_offset as u32));
    write_ifd(&mut out, order, &ifd0, ifd1_offset, ifd0_values_offset);
    write_ifd(&mut out, order, &exif, 0, exif_values_offset);
    if let Some(g) = &gps_sorted {
        write_ifd(&mut out, order, g, 0, gps_values_offset);
    }
    if let Some(it) = &interop_sorted {
        write_ifd(&mut out, order, it, 0, interop_values_offset);
    }
    if let Some(f1) = &ifd1 {
        write_ifd(&mut out, order, f1, 0, ifd1_values_offset);
    }
    if let Some(thumb) = &parsed.thumbnail_jpeg {
        if out.len() < thumb_offset {
            out.resize(thumb_offset, 0);
        }
        out.extend_from_slice(thumb);
        if thumb.len() % 2 == 1 {
            out.push(0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个最小 TIFF：IFD0（Make + Exif 指针）+ Exif（DateTimeOriginal）。
    pub fn sample_tiff(order: ByteOrder) -> Vec<u8> {
        // 手工布局：头 8 | IFD0(2+2*12+4=30) | IFD0 值区 | Exif IFD | Exif 值区。
        // 为简单起见用 build 往返以外的手工构造：直接用 ParsedTiff + build。
        let parsed = ParsedTiff {
            order,
            ifd0: vec![TiffEntry::ascii(0x010f, "HONOR")],
            exif: Some(vec![
                TiffEntry::ascii(0x9003, "2024:05:01 10:00:00"),
                TiffEntry::ascii(0xa357, "123"),
            ]),
            gps: None,
            interop: None,
            ifd1: None,
            thumbnail_jpeg: None,
        };
        build_tiff(&parsed, &[], None, None)
    }

    #[test]
    fn tiff_roundtrip_preserves_entries() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let tiff = sample_tiff(order);
            let parsed = parse_tiff(&tiff).expect("parse sample");
            assert_eq!(parsed.order, order);
            let make = parsed
                .ifd0
                .iter()
                .find(|e| e.tag == 0x010f)
                .unwrap()
                .ascii_string()
                .unwrap();
            assert_eq!(make, "HONOR");
            let exif = parsed.exif.unwrap();
            assert_eq!(
                exif
                    .iter()
                    .find(|e| e.tag == 0x9003)
                    .unwrap()
                    .ascii_string()
                    .unwrap(),
                "2024:05:01 10:00:00"
            );
            // 注意：parse_tiff 会像 Swift 版一样把 0x927c 从 exif 里滤掉
            // （重建时再重新写入），所以这里不以解析结果断言 MakerNote 存在；
            // MakerNote 往返由 makernote / jpeg 测试覆盖。
        }
    }

    #[test]
    fn tiff_rejects_bad_magic() {
        assert!(parse_tiff(b"XX\x00\x00\x00\x00\x00\x00").is_err());
        assert!(parse_tiff(b"II*\x00").is_err());
    }
}
