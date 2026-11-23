//! The citizenship judge's verbs (ported from sidequest
//! `scripts/citizenship-judge.ts`):
//!   answers [file] [--json]        ready? exit 0 only when nothing to fix or answer
//!   questions [file] [--json]      the questions still missing (all, with no draft)
//!   screen <screen.json> [--answers file] [--json]   a typed form screen vs the answers
//!   template [--out file]          a blank TOML answers file (--out: mode 600)
//!   put <file.toml>                the owner's answers from a scratch file (then deleted)
//!   show [file] [--reveal]         the answers, ID numbers hidden
//!   collect <kind> <file...>       parse one source and merge it in
//!   collect-all <dir>              every source file in a folder
//!   status [--json]                each question: done, fix, open, confirm
//!   police                         countries that need a certificate
//!   versions | restore <name>
//!   proof <question> <image> --source "<where>" [--url u] [--note n]
//!   report                         the evidence page (mode 600), path printed
//! The draft is `<user dir>/paperwork/citizenship/answers.sqv`, sealed. The judges never
//! print names, ID numbers or contact details: only dates, day counts and countries.
//! Errors (usage included) come back as `Err`, exit 2.

use super::answers::{Answers, judge_answers, judge_screen, police_countries, questions};
use super::collect::{self as c, Partial};
use super::draft::{
    self, add_proof, answers_file, citizenship_dir, coverage, keep_version, list_versions, masked,
    merge, open_scratch, read_answers, read_proof, sealed, status, versions_dir, write_answers,
};
use super::evidence::{EvidenceInput, evidence_page};
use chrono::NaiveDate;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// How `collect nomads` starts its refusal of a file with no readable stay.
const NO_STAYS: &str = "collect nomads: no stay could be read";
pub const KINDS: &[&str] = &[
    "pr-card", "passport", "id", "marital", "language", "leases", "linkedin", "nomads", "cra",
];
const EVIDENCE_TTL_ENV: &str = "CASHCAST_EVIDENCE_TTL_SECS";

struct Args {
    pos: Vec<String>,
    json: bool,
    reveal: bool,
    today: String,
    answers: Option<String>,
    source: Option<String>,
    url: Option<String>,
    note: Option<String>,
    out: Option<String>,
}

fn parse(args: &[String], today: NaiveDate) -> Result<Args, String> {
    let mut a = Args {
        pos: vec![],
        json: false,
        reveal: false,
        today: today.format("%Y-%m-%d").to_string(),
        answers: None,
        source: None,
        url: None,
        note: None,
        out: None,
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{x} needs a value"));
        match x.as_str() {
            "--json" => a.json = true,
            "--reveal" => a.reveal = true,
            "--today" => a.today = val()?,
            "--answers" => a.answers = Some(val()?),
            "--source" => a.source = Some(val()?),
            "--url" => a.url = Some(val()?),
            "--note" => a.note = Some(val()?),
            "--out" => a.out = Some(val()?),
            _ => a.pos.push(x.clone()),
        }
    }
    Ok(a)
}

