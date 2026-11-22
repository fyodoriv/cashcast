//! The web app's local API (US-13) and the documents moved from Notion (US-15):
//! loopback host only, token required, no free text, offline import, private
//! files. Ported from tests/unit/test_web.py and test_docs.py.

mod app;

use app::{Env, mode, read, set, today};
use cashcast_engine::core::paths::UserHome;
use cashcast_engine::docs;
use cashcast_engine::web;
use std::io::Write;
use std::path::{Path, PathBuf};

const TOKEN: &str = "tok123";
const PORT: u16 = 8765;
const ID: &str = "0123456789abcdef0123456789abcdef";
const ID2: &str = "fedcba9876543210fedcba9876543210";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

fn ok() -> Vec<(String, String)> {
    vec![
        ("Host".into(), format!("127.0.0.1:{PORT}")),
        ("X-Cashcast-Token".into(), TOKEN.into()),
    ]
}

fn call_with(path: &str, headers: &[(String, String)], method: &str) -> web::Reply {
    web::handle(method, path, headers, TOKEN, PORT, today(), b"")
}

fn call(path: &str) -> web::Reply {
    call_with(path, &ok(), "GET")
}

fn json(r: &web::Reply) -> serde_json::Value {
    serde_json::from_slice(&r.2).unwrap()
}

// ---------- web (test_web.py) ----------

#[test]
fn api_needs_the_loopback_host_and_the_token() {
    let env = Env::new();
    env.person(&[("Chequing", 40000.0)]);
    let host = |h: &str| ("Host".to_string(), h.to_string());
    let tok = |t: &str| ("X-Cashcast-Token".to_string(), t.to_string());
    assert_eq!(
        call_with(
            "/api/users",
            &[host(&format!("evil.example:{PORT}")), tok(TOKEN)],
            "GET"
        )
        .0,
        403
    );
    assert_eq!(
        call_with("/api/users", &[host(&format!("127.0.0.1:{PORT}"))], "GET").0,
        401
    );
    assert_eq!(
        call_with(
            "/api/users",
            &[host(&format!("127.0.0.1:{PORT}")), tok("wrong")],
            "GET"
        )
        .0,
        401
    );
    assert_eq!(call_with("/api/users", &ok(), "POST").0, 405);
    let r = call("/api/users");
    assert_eq!(r.0, 200);
    assert_eq!(json(&r)[0]["name"], "alex");
}

#[test]
fn plan_payload_has_every_view_and_no_free_text() {
    let env = Env::new();
    env.person(&[("Chequing", 40000.0)]);
    let r = call("/api/plan?user=alex");
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    let plan = json(&r);
    let ids: Vec<&str> = plan["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    for want in [
        "cashback",
        "credit",
        "reserve",
        "tax",
        "payments",
        "budget",
        "alternatives",
        "review",
    ] {
        assert!(ids.contains(&want), "{want}");
    }
    let truthy = |v: &serde_json::Value| {
        !(v.is_null()
            || v.as_array().is_some_and(|a| a.is_empty())
            || v.as_object().is_some_and(|o| o.is_empty()))
    };
    for k in ["forecast", "tasks", "connectors", "consent"] {
        assert!(truthy(&plan[k]), "{k}");
    }
    assert!(truthy(&plan["cashback"]["routes"]));
    let body = String::from_utf8_lossy(&r.2);
    assert!(!body.contains("CANARY") && !body.contains("section failed"));
}

#[test]
fn unknown_user_is_a_clean_error() {
    let env = Env::new();
    env.person(&[("Chequing", 40000.0)]);
    let r = call("/api/plan?user=nobody");
    assert_eq!(r.0, 400);
    assert!(json(&r)["error"].as_str().unwrap().contains("profile"));
}

