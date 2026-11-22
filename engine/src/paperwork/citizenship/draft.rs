//! The citizenship answers as a draft that grows over several days (ported
//! from sidequest `citizenship-draft.ts`). Collectors merge into one sealed
//! file, `<user dir>/paperwork/citizenship/answers.sqv` (TOML inside the
//! vault). Each change first keeps a sealed dated copy in `versions/`.
//! `status` says, per question, what is filled, from which source, and what
//! is still open. File parts take paths and never print a value.

use super::answers::{Answers, Finding, get, judge_answers, questions};
use crate::paperwork::vault;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub const PURPOSE: &str = "citizenship:answers";
pub const PROOF_PURPOSE: &str = "citizenship:proof";
const PROOF_EXT: &[&str] = &["png", "jpg", "webp", "gif"];

/// How each answer is found. `fact`: a source holds it. `choice`: the owner decides;
/// `propose` (JSON) is the usual answer, said once in a yes/no batch.
pub const PLAN: &[(&str, &str, &[&str], Option<&str>)] = &[
    ("method", "choice", &[], Some("\"online\"")),
    ("service_language", "choice", &[], Some("\"English\"")),
    ("accommodation", "choice", &[], Some("\"none\"")),
    ("uci", "fact", &["pr-card"], None),
    ("name", "fact", &["pr-card", "passport"], None),
    ("other_names", "fact", &["passport", "pr-card"], None),
    (
        "name_changed_since_pr",
        "fact",
        &["passport", "pr-card"],
        None,
    ),
    ("marital_status", "fact", &["marital"], None),
    ("birth_date", "fact", &["pr-card", "passport"], None),
    ("pr_date", "fact", &["pr-card"], None),
    ("sign_date", "fact", &["derived"], None),
    ("contact", "fact", &["linkedin", "leases"], None),
    ("temp_periods", "fact", &["nomads"], None),
    ("trips", "fact", &["nomads"], None),
    ("addresses", "fact", &["leases"], None),
    ("activities", "fact", &["linkedin"], None),
    ("taxes", "fact", &["cra"], None),
    ("other_status", "fact", &["passport"], None),
    ("passports", "fact", &["passport"], None),
    ("ids", "fact", &["derived"], None),
    ("language_proof", "fact", &["language"], None),
    ("prohibitions_none", "choice", &[], None),
    ("representative", "choice", &[], Some("false")),
    ("police_certificates", "fact", &["derived"], None),
    ("certificate", "choice", &[], Some("\"electronic\"")),
    ("consent_mp", "choice", &[], None),
    ("consent_electors", "choice", &[], None),
    ("city", "fact", &["leases"], None),
];
fn plan(
    k: &str,
) -> Option<&'static (
    &'static str,
    &'static str,
    &'static [&'static str],
    Option<&'static str>,
)> {
    PLAN.iter().find(|p| p.0 == k)
}

/// A short fingerprint (FNV-1a) of what a collector wrote: when it differs, the owner edited it.
pub fn mark(v: &Value) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in serde_json::to_string(v).unwrap_or_default().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")[..12].to_string()
}

