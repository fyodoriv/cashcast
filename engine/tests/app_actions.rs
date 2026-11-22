//! Every cashcast action runs three ways: the CLI, agents, and the web app
//! (task action-parity). The catalog is generated from the CLI parser. This
//! file fails when a command misses a path: not in the catalog, not in the
//! agent JSON, not runnable from the web app without a written terminal-only
//! reason, or the agent entry points do not name the catalog.
//! Ported from tests/unit/test_action_parity.py and test_actions_catalog.py.

mod app;

use app::{Env, cli, repo, today};
use cashcast_engine::web;
use serde_json::Value;
use std::collections::BTreeSet;

const TOKEN: &str = "tok123";
const PORT: u16 = 8765;

fn headers(token: &str, ctype: &str) -> Vec<(String, String)> {
    vec![
        ("Host".into(), format!("127.0.0.1:{PORT}")),
        ("X-Cashcast-Token".into(), token.into()),
        ("Content-Type".into(), ctype.into()),
    ]
}

fn post(path: &str, body: &str) -> (u16, Value) {
    post_with(path, body, &headers(TOKEN, "application/json"))
}

fn post_with(path: &str, body: &str, h: &[(String, String)]) -> (u16, Value) {
    let r = web::handle("POST", path, h, TOKEN, PORT, today(), body.as_bytes());
    let v = serde_json::from_slice(&r.2)
        .unwrap_or(Value::String(String::from_utf8_lossy(&r.2).to_string()));
    (r.0, v)
}

/// A running server leaves its URL (with the token) in private/out/web.json;
/// a run that passes that token is the owner.
fn serve(env: &Env) {
    let out = env.root.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let url = format!("http://127.0.0.1:{PORT}/#t={TOKEN}");
    std::fs::write(
        out.join("web.json"),
        serde_json::json!({"url": url}).to_string(),
    )
    .unwrap();
}

fn catalog() -> Vec<Value> {
    let (code, out, err) = cli(&["actions", "--json"]);
    assert_eq!(code, 0, "{err}");
    serde_json::from_str::<Value>(&out)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn names(list: &[Value]) -> BTreeSet<String> {
    list.iter()
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect()
}

fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
        && v != ""
        && v != &Value::Object(Default::default())
}

#[test]
fn every_cli_command_is_an_action_with_a_ui_path_or_a_reason() {
    let _env = Env::new();
    let cat = catalog();
    let set = names(&cat);
    assert_eq!(set.len(), cat.len(), "action names are unique");
    assert!(cat.len() >= 35);
    for a in &cat {
        let name = &a["name"];
        assert!(truthy(&a["help"]), "{name}: needs a one-line summary");
        assert!(
            a["ui"] == true || truthy(&a["terminal_only"]),
            "{name}: runs in the web app or names why it cannot"
        );
    }
}

#[test]
fn agents_and_the_web_app_get_the_same_catalog() {
    let _env = Env::new();
    let cat = catalog();
    for entry in ["commands/cashcast.md", ".claude/skills/cashcast/SKILL.md"] {
        let text = std::fs::read_to_string(repo().join(entry)).unwrap();
        assert!(
            text.contains("bin/cashcast actions"),
            "{entry} points agents at the catalog"
        );
    }
    let r = web::handle(
        "GET",
        "/api/actions",
        &headers(TOKEN, "application/json"),
        TOKEN,
        PORT,
        today(),
        b"",
    );
    assert_eq!(r.0, 200);
    let web: Vec<Value> = serde_json::from_slice(&r.2).unwrap();
    assert_eq!(names(&web), names(&cat));
}

/// Need the built engine's data or a slow import: the e2e stories cover them.
const SKIP: [&str; 9] = [
    "import", "status", "forecast", "backtest", "totals", "columns", "holdings", "numbers", "rrsp",
];

