//! The second half of `bin/cashcast doctor`: users, recent errors, knowledge
//! and connectors. Every failing line says how to fix it. Prints no private values.

use super::paths;
use super::pyjson::{self, Json};
use super::users;
use std::collections::BTreeMap;
use std::path::Path;

/// The import formats a connector may name (the readers in export.rs, ofx.rs, pdf.rs and ledger.rs).
pub const FORMATS: [&str; 30] = [
    "moneywiz-csv",
    "moneywiz-database",
    "rbc-csv",
    "scotiabank-csv",
    "scotiabank-legacy-csv",
    "td-csv",
    "wise-statement-csv",
    "wealthsimple-statement-csv",
    "wealthsimple-activity-csv",
    "wealthsimple-activities-csv",
    "paypal-activity-csv",
    "kraken-ledger-csv",
    "etrade-transactions-csv",
    "etrade-stockplan-csv",
    "etrade-benefit-rs",
    "etrade-benefit-espp",
    "etrade-gains-losses",
    "casa-csv",
    "sunlife-transactions-csv",
    "tinkoff-csv",
    "crypto-generic-csv",
    "ofx",
    "generic-csv",
    "td-credit-pdf",
    "td-chequing-pdf",
    "scotiabank-visa-pdf",
    "scotiabank-chequing-pdf",
    "etrade-stockplan-pdf",
    "sunlife-statement-pdf",
    "scotiabank-ssi-pdf",
];
pub const RISKS: [&str; 3] = ["read", "settings", "money"];
const REQUIRED: [&str; 10] = [
    "id",
    "name",
    "kinds",
    "website",
    "available_in_canada",
    "import",
    "api",
    "browser",
    "actions",
    "meta",
];

fn list(v: Option<&Json>) -> Vec<Json> {
    match v {
        Some(Json::List(items)) => items.clone(),
        _ => Vec::new(),
    }
}

fn text(v: Option<&Json>) -> String {
    v.map_or(String::new(), Json::py_str)
}