#[test]
fn static_files_cannot_escape_the_build_folder() {
    let env = Env::new();
    let dist = env.tmp.join("web/dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>app</html>").unwrap();
    set("CASHCAST_ROOT", &env.tmp.display().to_string());
    for path in ["/", "/../../etc/passwd", "/%2e%2e/%2e%2e/etc/hosts"] {
        assert_eq!(call(path).2, b"<html>app</html>", "{path}");
    }
}

// test_the_ui_fixture_matches_the_api: dropped here; it runs
// scripts/web-fixture.sh, which rebuilds the fixture from this engine.
// test_bank_sync_reads_only_stdout_so_a_warning_does_not_break_it: dropped;
// it patched Python's subprocess call to the engine. Rust builds the payload
// in-process.

// ---------- documents (test_docs.py) ----------

fn zip_of(path: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, data) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    std::fs::write(path, buf.into_inner()).unwrap();
    path.to_path_buf()
}

fn export_zip(path: &Path) -> PathBuf {
    let finances = format!(
        "# Finances\n\n[Taxes](Finances%20{ID}/Taxes%20{ID2}.md)\n\n[Budget](Finances%20{ID}/Budget%20{ID2}.csv)\n"
    );
    let taxes = format!(
        "# Taxes\n\nNote CANARY_NOTION_1 about tax.\n\n- [ ] File the return\n- [x] Ask the accountant\n\n\
         ![chart](Taxes%20{ID2}/chart.png)\n\n[outside](https://example.com/CANARY_NOTION_URL)\n\n[escape](../../../etc/passwd)\n"
    );
    let names = [
        format!("Finances {ID}.md"),
        format!("Finances {ID}/Taxes {ID2}.md"),
        format!("Finances {ID}/Taxes {ID2}/chart.png"),
        format!("Finances {ID}/Budget {ID2}.csv"),
        format!("Finances {ID}/Budget {ID2}_all.csv"),
        format!("Finances {ID}/notes {ID2}.docx"),
    ];
    let data: [&[u8]; 6] = [
        finances.as_bytes(),
        taxes.as_bytes(),
        PNG,
        b"Month,Amount\nJan,10\n",
        b"Month,Amount\nJan,10\nFeb,CANARY_NOTION_2\n",
        b"binary",
    ];
    let files: Vec<(&str, &[u8])> = names.iter().map(String::as_str).zip(data).collect();
    zip_of(path, &files)
}

fn imported(env: &Env) -> (UserHome, docs::ImportReport) {
    let u = env.user("alex");
    let src = export_zip(&env.tmp.join("Export.zip"));
    let report = docs::import_notion(&u, &src).unwrap();
    (u, report)
}

fn files_under(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn import_cleans_names_converts_tables_and_skips_the_rest() {
    let env = Env::new();
    let (u, report) = imported(&env);
    assert_eq!((report.pages, report.tables, report.files), (2, 1, 1));
    assert_eq!(report.skipped, vec!["unsupported file type"]);
    let root = u.docs().join("notion");
    let names = files_under(&root);
    assert_eq!(
        names,
        vec![
            "Finances.md",
            "Finances/Budget.md",
            "Finances/Taxes.md",
            "Finances/Taxes/chart.png"
        ]
    );
    assert!(!names.concat().contains(ID));
    let table = read(&root.join("Finances/Budget.md"));
    assert!(
        table.contains("| Month | Amount |") && table.contains("CANARY_NOTION_2"),
        "the _all export wins"
    );
}

#[test]
fn links_point_at_the_local_copies_and_never_escape() {
    let env = Env::new();
    let (u, _) = imported(&env);
    let page = read(&u.docs().join("notion/Finances.md"));
    assert!(
        page.contains("(doc:notion/Finances/Taxes.md)")
            && page.contains("(doc:notion/Finances/Budget.md)")
    );
    let taxes = read(&u.docs().join("notion/Finances/Taxes.md"));
    assert!(taxes.contains("(doc:notion/Finances/Taxes/chart.png)"));
    assert!(
        !taxes.contains("etc/passwd") && taxes.contains("escape"),
        "a link out of the export becomes plain text"
    );
    assert!(
        taxes.contains("https://example.com/CANARY_NOTION_URL"),
        "outside links stay"
    );
}

#[test]
fn files_are_owner_only() {
    let env = Env::new();
    let (u, _) = imported(&env);
    assert_eq!(mode(&u.docs()), 0o700);
    let root = u.docs();
    for rel in files_under(&root) {
        let p = root.join(&rel);
        assert_eq!(mode(&p), 0o600, "{rel}");
        let mut dir = p.parent().unwrap().to_path_buf();
        while dir != root {
            assert_eq!(mode(&dir), 0o700, "{}", dir.display());
            dir = dir.parent().unwrap().to_path_buf();
        }
    }
}

#[test]
fn report_names_no_page_and_no_text() {
    let env = Env::new();
    let (_, report) = imported(&env);
    let text = report.render("private/users/alex/docs/notion");
    assert!(text.contains("Imported 2 pages, 1 tables and 1 files"));
    assert!(!text.contains("CANARY") && !text.contains("Taxes") && !text.contains("Budget"));
}

fn titles(r: &web::Reply) -> Vec<String> {
    let mut t: Vec<String> = json(r)
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["title"].as_str().unwrap().to_string())
        .collect();
    t.sort();
    t
}

