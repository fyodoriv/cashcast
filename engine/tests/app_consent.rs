//! US-06 through the CLI: reports reach an agent only after the owner says
//! yes, an agent cannot grant itself, grants expire, telemetry needs consent
//! and a key; and the full gate queues itself behind heavy-run.
//! Ported from tests/e2e/test_us06_consent.py and tests/unit/test_gate_queue.py
//! (the unit assertions of test_consent.py live in app_users.rs).

mod app;

use app::{Env, cli, grant_by_hand, read, repo, set};
use cashcast_engine::core::consent;

fn out(r: &(i32, String, String)) -> String {
    format!("{}{}", r.1, r.2)
}

#[test]
fn us06_happy_owner_grant_shows_reports() {
    let env = Env::new();
    set("CASHCAST_AGENT", "1");
    set("CASHCAST_LOG", "on");
    let u = env.user("alex");
    std::fs::copy(
        repo().join("tests/parity/fixture/bank.csv"),
        u.inbox().join("bank.csv"),
    )
    .unwrap();
    let hidden = cli(&["--user", "alex", "import", "--today", "2026-10-18"]);
    let text = out(&hidden);
    assert!(text.contains("Report saved for the owner only"), "{text}");
    assert!(text.contains("consent grant agent:aggregates"), "{text}");
    assert!(u.out().join("last-import.md").exists());
    grant_by_hand(&u, &["agent:aggregates"]);
    let shown = out(&cli(&["--user", "alex", "import", "--today", "2026-10-18"]));
    assert!(
        !shown.contains("Report saved for the owner only"),
        "{shown}"
    );
    let log = read(&u.logs().join("events.jsonl"));
    assert!(
        log.contains("\"report_withheld\"") && log.contains("\"report_shown\""),
        "{log}"
    );
}

#[test]
fn us06_error_agent_cannot_grant_itself() {
    let env = Env::new();
    set("CASHCAST_AGENT", "1");
    env.user("alex");
    let r = cli(&["--user", "alex", "consent", "grant", "agent:aggregates"]);
    assert_ne!(r.0, 0);
    assert!(out(&r).contains("needs a real terminal"), "{}", out(&r));
    assert!(out(&cli(&["--user", "alex", "consent", "list"])).contains("not granted"));
    assert!(out(&cli(&["--user", "alex", "first-error"])).contains("agent:raw-row"));
}

#[test]
fn us06_grants_expire_and_scopes_are_known() {
    let env = Env::new();
    let u = env.user("alex");
    cashcast_engine::write_private(
        &u.consent(),
        "[[grants]]\nscope = \"agent:aggregates\"\ngranted_on = 2026-01-01\nexpires_on = 2026-06-01\nvia = \"hand\"\n",
    )
    .unwrap();
    assert!(consent::allowed(&u, "agent:aggregates", app::d("2026-05-31")).unwrap());
    assert!(!consent::allowed(&u, "agent:aggregates", app::d("2026-06-02")).unwrap());
    assert!(consent::scope("agent:aggregates").is_some());
    assert!(consent::scope("everything").is_none());
}

#[test]
fn us06_telemetry_sends_nothing_without_consent_and_key() {
    let env = Env::new();
    let u = env.user("alex");
    let run = |a: &[&str]| out(&cli(&[&["--user", "alex"], a].concat()));
    assert!(run(&["telemetry"]).contains("no consent"));
    assert!(run(&["telemetry", "send", "--dry-run"]).contains("<posthog_key>"));
    grant_by_hand(&u, &["net:usage-counts"]);
    assert!(run(&["telemetry", "send"]).contains("no key"));
    assert!(run(&["telemetry", "off"]).contains("turned off"));
    assert!(run(&["config"]).contains("telemetry.enabled = False"));
}

#[test]
fn the_full_gate_reruns_itself_through_heavy_run() {
    let tmp = std::env::temp_dir().join(format!("cashcast-gate-queue-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let calls = tmp.join("calls");
    let fake = tmp.join("heavy-run");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$@\" > \"{}\"\n", calls.display()),
    )
    .unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&fake)
        .status()
        .unwrap();
    let path = format!(
        "{}:{}",
        tmp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = std::process::Command::new("bash")
        .arg("scripts/check.sh")
        .current_dir(repo())
        .env("PATH", path)
        .env_remove("HEAVY_RUN_SLOT")
        .env_remove("BASH_ENV")
        .status()
        .unwrap();
    assert!(status.success());
    let called = std::fs::read_to_string(&calls).unwrap();
    assert_eq!(
        called.trim(),
        repo().join("scripts/check.sh").to_str().unwrap()
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn fast_and_changed_runs_and_runs_inside_a_slot_skip_the_queue() {
    let text = read(&repo().join("scripts/check.sh"));
    assert!(
        text.contains(
            r#"[ "$mode" != fast ] && [ "$mode" != changed ] && [ -z "${HEAVY_RUN_SLOT:-}" ]"#
        ),
        "the queue condition changed"
    );
}
