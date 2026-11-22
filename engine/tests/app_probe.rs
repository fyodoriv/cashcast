//! The MoneyWiz probe reads an app bundle and the window's shape, never ledger
//! data. Ported from tests/unit/test_moneywiz_probe.py.

mod app;

use app::{Env, set, unset};
use cashcast_engine::tasks::moneywiz_app::{self as mw, VERIFIED_APP, VERIFIED_MODEL};
use plist::{Dictionary, Value};
use std::path::{Path, PathBuf};

const CANARY: &str = "CANARY-PAYEE-7731";

#[derive(Default)]
struct Fake<'a> {
    sdef: &'a str,
    intents: &'a [&'a str],
    model: Option<&'a str>,
    version: Option<&'a str>,
    catalyst: bool,
}

fn fake_app(tmp: &Path, f: Fake) -> PathBuf {
    let app = tmp.join("MoneyWiz 2026.app");
    let res = app.join("Contents/Resources");
    std::fs::create_dir_all(&res).unwrap();
    let mut info = Dictionary::new();
    info.insert(
        "CFBundleShortVersionString".into(),
        Value::String(f.version.unwrap_or("2026.30.0").into()),
    );
    let mut url = Dictionary::new();
    url.insert(
        "CFBundleURLSchemes".into(),
        Value::Array(vec![Value::String("moneywiz".into())]),
    );
    info.insert(
        "CFBundleURLTypes".into(),
        Value::Array(vec![Value::Dictionary(url)]),
    );
    if f.catalyst {
        info.insert(
            "UIDeviceFamily".into(),
            Value::Array(vec![Value::Integer(2.into()), Value::Integer(6.into())]),
        );
    }
    if !f.sdef.is_empty() {
        info.insert(
            "OSAScriptingDefinition".into(),
            Value::String("MoneyWiz.sdef".into()),
        );
        std::fs::write(res.join("MoneyWiz.sdef"), f.sdef).unwrap();
    }
    Value::Dictionary(info)
        .to_file_xml(app.join("Contents/Info.plist"))
        .unwrap();
    if !f.intents.is_empty() {
        let dir = res.join("Metadata.appintents");
        std::fs::create_dir_all(&dir).unwrap();
        let actions: serde_json::Map<String, serde_json::Value> = f
            .intents
            .iter()
            .map(|i| (i.to_string(), serde_json::json!({})))
            .collect();
        std::fs::write(
            dir.join("extract.actionsdata"),
            serde_json::json!({ "actions": actions }).to_string(),
        )
        .unwrap();
    }
    let momd = res.join("MoneyWizDataModel.momd");
    std::fs::create_dir_all(&momd).unwrap();
    let mut v = Dictionary::new();
    v.insert(
        "NSManagedObjectModel_CurrentVersionName".into(),
        Value::String(f.model.unwrap_or("MoneyWizDataModel 47").into()),
    );
    Value::Dictionary(v)
        .to_file_xml(momd.join("VersionInfo.plist"))
        .unwrap();
    app
}

fn probe(env: &Env, f: Fake, ui: bool) -> mw::Probe {
    let app = fake_app(&env.tmp, f);
    set("CASHCAST_MONEYWIZ_APP", &app.display().to_string());
    let p = mw::probe(ui);
    unset("CASHCAST_MONEYWIZ_APP");
    p
}

