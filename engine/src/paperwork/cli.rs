//! `cashcast paperwork …`: CLI only (the web app lists it as terminal-only).
//! Ported from sidequest's `src/cli/paperwork.ts`, without the browser unlock.
//!
//! Private values (dates, places, notes) are typed at a prompt, never after
//! the command: `ps` shows a command's words to every account on the Mac, and
//! the shell history keeps them. Private verbs and `--reveal` need a terminal
//! and the owner's Touch ID or login password (`presence`). Nothing here logs
//! whether one can apply (threat model T3).

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde_json::{Value, json};

use super::{facts, presence, store};

/// Who runs the command: whether it is a terminal, and how to ask the owner
/// a question (None when input ended).
pub struct Owner<'a> {
    pub terminal: bool,
    pub ask: &'a mut dyn FnMut(&str) -> Option<String>,
}

const USAGE: &str = "usage: cashcast paperwork <list|show|mark|set|trip|untrip|temp|untemp|presence|attach|files|export-file|sources|doctor>";

/// Verbs only the owner runs, in a terminal: an agent that could change dates
/// could probe the redacted view for the private values, and documents never
/// pass through an agent (threat model T4).
pub const OWNER_ONLY: &[&str] = &[
    "set",
    "trip",
    "untrip",
    "temp",
    "untemp",
    "attach",
    "export-file",
];
pub const OWNER_ONLY_REASON: &str =
    "private dates and documents: only you enter them, in a terminal";

