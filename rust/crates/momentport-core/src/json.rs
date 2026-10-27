//! 手动 JSON 转义（仅标准库，不引 serde）。只做转义，不做解析。

/// 把字符串转义成可放进 JSON 双引号里的内容（不含外层引号）。
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 加上外层双引号。
pub fn quoted(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_basics() {
        assert_eq!(escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(quoted("x"), "\"x\"");
    }
}
