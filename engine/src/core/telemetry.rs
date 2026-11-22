//! Opt-in field telemetry: which anonymous usage counts would go to PostHog EU.
//!
//! `telemetry status`, `send` (an HTTPS POST to PostHog EU), `send --dry-run`
//! and the start screen's one-time ask (`ask_blocked`, `ask`). What may leave: per event, its name, its time and allow-listed
//! fields, keyed by a random install id. Never amounts, names or paths.
//!
//! Lifecycle: the install id, the last send day, the notice flag, the ask count
//! and answer (`asks`, `last_ask_day`, `ask_answer`), and one log
//! cursor per user live in `<private>/telemetry.json` (mode 600; read and
//! written per call). Deleting the file makes a new install id.

use super::paths::{self, AppError, UserHome};
use super::pyjson::{self, Json};
use super::{consent, events, settings};
use chrono::NaiveDate;
use std::path::PathBuf;

pub const SCOPE: &str = "net:usage-counts";
const DEFAULT_HOST: &str = "https://eu.i.posthog.com";
pub const BATCH_SIZE: usize = 500;
const MAX_EVENTS_PER_SEND: usize = 5000;
const STRING_KEYS: [&str; 7] = [
    "command",
    "story",
    "result",
    "error_type",
    "section",
    "scope",
    "user_kind",
];

/// `^[a-z0-9_.:-]{1,48}$`
fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.:-".contains(&c))
}

/// `^[a-z][a-z0-9_]{0,31}$`
fn is_num_key(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

fn state_path() -> PathBuf {
    paths::private_root().join("telemetry.json")
}

fn state() -> Json {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| pyjson::loads(&t))
        .filter(|v| matches!(v, Json::Obj(_)))
        .unwrap_or(Json::Obj(Vec::new()))
}

fn save(state: &Json) -> Result<(), AppError> {
    let p = state_path();
    let fail = AppError::io_text;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
    }
    let tmp = p.with_file_name(format!(".telemetry.{}.tmp", std::process::id()));
    crate::write_private(&tmp, &(pyjson::dumps(state, None, true) + "\n")).map_err(fail)?;
    std::fs::rename(&tmp, &p).map_err(|e| fail(e.to_string()))
}

