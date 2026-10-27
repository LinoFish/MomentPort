//! 大写连字符 UUID（与 Swift `UUID().uuidString` 的写法一致）。
//! 优先读系统随机源（Linux/macOS 的 `/dev/urandom`）；读不到时用
//! 时间 + 进程号 + 进程内计数器混合兜底——仍保证同一进程内每次不同，
//! 但不承诺密码学强度；BurstUUID / ImageUniqueID 只作分组标识，不作密钥。

use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fallback_bytes() -> [u8; 16] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos
        ^ ((std::process::id() as u64) << 32)
        ^ COUNTER.fetch_add(1, Ordering::SeqCst).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    // splitmix64 展开，雪崩一次即可。
    let mut out = [0u8; 16];
    for chunk in out.chunks_mut(8) {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        chunk.copy_from_slice(&z.to_le_bytes());
    }
    out
}

fn random_bytes() -> [u8; 16] {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let mut buf = [0u8; 16];
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    fallback_bytes()
}

/// 生成一个版本 4、大写、带连字符的 UUID 字符串。
pub fn new_uuid_upper() -> String {
    let mut b = random_bytes();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_shape_and_uniqueness() {
        let a = new_uuid_upper();
        let b = new_uuid_upper();
        assert_eq!(a.len(), 36);
        assert_eq!(&a[8..9], "-");
        assert_eq!(&a[13..14], "-");
        assert_eq!(&a[18..19], "-");
        assert_eq!(&a[23..24], "-");
        assert_eq!(&a[14..15], "4"); // 版本 4
        assert!(a.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-'));
        assert_ne!(a, b);
    }
}