#[test]
fn reimport_replaces_the_old_copy() {
    let env = Env::new();
    let (u, _) = imported(&env);
    std::fs::write(u.docs().join("my-note.md"), "# Mine\n").unwrap();
    let only = format!("Only {ID}.md");
    let src = zip_of(&env.tmp.join("second.zip"), &[(&only, b"# Only\n")]);
    docs::import_notion(&u, &src).unwrap();
    assert_eq!(titles(&call("/api/docs?user=alex")), vec!["Mine", "Only"]);
}

#[test]
fn unsafe_zip_entries_are_skipped() {
    let env = Env::new();
    let u = env.user("alex");
    let dots = format!(".. {ID}/.. {ID}/evil2.md");
    let src = zip_of(
        &env.tmp.join("bad.zip"),
        &[
            ("../../evil.md", b"# x"),
            (&dots, b"# x"),
            ("ok.md", b"# ok"),
        ],
    );
    let report = docs::import_notion(&u, &src).unwrap();
    assert!(report.pages == 1 && report.skipped.contains(&"unsafe path"));
    for p in [
        env.tmp.join("evil.md"),
        u.home.join("evil.md"),
        u.home.join("evil2.md"),
        u.docs().join("evil2.md"),
    ] {
        assert!(!p.exists(), "{}", p.display());
    }
}

#[test]
fn pages_with_the_same_title_are_both_kept_and_links_follow_them() {
    let env = Env::new();
    let u = env.user("alex");
    let home =
        format!("# Home\n\n[a](Home%20{ID}/Note%20{ID}.md) [b](Home%20{ID}/Note%20{ID2}.md)\n");
    let a = format!("# Note\n\nCANARY_NOTE_A\n\n[up](../Home%20{ID}.md)\n");
    let names = [
        format!("Home {ID}.md"),
        format!("Home {ID}/Note {ID}.md"),
        format!("Home {ID}/Note {ID2}.md"),
    ];
    let src = zip_of(
        &env.tmp.join("export.zip"),
        &[
            (&names[0], home.as_bytes()),
            (&names[1], a.as_bytes()),
            (&names[2], b"# Note\n\nCANARY_NOTE_B\n"),
        ],
    );
    let report = docs::import_notion(&u, &src).unwrap();
    assert_eq!(report.pages, 3);
    let root = u.docs().join("notion");
    assert_eq!(
        files_under(&root),
        vec!["Home.md", "Home/Note (2).md", "Home/Note.md"]
    );
    let text = |rel: &str| read(&root.join(rel));
    assert!(
        text("Home/Note.md").contains("CANARY_NOTE_A")
            && text("Home/Note (2).md").contains("CANARY_NOTE_B")
    );
    assert!(
        text("Home.md").contains("(doc:notion/Home/Note.md)")
            && text("Home.md").contains("(doc:notion/Home/Note%20%282%29.md)")
    );
    assert!(text("Home/Note.md").contains("[up](doc:notion/Home.md)"));
}

