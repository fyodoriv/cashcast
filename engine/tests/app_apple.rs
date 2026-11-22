//! US-09 and US-16: one Apple Reminders list and one Apple Calendar per user.
//! Ported from tests/unit/test_reminders.py and test_apple_calendar.py. A fake
//! `osascript` shell script on PATH stands in for macOS.

mod app;

use app::{Env, d, grant_by_hand, read, set};
use cashcast_engine::core::pyjson::{Json, loads};
use cashcast_engine::strategy::Task;
use cashcast_engine::tasks::apple;
use cashcast_engine::tasks::connectors;

fn profile(text: &str) -> Json {
    loads(text).unwrap()
}

fn list(j: &Json, key: &str) -> Vec<Json> {
    match j.get(key) {
        Some(Json::List(v)) => v.clone(),
        _ => vec![],
    }
}

fn s(j: &Json, key: &str) -> String {
    j.get(key).map_or(String::new(), Json::py_str)
}

fn rem(u: &cashcast_engine::core::paths::UserHome, p: &Json, tasks: &[Task]) -> Json {
    apple::reminders_payload(u, p, tasks, &[], &[], None)
        .map_err(|e| e.0)
        .unwrap()
}

// ---------- reminders ----------

#[test]
fn each_user_has_their_own_list() {
    let env = Env::new();
    let (a, b) = (env.user("alex"), env.user("sam"));
    let pa = rem(&a, &profile(r#"{"display_name": "Alex"}"#), &[]);
    let pb = rem(
        &b,
        &profile(r#"{"display_name": "Sam", "reminders": {"list": "Money"}}"#),
        &[],
    );
    assert_eq!(
        (s(&pa, "list"), s(&pb, "list")),
        ("Cashcast · Alex".into(), "Money".into())
    );
}

#[test]
fn reminders_script_touches_only_the_named_list() {
    let s = apple::REMINDERS_SCRIPT;
    assert!(s.contains("predicateForRemindersInCalendars($.NSArray.arrayWithObject(list))"));
    assert!(s.contains("str(c.title) === data.list") && s.contains("list.title = data.list"));
    assert!(s.contains("[cashcast:") && s.contains("r.completed"));
    assert_eq!(s.matches("removeReminderCommitError(").count(), 1);
    assert!(s.contains("data.owner + ':'"));
}

#[test]
fn payload_marks_who_does_money_tasks() {
    let env = Env::new();
    let u = env.user("alex");
    let specs = connectors::load_all().map_err(|e| e.0).unwrap();
    let kraken = &specs.iter().find(|(id, _)| id == "kraken").unwrap().1;
    let money = list(kraken, "actions")
        .into_iter()
        .find(|a| s(a, "risk") == "money")
        .unwrap();
    app::cli(&["--user", "alex", "action", "kraken", &s(&money, "id")]);
    let mut t1 = Task::new("t1", "Pay card down")
        .due(d("2026-11-16"))
        .risk("money");
    t1.notes = "Transfer first.".into();
    let t2 = Task::new("t2", "Switch rent card").risk("settings");
    let data = rem(&u, &Json::Obj(vec![]), &[t1, t2]);
    let tasks = list(&data, "tasks");
    let by = |id: &str| tasks.iter().find(|t| s(t, "id") == id).unwrap().clone();
    assert!(
        s(&by("alex:t1"), "notes").contains("You do this")
            && s(&by("alex:t1"), "due") == "2026-11-16"
    );
    assert!(s(&by("alex:t2"), "notes").contains("Approve"));
    by(&format!("alex:kraken-{}", s(&money, "id")));
}

#[test]
fn task_ids_are_scoped_per_user_so_a_shared_list_cannot_mix_people() {
    let env = Env::new();
    let (a, b) = (env.user("alex"), env.user("sam"));
    let t = [Task::new("sweep-cash", "Invest")];
    let p = profile(r#"{"reminders": {"list": "Money"}}"#);
    let ia = s(&list(&rem(&a, &p, &t), "tasks")[0], "id");
    let ib = s(&list(&rem(&b, &p, &t), "tasks")[0], "id");
    assert!(ia != ib && ia.starts_with("alex:"));
}

// ---------- calendar ----------

#[test]
fn each_user_has_their_own_calendar() {
    let env = Env::new();
    let (a, b) = (env.user("alex"), env.user("sam"));
    let today = d("2026-10-21");
    let pa = apple::calendar_payload(&a, &profile(r#"{"display_name": "Alex"}"#), &[], today);
    let pb = apple::calendar_payload(
        &b,
        &profile(r#"{"calendar": {"name": "Money"}}"#),
        &[],
        today,
    );
    assert_eq!(
        (s(&pa, "calendar"), s(&pb, "calendar")),
        ("Cashcast · Alex".into(), "Money".into())
    );
}

#[test]
fn only_dated_tasks_from_a_week_ago_on_become_events() {
    let env = Env::new();
    let u = env.user("alex");
    let mut pay = Task::new("pay", "Pay card down")
        .due(d("2026-11-16"))
        .risk("money");
    pay.notes = "Transfer first.".into();
    let tasks = vec![
        Task::new("past", "CANARY past").due(d("2026-10-01")),
        Task::new("missed", "Missed yesterday").due(d("2026-10-20")),
        Task::new("undated", "CANARY undated"),
        pay,
        Task::new("switch", "Switch package")
            .due(d("2026-10-21"))
            .risk("settings"),
    ];
    let data = apple::calendar_payload(&u, &Json::Obj(vec![]), &tasks, d("2026-10-21"));
    let events = list(&data, "events");
    let mut ids: Vec<String> = events.iter().map(|e| s(e, "id")).collect();
    ids.sort();
    assert_eq!(ids, vec!["alex:missed", "alex:pay", "alex:switch"]);
    assert_eq!(s(&data, "owner"), "alex");
    let by = |id: &str| events.iter().find(|e| s(e, "id") == id).unwrap().clone();
    assert!(
        s(&by("alex:pay"), "notes").contains("You do this")
            && s(&by("alex:pay"), "date") == "2026-11-16"
    );
    assert!(s(&by("alex:switch"), "notes").contains("Approve"));
    assert_eq!(s(&by("alex:pay"), "alert_minutes"), (15 * 60).to_string());
}

#[test]
fn alert_days_come_from_the_profile() {
    let env = Env::new();
    let u = env.user("alex");
    let today = d("2026-10-21");
    let data = apple::calendar_payload(
        &u,
        &profile(r#"{"calendar": {"alert_days": 0}}"#),
        &[Task::new("t", "T").due(today)],
        today,
    );
    assert_eq!(s(&list(&data, "events")[0], "alert_minutes"), "0");
}

#[test]
fn calendar_script_touches_only_the_named_calendar_and_marked_events() {
    let s = apple::CALENDAR_SCRIPT;
    let calls: Vec<&str> = s
        .match_indices("app.calendars.")
        .map(|(i, _)| &s[i..i + 20])
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[0].starts_with("app.calendars.whose(") && calls[1].starts_with("app.calendars.push(")
    );
    assert!(s.contains("data.calendar") && s.contains("[cashcast:"));
    assert_eq!(s.matches("app.delete(").count(), 1);
    assert!(s.contains("app.delete(existing[id])") && s.contains("data.owner + ':'"));
}

// ---------- running osascript ----------

/// Puts a fake `osascript` first on PATH for this test; restores PATH on drop.
struct FakeOsa {
    old_path: String,
}

impl FakeOsa {
    fn new(env: &Env, body: &str) -> FakeOsa {
        use std::os::unix::fs::PermissionsExt;
        let bin = env.tmp.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("osascript");
        std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        set("PATH", &format!("{}:{old_path}", bin.display()));
        FakeOsa { old_path }
    }
}

impl Drop for FakeOsa {
    fn drop(&mut self) {
        set("PATH", &self.old_path);
    }
}

fn empty_list() -> Json {
    profile(
        r#"{"list": "Cashcast · Alex", "owner": "alex", "tasks": [{"id": "a", "title": "A", "due": null, "notes": ""}]}"#,
    )
}

fn empty_calendar() -> Json {
    profile(r#"{"calendar": "Cashcast · Alex", "owner": "alex", "events": []}"#)
}

#[test]
fn sync_runs_osascript_with_the_payload() {
    let env = Env::new();
    let u = env.user("alex");
    grant_by_hand(&u, &["local:reminders", "local:calendar"]);
    let seen = env.tmp.join("seen");
    let _osa = FakeOsa::new(
        &env,
        &format!(
            "echo \"$1 $2\" > {seen}.args; cat \"$4\" > {seen}.json\n\
             case \"$(cat \"$3\")\" in *EKReminder*) echo '{{\"created\": 2, \"updated\": 0}}';; *) echo '{{\"created\": 1, \"removed\": 2}}';; esac",
            seen = seen.display()
        ),
    );
    let out = apple::reminders_sync(&u, &empty_list()).unwrap();
    assert_eq!(apple::count(&out, "created"), 2);
    assert_eq!(read(&env.tmp.join("seen.args")).trim(), "-l JavaScript");
    assert!(read(&env.tmp.join("seen.json")).contains("\"list\": \"Cashcast "));
    let out = apple::calendar_sync(&u, &empty_calendar()).unwrap();
    assert_eq!(apple::count(&out, "removed"), 2);
    assert!(read(&env.tmp.join("seen.json")).contains("\"owner\": \"alex\""));
}

#[test]
fn writing_needs_consent() {
    let env = Env::new();
    let u = env.user("alex");
    let e = apple::reminders_sync(&u, &empty_list()).unwrap_err();
    assert!(e.message.contains("reminders"), "{}", e.message);
    let e = apple::calendar_sync(&u, &empty_calendar()).unwrap_err();
    assert!(e.message.contains("calendar"), "{}", e.message);
}

#[test]
fn sync_failure_shows_the_osascript_error_and_the_terminal_command() {
    let env = Env::new();
    let u = env.user("alex");
    grant_by_hand(&u, &["local:reminders", "local:calendar"]);
    let _osa = FakeOsa::new(
        &env,
        "echo 'sync.js: execution error: Error: AppleEvent timed out. (-1712)' >&2\nexit 1",
    );
    let e = apple::reminders_sync(&u, &empty_list()).unwrap_err();
    assert!(
        e.message.contains("AppleEvent timed out. (-1712)"),
        "{}",
        e.message
    );
    let e = apple::calendar_sync(&u, &empty_calendar()).unwrap_err();
    assert!(
        e.message.contains("bin/cashcast calendar sync"),
        "{}",
        e.message
    );
}

#[test]
fn sync_without_osascript_explains_instead_of_crashing() {
    let env = Env::new();
    let u = env.user("alex");
    grant_by_hand(&u, &["local:reminders"]);
    let old = std::env::var("PATH").unwrap_or_default();
    set("PATH", &env.tmp.join("empty").display().to_string());
    let e = apple::reminders_sync(&u, &empty_list()).unwrap_err();
    set("PATH", &old);
    assert!(e.message.contains("Terminal"), "{}", e.message);
}

// test_a_frozen_calendar_fails_fast / test_a_frozen_reminders_store_fails_fast:
// dropped. The Rust timeout is a private 90 s constant; a test would wait 90 s.
// The kill-and-explain path is in `apple::run_jxa`.
