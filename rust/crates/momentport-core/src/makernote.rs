//! Apple iOS MakerNote layout, checked against the native iPhone 12 fixtures.
//! The 14-byte signature includes its own byte order. IFD starts at byte 14;
//! out-of-line offsets are relative to byte 0 of the MakerNote, not the IFD
//! or outer TIFF. Do not infer frame numbers or cover flags from unknown tags.
use crate::tiff::ByteOrder;

pub const HEADER: &[u8; 14] = b"Apple iOS\0\0\x01MM";
pub const BURST_UUID_TAG: u16 = 0x000b;
pub const PHOTO_IDENTIFIER_TAG: u16 = 0x002b;
pub const VERSION_TAG: u16 = 0x0001;
pub const SYNTHETIC_VERSION: i32 = 17;

/// The outer TIFF order is deliberately independent of the MakerNote's MM.
pub fn build(burst_uuid: &str, image_unique_id: &str, _outer_order: ByteOrder) -> Vec<u8> {
    let order = ByteOrder::Big;
    let entries = [
        (
            VERSION_TAG,
            9u16,
            1u32,
            order.bytes32(SYNTHETIC_VERSION as u32).to_vec(),
        ),
        (
            BURST_UUID_TAG,
            2,
            (burst_uuid.len() + 1) as u32,
            [burst_uuid.as_bytes(), &[0]].concat(),
        ),
        (
            PHOTO_IDENTIFIER_TAG,
            2,
            (image_unique_id.len() + 1) as u32,
            [image_unique_id.as_bytes(), &[0]].concat(),
        ),
    ];
    let mut offset = HEADER.len() + 2 + entries.len() * 12 + 4;
    let mut out = HEADER.to_vec();
    let mut values = Vec::new();
    out.extend_from_slice(&order.bytes16(entries.len() as u16));
    for (tag, typ, count, value) in entries {
        out.extend_from_slice(&order.bytes16(tag));
        out.extend_from_slice(&order.bytes16(typ));
        out.extend_from_slice(&order.bytes32(count));
        if value.len() <= 4 {
            out.extend_from_slice(&value);
            out.resize(out.len() + 4 - value.len(), 0);
        } else {
            out.extend_from_slice(&order.bytes32(offset as u32));
            offset += value.len();
            values.extend_from_slice(&value);
        }
    }
    out.extend_from_slice(&order.bytes32(0));
    out.extend_from_slice(&values);
    out
}

/// Read native Apple notes only; never validate the old headerless custom IFD.
/// The second result is PhotoIdentifier (0x2b); older 0x15 is a fallback only.
pub fn parse(data: &[u8], _outer_order: ByteOrder) -> (Option<String>, Option<String>) {
    if data.len() < 20 || &data[..12] != &HEADER[..12] {
        return (None, None);
    }
    let order = match &data[12..14] {
        b"MM" => ByteOrder::Big,
        b"II" => ByteOrder::Little,
        _ => return (None, None),
    };
    let count = order.u16_at(data, 14) as usize;
    if count == 0 || 16 + count * 12 + 4 > data.len() {
        return (None, None);
    }
    let mut burst = None;
    let mut photo = None;
    let mut legacy = None;
    for i in 0..count {
        let e = 16 + i * 12;
        let tag = order.u16_at(data, e);
        let typ = order.u16_at(data, e + 2);
        let len = order.u32_at(data, e + 4) as usize;
        if typ != 2 || len == 0 {
            continue;
        }
        let offset = if len <= 4 {
            e + 8
        } else {
            order.u32_at(data, e + 8) as usize
        };
        let raw = match offset
            .checked_add(len)
            .and_then(|end| data.get(offset..end))
        {
            Some(raw) => raw,
            None => continue,
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let value = match std::str::from_utf8(&raw[..end]) {
            Ok(s) if !s.is_empty() => Some(s.to_owned()),
            _ => continue,
        };
        match tag {
            BURST_UUID_TAG => burst = value,
            PHOTO_IDENTIFIER_TAG => photo = value,
            0x0015 => legacy = value,
            _ => (),
        }
    }
    (burst, photo.or(legacy))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn makernote_roundtrip_both_outer_orders() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let b = "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA";
            let u = "BBBBBBBB-2222-4222-8222-BBBBBBBBBBBB";
            let data = build(b, u, order);
            assert_eq!(&data[..14], HEADER);
            assert_eq!(ByteOrder::Big.u16_at(&data, 18), 9); // signed LONG
            assert_eq!(ByteOrder::Big.u32_at(&data, 36), 56); // note-relative
            assert_eq!(parse(&data, order), (Some(b.into()), Some(u.into())));
        }
    }
    #[test]
    fn makernote_parse_rejects_short_headerless_and_invalid_offsets() {
        for data in [&[][..], &[0][..], &[0; 118][..]] {
            assert_eq!(parse(data, ByteOrder::Big), (None, None));
        }
        let mut data = build("12345", "67890", ByteOrder::Big);
        data[36..40].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(parse(&data, ByteOrder::Big), (None, Some("67890".into())));
    }
}
