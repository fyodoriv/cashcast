//! JSON that reads and prints exactly like Python's `json` module.
//!
//! The port must print byte-identical output (decision 0010), so objects keep
//! their key order, ints stay ints, floats print as Python's `repr`, and
//! non-ASCII text is escaped as with `ensure_ascii=True`.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            // Python dicts keep the last value of a repeated key.
            Json::Obj(items) => items.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: Json) {
        if let Json::Obj(items) = self {
            if let Some(slot) = items.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                items.push((key.to_string(), value));
            }
        }
    }

    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Int(i) => *i != 0,
            Json::Float(f) => *f != 0.0,
            Json::Str(s) => !s.is_empty(),
            Json::List(v) => !v.is_empty(),
            Json::Obj(v) => !v.is_empty(),
        }
    }

    /// `str(value)` in Python, for the scalar values a state file holds.
    pub fn py_str(&self) -> String {
        match self {
            Json::Null => "None".into(),
            Json::Bool(true) => "True".into(),
            Json::Bool(false) => "False".into(),
            Json::Int(i) => i.to_string(),
            Json::Float(f) => float_repr(*f),
            Json::Str(s) => s.clone(),
            other => dumps(other, None, false),
        }
    }
}

/// Python's `repr(float)`: shortest round-trip digits, fixed notation for
/// exponents from -4 to 15, otherwise `1e+16` style.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let sci = format!("{x:e}"); // e.g. "-1.2345e3"
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mant) = mant.strip_prefix('-').map_or(("", mant), |m| ("-", m));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exp) {
        let point = exp + 1; // digits before the decimal point
        let body = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        format!("{sign}{body}")
    } else {
        let tail = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{}{tail}e{esign}{:02}", &digits[..1], exp.abs())
    }
}

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `json.dumps(value, indent=indent, sort_keys=sort)` with default separators.
pub fn dumps(value: &Json, indent: Option<usize>, sort: bool) -> String {
    let mut out = String::new();
    write_value(value, indent, sort, 0, &mut out);
    out
}

fn write_value(v: &Json, indent: Option<usize>, sort: bool, level: usize, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(f) => out.push_str(&float_repr(*f)),
        Json::Str(s) => escape(s, out),
        Json::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                sep(i, indent, level + 1, out);
                write_value(item, indent, sort, level + 1, out);
            }
            close(indent, level, out);
            out.push(']');
        }
        Json::Obj(items) => {
            if items.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut refs: Vec<&(String, Json)> = items.iter().collect();
            if sort {
                refs.sort_by(|a, b| a.0.cmp(&b.0));
            }
            out.push('{');
            for (i, (k, val)) in refs.into_iter().map(|p| (&p.0, &p.1)).enumerate() {
                sep(i, indent, level + 1, out);
                escape(k, out);
                out.push_str(": ");
                write_value(val, indent, sort, level + 1, out);
            }
            close(indent, level, out);
            out.push('}');
        }
    }
}

fn sep(i: usize, indent: Option<usize>, level: usize, out: &mut String) {
    match indent {
        Some(n) => {
            if i > 0 {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&" ".repeat(n * level));
        }
        None if i > 0 => out.push_str(", "),
        None => {}
    }
}

fn close(indent: Option<usize>, level: usize, out: &mut String) {
    if let Some(n) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * level));
    }
}

/// `json.loads`: one JSON document, or None when Python would raise.
pub fn loads(text: &str) -> Option<Json> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > 500 {
            return None;
        }
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Some(Json::Obj(items));
                }
                loop {
                    self.ws();
                    let Json::Str(k) = self.string()? else {
                        return None;
                    };
                    self.ws();
                    if !self.eat(":") {
                        return None;
                    }
                    self.ws();
                    let v = self.value(depth + 1)?;
                    // A repeated key keeps its first place and its last value.
                    if let Some(slot) = items
                        .iter_mut()
                        .find(|(key, _): &&mut (String, Json)| *key == k)
                    {
                        slot.1 = v;
                    } else {
                        items.push((k, v));
                    }
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    return self.eat("}").then_some(Json::Obj(items));
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Some(Json::List(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    return self.eat("]").then_some(Json::List(items));
                }
            }
            b'"' => self.string(),
            _ if self.eat("null") => Some(Json::Null),
            _ if self.eat("true") => Some(Json::Bool(true)),
            _ if self.eat("false") => Some(Json::Bool(false)),
            _ if self.eat("NaN") => Some(Json::Float(f64::NAN)),
            _ if self.eat("Infinity") => Some(Json::Float(f64::INFINITY)),
            _ if self.eat("-Infinity") => Some(Json::Float(f64::NEG_INFINITY)),
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        let digits = |p: &mut Self| {
            let s = p.i;
            while p.i < p.s.len() && p.s[p.i].is_ascii_digit() {
                p.i += 1;
            }
            p.i > s
        };
        self.eat("-");
        if self.eat("0") {
        } else if !digits(self) {
            return None;
        }
        let mut float = false;
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            float = true;
            if !digits(self) {
                return None;
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            float = true;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !digits(self) {
                return None;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        if float {
            text.parse().ok().map(Json::Float)
        } else {
            // Python ints have no limit; past i64 this keeps the value as a float.
            text.parse()
                .map(Json::Int)
                .ok()
                .or_else(|| text.parse().ok().map(Json::Float))
        }
    }

    fn string(&mut self) -> Option<Json> {
        if !self.eat("\"") {
            return None;
        }
        let mut out = String::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(Json::Str(out)),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&hi)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                let save = self.i;
                                self.i += 2;
                                let lo = self.hex4()?;
                                if (0xdc00..0xe000).contains(&lo) {
                                    0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)
                                } else {
                                    self.i = save;
                                    hi
                                }
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return None,
                    }
                }
                c if c < 0x20 => return None, // Python's strict mode refuses control characters
                _ => {
                    // Copy one UTF-8 character.
                    let start = self.i - 1;
                    let len = match c {
                        0x00..=0x7f => 1,
                        0xc0..=0xdf => 2,
                        0xe0..=0xef => 3,
                        _ => 4,
                    };
                    let end = start + len;
                    out.push_str(std::str::from_utf8(self.s.get(start..end)?).ok()?);
                    self.i = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
        self.i += 4;
        u32::from_str_radix(h, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_like_python_repr() {
        assert_eq!(float_repr(1.0), "1.0");
        assert_eq!(float_repr(1798000000.123), "1798000000.123");
        assert_eq!(float_repr(0.0001), "0.0001");
        assert_eq!(float_repr(0.00001), "1e-05");
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(-2.5), "-2.5");
    }

    #[test]
    fn round_trips_with_python_separators_and_order() {
        let v = loads(r#"{"b": 1, "a": [1.5, "é"], "c": {}}"#).unwrap();
        assert_eq!(
            dumps(&v, None, false),
            r#"{"b": 1, "a": [1.5, "\u00e9"], "c": {}}"#
        );
        assert_eq!(
            dumps(&v, None, true),
            r#"{"a": [1.5, "\u00e9"], "b": 1, "c": {}}"#
        );
        assert_eq!(
            dumps(&v, Some(1), false),
            "{\n \"b\": 1,\n \"a\": [\n  1.5,\n  \"\\u00e9\"\n ],\n \"c\": {}\n}"
        );
        assert!(loads("{\"a\": 1").is_none());
    }
}