#[test]
fn oversized_pages_are_skipped_not_imported_empty() {
    let env = Env::new();
    let u = env.user("alex");
    let big = vec![b'a'; 25 * 1024 * 1024 + 1];
    let src = zip_of(
        &env.tmp.join("export.zip"),
        &[("big.md", &big), ("ok.md", b"# ok")],
    );
    let report = docs::import_notion(&u, &src).unwrap();
    assert!(report.pages == 1 && report.skipped == vec!["over 25 MB"]);
    assert!(!u.docs().join("notion/big.md").exists());
}

#[test]
fn api_reads_a_name_with_a_percent_sign() {
    let env = Env::new();
    let (u, _) = imported(&env);
    std::fs::write(u.docs().join("Save 10%20 more.md"), "# Pct\n").unwrap();
    let r = call("/api/doc?user=alex&path=Save%2010%2520%20more.md");
    assert_eq!(r.0, 200);
    assert_eq!(json(&r)["title"], "Pct");
}

#[test]
fn not_an_export_is_a_clear_error() {
    let env = Env::new();
    let u = env.user("alex");
    let bad = env.tmp.join("x.txt");
    std::fs::write(&bad, "hello").unwrap();
    let Err(e) = docs::import_notion(&u, &bad) else {
        panic!("a text file imported")
    };
    assert!(e.message.contains("not a Notion export"), "{}", e.message);
}

#[test]
fn api_lists_reads_searches_and_serves_files_with_the_token_only() {
    let env = Env::new();
    imported(&env);
    let r = call("/api/docs?user=alex");
    assert_eq!(r.0, 200);
    assert_eq!(titles(&r), vec!["Budget", "Finances", "Taxes"]);
    let r = call("/api/doc?user=alex&path=notion/Finances/Taxes.md");
    assert!(
        r.0 == 200
            && json(&r)["markdown"]
                .as_str()
                .unwrap()
                .contains("CANARY_NOTION_1")
    );
    assert_eq!(
        titles(&call("/api/docs?user=alex&q=canary_notion_1")),
        vec!["Taxes"]
    );
    let r = call("/api/doc-file?user=alex&path=notion/Finances/Taxes/chart.png");
    assert_eq!((r.0, r.1.as_str(), r.2.as_slice()), (200, "image/png", PNG));
    let no_token = [("Host".to_string(), format!("127.0.0.1:{PORT}"))];
    assert_eq!(
        call_with(
            "/api/doc?user=alex&path=notion/Finances/Taxes.md",
            &no_token,
            "GET"
        )
        .0,
        401
    );
}

#[test]
fn api_cannot_read_outside_the_documents() {
    let env = Env::new();
    imported(&env);
    for path in [
        "/api/doc?user=alex&path=../profile.toml",
        "/api/doc?user=alex&path=%2e%2e/consent.toml",
        "/api/doc?user=alex&path=/etc/passwd",
        "/api/doc-file?user=alex&path=../profile.toml",
        "/api/doc?user=alex&path=notion/Finances/Taxes/chart.png",
        "/api/doc?user=alex&path=.notion-new/x.md",
    ] {
        assert_eq!(call(path).0, 404, "{path}");
    }
}

#[test]
fn server_forbids_loading_anything_from_the_internet() {
    for want in [
        "default-src 'self'",
        "img-src 'self' blob:",
        "frame-ancestors 'none'",
        "connect-src 'self'",
    ] {
        assert!(web::CSP.contains(want), "{want}");
    }
}
