//! Open tasks and the services they name: changes (US-17), triage (US-23),
//! connectors (US-10), `cards now` inputs (US-29), knowledge (US-11), fetch
//! (US-22) and opt-in telemetry. Ported from tests/unit/test_changes.py,
//! test_triage.py, test_connectors.py, test_cards_now.py, test_knowledge.py,
//! test_fetch.py and test_telemetry.py.

mod app;

use std::cell::RefCell;
use std::rc::Rc;

use app::{Env, append, d, grant_by_hand, mode, read, repo, set, today, unset, write_json};
use cashcast_engine::core::http::{self, Response};
use cashcast_engine::core::paths::UserHome;
use cashcast_engine::core::pyjson::Json;
use cashcast_engine::core::telemetry;
use cashcast_engine::knowledge;
use cashcast_engine::strategy::{Task, plan, py};
use cashcast_engine::tasks::{changes, connectors, fetch, triage};
use chrono::NaiveDate;
use serde_json::json;

fn specs() -> Vec<(String, Json)> {
    connectors::load_all().map_err(|e| e.0).unwrap()
}

fn spec(id: &str) -> Json {
    specs().into_iter().find(|(s, _)| s == id).unwrap().1
}

fn actions(spec: &Json) -> Vec<Json> {
    py::list(spec, "actions").to_vec()
}

fn s(j: &Json, k: &str) -> String {
    py::gets(j, k, "")
}

fn strs(j: Option<&Json>) -> Vec<String> {
    match j {
        Some(Json::List(v)) => v.iter().map(Json::py_str).collect(),
        _ => vec![],
    }
}

fn settings(id: &str, title: &str, service: &str, action: &str) -> Task {
    let mut t = Task::new(id, title).risk("settings");
    t.service = service.into();
    t.action = action.into();
    t
}

// ---------- changes (test_changes.py) ----------

const CHANGE_DAY: &str = "2026-10-21";

#[test]
fn issuer_names_map_to_connectors() {
    let _env = Env::new();
    let all = specs();
    assert_eq!(connectors::find_by_name(&all, "TD").as_deref(), Some("td"));
    assert_eq!(
        connectors::find_by_name(&all, "scotiabank").as_deref(),
        Some("scotiabank")
    );
    assert_eq!(connectors::find_by_name(&all, "CANARY bank"), None);
    assert_eq!(connectors::find_by_name(&all, ""), None);
}

#[test]
fn only_open_settings_tasks_are_listed_with_their_playbook() {
    let env = Env::new();
    let u = env.user("alex");
    let tasks = vec![
        Task::new("pay", "CANARY pay the card")
            .due(d(CHANGE_DAY))
            .risk("money"),
        Task::new("read", "Check scores"),
        settings(
            "downgrade-x",
            "Product-change Card X",
            "td",
            "product-change-card",
        ),
        Task::new("keep-active-y", "Keep card Y active").risk("settings"),
    ];
    let found = changes::pending(&u, &tasks, d(CHANGE_DAY))
        .map_err(|e| e.0)
        .unwrap();
    assert_eq!(
        found.iter().map(|c| c.task.id.as_str()).collect::<Vec<_>>(),
        vec!["downgrade-x", "keep-active-y"]
    );
    let td = &found[0];
    assert!(td.allowed && td.login_url.starts_with("https://") && !td.steps.is_empty());
    assert!(found[1].service.is_empty() && !found[1].allowed);
    let text = changes::render(&found, None).unwrap();
    assert!(
        !text.contains("CANARY") && text.contains("bin/cashcast action td product-change-card")
    );
    assert!(
        text.contains("issuer's app, or by phone")
            && text.contains("consent grant actions:browser")
    );
    assert!(
        changes::render(&found, Some("downgrade-x"))
            .unwrap()
            .contains(&td.steps)
    );
}

#[test]
fn done_hides_a_change_until_undo() {
    let env = Env::new();
    let u = env.user("alex");
    let day = d(CHANGE_DAY);
    let tasks = vec![settings(
        "downgrade-x",
        "Product-change Card X",
        "td",
        "product-change-card",
    )];
    assert!(
        changes::mark(&u, "downgrade-x", day, false)
            .unwrap()
            .contains("Marked downgrade-x done")
    );
    assert!(
        changes::pending(&u, &tasks, day)
            .map_err(|e| e.0)
            .unwrap()
            .is_empty()
    );
    let done = changes::done(&u).map_err(|e| e.0).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(
        (done[0].0.as_str(), done[0].1.py_str()),
        ("downgrade-x", "2026-10-21".to_string())
    );
    assert_eq!(mode(&u.data().join("changes-done.json")), 0o600);
    assert!(
        changes::mark(&u, "downgrade-x", day, true)
            .unwrap()
            .contains("Reopened")
    );
    assert_eq!(
        changes::pending(&u, &tasks, day)
            .map_err(|e| e.0)
            .unwrap()
            .len(),
        1
    );
    assert!(
        changes::mark(&u, "nope", day, true)
            .unwrap()
            .contains("was not marked done")
    );
}

#[test]
fn show_of_an_unknown_change_explains() {
    let _env = Env::new();
    assert!(
        changes::render(&[], Some("nope"))
            .unwrap_err()
            .message
            .contains("changes list")
    );
}

#[test]
fn a_costly_bank_package_becomes_a_change_with_its_connector() {
    let env = Env::new();
    let extra = "[[banking.accounts]]\nid = \"td-unlimited\"\nname = \"CANARY plan\"\nwaive_min_balance = 0\nmonthly_fee = 17\n";
    let u = env.make_user("alex", "ON", extra, None, &[]);
    let day = d(CHANGE_DAY);
    let (ctx, _) = plan::run(&u, day).map_err(|e| e.0).unwrap();
    let task = ctx
        .result("banking")
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.id == "bank-package-td-unlimited")
        .unwrap()
        .clone();
    assert_eq!(
        (
            task.risk.as_str(),
            task.service.as_str(),
            task.action.as_str()
        ),
        ("settings", "td", "change-chequing-package")
    );
    let all = plan::tasks(&ctx).map_err(|e| e.0).unwrap();
    let found = changes::pending(&u, &all, day).map_err(|e| e.0).unwrap();
    assert!(found.iter().any(|c| c.task.id == task.id && c.allowed));
}

