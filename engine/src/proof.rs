//! The README's Proof section: measure the repo's own numbers, save them, and write the section.
//!
//! ```text
//! cashcast-engine proof measure [--events PATH]   # run the counts, save docs/proof/metrics.json, write README
//! cashcast-engine proof field [--private PATH]    # this machine's real-use totals into metrics.json (local)
//! cashcast-engine proof render                    # write the README section from the saved file
//! cashcast-engine proof check                     # exit 1 when the README differs from the saved file
//! ```
//!
//! Every number comes from the repo itself (tests, knowledge files, connectors,
//! gate log). Nothing here reads private/ data: counts and rates only.

use crate::strategy::py::fmt_g;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub const START: &str = "<!-- proof:start -->";
pub const END: &str = "<!-- proof:end -->";
const TASK: &str = "proof-real-use-outcomes";
const MEASURE: &str = "engine/target/release/cashcast-engine proof measure";
/// `events::log(..., "command", ...)` landed in the CLI that day.
const FIELD_SINCE: &str = "2026-10-18";

fn root() -> PathBuf {
    crate::core::paths::root()
}

fn metrics_path() -> PathBuf {
    root().join("docs/proof/metrics.json")
}

pub fn machine() -> String {
    let system = match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        o => o,
    };
    let arch = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "arm64",
        (_, a) => a,
    };
    format!("{system} {arch}")
}

/// Files under `dir` (recursive) whose name ends with one of `exts`.
fn files(dir: &str, exts: &[&str]) -> Vec<PathBuf> {
    fn walk(p: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if name != "node_modules" && name != "target" {
                    walk(&path, exts, out);
                }
            } else if exts.iter().any(|x| name.ends_with(x)) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root().join(dir), exts, &mut out);
    out.sort();
    out
}

fn read(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).to_string()
}

fn count(pattern: &str, list: &[PathBuf]) -> usize {
    let rx = Regex::new(pattern).expect("pattern");
    list.iter().map(|f| rx.find_iter(&read(f)).count()).sum()
}

/// The e2e cases: each `run_test NAME` in a story, or 1 for a story without them.
fn e2e_count() -> usize {
    files("tests/e2e", &[".sh"])
        .iter()
        .filter(|f| {
            f.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("us-"))
        })
        .map(|f| count(r"(?m)^\s*run_test ", std::slice::from_ref(f)).max(1))
        .sum()
}

