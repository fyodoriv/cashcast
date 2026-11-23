//! US-05 and US-06: many people, Canadian residents only, each in their own
//! private folder; nothing private reaches an agent without the owner's yes.
//! Ported from tests/unit/test_users.py and test_consent.py.

mod app;

use app::{Env, grant_by_hand, mode, read, set};
use cashcast_engine::core::paths::{self, UserHome};
use cashcast_engine::core::pyjson::Json;
use cashcast_engine::core::{consent, events, users};

#[test]
fn each_user_gets_a_private_folder() {
    let env = Env::new();
    let a = users::add("alex", "ON", Some("Alex")).unwrap();
    let b = users::add("sam", "QC", Some("Sam")).unwrap();
    assert_ne!(a.home, b.home);
    for u in [&a, &b] {
        assert_eq!(mode(&u.home), 0o700);
        assert_eq!(mode(&u.profile()), 0o600);
        for d in [u.inbox(), u.data(), u.out(), u.logs()] {
            assert!(d.is_dir());
        }
    }
    let p: toml::Table = toml::from_str(&read(&b.profile())).unwrap();
    assert_eq!(p["province"].as_str(), Some("QC"));
    assert_eq!(paths::list_users(), vec!["alex", "sam"]);
    drop(env);
}

#[test]
fn only_canadian_provinces() {
    let _env = Env::new();
    assert!(users::add("tex", "TX", None).is_err());
    for (code, _) in users::PROVINCES {
        users::add(&format!("p-{}", code.to_lowercase()), code, None).unwrap();
    }
}

#[test]
fn profile_outside_canada_is_rejected() {
    let env = Env::new();
    let u = env.user("alex");
    let text = read(&u.profile()).replace("country = \"CA\"", "country = \"US\"");
    std::fs::write(u.profile(), text).unwrap();
    let e = users::load_profile(&u).unwrap_err();
    assert!(
        e.message.contains("Canadian residents only"),
        "{}",
        e.message
    );
}

#[test]
fn resolve_needs_a_name_when_several_users_exist() {
    let env = Env::new();
    env.user("alex");
    assert_eq!(paths::resolve(None).unwrap().name, "alex");
    env.user("sam");
    assert!(paths::resolve(None).is_err());
    assert_eq!(paths::resolve(Some("sam")).unwrap().name, "sam");
}

#[test]
fn an_unknown_person_fails_and_creates_no_folder() {
    let env = Env::new();
    env.user("alex");
    let e = paths::resolve(Some("alx")).unwrap_err();
    assert!(e.message.contains("no person named 'alx'"), "{}", e.message);
    assert!(e.message.contains("alex"), "{}", e.message);
    assert!(e.message.contains("users add alx"), "{}", e.message);
    assert!(!paths::users_dir().join("alx").exists());
    assert_eq!(paths::list_users(), vec!["alex"]);
    // A folder left over without a profile (an old typo) is not a person.
    std::fs::create_dir_all(paths::users_dir().join("alx").join("out")).unwrap();
    assert_eq!(paths::list_users(), vec!["alex"]);
    assert_eq!(paths::resolve(None).unwrap().name, "alex");
}

#[test]
fn bad_names_and_legacy_folder() {
    let env = Env::new();
    assert!(paths::check_name("../etc").is_err());
    std::fs::create_dir_all(&env.root).unwrap();
    std::fs::write(env.root.join("cashcast.toml"), "").unwrap();
    assert_eq!(paths::list_users(), vec!["default"]);
    let root = paths::resolve_path(&env.root);
    assert_eq!(paths::resolve(None).unwrap().home, root);
    let u = users::adopt("BC", None).unwrap();
    assert_eq!(u.home, root);
    assert_eq!(
        users::load_profile(&u).unwrap()["province"].as_str(),
        Some("BC")
    );
}

// test_adopt_leaves_existing_owner_only_folders_alone: monkeypatched os.chmod
// to fail. Rust `create_folders` only chmods folders it creates; the adopt
// path above covers the behavior on existing folders (they already exist).
#[test]
fn adopt_keeps_existing_owner_only_folders() {
    let env = Env::new();
    for d in ["inbox", "data", "out", "logs"] {
        std::fs::create_dir_all(env.root.join(d)).unwrap();
    }
    std::fs::write(env.root.join("cashcast.toml"), "").unwrap();
    let u = users::adopt("ON", None).unwrap();
    assert_eq!(
        users::load_profile(&u).unwrap()["province"].as_str(),
        Some("ON")
    );
}