fn ttl() -> u64 {
    std::env::var(EVIDENCE_TTL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600)
}
fn mode_open(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o077 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        false
    }
}
fn chmod600(p: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot protect {}: {e}", p.display()))?;
    }
    Ok(())
}
fn load(file: &Path) -> Result<(Answers, bool), String> {
    if !file.exists() {
        return Err(format!(
            "no answers file at {}: run `template --out <file>`, fill it, then `put <file>`",
            file.display()
        ));
    }
    Ok((read_answers(file)?, mode_open(file)))
}
fn read(p: &str) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))
}
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn run(args: &[String], user_dir: &Path, today: NaiveDate) -> Result<(String, i32), String> {
    let a = parse(args, today)?;
    let today = a.today.as_str();
    let dir = citizenship_dir(user_dir);
    let mut out = String::new();
    // The evidence page lives at most its time to live: any run removes an older one.
    let page = dir.join("evidence.html");
    if let Ok(m) = std::fs::metadata(&page)
        && m.modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|e| e.as_secs() > ttl())
    {
        let _ = std::fs::remove_file(&page);
    }
    let verb = a.pos.first().map_or("answers", String::as_str);
    let arg = a.pos.get(1).map(String::as_str);
    let default = answers_file(user_dir);
    let file_of = || a.answers.as_ref().map_or(default.clone(), PathBuf::from);
    // A verb that writes the draft writes it sealed: a plain --answers file is refused.
    let write_file = || -> Result<PathBuf, String> {
        let f = file_of();
        if sealed(&f) {
            Ok(f)
        } else {
            Err(format!(
                "{verb}: --answers must name a sealed .sqv file; it never writes plain text"
            ))
        }
    };

    match verb {
        "answers" => {
            let file = arg.map_or(default.clone(), PathBuf::from);
            let (ans, open) = load(&file)?;
            let mut f = judge_answers(&ans, today);
            let fail = |id: &str, msg: String| super::answers::Finding { id: id.into(), level: "FAIL", msg };
            if open {
                f.insert(0, fail("file", format!("others can read {}: run chmod 600 on it", file.display())));
            }
            for p in open_scratch(&[std::env::temp_dir()]) {
                f.insert(
                    0,
                    fail(
                        "scratch",
                        format!(
                            "others can read {}: run `put` on it or delete it (make the next one with `template --out <file>`)",
                            p.display()
                        ),
                    ),
                );
            }
            let fails = f.iter().filter(|x| x.level == "FAIL").count();
            let asks = f.iter().filter(|x| x.level == "ASK").count();
            if a.json {
                out = serde_json::to_string_pretty(&f).unwrap();
            } else {
                for x in &f {
                    writeln!(out, "{:<4} {}: {}", x.level, x.id, x.msg).unwrap();
                }
                write!(
                    out,
                    "\nready: {} ({fails} to fix, {asks} to answer)",
                    if fails + asks > 0 { "no" } else { "yes" }
                )
                .unwrap();
            }
            Ok((out, i32::from(fails + asks > 0)))
        }
        "questions" => {
            let file = arg.map_or(default.clone(), PathBuf::from);
            let missing: Vec<(String, String)> = if arg.is_some() || file.exists() {
                judge_answers(&load(&file)?.0, today)
                    .into_iter()
                    .filter(|x| x.level == "ASK")
                    .map(|x| (x.id, x.msg))
                    .collect()
            } else {
                questions().into_iter().map(|(k, q)| (k.to_string(), q)).collect()
            };
            if a.json {
                let v: Vec<Value> = missing.iter().map(|(id, msg)| serde_json::json!({"id": id, "msg": msg})).collect();
                out = serde_json::to_string_pretty(&v).unwrap();
            } else {
                writeln!(out, "{} for you:", plural(missing.len(), "question", "questions")).unwrap();
                for (i, (id, msg)) in missing.iter().enumerate() {
                    writeln!(out, "{:>2}. [{id}] {msg}", i + 1).unwrap();
                }
            }
            Ok((out.trim_end().to_string(), 0))
        }
        "screen" => {
            let arg = arg.ok_or("usage: screen <screen.json> [--answers file]")?;
            let s: Value = serde_json::from_str(&read(arg)?).map_err(|e| format!("screen: not JSON: {e}"))?;
            let fields: Vec<(String, String)> = s
                .get("fields")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|f| {
                    let t = |k| match f.get(k) {
                        Some(Value::String(s)) => s.clone(),
                        None | Some(Value::Null) => String::new(),
                        Some(x) => x.to_string(), // a screen file may hold a number
                    };
                    (t("label"), t("value"))
                })
                .collect();
            let r = judge_screen(&fields, &load(&file_of())?.0);
            if a.json {
                out = serde_json::to_string_pretty(&r).unwrap();
            } else {
                for x in &r {
                    writeln!(out, "{:<8} {}: {}", x.level, x.label, x.msg).unwrap();
                }
            }
            let bad = r.iter().any(|x| x.level == "MISMATCH" || x.level == "EMPTY");
            Ok((out.trim_end().to_string(), i32::from(bad)))
        }
        "template" => {
            let mut text = String::from(
                "# Citizenship answers. Fill it, run `put <this file>` (sealed in the vault), then delete it.\n# TOML: dates are \"YYYY-MM-DD\", lists that do not apply are [], tables like name = { family = \"...\", given = \"...\" }.\n",
            );
            for (k, q) in questions() {
                write!(text, "\n# {q}\n# {k} = \n").unwrap();
            }
            let Some(o) = &a.out else { return Ok((text.trim_end().to_string(), 0)) };
            // The scratch file will hold ID numbers: mode 600 from its first byte, never over a file.
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(o).map_err(|e| format!("template: {e}"))?;
            std::io::Write::write_all(&mut f, text.as_bytes()).map_err(|e| format!("template: {e}"))?;
            chmod600(Path::new(o))?;
            Ok((format!("template: wrote {o} (mode 600); fill it, then run `put {o}`"), 0))
        }
        "put" => {
            let arg = arg.ok_or("usage: put <answers.toml> [--answers file]")?;
            let p = PathBuf::from(arg);
            // put deletes its input, so the draft or anything in its folder is refused first.
            let real = std::fs::canonicalize(&p).unwrap_or(p.clone());
            let home = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
            let in_dir = real.starts_with(&home);
            if sealed(&p) || in_dir {
                return Err(format!(
                    "put: {}; give a scratch TOML file outside it",
                    if in_dir { "refused a file in the citizenship folder" } else { "refused a sealed .sqv file" }
                ));
            }
            // The scratch file holds ID numbers in plain text: removed when put ends, also on an error.
            let r = (|| -> Result<String, String> {
                chmod600(&p)?;
                let part = draft::parse_toml(&read(arg)?)?;
                let file = write_file()?;
                let before = read_answers(&file)?;
                let keys: Vec<String> = part.keys().filter(|k| !k.starts_with('_')).cloned().collect();
                let mut next = before.clone();
                let mut src = before.get("_sources").and_then(Value::as_object).cloned().unwrap_or_default();
                for k in &keys {
                    // A section (a table) merges key by key, so a file with one key of
                    // `[taxes]` keeps the others. Lists and plain values are replaced whole.
                    let v = match (before.get(k), &part[k]) {
                        (Some(Value::Object(old)), Value::Object(new)) => {
                            let mut m = old.clone();
                            m.extend(new.clone());
                            Value::Object(m)
                        }
                        (_, new) => new.clone(),
                    };
                    next.insert(k.clone(), v);
                    src.insert(k.clone(), Value::String("owner".into()));
                }
                next.insert("_sources".into(), Value::Object(src));
                let v = keep_version(&file, 100)?;
                write_answers(&file, &next)?;
                Ok(format!(
                    "put: {}{}",
                    if keys.is_empty() { "nothing".into() } else { keys.join(", ") },
                    if v.is_some() { " (old draft kept)" } else { "" }
                ))
            })();
            let _ = std::fs::remove_file(&p);
            r.map(|s| (s, 0)).map_err(|e| format!("put: {e}"))
        }
        "show" => {
            use std::io::IsTerminal;
            // Only --reveal, in the owner's own terminal, after the owner types "reveal".
            let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            if a.reveal && !tty {
                return Err("show --reveal: only in your own terminal; nothing was shown".into());
            }
            let file = arg.map_or(default.clone(), PathBuf::from);
            let ans = load(&file)?.0;
            let mut reveal = false;
            if a.reveal {
                eprint!("ID numbers will show on this screen. Type \"reveal\" to go on: ");
                let mut line = String::new();
                let _ = std::io::stdin().read_line(&mut line);
                reveal = line.trim() == "reveal";
            }
            out = draft::to_toml(&if reveal { ans } else { masked(&ans) });
            if !reveal {
                out.push_str("# ID numbers are hidden: to see them, run `show --reveal` in your own terminal");
            }
            Ok((out.trim_end().to_string(), 0))
        }
        "collect" => {
            let kind = arg.unwrap_or("");
            let files = &a.pos[2.min(a.pos.len())..];
            if !KINDS.contains(&kind) || files.is_empty() {
                return Err(format!("usage: collect <{}> <file...>", KINDS.join("|")));
            }
            let file = write_file()?;
            let cur = read_answers(&file)?;
            let (part, valid) = collect_part(kind, files, &cur, today, &mut out)?;
            apply(&file, kind, &part, valid, today, &mut out)?;
            Ok((out.trim_end().to_string(), 0))
        }
        "collect-all" => {
            let src = arg.ok_or("usage: collect-all <dir>")?;
            let mut all: Vec<String> = std::fs::read_dir(src)
                .map_err(|e| format!("cannot read {src}: {e}"))?
                .flatten()
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            all.sort();
            let file = write_file()?;
            for kind in KINDS {
                let files: Vec<String> = all.iter().filter(|f| matches_kind(kind, f)).cloned().collect();
                if !files.is_empty() {
                    let cur = read_answers(&file)?;
                    let (part, valid) = match collect_part(kind, &files, &cur, today, &mut out) {
                        // An unreadable trips file must not stop the kinds after it.
                        Err(e) if e.starts_with(NO_STAYS) => {
                            writeln!(out, "note: {e}").unwrap();
                            continue;
                        }
                        r => r?,
                    };
                    apply(&file, kind, &part, valid, today, &mut out)?;
                }
            }
            Ok((out.trim_end().to_string(), 0))
        }
        "status" => {
            let rows = status(&read_answers(&file_of())?, today);
            let cv = coverage(&rows);
            if a.json {
                out = serde_json::to_string_pretty(&serde_json::json!({"rows": rows, "coverage": cv})).unwrap();
            } else {
                for r in &rows {
                    let src = if r.source.is_empty() { String::new() } else { format!(" [{}]", r.source) };
                    let note = if r.note.is_empty() { String::new() } else { format!(": {}", r.note) };
                    writeln!(out, "{:<7} {}{src}{note}", r.state, r.id).unwrap();
                }
                write!(out, "\nfacts filled: {}/{} ({}%), passing: {}/{}", cv.filled, cv.facts, cv.pct, cv.done, cv.facts).unwrap();
            }
            Ok((out, 0))
        }
        "police" => {
            let ans = read_answers(&file_of())?;
            let need = police_countries(&ans);
            if ans.get("sign_date").is_none() || ans.get("trips").is_none() {
                out = "needs sign_date and trips first (collect nomads, pr-card)".into();
            } else if need.is_empty() {
                out = "no police certificate needed (no 183+ days in a row abroad in the last 4 years, from 18)".into();
            } else {
                for (c, l) in need {
                    writeln!(out, "{c}: needed (last stay ends {l}); issue after that day or within 6 months of signing").unwrap();
                }
            }
            Ok((out.trim_end().to_string(), 0))
        }
        "versions" => {
            let file = file_of();
            let v = list_versions(&file);
            out = if v.is_empty() { "no versions yet".into() } else { v.join("\n") };
            write!(out, "\n(in {})", versions_dir(&file).display()).unwrap();
            Ok((out, 0))
        }
        "restore" => {
            let file = write_file()?;
            let name = arg.filter(|n| list_versions(&file).iter().any(|v| v == n)).ok_or("usage: restore <name from versions>")?;
            let old = read_answers(&versions_dir(&file).join(name))?;
            keep_version(&file, 100)?;
            write_answers(&file, &old)?;
            Ok((format!("restored {name} (the draft before it is kept too)"), 0))
        }
        "proof" => {
            let (Some(key), Some(image), Some(source)) = (arg, a.pos.get(2), a.source.as_deref()) else {
                return Err("usage: proof <question> <image> --source '<where it was taken>' [--url u] [--note n]".into());
            };
            let name = add_proof(&write_file()?, key, Path::new(image), source, a.url.as_deref(), a.note.as_deref(), today)
                .map_err(|e| format!("proof: {e}"))?;
            Ok((format!("{key}: proof kept ({name}); run report to see it"), 0))
        }
        "report" => {
            let file = file_of();
            let ans = read_answers(&file)?;
            let rows = status(&ans, today);
            let cv = coverage(&rows);
            let mut images: Vec<(String, String)> = vec![];
            let mut missing = 0;
            let proofs = ans.get("_proofs").and_then(Value::as_object).cloned().unwrap_or_default();
            for p in proofs.values().filter_map(Value::as_array).flatten() {
                let Some(name) = p.get("image").and_then(Value::as_str) else { continue };
                match read_proof(&file, name)? {
                    Some(bytes) => {
                        let mime = match name.rsplit('.').next().unwrap_or("") {
                            "png" => "image/png",
                            "jpg" => "image/jpeg",
                            "webp" => "image/webp",
                            "gif" => "image/gif",
                            _ => "application/octet-stream",
                        };
                        images.push((name.to_string(), format!("data:{mime};base64,{}", base64(&bytes))));
                    }
                    None => missing += 1,
                }
            }
            let police = (ans.get("sign_date").is_some() && ans.get("trips").is_some()).then(|| police_countries(&ans));
            let html = evidence_page(&EvidenceInput {
                answers: &ans,
                rows: &rows,
                coverage: &cv,
                police,
                images: &images,
                today,
            });
            let folder = crate::paperwork::vault::private_dir(file.parent().unwrap_or(&dir))?;
            let page = folder.join("evidence.html");
            write_private(&page, html.as_bytes())?;
            // The page holds every answer in plain text: a detached timer removes it.
            let t = ttl();
            let _ = std::process::Command::new("sh")
                .args(["-c", "sleep \"$1\"; rm -f \"$2\"", "sh", &t.to_string()])
                .arg(&page)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            writeln!(out, "report: {}", page.display()).unwrap();
            write!(
                out,
                "facts filled {}/{} ({}%), {} screenshot(s){}; open it in your browser on this Mac within {} minutes (then it is removed; run report again)",
                cv.filled,
                cv.facts,
                cv.pct,
                images.len(),
                if missing > 0 { format!(", {missing} missing") } else { String::new() },
                (t as f64 / 60.0).round()
            )
            .unwrap();
            Ok((out, 0))
        }
        _ => Err(
            "usage: paperwork citizenship <answers|questions|screen|template|put|show|collect|collect-all|status|police|versions|restore|proof|report>"
                .into(),
        ),
    }
}

