//! The MoneyWiz app around the engine, as `moneywiz.py`, `moneywiz_write.py`
//! and `moneywiz_probe.py` do it: open or quit the app, apply a sync, run
//! in-place edits and category migrations through the Core Data writer
//! (decision 0007), and probe which ways in an install offers.
//!
//! Only the engine subcommands read transactions. This module prints their
//! reports, which hold names, counts and sums only.

use crate::core::paths::{self, AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::core::{consent, events, gate};
use chrono::NaiveDate;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const APP_NAMES: [&str; 5] = [
    "MoneyWiz",
    "MoneyWiz 2026",
    "MoneyWiz 2025",
    "MoneyWiz 2024",
    "MoneyWiz 3",
];
const KEEP_BACKUPS: usize = 10;
pub const VERIFIED_MODEL: &str = "MoneyWizDataModel 48";
pub const VERIFIED_APP: &str = "2026.32.1";

fn runtime(msg: impl Into<String>) -> AppError {
    AppError::new("runtimeerror", msg)
}

fn os_err(e: impl std::fmt::Display) -> AppError {
    AppError::io_text(e.to_string())
}

/// Runs `cashcast-engine ARGS… --home DIR`: (exit code, stdout then stderr).
pub type Engine<'a> = &'a dyn Fn(&[&str]) -> Result<(i32, String), AppError>;

fn fake_app() -> bool {
    std::env::var("CASHCAST_MONEYWIZ_OPEN").is_ok_and(|v| !v.is_empty())
}

fn sleep(seconds: f64) {
    std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
}

/// The name of the running MoneyWiz app, or None.
pub fn running_app() -> Option<&'static str> {
    for name in APP_NAMES {
        match Command::new("pgrep").args(["-x", name]).output() {
            Err(_) => return None,
            Ok(o) if o.status.success() => return Some(name),
            Ok(_) => {}
        }
    }
    None
}

/// Starts MoneyWiz in the background when it is closed.
pub fn ensure_open() -> Result<(), AppError> {
    if fake_app() || running_app().is_some() {
        return Ok(());
    }
    let opened = APP_NAMES.iter().any(|name| {
        Command::new("open")
            .args(["-g", "-a", name])
            .output()
            .is_ok_and(|o| o.status.success())
    });
    if !opened {
        return Err(runtime(
            "cannot open MoneyWiz. Open it yourself, then run the command again.",
        ));
    }
    let mut waited = 0.0;
    while running_app().is_none() && waited < 20.0 {
        sleep(0.5);
        waited += 0.5;
    }
    sleep(3.0); // let it load its database before the first link arrives
    Ok(())
}

fn quit_app(name: Option<&str>) -> Result<(), AppError> {
    let Some(name) = name else { return Ok(()) };
    if fake_app() {
        return Ok(());
    }
    let _ = Command::new("osascript")
        .args(["-e", &format!("tell application \"{name}\" to quit")])
        .output();
    let mut waited = 0.0;
    while running_app().is_some() && waited < 30.0 {
        sleep(0.5);
        waited += 0.5;
    }
    if running_app().is_some() {
        return Err(runtime(
            "MoneyWiz did not quit. Quit it yourself, then run the command again.",
        ));
    }
    Ok(())
}

/// `[moneywiz]` of the user's cashcast.toml.
pub fn settings(user: &UserHome) -> Result<toml::Table, AppError> {
    let path = user.home.join("cashcast.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(toml::Table::new());
    };
    let v: toml::Table =
        toml::from_str(&text).map_err(|e| AppError::new("tomldecodeerror", e.to_string()))?;
    Ok(v.get("moneywiz")
        .and_then(|m| m.as_table())
        .cloned()
        .unwrap_or_default())
}

fn setting<'a>(cfg: &'a toml::Table, key: &str) -> Option<&'a str> {
    cfg.get(key).and_then(|v| v.as_str())
}

fn say(user: &UserHome, text: &str, today: NaiveDate) -> Result<(), AppError> {
    gate::emit(user, text, "agent:aggregates", "moneywiz", today).map(|_| ())
}