fn knowledge() -> Result<(i64, i64), String> {
    let out = std::process::Command::new(root().join("bin/cashcast"))
        .args(["knowledge", "check"])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let num = |p: &str| {
        Regex::new(p)
            .ok()
            .and_then(|r| r.captures(&text))
            .and_then(|c| c[1].parse::<i64>().ok())
    };
    match (
        num(r"facts with a source: (\d+)"),
        num(r"Stale files \(past review_by\): (\d+)"),
    ) {
        (Some(f), Some(s)) => Ok((f, s)),
        _ => Err("proof: `cashcast knowledge check` printed no counts".into()),
    }
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

fn round(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().unwrap_or(x)
}

fn events_read(path: &Path) -> Vec<Value> {
    read(path)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Per-day share of gate steps that passed (cached steps left out), and the median e2e step time.
fn gate(events: &Path) -> (Vec<(String, f64)>, Option<f64>) {
    let mut days: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut e2e = Vec::new();
    for ev in events_read(events) {
        if ev["event"] != "check.step" || ev["cached"].as_bool() == Some(true) {
            continue;
        }
        let day: String = ev["ts"].as_str().unwrap_or("").chars().take(10).collect();
        let ok = ev["ok"].as_bool() == Some(true);
        let d = days.entry(day).or_default();
        d.0 += usize::from(ok);
        d.1 += 1;
        if ev["name"] == "e2e" && ok {
            e2e.push(ev["seconds"].as_f64().unwrap_or(0.0));
        }
    }
    let rates = days
        .into_iter()
        .map(|(d, (ok, n))| (d, round(100.0 * ok as f64 / n as f64, 1)))
        .collect();
    (rates, median(e2e))
}

fn put(
    m: &mut Map<String, Value>,
    key: &str,
    value: Value,
    unit: &str,
    date: &str,
    kind: &str,
    command: &str,
    history: Option<Vec<Value>>,
) {
    let entry = m.entry(key).or_insert_with(|| json!({}));
    let hist = match history {
        Some(h) => h,
        None => {
            let mut h: Vec<Value> = entry["history"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|h| h["date"] != date)
                .collect();
            h.push(json!({"date": date, "value": value}));
            h.sort_by(|a, b| a["date"].as_str().cmp(&b["date"].as_str()));
            h
        }
    };
    let o = entry.as_object_mut().expect("metric");
    o.insert("value".into(), value);
    o.insert("unit".into(), json!(unit));
    o.insert("date".into(), json!(date));
    o.insert("kind".into(), json!(kind));
    o.insert("command".into(), json!(command));
    o.insert("history".into(), Value::Array(hist));
}

fn load() -> Result<Value, String> {
    let text = std::fs::read_to_string(metrics_path()).map_err(|e| format!("proof: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("proof: {e}"))
}

fn save(data: &Value) -> Result<(), String> {
    let p = metrics_path();
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())? + "\n";
    std::fs::write(p, text).map_err(|e| e.to_string())
}

pub fn measure(events: &Path) -> Result<Value, String> {
    let mut data = load().unwrap_or_else(|_| json!({"metrics": {}}));
    let today = chrono::Local::now().date_naive().to_string();
    let kind = format!("local run, {today}, {}", machine());
    let m = data["metrics"].as_object_mut().ok_or("proof: no metrics")?;
    // The forecast moved into the engine with the rest of the Python code.
    m.remove("forecast_tests");
    let rs = [".rs"];
    let p = |m: &mut Map<String, Value>, k: &str, v: Value, unit: &str, cmd: &str| {
        put(m, k, v, unit, &today, &kind, cmd, None)
    };
    p(
        m,
        "unit_tests",
        json!(count(r"#\[test\]", &files("engine/src", &rs))),
        "tests",
        "count of #[test] in engine/src",
    );
    p(
        m,
        "e2e_tests",
        json!(e2e_count()),
        "tests",
        "run_test cases in tests/e2e/us-*.sh",
    );
    p(
        m,
        "engine_tests",
        json!(count(r"#\[test\]", &files("engine/tests", &rs))),
        "tests",
        "count of #[test] in engine/tests",
    );
    p(
        m,
        "web_tests",
        json!(count(
            r"(?m)^\s*(?:it|test)\(",
            &files("web/src", &[".ts", ".tsx"])
        )),
        "tests",
        "count of it(/test( in web/src",
    );
    let mut planted = files("tests", &[".sh"]);
    planted.extend(files("engine/tests", &rs));
    planted.extend(files("engine/src", &rs));
    planted.extend(files("web/src", &[".ts", ".tsx"]));
    let rx = Regex::new("(?i)canary").unwrap();
    let canary = planted.iter().filter(|f| rx.is_match(&read(f))).count();
    p(
        m,
        "leak_canary_files",
        json!(canary),
        "test files",
        "files that plant canary strings",
    );
    let (facts, stale) = knowledge()?;
    p(
        m,
        "sourced_facts",
        json!(facts),
        "facts",
        "cashcast knowledge check",
    );
    p(
        m,
        "stale_fact_files",
        json!(stale),
        "files",
        "cashcast knowledge check",
    );
    let connectors = files("connectors", &[".toml"])
        .iter()
        .filter(|f| f.parent() == Some(root().join("connectors").as_path()))
        .count();
    p(
        m,
        "connectors",
        json!(connectors),
        "services",
        "ls connectors/*.toml",
    );
    let (rates, e2e_median) = gate(events);
    if let Some((last, rate)) = rates.last().cloned() {
        let gkind = format!("local run, {last}, {}", machine());
        let hist = rates
            .iter()
            .map(|(d, v)| json!({"date": d, "value": v}))
            .collect();
        let cmd = "scripts/check.sh steps in .metrics/events.jsonl";
        put(
            m,
            "gate_step_pass_rate",
            json!(rate),
            "%",
            &last,
            &gkind,
            cmd,
            Some(hist),
        );
        if let Some(med) = e2e_median {
            put(
                m,
                "e2e_step_median_seconds",
                json!(med),
                "s",
                &last,
                &gkind,
                "median passing e2e step in .metrics/events.jsonl",
                None,
            );
        }
    }
    save(&data)?;
    Ok(data)
}

pub fn problems(data: &Value) -> Vec<String> {
    let kind_rx = Regex::new(r"^(real use|local run, \d{4}-\d{2}-\d{2}, .+)$").unwrap();
    let mut out = Vec::new();
    for (key, m) in data["metrics"].as_object().into_iter().flatten() {
        for field in ["value", "unit", "date", "kind", "command"] {
            if matches!(&m[field], Value::Null) || m[field] == "" {
                out.push(format!("{key}: no {field}"));
            }
        }
        if let Some(k) = m["kind"].as_str().filter(|k| !k.is_empty())
            && !kind_rx.is_match(k)
        {
            out.push(format!(
                "{key}: kind must be `real use` or `local run, YYYY-MM-DD, machine`"
            ));
        }
    }
    out
}

/// A salted hash per machine: a random salt kept in private/, never a host or user name.
fn machine_label(private: &Path) -> Result<String, String> {
    let salt = private.join("proof/salt");
    if !salt.is_file() {
        std::fs::create_dir_all(salt.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut bytes = [0u8; 16];
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|e| e.to_string())?;
        std::fs::write(&salt, crate::digest::hex(&bytes)).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&salt, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
    }
    let text = std::fs::read_to_string(&salt).map_err(|e| e.to_string())?;
    Ok(format!(
        "m-{}",
        &crate::digest::hex(&crate::digest::sha256(text.as_bytes()))[..8]
    ))
}

fn int(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .unwrap_or(0)
}

/// Totals from this machine's content-free app logs (private/users/*/logs). Counts only.
pub fn field(private: &Path) -> Result<Value, String> {
    let (mut runs, mut ok, mut errors, mut done, mut rows, mut checked, mut misses) =
        (0i64, 0i64, 0usize, 0i64, 0i64, 0i64, 0i64);
    let mut ratios = Vec::new();
    let mut ms = Vec::new();
    let mut first: Option<String> = None;
    let mut homes: Vec<PathBuf> = std::fs::read_dir(private.join("users"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path().join("logs"))
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    homes.sort();
    for logs in homes {
        let mut scored: Option<Value> = None;
        let mut ratio: Option<f64> = None;
        for ev in events_read(&logs.join("events.jsonl")) {
            let ts = ev["ts"].as_f64().unwrap_or(0.0);
            let day = chrono::DateTime::from_timestamp(ts as i64, 0)
                .map_or(String::new(), |d| d.date_naive().to_string());
            first = Some(match first {
                Some(f) if f <= day => f,
                _ => day,
            });
            match ev["event"].as_str() {
                Some("command") => {
                    runs += 1;
                    ok += i64::from(ev["result"] == "ok");
                    if let Some(n) = ev["ms"].as_i64() {
                        ms.push(n as f64);
                    }
                }
                Some("sync") => done += int(&ev["done"]),
                Some("moneywiz_sync") if ev["result"] == "ok" => rows += int(&ev["sent"]),
                Some("forecast_scored") => scored = Some(ev.clone()),
                Some("cashback_scored") => {
                    if let Some(r) = ev["ratio"].as_f64() {
                        ratio = Some(r);
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = scored {
            checked += int(&s["checked"]);
            misses += int(&s["misses"]);
        }
        ratios.extend(ratio);
        errors += events_read(&logs.join("errors.jsonl")).len();
    }
    Ok(json!({
        "machine": machine_label(private)?,
        "first_day": first,
        "date": chrono::Local::now().date_naive().to_string(),
        "runs": runs,
        "ok_runs": ok,
        "errors": errors,
        "median_run_ms": median(ms).map(|m| m as i64),
        "tasks_done_elsewhere": done,
        "moneywiz_rows_added": rows,
        "forecast_checked": checked,
        "forecast_misses": misses,
        "cashback_ratio": median(ratios).map(|r| round(r, 2)),
    }))
}

pub fn field_rows(data: &Value) -> Vec<[String; 5]> {
    let machines: Vec<&Value> = data["field"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter(|f| int(&f["runs"]) != 0)
        .collect();
    let any = !machines.is_empty();
    let sum = |k: &str| machines.iter().map(|f| int(&f[k])).sum::<i64>();
    let runs = sum("runs");
    let since = format!("not measured yet (field recording on since {FIELD_SINCE})");
    let src = if any {
        format!(
            "`docs/proof/metrics.json` `field`; real use, {} machine(s)",
            machines.len()
        )
    } else {
        since.clone()
    };
    let cmd = " (`cashcast-engine proof field`)";
    let res = |t: String| if any { t } else { since.clone() };
    let med: Vec<f64> = machines
        .iter()
        .filter_map(|f| f["median_run_ms"].as_f64())
        .collect();
    let checked = sum("forecast_checked");
    let missed = sum("forecast_misses");
    let ratios: Vec<f64> = machines
        .iter()
        .filter_map(|f| f["cashback_ratio"].as_f64())
        .collect();
    let later = format!("not measured yet: task `{TASK}` in `TASKS.md`");
    let s = String::from;
    vec![
        [
            s("Real runs finish without an error."),
            s(
                "A tool people trust is one whose real runs succeed (DORA change-failure rate, Forsgren et al. 2018).",
            ),
            format!("Share of real `bin/cashcast` runs with result ok{cmd}"),
            res(format!(
                "{:.1} % of {runs} runs",
                100.0 * sum("ok_runs") as f64 / runs.max(1) as f64
            )),
            src.clone(),
        ],
        [
            s("Errors are rare in real use."),
            s("Each caught error is logged with the story it breaks, so the rate is countable."),
            format!("Logged errors per 100 real runs{cmd}"),
            res(format!(
                "{:.1} per 100 runs",
                100.0 * sum("errors") as f64 / runs.max(1) as f64
            )),
            src.clone(),
        ],
        [
            s("A run takes seconds, not a session."),
            s("Fast answers get used; slow ones get skipped."),
            format!("Median real run time (median of each machine's median){cmd}"),
            match median(med.clone()) {
                Some(m) => res(format!("{:.1} s", m / 1000.0)),
                None => since.clone(),
            },
            if med.is_empty() {
                since.clone()
            } else {
                src.clone()
            },
        ],
        [
            s("Tasks stay done everywhere."),
            s(
                "A tick in Reminders, Calendar or Notion closes the task in the others, so you tick once.",
            ),
            format!("Tasks ticked in one place and closed by `sync all`{cmd}"),
            res(format!("{} tasks", sum("tasks_done_elsewhere"))),
            src.clone(),
        ],
        [
            s("You record only cash in MoneyWiz."),
            s("Rows cashcast adds are rows you do not type."),
            format!("Bank and card rows `moneywiz sync` added{cmd}"),
            res(format!("{} rows", sum("moneywiz_rows_added"))),
            src.clone(),
        ],
        [
            s("Spending forecasts get better."),
            s(
                "The monthly review compares each prediction with what you spent; a miss is more than 15 % off.",
            ),
            format!("Share of scored months the forecast missed (`bin/cashcast review`){cmd}"),
            if checked > 0 {
                format!(
                    "{:.1} % of {checked} months missed by more than 15 %",
                    100.0 * missed as f64 / checked as f64
                )
            } else {
                s("not measured yet")
            },
            if checked > 0 {
                src.clone()
            } else {
                later.clone()
            },
        ],
        [
            s("Card picks earn more cash back."),
            s(
                "The cash-back plan routes each kind of spending to the best card; one card for everything is the baseline.",
            ),
            format!(
                "Net cash back of the plan vs. your best single card, on your real spending (`bin/cashcast strategy`){cmd}"
            ),
            match median(ratios) {
                Some(r) => format!("{r:.2} × the cash back of your best single card"),
                None => s("not measured yet"),
            },
            if any && machines.iter().any(|f| f["cashback_ratio"].is_number()) {
                src
            } else {
                later
            },
        ],
    ]
}

fn cell_g(v: &Value) -> String {
    match v {
        Value::Number(n) if n.is_f64() => fmt_g(n.as_f64().unwrap(), 6, false),
        other => other.to_string(),
    }
}

pub fn render(data: &Value) -> String {
    let m = &data["metrics"];
    let total: i64 = ["unit_tests", "e2e_tests", "engine_tests", "web_tests"]
        .iter()
        .map(|k| int(&m[k]["value"]))
        .sum();
    let machine_of = m["gate_step_pass_rate"]["kind"]
        .as_str()
        .unwrap_or("")
        .splitn(3, ", ")
        .nth(2)
        .unwrap_or("")
        .to_string();
    let hist = m["gate_step_pass_rate"]["history"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut lines: Vec<String> = [
        "## Proof",
        "",
        "Each claim, why it should hold, and what real use shows. Results come from",
        "content-free usage events each real run writes on its own machine",
        "(command, ok or error, time, counts). Each machine shows as a salted hash.",
        "No aggregate leaves a machine yet; until one is added here, a row says so.",
        "",
        "| Claim | Theory | Metric | Result | Source |",
        "|---|---|---|---|---|",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    lines.extend(
        field_rows(data)
            .iter()
            .map(|r| format!("| {} |", r.join(" | "))),
    );
    lines.push(String::new());
    lines.push(format!(
        "**Engineering health** (not proof of outcomes; {}): {total} tests, {} leak-canary test files, {} sourced facts ({} stale files), median e2e step {} s. `{MEASURE}` refreshes it.",
        m["unit_tests"]["kind"].as_str().unwrap_or(""),
        m["leak_canary_files"]["value"],
        m["sourced_facts"]["value"],
        m["stale_fact_files"]["value"],
        cell_g(&m["e2e_step_median_seconds"]["value"]),
    ));
    lines.push(String::new());
    lines.push(format!(
        "Gate steps that passed, per day (local runs on {machine_of}; cached steps left out):"
    ));
    lines.push(String::new());
    lines.push("```mermaid".into());
    lines.push("xychart-beta".into());
    lines.push("  title \"Gate steps passed (%)\"".into());
    let days: Vec<String> = hist
        .iter()
        .map(|h| {
            format!(
                "\"{}\"",
                h["date"].as_str().unwrap_or("").get(5..).unwrap_or("")
            )
        })
        .collect();
    lines.push(format!("  x-axis [{}]", days.join(", ")));
    lines.push("  y-axis \"%\" 0 --> 100".into());
    let bars: Vec<String> = hist
        .iter()
        .map(|h| fmt_g(h["value"].as_f64().unwrap_or(0.0), 6, false))
        .collect();
    lines.push(format!("  bar [{}]", bars.join(", ")));
    lines.push("```".into());
    lines.join("\n") + "\n"
}

pub fn splice(readme: &str, section: &str) -> Option<String> {
    let a = readme.find(START)? + START.len();
    let b = readme.find(END)?;
    Some(format!("{}\n{}{}", &readme[..a], section, &readme[b..]))
}

/// `cashcast-engine proof MODE [--events PATH] [--private PATH]`. Returns the exit code.
pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let usage = "usage: cashcast-engine proof {measure,field,render,check} [--events PATH] [--private PATH]";
    let mode = args.first().map(String::as_str).ok_or(usage)?;
    let flag = |name: &str| {
        args.windows(2)
            .find(|w| w[0] == name)
            .map(|w| PathBuf::from(&w[1]))
    };
    let readme_path = root().join("README.md");
    if mode == "field" {
        let agg = field(&flag("--private").unwrap_or_else(|| root().join("private")))?;
        let mut data = load()?;
        let label = agg["machine"].as_str().unwrap_or("").to_string();
        let runs = agg["runs"].clone();
        data.as_object_mut()
            .ok_or("proof: bad metrics file")?
            .entry("field")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("proof: bad field")?
            .insert(label.clone(), agg);
        save(&data)?;
        println!("proof: field totals for {label}: {runs} runs (local only; nothing uploaded)");
    }
    let data = match mode {
        "measure" => {
            measure(&flag("--events").unwrap_or_else(|| root().join(".metrics/events.jsonl")))?
        }
        "field" | "render" | "check" => load()?,
        _ => return Err(usage.into()),
    };
    let bad = problems(&data);
    if !bad.is_empty() {
        eprintln!("{}", bad.join("\n"));
        return Ok(1);
    }
    let readme = std::fs::read_to_string(&readme_path).map_err(|e| e.to_string())?;
    let want = splice(&readme, &render(&data)).ok_or("proof: README has no proof markers")?;
    if mode == "check" {
        if want != readme {
            eprintln!(
                "proof: README Proof section is stale; run `engine/target/release/cashcast-engine proof render`"
            );
            return Ok(1);
        }
        return Ok(0);
    }
    std::fs::write(&readme_path, want).map_err(|e| e.to_string())?;
    println!(
        "proof: {} metrics -> docs/proof/metrics.json, README Proof section written",
        data["metrics"].as_object().map_or(0, Map::len)
    );
    Ok(0)
}