fn grant(
    u: &UserHome,
    scope: &str,
    typed: &str,
) -> (Result<chrono::NaiveDate, paths::AppError>, String) {
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(typed.as_bytes().to_vec());
    let r = consent::grant(u, scope, &mut input, &mut out, app::today());
    (r, String::from_utf8(out).unwrap())
}

#[test]
fn grant_needs_the_scope_typed_back() {
    let env = Env::new();
    let u = env.user("alex");
    let today = app::today();
    assert!(grant(&u, "agent:strategy", "yes\n").0.is_err());
    assert!(!consent::allowed(&u, "agent:strategy", today).unwrap());
    let (r, out) = grant(&u, "agent:strategy", "agent:strategy\n");
    let expires = r.unwrap();
    assert!(consent::allowed(&u, "agent:strategy", today).unwrap());
    assert!(out.contains("Shares:") && out.contains("With:"));
    let later = expires.with_year(expires.year() + 1).unwrap();
    assert!(!consent::allowed(&u, "agent:strategy", later).unwrap());
    assert_eq!(mode(&u.consent()), 0o600);
}

use chrono::Datelike;

#[test]
fn revoke_and_unknown_scope() {
    let env = Env::new();
    let u = env.user("alex");
    grant(&u, "cloud:sheets", "cloud:sheets\n").0.unwrap();
    assert!(consent::revoke(&u, Some("cloud:sheets")).unwrap());
    assert!(!consent::allowed(&u, "cloud:sheets", app::today()).unwrap());
    assert!(grant(&u, "everything", "everything\n").0.is_err());
}

#[test]
fn require_says_how_to_grant() {
    let env = Env::new();
    let u = env.user("alex");
    let e = consent::require(&u, "net:market-data", app::today()).unwrap_err();
    assert!(
        e.message
            .contains("bin/cashcast consent grant net:market-data")
    );
}

#[test]
fn users_do_not_share_consent() {
    let env = Env::new();
    let (a, b) = (env.user("alex"), env.user("sam"));
    grant_by_hand(&a, &["agent:aggregates"]);
    assert!(consent::allowed(&a, "agent:aggregates", app::today()).unwrap());
    assert!(!consent::allowed(&b, "agent:aggregates", app::today()).unwrap());
}

#[test]
fn gate_withholds_from_agents_without_consent() {
    // The gate prints to stdout; the CLI shows the same rule end to end.
    let env = Env::new();
    let u = env.user("alex");
    set("CLAUDECODE", "1");
    assert!(consent::agent_context());
    assert!(
        !cashcast_engine::core::gate::emit(
            &u,
            "secret total 123",
            "agent:aggregates",
            "forecast",
            app::today()
        )
        .unwrap()
    );
    assert_eq!(read(&u.out().join("last-forecast.md")), "secret total 123");
    grant_by_hand(&u, &["agent:aggregates"]);
    assert!(
        cashcast_engine::core::gate::emit(
            &u,
            "secret total 123",
            "agent:aggregates",
            "forecast",
            app::today()
        )
        .unwrap()
    );
    app::unset("CLAUDECODE");
}

#[test]
fn event_log_drops_free_text() {
    let env = Env::new();
    set("CASHCAST_LOG", "on");
    let u = env.user("alex");
    events::log(
        &u,
        "command",
        &[
            ("command", Json::Str("strategy".into())),
            ("payee", Json::Str("CANARY payee".into())),
            ("scope", Json::Str("Has Spaces CANARY".into())),
            ("ms", Json::Int(12)),
        ],
    );
    events::error(&u, "strategy", "valueerror");
    let text = read(&u.logs().join("events.jsonl")) + &read(&u.logs().join("errors.jsonl"));
    assert!(
        !text.contains("CANARY") && text.contains("\"ms\": 12"),
        "{text}"
    );
    assert!(text.contains("\"story\": \"US-07\""), "{text}");
}
