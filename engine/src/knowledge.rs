//! The knowledge base: sourced, dated facts in `knowledge/data/*.toml`.
//!
//! Every file has `[meta]` with `as_of`, `review_by` and `sources`. Every fact
//! table has `source` and `confidence`: "verified" (from a primary source),
//! "estimate" (not yet confirmed) or "heuristic" (a rule of thumb).
//!
//! A user can override or add facts privately in `<user>/knowledge/*.toml`
//! (same schema). Entries with an `id` replace the shared entry with that id;
//! other tables merge key by key. That override is still read by the Python
//! strategy code; this module covers `knowledge list|check|propose`.
//!
//! Output matches the Python code byte for byte (decision 0010), so TOML
//! values print as Python's `str()` and `repr()` would.

use crate::core::pyjson::float_repr;
use chrono::NaiveDate;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

pub const CATEGORIES: [&str; 15] = [
    "groceries",
    "gas",
    "ev_charging",
    "transit",
    "rideshare",
    "delivery",
    "dining",
    "entertainment",
    "recurring",
    "streaming",
    "drugstore",
    "travel",
    "foreign",
    "rent",
    "other",
];

pub fn data_dir() -> PathBuf {
    crate::core::paths::root().join("knowledge").join("data")
}

/// A TOML file that does not parse: Python's message differs, so the caller
/// hands the command back to Python.
#[derive(Debug)]
pub struct Unparsed(pub PathBuf);

fn read(path: &Path) -> Result<Table, Unparsed> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.parse::<Table>().ok())
        .ok_or_else(|| Unparsed(path.to_path_buf()))
}

fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn names() -> Vec<String> {
    toml_files(&data_dir()).iter().map(|p| stem(p)).collect()
}

/// Loads `knowledge/data/<name>.toml` (no user override).
pub fn load(name: &str) -> Result<Table, Unparsed> {
    let path = data_dir().join(format!("{name}.toml"));
    if path.exists() {
        read(&path)
    } else {
        Ok(Table::new())
    }
}

fn date_of(v: Option<&Value>) -> Option<NaiveDate> {
    let Value::Datetime(dt) = v? else { return None };
    if dt.time.is_some() {
        return None;
    }
    let d = dt.date?;
    NaiveDate::from_ymd_opt(d.year as i32, d.month as u32, d.day as u32)
}

fn py_float(f: f64) -> String {
    if f.is_nan() {
        "nan".into()
    } else if f.is_infinite() {
        if f > 0.0 { "inf" } else { "-inf" }.into()
    } else {
        float_repr(f)
    }
}

fn py_datetime(dt: &toml::value::Datetime) -> String {
    let mut s = dt.to_string().replacen('T', " ", 1);
    if dt.date.is_none() {
        s = dt.to_string();
    }
    if let Some(t) = dt.time.filter(|_| dt.date.is_some()) {
        // Python prints seconds always and the fraction as microseconds.
        let ns = t.nanosecond.unwrap_or(0);
        let frac = if ns > 0 {
            format!(".{:06}", ns / 1000)
        } else {
            String::new()
        };
        let date = dt.date.unwrap();
        let tz = match dt.offset {
            Some(toml::value::Offset::Z) => "+00:00".to_string(),
            Some(toml::value::Offset::Custom { minutes }) => {
                format!(
                    "{}{:02}:{:02}",
                    if minutes < 0 { '-' } else { '+' },
                    minutes.abs() / 60,
                    minutes.abs() % 60
                )
            }
            None => String::new(),
        };
        s = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{frac}{tz}",
            date.year,
            date.month,
            date.day,
            t.hour,
            t.minute,
            t.second.unwrap_or(0)
        );
    }
    s
}

/// Python's `str()` of a TOML value.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Datetime(dt) => py_datetime(dt),
        other => py_repr(other),
    }
}

fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr()` of a TOML value.
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::String(s) => str_repr(s),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => py_float(*f),
        Value::Boolean(b) => if *b { "True" } else { "False" }.into(),
        Value::Datetime(dt) => match (dt.date, dt.time) {
            (Some(d), None) => format!("datetime.date({}, {}, {})", d.year, d.month, d.day),
            _ => format!("datetime({})", py_datetime(dt)),
        },
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Table(t) => format!(
            "{{{}}}",
            t.iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Integer(i)) => *i != 0,
        Some(Value::Float(f)) => *f != 0.0,
        Some(Value::Boolean(b)) => *b,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Table(t)) => !t.is_empty(),
        Some(Value::Datetime(_)) => true,
    }
}

/// Python `==` between TOML values (1 == 1.0, True == 1).
fn py_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    let num = |v: &Value| match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Boolean(b) => Some(*b as i64 as f64),
        _ => None,
    };
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => match (num(x), num(y)) {
            (Some(p), Some(q)) => p == q,
            _ => match (x, y) {
                (Value::Array(p), Value::Array(q)) => {
                    p.len() == q.len() && p.iter().zip(q).all(|(m, n)| py_eq(Some(m), Some(n)))
                }
                (Value::Table(p), Value::Table(q)) => {
                    p.len() == q.len() && p.iter().all(|(k, m)| py_eq(Some(m), q.get(k)))
                }
                _ => x == y,
            },
        },
        _ => false,
    }
}

#[derive(Default)]
pub struct CheckReport {
    pub files: usize,
    pub facts: usize,
    pub estimates: Vec<String>,
    pub heuristics: Vec<String>,
    pub stale: Vec<String>,
    pub missing_source: Vec<String>,
    pub problems: Vec<String>,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.missing_source.is_empty() && self.problems.is_empty()
    }
}

fn walk(at: &str, node: &Value, report: &mut CheckReport) {
    match node {
        Value::Table(t) => {
            if t.contains_key("confidence") || t.contains_key("source") {
                report.facts += 1;
                if !truthy(t.get("source")) {
                    report.missing_source.push(at.to_string());
                }
                match t.get("confidence").and_then(Value::as_str) {
                    Some("verified") => {}
                    Some("estimate") => report.estimates.push(at.to_string()),
                    Some("heuristic") => report.heuristics.push(at.to_string()),
                    _ => report.problems.push(format!(
                        "{at}: confidence must be verified, estimate or heuristic"
                    )),
                }
            }
            for (k, v) in t {
                if k != "meta" {
                    walk(&format!("{at}.{k}"), v, report);
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                let label = match v {
                    Value::Table(t) => [t.get("id"), t.get("ticker")]
                        .into_iter()
                        .flatten()
                        .find(|x| truthy(Some(x)))
                        .map_or(i.to_string(), py_str),
                    _ => i.to_string(),
                };
                walk(&format!("{at}[{label}]"), v, report);
            }
        }
        _ => {}
    }
}

pub fn check(today: NaiveDate, dir: &Path) -> Result<CheckReport, Unparsed> {
    let mut report = CheckReport::default();
    for path in toml_files(dir) {
        report.files += 1;
        let data = read(&path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let meta = match data.get("meta") {
            Some(Value::Table(m)) => m.clone(),
            None => Table::new(),
            Some(_) => return Err(Unparsed(path)), // Python fails on a non-table [meta]
        };
        for key in ["as_of", "review_by", "sources"] {
            if !meta.contains_key(key) {
                report.problems.push(format!("{name}: [meta] needs {key}"));
            }
        }
        if let Some(review_by) = date_of(meta.get("review_by")).filter(|d| *d < today) {
            report
                .stale
                .push(format!("{} (review by {review_by})", stem(&path)));
        }
        walk(&stem(&path), &Value::Table(data), &mut report);
    }
    if names().iter().any(|n| n == "credit_cards") {
        let cards = load("credit_cards")?;
        if let Some(Value::Array(cards)) = cards.get("cards") {
            for card in cards {
                let Value::Table(card) = card else {
                    return Err(Unparsed(data_dir()));
                };
                let earn = match card.get("earn") {
                    Some(Value::Table(e)) => e.clone(),
                    None => Table::new(),
                    Some(_) => return Err(Unparsed(data_dir())),
                };
                let bad: Vec<&String> = earn
                    .keys()
                    .filter(|k| !CATEGORIES.contains(&k.as_str()))
                    .collect();
                if !bad.is_empty() || !earn.contains_key("other") {
                    let shown = if bad.is_empty() {
                        "missing other".to_string()
                    } else {
                        format!(
                            "[{}]",
                            bad.iter()
                                .map(|k| str_repr(k))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    let id = card.get("id").map_or("None".to_string(), py_str);
                    report
                        .problems
                        .push(format!("credit_cards[{id}]: earn keys {shown}"));
                }
            }
        }
    }
    Ok(report)
}

pub fn render_check(r: &CheckReport) -> String {
    let mut lines = vec![
        "# Knowledge check\n".to_string(),
        format!("- Files: {}; facts with a source: {}.", r.files, r.facts),
        format!(
            "- Estimates (verify before relying on them): {}.",
            r.estimates.len()
        ),
        format!(
            "- Rules of thumb (judgment with no official source, by nature): {}.",
            r.heuristics.len()
        ),
        format!("- Stale files (past review_by): {}.", r.stale.len()),
    ];
    lines.extend(r.stale.iter().map(|s| format!("  - {s}")));
    if !r.missing_source.is_empty() {
        lines.push(format!(
            "- Facts without a source: {}",
            r.missing_source.len()
        ));
        lines.extend(r.missing_source.iter().take(20).map(|m| format!("  - {m}")));
    }
    if !r.problems.is_empty() {
        lines.push("- Problems:".into());
        lines.extend(r.problems.iter().take(20).map(|p| format!("  - {p}")));
    }
    lines.push(
        "\nTo update a fact: edit its TOML with a new source and dates, run `bin/cashcast knowledge check`, \
         then open a PR (see CONTRIBUTING.md). Private overrides go in your user folder's knowledge/."
            .into(),
    );
    lines.join("\n")
}

/// `knowledge list`: one line per file.
pub fn render_list() -> Result<String, Unparsed> {
    let mut out = String::new();
    for name in names() {
        let data = load(&name)?;
        let meta = match data.get("meta") {
            Some(Value::Table(m)) => m.clone(),
            None => Table::new(),
            Some(_) => return Err(Unparsed(data_dir())),
        };
        let show = |k: &str| meta.get(k).map_or("None".to_string(), py_str);
        let title = meta.get("title").map_or(String::new(), py_str);
        out += &format!(
            "- {name}: {title} (checked {}, review by {})\n",
            show("as_of"),
            show("review_by")
        );
    }
    Ok(out)
}

/// A candidate fact file, reviewed against the current one before it becomes a PR.
pub struct Proposal {
    pub name: String,
    pub check: CheckReport,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub problems: Vec<String>,
}

impl Proposal {
    pub fn ok(&self) -> bool {
        self.check.ok() && self.problems.is_empty()
    }
}

fn entries(data: &Table) -> Vec<(String, Table)> {
    let mut out: Vec<(String, Table)> = Vec::new();
    let mut put = |k: String, t: Table| match out.iter_mut().find(|(x, _)| *x == k) {
        Some(slot) => slot.1 = t,
        None => out.push((k, t)),
    };
    for (key, value) in data {
        if key == "meta" {
            continue;
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    if let Value::Table(t) = item
                        && (t.contains_key("id") || t.contains_key("ticker"))
                    {
                        let id = [t.get("id"), t.get("ticker")]
                            .into_iter()
                            .flatten()
                            .find(|x| truthy(Some(x)))
                            .or(t.get("ticker"))
                            .map_or("None".to_string(), py_str);
                        put(format!("{key}[{id}]"), t.clone());
                    }
                }
            }
            Value::Table(t) => put(key.clone(), t.clone()),
            _ => {}
        }
    }
    out
}

/// Validates `candidate` and lists what it adds, removes and changes in `dir`/<same name>.
pub fn propose(candidate: &Path, today: NaiveDate, dir: &Path) -> Result<Proposal, Unparsed> {
    let tmp = std::env::temp_dir().join(format!("cashcast-propose-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|_| Unparsed(tmp.clone()))?;
    let file = candidate.file_name().unwrap_or_default();
    let copied = std::fs::copy(candidate, tmp.join(file));
    let report = copied
        .map_err(|_| Unparsed(candidate.to_path_buf()))
        .and_then(|_| check(today, &tmp));
    let _ = std::fs::remove_dir_all(&tmp);
    let report = report?;
    let new = read(candidate)?;
    let current = dir.join(file);
    let old = if current.exists() {
        read(&current)?
    } else {
        Table::new()
    };
    let as_of = |t: &Table| date_of(t.get("meta").and_then(|m| m.get("as_of")));
    let mut prop = Proposal {
        name: stem(candidate),
        check: report,
        added: Vec::new(),
        removed: Vec::new(),
        changed: Vec::new(),
        problems: Vec::new(),
    };
    if let (Some(o), Some(n)) = (as_of(&old), as_of(&new))
        && n < o
    {
        prop.problems
            .push(format!("[meta] as_of {n} is older than the current {o}"));
    }
    let (a, b) = (entries(&old), entries(&new));
    let find = |list: &[(String, Table)], k: &str| {
        list.iter().find(|(x, _)| x == k).map(|(_, t)| t.clone())
    };
    prop.added = b
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| find(&a, k).is_none())
        .collect();
    prop.added.sort();
    prop.removed = a
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| find(&b, k).is_none())
        .collect();
    prop.removed.sort();
    let mut both: Vec<String> = a
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| find(&b, k).is_some())
        .collect();
    both.sort();
    for k in both {
        let (x, y) = (find(&a, &k).unwrap(), find(&b, &k).unwrap());
        let mut fields: Vec<&String> = x.keys().chain(y.keys()).collect();
        fields.sort();
        fields.dedup();
        let show = |v: Option<&Value>| v.map_or("None".to_string(), py_str);
        let mut diffs: Vec<String> = Vec::new();
        for f in fields {
            if py_eq(x.get(f), y.get(f)) {
                continue;
            }
            if f == "source" {
                diffs.push("source changed".into());
            } else {
                diffs.push(format!("{f} {} → {}", show(x.get(f)), show(y.get(f))));
            }
        }
        if !diffs.is_empty() {
            // keep value changes first, the source note last
            diffs.sort_by_key(|d| d == "source changed");
            prop.changed.push(format!("{k}: {}", diffs.join(", ")));
        }
    }
    Ok(prop)
}

pub fn render_proposal(p: &Proposal) -> String {
    let mut lines = vec![format!("# Knowledge proposal: {}", p.name), String::new()];
    for (title, items) in [
        ("Added", &p.added),
        ("Removed", &p.removed),
        ("Changed", &p.changed),
    ] {
        lines.push(format!("- {title}: {}", items.len()));
        lines.extend(items.iter().take(30).map(|i| format!("  - {i}")));
    }
    if p.ok() {
        lines.extend([
            String::new(),
            "Ready. Shared fact: copy it to knowledge/data/ and open a PR (CONTRIBUTING.md)."
                .into(),
            "Only true for you: put it in private/users/<you>/knowledge/ instead.".into(),
        ]);
    } else {
        lines.extend([String::new(), "Blocked:".into()]);
        let blocked = p.problems.iter().chain(&p.check.problems).cloned().chain(
            p.check
                .missing_source
                .iter()
                .map(|m| format!("no source: {m}")),
        );
        lines.extend(blocked.map(|x| format!("- {x}")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_print_like_python() {
        let t: Table = "a = 'x'\nb = 1.0\nc = [1, 'y']\nd = 2026-10-18\ne = true\nf = {g = 1}"
            .parse()
            .unwrap();
        assert_eq!(py_str(&t["a"]), "x");
        assert_eq!(py_str(&t["b"]), "1.0");
        assert_eq!(py_str(&t["c"]), "[1, 'y']");
        assert_eq!(py_str(&t["d"]), "2026-10-18");
        assert_eq!(py_repr(&t["d"]), "datetime.date(2026, 10, 18)");
        assert_eq!(py_str(&t["e"]), "True");
        assert_eq!(py_str(&t["f"]), "{'g': 1}");
    }

    #[test]
    fn check_finds_stale_files_and_missing_sources() {
        let dir = std::env::temp_dir().join(format!("cc-knowledge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("topic.toml"),
            "[meta]\nas_of = 2026-01-01\nreview_by = 2026-02-01\nsources = []\n\n[[items]]\nid = \"a\"\nconfidence = \"estimate\"\nsource = \"\"\n\n[rule]\nconfidence = \"guess\"\nsource = \"s\"\n",
        )
        .unwrap();
        let r = check(NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(), &dir).unwrap();
        assert_eq!((r.files, r.facts), (1, 2));
        assert_eq!(r.stale, vec!["topic (review by 2026-02-01)"]);
        assert_eq!(r.missing_source, vec!["topic.items[a]"]);
        assert_eq!(r.estimates, vec!["topic.items[a]"]);
        assert_eq!(
            r.problems[0],
            "topic.rule: confidence must be verified, estimate or heuristic"
        );
        assert!(!r.ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
