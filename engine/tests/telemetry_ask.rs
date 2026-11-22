//! The start screen's telemetry ask (US-06): a person at a real terminal is
//! asked y/n at most 3 times; a yes records consent, a no ends the asks; it
//! never asks an agent, a pipe, CI, or under DO_NOT_TRACK; and a missing key
//! means there is nothing to ask about. Synthetic users only.

mod app;

use app::{Env, d, set};
use cashcast_engine::core::{consent, telemetry};

fn ask(
    u: &cashcast_engine::core::paths::UserHome,
    answer: &str,
    day: &str,
) -> (Option<bool>, String) {
    let mut tty = std::io::Cursor::new(answer.as_bytes().to_vec());
    let mut out = Vec::new();
    let r = telemetry::ask(u, &mut tty, &mut out, d(day)).unwrap();
    (r, String::from_utf8(out).unwrap())
}

#[test]
fn yes_records_consent_and_stops_asking() {
    let env = Env::new();
    set("CI", ""); // GitHub runners set CI=true
    set("CASHCAST_POSTHOG_KEY", "phc_test_canary");
    let u = env.user("alex");
    let day = d("2026-11-22");
    assert_eq!(telemetry::ask_blocked(&u, day, true).unwrap(), None);
    let (r, out) = ask(&u, "y\n", "2026-11-22");
    assert_eq!(r, Some(true));
    assert!(out.contains("Never: amounts, balances, payees"), "{out}");
    assert!(out.contains("telemetry send --dry-run"), "{out}");
    assert!(consent::allowed(&u, telemetry::SCOPE, day).unwrap());
    assert_eq!(
        telemetry::ask_blocked(&u, d("2026-11-23"), true).unwrap(),
        Some("already yes")
    );
    assert!(telemetry::enabled(&u, day).unwrap().0);
}

#[test]
fn no_ends_the_asks_and_sends_nothing() {
    let env = Env::new();
    set("CI", ""); // GitHub runners set CI=true
    set("CASHCAST_POSTHOG_KEY", "phc_test_canary");
    let u = env.user("alex");
    let (r, _) = ask(&u, "no\n", "2026-11-22");
    assert_eq!(r, Some(false));
    assert!(!consent::allowed(&u, telemetry::SCOPE, d("2026-11-22")).unwrap());
    assert_eq!(
        telemetry::ask_blocked(&u, d("2026-12-30"), true).unwrap(),
        Some("answered no")
    );
    assert!(!telemetry::enabled(&u, d("2026-11-22")).unwrap().0);
}

#[test]
fn asks_once_a_day_and_three_times_in_all() {
    let env = Env::new();
    set("CI", ""); // GitHub runners set CI=true
    set("CASHCAST_POSTHOG_KEY", "phc_test_canary");
    let u = env.user("alex");
    assert_eq!(ask(&u, "\n", "2026-11-22").0, None);
    assert_eq!(
        telemetry::ask_blocked(&u, d("2026-11-22"), true).unwrap(),
        Some("asked today")
    );
    assert_eq!(ask(&u, "maybe\n", "2026-11-23").0, None);
    assert_eq!(
        telemetry::ask_blocked(&u, d("2026-11-24"), true).unwrap(),
        None
    );
    assert_eq!(ask(&u, "\n", "2026-11-24").0, None);
    assert_eq!(
        telemetry::ask_blocked(&u, d("2026-11-25"), true).unwrap(),
        Some("asked 3 times")
    );
}

#[test]
fn never_asks_without_a_person_or_with_an_off_switch() {
    let env = Env::new();
    set("CI", ""); // GitHub runners set CI=true
    set("CASHCAST_POSTHOG_KEY", "phc_test_canary");
    let u = env.user("alex");
    let day = d("2026-11-22");
    assert_eq!(
        telemetry::ask_blocked(&u, day, false).unwrap(),
        Some("no terminal")
    );
    set("CI", "true");
    assert_eq!(telemetry::ask_blocked(&u, day, true).unwrap(), Some("CI"));
    set("CI", "");
    set("DO_NOT_TRACK", "1");
    assert_eq!(
        telemetry::ask_blocked(&u, day, true).unwrap(),
        Some("DO_NOT_TRACK is set")
    );
    set("DO_NOT_TRACK", "");
    set("CASHCAST_POSTHOG_KEY", "");
    assert_eq!(
        telemetry::ask_blocked(&u, day, true).unwrap(),
        Some("no key, so nothing could be sent")
    );
}