/// New file, then rename (a reader never sees half), mode 600.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let err = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    let mut f = o.open(&tmp).map_err(err)?;
    std::io::Write::write_all(&mut f, bytes).map_err(err)?;
    chmod600(&tmp)?;
    std::fs::rename(&tmp, path).map_err(err)
}

pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for ch in b.chunks(3) {
        let n = (u32::from(ch[0]) << 16)
            | (u32::from(*ch.get(1).unwrap_or(&0)) << 8)
            | u32::from(*ch.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= ch.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn matches_kind(kind: &str, path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let re = |r: &str| {
        regex::Regex::new(&format!("(?i){r}"))
            .unwrap()
            .is_match(&name)
    };
    match kind {
        "pr-card" => re(r"pr.?card|copr"),
        "passport" => re("passport"),
        "id" => re(r"licen[cs]e|health|photo.?card"),
        "marital" => re("separation|divorce|marriage|marital"),
        "language" => re("language|celpip|ielts|tef|tcf|pte"),
        "leases" => re("lease") || (re(r"^(home|past)?.?addresses?\b") && !re("email")),
        "linkedin" => re("positions|education|email addresses|phonenumbers|linkedin"),
        "nomads" => re("nomad"),
        "cra" => re("cra|tax|assessment"),
        _ => false,
    }
}

fn collect_part(
    kind: &str,
    files: &[String],
    cur: &Answers,
    today: &str,
    out: &mut String,
) -> Result<(Partial, Option<bool>), String> {
    let texts: Vec<String> = files.iter().map(|f| read(f)).collect::<Result<_, _>>()?;
    let joined = texts.join("\n");
    let one = |p: Partial| Ok((p, None));
    match kind {
        "pr-card" => Ok(c::parse_pr_card(&joined, today)),
        "passport" => {
            // Several passports: one file each.
            let (mut ps, mut st, mut bad) = (vec![], vec![], false);
            for t in &texts {
                let (p, v) = c::parse_passport(t, today);
                bad |= v == Some(false);
                for (k, dst) in [("passports", &mut ps), ("other_status", &mut st)] {
                    if let Some(Value::Array(a)) = p.get(k) {
                        dst.extend(a.iter().cloned());
                    }
                }
            }
            let mut p = Partial::new();
            if !ps.is_empty() {
                p.insert("passports".into(), Value::Array(ps));
            }
            if !st.is_empty() {
                p.insert("other_status".into(), Value::Array(st));
            }
            Ok((p, bad.then_some(false)))
        }
        "id" => one(c::parse_id(&joined)),
        "marital" => one(c::parse_marital(&joined)),
        "language" => one(c::parse_language(&joined)),
        "cra" => one(c::parse_cra(&joined)),
        "leases" => {
            // One CSV per file: joining them would read the second header as a row.
            let mut rows: Vec<Value> = texts
                .iter()
                .flat_map(|t| {
                    c::parse_leases(t)
                        .get("addresses")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect();
            rows.sort_by(|x, y| {
                y.get("from")
                    .and_then(Value::as_str)
                    .cmp(&x.get("from").and_then(Value::as_str))
            });
            one(if rows.is_empty() {
                Partial::new()
            } else {
                Partial::from_iter([("addresses".into(), Value::Array(rows))])
            })
        }
        "linkedin" => {
            let named: Vec<(String, String)> = files
                .iter()
                .zip(&texts)
                .map(|(f, t)| {
                    (
                        Path::new(f)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        t.clone(),
                    )
                })
                .collect();
            one(c::parse_linkedin(&named))
        }
        _ => {
            let mut stays = vec![];
            for t in &texts {
                stays.extend(c::parse_nomads(t)?);
            }
            // No readable stay is a file this reader does not know, never "no trips".
            if stays.is_empty() {
                return Err(format!(
                    "{NO_STAYS}, so the saved trips were kept; the file needs start, end and country columns (to say you took no trips, `put` a file with `trips = []`)"
                ));
            }
            stays.sort_by(|x, y| x.from.cmp(&y.from));
            let mut p = Partial::new();
            p.insert("trips".into(), Value::Array(c::trips_from_stays(&stays)));
            match cur.get("pr_date").and_then(Value::as_str) {
                Some(pr) => {
                    p.insert("temp_periods".into(), Value::Array(c::temp_from_stays(&stays, pr)));
                }
                None => writeln!(out, "note: no PR date yet; collect the PR card, then nomads again for time before PR").unwrap(),
            }
            one(p)
        }
    }
}

fn apply(
    file: &Path,
    kind: &str,
    part: &Partial,
    valid: Option<bool>,
    today: &str,
    out: &mut String,
) -> Result<(), String> {
    if valid == Some(false) {
        writeln!(out, "{kind}: the machine readable zone fails its check digits: read it again (a letter is misread)").unwrap();
    }
    let before = read_answers(file)?;
    let (next, mut added, kept) = merge(&before, part, kind, Some(today));
    let (next, der, _) = merge(&next, &c::derive(&next, &[], today), "derived", Some(today));
    added.extend(der.into_iter().map(|k| format!("{k} (derived)")));
    if added.is_empty() {
        writeln!(out, "{kind}: nothing new").unwrap();
        return Ok(());
    }
    let v = keep_version(file, 100)?;
    write_answers(file, &next)?;
    writeln!(
        out,
        "{kind}: added {}{}{}",
        added.join(", "),
        if kept.is_empty() {
            String::new()
        } else {
            format!("; kept yours: {}", kept.join(", "))
        },
        if v.is_some() { " (old draft kept)" } else { "" }
    )
    .unwrap();
    Ok(())
}