// ---------- cards now inputs (test_cards_now.py) ----------

const WALLET: &str = "
[[wallet.cards]]
id = \"alpha-visa\"

[[wallet.cards]]
id = \"beta-visa\"

[[tasks]]
id = \"downgrade-alpha-visa\"
title = \"Product-change the alpha card\"

[[tasks]]
id = \"downgrade-not-a-card\"
title = \"Not a wallet card\"
";

// test_engine_category_rules_match_the_strategy: compared the Python list
// with engine/src/cards.rs; with Python gone, cards.rs is the one source.

#[test]
fn card_inputs_merge_balances_and_name_open_changes() {
    let env = Env::new();
    let u = env.user("alex");
    append(&u.profile(), WALLET);
    write_json(
        &u.data().join("api-balances.json"),
        &json!({"alpha": {"balances": {"alpha-visa": 10.0, "TFSA": 5.0}}, "beta": {"balances": {"beta-visa": 7.5}}}),
    );
    let (args, ids) =
        cashcast_engine::core::cli::card_inputs(&u, &["beta-visa=20".to_string()]).unwrap();
    assert_eq!(ids, vec!["alpha-visa", "beta-visa"]);
    assert_eq!(
        args,
        vec![
            "--balance",
            "beta-visa=20",
            "--balance",
            "alpha-visa=10.0",
            "--pending",
            "alpha-visa=downgrade-alpha-visa"
        ]
    );
}

#[test]
fn a_done_change_is_no_longer_pending() {
    let env = Env::new();
    let u = env.user("alex");
    append(&u.profile(), WALLET);
    write_json(
        &u.data().join("changes-done.json"),
        &json!({"downgrade-alpha-visa": "2026-11-01"}),
    );
    let (args, _) = cashcast_engine::core::cli::card_inputs(&u, &[]).unwrap();
    assert!(!args.contains(&"--pending".to_string()));
}

// ---------- triage (test_triage.py) ----------

const TRIAGE_DAY: &str = "2026-10-30";

fn open_tasks() -> Vec<Task> {
    vec![
        Task::new("paydown-x-2026-11", "CANARY pay card X")
            .due(d("2026-11-16"))
            .risk("money"),
        Task::new("monthly-review-2026-11", "Monthly money review").due(d("2026-11-02")),
        Task::new("monthly-review-2026-12", "Monthly money review").due(d("2026-12-02")),
        {
            let mut t = settings(
                "downgrade-x",
                "Product-change Card X",
                "td",
                "product-change-card",
            );
            t.due = Some(d("2026-10-26"));
            t
        },
        Task::new("keep-active-y", "Keep card Y active").risk("settings"),
        Task::new("credit-scores-monthly", "Check both credit scores").due(d("2026-11-01")),
        Task::new("confirm-chexy-2026-10-30", "Confirm chexy worked").due(d("2026-10-30")),
        Task::new("rrsp-deadline", "Decide your RRSP contribution").due(d("2027-03-01")),
    ]
}

fn classify(tasks: &[Task], browser_ok: bool, changed: &[&str], day: &str) -> Vec<triage::Row> {
    let ids: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
    triage::classify(tasks, browser_ok, &ids, d(day))
        .map_err(|e| e.0)
        .unwrap()
}

fn row<'a>(rows: &'a [triage::Row], id: &str) -> &'a triage::Row {
    rows.iter().find(|r| r.task.id == id).unwrap()
}