/// The number in "Sent N links to MoneyWiz.".
pub fn sent(report: &str) -> i64 {
    report
        .lines()
        .find(|l| l.starts_with("Sent ") && l.ends_with(" links to MoneyWiz."))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn changed(report: &str) -> i64 {
    report
        .lines()
        .find(|l| l.starts_with("Verified "))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// `moneywiz sync` that hands the missing rows to MoneyWiz. `notes` are the
/// download lines printed before the report.
pub fn sync_apply(
    user: &UserHome,
    engine: Engine,
    extra: &[&str],
    resend: bool,
    notes: &[String],
    today: NaiveDate,
) -> Result<i32, AppError> {
    consent::require(user, "local:moneywiz", today)?;
    ensure_open()?;
    let mut args = vec!["moneywiz", "sync", "--apply"];
    args.extend(extra);
    if resend {
        args.push("--resend");
    }
    let (code, out) = engine(&args)?;
    let text: String = notes.iter().map(|n| format!("{n}\n")).collect::<String>() + &out;
    say(user, &text, today)?;
    events::log(
        user,
        "moneywiz_sync",
        &[
            (
                "result",
                Json::Str(if code == 0 { "ok" } else { "failed" }.into()),
            ),
            ("sent", Json::Int(sent(&out))),
        ],
    );
    if code == 0 && setting(&settings(user)?, "fixes") == Some("edit") {
        return edit(user, engine, false, true, None, today);
    }
    Ok(code)
}

/// `moneywiz edit`: resolve the edits, then (not a dry run) write them.
pub fn edit(
    user: &UserHome,
    engine: Engine,
    dry_run: bool,
    fixes: bool,
    archive: Option<(&str, &str)>,
    today: NaiveDate,
) -> Result<i32, AppError> {
    let mut args = vec!["moneywiz", "edits"];
    if fixes {
        args.push("--fixes");
    }
    // Archive an account, or show it again (MoneyWiz drops links to an
    // archived account).
    if let Some((account, archived)) = archive {
        args.extend(["--account", account, "--archived", archived]);
    }
    let (code, out) = engine(&args)?;
    say(user, &out, today)?;
    if code != 0 || dry_run || out.contains("Nothing to change.") {
        return Ok(code);
    }
    Ok(write_plan(user, engine, "moneywiz_edit", &[], today)?.0)
}

/// `moneywiz categories STEP` (decision 0012).
#[allow(clippy::too_many_arguments)]
pub fn categories(
    user: &UserHome,
    engine: Engine,
    step: &str,
    dry_run: bool,
    name: Option<&str>,
    ops: Option<&str>,
    yes: bool,
    today: NaiveDate,
) -> Result<i32, AppError> {
    if matches!(step, "list" | "suggest" | "new") {
        let mut args = vec!["moneywiz", "categories", step];
        let day = today.to_string();
        if step == "suggest" {
            // The suggestions file names its day, and the migrate record hashes that file.
            args.extend(["--today", day.as_str()]);
        }
        if step == "new"
            && let Some(n) = name.filter(|n| !n.is_empty())
        {
            args.extend(["--name", n]);
        }
        let (code, out) = if step == "new"
            && let Some(ops) = ops.filter(|o| !o.is_empty())
        {
            std::fs::create_dir_all(user.data()).map_err(os_err)?;
            let file = user
                .data()
                .join(format!("tmp-ops-{}.json", std::process::id()));
            std::fs::write(&file, ops).map_err(os_err)?;
            let f = file.to_string_lossy().to_string();
            args.extend(["--ops", f.as_str()]);
            let r = engine(&args);
            let _ = std::fs::remove_file(&file);
            r?
        } else {
            engine(&args)?
        };
        say(user, &out, today)?;
        return Ok(code);
    }
    let rollback = step == "rollback";
    loop {
        let mut args = vec!["moneywiz", "categories", "plan"];
        if rollback {
            args.push("--rollback");
        }
        let (code, out) = engine(&args)?;
        say(user, &out, today)?;
        if code != 0 || dry_run || out.starts_with("No ") {
            return Ok(code);
        }
        if !yes {
            say(
                user,
                &format!(
                    "Nothing changed. To change MoneyWiz (a backup first), run it again with --yes: moneywiz categories {step} --yes"
                ),
                today,
            )?;
            return Ok(1);
        }
        let migration = out
            .lines()
            .next()
            .and_then(|l| l.rsplit(' ').next())
            .unwrap_or("")
            .to_string();
        let fields = [
            ("migration", Json::Str(migration)),
            ("rollback", Json::Bool(rollback)),
        ];
        let (code, saved) = write_plan(user, engine, "moneywiz_migrate", &fields, today)?;
        let Some(saved) = saved.filter(|_| code == 0) else {
            return Ok(if code == 0 { 1 } else { code });
        };
        let (code, out) = engine(&["moneywiz", "categories", "commit", "--backup", &saved])?;
        say(user, &out, today)?;
        if code != 0 || rollback {
            return Ok(code);
        }
    }
}

/// The writer's folder: the one in the engine's own checkout, so a plan never
/// reaches an older writer (an engine run from the sync worktree with
/// `CASHCAST_ROOT` set used the data root's writer). Falls back to the root.
pub fn writer_dir(exe: Option<&Path>, root: &Path) -> PathBuf {
    // <checkout>/engine/target/<profile>/cashcast-engine
    let own = exe
        .and_then(|e| e.ancestors().nth(4))
        .map(|c| c.join("engine").join("moneywiz-writer"))
        .filter(|d| d.join("MoneyWizWriter.swift").is_file());
    own.unwrap_or_else(|| root.join("engine").join("moneywiz-writer"))
}

fn writer() -> Result<Vec<String>, AppError> {
    if let Ok(fake) = std::env::var("CASHCAST_MONEYWIZ_WRITER")
        && !fake.is_empty()
    {
        return Ok(vec![fake]);
    }
    if !cfg!(target_os = "macos") {
        return Err(runtime(
            "The MoneyWiz writer runs on the Mac where MoneyWiz is installed.",
        ));
    }
    let dir = writer_dir(std::env::current_exe().ok().as_deref(), &paths::root());
    let src = dir.join("MoneyWizWriter.swift");
    let bin = dir.join("build").join("moneywiz-writer");
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if !bin.exists() || mtime(&bin) < mtime(&src) {
        std::fs::create_dir_all(dir.join("build")).map_err(os_err)?;
        let o = Command::new("swiftc")
            .args(["-O", "-o"])
            .arg(&bin)
            .arg(&src)
            .output()
            .map_err(os_err)?;
        if !o.status.success() {
            let err = String::from_utf8_lossy(&o.stderr);
            let tail: String = err
                .chars()
                .rev()
                .take(2000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return Err(runtime(format!(
                "Building the MoneyWiz writer failed. It needs the Xcode Command Line Tools: xcode-select --install\n{tail}"
            )));
        }
    }
    Ok(vec![bin.to_string_lossy().to_string()])
}

fn run(cmd: &[String]) -> Result<String, AppError> {
    let o = Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .map_err(os_err)?;
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        let msg = if err.is_empty() {
            out.trim().to_string()
        } else {
            err
        };
        let tail: String = msg
            .chars()
            .rev()
            .take(1000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(runtime(format!(
            "The MoneyWiz writer stopped and changed nothing: {tail}"
        )));
    }
    Ok(out)
}

fn store_files(db: &Path) -> Vec<PathBuf> {
    let s = db.to_string_lossy();
    [
        db.to_path_buf(),
        PathBuf::from(format!("{s}-wal")),
        PathBuf::from(format!("{s}-shm")),
    ]
    .into_iter()
    .filter(|f| f.exists())
    .collect()
}

fn copy_store(db: &Path, folder: &Path) -> Result<PathBuf, AppError> {
    for f in store_files(db) {
        let name = f.file_name().unwrap_or_default();
        std::fs::copy(&f, folder.join(name)).map_err(os_err)?;
    }
    Ok(folder.join(db.file_name().unwrap_or_default()))
}

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).map_err(os_err)
}

/// Copies the database into data/moneywiz-backups/<time>/; keeps the newest few.
fn backup(user: &UserHome, db: &Path) -> Result<PathBuf, AppError> {
    let root = user.data().join("moneywiz-backups");
    let folder = root.join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    std::fs::create_dir_all(&folder).map_err(os_err)?;
    chmod(&root, 0o700)?;
    chmod(&folder, 0o700)?;
    copy_store(db, &folder)?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(&root)
        .map_err(os_err)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    old.sort();
    let n = old.len().saturating_sub(KEEP_BACKUPS);
    for p in &old[..n] {
        std::fs::remove_dir_all(p).map_err(os_err)?;
    }
    Ok(folder)
}

/// A folder under `data/` removed when dropped.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Applies data/moneywiz-plan.json: on a copy, then live. Returns the exit
/// code and the backup folder (relative to the repo) after a live write.
fn write_plan(
    user: &UserHome,
    engine: Engine,
    event: &str,
    fields: &[(&str, Json)],
    today: NaiveDate,
) -> Result<(i32, Option<String>), AppError> {
    let log = |result: &str, changed: i64| {
        let mut f = vec![
            ("result", Json::Str(result.into())),
            ("changed", Json::Int(changed)),
        ];
        f.extend(fields.iter().cloned());
        events::log(user, event, &f);
    };
    consent::require(user, "local:moneywiz", today)?;
    let cfg = settings(user)?;
    if setting(&cfg, "writer") != Some("core-data") {
        return Err(runtime(
            "In-place MoneyWiz edits are off. To turn them on, add writer = \"core-data\" under [moneywiz] in cashcast.toml (see connectors/playbooks/moneywiz.md).",
        ));
    }
    let model_dir = model_dir()?;
    let (code, db_out) = engine(&["moneywiz", "db"])?;
    if code != 0 {
        say(user, &db_out, today)?;
        return Ok((code, None));
    }
    let db = PathBuf::from(db_out.trim());
    let cmd = writer()?;
    let model = model_dir.to_string_lossy().to_string();
    let plan = user
        .data()
        .join("moneywiz-plan.json")
        .to_string_lossy()
        .to_string();
    let db_s = db.to_string_lossy().to_string();
    let with = |rest: &[&str]| -> Vec<String> {
        cmd.iter()
            .cloned()
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    };
    let sums = pyjson::loads(&run(&with(&[
        "checksum", "--store", &db_s, "--model", &model,
    ]))?)
    .unwrap_or(Json::Null);
    let checksum = sums.get("store").map_or(String::new(), Json::py_str);
    if checksum.is_empty() || sums.get("model").map(Json::py_str) != Some(checksum.clone()) {
        return Err(runtime(
            "MoneyWiz's app and database are different versions. Open MoneyWiz once, quit it, and try again.",
        ));
    }

    // 1. A test on a copy: the live database is not touched.
    std::fs::create_dir_all(user.data()).map_err(os_err)?;
    let (code, report) = {
        let tmp = TempDir(user.data().join(format!("tmp-copy-{}", std::process::id())));
        std::fs::create_dir_all(&tmp.0).map_err(os_err)?;
        chmod(&tmp.0, 0o700)?;
        let copy = copy_store(&db, &tmp.0)?.to_string_lossy().to_string();
        run(&with(&[
            "apply",
            "--store",
            &copy,
            "--model",
            &model,
            "--plan",
            &plan,
            "--checksum",
            &checksum,
            "--allow-running",
        ]))?;
        engine(&["moneywiz", "verify-edits", "--db", &copy])?
    };
    say(user, &format!("Test on a copy: {report}"), today)?;
    if code != 0 {
        log("failed", 0);
        return Ok((code, None));
    }
    if setting(&cfg, "writer_checksum") != Some(checksum.as_str()) {
        say(
            user,
            &format!(
                "The test on a copy passed. Live writes are not allowed yet for this MoneyWiz model.\nRead connectors/playbooks/moneywiz.md (edit-in-place), then add under [moneywiz] in cashcast.toml:\n  writer_checksum = \"{checksum}\"\nand run the command again. A MoneyWiz update changes the checksum and asks again."
            ),
            today,
        )?;
        log("needs_checksum", 0);
        return Ok((1, None));
    }

    // 2. The live database: MoneyWiz closed, a backup first, a check after.
    quit_app(running_app())?;
    let saved = backup(user, &db)?;
    let live = run(&with(&[
        "apply",
        "--store",
        &db_s,
        "--model",
        &model,
        "--plan",
        &plan,
        "--checksum",
        &checksum,
    ]))
    .and_then(|_| engine(&["moneywiz", "verify-edits"]));
    ensure_open()?; // MoneyWiz syncs the change when it opens
    let (code, report) = live?;
    let root = paths::root();
    let rel = saved
        .strip_prefix(&root)
        .map_or(saved.clone(), Path::to_path_buf)
        .to_string_lossy()
        .to_string();
    say(
        user,
        &format!(
            "{report}Backup of the database before the change: {rel}\nCheck that MoneyWiz sync shows Up to Date."
        ),
        today,
    )?;
    log(if code == 0 { "ok" } else { "failed" }, changed(&report));
    Ok((code, Some(rel)))
}

// --- The probe -------------------------------------------------------------

#[derive(Default)]
pub struct Probe {
    pub app: Option<PathBuf>,
    pub kind: String,
    pub version: String,
    pub schemes: Vec<String>,
    pub script_terms: Vec<String>,
    pub has_dictionary: bool,
    pub intents: Vec<String>,
    pub model: String,
    pub ui: Option<Ui>,
    pub ui_error: String,
    pub notes: Vec<String>,
}

pub struct Ui {
    pub windows: i64,
    pub menu: Vec<(&'static str, bool)>,
    pub roles: Vec<(String, i64)>,
    pub ids: i64,
}

fn word(text: &str, w: &str) -> bool {
    let t = text.to_lowercase();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    t.match_indices(w).any(|(i, _)| {
        let before = t[..i].chars().next_back();
        let after = t[i + w.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn edit_words(t: &str) -> bool {
    let t = t.to_lowercase();
    [
        "transaction",
        "categor",
        "payee",
        "edit",
        "update",
        "delete",
        "remove",
    ]
    .iter()
    .any(|w| t.contains(w))
}

const MENU: [&str; 7] = [
    "new transaction",
    "delete",
    "duplicate",
    "categories",
    "rules",
    "find",
    "undo",
];

fn menu_match(key: &str, item: &str) -> bool {
    let l = item.to_lowercase();
    match key {
        "new transaction" => ["new transaction", "new expense", "new withdrawal"]
            .iter()
            .any(|p| l.starts_with(p)),
        "delete" => word(item, "delete"),
        "duplicate" => word(item, "duplicate"),
        "categories" => l.contains("categor"),
        "rules" => word(item, "rule") || word(item, "rules"),
        "find" => l.starts_with("find") || l.starts_with("search"),
        _ => l.starts_with("undo"),
    }
}

impl Probe {
    pub fn applescript(&self) -> bool {
        self.script_terms.iter().any(|t| edit_words(t))
    }
    pub fn shortcuts(&self) -> bool {
        self.intents.iter().any(|i| {
            let l = i.to_lowercase();
            edit_words(i) && !["add", "create", "new"].iter().any(|p| l.starts_with(p))
        })
    }
    pub fn core_data_verified(&self) -> bool {
        self.model == VERIFIED_MODEL
    }
    pub fn window(&self) -> bool {
        self.ui
            .as_ref()
            .is_some_and(|u| u.windows != 0 && u.menu.iter().any(|(k, v)| *k == "delete" && *v))
    }
}

fn plist_dict(path: &Path) -> plist::Dictionary {
    plist::Value::from_file(path)
        .ok()
        .and_then(|v| v.into_dictionary())
        .unwrap_or_default()
}

fn pstr(d: &plist::Dictionary, key: &str) -> Option<String> {
    d.get(key).map(|v| match v {
        plist::Value::String(s) => s.clone(),
        plist::Value::Integer(i) => i.to_string(),
        plist::Value::Real(r) => r.to_string(),
        plist::Value::Boolean(b) => if *b { "True" } else { "False" }.into(),
        _ => String::new(),
    })
}

pub fn find_app() -> Option<PathBuf> {
    if let Ok(o) = std::env::var("CASHCAST_MONEYWIZ_APP")
        && !o.is_empty()
    {
        return Some(PathBuf::from(o));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    for folder in [
        PathBuf::from("/Applications"),
        Path::new(&home).join("Applications"),
    ] {
        for name in APP_NAMES {
            let p = folder.join(format!("{name}.app"));
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

/// (bundle root, resources folder, kind) for a Mac app or an iPad app on a Mac.
fn bundle(app: &Path) -> (PathBuf, PathBuf, &'static str) {
    let wrapped = app.join("WrappedBundle");
    if wrapped.exists() {
        let inner = std::fs::canonicalize(&wrapped).unwrap_or(wrapped);
        return (inner.clone(), inner, "iPad app");
    }
    let info = plist_dict(&app.join("Contents").join("Info.plist"));
    let kind =
        if info.contains_key("UIDeviceFamily") || info.contains_key("UIApplicationSceneManifest") {
            "Mac Catalyst"
        } else {
            "Mac (AppKit)"
        };
    (
        app.join("Contents"),
        app.join("Contents").join("Resources"),
        kind,
    )
}

fn sorted_children(dir: &Path, pred: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| pred(p))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn ext_is(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e == ext)
}

/// Every `*.momd` under `root`, sorted like `Path.rglob`.
fn momds(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for p in sorted_children(&d, |p| p.is_dir()) {
            if ext_is(&p, "momd") {
                out.push(p.clone());
            }
            if !p.is_symlink() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

fn script_terms(resources: &Path, info: &plist::Dictionary) -> (bool, Vec<String>) {
    let sdefs = match pstr(info, "OSAScriptingDefinition").filter(|s| !s.is_empty()) {
        Some(name) => vec![resources.join(name)],
        None => sorted_children(resources, |p| ext_is(p, "sdef")),
    };
    let mut terms = std::collections::BTreeSet::new();
    let mut found = false;
    for sdef in sdefs {
        let Ok(text) = std::fs::read_to_string(&sdef) else {
            continue;
        };
        let mut reader = quick_xml::Reader::from_str(&text);
        let mut names = Vec::new();
        let mut ok = true;
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)) => {
                    let tag = e.name().as_ref().to_string();
                    if tag == "command" || tag == "class" {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "name"
                                && let Ok(v) =
                                    a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                && !v.is_empty()
                            {
                                names.push(v.to_string());
                            }
                        }
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => {
                    ok = false;
                    break;
                }
                _ => {}
            }
        }
        if ok {
            found = true;
            terms.extend(names);
        }
    }
    (found, terms.into_iter().collect())
}

fn intents(root: &Path, resources: &Path) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for data in sorted_children(&resources.join("Metadata.appintents"), |p| {
        ext_is(p, "actionsdata")
    }) {
        let Some(Json::Obj(top)) = std::fs::read_to_string(&data)
            .ok()
            .and_then(|t| pyjson::loads(&t))
        else {
            continue;
        };
        if let Some((_, Json::Obj(actions))) = top.iter().find(|(k, _)| k == "actions") {
            names.extend(actions.iter().map(|(k, _)| k.clone()));
        }
    }
    for ext in sorted_children(&root.join("PlugIns"), |p| ext_is(p, "appex")) {
        let mut info = plist_dict(&ext.join("Contents").join("Info.plist"));
        if info.is_empty() {
            info = plist_dict(&ext.join("Info.plist"));
        }
        let Some(ns) = info.get("NSExtension").and_then(|v| v.as_dictionary()) else {
            continue;
        };
        if pstr(ns, "NSExtensionPointIdentifier").as_deref() == Some("com.apple.intents-service")
            && let Some(list) = ns
                .get("NSExtensionAttributes")
                .and_then(|v| v.as_dictionary())
                .and_then(|a| a.get("IntentsSupported"))
                .and_then(|v| v.as_array())
        {
            names.extend(list.iter().filter_map(|v| v.as_string().map(String::from)));
        }
    }
    names.into_iter().collect()
}

fn model(root: &Path) -> String {
    for m in momds(root) {
        if let Some(c) = pstr(
            &plist_dict(&m.join("VersionInfo.plist")),
            "NSManagedObjectModel_CurrentVersionName",
        )
        .filter(|c| !c.is_empty())
        {
            return c;
        }
    }
    String::new()
}

/// The installed MoneyWiz Core Data model (`*.momd`), for the writer.
pub fn model_dir() -> Result<PathBuf, AppError> {
    if let Ok(o) = std::env::var("CASHCAST_MONEYWIZ_MODEL")
        && !o.is_empty()
    {
        return Ok(PathBuf::from(o));
    }
    let app =
        find_app().ok_or_else(|| runtime("MoneyWiz is not in /Applications or ~/Applications."))?;
    let (root, _, _) = bundle(&app);
    momds(&root)
        .into_iter()
        .find(|m| m.join("VersionInfo.plist").exists())
        .ok_or_else(|| {
            runtime(format!(
                "{} has no Core Data model this writer can load.",
                app.file_name().unwrap_or_default().to_string_lossy()
            ))
        })
}

const UI_SCRIPT: &str = r#"
function run(argv) {
  const p = Application('System Events').processes.byName(argv[0]);
  const items = [];
  p.menuBars[0].menuBarItems().forEach(function (m) {
    if (['Window', 'Help'].indexOf(m.name()) >= 0) return;
    try { m.menus[0].menuItems().forEach(function (i) { items.push(i.name() || ''); }); } catch (e) {}
  });
  const roles = {};
  let ids = 0;
  if (p.windows.length > 0) {
    p.windows[0].entireContents().forEach(function (e) {
      const r = e.role();
      roles[r] = (roles[r] || 0) + 1;
      try { if (e.attributes.byName('AXIdentifier').value()) ids++; } catch (x) {}
    });
  }
  return JSON.stringify({windows: p.windows.length, items: items, roles: roles, ids: ids});
}
"#;

fn role_ok(k: &str) -> bool {
    k.len() >= 3
        && k.len() <= 42
        && k.starts_with("AX")
        && k[2..].chars().all(|c| c.is_ascii_alphabetic())
}

/// Roles and counts of the front MoneyWiz window, and which menu commands exist.
fn probe_ui(name: &str) -> (Option<Ui>, String) {
    let Ok(o) = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", UI_SCRIPT, name])
        .output()
    else {
        return (None, "osascript failed (FileNotFoundError)".into());
    };
    if !o.status.success() {
        return (
            None,
            "macOS refused to show the MoneyWiz window. Give your terminal app Accessibility access in System Settings > Privacy & Security > Accessibility, then run the probe again.".into(),
        );
    }
    let Some(raw) = pyjson::loads(&String::from_utf8_lossy(&o.stdout)) else {
        return (None, "osascript returned something unexpected".into());
    };
    let int = |v: Option<&Json>| {
        v.and_then(|v| crate::strategy::py::num(Some(v)))
            .map_or(0, |n| n as i64)
    };
    let items: Vec<String> = crate::strategy::py::list(&raw, "items")
        .iter()
        .map(Json::py_str)
        .collect();
    let menu = MENU
        .iter()
        .map(|k| (*k, items.iter().any(|i| menu_match(k, i))))
        .collect();
    let roles = match raw.get("roles") {
        Some(Json::Obj(r)) => r
            .iter()
            .filter(|(k, _)| role_ok(k))
            .map(|(k, v)| (k.clone(), int(Some(v))))
            .collect(),
        _ => Vec::new(),
    };
    (
        Some(Ui {
            windows: int(raw.get("windows")),
            menu,
            roles,
            ids: int(raw.get("ids")),
        }),
        String::new(),
    )
}

pub fn probe(ui: bool) -> Probe {
    let mut result = Probe {
        app: find_app().filter(|a| a.is_dir()),
        ..Default::default()
    };
    let Some(app) = result.app.clone() else {
        result
            .notes
            .push("MoneyWiz is not in /Applications or ~/Applications.".into());
        return result;
    };
    let (root, resources, kind) = bundle(&app);
    result.kind = kind.into();
    let info = plist_dict(&root.join("Info.plist"));
    result.version = pstr(&info, "CFBundleShortVersionString").unwrap_or_default();
    let mut schemes = std::collections::BTreeSet::new();
    if let Some(types) = info.get("CFBundleURLTypes").and_then(|v| v.as_array()) {
        for t in types {
            if let Some(list) = t
                .as_dictionary()
                .and_then(|d| d.get("CFBundleURLSchemes"))
                .and_then(|v| v.as_array())
            {
                schemes.extend(list.iter().filter_map(|s| s.as_string().map(String::from)));
            }
        }
    }
    result.schemes = schemes.into_iter().collect();
    (result.has_dictionary, result.script_terms) = script_terms(&resources, &info);
    result.intents = intents(&root, &resources);
    result.model = model(&root);
    if ui {
        let name = running_app().map(String::from).or_else(|| {
            std::env::var("CASHCAST_MONEYWIZ_PROCESS")
                .ok()
                .filter(|s| !s.is_empty())
        });
        match name {
            Some(n) => (result.ui, result.ui_error) = probe_ui(&n),
            None => {
                result.ui_error =
                    "MoneyWiz is not running. Open it, then run `moneywiz probe --ui` again.".into()
            }
        }
    }
    result
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn listed(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!(
            " ({})",
            items
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub fn render(p: &Probe) -> String {
    let Some(app) = &p.app else {
        return format!("MoneyWiz probe\n\n{}\n", p.notes.join("\n"));
    };
    let newer = if p.core_data_verified() && p.version != VERIFIED_APP {
        "; newer app: the copy test checks it"
    } else {
        ""
    };
    let mut lines = vec![
        "MoneyWiz probe (reads the app bundle only; no ledger data)".to_string(),
        String::new(),
        format!(
            "App: {}, version {}, built as {}",
            app.file_name().unwrap_or_default().to_string_lossy(),
            if p.version.is_empty() {
                "unknown"
            } else {
                &p.version
            },
            p.kind
        ),
        format!(
            "URL schemes: {}",
            if p.schemes.is_empty() {
                "none".into()
            } else {
                p.schemes.join(", ")
            }
        ),
        String::new(),
        "| Way in | Found | Can edit rows or categories |".into(),
        "|---|---|---|".into(),
        format!(
            "| 1. AppleScript dictionary | {}{} | {} |",
            yes(p.has_dictionary),
            listed(&p.script_terms),
            yes(p.applescript())
        ),
        format!(
            "| 2. Shortcuts actions | {}{} | {} |",
            yes(!p.intents.is_empty()),
            listed(&p.intents),
            yes(p.shortcuts())
        ),
        format!(
            "| 3. Core Data model | {} (moneywiz-tools verified {VERIFIED_MODEL} in MoneyWiz {VERIFIED_APP}) | {}, with MoneyWiz closed{newer} |",
            if p.model.is_empty() {
                "not found"
            } else {
                &p.model
            },
            yes(p.core_data_verified())
        ),
    ];
    match &p.ui {
        Some(ui) => {
            let menu: Vec<String> = ui
                .menu
                .iter()
                .map(|(k, v)| format!("{k} {}", yes(*v)))
                .collect();
            let mut roles = ui.roles.clone();
            roles.sort_by_key(|(_, n)| -n); // stable, like Python's sorted
            let top: Vec<String> = roles
                .iter()
                .take(8)
                .map(|(r, n)| format!("{r} {n}"))
                .collect();
            lines.push(format!(
                "| 4. The MoneyWiz window | {} window(s); {}; {} elements with an accessibility id; menu: {} | {} |",
                ui.windows,
                if top.is_empty() { "no elements".into() } else { top.join(", ") },
                ui.ids,
                menu.join(", "),
                yes(p.window())
            ));
        }
        None => lines.push(format!(
            "| 4. The MoneyWiz window | not checked | {} |",
            if p.ui_error.is_empty() {
                "run `moneywiz probe --ui`"
            } else {
                &p.ui_error
            }
        )),
    }
    lines.push(String::new());
    lines.push(format!("Best way in: {}", best(p)));
    lines.join("\n") + "\n"
}

fn best(p: &Probe) -> &'static str {
    if p.applescript() {
        "AppleScript: MoneyWiz scripts its own edits, with the app open and its sync intact."
    } else if p.shortcuts() {
        "Shortcuts: MoneyWiz's own edit actions, run with `shortcuts run`."
    } else if p.core_data_verified() {
        "the Core Data writer (`bin/cashcast moneywiz edit`, after marcomc/moneywiz-tools). It saves like MoneyWiz does, with MoneyWiz closed."
    } else if p.window() {
        "the MoneyWiz window, driven through Accessibility. Slower, but every edit goes through MoneyWiz itself."
    } else if p.ui.is_none() {
        "none found in the bundle. Run `bin/cashcast moneywiz probe --ui` with MoneyWiz open to check the window."
    } else {
        "none found. Edits stay in out/moneywiz-fixes.md for you."
    }
}

/// `moneywiz probe`: prints the report and logs what it found.
pub fn probe_command(user: &UserHome, ui: bool) -> i32 {
    if !cfg!(target_os = "macos")
        && std::env::var("CASHCAST_MONEYWIZ_APP").map_or(true, |v| v.is_empty())
    {
        print!("The MoneyWiz probe runs on the Mac where MoneyWiz is installed.\n");
        events::log(
            user,
            "moneywiz_probe",
            &[
                ("applescript", Json::Bool(false)),
                ("shortcuts", Json::Bool(false)),
                ("core_data", Json::Bool(false)),
                ("window", Json::Bool(false)),
            ],
        );
        return 1;
    }
    let p = probe(ui);
    print!("{}", render(&p));
    events::log(
        user,
        "moneywiz_probe",
        &[
            ("applescript", Json::Bool(p.applescript())),
            ("shortcuts", Json::Bool(p.shortcuts())),
            ("core_data", Json::Bool(p.core_data_verified())),
            ("window", Json::Bool(p.window())),
        ],
    );
    if p.app.is_some() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_words_match_whole_words() {
        assert!(menu_match("delete", "Delete Transaction"));
        assert!(!menu_match("delete", "Undeleted"));
        assert!(menu_match("rules", "Edit Rules…"));
        assert!(menu_match("new transaction", "New Expense"));
        assert!(role_ok("AXButton") && !role_ok("AX") && !role_ok("button"));
    }

    #[test]
    fn report_counts() {
        assert_eq!(sent("x\nSent 3 links to MoneyWiz.\n"), 3);
        assert_eq!(changed("Verified 2 of 2 changes."), 2);
    }
}

#[cfg(test)]
mod writer_tests {
    use super::writer_dir;

    #[test]
    fn writer_comes_from_the_engines_own_checkout() {
        let base = std::env::temp_dir().join(format!("cc-writer-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (sync, root) = (base.join("sync"), base.join("root"));
        let own = sync.join("engine").join("moneywiz-writer");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("MoneyWizWriter.swift"), "// canary").unwrap();
        std::fs::create_dir_all(sync.join("engine/target/release")).unwrap();
        let exe = sync.join("engine/target/release/cashcast-engine");
        assert_eq!(writer_dir(Some(&exe), &root), own);
        // An engine outside a checkout (an installed binary) uses the root's.
        let lone = base.join("bin/cashcast-engine");
        assert_eq!(writer_dir(Some(&lone), &root), root.join("engine/moneywiz-writer"));
        assert_eq!(writer_dir(None, &root), root.join("engine/moneywiz-writer"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