fn best(p: &mw::Probe) -> String {
    let out = mw::render(p);
    out.lines()
        .find_map(|l| l.strip_prefix("Best way in: "))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn url_scheme_only_install_has_no_edit_path() {
    let env = Env::new();
    let p = probe(&env, Fake::default(), false);
    assert_eq!(
        (p.schemes.clone(), p.kind.as_str(), p.model.as_str()),
        (
            vec!["moneywiz".to_string()],
            "Mac (AppKit)",
            "MoneyWizDataModel 47"
        )
    );
    assert!(!(p.applescript() || p.shortcuts() || p.core_data_verified() || p.window()));
    let out = mw::render(&p);
    assert!(
        out.contains("version 2026.30.0") && out.contains("moneywiz probe --ui"),
        "{out}"
    );
}

#[test]
fn applescript_dictionary_with_transaction_terms_wins() {
    let env = Env::new();
    let sdef = r#"<dictionary><suite name="MoneyWiz"><class name="transaction"/><command name="delete"/></suite></dictionary>"#;
    let p = probe(
        &env,
        Fake {
            sdef,
            intents: &["AddExpenseIntent"],
            catalyst: true,
            ..Fake::default()
        },
        false,
    );
    assert!(p.has_dictionary && p.applescript() && p.kind == "Mac Catalyst");
    assert!(!p.shortcuts(), "an add-only Shortcuts action cannot edit");
    assert!(best(&p).starts_with("AppleScript"));
}

#[test]
fn edit_intent_and_verified_model() {
    let env = Env::new();
    let f = Fake {
        intents: &["UpdateTransactionIntent"],
        model: Some(VERIFIED_MODEL),
        version: Some(VERIFIED_APP),
        ..Fake::default()
    };
    let mut p = probe(&env, f, false);
    assert!(p.shortcuts() && p.core_data_verified() && best(&p).starts_with("Shortcuts"));
    p.intents.clear();
    assert!(best(&p).starts_with("the Core Data writer"));
}

#[test]
fn verified_model_in_a_newer_app_is_a_way_in() {
    let env = Env::new();
    let p = probe(
        &env,
        Fake {
            model: Some(VERIFIED_MODEL),
            version: Some("2026.38.1"),
            ..Fake::default()
        },
        false,
    );
    assert!(p.core_data_verified() && best(&p).starts_with("the Core Data writer"));
    assert!(mw::render(&p).contains("newer app: the copy test checks it"));
}

/// A fake `osascript` on PATH that prints `out` and exits with `code`.
fn fake_osascript(env: &Env, out: &str, code: i32) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin = env.tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(env.tmp.join("ui.json"), out).unwrap();
    let exe = bin.join("osascript");
    std::fs::write(
        &exe,
        format!(
            "#!/bin/sh\ncat '{}'\nexit {code}\n",
            env.tmp.join("ui.json").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = std::env::var("PATH").unwrap_or_default();
    set("PATH", &format!("{}:{old}", bin.display()));
    old
}

#[test]
fn ui_probe_prints_shape_not_titles() {
    let env = Env::new();
    let raw = serde_json::json!({
        "windows": 1,
        "items": ["New Transaction", "Delete", "Undo", format!("Go to {CANARY}")],
        "roles": {"AXRow": 40, "AXButton": 12, CANARY: 1},
        "ids": 3,
    });
    let old = fake_osascript(&env, &raw.to_string(), 0);
    set("CASHCAST_MONEYWIZ_PROCESS", "MoneyWiz 2026");
    let p = probe(&env, Fake::default(), true);
    set("PATH", &old);
    unset("CASHCAST_MONEYWIZ_PROCESS");
    let ui = p.ui.as_ref().expect("the window was read");
    let menu = |k: &str| ui.menu.iter().find(|(m, _)| *m == k).unwrap().1;
    assert!(p.window() && menu("new transaction") && !menu("rules"));
    let out = mw::render(&p);
    assert!(
        out.contains("AXRow 40") && out.contains("3 elements with an accessibility id"),
        "{out}"
    );
    assert!(
        !out.contains(CANARY),
        "menu titles and odd role names never reach the output"
    );
    assert!(best(&p).starts_with("the MoneyWiz window"));
}

#[test]
fn ui_probe_names_the_accessibility_fix() {
    let env = Env::new();
    let old = fake_osascript(&env, "", 1);
    set("CASHCAST_MONEYWIZ_PROCESS", "MoneyWiz");
    let p = probe(&env, Fake::default(), true);
    set("PATH", &old);
    unset("CASHCAST_MONEYWIZ_PROCESS");
    assert!(p.ui.is_none() && mw::render(&p).contains("Accessibility"));
}

#[test]
fn missing_app() {
    let env = Env::new();
    set(
        "CASHCAST_MONEYWIZ_APP",
        &env.tmp.join("nowhere.app").display().to_string(),
    );
    let p = mw::probe(false);
    unset("CASHCAST_MONEYWIZ_APP");
    assert!(p.app.is_none() && mw::render(&p).contains("not in /Applications"));
}