fn done_ids(u: &UserHome) -> Vec<String> {
    changes::done(u)
        .map_err(|e| e.0)
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

#[test]
fn every_open_task_gets_a_lane_and_a_step() {
    let _env = Env::new();
    let rows = classify(&open_tasks(), false, &["downgrade-x"], TRIAGE_DAY);
    let mut lanes: Vec<(&str, &str)> = rows.iter().map(|r| (r.task.id.as_str(), r.lane)).collect();
    lanes.sort();
    assert_eq!(
        lanes,
        vec![
            ("confirm-chexy-2026-10-30", "agent"),
            ("credit-scores-monthly", "agent"),
            ("downgrade-x", "browser"),
            ("keep-active-y", "you"),
            ("monthly-review-2026-11", "auto"),
            ("monthly-review-2026-12", "auto"),
            ("paydown-x-2026-11", "you"),
            ("rrsp-deadline", "you"),
        ]
    );
    let chexy = row(&rows, "confirm-chexy-2026-10-30");
    assert!(chexy.step.contains("connectors show chexy") && chexy.step.contains("evidence"));
    assert!(rows.iter().all(|r| !r.step.is_empty()));
    let browser = rows.iter().find(|r| r.lane == "browser").unwrap();
    assert!(browser.step.contains("consent grant actions:browser"));
    let ok = classify(&open_tasks(), true, &["downgrade-x"], TRIAGE_DAY);
    assert!(
        !ok.iter()
            .find(|r| r.lane == "browser")
            .unwrap()
            .step
            .contains("consent grant")
    );
    assert!(
        row(&rows, "paydown-x-2026-11")
            .step
            .contains("never moves money")
    );
}

#[test]
fn resolve_marks_only_due_auto_tasks_and_lists_each_blocker() {
    let env = Env::new();
    let u = env.user("alex");
    let rows = classify(&open_tasks(), false, &["downgrade-x"], TRIAGE_DAY);
    let mut ran = Vec::new();
    let (resolved, blocked) = triage::resolve(&u, &rows, d(TRIAGE_DAY), |s| {
        ran.push(s.to_string());
        Ok(true)
    })
    .unwrap();
    assert_eq!(
        resolved
            .iter()
            .map(|r| r.task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["monthly-review-2026-11"]
    );
    assert_eq!(ran, vec!["review"]);
    let done = done_ids(&u);
    assert!(
        done.contains(&"monthly-review-2026-11".into())
            && !done.contains(&"monthly-review-2026-12".into())
    );
    let mut b: Vec<&str> = blocked.iter().map(|r| r.task.id.as_str()).collect();
    b.sort();
    assert_eq!(
        b,
        vec![
            "downgrade-x",
            "keep-active-y",
            "paydown-x-2026-11",
            "rrsp-deadline"
        ]
    );
    let text = triage::render(&rows, Some((&resolved, &blocked)));
    for want in [
        "Resolved 1 of 8",
        "BLOCKED (4)",
        "Agent checks next (2)",
        "bin/cashcast sync all",
    ] {
        assert!(text.contains(want), "{want}\n{text}");
    }
    assert!(
        blocked
            .iter()
            .all(|r| text.contains(&format!("`{}`", r.task.id)))
    );
}

#[test]
fn a_failed_report_does_not_mark_its_task_done() {
    let env = Env::new();
    let u = env.user("alex");
    let rows = classify(&open_tasks(), false, &[], TRIAGE_DAY);
    let (resolved, _) = triage::resolve(&u, &rows, d(TRIAGE_DAY), |_| Ok(false)).unwrap();
    assert!(resolved.is_empty() && done_ids(&u).is_empty());
}

#[test]
fn list_groups_by_lane_and_json_has_lane_and_step() {
    let _env = Env::new();
    let rows = classify(&open_tasks(), false, &["downgrade-x"], TRIAGE_DAY);
    let text = triage::render(&rows, None);
    let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s}"));
    assert!(
        at("cashcast resolves") < at("Agent checks")
            && at("Agent checks") < at("Agent in your browser")
    );
    assert!(at("Agent in your browser") < at("Blocked: you do these"));
    let Json::List(items) = triage::as_json(&rows) else {
        panic!()
    };
    assert!(
        items
            .iter()
            .all(|r| !s(r, "lane").is_empty() && !s(r, "step").is_empty())
    );
}

#[test]
fn a_check_carries_the_owners_rule_and_waits_for_its_due_date() {
    let _env = Env::new();
    let rent = vec![
        Task::new("rent-fine-2026-11-01", "Ensure rent is fine")
            .due(d("2026-11-01"))
            .notes("Done only when the rent website shows it passed."),
    ];
    let r = &classify(&rent, false, &[], TRIAGE_DAY)[0];
    assert_eq!(r.lane, "agent");
    assert!(r.step.contains("Not before 2026-11-01"));
    assert!(
        r.step
            .contains("Your rule: Done only when the rent website shows it passed.")
    );
    assert!(
        !classify(&rent, false, &[], "2026-11-01")[0]
            .step
            .contains("Not before")
    );
}

#[test]
fn use_cashcast_and_priorities_resolve_with_the_review_report() {
    let _env = Env::new();
    let ts = vec![
        Task::new("use-cashcast", "Use cashcast").due(d("2026-11-01")),
        Task::new(
            "update-priorities-finances-2026-11-01",
            "Update priorities & finances",
        )
        .due(d("2026-11-01")),
    ];
    let rows = classify(&ts, false, &[], TRIAGE_DAY);
    assert!(
        rows.iter()
            .all(|r| r.lane == "auto" && r.step.contains("bin/cashcast review"))
    );
}

fn verdict(
    u: &UserHome,
    id: &str,
    result: &str,
    reason: &str,
    source: &str,
    browser_ok: bool,
) -> Result<String, String> {
    let rows = classify(&open_tasks(), false, &[], TRIAGE_DAY);
    triage::verdict(
        u,
        &rows,
        id,
        result,
        reason,
        source,
        d(TRIAGE_DAY),
        browser_ok,
    )
    .unwrap()
}

#[test]
fn a_pass_verdict_closes_the_check_and_keeps_no_amount() {
    let env = Env::new();
    let u = env.user("alex");
    let msg = verdict(
        &u,
        "confirm-chexy-2026-10-30",
        "pass",
        "charge posted on the right card",
        "local",
        false,
    )
    .unwrap();
    assert!(msg.contains("Marked confirm-chexy-2026-10-30 done"));
    assert!(done_ids(&u).contains(&"confirm-chexy-2026-10-30".into()));
    let saved = read(&u.data().join("verdicts.jsonl"));
    assert!(
        saved.contains("\"result\": \"pass\"")
            && saved.contains("\"source\": \"local\"")
            && !saved.contains("CANARY")
    );
}

#[test]
fn a_fail_verdict_keeps_the_task_open_and_says_what_to_do() {
    let env = Env::new();
    let u = env.user("alex");
    let msg = verdict(
        &u,
        "confirm-chexy-2026-10-30",
        "fail",
        "charge missing",
        "local",
        false,
    )
    .unwrap();
    assert!(msg.contains("stays open") && done_ids(&u).is_empty());
}

#[test]
fn a_reason_with_an_amount_or_too_long_is_refused() {
    let env = Env::new();
    let u = env.user("alex");
    let long = "x".repeat(121);
    for reason in [
        "paid $3,661.36",
        "balance 3661.36",
        "score 781",
        "CANARY 12,500 owed",
        long.as_str(),
    ] {
        assert!(
            verdict(
                &u,
                "confirm-chexy-2026-10-30",
                "pass",
                reason,
                "local",
                false
            )
            .is_err(),
            "{reason}"
        );
    }
}

#[test]
fn only_agent_checks_that_are_due_take_a_verdict() {
    let env = Env::new();
    let u = env.user("alex");
    assert!(
        verdict(&u, "paydown-x-2026-11", "pass", "paid", "local", false)
            .unwrap_err()
            .contains("not a check")
    );
    let e = verdict(
        &u,
        "credit-scores-monthly",
        "pass",
        "both fetched",
        "local",
        false,
    )
    .unwrap_err();
    assert!(e.contains("not before 2026-11-01"), "{e}");
}

#[test]
fn a_browser_verdict_needs_the_browser_read_consent() {
    let env = Env::new();
    let u = env.user("alex");
    let e = verdict(
        &u,
        "confirm-chexy-2026-10-30",
        "pass",
        "delivered",
        "browser",
        false,
    )
    .unwrap_err();
    assert!(e.contains("consent grant agent:browser-read"), "{e}");
    assert!(
        verdict(
            &u,
            "confirm-chexy-2026-10-30",
            "pass",
            "delivered",
            "browser",
            true
        )
        .unwrap()
        .contains("done")
    );
}

#[test]
fn a_check_names_the_private_path_first_and_the_browser_path_with_its_consent() {
    let _env = Env::new();
    let rows = classify(&open_tasks(), false, &[], TRIAGE_DAY);
    let step = &row(&rows, "confirm-chexy-2026-10-30").step;
    assert!(step.find("connectors show chexy").unwrap() < step.find("agent:browser-read").unwrap());
    assert!(step.contains("inbox") && step.contains("tasks verdict confirm-chexy-2026-10-30"));
}

// ---------- connectors (test_connectors.py) ----------

const SERVICES: [&str; 17] = [
    "moneywiz",
    "scotiabank",
    "td",
    "wise",
    "wealthsimple",
    "casa",
    "kraken",
    "chexy",
    "paypal",
    "paysend",
    "google-sheets",
    "etrade",
    "sunlife",
    "cibc",
    "rbc",
    "bmo",
    "tangerine",
];

/// The doctor's connector rules (`core::health::validate`), applied to every spec.
fn problems(spec: &Json) -> Vec<String> {
    cashcast_engine::core::health::validate(spec, &cashcast_engine::tasks::connectors::dir())
}

#[test]
fn tinkoff_is_a_bank_abroad_with_a_browser_export() {
    let _env = Env::new();
    let all = specs();
    let spec = &all.iter().find(|(s, _)| s == "tinkoff").unwrap().1;
    assert!(matches!(
        spec.get("available_in_canada"),
        Some(Json::Bool(false))
    ));
    let api = spec.get("api").unwrap();
    assert!(matches!(api.get("available"), Some(Json::Bool(false))));
    let import = spec.get("import").unwrap();
    assert_eq!(strs(import.get("formats")), vec!["tinkoff-csv"]);
}

#[test]
fn every_requested_service_has_a_valid_spec() {
    let _env = Env::new();
    let all = specs();
    for sid in SERVICES {
        assert!(all.iter().any(|(s, _)| s == sid), "{sid}");
    }
    for (sid, spec) in &all {
        assert_eq!(problems(spec), Vec::<String>::new(), "{sid}");
        assert_eq!(&s(spec, "id"), sid);
        if SERVICES.contains(&sid.as_str()) {
            assert!(
                matches!(spec.get("available_in_canada"), Some(Json::Bool(true))),
                "{sid}"
            );
        }
    }
    let mut bad = all[0].1.clone();
    bad.set(
        "actions",
        cashcast_engine::core::pyjson::loads(
            r#"[{"id": "x", "title": "x", "risk": "money", "via": ["browser"]}]"#,
        )
        .unwrap(),
    );
    assert!(problems(&bad).iter().any(|p| p.contains("money actions")));
    assert!(connectors::get(&all, Some("not-a-service")).is_err());
}

#[test]
fn every_bank_has_a_read_only_balance_step() {
    let _env = Env::new();
    for (sid, spec) in specs() {
        if strs(spec.get("kinds")).contains(&"bank".to_string()) {
            let acts = actions(&spec);
            let step = acts
                .iter()
                .find(|a| s(a, "id") == "read-balances")
                .or_else(|| acts.iter().find(|a| s(a, "id") == "check-balances"));
            assert!(step.is_some_and(|a| s(a, "risk") == "read"), "{sid}");
        }
    }
}

#[test]
fn every_investment_or_crypto_source_reaches_the_strategy() {
    let _env = Env::new();
    let fx = py::table(&knowledge::load("foreign_cash").unwrap());
    let Some(Json::Obj(mapped)) = fx.get("api_services") else {
        panic!("no [api_services]")
    };
    let all = specs();
    for (k, _) in mapped {
        assert!(all.iter().any(|(s, _)| s == k), "{k} is not a connector");
    }
    for (sid, spec) in &all {
        let kinds = strs(spec.get("kinds"));
        if !["broker", "crypto", "retirement"]
            .iter()
            .any(|k| kinds.contains(&k.to_string()))
        {
            continue;
        }
        assert!(
            mapped.iter().any(|(k, _)| k == sid),
            "{sid}: add it to [api_services]"
        );
        let api = strs(py::obj(spec, "api").get("read"))
            .iter()
            .any(|r| r.contains("balance"));
        let acts = actions(spec);
        let step = [
            "read-investment-balances",
            "read-balances",
            "check-balances",
            "check-vault-balance",
        ]
        .iter()
        .find_map(|id| acts.iter().find(|a| s(a, "id") == *id));
        assert!(api || step.is_some_and(|a| s(a, "risk") == "read"), "{sid}");
    }
}

#[test]
fn money_actions_are_human_only_and_every_action_has_playbook_steps() {
    let _env = Env::new();
    for (sid, spec) in specs() {
        for a in actions(&spec) {
            if s(&a, "risk") == "money" {
                assert_eq!(strs(a.get("via")), vec!["human"], "{sid} {}", s(&a, "id"));
            }
            let section = connectors::playbook_section(&spec, &s(&a, "id"))
                .map_err(|e| e.0)
                .unwrap();
            assert!(!section.trim().is_empty(), "{sid} {}", s(&a, "id"));
        }
    }
}

#[test]
fn money_action_becomes_a_reminder_not_an_action() {
    let env = Env::new();
    let u = env.user("alex");
    let money = actions(&spec("wise"))
        .into_iter()
        .find(|a| s(a, "risk") == "money")
        .unwrap();
    let (_, out, _) = app::cli(&["--user", "alex", "action", "wise", &s(&money, "id")]);
    assert!(out.contains("never does it"), "{out}");
    let queued: serde_json::Value =
        serde_json::from_str(&read(&u.data().join("manual-tasks.json"))).unwrap();
    assert_eq!(queued[0]["id"], format!("wise-{}", s(&money, "id")));
}

#[test]
fn browser_actions_need_consent_and_settings_need_confirmation() {
    let env = Env::new();
    let u = env.user("alex");
    let action = actions(&spec("chexy"))
        .into_iter()
        .find(|a| s(a, "risk") == "settings")
        .unwrap();
    let id = s(&action, "id");
    let (_, out, _) = app::cli(&["--user", "alex", "action", "chexy", &id]);
    assert!(
        out.contains("consent grant actions:browser") && !out.contains("confirm this exact change"),
        "{out}"
    );
    grant_by_hand(&u, &["actions:browser"]);
    let (_, out, _) = app::cli(&["--user", "alex", "action", "chexy", &id]);
    assert!(out.contains("confirm this exact change"), "{out}");
}

#[test]
fn notion_holds_only_scrubbed_tasks() {
    // Decisions 0005, 0008, 0009: no Notion connector, one scope, and the
    // Notion task client sends titles without amounts.
    let _env = Env::new();
    assert!(!specs().iter().any(|(s, _)| s == "notion"));
    let notion: Vec<&str> = cashcast_engine::core::consent::SCOPES
        .iter()
        .map(|s| s.name)
        .filter(|n| n.contains("notion"))
        .collect();
    assert_eq!(notion, vec!["cloud:notion"]);
    let docs = read(&repo().join("engine/src/docs.rs"));
    for net in ["std::net", "TcpStream", "http::"] {
        assert!(
            !docs.contains(net),
            "the Notion import must stay offline: {net}"
        );
    }
}

// ---------- knowledge (test_knowledge.py) ----------

#[test]
fn shared_knowledge_passes_its_own_check() {
    let _env = Env::new();
    let r = knowledge::check(d("2026-10-18"), &knowledge::data_dir()).unwrap();
    assert!(r.files >= 1);
    assert!(r.ok(), "{:?} {:?}", r.problems, r.missing_source);
    assert!(r.stale.is_empty(), "{:?}", r.stale);
}

#[test]
fn required_topics_exist() {
    let _env = Env::new();
    let have = knowledge::names();
    for n in [
        "tax_ca_2026",
        "credit_cards",
        "banks",
        "insurance",
        "corporate_stock",
        "investing",
        "credit_score",
        "payment_services",
        "crypto_exchanges",
    ] {
        assert!(have.contains(&n.to_string()), "{n}");
    }
}

#[test]
fn check_flags_stale_unsourced_and_estimates() {
    let env = Env::new();
    std::fs::write(
        env.tmp.join("x.toml"),
        "[meta]\nas_of = 2025-01-01\nreview_by = 2025-06-01\nsources = [\"https://example.com\"]\n\
         [a]\nvalue = 1\nsource = \"\"\nconfidence = \"verified\"\n[b]\nvalue = 2\nsource = \"https://example.com\"\nconfidence = \"estimate\"\n",
    )
    .unwrap();
    let r = knowledge::check(d("2026-01-01"), &env.tmp).unwrap();
    assert!(!r.stale.is_empty());
    assert_eq!(
        (r.missing_source, r.estimates),
        (vec!["x.a".to_string()], vec!["x.b".to_string()])
    );
}

#[test]
fn a_user_can_override_a_card_privately() {
    let env = Env::new();
    let u = env.user("alex");
    let base = py::table(&knowledge::load("credit_cards").unwrap());
    let base_cards = py::list(&base, "cards").to_vec();
    let first = s(&base_cards[0], "id");
    std::fs::create_dir_all(u.home.join("knowledge")).unwrap();
    std::fs::write(
        u.home.join("knowledge/credit_cards.toml"),
        format!("[[cards]]\nid = \"{first}\"\nannual_fee = 0\n\n[[cards]]\nid = \"my-own-card\"\nname = \"Mine\"\nearn = {{ other = 0.03 }}\n"),
    )
    .unwrap();
    let merged = cashcast_engine::strategy::situation::knowledge("credit_cards", Some(&u.home))
        .map_err(|e| e.0)
        .unwrap();
    let cards = py::list(&merged, "cards");
    let card = |id: &str| cards.iter().find(|c| s(c, "id") == id).unwrap();
    assert_eq!(py::getf(card(&first), "annual_fee", -1.0), 0.0);
    assert_eq!(s(card(&first), "name"), s(&base_cards[0], "name"));
    assert_eq!(
        py::getf(py::obj(card("my-own-card"), "earn"), "other", 0.0),
        0.03
    );
    assert_eq!(cards.len(), base_cards.len() + 1);
}

#[test]
fn propose_reviews_a_candidate_against_the_current_facts() {
    let env = Env::new();
    let current = env.tmp.join("data");
    std::fs::create_dir_all(&current).unwrap();
    std::fs::write(
        current.join("cards.toml"),
        "[meta]\nas_of = 2026-01-01\nreview_by = 2026-12-31\nsources = [\"https://example.com\"]\n\
         [[cards]]\nid = \"a\"\nannual_fee = 120\nsource = \"https://example.com/a\"\nconfidence = \"estimate\"\n\
         [[cards]]\nid = \"b\"\nannual_fee = 0\nsource = \"https://example.com/b\"\nconfidence = \"verified\"\n",
    )
    .unwrap();
    let candidate = env.tmp.join("cards.toml");
    std::fs::write(
        &candidate,
        "[meta]\nas_of = 2026-10-18\nreview_by = 2027-04-18\nsources = [\"https://example.com\"]\n\
         [[cards]]\nid = \"a\"\nannual_fee = 139\nsource = \"https://example.com/a2\"\nconfidence = \"verified\"\n\
         [[cards]]\nid = \"c\"\nannual_fee = 0\nsource = \"\"\nconfidence = \"verified\"\n",
    )
    .unwrap();
    let r = knowledge::propose(&candidate, d("2026-10-18"), &current).unwrap();
    assert_eq!(
        r.changed,
        vec!["cards[a]: annual_fee 120 → 139, confidence estimate → verified, source changed"]
    );
    assert_eq!(
        (r.added.clone(), r.removed.clone()),
        (vec!["cards[c]".to_string()], vec!["cards[b]".to_string()])
    );
    assert_eq!(r.check.missing_source, vec!["cards.cards[c]"]);
    assert!(!r.ok());
    let text = knowledge::render_proposal(&r);
    assert!(text.contains("Changed") && text.contains("Blocked"));
}

#[test]
fn propose_rejects_an_older_as_of() {
    let env = Env::new();
    let current = env.tmp.join("data");
    std::fs::create_dir_all(&current).unwrap();
    std::fs::write(
        current.join("x.toml"),
        "[meta]\nas_of = 2026-10-01\nreview_by = 2027-01-01\nsources = [\"https://e.com\"]\n",
    )
    .unwrap();
    let cand = env.tmp.join("x.toml");
    std::fs::write(
        &cand,
        "[meta]\nas_of = 2026-09-01\nreview_by = 2027-01-01\nsources = [\"https://e.com\"]\n",
    )
    .unwrap();
    let r = knowledge::propose(&cand, d("2026-10-18"), &current).unwrap();
    assert!(!r.ok() && r.problems.iter().any(|p| p.contains("as_of")));
}

#[test]
fn rules_of_thumb_are_labelled_not_counted_as_unverified() {
    let env = Env::new();
    std::fs::write(
        env.tmp.join("x.toml"),
        "[meta]\nas_of = 2026-10-19\nreview_by = 2027-04-19\nsources = [\"https://example.com\"]\n\
         [a]\nrule = \"10x income\"\nsource = \"https://example.com\"\nconfidence = \"heuristic\"\n",
    )
    .unwrap();
    let r = knowledge::check(d("2026-10-19"), &env.tmp).unwrap();
    assert!(r.ok() && r.heuristics == vec!["x.a"] && r.estimates.is_empty());
    assert!(knowledge::render_check(&r).contains("Rules of thumb"));
}

// ---------- fetch (test_fetch.py) ----------

const FETCH_DAY: &str = "2026-10-25";

fn fetch_home(env: &Env, files: Option<serde_json::Value>) -> std::path::PathBuf {
    let home = env.tmp.join("alex");
    std::fs::create_dir_all(home.join("data")).unwrap();
    if let Some(f) = files {
        write_json(&home.join("data/summary.json"), &json!({"files": f}));
    }
    home
}

fn needs(home: &std::path::Path, everything: bool) -> Vec<fetch::Need> {
    fetch::plan(home, &specs(), d(FETCH_DAY), everything, 35, 35)
        .map_err(|e| e.0)
        .unwrap()
}

/// A crypto-only service (kinds crypto or fx: Kraken, Casa) may wait a
/// month; a bank that also holds crypto (Wealthsimple) keeps the bank limit.
#[test]
fn a_crypto_only_service_waits_longer() {
    let env = Env::new();
    let home = fetch_home(
        &env,
        Some(json!([
            {"format": "td-csv", "date_max": "2026-10-05"},
            {"format": "wealthsimple-activities-csv", "date_max": "2026-10-05"},
            {"format": "kraken-ledger-csv", "date_max": "2026-10-05"}])),
    );
    let n = fetch::plan(&home, &specs(), d(FETCH_DAY), false, 6, 31)
        .map_err(|e| e.0)
        .unwrap();
    let got: Vec<(&str, bool, i64)> = n
        .iter()
        .map(|x| (x.service.as_str(), x.stale, x.limit))
        .collect();
    assert_eq!(
        got,
        vec![
            ("td", true, 6),
            ("wealthsimple", true, 6),
            ("kraken", false, 31)
        ]
    );
    let out = fetch::render_plan(&n, 6, 0);
    assert!(
        out.contains("kraken (Kraken): fresh (crypto: stale after 31 days), newest 2026-10-05"),
        "{out}"
    );
    let formats = fetch::crypto_formats(&specs());
    assert!(formats.contains(&"kraken-ledger-csv".to_string()));
    assert!(!formats.iter().any(|f| f.starts_with("wealthsimple")));
}

#[test]
fn plan_lists_services_seen_in_the_inbox_stale_first() {
    let env = Env::new();
    let home = fetch_home(
        &env,
        Some(json!([
            {"format": "td-csv", "date_max": "2026-08-01"},
            {"format": "wise-statement-csv", "date_max": "2026-10-20"},
            {"format": "td-csv", "date_max": "2026-07-01"}])),
    );
    let n = needs(&home, false);
    let got: Vec<(&str, bool, Option<&str>)> = n
        .iter()
        .map(|x| (x.service.as_str(), x.stale, x.newest.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("td", true, Some("2026-08-01")),
            ("wise", false, Some("2026-10-20"))
        ]
    );
    assert!(
        n[0].route == "browser"
            && n[0].login_url.starts_with("https://")
            && n[0].action == "download-transactions"
    );
    assert_eq!(n[1].route, "api");
}

#[test]
fn a_shared_format_names_no_service_and_says_so() {
    let env = Env::new();
    let home = fetch_home(
        &env,
        Some(
            json!([{"format": "ofx", "date_max": "2026-01-01"}, {"format": "td-csv", "date_max": "2026-10-20"}]),
        ),
    );
    let n = needs(&home, false);
    assert_eq!(
        n.iter().map(|x| x.service.as_str()).collect::<Vec<_>>(),
        vec!["td"]
    );
    assert_eq!(fetch::unnamed(&home, &specs()).map_err(|e| e.0).unwrap(), 1);
    assert!(
        fetch::render_plan(&n, 35, 1)
            .contains("1 inbox file(s) use a format several services share")
    );
}

#[test]
fn all_lists_every_service_with_an_export_and_none_as_stale() {
    let env = Env::new();
    let n = needs(&fetch_home(&env, None), true);
    let ids: Vec<&str> = n.iter().map(|x| x.service.as_str()).collect();
    for want in ["td", "scotiabank", "wealthsimple", "casa", "etrade"] {
        assert!(ids.contains(&want), "{want}");
    }
    assert!(!ids.contains(&"chexy"));
    assert!(n.iter().all(|x| x.stale && x.newest.is_none()));
}

#[test]
fn render_offers_both_ways() {
    let env = Env::new();
    let home = fetch_home(
        &env,
        Some(json!([{"format": "td-csv", "date_max": "2026-01-01"}])),
    );
    let out = fetch::render_plan(&needs(&home, false), 35, 0);
    assert!(out.contains("td (TD Canada Trust): stale"));
    assert!(out.contains("export them yourself") && out.contains("fetch-exports"));
}

/// Writes `inbox/<name>` with its modified time at noon on `day`.
fn saved_on(home: &std::path::Path, name: &str, day: &str) {
    let path = home.join("inbox").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "canary-not-read").unwrap();
    let when = d(day)
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::from(when))
        .unwrap();
}