fn sub_map(a: &Answers, k: &str) -> Map<String, Value> {
    a.get(k)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Merges a collector's part. What the owner wrote or edited (no source, "owner", or a
/// value that no longer matches its mark) is never replaced; an object of theirs only gets
/// missing fields. A collector's own value is refreshed by any collector; `Null` removes it.
pub fn merge(
    a: &Answers,
    part: &Map<String, Value>,
    source: &str,
    on: Option<&str>,
) -> (Answers, Vec<String>, Vec<String>) {
    let mut next = a.clone();
    let mut src = sub_map(a, "_sources");
    let mut marks = sub_map(a, "_marks");
    let mut when = sub_map(a, "_collected");
    let (mut added, mut kept) = (vec![], vec![]);
    for (k, v) in part {
        if k.starts_with('_') {
            continue;
        }
        let cur = get(a, k);
        let had = cur.is_some();
        let edited = marks
            .get(k)
            .and_then(Value::as_str)
            .is_some_and(|m| m != mark(cur.unwrap_or(&Value::Null)));
        let s = src.get(k).and_then(Value::as_str);
        let owners = had && (s.is_none() || s == Some("owner") || edited);
        if owners {
            let cur = cur.unwrap();
            match (v, cur) {
                (Value::Object(new), Value::Object(old)) => {
                    let mut val = new.clone();
                    for (x, y) in old {
                        val.insert(x.clone(), y.clone());
                    }
                    let val = Value::Object(val);
                    if &val != cur {
                        next.insert(k.clone(), val);
                        added.push(k.clone()); // still the owner's: no source, no mark
                    }
                }
                _ => {
                    if !v.is_null() && v != cur {
                        kept.push(k.clone());
                    }
                }
            }
            continue;
        }
        if v.is_null() {
            if had {
                next.remove(k);
                src.remove(k);
                marks.remove(k);
                when.remove(k);
                added.push(k.clone());
            }
            continue;
        }
        let mut val = v.clone();
        if let (Some(Value::Object(old)), Value::Object(new)) = (cur, v) {
            let mut m = old.clone();
            for (x, y) in new {
                m.insert(x.clone(), y.clone());
            }
            val = Value::Object(m);
        }
        if cur == Some(&val) {
            continue;
        }
        marks.insert(k.clone(), json!(mark(&val)));
        next.insert(k.clone(), val);
        src.insert(k.clone(), json!(source));
        if let Some(on) = on {
            when.insert(k.clone(), json!(on));
        }
        added.push(k.clone());
    }
    next.insert("_sources".into(), Value::Object(src));
    next.insert("_marks".into(), Value::Object(marks));
    if when.is_empty() {
        next.remove("_collected");
    } else {
        next.insert("_collected".into(), Value::Object(when));
    }
    (next, added, kept)
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: String,
    pub kind: &'static str,
    pub state: &'static str, // done, fix, open, confirm
    pub source: String,
    pub note: String,
}

/// One row per question. Notes never hold values.
pub fn status(a: &Answers, today: &str) -> Vec<Row> {
    let f = judge_answers(a, today);
    // Findings with their own ids belong to an answer.
    let owner = |id: &str| {
        match id {
            "presence" => "trips",
            "police" => "police_certificates",
            "age" => "birth_date",
            "file" => "uci",
            x => x,
        }
        .to_string()
    };
    questions()
        .into_iter()
        .map(|(k, _)| {
            let p = plan(k).unwrap();
            let fails: Vec<&Finding> = f
                .iter()
                .filter(|x| x.level == "FAIL" && (x.id == k || owner(&x.id) == k))
                .collect();
            let has = get(a, k).is_some();
            let source = a
                .get("_sources")
                .and_then(|s| s.get(k))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| if has { "owner".into() } else { String::new() });
            let state = if !has {
                if p.3.is_some() { "confirm" } else { "open" }
            } else if !fails.is_empty() {
                "fix"
            } else {
                "done"
            };
            let note = match state {
                "fix" => fails
                    .iter()
                    .map(|x| x.msg.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
                "confirm" => format!("usual answer: {}", p.3.unwrap()),
                "open" if !p.2.is_empty() => format!("collect from: {}", p.2.join(", ")),
                "open" => "the owner answers".into(),
                _ => String::new(),
            };
            Row {
                id: k.into(),
                kind: p.1,
                state,
                source,
                note,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    pub facts: usize,
    pub filled: usize,
    pub done: usize,
    pub pct: f64,
}
/// Share of facts answered (done or fix: the value is there) and of facts done.
pub fn coverage(rows: &[Row]) -> Coverage {
    let facts: Vec<&Row> = rows.iter().filter(|r| r.kind == "fact").collect();
    let filled = facts
        .iter()
        .filter(|r| r.state == "done" || r.state == "fix")
        .count();
    let done = facts.iter().filter(|r| r.state == "done").count();
    let pct = (1000.0 * filled as f64 / facts.len().max(1) as f64).round() / 10.0;
    Coverage {
        facts: facts.len(),
        filled,
        done,
        pct,
    }
}

// ---- Files: the draft and its versions ----

pub fn citizenship_dir(user_dir: &Path) -> PathBuf {
    user_dir.join("paperwork").join("citizenship")
}
pub fn answers_file(user_dir: &Path) -> PathBuf {
    citizenship_dir(user_dir).join("answers.sqv")
}
pub fn sealed(file: &Path) -> bool {
    file.extension().is_some_and(|e| e == "sqv")
}
pub fn versions_dir(file: &Path) -> PathBuf {
    file.parent().unwrap_or(Path::new(".")).join("versions")
}
pub fn proofs_dir(file: &Path) -> PathBuf {
    file.parent().unwrap_or(Path::new(".")).join("proofs")
}
fn stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string()
}
#[cfg(unix)]
fn chmod(p: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("cannot protect {}: {e}", p.display()))
}
#[cfg(not(unix))]
fn chmod(_: &Path, _: u32) -> Result<(), String> {
    Ok(())
}

/// Keeps a dated copy of the sealed draft (mode 600). Returns its path, or None with no file.
pub fn keep_version(file: &Path, max: usize) -> Result<Option<PathBuf>, String> {
    if !file.exists() {
        return Ok(None);
    }
    let dir = vault::private_dir(&versions_dir(file))?;
    let t = stamp();
    let mut i = 1;
    let to = loop {
        let p = dir.join(format!("answers-{t}-{i:02}.sqv"));
        if !p.exists() {
            break p;
        }
        i += 1;
    };
    std::fs::copy(file, &to).map_err(|e| format!("cannot keep a version: {e}"))?;
    chmod(&to, 0o600)?;
    let all = list_versions(file);
    for old in &all[..all.len().saturating_sub(max)] {
        let _ = std::fs::remove_file(dir.join(old));
    }
    Ok(Some(to))
}

pub fn list_versions(file: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(versions_dir(file))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("answers-") && n.ends_with(".sqv"))
        .collect();
    v.sort();
    v
}

// ---- TOML in, TOML out ----

pub fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => json!(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        // An unquoted 2030-06-01 is a TOML date: keep it as the text.
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}
fn json_to_toml(v: &Value) -> Option<toml::Value> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => toml::Value::Integer(i),
            None => toml::Value::Float(n.as_f64()?),
        },
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Array(a) => toml::Value::Array(a.iter().filter_map(json_to_toml).collect()),
        Value::Object(o) => toml::Value::Table(
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), json_to_toml(v)?)))
                .collect(),
        ),
    })
}
pub fn parse_toml(text: &str) -> Result<Answers, String> {
    let t: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("not valid TOML: {}", e.message()))?;
    match toml_to_json(toml::Value::Table(t)) {
        Value::Object(m) => Ok(m),
        _ => Ok(Map::new()),
    }
}
pub fn to_toml(a: &Answers) -> String {
    let t =
        json_to_toml(&Value::Object(a.clone())).unwrap_or(toml::Value::Table(Default::default()));
    toml::to_string(&t).unwrap_or_default()
}

