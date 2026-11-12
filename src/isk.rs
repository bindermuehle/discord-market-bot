//! Parsing and formatting of ISK amounts ("1.2b", "350m", "1,500,000").

/// Parses an ISK amount. Accepts plain numbers (with `,`, `_`, `'` or spaces as
/// separators) and k/m/b/t suffixes, case-insensitive, optional trailing "isk".
pub fn parse(input: &str) -> Option<i64> {
    let s: String = input
        .trim()
        .to_lowercase()
        .trim_end_matches("isk")
        .chars()
        .filter(|c| !matches!(c, ',' | '_' | '\'' | ' '))
        .collect();
    let (num, mult) = match s.chars().last()? {
        'k' => (&s[..s.len() - 1], 1e3),
        'm' => (&s[..s.len() - 1], 1e6),
        'b' => (&s[..s.len() - 1], 1e9),
        't' => (&s[..s.len() - 1], 1e12),
        _ => (&s[..], 1.0),
    };
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let value = num.parse::<f64>().ok()? * mult;
    if !value.is_finite() || value <= 0.0 || value > 9e18 {
        return None;
    }
    Some(value.round() as i64)
}

/// Formats ISK compactly: 1200000000 -> "1.2B ISK".
pub fn format(isk: i64) -> String {
    let (div, suffix) = match isk {
        n if n >= 1_000_000_000_000 => (1e12, "T"),
        n if n >= 1_000_000_000 => (1e9, "B"),
        n if n >= 1_000_000 => (1e6, "M"),
        n if n >= 1_000 => (1e3, "K"),
        _ => return format!("{isk} ISK"),
    };
    let s = format!("{:.2}", isk as f64 / div);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}{suffix} ISK")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse("1.2B"), Some(1_200_000_000));
        assert_eq!(parse("350m"), Some(350_000_000));
        assert_eq!(parse("750k"), Some(750_000));
        assert_eq!(parse("1,500,000"), Some(1_500_000));
        assert_eq!(parse("1 200 000 000 ISK"), Some(1_200_000_000));
        assert_eq!(parse("42"), Some(42));
        assert_eq!(parse(""), None);
        assert_eq!(parse("b"), None);
        assert_eq!(parse("cheap"), None);
        assert_eq!(parse("-5m"), None);
        assert_eq!(parse("0"), None);
    }

    #[test]
    fn formats() {
        assert_eq!(format(1_200_000_000), "1.2B ISK");
        assert_eq!(format(350_000_000), "350M ISK");
        assert_eq!(format(1_234_567), "1.23M ISK");
        assert_eq!(format(999), "999 ISK");
    }
}