#[test]
fn every_argument_free_action_runs_from_the_web_app() {
    let env = Env::new();
    env.user("alex");
    let cat = catalog();
    let mut ran = 0;
    for a in &cat {
        let name = a["name"].as_str().unwrap();
        let required = a["args"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|x| x["required"] == true);
        if a["ui"] != true || truthy(&a["confirm"]) || required || SKIP.contains(&name) {
            continue;
        }
        let (status, out) = post(&format!("/api/actions/{name}?user=alex"), "{}");
        assert_eq!(status, 200, "{name}: {out}");
        assert!(out["exit"].is_i64(), "{name}: {out}");
        let text = out["output"].as_str().unwrap_or("");
        assert!(
            !text.contains("Traceback") && !text.contains("panicked"),
            "{name}: {text}"
        );
        ran += 1;
    }
    assert!(ran >= 10, "only {ran} argument-free actions ran");
}

#[test]
fn terminal_only_confirm_and_validation() {
    let env = Env::new();
    env.user("alex");
    let err = |path: &str, body: &str, want: &str| {
        let (status, out) = post(path, body);
        assert_eq!(status, 400, "{path} {body}: {out}");
        let e = out["error"].as_str().unwrap_or("");
        assert!(e.contains(want), "{path} {body}: {e}");
    };
    err("/api/actions/web", "{}", "terminal only");
    err(
        "/api/actions/consent",
        r#"{"values": {"action": "grant", "scope": "agent:strategy"}}"#,
        "terminal only",
    );
    err(
        "/api/actions/calendar?user=alex",
        r#"{"values": {"action": "sync"}}"#,
        "confirm",
    );
    err(
        "/api/actions/action",
        r#"{"values": {"service": "td"}}"#,
        "required",
    );
    err(
        "/api/actions/calendar",
        r#"{"values": {"action": "delete"}}"#,
        "one of",
    );
    err(
        "/api/actions/review",
        r#"{"values": {"nope": 1}}"#,
        "unknown field",
    );
    err(
        "/api/actions/action",
        r#"{"values": {"service": "--user", "action_id": "x"}}"#,
        "cannot start with",
    );
    let (status, out) = post(
        "/api/actions/config",
        r#"{"values": {"action": "set", "key": "start.tasks", "value": "3"}}"#,
    );
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["exit"], 0, "{out}");
    assert_eq!(
        out["argv"],
        serde_json::json!(["bin/cashcast", "config", "set", "start.tasks", "3"])
    );
    let (_, out) = post("/api/actions/config", "{}");
    assert!(
        out["output"].as_str().unwrap().contains("start.tasks = 3"),
        "{out}"
    );
}

#[test]
fn a_web_run_is_the_owner_so_the_gate_shows_the_report() {
    let env = Env::new();
    env.person(&[("Chequing", 40000.0)]);
    serve(&env);
    app::set("CLAUDECODE", "1"); // the server was started from an agent's shell
    let (status, out) = post("/api/actions/review?user=alex", "{}");
    app::unset("CLAUDECODE");
    assert_eq!(status, 200, "{out}");
    let text = out["output"].as_str().unwrap();
    assert!(
        !text.contains("Report saved for the owner only") && text.contains("Monthly money review"),
        "{text}"
    );
}

#[test]
fn post_runs_an_action_with_token_and_json_only() {
    let env = Env::new();
    env.user("alex");
    serve(&env);
    let body = r#"{"values": {"action": "list"}}"#;
    let path = "/api/actions/changes?user=alex";
    assert_eq!(
        post_with(path, body, &headers("bad", "application/json")).0,
        401
    );
    assert_eq!(post_with(path, body, &headers(TOKEN, "text/plain")).0, 415);
    assert_eq!(post("/api/plan", body).0, 405);
    let (status, out) = post(path, body);
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["exit"], 0, "{out}");
    assert!(
        out["output"]
            .as_str()
            .unwrap()
            .contains("Bank and card changes"),
        "{out}"
    );
    let (status, out) = post(
        "/api/actions/calendar?user=alex",
        r#"{"values": {"action": "sync"}}"#,
    );
    assert_eq!(status, 400);
    assert!(out["error"].as_str().unwrap().contains("confirm"));
}