#[test]
fn a_fresh_download_with_no_new_rows_is_not_stale() {
    // An account with no activity since: the newest row is old, but the
    // file came today. A file saved after `today` (a fixed test day) is
    // ignored, so only the rows count.
    let env = Env::new();
    let home = fetch_home(
        &env,
        Some(json!([
            {"format": "td-csv", "date_max": "2026-08-01", "name": "td.csv"},
            {"format": "wise-statement-csv", "date_max": "2026-08-01", "name": "wise.csv"},
            {"format": "paypal-activity-csv", "date_max": "2026-08-01", "name": "paypal.csv"}])),
    );
    saved_on(&home, "td.csv", "2026-10-24");
    saved_on(&home, "wise.csv", "2026-09-01");
    saved_on(&home, "paypal.csv", "2026-10-30");
    let n = needs(&home, false);
    let got: Vec<(&str, bool, Option<&str>)> = n
        .iter()
        .map(|x| (x.service.as_str(), x.stale, x.downloaded.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("paypal", true, None),
            ("wise", true, Some("2026-09-01")),
            ("td", false, Some("2026-10-24")),
        ]
    );
    let out = fetch::render_plan(&n, 35, 0);
    assert!(
        out.contains(
            "td (TD Canada Trust): fresh, newest 2026-08-01 (85 days old), downloaded 2026-10-24."
        ),
        "{out}"
    );
    assert!(
        out.contains("wise (Wise): stale, newest 2026-08-01 (85 days old), downloaded 2026-09-01."),
        "{out}"
    );
}

