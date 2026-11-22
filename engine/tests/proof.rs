//! The README's Proof section matches docs/proof/metrics.json, and every saved
//! metric says when, how and what kind (was tests/unit/test_proof.py).

use cashcast_engine::proof;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn data() -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo().join("docs/proof/metrics.json")).unwrap())
        .unwrap()
}

fn readme() -> String {
    std::fs::read_to_string(repo().join("README.md")).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("proof-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn log(path: &Path, rows: &[Value]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_readme_proof_section_is_what_the_generator_writes() {
    let r = readme();
    assert_eq!(
        proof::splice(&r, &proof::render(&data())).unwrap(),
        r,
        "run `engine/target/release/cashcast-engine proof render`"
    );
}

#[test]
fn every_metric_has_a_value_date_kind_and_command() {
    assert_eq!(proof::problems(&data()), Vec::<String>::new());
}

#[test]
fn a_metric_without_a_date_or_an_honest_kind_is_a_problem() {
    let d = json!({"metrics": {"x": {"value": 1, "unit": "n", "date": "", "kind": "made up", "command": "c"}}});
    assert_eq!(
        proof::problems(&d),
        vec![
            "x: no date",
            "x: kind must be `real use` or `local run, YYYY-MM-DD, machine`"
        ]
    );
}

#[test]
fn the_proof_section_follows_features_and_has_a_chart() {
    let r = readme();
    assert!(
        r.find("## Features") < r.find("## Proof")
            && r.find("## Proof") < r.find("## What it reads")
    );
    let section = &r[r.find(proof::START).unwrap()..r.find(proof::END).unwrap()];
    assert!(section.contains("```mermaid"));
}

#[test]
fn field_totals_are_counts_from_real_run_events_under_a_salted_machine_label() {
    let tmp = scratch("field");
    let logs = tmp.join("users/canary-person/logs");
    log(
        &logs.join("events.jsonl"),
        &[
            json!({"ts": 1792000000, "event": "command", "command": "status", "result": "ok", "ms": 1000}),
            json!({"ts": 1792000001, "event": "command", "command": "sync", "result": "failed", "ms": 3000}),
            json!({"ts": 1792000002, "event": "sync", "result": "ok", "targets": 2, "done": 4}),
            json!({"ts": 1792000003, "event": "moneywiz_sync", "result": "ok", "sent": 7}),
        ],
    );
    log(
        &logs.join("errors.jsonl"),
        &[json!({"ts": 1792000001, "event": "error", "command": "sync"})],
    );
    let agg = proof::field(&tmp).unwrap();
    assert_eq!(
        (
            &agg["runs"],
            &agg["ok_runs"],
            &agg["errors"],
            &agg["median_run_ms"]
        ),
        (&json!(2), &json!(1), &json!(1), &json!(2000))
    );
    assert_eq!(
        (&agg["tasks_done_elsewhere"], &agg["moneywiz_rows_added"]),
        (&json!(4), &json!(7))
    );
    let label = agg["machine"].as_str().unwrap();
    assert!(label.starts_with("m-"));
    assert_eq!(proof::field(&tmp).unwrap()["machine"], agg["machine"]);
    assert!(!agg.to_string().contains("canary-person"));
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn headline_rows_are_field_outcomes_never_test_counts() {
    let d = data();
    let rows = proof::field_rows(&d);
    assert!(rows.iter().all(|r| !r[2].to_lowercase().contains("test")));
    let any = d["field"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .any(|f| f["runs"].as_i64().unwrap_or(0) != 0);
    if !any {
        assert!(rows.iter().all(|r| r[3].starts_with("not measured yet")));
    }
}

#[test]
fn forecast_miss_rate_uses_each_users_latest_review_counts() {
    let tmp = scratch("forecast");
    log(
        &tmp.join("users/canary-a/logs/events.jsonl"),
        &[
            json!({"ts": 1792000000, "event": "forecast_scored", "checked": 1, "misses": 1}),
            json!({"ts": 1792000100, "event": "forecast_scored", "checked": 3, "misses": 1}),
        ],
    );
    log(
        &tmp.join("users/canary-b/logs/events.jsonl"),
        &[json!({"ts": 1792000200, "event": "forecast_scored", "checked": 1, "misses": 0})],
    );
    let agg = proof::field(&tmp).unwrap();
    assert_eq!(
        (&agg["forecast_checked"], &agg["forecast_misses"]),
        (&json!(4), &json!(1))
    );
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn the_forecast_row_shows_a_real_use_miss_rate_once_reviews_scored_months() {
    let mut d = json!({"field": {"m-1": {"runs": 5, "ok_runs": 5, "errors": 0, "median_run_ms": 900,
        "tasks_done_elsewhere": 0, "moneywiz_rows_added": 0, "forecast_checked": 4, "forecast_misses": 1}}});
    let rows = proof::field_rows(&d);
    let row = &rows[rows.len() - 2];
    assert_eq!(row[3], "25.0 % of 4 months missed by more than 15 %");
    assert!(row[4].contains("real use"));
    d["field"]["m-1"]["forecast_checked"] = json!(0);
    let rows = proof::field_rows(&d);
    assert!(rows[rows.len() - 2][3].starts_with("not measured yet"));
}

#[test]
fn cashback_ratio_is_the_median_of_each_users_latest_ratio() {
    let tmp = scratch("cashback");
    log(
        &tmp.join("users/canary-a/logs/events.jsonl"),
        &[
            json!({"ts": 1792000000, "event": "cashback_scored", "ratio": 9.0, "cards": 2}),
            json!({"ts": 1792000100, "event": "cashback_scored", "ratio": 1.2, "cards": 2}),
        ],
    );
    log(
        &tmp.join("users/canary-b/logs/events.jsonl"),
        &[json!({"ts": 1792000200, "event": "cashback_scored", "ratio": 1.6, "cards": 3})],
    );
    assert_eq!(proof::field(&tmp).unwrap()["cashback_ratio"], json!(1.4));
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn the_cashback_row_shows_a_real_use_ratio() {
    let base = json!({"runs": 5, "ok_runs": 5, "errors": 0, "median_run_ms": 900, "tasks_done_elsewhere": 0, "moneywiz_rows_added": 0});
    let mut with = base.clone();
    with["cashback_ratio"] = json!(1.4);
    let row = proof::field_rows(&json!({"field": {"m-1": with}}))
        .pop()
        .unwrap();
    assert_eq!(row[3], "1.40 × the cash back of your best single card");
    assert!(row[4].contains("real use"));
    let row = proof::field_rows(&json!({"field": {"m-1": base}}))
        .pop()
        .unwrap();
    assert!(row[3].starts_with("not measured yet"));
}