/// Entry point: stdout text and exit code. Prompts go to stderr and answers
/// come from stdin.
pub fn run(args: &[String], user_dir: &Path, today: NaiveDate) -> Result<(String, i32), String> {
    let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut ask = |q: &str| -> Option<String> {
        eprint!("{q}: ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    };
    run_as(
        args,
        user_dir,
        today,
        &mut Owner {
            terminal,
            ask: &mut ask,
        },
    )
}

#[derive(Default)]
struct Flags {
    reveal: bool,
    json: bool,
    live: bool,
    sign: bool,
    add_note: bool,
    out: Option<String>,
    words: Vec<String>,
}

fn parse(args: &[String]) -> Result<Flags, String> {
    let mut f = Flags::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--reveal" => f.reveal = true,
            "--json" => f.json = true,
            "--live" => f.live = true,
            "--sign" => f.sign = true,
            "--add-note" => f.add_note = true,
            "--out" => f.out = Some(it.next().cloned().ok_or("--out needs a path")?),
            x if x.starts_with("--out=") => f.out = Some(x["--out=".len()..].to_string()),
            x if x.starts_with("--") => return Err(format!("unknown option {x}")),
            _ => f.words.push(a.clone()),
        }
    }
    Ok(f)
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn out(f: &Flags, v: Value, text: String) -> String {
    if f.json { pretty(&v) } else { text }
}

pub fn run_as(
    args: &[String],
    user_dir: &Path,
    today: NaiveDate,
    owner: &mut Owner,
) -> Result<(String, i32), String> {
    let f = parse(args)?;
    let verb = f.words.first().map(String::as_str).unwrap_or("list");
    let id = f.words.get(1).map(String::as_str);
    let rest: Vec<&str> = f.words.iter().skip(2).map(String::as_str).collect();
    let terminal = owner.terminal;
    if f.reveal && !terminal {
        return Ok((
            "cashcast: --reveal works only in a terminal: dates, notes and files never go to agents or pipes".into(),
            2,
        ));
    }
    let owner_only = OWNER_ONLY.contains(&verb);
    if !terminal && owner_only {
        return Ok((
            format!("cashcast: {OWNER_ONLY_REASON}; agents and pipes cannot"),
            2,
        ));
    }
    if !terminal && (f.sign || f.add_note) {
        return Ok((
            "cashcast: only you type sign days and notes, in a terminal; agents and pipes cannot"
                .into(),
            2,
        ));
    }
    // A terminal proves nothing: any program can make one with `script`.
    if terminal
        && (f.reveal || owner_only || f.sign || f.add_note)
        && !presence::confirm(&format!(
            "show or change your private paperwork (cashcast paperwork {verb})"
        ))
    {
        return Ok((
            "cashcast: macOS did not confirm you (Touch ID or your login password), so nothing private was shown or changed".into(),
            2,
        ));
    }
    let reveal = terminal && f.reveal;

    match verb {
        "list" => {
            let st = store::load(user_dir)?;
            let mut rows = vec![];
            let mut lines = vec![];
            for p in facts::processes() {
                let ps = st.processes.get(p.id).cloned().unwrap_or_default();
                let v = store::agent_view(&store::view(&p, &ps, today)?);
                let next = v["next"]["title"]
                    .as_str()
                    .map(|t| format!(" · next: {t}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "{}  {}: {}/{} done{next}",
                    v["id"].as_str().unwrap_or(""),
                    p.title,
                    v["done"],
                    v["total"]
                ));
                rows.push(v);
            }
            return Ok((out(&f, Value::Array(rows), lines.join("\n")), 0));
        }
        "sources" => {
            let mut all = vec![];
            for p in facts::processes() {
                for c in store::check_sources(&p, f.live, today, &store::fetch_page) {
                    all.push((p.id, c));
                }
            }
            // Offline it only reports; live, a page that no longer says what we cite fails.
            let bad = all.iter().any(|(_, c)| c.status == "changed");
            let rows: Vec<Value> = all
                .iter()
                .map(|(pid, c)| {
                    let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
                    v["process"] = json!(pid);
                    v
                })
                .collect();
            let mut lines: Vec<String> = all
                .iter()
                .map(|(_, c)| {
                    format!(
                        "{:<11} {}\n            {}",
                        c.status.to_uppercase(),
                        c.url,
                        c.detail
                    )
                })
                .collect();
            lines.push(if f.live {
                "facts: compare any CHANGED or UNREACHABLE page by hand, and look up each MANUAL fact (a PDF), then update engine/src/paperwork/facts.rs".into()
            } else {
                "facts: run with --live to re-read every official page".into()
            });
            return Ok((
                out(&f, Value::Array(rows), lines.join("\n")),
                if bad { 1 } else { 0 },
            ));
        }
        "doctor" => {
            let found = store::doctor(user_dir);
            let text = found
                .iter()
                .map(|(l, m)| format!("{l:<5} {m}"))
                .collect::<Vec<_>>()
                .join("\n");
            let fail = found.iter().any(|(l, _)| *l == "FAIL");
            return Ok((text, if fail { 1 } else { 0 }));
        }
        "steps" => {
            // The public checklist: no state is read.
            let p = store::find(id.unwrap_or(facts::CITIZENSHIP_ID))?;
            let mut lines = vec![p.title.to_string()];
            for st in &p.steps {
                lines.push(format!("\n{}", st.title));
                for it in &st.items {
                    lines.push(format!(
                        "  {}: {}{}",
                        it.id,
                        it.title,
                        if it.owner_only { " [only you]" } else { "" }
                    ));
                }
            }
            return Ok((
                out(
                    &f,
                    serde_json::to_value(&p).unwrap_or(Value::Null),
                    lines.join("\n"),
                ),
                0,
            ));
        }
        _ => {}
    }
    let known = [
        "show",
        "mark",
        "set",
        "trip",
        "untrip",
        "temp",
        "untemp",
        "presence",
        "attach",
        "files",
        "export-file",
    ];
    if !known.contains(&verb) {
        return Ok((USAGE.into(), 2));
    }
    let Some(id) = id else {
        let ids: Vec<&str> = facts::processes().iter().map(|p| p.id).collect();
        return Ok((
            format!(
                "usage: cashcast paperwork {verb} <process> … (processes: {})",
                ids.join(", ")
            ),
            2,
        ));
    };
    let p = store::find(id)?;
    let pid = p.id;
    // Dates, places and notes are typed at a prompt, never after the command.
    let typed_after = |words: &[&str]| -> Option<(String, i32)> {
        if words.is_empty() {
            return None;
        }
        let first = rest.first().copied().unwrap_or("");
        let second = rest.get(1).copied().unwrap_or("");
        let how = match verb {
            "set" => format!("set {pid} {first}"),
            "mark" => format!("mark {pid} {first} {second} --add-note"),
            "presence" => format!("presence {pid} --sign"),
            _ => format!("{verb} {pid}"),
        };
        Some((
            format!(
                "cashcast: type private values at the prompt, not after the command: `ps` shows a command's words to every account on this Mac, and your shell history keeps them (delete that line from it). Run `cashcast paperwork {how}` and answer the questions."
            ),
            2,
        ))
    };
    let mut ask = |questions: &[&str]| -> Result<Vec<String>, String> {
        questions
            .iter()
            .map(|q| (owner.ask)(q).ok_or_else(|| "no answer; nothing was changed".to_string()))
            .collect()
    };
    let trip_q = [
        "Day you left (YYYY-MM-DD)",
        "Day you came back (YYYY-MM-DD)",
    ];
    let period_q = ["Status start (YYYY-MM-DD)", "Status end (YYYY-MM-DD)"];

    match verb {
        "show" => {
            let st = store::load(user_dir)?;
            let ps = st.processes.get(pid).cloned().unwrap_or_default();
            let v = store::view(&p, &ps, today)?;
            if f.json {
                let j = if reveal {
                    serde_json::to_value(&v).unwrap_or(Value::Null)
                } else {
                    store::agent_view(&v)
                };
                return Ok((pretty(&j), 0));
            }
            let mut lines = vec![format!("{}: {}/{} done", v.title, v.done, v.total)];
            for st in &v.steps {
                lines.push(format!(
                    "\n{} {} ({}/{})",
                    if st.status == "now" { "▶" } else { " " },
                    st.title,
                    st.done,
                    st.total
                ));
                for it in &st.items {
                    let sym = match it.status {
                        "done" => "✓",
                        "na" => "–",
                        _ => "○",
                    };
                    let mut line = format!("   {sym} {}: {}", it.item.id, it.item.title);
                    if it.item.owner_only {
                        line.push_str(" [only you]");
                    }
                    if it.applies == store::MAYBE && it.status == "todo" {
                        line.push_str(" (if it applies)");
                    }
                    if reveal && let Some(w) = &it.why {
                        line.push_str(&format!(" · {w}"));
                    }
                    if reveal && let Some(n) = &it.note {
                        line.push_str(&format!(" · note: {n}"));
                    }
                    if it.files > 0 {
                        line.push_str(&format!(
                            " · {} file{}",
                            it.files,
                            if it.files == 1 { "" } else { "s" }
                        ));
                    }
                    lines.push(line);
                }
            }
            if let Some(n) = &v.next {
                let s = n.item.sources[0];
                lines.push(format!(
                    "\nnext: {}{}\n  {}\n  source: {} (checked {})",
                    n.item.title,
                    if n.item.owner_only {
                        " (only you do this)"
                    } else {
                        ""
                    },
                    n.item.how,
                    s.url,
                    s.checked
                ));
            }
            if reveal {
                for d in &v.deadlines {
                    lines.push(format!("deadline: {} {}", d.what, d.date));
                }
            } else {
                lines.push(
                    "\nprivate: dates, notes and files show with --reveal in a terminal".into(),
                );
            }
            Ok((lines.join("\n"), 0))
        }
        "mark" => {
            let (Some(item), Some(state)) = (rest.first().copied(), rest.get(1).copied()) else {
                return Ok((
                    format!(
                        "usage: cashcast paperwork mark {pid} <item> <todo|done|na> [--add-note]"
                    ),
                    2,
                ));
            };
            if let Some(r) = typed_after(&rest[2..]) {
                return Ok(r);
            }
            let note = if f.add_note {
                ask(&["Note (encrypted; empty keeps the old one)"])?.remove(0)
            } else {
                String::new()
            };
            let note = (!note.is_empty()).then_some(note);
            store::update(user_dir, pid, |ps, p| {
                store::mark(ps, p, item, state, note.as_deref(), today)
            })?;
            Ok((format!("marked {item} {state}"), 0))
        }
        "set" => {
            let key = rest.first().copied().unwrap_or("");
            if let Some(r) = typed_after(rest.get(1..).unwrap_or(&[])) {
                return Ok(r);
            }
            let Some(help) = store::field_help(key) else {
                let names: Vec<&str> = store::FIELDS.iter().map(|(k, _)| *k).collect();
                return Err(format!(
                    "unknown field '{key}'; fields: {}",
                    names.join(", ")
                ));
            };
            let value = ask(&[&format!("{key}, {help}; empty clears it")])?.remove(0);
            store::update(user_dir, pid, |ps, _| store::set_field(ps, key, &value))?;
            Ok((
                if value.is_empty() {
                    format!("cleared {key}")
                } else {
                    format!("saved {key} (encrypted)")
                },
                0,
            ))
        }
        "trip" => {
            if let Some(r) = typed_after(&rest) {
                return Ok(r);
            }
            let a = ask(&[trip_q[0], trip_q[1], "Country (optional)"])?;
            let n = store::update(user_dir, pid, |ps, _| {
                store::add_trip(ps, &a[0], &a[1], &a[2])?;
                Ok(ps.trips.len())
            })?;
            Ok((
                format!(
                    "saved the trip (encrypted); {n} trip{}",
                    if n == 1 { "" } else { "s" }
                ),
                0,
            ))
        }
        "untrip" => {
            if let Some(r) = typed_after(&rest) {
                return Ok(r);
            }
            let a = ask(&[
                trip_q[0],
                trip_q[1],
                "Country (empty when only one trip has these dates; - for a trip with none)",
            ])?;
            // Empty means any trip with these dates; "-" means the one with no country.
            let place = match a[2].as_str() {
                "-" => Some(""),
                "" => None,
                w => Some(w),
            };
            let gone = store::update(user_dir, pid, |ps, _| {
                store::remove_trip_on(ps, &a[0], &a[1], place)
            })?;
            Ok(if gone {
                ("removed the trip".into(), 0)
            } else {
                ("no trip has these dates; nothing changed".into(), 1)
            })
        }
        "temp" => {
            if let Some(r) = typed_after(&rest) {
                return Ok(r);
            }
            let a = ask(&period_q)?;
            store::update(user_dir, pid, |ps, _| {
                store::add_temp_period(ps, &a[0], &a[1])
            })?;
            Ok(("saved the period (encrypted)".into(), 0))
        }
        "untemp" => {
            if let Some(r) = typed_after(&rest) {
                return Ok(r);
            }
            let a = ask(&period_q)?;
            let gone = store::update(user_dir, pid, |ps, _| {
                Ok(store::remove_temp_on(ps, &a[0], &a[1]))
            })?;
            Ok(if gone {
                ("removed the period".into(), 0)
            } else {
                ("no period has these dates; nothing changed".into(), 1)
            })
        }
        "presence" => {
            if let Some(r) = typed_after(&rest) {
                return Ok(r);
            }
            let sign = if f.sign {
                ask(&["Sign day (YYYY-MM-DD)"])?.remove(0)
            } else {
                String::new()
            };
            let st = store::load(user_dir)?;
            let ps = st.processes.get(pid).cloned().unwrap_or_default();
            let Some(r) =
                store::presence(&p, &ps, (!sign.is_empty()).then_some(sign.as_str()), today)?
            else {
                return Ok((
                    format!(
                        "enter the day you became a PR first: run cashcast paperwork set {pid} pr_date in a terminal"
                    ),
                    1,
                ));
            };
            let code = if r.met { 0 } else { 1 };
            // Met or not, never how many: with today's date, a count gives away the PR date (T4).
            if !reveal {
                let text = if r.met {
                    format!("presence: enough days ({} needed)", r.needed)
                } else {
                    format!(
                        "presence: not enough days yet ({} needed); the count and the earliest sign day show with --reveal in a terminal",
                        r.needed
                    )
                };
                return Ok((
                    out(&f, json!({"met": r.met, "needed": r.needed}), text),
                    code,
                ));
            }
            let text = [
                format!(
                    "window {} to {} (sign day {})",
                    r.window.from, r.window.to, r.sign
                ),
                format!(
                    "as PR: {} days · before PR: {} days = {} credit · away: {} full days",
                    r.pr_days, r.temp_days, r.temp_credit, r.absent_days
                ),
                // Not "1293 of 1095": that reads like more than 100%.
                format!(
                    "total: {} days · {} needed · {}",
                    r.total,
                    r.needed,
                    if r.met {
                        "enough".to_string()
                    } else {
                        format!("{} short", r.short_by)
                    }
                ),
                format!(
                    "earliest sign day: {}",
                    r.earliest.as_deref().unwrap_or("?")
                ),
            ]
            .join("\n");
            Ok((
                out(&f, serde_json::to_value(&r).unwrap_or(Value::Null), text),
                code,
            ))
        }
        "attach" => {
            let (Some(item), Some(file)) = (rest.first().copied(), rest.get(1).copied()) else {
                return Ok((
                    format!("usage: cashcast paperwork attach {pid} <item> <file>"),
                    2,
                ));
            };
            let path = absolute(file);
            let r = store::update(user_dir, pid, |ps, p| {
                store::attach(user_dir, ps, p, item, &path, today)
            })?;
            Ok((
                format!(
                    "saved an encrypted copy ({} bytes, id {}) for {item}.\nThe original is still at {file}: delete it yourself when you no longer need it.",
                    r.bytes, r.id
                ),
                0,
            ))
        }
        "files" => {
            let st = store::load(user_dir)?;
            let ps = st.processes.get(pid).cloned().unwrap_or_default();
            if !reveal {
                let n = ps.files.len();
                return Ok((
                    format!(
                        "{n} encrypted file{}; names show with --reveal in a terminal",
                        if n == 1 { "" } else { "s" }
                    ),
                    0,
                ));
            }
            let lines: Vec<String> = ps
                .files
                .iter()
                .map(|x| {
                    format!(
                        "{}  {}  {} ({} bytes, {})",
                        x.id, x.item, x.name, x.bytes, x.added
                    )
                })
                .collect();
            Ok((lines.join("\n"), 0))
        }
        "export-file" => {
            let target = f.out.clone().ok_or("--out <new path> is required")?;
            let st = store::load(user_dir)?;
            let ps = st.processes.get(pid).cloned().unwrap_or_default();
            store::export_file(
                user_dir,
                &ps,
                rest.first().copied().unwrap_or(""),
                &absolute(&target),
            )?;
            Ok((
                format!("wrote a decrypted copy (mode 600) to {target}; delete it when done"),
                0,
            ))
        }
        _ => Ok((USAGE.into(), 2)),
    }
}

fn absolute(p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}