/// The problems in one connector spec; empty when it is valid.
pub fn validate(spec: &Json, dir: &Path) -> Vec<String> {
    let missing: Vec<String> = REQUIRED
        .iter()
        .filter(|k| spec.get(k).is_none())
        .map(|k| format!("missing {k}"))
        .collect();
    if !missing.is_empty() {
        return missing;
    }
    let mut problems = Vec::new();
    let imp = spec.get("import");
    let formats = list(imp.and_then(|i| i.get("formats")));
    let bad: Vec<String> = formats
        .iter()
        .map(Json::py_str)
        .filter(|f| !FORMATS.contains(&f.as_str()))
        .map(|f| format!("'{f}'"))
        .collect();
    if !bad.is_empty() {
        problems.push(format!("unknown formats [{}]", bad.join(", ")));
    }
    let truthy = |k: &str| imp.and_then(|i| i.get(k)).is_some_and(Json::truthy);
    if formats.is_empty() && !truthy("no_export_reason") {
        problems.push("no formats and no no_export_reason".into());
    }
    if !truthy("how_to_export") && !formats.is_empty() {
        problems.push("formats without how_to_export".into());
    }
    let playbook = text(spec.get("browser").and_then(|b| b.get("playbook")));
    if !dir.join(playbook).exists() {
        problems.push("playbook file missing".into());
    }
    for a in list(spec.get("actions")) {
        let id = text(a.get("id"));
        let risk = text(a.get("risk"));
        if !RISKS.contains(&risk.as_str()) {
            problems.push(format!(
                "action {id}: risk must be one of ('read', 'settings', 'money')"
            ));
        }
        let human =
            matches!(a.get("via"), Some(Json::List(v)) if v.len() == 1 && v[0].py_str() == "human");
        if risk == "money" && !human {
            problems.push(format!(
                "action {id}: money actions must be via = [\"human\"]"
            ));
        }
    }
    if !spec
        .get("meta")
        .and_then(|m| m.get("sources"))
        .is_some_and(Json::truthy)
    {
        problems.push("meta.sources is empty".into());
    }
    problems
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn ts(e: &Json) -> f64 {
    match e.get("ts") {
        Some(Json::Int(i)) => *i as f64,
        Some(Json::Float(f)) => *f,
        _ => 0.0,
    }
}

/// Runs the checks and prints one line each. Returns the exit code: 1 when a check fails.
pub fn main() -> i32 {
    let mut bad = 0;
    let names = paths::list_users();
    if names.is_empty() {
        println!("· No users yet. Run: bin/cashcast users add NAME --province XX");
    } else {
        println!("✓ Users: {}", names.join(", "));
    }
    for name in &names {
        let user = match paths::resolve(Some(name)) {
            Ok(u) => u,
            Err(e) => {
                println!("✗ {name}: {}", e.message);
                bad += 1;
                continue;
            }
        };
        if let Err(e) = users::load_profile(&user) {
            println!("✗ {name}: {}", e.message);
            bad += 1;
        }
        let since = now() - 30.0 * 86400.0;
        let log = std::fs::read_to_string(user.logs().join("errors.jsonl")).unwrap_or_default();
        let mut stories: BTreeMap<String, usize> = BTreeMap::new();
        let mut recent = 0;
        for e in log.lines().filter_map(pyjson::loads) {
            if ts(&e) > since {
                recent += 1;
                let s = e.get("story").map_or("US-00".into(), Json::py_str);
                *stories.entry(s).or_default() += 1;
            }
        }
        if recent > 0 {
            let by: Vec<String> = stories.iter().map(|(s, n)| format!("{s} ×{n}")).collect();
            println!(
                "✗ {name}: {recent} error(s) in 30 days, by story: {}. Fix the story with the most errors first; see docs/user-stories.md.",
                by.join(", ")
            );
            bad += 1;
        }
    }
    let today = chrono::Local::now().date_naive();
    match crate::knowledge::check(today, &crate::knowledge::data_dir()) {
        Ok(k) if k.ok() && k.stale.is_empty() => println!(
            "✓ Knowledge: {} files, {} sourced facts ({} estimates)",
            k.files,
            k.facts,
            k.estimates.len()
        ),
        Ok(k) => {
            println!(
                "✗ Knowledge: {} stale, {} problems. Run: bin/cashcast knowledge check",
                k.stale.len(),
                k.problems.len() + k.missing_source.len()
            );
            bad += 1;
        }
        Err(_) => {
            println!("✗ Knowledge: a file does not parse. Run: bin/cashcast knowledge check");
            bad += 1;
        }
    }
    let dir = crate::tasks::connectors::dir();
    match crate::tasks::connectors::load_all() {
        Ok(specs) => {
            let broken: Vec<&str> = specs
                .iter()
                .filter(|(_, s)| !validate(s, &dir).is_empty())
                .map(|(sid, _)| sid.as_str())
                .collect();
            if broken.is_empty() {
                println!("✓ Connectors: {} services", specs.len());
            } else {
                println!(
                    "✗ Connectors with problems: {}. Run the connector tests: cargo test --manifest-path engine/Cargo.toml --test app_tasks spec",
                    broken.join(", ")
                );
                bad += 1;
            }
        }
        Err(e) => {
            println!("✗ Connectors: {}", e.0);
            bad += 1;
        }
    }
    if bad == 0 {
        println!("doctor: all checks passed");
        0
    } else {
        println!("doctor: {bad} check(s) need attention");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(extra: &str) -> Json {
        let base = r#"{"id": "x", "name": "X", "kinds": ["bank"], "website": "https://x.example.com",
            "available_in_canada": true, "import": {"formats": ["td-csv"], "how_to_export": "menu"},
            "api": {"available": false}, "browser": {"playbook": "playbooks/x.md"},
            "actions": [ACTIONS], "meta": {"sources": ["https://x.example.com"]}}"#;
        pyjson::loads(&base.replace("ACTIONS", extra)).unwrap()
    }

    #[test]
    fn a_valid_spec_has_no_problems_and_a_missing_playbook_is_one() {
        let dir = std::env::temp_dir().join(format!("health-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("playbooks")).unwrap();
        std::fs::write(dir.join("playbooks/x.md"), "# X\n").unwrap();
        let ok = spec(r#"{"id": "a", "risk": "read"}"#);
        assert!(validate(&ok, &dir).is_empty(), "{:?}", validate(&ok, &dir));
        std::fs::remove_file(dir.join("playbooks/x.md")).unwrap();
        assert_eq!(validate(&ok, &dir), vec!["playbook file missing"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn money_actions_must_be_human_and_risks_known() {
        let s = spec(
            r#"{"id": "pay", "risk": "money", "via": ["agent"]}, {"id": "b", "risk": "nope"}"#,
        );
        let p = validate(&s, Path::new("/nonexistent"));
        assert!(p.contains(&"action pay: money actions must be via = [\"human\"]".to_string()));
        assert!(
            p.iter()
                .any(|x| x.starts_with("action b: risk must be one of"))
        );
    }

    #[test]
    fn missing_keys_and_unknown_formats_are_named() {
        let p = validate(&pyjson::loads(r#"{"id": "x"}"#).unwrap(), Path::new("."));
        assert!(p.contains(&"missing name".to_string()));
        let s = spec("").to_owned();
        let mut s2 = s.clone();
        s2.set(
            "import",
            pyjson::loads(r#"{"formats": ["nope-csv"], "how_to_export": "x"}"#).unwrap(),
        );
        assert!(
            validate(&s2, Path::new(".")).contains(&"unknown formats ['nope-csv']".to_string())
        );
    }
}
