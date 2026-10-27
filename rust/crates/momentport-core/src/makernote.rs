//! Apple MakerNote。直接移植 Swift 版 `AppleMakerNote.swift`：
//! - 本身就是一个迷你 TIFF IFD，没有 TIFF 文件头：
//!   UInt16 条目数 + 每条 12 字节 + UInt32 下一 IFD（0）；
//! - 字节序与外层 TIFF 相同；值超过 4 字节时，条目里写的是
//!   “相对 MakerNote 起始位置的偏移”，不是相对 TIFF 文件头的偏移；
//! - 字符串是 ASCII，count 含结尾 NUL；为偶数对齐，字符串之间可多补
//!   一个 NUL，但 count 不变，解析时只读 count 个字节；
//! - tag 0x0001 MakerNoteVersion 写 15（对应 macOS 12 / iOS 15 时代常见
//!   版本，不是声称照片就是 iPhone 拍的）；Make / Model 一律不动。

use crate::tiff::ByteOrder;

pub const BURST_UUID_TAG: u16 = 0x000b;
pub const IMAGE_UNIQUE_ID_TAG: u16 = 0x0015;
pub const VERSION_TAG: u16 = 0x0001;
pub const SYNTHETIC_VERSION: i32 = 15;

/// 构造 Apple MakerNote 数据块。
pub fn build(burst_uuid: &str, image_unique_id: &str, order: ByteOrder) -> Vec<u8> {
    let mut burst_bytes = burst_uuid.as_bytes().to_vec();
    burst_bytes.push(0);
    let mut unique_bytes = image_unique_id.as_bytes().to_vec();
    unique_bytes.push(0);
    // IFD: 2（条目数）+ 3*12（条目）+ 4（下一 IFD）= 42 字节，字符串从 42 开始。
    let header_size = 2 + 3 * 12 + 4;
    let burst_offset = header_size;
    let burst_padded = if burst_bytes.len() % 2 == 0 {
        burst_bytes.len()
    } else {
        burst_bytes.len() + 1
    };
    let unique_offset = burst_offset + burst_padded;

    let mut out: Vec<u8> = Vec::new();
    let append_entry = |out: &mut Vec<u8>, tag: u16, typ: u16, count: u32, v: u32| {
        out.extend_from_slice(&order.bytes16(tag));
        out.extend_from_slice(&order.bytes16(typ));
        out.extend_from_slice(&order.bytes32(count));
        out.extend_from_slice(&order.bytes32(v));
    };
    out.extend_from_slice(&order.bytes16(3));
    append_entry(
        &mut out,
        VERSION_TAG,
        4,
        1,
        SYNTHETIC_VERSION as u32,
    );
    append_entry(
        &mut out,
        BURST_UUID_TAG,
        2,
        burst_bytes.len() as u32,
        burst_offset as u32,
    );
    append_entry(
        &mut out,
        IMAGE_UNIQUE_ID_TAG,
        2,
        unique_bytes.len() as u32,
        unique_offset as u32,
    );
    out.extend_from_slice(&order.bytes32(0));
    out.extend_from_slice(&burst_bytes);
    if burst_bytes.len() % 2 == 1 {
        out.push(0);
    }
    out.extend_from_slice(&unique_bytes);
    if unique_bytes.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// 解析 Apple MakerNote，返回 (burstUUID, imageUniqueID)。
/// 解析失败返回 (None, None)，调用方不得因此丢弃照片，只是不再断言成组。
pub fn parse(data: &[u8], order: ByteOrder) -> (Option<String>, Option<String>) {
    if data.len() < 2 {
        return (None, None);
    }
    let count = order.u16_at(data, 0) as usize;
    if count == 0 || data.len() < 2 + count * 12 + 4 {
        return (None, None);
    }
    let mut burst: Option<String> = None;
    let mut unique: Option<String> = None;
    for i in 0..count {
        let off = 2 + i * 12;
        if off + 12 > data.len() {
            break;
        }
        let tag = order.u16_at(data, off);
        let typ = order.u16_at(data, off + 2);
        let cnt = order.u32_at(data, off + 4) as usize;
        let val_off = order.u32_at(data, off + 8) as usize;
        if typ != 2 || cnt == 0 || val_off + cnt > data.len() {
            continue;
        }
        let raw = &data[val_off..val_off + cnt];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let s = String::from_utf8_lossy(&raw[..end]).into_owned();
        if tag == BURST_UUID_TAG {
            burst = Some(s);
        } else if tag == IMAGE_UNIQUE_ID_TAG {
            unique = Some(s);
        }
    }
    (burst, unique)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makernote_roundtrip_both_orders() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let burst = "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA";
            let unique = "BBBBBBBB-2222-4222-8222-BBBBBBBBBBBB";
            let data = build(burst, unique, order);
            // 头 42 字节 + 两串（各 37 字节含 NUL，奇数补 1 → 各 38）= 118。
            assert_eq!(data.len(), 42 + 38 + 38);
            let (b, u) = parse(&data, order);
            assert_eq!(b.as_deref(), Some(burst));
            assert_eq!(u.as_deref(), Some(unique));
        }
    }

    #[test]
    fn makernote_parse_rejects_short() {
        assert_eq!(parse(&[], ByteOrder::Big), (None, None));
        assert_eq!(parse(&[0x00], ByteOrder::Big), (None, None));
    }
}