/// The answers of a sealed `.sqv` draft or a plain `.toml` file; empty when missing.
pub fn read_answers(file: &Path) -> Result<Answers, String> {
    let text = if sealed(file) {
        match vault::read_sealed(file, PURPOSE)? {
            Some(b) => String::from_utf8(b).map_err(|_| "the draft is not text".to_string())?,
            None => return Ok(Map::new()),
        }
    } else {
        if !file.exists() {
            return Ok(Map::new());
        }
        std::fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?
    };
    parse_toml(&text)
}

/// Writes the answers sealed (the vault writes a new file, then renames it). Never plain.
pub fn write_answers(file: &Path, a: &Answers) -> Result<(), String> {
    if !sealed(file) {
        return Err("the draft is always a sealed .sqv file; nothing was written".into());
    }
    let text = format!(
        "# Citizenship answers: private, sealed. Collectors never replace what you wrote.\n# Each change keeps a copy in versions/.\n{}",
        to_toml(a)
    );
    vault::write_sealed(file, text.as_bytes(), PURPOSE)
}

/// Scratch answers files (top-level TOML or YAML in `dirs`) that others can read: the ones
/// `put` takes. Only an open file is read, only to see whether it has an answers key.
pub fn open_scratch(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let keys: Vec<&str> = questions().iter().map(|(k, _)| *k).collect();
    let re = regex::Regex::new(&format!(r"(?m)^({})\s*[:=]", keys.join("|"))).unwrap();
    let mut out = vec![];
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let n = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !(n.ends_with(".toml") || n.ends_with(".yaml") || n.ends_with(".yml")) {
                continue;
            }
            let Ok(m) = std::fs::metadata(&p) else {
                continue;
            };
            #[cfg(unix)]
            let open = {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o077 != 0
            };
            #[cfg(not(unix))]
            let open = false;
            if !m.is_file() || !open || m.len() > 1_000_000 {
                continue;
            }
            if std::fs::read_to_string(&p).is_ok_and(|t| re.is_match(&t)) {
                out.push(p);
            }
        }
    }
    out
}

