//! App settings: every default in one table, overrides in one file.
//!
//! `SETTINGS` below is the only list of settings. Overrides live in
//! `private/settings.toml` (this computer, all users; git never tracks it).
//! `bin/cashcast config` shows each value and where it comes from;
//! `bin/cashcast config set KEY VALUE` checks the value and saves it.

use super::paths::{self, AppError};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Int,
    Str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Other(String),
}

impl Value {
    /// `str(value)` in Python.
    pub fn py_str(&self) -> String {
        match self {
            Value::Bool(true) => "True".into(),
            Value::Bool(false) => "False".into(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => super::pyjson::float_repr(*f),
            Value::Str(s) | Value::Other(s) => s.clone(),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Other(_) => true,
        }
    }
}

pub struct Setting {
    pub key: &'static str,
    pub kind: Kind,
    pub default: fn() -> Value,
    pub help: &'static str,
}

pub const SETTINGS: [Setting; 11] = [
    Setting {
        key: "web.autostart",
        kind: Kind::Bool,
        default: || Value::Bool(true),
        help: "start the web app in the background when you run `cashcast`",
    },
    Setting {
        key: "web.port",
        kind: Kind::Int,
        default: || Value::Int(8765),
        help: "port of the web app that `cashcast` starts (0 picks a free port)",
    },
    Setting {
        key: "start.refresh",
        kind: Kind::Bool,
        default: || Value::Bool(true),
        help: "read new exports from the inbox when you run `cashcast`",
    },
    Setting {
        key: "start.tasks",
        kind: Kind::Int,
        default: || Value::Int(5),
        help: "how many upcoming tasks `cashcast` prints",
    },
    Setting {
        key: "start.net_total",
        kind: Kind::Bool,
        default: || Value::Bool(true),
        help: "print your net total and its trend first when you run `cashcast`",
    },
    Setting {
        key: "fetch.stale_days",
        kind: Kind::Int,
        default: || Value::Int(35),
        help: "an export older than this many days needs a refresh (`fetch plan`)",
    },
    Setting {
        key: "fetch.crypto_stale_days",
        kind: Kind::Int,
        default: || Value::Int(31),
        help: "the same for a crypto-only service (Kraken, Casa): checked once a month",
    },
    Setting {
        key: "fetch.downloads",
        kind: Kind::Str,
        default: || Value::Str("~/Downloads".into()),
        help: "the folder a browser saves exports to (`fetch collect`)",
    },
    Setting {
        key: "telemetry.enabled",
        kind: Kind::Bool,
        default: || Value::Bool(true),
        help: "send anonymous usage counts with consent net:usage-counts and a key (`telemetry`)",
    },
    Setting {
        key: "live.refresh",
        kind: Kind::Bool,
        default: || Value::Bool(true),
        help: "refresh Bank of Canada rates and inflation once a day (`knowledge refresh`; no personal data)",
    },
    Setting {
        key: "reminders.weekly",
        kind: Kind::Str,
        default: || Value::Str("sunday".into()),
        help: "weekday of the one weekly session reminder that holds every weekly and monthly task (`off`: each task syncs on its own)",
    },
];

fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

pub fn path() -> PathBuf {
    paths::private_root().join("settings.toml")
}

fn err(msg: impl Into<String>) -> AppError {
    AppError::new("settingserror", msg)
}

fn overrides() -> Result<Vec<(String, Value)>, AppError> {
    let p = path();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Ok(Vec::new());
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e| AppError::new("tomldecodeerror", format!("{e}")))?;
    let mut flat = Vec::new();
    for (name, values) in table {
        if let toml::Value::Table(t) = values {
            for (k, v) in t {
                let v = match v {
                    toml::Value::Boolean(b) => Value::Bool(b),
                    toml::Value::Integer(i) => Value::Int(i),
                    toml::Value::Float(f) => Value::Float(f),
                    toml::Value::String(s) => Value::Str(s),
                    other => Value::Other(other.to_string()),
                };
                flat.push((format!("{name}.{k}"), v));
            }
        }
    }
    Ok(flat)
}