fn touch(path: &std::path::Path, age_s: u64, body: &str) {
    std::fs::write(path, body).unwrap();
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(age_s);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn collect_moves_only_new_export_files_and_prints_no_names() {
    let env = Env::new();
    let inbox = env.tmp.join("alex/inbox");
    let dl = env.tmp.join("Downloads");
    std::fs::create_dir_all(&dl).unwrap();
    touch(&dl.join("acct-1234-canary.csv"), 0, "canary-row");
    touch(&dl.join("statement.qfx"), 0, "canary-row");
    touch(&dl.join("old.csv"), 3 * 3600, "canary-row");
    touch(&dl.join("notion-export.zip"), 0, "canary-row");
    touch(&dl.join("half.csv.crdownload"), 0, "canary-row");
    touch(&dl.join("photo.png"), 0, "canary-row");
    let moved = fetch::collect(&inbox, &dl, 60).unwrap();
    assert_eq!(
        moved,
        vec![(".csv".to_string(), 1), (".qfx".to_string(), 1)]
    );
    let names = |p: &std::path::Path| {
        let mut v: Vec<String> = std::fs::read_dir(p)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&inbox), vec!["acct-1234-canary.csv", "statement.qfx"]);
    assert_eq!(
        names(&dl),
        vec![
            "half.csv.crdownload",
            "notion-export.zip",
            "old.csv",
            "photo.png"
        ]
    );
    let out = fetch::render_collect(&moved, 60);
    assert!(
        out.starts_with("Moved 2 file(s) (.csv ×1, .qfx ×1)")
            && !out.contains("canary")
            && !out.contains("1234"),
        "{out}"
    );
}

