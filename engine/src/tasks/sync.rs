//! Keeps Apple Reminders, Apple Calendar and the Notion tasks database in
//! step, as `src/cashcast/sync.py`: `sync all` (one pass over every target
//! that is on, run again when one reports new ticks), `sync status` (drift
//! without touching any app), and `sync install` / `uninstall` (the hourly
//! LaunchAgent). `data/sync-state.json` holds each target's last run.

use super::{apple, changes, notion};
use crate::core::paths::{self, AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::core::{consent, events};
use crate::strategy::Task;
use crate::strategy::situation::Unported;
use chrono::{Duration, NaiveDate, NaiveDateTime};

const TARGETS: [&str; 3] = ["reminders", "calendar", "notion"];
const STALE_HOURS: i64 = 3;
const INTERVAL: i64 = 3600; // seconds between LaunchAgent runs
const BOOTSTRAP_TRIES: usize = 4;

pub fn notion_database(user: &UserHome) -> Result<String, Unported> {
    let path = user.config();
    if !path.exists() {
        return Ok(String::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| Unported(e.to_string()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| Unported(e.to_string()))?;
    let cfg = crate::strategy::py::from_toml(&toml::Value::Table(table));
    Ok(crate::strategy::py::obj(&cfg, "notion")
        .get("tasks_database")
        .map_or(String::new(), Json::py_str)
        .trim()
        .to_string())
}

/// Target → "" when on, else the step that turns it on.
fn enabled(
    user: &UserHome,
    real_today: NaiveDate,
) -> Result<Vec<(&'static str, String)>, AppError> {
    let back = |e: Unported| AppError::new("crash", e.0);
    let step = |scope: &str| format!("run in Terminal: bin/cashcast consent grant {scope}");
    let on = |scope: &str| consent::allowed(user, scope, real_today);
    let notion = if notion_database(user).map_err(back)?.is_empty() {
        "add [notion] tasks_database = \"<database id>\" to cashcast.toml (docs/decisions/0009)"
            .to_string()
    } else if !on("cloud:notion")? {
        step("cloud:notion")
    } else {
        String::new()
    };
    Ok(vec![
        (
            "reminders",
            if on("local:reminders")? {
                String::new()
            } else {
                step("local:reminders")
            },
        ),
        (
            "calendar",
            if on("local:calendar")? {
                String::new()
            } else {
                step("local:calendar")
            },
        ),
        ("notion", notion),
    ])
}

pub fn read_state(user: &UserHome) -> Json {
    std::fs::read_to_string(user.data().join("sync-state.json"))
        .ok()
        .and_then(|t| pyjson::loads(&t))
        .unwrap_or(Json::Obj(Vec::new()))
}

/// The drift hash of a payload without its run bookkeeping.
fn digest(data: &Json) -> String {
    let content = match data {
        Json::Obj(items) => Json::Obj(
            items
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "since" | "closed_by_sync" | "max_remove"))
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    };
    let text = pyjson::dumps(&content, None, true);
    crate::digest::hex(&crate::digest::sha256(text.as_bytes()))[..16].to_string()
}

fn parse_at(s: &str) -> Option<NaiveDateTime> {
    [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ]
    .iter()
    .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
}

/// (in sync, lines).
pub fn status(
    user: &UserHome,
    profile: &Json,
    tasks: &[Task],
    today: NaiveDate,
    real_today: NaiveDate,
) -> Result<(bool, Vec<String>), AppError> {
    let back = |e: Unported| AppError::new("crash", e.0);
    let now = chrono::Local::now().naive_local();
    let on = enabled(user, real_today)?;
    let state = read_state(user);
    let mut done: Vec<String> = changes::done(user)
        .map_err(back)?
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    done.sort();
    let listed: Vec<Task> = tasks
        .iter()
        .filter(|t| t.due.is_none_or(|d| d >= today - Duration::days(7)))
        .cloned()
        .collect();
    let open_ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let reminders =
        apple::reminders_payload(user, profile, &listed, &done, &open_ids, None).map_err(back)?;
    let calendar = apple::calendar_payload(user, profile, tasks, today);
    let (mut ok, mut lines) = (true, Vec::new());
    for name in TARGETS {
        let off = &on.iter().find(|(n, _)| *n == name).expect("every target").1;
        if !off.is_empty() {
            lines.push(format!("- {name}: off ({off})"));
            continue;
        }
        let Some(st) = state.get(name).filter(|s| s.truthy()) else {
            ok = false;
            lines.push(format!("- {name}: never synced"));
            continue;
        };
        let at = st
            .get("at")
            .and_then(|a| parse_at(&a.py_str()))
            .ok_or_else(|| AppError::new("crash", "sync-state time"))?;
        let mut problems = Vec::new();
        if st.get("result").map(Json::py_str).as_deref() != Some("ok") {
            problems.push(format!(
                "last run failed: {}",
                st.get("error").map_or(String::new(), Json::py_str)
            ));
        }
        if let Some(held) = st.get("held").filter(|h| h.truthy()) {
            problems.push(format!(
                "{} removals held: check the plan, then run bin/cashcast sync all --allow-removals",
                held.py_str()
            ));
        }
        let data = if name == "calendar" {
            &calendar
        } else {
            &reminders
        };
        if st.get("hash").map(Json::py_str) != Some(digest(data)) {
            problems.push("tasks changed since the last sync".into());
        }
        let when = at.format("%a %b %-d %H:%M").to_string();
        if now - at > Duration::hours(STALE_HOURS) {
            problems.push(format!("last synced {when}"));
        }
        ok = ok && problems.is_empty();
        lines.push(format!(
            "- {name}: {}",
            if problems.is_empty() {
                format!("in sync (last {when})")
            } else {
                problems.join("; ")
            }
        ));
    }
    if on.iter().all(|(_, off)| !off.is_empty()) {
        ok = false;
        lines.push("- Nothing is on yet: turn on at least one target above.".into());
    }
    let agent = agent_path(user, None);
    lines.push(format!(
        "- hourly sync: {}",
        if agent.exists() {
            "installed"
        } else {
            "not installed (run in Terminal: bin/cashcast sync install)"
        }
    ));
    Ok((ok, lines))
}

/// What one `sync all` printed, and what it found.
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub newly_done: Vec<String>,
    pub failed: Vec<String>,
}

/// What each target gets. `since` is that target's last good sync (else 30
/// days ago): older ticks are not new.
fn payloads(
    user: &UserHome,
    profile: &Json,
    tasks: &[Task],
    today: NaiveDate,
    state: &Json,
    allow_removals: bool,
) -> Result<Vec<(&'static str, Json)>, AppError> {
    let back = |e: Unported| AppError::new("crash", e.0);
    let mut done: Vec<String> = changes::done(user)
        .map_err(back)?
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    done.sort();
    let listed: Vec<Task> = tasks
        .iter()
        .filter(|t| t.due.is_none_or(|d| d >= today - Duration::days(7)))
        .cloned()
        .collect();
    let fallback = (today - Duration::days(30))
        .format("%Y-%m-%dT00:00:00")
        .to_string();
    let since = |name: &str| {
        state
            .get(name)
            .and_then(|s| s.get("ok_at"))
            .filter(|v| v.truthy())
            .map_or(fallback.clone(), Json::py_str)
    };
    let open_ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let mut out = Vec::new();
    for name in TARGETS {
        let mut data = if name == "calendar" {
            apple::calendar_payload(user, profile, tasks, today)
        } else {
            let mut d = apple::reminders_payload(
                user,
                profile,
                &listed,
                &done,
                &open_ids,
                Some(&since(name)),
            )
            .map_err(back)?;
            let closed = state
                .get(name)
                .and_then(|s| s.get("closed_ids"))
                .cloned()
                .unwrap_or(Json::List(Vec::new()));
            d.set("closed_by_sync", closed);
            d
        };
        if allow_removals {
            data.set("max_remove", Json::Int(1_000_000));
        }
        out.push((name, data));
    }
    Ok(out)
}

fn held(out: &Json) -> String {
    match apple::count(out, "held") {
        0 => String::new(),
        n => format!(
            "; {n} removals held (more than {} at once: check the plan, then run sync all --allow-removals)",
            apple::MAX_REMOVE
        ),
    }
}

/// `out.get(key, 0)` as Python prints it.
fn raw(out: &Json, key: &str) -> String {
    out.get(key).map_or("0".into(), Json::py_str)
}

fn strings(v: Option<&Json>) -> Vec<String> {
    match v {
        Some(Json::List(items)) => items.iter().map(Json::py_str).collect(),
        _ => Vec::new(),
    }
}

/// One target's sync: (ticked ids, summary, held, closed ids).
fn sync_one(
    user: &UserHome,
    name: &str,
    data: &Json,
) -> Result<(Vec<String>, String, i64, Json), AppError> {
    match name {
        "reminders" => {
            let out = apple::reminders_sync(user, data)?;
            let msg = format!(
                "{} added, {} updated, {} reopened, {} closed, {} removed{}",
                raw(&out, "created"),
                raw(&out, "updated"),
                raw(&out, "reopened"),
                raw(&out, "closed"),
                raw(&out, "removed"),
                held(&out)
            );
            let closed = out
                .get("closed_ids")
                .cloned()
                .unwrap_or(Json::List(Vec::new()));
            Ok((
                strings(out.get("completed")),
                msg,
                apple::count(&out, "held"),
                closed,
            ))
        }
        "calendar" => {
            let out = apple::calendar_sync(user, data)?;
            let msg = format!(
                "{} added, {} updated, {} removed{}",
                raw(&out, "created"),
                raw(&out, "updated"),
                raw(&out, "removed"),
                held(&out)
            );
            let closed = out
                .get("closed_ids")
                .cloned()
                .unwrap_or(Json::List(Vec::new()));
            Ok((Vec::new(), msg, apple::count(&out, "held"), closed))
        }
        _ => {
            let db = notion_database(user).map_err(|e| AppError::new("crash", e.0))?;
            let out = notion::sync(user, &db, data)?;
            let n = out.held;
            let held = if n == 0 {
                String::new()
            } else {
                format!(
                    "; {n} removals held (more than {} at once: check the plan, then run sync all --allow-removals)",
                    apple::MAX_REMOVE
                )
            };
            let msg = format!(
                "{} added, {} updated, {} reopened, {} closed, {} archived{held}",
                out.created, out.updated, out.reopened, out.closed, out.removed
            );
            let closed = Json::List(out.closed_ids.into_iter().map(Json::Str).collect());
            Ok((out.completed, msg, n, closed))
        }
    }
}

fn stamp(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// One `sync all`. `open_tasks` is called on each pass, after new ticks are recorded.
pub fn run(
    user: &UserHome,
    profile: &Json,
    open_tasks: &mut dyn FnMut() -> Result<Vec<Task>, AppError>,
    today: NaiveDate,
    now: &dyn Fn() -> NaiveDateTime,
    allow_removals: bool,
) -> Result<Report, AppError> {
    let back = |e: Unported| AppError::new("crash", e.0);
    let mut rep = Report::default();
    let on = enabled(user, chrono::Local::now().date_naive())?;
    let mut state = match read_state(user) {
        s @ Json::Obj(_) => s,
        _ => Json::Obj(Vec::new()),
    };
    for (name, off) in &on {
        if !off.is_empty() {
            rep.lines.push(format!("- {name}: off ({off})"));
        }
    }
    let active: Vec<&str> = on
        .iter()
        .filter(|(_, off)| off.is_empty())
        .map(|(n, _)| *n)
        .collect();
    if active.is_empty() {
        rep.lines
            .push("- Nothing to sync yet: turn on at least one target above.".into());
        return Ok(rep);
    }
    for _ in 0..2 {
        let tasks = open_tasks()?;
        let data = payloads(user, profile, &tasks, today, &state, allow_removals)?;
        let started = stamp(now());
        let mut ticked: Vec<String> = Vec::new();
        for name in &active {
            let payload = &data
                .iter()
                .find(|(n, _)| n == name)
                .expect("every target")
                .1;
            match sync_one(user, name, payload) {
                Ok((got, msg, held, closed)) => {
                    ticked.extend(got);
                    state.set(
                        name,
                        Json::Obj(vec![
                            ("at".into(), Json::Str(started.clone())),
                            ("ok_at".into(), Json::Str(started.clone())),
                            ("result".into(), Json::Str("ok".into())),
                            ("hash".into(), Json::Str(digest(payload))),
                            ("summary".into(), Json::Str(msg)),
                            ("held".into(), Json::Int(held)),
                            ("closed_ids".into(), closed),
                        ]),
                    );
                }
                Err(e) if matches!(e.kind, "runtimeerror" | "apierror" | "consenterror") => {
                    let mut entry = match state.get(name) {
                        Some(s @ Json::Obj(_)) => s.clone(),
                        _ => Json::Obj(Vec::new()),
                    };
                    entry.set("at", Json::Str(stamp(now())));
                    entry.set("result", Json::Str("failed".into()));
                    entry.set("error", Json::Str(e.message));
                    state.set(name, entry);
                    if !rep.failed.iter().any(|f| f == name) {
                        rep.failed.push(name.to_string());
                    }
                }
                Err(e) => return Err(e),
            }
        }
        let known = changes::done(user).map_err(back)?;
        let prefix = format!("{}:", user.name);
        let mut new: Vec<(String, Option<String>)> = Vec::new();
        for t in &tasks {
            let ticked_here = ticked
                .iter()
                .any(|i| i.strip_prefix(&prefix) == Some(t.id.as_str()));
            if ticked_here
                && changes::is_open(&known, t, today).map_err(back)?
                && !new.iter().any(|(i, _)| *i == t.id)
            {
                new.push((t.id.clone(), t.due.map(|d| d.to_string())));
            }
        }
        if new.is_empty() {
            break;
        }
        new.sort();
        changes::record_done(user, &new, today)?;
        rep.newly_done.extend(new.into_iter().map(|(i, _)| i));
    }
    save_state(user, &state)?;
    for name in &active {
        let s = state.get(name).cloned().unwrap_or(Json::Obj(Vec::new()));
        let text = |k: &str| s.get(k).map_or(String::new(), Json::py_str);
        rep.lines.push(format!(
            "- {name}: {}",
            if text("result") == "ok" {
                text("summary")
            } else {
                format!("FAILED: {}", text("error"))
            }
        ));
    }
    if !rep.newly_done.is_empty() {
        rep.lines.push(format!(
            "- Ticked done elsewhere and now done everywhere: {} task(s).",
            rep.newly_done.len()
        ));
    }
    events::log(
        user,
        "sync",
        &[
            (
                "result",
                Json::Str(
                    if rep.failed.is_empty() {
                        "ok"
                    } else {
                        "failed"
                    }
                    .into(),
                ),
            ),
            ("targets", Json::Int(active.len() as i64)),
            ("done", Json::Int(rep.newly_done.len() as i64)),
        ],
    );
    Ok(rep)
}

fn save_state(user: &UserHome, state: &Json) -> Result<(), AppError> {
    let io = |e: String| AppError::new("oserror", e);
    std::fs::create_dir_all(user.data()).map_err(|e| io(e.to_string()))?;
    crate::write_private(
        &user.data().join("sync-state.json"),
        &pyjson::dumps(state, Some(1), true),
    )
    .map_err(io)
}

// ---------------------------------------------------------------- LaunchAgent

fn label(user: &UserHome) -> String {
    format!("ca.cashcast.sync.{}", user.name)
}

pub fn agent_path(user: &UserHome, agents_dir: Option<&std::path::Path>) -> std::path::PathBuf {
    let dir = agents_dir.map_or_else(
        || {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join("Library/LaunchAgents")
        },
        std::path::Path::to_path_buf,
    );
    dir.join(format!("{}.plist", label(user)))
}

/// The LaunchAgent, in plistlib's XML with its sorted keys.
pub fn plist_xml(user: &UserHome) -> Result<Vec<u8>, AppError> {
    use plist::{Dictionary, Value};
    let log = Value::String(user.logs().join("sync.log").display().to_string());
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    let program = [
        paths::root()
            .join("bin")
            .join("cashcast")
            .display()
            .to_string(),
        "--user".into(),
        user.name.clone(),
        "sync".into(),
        "all".into(),
    ];
    let mut env = Dictionary::new();
    env.insert("PATH".into(), Value::String(path));
    let mut d = Dictionary::new();
    d.insert("EnvironmentVariables".into(), Value::Dictionary(env));
    d.insert("Label".into(), Value::String(label(user)));
    d.insert(
        "ProgramArguments".into(),
        Value::Array(program.into_iter().map(Value::String).collect()),
    );
    d.insert("RunAtLoad".into(), Value::Boolean(true));
    d.insert("StandardErrorPath".into(), log.clone());
    d.insert("StandardOutPath".into(), log);
    d.insert("StartInterval".into(), Value::Integer(INTERVAL.into()));
    let mut out = Vec::new();
    Value::Dictionary(d)
        .to_writer_xml(&mut out)
        .map_err(|e| AppError::new("oserror", e.to_string()))?;
    out.push(b'\n');
    Ok(out)
}

/// Runs a command quietly; true when it exits 0.
pub type Runner<'a> = &'a dyn Fn(&[String]) -> bool;

pub fn launchctl(args: &[String]) -> bool {
    std::process::Command::new(&args[0])
        .args(&args[1..])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn uid() -> u32 {
    unsafe extern "C" {
        safe fn getuid() -> u32;
    }
    getuid()
}

/// Writes the LaunchAgent and loads it. Re-running replaces it.
pub fn install(
    user: &UserHome,
    agents_dir: Option<&std::path::Path>,
    runner: Runner,
    sleep: &dyn Fn(),
) -> Result<String, AppError> {
    let io = |e: std::io::Error| AppError::new("oserror", e.to_string());
    let path = agent_path(user, agents_dir);
    std::fs::create_dir_all(path.parent().expect("a folder")).map_err(io)?;
    std::fs::create_dir_all(user.logs()).map_err(io)?;
    std::fs::write(&path, plist_xml(user)?).map_err(io)?;
    let domain = format!("gui/{}", uid());
    let p = path.display().to_string();
    runner(&[
        "launchctl".into(),
        "bootout".into(),
        format!("{domain}/{}", label(user)),
    ]);
    let mut loaded = false;
    for attempt in 0..BOOTSTRAP_TRIES {
        // bootout returns before launchd has let go of the old job
        loaded = runner(&[
            "launchctl".into(),
            "bootstrap".into(),
            domain.clone(),
            p.clone(),
        ]);
        if loaded {
            break;
        }
        if attempt < BOOTSTRAP_TRIES - 1 {
            sleep();
        }
    }
    events::log(
        user,
        "sync_install",
        &[(
            "result",
            Json::Str(if loaded { "ok" } else { "failed" }.into()),
        )],
    );
    if !loaded {
        return Err(AppError::new(
            "runtimeerror",
            format!(
                "launchctl could not load {p}. Run in Terminal: launchctl bootstrap {domain} {p}"
            ),
        ));
    }
    Ok(format!(
        "Installed {}: sync all runs every hour and at login. Log: {}",
        path.file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned()),
        user.logs().join("sync.log").display()
    ))
}

pub fn uninstall(user: &UserHome, agents_dir: Option<&std::path::Path>, runner: Runner) -> String {
    let path = agent_path(user, agents_dir);
    runner(&[
        "launchctl".into(),
        "bootout".into(),
        format!("gui/{}/{}", uid(), label(user)),
    ]);
    let existed = path.exists();
    let _ = std::fs::remove_file(&path);
    events::log(
        user,
        "sync_install",
        &[("result", Json::Str("removed".into()))],
    );
    if existed {
        format!(
            "Removed {}.",
            path.file_name()
                .map_or(String::new(), |n| n.to_string_lossy().into_owned())
        )
    } else {
        "The hourly sync was not installed.".into()
    }
}

#[cfg(test)]
mod tests {
    //! US-21, as tests/unit/test_sync.py: the real JXA scripts run against the
    //! fake Reminders and Calendar in `tests/fakes/jxa_reminders_calendar.js` (through a fake
    //! `osascript` on PATH); Notion is a fake HTTP transport. Synthetic tasks only.
    use super::*;
    use crate::core::http::{self, Response};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    const DB: &str = "db-canary";

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
    }

    fn tasks() -> Vec<Task> {
        let mut pay = Task::new("pay-card", "Pay the card $1,234.56 before the due date")
            .due(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap());
        pay.notes = "CANARY note".into();
        pay.risk = "money".into();
        let mut switch = Task::new("switch-package", "Switch the bank package");
        switch.risk = "settings".into();
        vec![
            pay,
            Task::new(
                "rrsp-limit-2026",
                "Copy your 2026 RRSP deduction limit into cashcast",
            )
            .due(today()),
            switch,
        ]
    }

    /// A user folder with every target on. `name` is unique per test: the
    /// fake apps keep their state by owner.
    fn person(name: &str, scopes: &[&str]) -> UserHome {
        let home = std::env::temp_dir().join(format!("cc-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let grants: String = scopes
            .iter()
            .map(|s| format!("[[grants]]\nscope = \"{s}\"\ngranted_on = 2026-10-18\nexpires_on = 2099-01-01\nvia = \"hand\"\n\n"))
            .collect();
        std::fs::write(home.join("consent.toml"), grants).unwrap();
        std::fs::write(
            home.join("cashcast.toml"),
            format!("[notion]\ntasks_database = \"{DB}\"\n"),
        )
        .unwrap();
        std::fs::write(home.join(".env"), "CASHCAST_NOTION_TOKEN=tok-canary\n").unwrap();
        let state = fake_dir();
        for which in ["reminders", "calendar"] {
            let _ = std::fs::remove_file(state.join(format!("{name}-{which}.json")));
        }
        UserHome::new(name, home)
    }

    const ALL: [&str; 3] = ["local:reminders", "local:calendar", "cloud:notion"];

    fn fake_dir() -> PathBuf {
        std::env::temp_dir().join("cashcast-fake-apps")
    }

    /// Puts a fake `osascript` first on PATH, once. None when node is missing.
    fn fake_osascript() -> Option<()> {
        static SET: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let ok = *SET.get_or_init(|| {
            let has_node = std::process::Command::new("node").arg("--version").output().is_ok();
            if !has_node {
                return false;
            }
            let bin = std::env::temp_dir().join(format!("cc-fake-osascript-{}", std::process::id()));
            std::fs::create_dir_all(&bin).unwrap();
            let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fakes/jxa_reminders_calendar.js");
            let script = format!(
                r#"#!/usr/bin/env node
const fs = require('fs'), os = require('os'), path = require('path'), cp = require('child_process');
const [script, payload] = process.argv.slice(4);
const which = fs.readFileSync(script, 'utf8').includes('EKReminder') ? 'reminders' : 'calendar';
const owner = JSON.parse(fs.readFileSync(payload, 'utf8')).owner;
const dir = path.join(os.tmpdir(), 'cashcast-fake-apps');
fs.mkdirSync(dir, {{ recursive: true }});
const state = path.join(dir, owner + '-' + which + '.json');
if (!fs.existsSync(state)) fs.writeFileSync(state, JSON.stringify(which === 'reminders' ? {{ lists: {{}} }} : {{ calendars: {{}} }}));
const out = JSON.parse(cp.execFileSync(process.execPath, [{fake:?}, script, payload, state], {{ encoding: 'utf8' }}));
fs.writeFileSync(state, JSON.stringify(out.state));
process.stdout.write(JSON.stringify(out.result));
"#,
                fake = fake.display().to_string()
            );
            let exe = bin.join("osascript");
            std::fs::write(&exe, script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
            // SAFETY: set once, before any test spawns osascript; other tests only gain a dir.
            unsafe { std::env::set_var("PATH", path) };
            true
        });
        ok.then_some(())
    }

    fn app_state(name: &str, which: &str) -> Json {
        let text =
            std::fs::read_to_string(fake_dir().join(format!("{name}-{which}.json"))).unwrap();
        pyjson::loads(&text).unwrap()
    }

    fn save_app_state(name: &str, which: &str, state: &Json) {
        std::fs::write(
            fake_dir().join(format!("{name}-{which}.json")),
            pyjson::dumps(state, None, false),
        )
        .unwrap();
    }

    fn reminders(name: &str) -> Vec<Json> {
        let s = app_state(name, "reminders");
        let list = s
            .get("lists")
            .and_then(|l| l.get(&format!("Cashcast · {name}")))
            .cloned();
        match list {
            Some(Json::List(items)) => items,
            _ => Vec::new(),
        }
    }

    fn event_ids(name: &str) -> Vec<String> {
        let s = app_state(name, "calendar");
        let cal = s
            .get("calendars")
            .and_then(|l| l.get(&format!("Cashcast · {name}")))
            .cloned();
        match cal {
            Some(Json::List(items)) => items
                .iter()
                .map(|e| e.get("description").map_or(String::new(), Json::py_str))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A fake Notion API: one database, pages by id.
    #[derive(Default)]
    struct Notion {
        pages: Vec<(String, Json)>,
        props: Option<Json>,
        sent: Vec<String>,
    }

    fn page_json(pid: &str, p: &Json) -> Json {
        let get = |k: &str| p.get(k).cloned().unwrap_or(Json::Null);
        let text = |k: &str| Json::List(vec![Json::Obj(vec![("plain_text".into(), get(k))])]);
        Json::Obj(vec![
            ("id".into(), Json::Str(pid.into())),
            ("archived".into(), get("archived")),
            (
                "last_edited_time".into(),
                p.get("edited").cloned().unwrap_or(Json::Str(String::new())),
            ),
            (
                "properties".into(),
                Json::Obj(vec![
                    (
                        "cashcast id".into(),
                        Json::Obj(vec![("rich_text".into(), text("cid"))]),
                    ),
                    (
                        "Done".into(),
                        Json::Obj(vec![("checkbox".into(), get("done"))]),
                    ),
                    (
                        "Name".into(),
                        Json::Obj(vec![("title".into(), text("title"))]),
                    ),
                    (
                        "Due".into(),
                        Json::Obj(vec![(
                            "date".into(),
                            match p.get("due").filter(|d| d.truthy()) {
                                Some(d) => Json::Obj(vec![("start".into(), d.clone())]),
                                None => Json::Null,
                            },
                        )]),
                    ),
                    (
                        "Kind".into(),
                        Json::Obj(vec![(
                            "select".into(),
                            match p.get("kind").filter(|k| k.truthy()) {
                                Some(k) => Json::Obj(vec![("name".into(), k.clone())]),
                                None => Json::Null,
                            },
                        )]),
                    ),
                ]),
            ),
        ])
    }

    fn install_notion(notion: &Rc<RefCell<Notion>>) {
        let n = Rc::clone(notion);
        http::set_transport(Some(Box::new(move |method, url, headers, body| {
            let mut me = n.borrow_mut();
            assert!(
                headers
                    .iter()
                    .any(|(k, v)| k == "Authorization" && v == "Bearer tok-canary")
            );
            let text = body
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default();
            me.sent.push(text.clone());
            let data = pyjson::loads(&text).unwrap_or(Json::Null);
            let path = url
                .strip_prefix(&format!("{}/", notion::API))
                .unwrap()
                .to_string();
            let reply = |j: Json| {
                Ok(Response {
                    status: 200,
                    body: pyjson::dumps(&j, None, false).into_bytes(),
                })
            };
            if method == "GET" && path == format!("databases/{DB}") {
                let props = me.props.clone().unwrap_or_else(|| {
                    Json::Obj(
                        notion::PROPS
                            .iter()
                            .map(|(k, v)| {
                                (
                                    k.to_string(),
                                    Json::Obj(vec![("type".into(), Json::Str(v.to_string()))]),
                                )
                            })
                            .collect(),
                    )
                });
                return reply(Json::Obj(vec![("properties".into(), props)]));
            }
            if method == "POST" && path == format!("databases/{DB}/query") {
                let results = me.pages.iter().map(|(pid, p)| page_json(pid, p)).collect();
                return reply(Json::Obj(vec![
                    ("results".into(), Json::List(results)),
                    ("has_more".into(), Json::Bool(false)),
                ]));
            }
            let props = data
                .get("properties")
                .cloned()
                .unwrap_or(Json::Obj(Vec::new()));
            let content = |k: &str, inner: &str| {
                props
                    .get(k)
                    .and_then(|p| p.get(inner))
                    .and_then(|l| match l {
                        Json::List(items) => items
                            .first()
                            .and_then(|x| x.get("text"))
                            .and_then(|t| t.get("content"))
                            .cloned(),
                        _ => None,
                    })
            };
            let apply = |p: &mut Json| {
                if let Some(t) = content("Name", "title") {
                    p.set("title", t);
                }
                if let Some(c) = content("cashcast id", "rich_text") {
                    p.set("cid", c);
                }
                if let Some(d) = props.get("Done").and_then(|d| d.get("checkbox")) {
                    p.set("done", d.clone());
                }
                if let Some(d) = props.get("Due").and_then(|d| d.get("date")) {
                    p.set("due", d.get("start").cloned().unwrap_or(Json::Null));
                }
                if let Some(k) = props.get("Kind").and_then(|k| k.get("select")) {
                    p.set("kind", k.get("name").cloned().unwrap_or(Json::Null));
                }
            };
            if method == "POST" && path == "pages" {
                let pid = format!("page-{}", me.pages.len());
                let mut p = Json::Obj(vec![("archived".into(), Json::Bool(false))]);
                apply(&mut p);
                me.pages.push((pid, p));
                return reply(Json::Obj(Vec::new()));
            }
            if method == "PATCH" && path.starts_with("pages/") {
                let pid = path["pages/".len()..].to_string();
                let p = &mut me.pages.iter_mut().find(|(k, _)| *k == pid).unwrap().1;
                p.set("edited", Json::Str("2099-01-01T00:00:00.000Z".into()));
                if data.get("archived").is_some_and(Json::truthy) {
                    p.set("archived", Json::Bool(true));
                }
                apply(p);
                return reply(Json::Obj(Vec::new()));
            }
            panic!("unexpected {method} {url}");
        })));
    }

    /// Live pages: cid → (done, title).
    fn live(notion: &Rc<RefCell<Notion>>) -> Vec<(String, bool, String)> {
        notion
            .borrow()
            .pages
            .iter()
            .filter(|(_, p)| !p.get("archived").is_some_and(Json::truthy))
            .map(|(_, p)| {
                let s = |k: &str| p.get(k).map_or(String::new(), Json::py_str);
                (
                    s("cid"),
                    p.get("done").is_some_and(Json::truthy),
                    s("title"),
                )
            })
            .collect()
    }

    fn sync_all(user: &UserHome, list: &dyn Fn() -> Vec<Task>, allow: bool) -> Report {
        let mut open = || -> Result<Vec<Task>, AppError> {
            let finished = changes::done(user).unwrap();
            Ok(list()
                .into_iter()
                .filter(|t| changes::is_open(&finished, t, today()).unwrap())
                .collect())
        };
        let now = || chrono::Local::now().naive_local();
        run(
            user,
            &Json::Obj(Vec::new()),
            &mut open,
            today(),
            &now,
            allow,
        )
        .unwrap()
    }

    fn open_reminders(name: &str) -> Vec<String> {
        reminders(name)
            .iter()
            .filter(|r| !r.get("completed").is_some_and(Json::truthy))
            .map(|r| r.get("body").map_or(String::new(), Json::py_str))
            .collect()
    }

    #[test]
    fn one_sync_puts_the_open_tasks_everywhere_and_a_tick_finishes_it_everywhere() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncone", &ALL);
        let notion = Rc::new(RefCell::new(Notion::default()));
        install_notion(&notion);
        let rep = sync_all(&u, &tasks, false);
        assert!(rep.failed.is_empty(), "{:?}", rep.lines);
        assert_eq!(open_reminders("syncone").len(), 3);
        assert_eq!(live(&notion).len(), 3);
        let events = event_ids("syncone");
        assert_eq!(events.len(), 2, "only dated tasks go to the calendar");
        let (ok, lines) = status(
            &u,
            &Json::Obj(Vec::new()),
            &tasks(),
            today(),
            chrono::Local::now().date_naive(),
        )
        .unwrap();
        assert!(ok, "{lines:?}");
        // Notion never gets amounts or notes.
        let sent = notion.borrow().sent.join("\n");
        assert!(!sent.contains("1,234") && !sent.contains("CANARY"));
        assert!(
            live(&notion)
                .iter()
                .any(|(c, _, t)| c == "syncone:pay-card"
                    && t == "Pay the card … before the due date")
        );
        // A tick in Reminders finishes the task everywhere.
        let mut state = app_state("syncone", "reminders");
        if let Some(Json::Obj(lists)) = match &mut state {
            Json::Obj(items) => items.iter_mut().find(|(k, _)| k == "lists").map(|(_, v)| v),
            _ => None,
        } {
            for (_, list) in lists.iter_mut() {
                if let Json::List(items) = list {
                    for r in items.iter_mut() {
                        if r.get("body")
                            .map(Json::py_str)
                            .is_some_and(|b| b.contains("pay-card"))
                        {
                            r.set("completed", Json::Bool(true));
                        }
                    }
                }
            }
        }
        save_app_state("syncone", "reminders", &state);
        let rep = sync_all(&u, &tasks, false);
        assert_eq!(rep.newly_done, vec!["pay-card".to_string()]);
        assert!(
            changes::done(&u)
                .unwrap()
                .iter()
                .any(|(k, _)| k == "pay-card")
        );
        assert!(!event_ids("syncone").iter().any(|d| d.contains("pay-card")));
        assert!(
            live(&notion)
                .iter()
                .any(|(c, done, _)| c == "syncone:pay-card" && *done),
            "Notion shows it done"
        );
        let saved = std::fs::read_to_string(u.data().join("sync-state.json")).unwrap();
        assert!(
            saved.starts_with("{\n \"calendar\": {\n  \"at\": "),
            "indent=1, sorted keys"
        );
        http::set_transport(None);
    }

    #[test]
    fn a_plan_that_suddenly_shrinks_holds_the_removals_until_allowed() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncheld", &ALL);
        let notion = Rc::new(RefCell::new(Notion::default()));
        install_notion(&notion);
        let many = || {
            (0..15)
                .map(|i| Task::new(format!("t{i}"), format!("Task {i}")))
                .collect::<Vec<_>>()
        };
        sync_all(&u, &many, false);
        let rep = sync_all(&u, &Vec::new, false);
        assert_eq!(reminders("syncheld").len(), 15);
        assert_eq!(live(&notion).len(), 15);
        assert!(
            rep.lines.join("\n").contains("removals held"),
            "{:?}",
            rep.lines
        );
        let (ok, lines) = status(
            &u,
            &Json::Obj(Vec::new()),
            &[],
            today(),
            chrono::Local::now().date_naive(),
        )
        .unwrap();
        assert!(!ok && lines.iter().any(|l| l.contains("held")));
        sync_all(&u, &Vec::new, true);
        assert!(reminders("syncheld").is_empty() && live(&notion).is_empty());
        http::set_transport(None);
    }

    #[test]
    fn off_targets_say_how_to_turn_them_on() {
        let u = person("syncoff", &[]);
        std::fs::remove_file(u.config()).unwrap();
        let rep = sync_all(&u, &tasks, false);
        let text = rep.lines.join("\n");
        assert!(text.contains("consent grant local:reminders") && text.contains("tasks_database"));
        assert!(text.contains("Nothing to sync"));
        assert!(!u.data().join("sync-state.json").exists());
    }

    #[test]
    fn a_failed_target_is_reported_and_the_others_still_run() {
        let notion = Rc::new(RefCell::new(Notion {
            props: Some(Json::Obj(vec![(
                "Name".into(),
                Json::Obj(vec![("type".into(), Json::Str("title".into()))]),
            )])),
            ..Notion::default()
        }));
        install_notion(&notion);
        let only_notion = person("syncfail", &["cloud:notion"]);
        let rep = sync_all(&only_notion, &tasks, false);
        assert_eq!(rep.failed, vec!["notion".to_string()]);
        let state = read_state(&only_notion);
        let err = state
            .get("notion")
            .and_then(|n| n.get("error"))
            .map(Json::py_str)
            .unwrap();
        assert!(
            err.contains("property 'Due' must be date (missing)"),
            "{err}"
        );
        assert!(
            rep.lines
                .iter()
                .any(|l| l.starts_with("- notion: FAILED: the Notion tasks database does not fit"))
        );
        http::set_transport(None);
    }

    #[test]
    fn install_writes_plistlibs_xml_and_retries_while_launchd_holds_the_old_job() {
        let u = person("syncagent", &[]);
        let dir = u.home.join("agents");
        let tries = RefCell::new(Vec::new());
        let runner = |cmd: &[String]| {
            if cmd[1] == "bootstrap" {
                tries.borrow_mut().push(cmd.to_vec());
                return tries.borrow().len() >= 3;
            }
            true
        };
        let msg = install(&u, Some(&dir), &runner, &|| {}).unwrap();
        assert!(msg.contains("every hour"), "{msg}");
        assert_eq!(tries.borrow().len(), 3);
        let path = dir.join("ca.cashcast.sync.syncagent.plist");
        let xml = std::fs::read_to_string(&path).unwrap();
        let log = u.logs().join("sync.log").display().to_string();
        let want = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>PATH</key>\n\t\t<string>{}</string>\n\t</dict>\n\t<key>Label</key>\n\t<string>ca.cashcast.sync.syncagent</string>\n\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t\t<string>--user</string>\n\t\t<string>syncagent</string>\n\t\t<string>sync</string>\n\t\t<string>all</string>\n\t</array>\n\t<key>RunAtLoad</key>\n\t<true/>\n\t<key>StandardErrorPath</key>\n\t<string>{log}</string>\n\t<key>StandardOutPath</key>\n\t<string>{log}</string>\n\t<key>StartInterval</key>\n\t<integer>3600</integer>\n</dict>\n</plist>\n",
            // PATH is the environment's at install time (another test may prepend to it).
            xml.split("<key>PATH</key>\n\t\t<string>")
                .nth(1)
                .and_then(|r| r.split("</string>").next())
                .unwrap(),
            paths::root().join("bin/cashcast").display()
        );
        assert_eq!(xml, want);
        let never = |_: &[String]| false;
        let e = install(&u, Some(&dir), &never, &|| {}).unwrap_err();
        assert!(
            e.message
                .contains("Run in Terminal: launchctl bootstrap gui/"),
            "{}",
            e.message
        );
        assert_eq!(
            uninstall(&u, Some(&dir), &never),
            "Removed ca.cashcast.sync.syncagent.plist."
        );
        assert!(!path.exists());
        assert_eq!(
            uninstall(&u, Some(&dir), &never),
            "The hourly sync was not installed."
        );
    }

    // ---------- more of tests/unit/test_sync.py ----------

    fn at(h: u32, m: u32, s: u32) -> NaiveDateTime {
        today().and_hms_opt(h, m, s).unwrap()
    }

    /// One `sync all` at a fixed clock, listing only tasks still open on `day`.
    fn sync_at(
        user: &UserHome,
        list: &dyn Fn() -> Vec<Task>,
        day: NaiveDate,
        now: NaiveDateTime,
        allow: bool,
    ) -> Report {
        let mut open = || -> Result<Vec<Task>, AppError> {
            let finished = changes::done(user).unwrap();
            Ok(list()
                .into_iter()
                .filter(|t| changes::is_open(&finished, t, day).unwrap())
                .collect())
        };
        run(
            user,
            &Json::Obj(Vec::new()),
            &mut open,
            day,
            &move || now,
            allow,
        )
        .unwrap()
    }

    fn edit_reminders(name: &str, f: &dyn Fn(&mut Json)) {
        let mut state = app_state(name, "reminders");
        if let Json::Obj(items) = &mut state {
            if let Some((_, Json::Obj(lists))) = items.iter_mut().find(|(k, _)| k == "lists") {
                for (_, list) in lists.iter_mut() {
                    if let Json::List(rs) = list {
                        rs.iter_mut().for_each(f);
                    }
                }
            }
        }
        save_app_state(name, "reminders", &state);
    }

    fn seed_reminders(name: &str, items: &str) {
        std::fs::create_dir_all(fake_dir()).unwrap();
        let state = format!(r#"{{"lists": {{"Cashcast · {name}": {items}}}}}"#);
        save_app_state(name, "reminders", &pyjson::loads(&state).unwrap());
    }

    fn reminder(name: &str, id: &str) -> Json {
        reminders(name)
            .into_iter()
            .find(|r| {
                r.get("body")
                    .map(Json::py_str)
                    .is_some_and(|b| b.contains(id))
            })
            .unwrap()
    }

    fn done_flag(r: &Json) -> bool {
        r.get("completed").is_some_and(Json::truthy)
    }

    fn pages(text: &str) -> Vec<(String, Json)> {
        match pyjson::loads(text).unwrap() {
            Json::Obj(items) => items,
            _ => panic!(),
        }
    }

    fn late() -> Vec<Task> {
        let mut t = Task::new("late-one", "Pay the late bill")
            .due(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());
        t.risk = "money".into();
        vec![t]
    }

    fn events(name: &str) -> Vec<Json> {
        let s = app_state(name, "calendar");
        match s
            .get("calendars")
            .and_then(|c| c.get(&format!("Cashcast · {name}")))
            .cloned()
        {
            Some(Json::List(items)) => items,
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_tick_in_notion_completes_the_reminder_and_drops_the_event() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncntick", &ALL);
        let notion = Rc::new(RefCell::new(Notion::default()));
        install_notion(&notion);
        sync_at(&u, &tasks, today(), at(9, 0, 0), false);
        for (_, p) in notion.borrow_mut().pages.iter_mut() {
            if p.get("cid").map(Json::py_str).as_deref() == Some("syncntick:rrsp-limit-2026") {
                p.set("done", Json::Bool(true));
            }
        }
        let rep = sync_at(&u, &tasks, today(), at(10, 0, 0), false);
        assert_eq!(rep.newly_done, vec!["rrsp-limit-2026".to_string()]);
        assert!(done_flag(&reminder("syncntick", "rrsp-limit-2026")));
        assert!(
            !event_ids("syncntick")
                .iter()
                .any(|d| d.contains("rrsp-limit-2026"))
        );
        http::set_transport(None);
    }

    #[test]
    fn a_task_that_left_the_plan_leaves_every_target_but_other_items_stay() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncleft", &ALL);
        seed_reminders(
            "syncleft",
            r#"[{"name": "Old task", "body": "[cashcast:syncleft:old-task]", "completed": false},
                {"name": "Sam's task", "body": "[cashcast:sam:x]", "completed": false},
                {"name": "Buy milk", "body": "", "completed": false}]"#,
        );
        let notion = Rc::new(RefCell::new(Notion {
            pages: pages(
                r#"{"p-old": {"cid": "syncleft:old-task", "done": false, "archived": false}}"#,
            ),
            ..Notion::default()
        }));
        install_notion(&notion);
        sync_at(&u, &tasks, today(), at(9, 0, 0), false);
        let names: Vec<String> = reminders("syncleft")
            .iter()
            .map(|r| r.get("name").map_or(String::new(), Json::py_str))
            .collect();
        assert!(!names.contains(&"Old task".to_string()), "{names:?}");
        assert!(
            names.contains(&"Sam's task".to_string()) && names.contains(&"Buy milk".to_string())
        );
        assert!(
            notion.borrow().pages[0]
                .1
                .get("archived")
                .is_some_and(Json::truthy)
        );
        http::set_transport(None);
    }

    #[test]
    fn status_reports_drift_after_the_plan_changes_or_a_stale_sync() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncdrift", &ALL);
        install_notion(&Rc::new(RefCell::new(Notion::default())));
        let real = chrono::Local::now();
        sync_at(&u, &tasks, today(), real.naive_local(), false);
        let mut more = tasks();
        more.push(Task::new("new-one", "A new task"));
        let empty = Json::Obj(Vec::new());
        let (ok, lines) = status(&u, &empty, &more, today(), real.date_naive()).unwrap();
        assert!(
            !ok && lines.iter().any(|l| l.contains("tasks changed")),
            "{lines:?}"
        );
        sync_at(
            &u,
            &tasks,
            today(),
            real.naive_local() - Duration::days(1),
            false,
        );
        let (ok, lines) = status(&u, &empty, &tasks(), today(), real.date_naive()).unwrap();
        assert!(
            !ok && lines.iter().any(|l| l.contains("last synced")),
            "{lines:?}"
        );
        http::set_transport(None);
    }

    #[test]
    fn reminders_payload_names_the_owner_and_the_done_ids() {
        let u = person("syncpay", &[]);
        let empty = Json::Obj(Vec::new());
        let data = apple::reminders_payload(
            &u,
            &empty,
            &tasks(),
            &["switch-package".to_string()],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(
            data.get("owner").map(Json::py_str).as_deref(),
            Some("syncpay")
        );
        let Some(Json::List(done)) = data.get("done") else {
            panic!()
        };
        assert_eq!(
            done.iter().map(Json::py_str).collect::<Vec<_>>(),
            vec!["syncpay:switch-package"]
        );
        let Some(Json::List(items)) = data.get("tasks") else {
            panic!()
        };
        let mut kinds: Vec<String> = items
            .iter()
            .map(|t| t.get("kind").map_or(String::new(), Json::py_str))
            .collect();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds, vec!["money", "read", "settings"]);
        let cal = apple::calendar_payload(&u, &empty, &tasks(), today());
        assert_eq!(
            cal.get("owner").map(Json::py_str).as_deref(),
            Some("syncpay")
        );
    }

    #[test]
    fn an_old_tick_never_closes_a_task_that_is_open_again() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncold", &ALL);
        seed_reminders(
            "syncold",
            r#"[{"name": "Decide RRSP", "body": "[cashcast:syncold:rrsp-limit-2026]", "completed": true, "completed_at": "2026-02-01T10:00:00"}]"#,
        );
        let notion = Rc::new(RefCell::new(Notion {
            pages: pages(
                r#"{"p1": {"cid": "syncold:rrsp-limit-2026", "done": true, "archived": false, "edited": "2026-02-01T10:00:00.000Z"}}"#,
            ),
            ..Notion::default()
        }));
        install_notion(&notion);
        let rep = sync_at(&u, &tasks, today(), at(9, 0, 0), false);
        assert!(rep.newly_done.is_empty());
        assert!(
            !changes::done(&u)
                .unwrap()
                .iter()
                .any(|(k, _)| k == "rrsp-limit-2026")
        );
        assert!(
            !done_flag(&reminder("syncold", "rrsp-limit-2026")),
            "reopened"
        );
        assert!(
            !notion.borrow().pages[0]
                .1
                .get("done")
                .is_some_and(Json::truthy),
            "the old tick is cleared"
        );
        http::set_transport(None);
    }

    #[test]
    fn a_fresh_tick_after_the_last_sync_counts() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncfresh", &ALL);
        install_notion(&Rc::new(RefCell::new(Notion::default())));
        sync_at(&u, &tasks, today(), at(9, 0, 0), false);
        edit_reminders("syncfresh", &|r| {
            if r.get("body")
                .map(Json::py_str)
                .is_some_and(|b| b.contains("pay-card"))
            {
                r.set("completed", Json::Bool(true));
                r.set("completed_at", Json::Str("2026-10-25T09:30:00".into()));
            }
        });
        let rep = sync_at(&u, &tasks, today(), at(10, 0, 0), false);
        assert_eq!(rep.newly_done, vec!["pay-card".to_string()]);
        http::set_transport(None);
    }

    #[test]
    fn an_open_task_more_than_a_week_overdue_keeps_its_reminder_and_page() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("synclate", &ALL);
        let notion = Rc::new(RefCell::new(Notion::default()));
        install_notion(&notion);
        sync_at(
            &u,
            &late,
            NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
            at(9, 0, 0),
            false,
        );
        sync_at(&u, &late, today(), at(9, 0, 0), false);
        reminder("synclate", "late-one");
        assert!(
            live(&notion)
                .iter()
                .any(|(c, _, _)| c == "synclate:late-one")
        );
        http::set_transport(None);
    }

    #[test]
    fn a_tick_is_for_one_due_date_and_holds_for_30_days() {
        let u = person("syncticks", &[]);
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let rrsp = |due| Task::new("rrsp-deadline", "Decide your RRSP contribution").due(due);
        changes::record_done(
            &u,
            &[("rrsp-deadline".into(), Some("2027-03-01".into()))],
            today(),
        )
        .unwrap();
        let store = changes::done(&u).unwrap();
        assert!(!changes::is_open(&store, &rrsp(day(2027, 3, 1)), today()).unwrap());
        assert!(changes::is_open(&store, &rrsp(day(2028, 3, 1)), today()).unwrap());
        changes::mark(&u, "switch-package", today(), false).unwrap();
        let by_hand = Task::new("switch-package", "x").due(day(2030, 1, 1));
        assert!(!changes::is_open(&changes::done(&u).unwrap(), &by_hand, today()).unwrap());

        changes::record_done(
            &u,
            &[
                ("sweep-cash".into(), Some("2026-10-25".into())),
                ("keep-active-card".into(), None),
            ],
            today(),
        )
        .unwrap();
        let store = changes::done(&u).unwrap();
        let daily = |d| Task::new("sweep-cash", "Invest the extra cash").due(d);
        assert!(!changes::is_open(&store, &daily(day(2026, 10, 26)), day(2026, 10, 26)).unwrap());
        assert!(changes::is_open(&store, &daily(day(2026, 11, 24)), day(2026, 11, 24)).unwrap());
        let undated = Task::new("keep-active-card", "Use the card once");
        assert!(!changes::is_open(&store, &undated, day(2026, 11, 1)).unwrap());
        assert!(changes::is_open(&store, &undated, day(2026, 11, 25)).unwrap());
    }

    #[test]
    fn cashcasts_own_close_is_not_read_back_as_a_tick_after_undo() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncundo", &ALL);
        let notion = Rc::new(RefCell::new(Notion::default()));
        install_notion(&notion);
        sync_at(&u, &tasks, today(), at(9, 0, 0), false);
        changes::mark(&u, "pay-card", today(), false).unwrap();
        sync_at(&u, &tasks, today(), at(10, 0, 0), false);
        assert!(done_flag(&reminder("syncundo", "pay-card")));
        edit_reminders("syncundo", &|r| {
            if r.get("body")
                .map(Json::py_str)
                .is_some_and(|b| b.contains("pay-card"))
            {
                r.set("completed_at", Json::Str("2026-10-25T10:00:30".into()));
            }
        });
        changes::mark(&u, "pay-card", today(), true).unwrap();
        let rep = sync_at(&u, &tasks, today(), at(10, 30, 0), false);
        assert!(rep.newly_done.is_empty());
        assert!(
            !changes::done(&u)
                .unwrap()
                .iter()
                .any(|(k, _)| k == "pay-card")
        );
        assert!(!done_flag(&reminder("syncundo", "pay-card")));
        assert!(
            live(&notion)
                .iter()
                .any(|(c, d, _)| c == "syncundo:pay-card" && !d)
        );
        http::set_transport(None);
    }

    #[test]
    fn a_tick_on_a_reminder_more_than_a_week_overdue_counts() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("synclatetick", &ALL);
        install_notion(&Rc::new(RefCell::new(Notion::default())));
        sync_at(
            &u,
            &late,
            NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
            at(9, 0, 0),
            false,
        );
        edit_reminders("synclatetick", &|r| {
            r.set("completed", Json::Bool(true));
            r.set("completed_at", Json::Str("2026-10-25T09:30:00".into()));
        });
        let rep = sync_at(&u, &late, today(), at(10, 0, 0), false);
        assert_eq!(rep.newly_done, vec!["late-one".to_string()]);
        http::set_transport(None);
    }

    #[test]
    fn an_old_tick_on_an_overdue_open_task_reopens_the_reminder() {
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncreopen", &ALL);
        seed_reminders(
            "syncreopen",
            r#"[{"name": "Pay the late bill", "body": "[cashcast:syncreopen:late-one]", "completed": true, "completed_at": "2026-08-01T10:00:00"}]"#,
        );
        install_notion(&Rc::new(RefCell::new(Notion::default())));
        let rep = sync_at(&u, &late, today(), at(9, 0, 0), false);
        assert!(rep.newly_done.is_empty());
        assert!(!done_flag(&reminders("syncreopen")[0]));
        http::set_transport(None);
    }

    #[test]
    fn calendar_events_get_one_cashcast_alert_and_a_quiet_run_writes_nothing() {
        // Calendar on macOS 27 refuses to delete an alert, and Reminders hung
        // under repeated writes (2026-10-25): a quiet hour must change nothing.
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncquiet", &ALL);
        let notion = Rc::new(RefCell::new(Notion::default()));
        install_notion(&notion);
        let rep = sync_at(&u, &tasks, today(), at(9, 0, 0), false);
        assert!(rep.failed.is_empty(), "{:?}", rep.lines);
        let writes = |n: &Rc<RefCell<Notion>>| {
            n.borrow()
                .sent
                .iter()
                .filter(|s| s.contains("\"properties\""))
                .count()
        };
        let before = writes(&notion);
        let rep = sync_at(&u, &tasks, today(), at(10, 0, 0), false);
        assert!(rep.failed.is_empty(), "{:?}", rep.lines);
        for name in ["reminders", "calendar", "notion"] {
            assert!(
                rep.lines
                    .iter()
                    .any(|l| l.starts_with(&format!("- {name}: 0 added, 0 updated"))),
                "{name}: {:?}",
                rep.lines
            );
        }
        assert_eq!(writes(&notion), before);
        // ALERT_DAYS = 1: one day before, at 9:00.
        let ours = (-(24 - 9) * 60).to_string();
        for e in events("syncquiet") {
            let Some(Json::List(alarms)) = e.get("alarms") else {
                panic!()
            };
            assert_eq!(alarms.iter().filter(|a| a.py_str() == ours).count(), 1);
        }
        http::set_transport(None);
    }

    #[test]
    fn a_task_moved_later_moves_its_calendar_event() {
        // Calendar refused to save an event whose new start was after its old end (-10025, 2026-10-30).
        if fake_osascript().is_none() {
            return;
        }
        let u = person("syncmove", &ALL);
        install_notion(&Rc::new(RefCell::new(Notion::default())));
        let first = || vec![tasks().into_iter().next().unwrap()];
        sync_at(&u, &first, today(), at(9, 0, 0), false);
        let due = first()[0].due.unwrap() + Duration::days(5);
        let later = move || vec![first()[0].clone().due(due)];
        let rep = sync_at(&u, &later, today(), at(10, 0, 0), false);
        assert!(rep.failed.is_empty(), "{:?}", rep.lines);
        let starts: Vec<String> = events("syncmove")
            .iter()
            .map(|e| e.get("start").unwrap().py_str()[..10].to_string())
            .collect();
        assert_eq!(starts, vec![due.to_string()]);
        http::set_transport(None);
    }
}
