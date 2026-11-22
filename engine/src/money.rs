//! Money amounts as integer cents, so totals stay exact.

/// Parses `1,234.56`, `-$4.50`, `(12.00)` or `1.234,56` into cents.
/// `decimal` is the decimal separator. Other non-digit characters are
/// thousands separators or currency marks and are skipped.
pub fn parse_cents(raw: &str, decimal: char) -> Option<i64> {
    let mut negative = false;
    let mut int_digits = String::new();
    let mut frac_digits = String::new();
    let mut in_frac = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_digit() {
            if in_frac {
                frac_digits.push(ch);
            } else {
                int_digits.push(ch);
            }
        } else if ch == decimal {
            if in_frac {
                return None;
            }
            in_frac = true;
        } else if matches!(ch, '-' | '\u{2212}' | '(') {
            negative = true;
        }
    }
    if int_digits.is_empty() && frac_digits.is_empty() {
        return None;
    }
    let units: i64 = if int_digits.is_empty() {
        0
    } else {
        int_digits.parse().ok()?
    };
    let frac = frac_digits.as_bytes();
    let digit = |i: usize| frac.get(i).map_or(0, |b| i64::from(b - b'0'));
    let mut cents = units
        .checked_mul(100)?
        .checked_add(digit(0) * 10 + digit(1))?;
    if digit(2) >= 5 {
        cents += 1;
    }
    Some(if negative { -cents } else { cents })
}

/// Picks the decimal separator by majority vote. A separator followed by
/// exactly three digits is skipped, because `1,234` could be either.
pub fn detect_decimal<'a>(amounts: impl Iterator<Item = &'a str>) -> char {
    let (mut dot, mut comma) = (0usize, 0usize);
    for raw in amounts {
        let s = raw.trim_end_matches(|c: char| !c.is_ascii_digit());
        let Some(pos) = s.rfind(['.', ',']) else {
            continue;
        };
        let tail = &s[pos + 1..];
        if tail.is_empty() || tail.len() == 3 || !tail.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if s.as_bytes()[pos] == b'.' {
            dot += 1;
        } else {
            comma += 1;
        }
    }
    if comma > dot { ',' } else { '.' }
}

/// Formats cents as `-1,234.56`.
pub fn fmt_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let units = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in units.chars().enumerate() {
        if i > 0 && (units.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped}.{:02}", abs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_formats() {
        assert_eq!(parse_cents("1,234.56", '.'), Some(123_456));
        assert_eq!(parse_cents("-$4.50", '.'), Some(-450));
        assert_eq!(parse_cents("(12.00)", '.'), Some(-1_200));
        assert_eq!(parse_cents("\u{2212}3.1", '.'), Some(-310));
        assert_eq!(parse_cents("1.234,56", ','), Some(123_456));
        assert_eq!(parse_cents("CA$ 7", '.'), Some(700));
        assert_eq!(parse_cents("0.005", '.'), Some(1));
        assert_eq!(parse_cents("", '.'), None);
        assert_eq!(parse_cents("1.2.3", '.'), None);
    }

    #[test]
    fn detects_decimal_separator() {
        assert_eq!(
            detect_decimal(["1,234.56", "-4.50", "1,000"].into_iter()),
            '.'
        );
        assert_eq!(detect_decimal(["1.234,56", "-4,50"].into_iter()), ',');
    }

    #[test]
    fn formats_cents() {
        assert_eq!(fmt_cents(123_456_789), "1,234,567.89");
        assert_eq!(fmt_cents(-5), "-0.05");
        assert_eq!(fmt_cents(0), "0.00");
    }
}
