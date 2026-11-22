//! What the Reminders list and the calendar get, as the payload and preview
//! parts of `reminders.py` and `apple_calendar.py`. The syncs themselves
//! (osascript) stay in Python until they are ported.

use crate::core::events;
use crate::core::paths::{AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::strategy::Task;
use crate::strategy::py::{self, obj};
use crate::strategy::situation::Unported;
use chrono::{Duration, NaiveDate};

pub const MAX_REMOVE: i64 = 10;
const ALERT_DAYS: i64 = 1;

fn s(x: &str) -> Json {
    Json::Str(x.to_string())
}

fn own_name(user: &UserHome, profile: &Json, table: &str, key: &str) -> String {
    match obj(profile, table).get(key).filter(|v| v.truthy()) {
        Some(v) => v.py_str(),
        None => format!(
            "Cashcast · {}",
            profile
                .get("display_name")
                .map_or(user.name.clone(), Json::py_str)
        ),
    }
}

fn notes(t: &Task) -> String {
    let who = match t.risk.as_str() {
        "money" => "You do this: it moves money.",
        "settings" => "Approve before any agent does it.",
        _ => "",
    };
    [t.notes.as_str(), who]
        .iter()
        .filter(|x| !x.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `data/manual-tasks.json`: money steps a connector left for the person.
fn manual_tasks(user: &UserHome) -> Result<Vec<Json>, Unported> {
    let path = user.data().join("manual-tasks.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| Unported(e.to_string()))?;
    match pyjson::loads(&text) {
        Some(Json::List(items)) => Ok(items),
        _ => Err(Unported("manual-tasks.json is not a list".into())),
    }
}

/// `reminders.payload`: `done` are finished task ids, `open_ids` every open task.
pub fn reminders_payload(
    user: &UserHome,
    profile: &Json,
    tasks: &[Task],
    done: &[String],
    open_ids: &[String],
    since: Option<&str>,
) -> Result<Json, Unported> {
    let mut items: Vec<Json> = tasks
        .iter()
        .map(|t| {
            Json::Obj(vec![
                ("id".into(), Json::Str(format!("{}:{}", user.name, t.id))),
                ("title".into(), s(&t.title)),
                (
                    "due".into(),
                    t.due.map_or(Json::Null, |d| Json::Str(d.to_string())),
                ),
                ("notes".into(), Json::Str(notes(t))),
                ("kind".into(), s(&t.risk)),
            ])
        })
        .collect();
    for m in manual_tasks(user)? {
        let need = |k: &str| {
            m.get(k)
                .cloned()
                .ok_or_else(|| Unported(format!("manual task without {k}")))
        };
        items.push(Json::Obj(vec![
            (
                "id".into(),
                Json::Str(format!("{}:{}", user.name, need("id")?.py_str())),
            ),
            ("title".into(), need("title")?),
            ("due".into(), m.get("due").cloned().unwrap_or(Json::Null)),
            ("notes".into(), s("You do this: it moves money.")),
            ("kind".into(), s("money")),
        ]));
    }
    let listed: Vec<String> = items
        .iter()
        .map(|t| t.get("id").map_or(String::new(), Json::py_str))
        .collect();
    let mut open: Vec<Json> = listed.iter().map(|i| s(i)).collect();
    for i in open_ids {
        let id = format!("{}:{i}", user.name);
        if !listed.contains(&id) {
            open.push(Json::Str(id));
        }
    }
    Ok(Json::Obj(vec![
        (
            "list".into(),
            Json::Str(own_name(user, profile, "reminders", "list")),
        ),
        ("owner".into(), s(&user.name)),
        ("tasks".into(), Json::List(items)),
        (
            "done".into(),
            Json::List(
                done.iter()
                    .map(|d| Json::Str(format!("{}:{d}", user.name)))
                    .collect(),
            ),
        ),
        ("open".into(), Json::List(open)),
        ("since".into(), since.map_or(Json::Null, s)),
        ("max_remove".into(), Json::Int(MAX_REMOVE)),
    ]))
}

pub fn reminders_preview(data: &Json) -> String {
    let tasks = py::list(data, "tasks");
    let due = |t: &Json| t.get("due").filter(|d| d.truthy()).map(Json::py_str);
    let mut rows: Vec<&Json> = tasks.iter().collect();
    let key = |t: &Json| {
        (
            due(t).unwrap_or_else(|| "9999".into()),
            t.get("id").map_or(String::new(), Json::py_str),
        )
    };
    rows.sort_by_key(|a| key(a));
    let list = data.get("list").map_or(String::new(), Json::py_str);
    let mut lines = vec![
        format!("# Reminders for list '{list}' ({} tasks)", tasks.len()),
        String::new(),
    ];
    for t in rows {
        lines.push(format!(
            "- {}: {}",
            due(t).unwrap_or_else(|| "no date".into()),
            t.get("title").map_or(String::new(), Json::py_str)
        ));
    }
    lines.push(
        "\nTo write them to Apple Reminders run in a terminal: bin/cashcast reminders sync".into(),
    );
    lines.join("\n")
}

/// `apple_calendar.payload`: dated tasks from a week ago on.
pub fn calendar_payload(user: &UserHome, profile: &Json, tasks: &[Task], today: NaiveDate) -> Json {
    let days = obj(profile, "calendar")
        .get("alert_days")
        .and_then(|v| py::num(Some(v)))
        .map_or(ALERT_DAYS, |d| d.trunc() as i64);
    let events: Vec<Json> = tasks
        .iter()
        .filter_map(|t| {
            let due = t.due.filter(|d| *d >= today - Duration::days(7))?;
            Some(Json::Obj(vec![
                ("id".into(), Json::Str(format!("{}:{}", user.name, t.id))),
                ("title".into(), s(&t.title)),
                ("date".into(), Json::Str(due.to_string())),
                ("notes".into(), Json::Str(notes(t))),
                (
                    "alert_minutes".into(),
                    Json::Int(if days > 0 { (days * 24 - 9) * 60 } else { 0 }),
                ),
            ]))
        })
        .collect();
    Json::Obj(vec![
        (
            "calendar".into(),
            Json::Str(own_name(user, profile, "calendar", "name")),
        ),
        ("owner".into(), s(&user.name)),
        ("events".into(), Json::List(events)),
        ("max_remove".into(), Json::Int(MAX_REMOVE)),
    ])
}

pub fn calendar_preview(data: &Json) -> String {
    let events = py::list(data, "events");
    let text = |e: &Json, k: &str| e.get(k).map_or(String::new(), Json::py_str);
    let mut rows: Vec<&Json> = events.iter().collect();
    rows.sort_by_key(|e| (text(e, "date"), text(e, "id")));
    let name = text(data, "calendar");
    let mut lines = vec![
        format!(
            "# Calendar '{name}' ({} dated deadlines, from a week ago on)",
            events.len()
        ),
        String::new(),
    ];
    lines.extend(
        rows.iter()
            .map(|e| format!("- {}: {}", text(e, "date"), text(e, "title"))),
    );
    if events.is_empty() {
        lines.push("- No dated deadlines.".into());
    }
    lines.push(
        "\nTo write them to Apple Calendar run in a terminal: bin/cashcast calendar sync".into(),
    );
    lines.join("\n")
}

pub const REMINDERS_SCRIPT: &str = include_str!("../../data/reminders-sync.js");
pub const CALENDAR_SCRIPT: &str = include_str!("../../data/calendar-sync.js");
/// Seconds: a frozen Reminders or Calendar once made one run wait 9 minutes (2026-10-25).
const TIMEOUT: u64 = 90;

/// Runs one JXA sync script with `data` through `osascript` (port of
/// `reminders.sync` / `apple_calendar.sync`). `app` is "Reminders" or
/// "Calendar"; `command` is the CLI word for the retry hint. Returns the
/// script's JSON answer. The error text is printed only, never logged: it
/// may quote a title.
pub fn run_jxa(
    user: &UserHome,
    script: &str,
    data: &Json,
    app: &str,
    command: &str,
) -> Result<Json, AppError> {
    use std::process::{Command, Stdio};
    let event = format!("{command}_sync");
    let scope = if command == "reminders" {
        "local:reminders"
    } else {
        "local:calendar"
    };
    crate::core::consent::require(user, scope, chrono::Local::now().date_naive())?;
    let fail = |m: String| AppError::new("runtimeerror", m);
    let mut rand = [0u8; 8];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut rand))
        .map_err(|e| fail(e.to_string()))?;
    let dir = std::env::temp_dir().join(format!("cashcast-jxa-{}", crate::digest::hex(&rand)));
    std::fs::create_dir(&dir).map_err(|e| fail(e.to_string()))?;
    let (js, payload) = (dir.join("sync.js"), dir.join("data.json"));
    let written = crate::write_private(&js, script)
        .and_then(|_| crate::write_private(&payload, &pyjson::dumps(data, None, false)));
    let child = written.map_err(fail).and_then(|_| {
        Command::new("osascript")
            .args(["-l", "JavaScript"])
            .arg(&js)
            .arg(&payload)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    fail(format!(
                        "Apple {app} sync needs macOS (osascript). Run it in Terminal on your Mac."
                    ))
                } else {
                    fail(e.to_string())
                }
            })
    });
    let result = child.and_then(|mut child| {
        // Read both pipes while waiting, as `subprocess.run` does, so a large
        // answer never blocks the child into a false timeout.
        let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut buf);
                }
                buf
            })
        };
        let out_t = drain(child.stdout.take().map(|p| Box::new(p) as _));
        let err_t = drain(child.stderr.take().map(|p| Box::new(p) as _));
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|e| fail(e.to_string()))? {
                return Ok(std::process::Output {
                    status,
                    stdout: out_t.join().unwrap_or_default(),
                    stderr: err_t.join().unwrap_or_default(),
                });
            }
            if start.elapsed().as_secs() >= TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                events::log(user, &event, &[("result", Json::Str("timeout".into()))]);
                return Err(fail(format!(
                    "{app} is not responding (no answer in {TIMEOUT} s): quit and reopen it, then run bin/cashcast sync all"
                )));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
    let out = result?;
    if !out.status.success() {
        events::log(user, &event, &[("result", Json::Str("failed".into()))]);
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err.lines().filter(|l| !l.trim().is_empty()).last();
        let why = why
            .map(|l| format!(" ({})", l.trim().chars().take(300).collect::<String>()))
            .unwrap_or_default();
        return Err(fail(format!(
            "osascript failed{why}. If an AI agent's sandbox blocked it, run in Terminal: bin/cashcast {command} sync. \
             macOS may ask once to let Terminal control {app}."
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let answer = pyjson::loads(if text.is_empty() { "{}" } else { &text })
        .ok_or_else(|| fail("osascript answered with something that is not JSON".into()))?;
    Ok(answer)
}

/// `reminders.sync`: writes the list and logs the counts.
pub fn reminders_sync(user: &UserHome, data: &Json) -> Result<Json, AppError> {
    let out = run_jxa(user, REMINDERS_SCRIPT, data, "Reminders", "reminders")?;
    let n = |k: &str| Json::Int(count(&out, k));
    let completed = match out.get("completed") {
        Some(Json::List(v)) => v.len() as i64,
        _ => 0,
    };
    events::log(
        user,
        "reminders_sync",
        &[
            ("result", Json::Str("ok".into())),
            ("created", n("created")),
            ("updated", n("updated")),
            ("closed", n("closed")),
            ("removed", n("removed")),
            ("held", n("held")),
            ("reopened", n("reopened")),
            ("completed", Json::Int(completed)),
        ],
    );
    Ok(out)
}

/// `apple_calendar.sync`: writes the calendar and logs the counts.
pub fn calendar_sync(user: &UserHome, data: &Json) -> Result<Json, AppError> {
    let out = run_jxa(user, CALENDAR_SCRIPT, data, "Calendar", "calendar")?;
    let n = |k: &str| Json::Int(count(&out, k));
    events::log(
        user,
        "calendar_sync",
        &[
            ("result", Json::Str("ok".into())),
            ("created", n("created")),
            ("updated", n("updated")),
            ("removed", n("removed")),
        ],
    );
    Ok(out)
}

/// `int(out.get(key, 0))` for the sync counts.
pub fn count(out: &Json, key: &str) -> i64 {
    match out.get(key) {
        Some(Json::Int(i)) => *i,
        Some(Json::Float(f)) => *f as i64,
        _ => 0,
    }
}
