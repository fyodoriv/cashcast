//! Python's number formatting and dict access, so the strategy prints
//! byte-identical text (decision 0010).

use crate::core::pyjson::Json;

/// TOML as Python's `tomllib` gives it, in the JSON value type. Dates become
/// their ISO text (what `str(date)` prints).
pub fn from_toml(v: &toml::Value) -> Json {
    match v {
        toml::Value::String(s) => Json::Str(s.clone()),
        toml::Value::Integer(i) => Json::Int(*i),
        toml::Value::Float(f) => Json::Float(*f),
        toml::Value::Boolean(b) => Json::Bool(*b),
        toml::Value::Datetime(d) => Json::Str(d.to_string()),
        toml::Value::Array(a) => Json::List(a.iter().map(from_toml).collect()),
        toml::Value::Table(t) => table(t),
    }
}

pub fn table(t: &toml::Table) -> Json {
    Json::Obj(t.iter().map(|(k, v)| (k.clone(), from_toml(v))).collect())
}

/// `float(x)` for a number; None for anything else.
pub fn num(v: Option<&Json>) -> Option<f64> {
    match v? {
        Json::Int(i) => Some(*i as f64),
        Json::Float(f) => Some(*f),
        Json::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Json::Str(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `float(d.get(key, default))`.
pub fn getf(d: &Json, key: &str, default: f64) -> f64 {
    num(d.get(key)).unwrap_or(default)
}

/// `str(d.get(key, default))` for a text value.
pub fn gets(d: &Json, key: &str, default: &str) -> String {
    d.get(key).map_or(default.to_string(), Json::py_str)
}

/// `d.get(key, {})`.
pub fn obj<'a>(d: &'a Json, key: &str) -> &'a Json {
    static EMPTY: Json = Json::Obj(Vec::new());
    d.get(key).unwrap_or(&EMPTY)
}

/// `d.get(key, [])`.
pub fn list<'a>(d: &'a Json, key: &str) -> &'a [Json] {
    match d.get(key) {
        Some(Json::List(l)) => l,
        _ => &[],
    }
}

/// Python's `round(x, n)` for a float: the nearest float to the correctly
/// rounded decimal.
pub fn round(x: f64, n: usize) -> f64 {
    format!("{x:.n$}").parse().unwrap_or(x)
}

/// Python's `round(x)`: an int, ties to even.
pub fn round0(x: f64) -> i64 {
    let f = x.floor();
    let d = x - f;
    let r = if d > 0.5 || (d == 0.5 && f.rem_euclid(2.0) == 1.0) {
        f + 1.0
    } else {
        f
    };
    r as i64
}

/// Python's `sum` of floats on 3.12 and later: compensated addition.
pub fn sum(values: impl IntoIterator<Item = f64>) -> f64 {
    crate::forecast::formula::py_sum(values)
}

fn group(int: &str) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `f"{x:,.{dp}f}"` for a float.
pub fn commas(x: f64, dp: usize) -> String {
    let s = format!("{:.dp$}", x.abs());
    let (int, frac) = s.split_once('.').map_or((s.as_str(), ""), |(a, b)| (a, b));
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if dp > 0 {
        format!("{sign}{}.{frac}", group(int))
    } else {
        format!("{sign}{}", group(int))
    }
}

/// `strategy.money`: "$1,234.50" or "-$1,234.50".
pub fn money(x: f64) -> String {
    if x >= 0.0 {
        format!("${}", commas(x, 2))
    } else {
        format!("-${}", commas(-x, 2))
    }
}

/// `strategy.pct`: "2.50%", and "3%" for "3.00%".
pub fn pct(x: f64) -> String {
    format!("{:.2}%", x * 100.0).replace(".00%", "%")
}

/// `f"{x:,.{prec}g}"` (with grouping) or `f"{x:.{prec}g}"`.
pub fn fmt_g(x: f64, prec: usize, grouping: bool) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let p = prec.max(1);
    let sci = format!("{:.*e}", p - 1, x);
    let (_, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("int");
    let strip = |s: String| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if exp < -4 || exp >= p as i32 {
        let (m, _) = sci.split_once('e').expect("exponent");
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", strip(m.to_string()), exp.abs());
    }
    let fixed = strip(format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, x));
    if !grouping {
        return fixed;
    }
    let (neg, body) = fixed
        .strip_prefix('-')
        .map_or((false, fixed.as_str()), |b| (true, b));
    let (int, frac) = body
        .split_once('.')
        .map_or((body, None), |(i, f)| (i, Some(f)));
    let digits: Vec<char> = int.chars().collect();
    let mut grouped = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*c);
    }
    format!(
        "{}{grouped}{}",
        if neg { "-" } else { "" },
        frac.map_or(String::new(), |f| format!(".{f}"))
    )
}

/// `math.fsum`: the correctly rounded sum (Shewchuk's partials).
pub fn fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut partials: Vec<f64> = Vec::new();
    for mut x in values {
        let mut i = 0;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        partials.truncate(i);
        partials.push(x);
    }
    let mut n = partials.len();
    let mut hi = 0.0;
    if n > 0 {
        n -= 1;
        hi = partials[n];
        let mut lo = 0.0;
        while n > 0 {
            let x = hi;
            n -= 1;
            let y = partials[n];
            hi = x + y;
            let yr = hi - x;
            lo = y - yr;
            if lo != 0.0 {
                break;
            }
        }
        if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
            let y = lo * 2.0;
            let x = hi + y;
            if y == x - hi {
                hi = x;
            }
        }
    }
    hi
}

/// `statistics.mean` of floats (near enough: the exact sum, then one division).
pub fn mean(values: &[f64]) -> f64 {
    fsum(values.iter().copied()) / values.len() as f64
}

/// `statistics.pstdev`.
pub fn pstdev(values: &[f64]) -> f64 {
    let m = mean(values);
    (fsum(values.iter().map(|v| (v - m) * (v - m))) / values.len() as f64).sqrt()
}

/// `f"{date:%b %d}"`.
pub fn mon_day(d: chrono::NaiveDate) -> String {
    d.format("%b %d").to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn g_format_matches_python() {
        assert_eq!(super::fmt_g(0.0123, 8, true), "0.0123");
        assert_eq!(super::fmt_g(1234567.5, 8, true), "1,234,567.5");
        assert_eq!(super::fmt_g(123456789.0, 8, true), "1.2345679e+08");
        assert_eq!(super::fmt_g(0.00001234, 8, true), "1.234e-05");
        assert_eq!(super::fmt_g(3.0, 8, true), "3");
    }

    use super::*;

    #[test]
    fn formats_like_python() {
        assert_eq!(money(1234.5), "$1,234.50");
        assert_eq!(money(-0.004), "-$0.00");
        assert_eq!(pct(0.03), "3%");
        assert_eq!(pct(0.0125), "1.25%");
        assert_eq!(round(2.675, 2), 2.67); // the float is below 2.675
        assert_eq!(round0(2.5), 2);
        assert_eq!(round0(3.5), 4);
        assert_eq!(round0(-2.5), -2);
        assert_eq!(commas(-1234567.891, 0), "-1,234,568");
    }
}