fn lookup(own: &[(String, Value)], key: &str) -> Option<Value> {
    own.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

pub fn parse(key: &str, raw: &str) -> Result<Value, AppError> {
    let Some(s) = setting(key) else {
        let all: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        return Err(err(format!(
            "unknown setting {}; settings: {}",
            paths::py_repr(key),
            all.join(", ")
        )));
    };
    match s.kind {
        Kind::Bool => {
            let low = raw.trim().to_lowercase();
            if !["true", "false", "yes", "no", "on", "off", "1", "0"].contains(&low.as_str()) {
                return Err(err(format!("{key} is true or false")));
            }
            Ok(Value::Bool(
                ["true", "yes", "on", "1"].contains(&low.as_str()),
            ))
        }
        Kind::Int => {
            let value = parse_int(raw).ok_or_else(|| err(format!("{key} is a whole number")))?;
            if key == "web.port" && !(0..=65535).contains(&value) {
                return Err(err("web.port is between 0 and 65535"));
            }
            if key == "start.tasks" && value < 0 {
                return Err(err("start.tasks is 0 or more"));
            }
            Ok(Value::Int(value))
        }
        Kind::Str => {
            if key == "reminders.weekly"
                && raw.trim().to_lowercase() != "off"
                && crate::tasks::weekly::weekday(raw).is_none()
            {
                return Err(err("reminders.weekly is a weekday (sunday) or off"));
            }
            Ok(Value::Str(raw.to_string()))
        }
    }
}

/// Python's `int(str)`: surrounding spaces, a sign and `_` between digits.
fn parse_int(raw: &str) -> Option<i64> {
    let t = raw.trim();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    if !clean.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: i64 = clean.parse().ok()?;
    Some(if neg { -v } else { v })
}

pub fn get(key: &str) -> Result<Value, AppError> {
    let own = overrides()?;
    Ok(lookup(&own, key)
        .unwrap_or_else(|| setting(key).map_or(Value::Other(String::new()), |s| (s.default)())))
}

/// (key, value, source, help) for every setting.
pub fn show() -> Result<Vec<(&'static str, Value, String, &'static str)>, AppError> {
    let own = overrides()?;
    let rel = paths::rel(&path());
    Ok(SETTINGS
        .iter()
        .map(|s| match lookup(&own, s.key) {
            Some(v) => (s.key, v, rel.clone(), s.help),
            None => (s.key, (s.default)(), "default".to_string(), s.help),
        })
        .collect())
}

pub fn set_value(key: &str, raw: &str) -> Result<Value, AppError> {
    let value = parse(key, raw)?;
    let mut own: Vec<(String, Value)> = Vec::new();
    for (k, v) in overrides()? {
        if setting(&k).is_some() {
            own.retain(|(x, _)| *x != k);
            own.push((k, v));
        }
    }
    own.retain(|(k, _)| k != key);
    let default = (setting(key).expect("parsed").default)();
    if !same(&value, &default) {
        own.push((key.to_string(), value.clone()));
    }
    own.sort_by(|a, b| a.0.cmp(&b.0));
    let mut tables: Vec<(String, Vec<String>)> = Vec::new();
    for (k, v) in &own {
        let (table, name) = k.split_once('.').unwrap_or((k, ""));
        let shown = match v {
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            other => format!("\"{}\"", other.py_str().replace('"', "\\\"")),
        };
        match tables.iter_mut().find(|(t, _)| t == table) {
            Some((_, lines)) => lines.push(format!("{name} = {shown}")),
            None => tables.push((table.to_string(), vec![format!("{name} = {shown}")])),
        }
    }
    let mut text = String::from(
        "# Overrides of the defaults in engine/src/core/settings.rs. Change with: bin/cashcast config set KEY VALUE\n",
    );
    for (t, lines) in tables {
        text += &format!("\n[{t}]\n{}\n", lines.join("\n"));
    }
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| err(e.to_string()))?;
    }
    crate::write_private(&p, &text).map_err(err)?;
    Ok(value)
}

/// Python `==`: True == 1, so a bool default equals the int of the same value.
fn same(a: &Value, b: &Value) -> bool {
    let num = |v: &Value| match v {
        Value::Bool(x) => Some(*x as i64 as f64),
        Value::Int(x) => Some(*x as f64),
        Value::Float(x) => Some(*x),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_like_python() {
        assert_eq!(parse("web.autostart", " Off ").unwrap(), Value::Bool(false));
        assert_eq!(parse("web.port", " 8_080 ").unwrap(), Value::Int(8080));
        assert!(parse("web.port", "70000").is_err());
        assert!(parse("start.tasks", "-1").is_err());
        assert_eq!(
            parse("nope", "1")
                .unwrap_err()
                .message
                .split(';')
                .next()
                .unwrap(),
            "unknown setting 'nope'"
        );
        assert_eq!(Value::Bool(true).py_str(), "True");
    }
}