#[test]
fn collect_keeps_an_existing_inbox_file() {
    let env = Env::new();
    let inbox = env.tmp.join("alex/inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    touch(&inbox.join("accountactivity.csv"), 0, "first");
    let dl = env.tmp.join("Downloads");
    std::fs::create_dir_all(&dl).unwrap();
    touch(&dl.join("accountactivity.csv"), 0, "second");
    fetch::collect(&inbox, &dl, 60).unwrap();
    assert_eq!(read(&inbox.join("accountactivity.csv")), "first");
    assert_eq!(read(&inbox.join("accountactivity-1.csv")), "second");
}

#[test]
fn collect_with_no_downloads_folder_moves_nothing() {
    let env = Env::new();
    let moved = fetch::collect(&env.tmp.join("alex/inbox"), &env.tmp.join("missing"), 60).unwrap();
    assert!(moved.is_empty());
    assert_eq!(
        fetch::render_collect(&moved, 60),
        "No new export files in Downloads from the last 60 minutes."
    );
}

// ---------- telemetry (test_telemetry.py) ----------

fn telemetry_user(env: &Env, grant: bool) -> (UserHome, Rc<RefCell<Vec<String>>>) {
    let u = env.user("alex");
    std::fs::create_dir_all(u.logs()).unwrap();
    std::fs::write(
        u.logs().join("events.jsonl"),
        "{\"ts\": 1792800000.5, \"event\": \"command\", \"command\": \"forecast\", \"result\": \"ok\", \"ms\": 812}\n\
         {\"ts\": 1792800100.0, \"event\": \"command\", \"command\": \"import\", \"result\": \"failed\", \"service\": \"canary-bank\", \
         \"payee\": \"CANARY payee\", \"amount_text\": \"CANARY 12.50\", \"ms\": 40}\n",
    )
    .unwrap();
    if grant {
        grant_by_hand(&u, &[telemetry::SCOPE]);
    }
    set("CASHCAST_POSTHOG_KEY", "phc_test_CANARY_key");
    set("CASHCAST_TELEMETRY_URL", "https://telemetry.invalid");
    unset("DO_NOT_TRACK");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    http::set_transport(Some(Box::new(move |_m, _url, _h, body| {
        seen.borrow_mut()
            .push(String::from_utf8_lossy(body.unwrap_or_default()).into_owned());
        Ok(Response {
            status: 200,
            body: b"{}".to_vec(),
        })
    })));
    (u, calls)
}

fn real_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

#[test]
fn dry_run_shows_only_allowlisted_fields_and_hides_the_key() {
    let env = Env::new();
    let (u, calls) = telemetry_user(&env, true);
    let out = cashcast_engine::core::pyjson::dumps(&telemetry::dry_run(&u).unwrap(), None, false);
    assert!(calls.borrow().is_empty());
    for want in [
        "\"api_key\": \"<posthog_key>\"",
        "\"$process_person_profile\": false",
        "\"$geoip_disable\": true",
        "\"event\": \"cashcast.command\"",
        "\"timestamp\": \"2026-",
    ] {
        assert!(out.contains(want), "{want}\n{out}");
    }
    assert!(!out.to_lowercase().contains("canary") && !out.contains("phc_test"));
    http::set_transport(None);
}

#[test]
fn send_posts_once_and_the_cursor_stops_repeats() {
    let env = Env::new();
    let (u, calls) = telemetry_user(&env, true);
    assert_eq!(telemetry::send(&u, real_today()), (2, "ok", String::new()));
    assert_eq!(calls.borrow().len(), 1);
    let body: serde_json::Value = serde_json::from_str(&calls.borrow()[0]).unwrap();
    assert_eq!(body["batch"].as_array().unwrap().len(), 2);
    assert!(!body["batch"].to_string().to_uppercase().contains("CANARY"));
    assert_eq!(telemetry::send(&u, real_today()).0, 0);
    assert_eq!(mode(&env.root.join("telemetry.json")), 0o600);
    http::set_transport(None);
}

#[test]
fn nothing_is_sent_without_consent_key_or_with_an_off_switch() {
    let env = Env::new();
    let (u, calls) = telemetry_user(&env, false);
    assert!(telemetry::send(&u, real_today()).2.contains("no consent"));
    grant_by_hand(&u, &[telemetry::SCOPE]);
    set("CASHCAST_POSTHOG_KEY", "");
    assert!(telemetry::send(&u, real_today()).2.contains("no key"));
    set("CASHCAST_POSTHOG_KEY", "phc_test_CANARY_key");
    set("DO_NOT_TRACK", "1");
    assert!(telemetry::send(&u, real_today()).2.contains("DO_NOT_TRACK"));
    unset("DO_NOT_TRACK");
    let (code, _, err) = app::cli(&["--user", "alex", "telemetry", "off"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(telemetry::send(&u, real_today()).1, "off");
    assert!(calls.borrow().is_empty());
    http::set_transport(None);
}

#[test]
fn a_failed_send_keeps_the_cursor_and_never_raises() {
    let env = Env::new();
    let (u, _) = telemetry_user(&env, true);
    http::set_transport(Some(Box::new(|_, _, _, _| {
        Err(cashcast_engine::core::paths::AppError::new(
            "oserror", "down",
        ))
    })));
    assert_eq!(telemetry::send(&u, real_today()).1, "fail");
    assert_eq!(telemetry::status(&u, real_today()).unwrap().pending, 2);
    http::set_transport(None);
}

#[test]
fn the_notice_shows_once() {
    // test_daily_send_waits_for_the_notice_then_runs_once_a_day: the Rust
    // start screen decides with `start_would_send`; the send itself is `send`.
    let env = Env::new();
    let (u, calls) = telemetry_user(&env, true);
    assert_eq!(
        telemetry::notice_once(&u, real_today()),
        Some(telemetry::NOTICE)
    );
    assert_eq!(telemetry::notice_once(&u, real_today()), None);
    assert!(telemetry::start_would_send(&u, real_today(), real_today()).unwrap());
    assert_eq!(telemetry::send(&u, real_today()).0, 2);
    assert!(!telemetry::start_would_send(&u, real_today(), real_today()).unwrap());
    assert_eq!(calls.borrow().len(), 1);
    http::set_transport(None);
}

#[test]
fn cli_telemetry_status_off_and_dry_run() {
    let env = Env::new();
    let (_u, calls) = telemetry_user(&env, true);
    let (code, out, _) = app::cli(&["--user", "alex", "telemetry", "send", "--dry-run"]);
    assert!(code == 0 && out.contains("<posthog_key>"), "{out}");
    assert_eq!(app::cli(&["--user", "alex", "telemetry", "off"]).0, 0);
    let (code, out, _) = app::cli(&["--user", "alex", "telemetry"]);
    assert!(code == 0 && out.contains("telemetry: off"), "{out}");
    assert!(calls.borrow().is_empty());
    http::set_transport(None);
}

#[allow(dead_code)]
fn unused() -> NaiveDate {
    today()
}