pub const HIDDEN: &str = "(hidden: an ID number)";
/// The answers with ID numbers hidden (UCI, SIN, passport numbers): for agents and pipes.
pub fn masked(a: &Answers) -> Answers {
    let mut m = a.clone();
    if m.get("uci").is_some_and(|v| !v.is_null()) {
        m.insert("uci".into(), json!(HIDDEN));
    }
    if let Some(Value::Object(t)) = m.get_mut("taxes")
        && t.contains_key("sin")
    {
        t.insert("sin".into(), json!(HIDDEN));
    }
    if let Some(Value::Array(ps)) = m.get_mut("passports") {
        for p in ps.iter_mut().filter_map(Value::as_object_mut) {
            if p.contains_key("number") {
                p.insert("number".into(), json!(HIDDEN));
            }
        }
    }
    m
}

// ---- Proofs: screenshots of the sources ----

fn rand_hex() -> String {
    let mut b = [0u8; 6];
    getrandom::getrandom(&mut b).expect("system random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Keeps a sealed copy of a screenshot as proof for one answer, and records where it came
/// from. A big photo is made at most 1600 px wide with `sips` (macOS) in a private temp
/// folder that is removed afterwards; without `sips` the file is copied as it is.
pub fn add_proof(
    file: &Path,
    key: &str,
    image: &Path,
    source: &str,
    url: Option<&str>,
    note: Option<&str>,
    today: &str,
) -> Result<String, String> {
    if plan(key).is_none() {
        return Err(format!("no question '{key}'"));
    }
    let ext = image
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .map(|e| if e == "jpeg" { "jpg".to_string() } else { e })
        .filter(|e| PROOF_EXT.contains(&e.as_str()))
        .ok_or("a proof is a .png, .jpg, .webp or .gif image")?;
    if let Some(u) = url {
        let l = u.to_lowercase();
        if !(l.starts_with("http://") || l.starts_with("https://")) {
            return Err("a proof's --url is a web address (http or https)".into());
        }
    }
    if !image.exists() {
        return Err(format!("no image at {}", image.display()));
    }
    let dir = vault::private_dir(&proofs_dir(file))?;
    let t = stamp();
    let mut i = 1;
    let name = loop {
        let n = format!("{key}-{t}-{i:02}.{ext}");
        if !dir.join(&n).exists() && !dir.join(format!("{n}.sqv")).exists() {
            break n;
        }
        i += 1;
    };
    let work = std::env::temp_dir().join(format!("cashcast-proof-{}", rand_hex()));
    std::fs::create_dir(&work).map_err(|e| format!("cannot make a private temp folder: {e}"))?;
    let sealed_ok = (|| {
        chmod(&work, 0o700)?;
        let to = work.join(&name);
        let small = std::process::Command::new("sips")
            .args(["-Z", "1600"])
            .arg(image)
            .arg("--out")
            .arg(&to)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !small.is_ok_and(|s| s.success()) || !to.exists() {
            std::fs::copy(image, &to).map_err(|e| format!("cannot copy the image: {e}"))?;
        }
        chmod(&to, 0o600)?;
        let bytes = std::fs::read(&to).map_err(|e| format!("cannot read the image: {e}"))?;
        vault::write_sealed(&dir.join(format!("{name}.sqv")), &bytes, PROOF_PURPOSE)
    })();
    let _ = std::fs::remove_dir_all(&work);
    sealed_ok?;
    let mut a = read_answers(file)?;
    let mut proofs = sub_map(&a, "_proofs");
    let mut p = Map::new();
    p.insert("image".into(), json!(name));
    p.insert("source".into(), json!(source));
    for (k, v) in [("url", url), ("note", note)] {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            p.insert(k.into(), json!(v));
        }
    }
    p.insert("taken".into(), json!(today));
    let entry = proofs.entry(key.to_string()).or_insert(json!([]));
    if let Some(l) = entry.as_array_mut() {
        l.push(Value::Object(p));
    }
    a.insert("_proofs".into(), Value::Object(proofs));
    keep_version(file, 100)?;
    write_answers(file, &a)?;
    Ok(name)
}

/// A proof's image bytes from its sealed copy; None when it is not there. Only a plain
/// file name from proofs/, never a path that leaves it.
pub fn read_proof(file: &Path, name: &str) -> Result<Option<Vec<u8>>, String> {
    let plain = !name.is_empty() && Path::new(name).file_name().is_some_and(|n| n == name);
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    if !plain || !PROOF_EXT.contains(&ext.as_str()) {
        return Ok(None);
    }
    vault::read_sealed(&proofs_dir(file).join(format!("{name}.sqv")), PROOF_PURPOSE)
}
