//! Opt-in tests using local private samples, never embedded in the executable.
//! cargo test --test native_samples -- --ignored
use momentport_core::{jpeg, makernote, tiff::ByteOrder};
use std::{collections::HashSet, path::PathBuf};
fn samples() -> PathBuf {
    std::env::var_os("MOMENTPORT_PRIVATE_SAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../样片"))
}
fn tiff(jpeg: &[u8]) -> &[u8] {
    let mut p = 2;
    while p + 4 < jpeg.len() {
        let len = u16::from_be_bytes([jpeg[p + 2], jpeg[p + 3]]) as usize;
        if jpeg[p + 1] == 0xe1 && &jpeg[p + 4..p + 10] == b"Exif\0\0" {
            return &jpeg[p + 10..p + 2 + len];
        }
        p += 2 + len;
    }
    panic!("missing EXIF")
}
// Independent fixture extraction: no production TIFF parser or MakerNote writer.
fn entry<'a>(data: &'a [u8], ifd: usize, tag: u16, o: ByteOrder) -> (u16, u32, &'a [u8]) {
    for i in 0..o.u16_at(data, ifd) as usize {
        let e = ifd + 2 + 12 * i;
        if o.u16_at(data, e) != tag {
            continue;
        }
        let typ = o.u16_at(data, e + 2);
        let count = o.u32_at(data, e + 4);
        let len = count as usize
            * match typ {
                3 => 2,
                4 | 9 => 4,
                5 | 10 | 16 => 8,
                _ => 1,
            };
        let off = if len <= 4 {
            e + 8
        } else {
            o.u32_at(data, e + 8) as usize
        };
        return (typ, count, &data[off..off + len]);
    }
    panic!("missing tag {tag:x}")
}
fn note(j: &[u8]) -> &[u8] {
    let t = tiff(j);
    let o = if &t[..2] == b"MM" {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    };
    let exif = entry(t, o.u32_at(t, 4) as usize, 0x8769, o).2;
    entry(t, o.u32_at(exif, 0) as usize, 0x927c, o).2
}
#[test]
#[ignore = "requires private 样片/apple; no sample bytes are bundled"]
fn native_structure_and_reader_against_four_real_frames() {
    let o = ByteOrder::Big;
    let mut ids = HashSet::new();
    let mut group = None;
    for n in 3672..=3675 {
        let j = std::fs::read(samples().join(format!("apple/IMG_{n}.JPG")))
            .expect("native fixture required");
        let m = note(&j);
        assert_eq!(&m[..14], b"Apple iOS\0\0\x01MM");
        let version = entry(m, 14, 1, o);
        assert_eq!(
            (version.0, version.1, version.2),
            (9, 1, &17i32.to_be_bytes()[..])
        );
        for tag in [0xb, 0x20, 0x2b] {
            let e = entry(m, 14, tag, o);
            assert_eq!((e.0, e.1), (2, 37));
            assert_eq!(e.2[36], 0);
        }
        let (burst, id) = jpeg::read_burst_info(&j);
        let burst = burst.expect("native BurstUUID");
        assert!(!burst.is_empty());
        if let Some(expected) = &group {
            assert_eq!(&burst, expected);
        } else {
            group = Some(burst);
        }
        assert!(ids.insert(id.expect("native PhotoIdentifier")));
        // Outer TIFF byte order cannot affect the internally declared MM.
        assert_eq!(
            makernote::parse(m, ByteOrder::Little),
            makernote::parse(m, o)
        );
        let out = jpeg::convert(
            &j,
            "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA",
            "BBBBBBBB-2222-4222-8222-BBBBBBBBBBBB",
        )
        .unwrap();
        assert_eq!(jpeg::sos_suffix(&j), jpeg::sos_suffix(&out));
        let generated = note(&out);
        assert_eq!(&generated[..14], &m[..14]);
        for tag in [1, 0xb, 0x2b] {
            let a = entry(m, 14, tag, o);
            let b = entry(generated, 14, tag, o);
            assert_eq!((a.0, a.1), (b.0, b.1));
        }
    }
}
#[test]
#[ignore = "requires private 样片/Honor; no sample bytes are bundled"]
fn honor_real_outputs_preserve_metadata_pixels_and_source_bytes() {
    let mut count = 0;
    for dir in std::fs::read_dir(samples().join("Honor")).unwrap() {
        let dir = dir.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        for path in std::fs::read_dir(dir).unwrap() {
            let path = path.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("JPG") {
                continue;
            }
            let before = std::fs::read(&path).unwrap();
            let after = jpeg::convert(
                &before,
                "AAAAAAAA-1111-4111-8111-AAAAAAAAAAAA",
                "BBBBBBBB-2222-4222-8222-BBBBBBBBBBBB",
            )
            .unwrap();
            assert_eq!(jpeg::sos_suffix(&before), jpeg::sos_suffix(&after));
            let mut p = momentport_core::tiff::parse_tiff(tiff(&before)).unwrap();
            let mut q = momentport_core::tiff::parse_tiff(tiff(&after)).unwrap();
            p.ifd0.sort_by_key(|e| e.tag);
            q.ifd0.sort_by_key(|e| e.tag);
            if let Some(e) = &mut p.exif {
                e.sort_by_key(|e| e.tag);
            }
            if let Some(e) = &mut q.exif {
                e.sort_by_key(|e| e.tag);
            }
            if let Some(e) = &mut p.gps {
                e.sort_by_key(|e| e.tag);
            }
            if let Some(e) = &mut q.gps {
                e.sort_by_key(|e| e.tag);
            }
            assert_eq!(p.ifd0, q.ifd0);
            assert_eq!(p.exif, q.exif);
            assert_eq!(p.gps, q.gps);
            assert_eq!(std::fs::read(&path).unwrap(), before);
            count += 1;
        }
    }
    assert_eq!(count, 46);
}
