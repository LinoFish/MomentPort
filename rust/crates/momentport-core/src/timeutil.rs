//! 时间工具：只用标准库。
//!
//! 口径说明（与 Swift 版 `BurstGrouper` 对齐，分组只依赖相对顺序）：
//! - EXIF `DateTimeOriginal` 是无时区的墙上时间（`yyyy:MM:dd HH:mm:ss`），
//!   本核心按 UTC 把它换算成 epoch 毫秒，只用于排序与间隔切分；
//!   混用 EXIF 时间与文件修改时间兜底时，排序仍以换算后的值为准，
//!   macOS 桥接层如需绝对本地时间，应以 ImageIO 读到的绝对时间为准。
//! - `SubSecTimeOriginal` 是纯数字字符串，按位数换算（"123"→0.123 秒，
//!   "12"→0.12 秒），与 Swift 版 `parseSubSec` 一致。

/// Howard Hinnant 的 days-from-civil 算法（1970-01-01 为第 0 天）。
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// days-from-civil 的逆运算。
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn parse_int(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}

/// 解析 `yyyy:MM:dd HH:mm:ss`，返回 UTC epoch 毫秒；格式或范围不对返回 None。
pub fn parse_exif_datetime(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 19 || b[4] != b':' || b[7] != b':' || b[10] != b' ' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let y = parse_int(&s[0..4])?;
    let mo = parse_int(&s[5..7])?;
    let d = parse_int(&s[8..10])?;
    let h = parse_int(&s[11..13])?;
    let mi = parse_int(&s[14..16])?;
    let sec = parse_int(&s[17..19])?;
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    Some((((days * 24 + h) * 60 + mi) * 60 + sec) * 1000)
}

/// 解析 SubSec 字符串，返回毫秒数（"123"→123，"12"→120，"1"→100）。
pub fn parse_subsec_ms(s: &str) -> Option<i64> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    let n = digits.len() as u32;
    if n >= 3 {
        // 3 位以上：按毫秒截断（第 4 位起是微秒及以下，不影响毫秒分组）。
        let divisor = 10i64.pow(n - 3);
        Some(value / divisor)
    } else {
        Some(value * 10i64.pow(3 - n))
    }
}

/// epoch 毫秒 → `YYYY-MM-DDTHH:MM:SS.mmmZ`（UTC）。
pub fn format_iso_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let rem = ms.rem_euclid(1000);
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        mo,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60,
        rem
    )
}

/// `SystemTime` → epoch 毫秒；1970 年之前返回 0（本工具不依赖该分支）。
pub fn system_time_ms(t: std::time::SystemTime) -> i64 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (d.as_millis()) as i64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_datetime_roundtrip() {
        let ms = parse_exif_datetime("2024:05:01 10:00:00").unwrap();
        assert_eq!(format_iso_ms(ms), "2024-05-01T10:00:00.000Z");
        assert_eq!(format_iso_ms(ms + 123), "2024-05-01T10:00:00.123Z");
    }

    #[test]
    fn subsec_by_digits() {
        assert_eq!(parse_subsec_ms("123"), Some(123));
        assert_eq!(parse_subsec_ms("12"), Some(120));
        assert_eq!(parse_subsec_ms("1"), Some(100));
        assert_eq!(parse_subsec_ms(""), None);
        assert_eq!(parse_subsec_ms("abc"), None);
    }

    #[test]
    fn exif_datetime_rejects_bad() {
        assert_eq!(parse_exif_datetime("2024-05-01 10:00:00"), None);
        assert_eq!(parse_exif_datetime("2024:13:01 10:00:00"), None);
        assert_eq!(parse_exif_datetime("2024:02:30 10:00:00"), None);
    }
}
