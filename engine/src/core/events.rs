//! Content-free event and error logs, kept on this computer only.
//!
//! Each line of `logs/events.jsonl` is one event. String fields are
//! allow-listed and must match a short identifier pattern, so free text
//! (payees, amounts as text, names) cannot leak into the log. Numbers are
//! counts and durations only. Errors go to `logs/errors.jsonl` with the user
//! story they break. Tests and `CASHCAST_LOG=off` turn logging off.

use super::paths::UserHome;
use super::pyjson::{Json, dumps};
use std::io::Write;
use std::path::Path;

pub const STRING_FIELDS: [&str; 11] = [
    "scope",
    "command",
    "story",
    "error_type",
    "format",
    "service",
    "action",
    "risk",
    "section",
    "user_kind",
    "result",
];

/// Command → the user story it serves (docs/user-stories.md).
pub fn story(command: &str) -> &'static str {
    match command {
        "setup" | "doctor" => "US-01",
        "init" | "import" | "first-error" | "totals" => "US-02",
        "status" => "US-03",
        "forecast" | "numbers" | "backtest" => "US-04",
        "users" => "US-05",
        "consent" => "US-06",
        "strategy" | "budget" | "invest" | "tax" | "market" => "US-07",
        "review" => "US-08",
        "reminders" => "US-09",
        "connectors" | "action" | "api" | "sync" => "US-10",
        "knowledge" => "US-11",
        "holdings" => "US-12",
        "web" => "US-13",
        "moneywiz" => "US-14",
        "notion" => "US-15",
        "fetch" => "US-22",
        _ => "US-00",
    }
}

/// `^[a-z0-9][a-z0-9:._/-]{0,63}$`
pub fn is_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b":._/-".contains(c))
}

fn enabled() -> bool {
    std::env::var("CASHCAST_LOG").map_or(true, |v| v != "off")
}

/// Keeps numbers and booleans, and allow-listed id strings; drops the rest.
pub fn clean(fields: &[(&str, Json)]) -> Vec<(String, Json)> {
    fields
        .iter()
        .filter(|(k, v)| match v {
            Json::Bool(_) | Json::Int(_) | Json::Float(_) => true,
            Json::Str(s) => STRING_FIELDS.contains(k) && is_id(s),
            _ => false,
        })
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn now() -> f64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (t.as_secs_f64() * 1000.0).round() / 1000.0
}

fn append(path: &Path, record: &Json) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let new = !path.exists();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{}", dumps(record, None, true));
    }
    #[cfg(unix)]
    if new {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn log(user: &UserHome, event: &str, fields: &[(&str, Json)]) {
    if !enabled() || !is_id(event) {
        return;
    }
    let mut record = vec![
        ("ts".to_string(), Json::Float(now())),
        ("event".to_string(), Json::Str(event.into())),
    ];
    record.extend(clean(fields));
    append(&user.logs().join("events.jsonl"), &Json::Obj(record));
}

/// Logs an error. `kind` is the Python exception class name, lowercased.
pub fn error(user: &UserHome, command: &str, kind: &str) {
    if !enabled() {
        return;
    }
    let record = Json::Obj(vec![
        ("ts".into(), Json::Float(now())),
        ("event".into(), Json::Str("error".into())),
        (
            "command".into(),
            Json::Str(if is_id(command) { command } else { "unknown" }.into()),
        ),
        ("story".into(), Json::Str(story(command).into())),
        ("error_type".into(), Json::Str(kind.into())),
    ]);
    append(&user.logs().join("errors.jsonl"), &record);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_text_never_reaches_the_log() {
        let kept = clean(&[
            ("scope", Json::Str("agent:aggregates".into())),
            ("payee", Json::Str("canary-shop".into())),
            ("command", Json::Str("Has Spaces".into())),
            ("ms", Json::Int(12)),
        ]);
        assert_eq!(
            kept,
            vec![
                ("scope".into(), Json::Str("agent:aggregates".into())),
                ("ms".into(), Json::Int(12))
            ]
        );
        assert_eq!(story("consent"), "US-06");
        assert_eq!(story("nope"), "US-00");
    }
}