/// A random uuid4 as 32 hex digits.
fn uuid4_hex() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_err()
    {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        b = (t.as_nanos() ^ ((std::process::id() as u128) << 64)).to_le_bytes();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn install_id() -> Result<String, AppError> {
    let mut st = state();
    match st.get("install_id") {
        Some(v) if v.truthy() => Ok(v.py_str()),
        _ => {
            let id = uuid4_hex();
            st.set("install_id", Json::Str(id.clone()));
            save(&st)?;
            Ok(id)
        }
    }
}

pub fn endpoint() -> String {
    let host = std::env::var("CASHCAST_TELEMETRY_URL").unwrap_or_else(|_| DEFAULT_HOST.into());
    format!("{}/batch/", host.trim_end_matches('/'))
}

/// The PostHog project key, or "". `CASHCAST_POSTHOG_KEY`, when set (tests set
/// it empty), wins. Never printed.
pub fn key() -> String {
    if let Ok(k) = std::env::var("CASHCAST_POSTHOG_KEY") {
        return k;
    }
    if !cfg!(target_os = "macos") {
        return String::new();
    }
    std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "cashcast",
            "-a",
            "posthog_key",
            "-w",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Why telemetry is off by a switch or a missing consent (not a missing key), or "".
pub fn blocked(user: &UserHome, today: NaiveDate) -> Result<String, AppError> {
    if !matches!(
        std::env::var("DO_NOT_TRACK").unwrap_or_default().trim(),
        "" | "0"
    ) {
        return Ok("DO_NOT_TRACK is set".into());
    }
    if !settings::get("telemetry.enabled")?.truthy() {
        return Ok("off (turn on: bin/cashcast telemetry on)".into());
    }
    if !consent::allowed(user, SCOPE, today)? {
        return Ok(format!(
            "no consent (the owner runs in a terminal: bin/cashcast consent grant {SCOPE})"
        ));
    }
    Ok(String::new())
}

pub fn enabled(user: &UserHome, today: NaiveDate) -> Result<(bool, String), AppError> {
    let why = blocked(user, today)?;
    if !why.is_empty() {
        return Ok((false, why));
    }
    if key().is_empty() {
        return Ok((false, "no key, so nothing is sent".into()));
    }
    Ok((true, "on".into()))
}

fn timestamp(ts: f64) -> Option<String> {
    let micros = (ts * 1e6).round_ties_even() as i64;
    let dt = chrono::DateTime::from_timestamp(micros.div_euclid(1_000_000), 0)?;
    Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// One log line as a PostHog event: allow-listed fields only, or None.
pub fn to_event(record: &Json, distinct_id: &str, version: &str) -> Option<Json> {
    let Json::Obj(items) = record else {
        return None;
    };
    let Some(Json::Str(name)) = record.get("event") else {
        return None;
    };
    if !is_id(name) {
        return None;
    }
    let mut props = vec![
        ("$process_person_profile".to_string(), Json::Bool(false)),
        ("$geoip_disable".to_string(), Json::Bool(true)),
        ("app_version".to_string(), Json::Str(version.into())),
    ];
    for (k, v) in items {
        if k == "event" || k == "ts" {
            continue;
        }
        let keep = match v {
            Json::Str(s) => STRING_KEYS.contains(&k.as_str()) && is_id(s),
            Json::Bool(_) | Json::Int(_) => is_num_key(k),
            Json::Float(f) => is_num_key(k) && f.is_finite(),
            _ => false,
        };
        // Anything else (service names, text, lists) never leaves the computer.
        if keep {
            match props.iter_mut().find(|(pk, _)| pk == k) {
                Some(slot) => slot.1 = v.clone(),
                None => props.push((k.clone(), v.clone())),
            }
        }
    }
    let mut out = vec![
        ("event".to_string(), Json::Str(format!("cashcast.{name}"))),
        ("distinct_id".to_string(), Json::Str(distinct_id.into())),
        ("properties".to_string(), Json::Obj(props)),
    ];
    let ts = match record.get("ts") {
        Some(Json::Int(i)) => Some(*i as f64),
        Some(Json::Float(f)) => Some(*f),
        _ => None,
    };
    if let Some(stamp) = ts.filter(|t| t.is_finite() && *t > 0.0).and_then(timestamp) {
        out.push(("timestamp".into(), Json::Str(stamp)));
    }
    Some(Json::Obj(out))
}

/// The app version: `version` in `engine/Cargo.toml`.
fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

fn cursor(st: &Json, name: &str) -> usize {
    match st.get("cursors").and_then(|c| c.get(name)) {
        Some(Json::Int(i)) => (*i).max(0) as usize,
        Some(Json::Float(f)) => f.max(0.0) as usize,
        Some(Json::Str(s)) => s.trim().parse::<i64>().map_or(0, |i| i.max(0) as usize),
        _ => 0,
    }
}

/// This user's events not sent yet (at most MAX_EVENTS_PER_SEND) and the log offset after them.
pub fn pending(user: &UserHome) -> Result<(Vec<Json>, usize), AppError> {
    let Ok(data) = std::fs::read(user.logs().join("events.jsonl")) else {
        return Ok((Vec::new(), 0));
    };
    let mut offset = cursor(&state(), &user.name);
    if offset > data.len() {
        offset = 0; // the log was replaced or cut: start again
    }
    let (distinct, version) = (install_id()?, version());
    let mut out = Vec::new();
    let mut pos = offset;
    while pos < data.len() && out.len() < MAX_EVENTS_PER_SEND {
        let Some(nl) = data[pos..].iter().position(|b| *b == b'\n') else {
            break;
        }; // a line still being written
        let end = pos + nl;
        let record = std::str::from_utf8(&data[pos..end])
            .ok()
            .and_then(pyjson::loads);
        pos = end + 1;
        if let Some(ev) = record
            .as_ref()
            .and_then(|r| to_event(r, &distinct, &version))
        {
            out.push(ev);
        }
    }
    Ok((out, pos))
}

/// The exact request the next send makes, with the key shown as a placeholder.
pub fn dry_run(user: &UserHome) -> Result<Json, AppError> {
    let (batch, _) = pending(user)?;
    let n = batch.len();
    let first: Vec<Json> = batch.into_iter().take(BATCH_SIZE).collect();
    Ok(Json::Obj(vec![
        ("url".into(), Json::Str(endpoint())),
        (
            "body".into(),
            Json::Obj(vec![
                ("api_key".into(), Json::Str("<posthog_key>".into())),
                ("batch".into(), Json::List(first)),
            ]),
        ),
        ("events_pending".into(), Json::Int(n as i64)),
    ]))
}

/// Sends this user's pending events now: (sent, status, why). Never fails a command.
pub fn send(user: &UserHome, today: NaiveDate) -> (usize, &'static str, String) {
    match enabled(user, today) {
        Ok((false, why)) => return (0, "off", why),
        Err(e) => return (0, "fail", e.kind.to_string()),
        Ok(_) => {}
    }
    let run = || -> Result<usize, AppError> {
        let api_key = key();
        let (batch, end) = pending(user)?;
        let mut sent = 0;
        for chunk in batch.chunks(BATCH_SIZE) {
            let body = Json::Obj(vec![
                ("api_key".into(), Json::Str(api_key.clone())),
                ("batch".into(), Json::List(chunk.to_vec())),
            ]);
            let headers = [
                ("Content-Type".to_string(), "application/json".to_string()),
                ("User-Agent".to_string(), "cashcast".to_string()),
            ];
            let r = crate::core::http::send(
                "POST",
                &endpoint(),
                &headers,
                Some(pyjson::dumps(&body, None, false).as_bytes()),
                10,
            )
            .map_err(|_| AppError::new("urlerror", "send failed"))?;
            // 400 and up is the server refusing; any other non-2xx is a transport problem.
            match r.status {
                200..=299 => {}
                400.. => return Err(AppError::new("httperror", "send failed")),
                _ => return Err(AppError::new("oserror", "send failed")),
            }
            sent += chunk.len();
        }
        let mut st = state();
        let mut cursors = st.get("cursors").cloned().unwrap_or(Json::Obj(Vec::new()));
        cursors.set(&user.name, Json::Int(end as i64));
        st.set("cursors", cursors);
        st.set("last_send_day", Json::Str(today.to_string()));
        save(&st)?;
        Ok(sent)
    };
    match run() {
        Ok(n) => (n, "ok", String::new()),
        Err(e) => {
            let name = match e.kind {
                "urlerror" => "URLError",
                "httperror" => "HTTPError",
                "oserror" => "OSError",
                other => other,
            };
            (0, "fail", name.to_string())
        }
    }
}

/// The start screen's one question, the same shape as careerbrew and agentbrew ask.
pub const ASK_QUESTION: &str = "cashcast: share anonymous usage counts?
  What: command names, ok or failed, durations and counts, under a random install id.
  Never: amounts, balances, payees, account or service names, file paths, host or user name.
  Where: PostHog EU (eu.i.posthog.com), at most once a day.
  See the exact body: bin/cashcast telemetry send --dry-run
  Stop any time: bin/cashcast telemetry off (or DO_NOT_TRACK=1)
Share anonymous usage counts? [y/n] ";
/// Asks at most this many times in all; a "no" ends the asks for good.
pub const MAX_ASKS: i64 = 3;

/// Why the start screen must not ask now, or None when it may.
/// `terminal`: a person at a real terminal (not an agent, not a pipe).
pub fn ask_blocked(
    user: &UserHome,
    today: NaiveDate,
    terminal: bool,
) -> Result<Option<&'static str>, AppError> {
    let env_on = |k: &str| !matches!(std::env::var(k).unwrap_or_default().trim(), "" | "0");
    if !terminal {
        return Ok(Some("no terminal"));
    }
    if env_on("CI") {
        return Ok(Some("CI"));
    }
    if env_on("DO_NOT_TRACK") {
        return Ok(Some("DO_NOT_TRACK is set"));
    }
    if !settings::get("telemetry.enabled")?.truthy() {
        return Ok(Some("telemetry is off"));
    }
    if consent::allowed(user, SCOPE, today)? {
        return Ok(Some("already yes"));
    }
    let st = state();
    if st.get("ask_answer").map(Json::py_str).as_deref() == Some("no") {
        return Ok(Some("answered no"));
    }
    if let Some(Json::Int(n)) = st.get("asks")
        && *n >= MAX_ASKS
    {
        return Ok(Some("asked 3 times"));
    }
    if st.get("last_ask_day").map(Json::py_str).as_deref() == Some(today.to_string().as_str()) {
        return Ok(Some("asked today"));
    }
    if key().is_empty() {
        return Ok(Some("no key, so nothing could be sent"));
    }
    Ok(None)
}

/// Asks once on `tty`. Some(true): yes, consent recorded. Some(false): no,
/// never asked again. None: no clear answer, asked again another day.
pub fn ask(
    user: &UserHome,
    tty: &mut dyn std::io::BufRead,
    out: &mut dyn std::io::Write,
    today: NaiveDate,
) -> Result<Option<bool>, AppError> {
    let mut st = state();
    let asks = match st.get("asks") {
        Some(Json::Int(n)) => *n,
        _ => 0,
    };
    st.set("asks", Json::Int(asks + 1));
    st.set("last_ask_day", Json::Str(today.to_string()));
    save(&st)?;
    let _ = write!(out, "{ASK_QUESTION}");
    let _ = out.flush();
    let mut line = String::new();
    let _ = tty.read_line(&mut line);
    let answer = match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    };
    let mut st = state();
    match answer {
        Some(true) => {
            consent::record(user, SCOPE, today, "telemetry-ask")?;
            st.set("ask_answer", Json::Str("yes".into()));
            let _ = writeln!(
                out,
                "Thanks. Turn it off any time: bin/cashcast telemetry off"
            );
        }
        Some(false) => {
            st.set("ask_answer", Json::Str("no".into()));
            let _ = writeln!(out, "OK, nothing is sent. cashcast will not ask again.");
        }
        None => {}
    }
    save(&st)?;
    events::log(
        user,
        "telemetry_ask",
        &[(
            "result",
            Json::Str(
                match answer {
                    Some(true) => "yes",
                    Some(false) => "no",
                    None => "skipped",
                }
                .into(),
            ),
        )],
    );
    Ok(answer)
}

pub struct Status {
    pub on: bool,
    pub why: String,
    pub pending: usize,
    pub last_send_day: Option<Json>,
}

pub fn status(user: &UserHome, today: NaiveDate) -> Result<Status, AppError> {
    let (on, why) = enabled(user, today)?;
    let (batch, _) = pending(user)?;
    let last = state().get("last_send_day").cloned();
    Ok(Status {
        on,
        why,
        pending: batch.len(),
        last_send_day: last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_keep_only_allow_listed_fields() {
        let rec = pyjson::loads(
            r#"{"command": "status", "event": "command", "ms": 12, "payee": "canary-shop", "result": "ok", "service": "bank", "ts": 1798000000.5}"#,
        )
        .unwrap();
        let ev = to_event(&rec, "id", "0.1.0").unwrap();
        assert_eq!(
            pyjson::dumps(&ev, None, false),
            r#"{"event": "cashcast.command", "distinct_id": "id", "properties": {"$process_person_profile": false, "$geoip_disable": true, "app_version": "0.1.0", "command": "status", "ms": 12, "result": "ok"}, "timestamp": "2026-12-23T04:26:40Z"}"#
        );
        assert!(
            to_event(
                &pyjson::loads(r#"{"event": "Bad Name"}"#).unwrap(),
                "id",
                "v"
            )
            .is_none()
        );
    }
}

pub const NOTICE: &str = "cashcast can send anonymous usage counts (command, ok or failed, duration; never amounts, names or paths) \
to PostHog EU, only with your consent net:usage-counts. Turn off: bin/cashcast telemetry off (or DO_NOT_TRACK=1).";

/// Whether the start screen's `maybe_send` would send on `day` (after its
/// notice).
pub fn start_would_send(
    user: &UserHome,
    day: NaiveDate,
    real: NaiveDate,
) -> Result<bool, AppError> {
    let st = state();
    let noticed = st.get("noticed").is_some_and(Json::truthy) || enabled(user, real)?.0;
    let last = st.get("last_send_day").map(Json::py_str);
    Ok(noticed
        && last.as_deref() != Some(day.to_string().as_str())
        && blocked(user, real)?.is_empty())
}

/// The one-line notice the first time telemetry could send; None after that or when it is off.
pub fn notice_once(user: &UserHome, real: NaiveDate) -> Option<&'static str> {
    let mut st = state();
    if st.get("noticed").is_some_and(Json::truthy) || !enabled(user, real).ok()?.0 {
        return None;
    }
    if let Json::Obj(items) = &mut st {
        items.retain(|(k, _)| k != "noticed");
        items.push(("noticed".into(), Json::Bool(true)));
    }
    save(&st).ok()?;
    Some(NOTICE)
}
