//! Runs every section and writes the full strategy document, as
//! `strategy/plan.py` (with `journal.record`).
//!
//! Log events wait until every section is built: a section that cannot read
//! its input (`Unported`) stops the whole command (exit 1) with no half log.

use super::py::{self};
use super::situation::{Situation, Unported};
use super::{Ctx, DISCLAIMER, Data, Event, PORTED, Section, Task};
use crate::core::events;
use crate::core::paths::UserHome;
use crate::core::pyjson::{self, Json};
use chrono::{Datelike, Duration, NaiveDate};

/// Every section, in registry order, and the events to log once they all ran.
pub fn run(user: &UserHome, today: NaiveDate) -> Result<(Ctx, Vec<Event>), Unported> {
    let sit = Situation::load(&user.home, &user.name, today)?;
    let mut ctx = Ctx {
        sit,
        results: Vec::new(),
        events: Vec::new(),
    };
    let mut log: Vec<Event> = Vec::new();
    for (id, build) in PORTED {
        let sec = build(&mut ctx)?;
        ctx.results.push(sec);
        log.append(&mut ctx.events);
        log.push((
            "section_built".into(),
            vec![("section".into(), Json::Str(id.to_string()))],
        ));
    }
    Ok((ctx, log))
}

/// Logs the buffered events, then records next month's spending prediction.
pub fn finish(user: &UserHome, ctx: &Ctx, log: &[Event]) -> Result<(), Unported> {
    for (name, fields) in log {
        let f: Vec<(&str, Json)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        events::log(user, name, &f);
    }
    if let Some(Data::Forecast(fc)) = ctx.result("forecast").map(|s| &s.data)
        && let Some(first) = fc.first_month
    {
        record(
            user,
            &first.format("%Y-%m").to_string(),
            fc.next_month_spending,
            ctx.sit.today,
        )?;
    }
    Ok(())
}

/// `journal.record`: one spending prediction per month, appended once.
fn record(user: &UserHome, month: &str, predicted: f64, today: NaiveDate) -> Result<(), Unported> {
    let data = user.home.join("data");
    let path = data.join("journal.jsonl");
    let err = |e: std::io::Error| Unported(format!("journal: {e}"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(err(e)),
    };
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let e = pyjson::loads(line).ok_or_else(|| Unported("journal line is not JSON".into()))?;
        let is = |k: &str, v: &str| e.get(k).is_some_and(|x| x.py_str() == v);
        if is("month", month) && is("metric", "spending") {
            return Ok(());
        }
    }
    std::fs::create_dir_all(&data).map_err(err)?;
    let entry = Json::Obj(vec![
        ("made_on".into(), Json::Str(today.to_string())),
        ("metric".into(), Json::Str("spending".into())),
        ("month".into(), Json::Str(month.into())),
        ("predicted".into(), Json::Float(py::round(predicted, 2))),
    ]);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .map_err(err)?;
    writeln!(f, "{}", pyjson::dumps(&entry, None, false)).map_err(err)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(err)?;
    Ok(())
}

/// The review's tasks, each id once, plus the owner's own `[[tasks]]`.
pub fn tasks(ctx: &Ctx) -> Result<Vec<Task>, Unported> {
    let mut out: Vec<Task> = Vec::new();
    if let Some(review) = ctx.result("review") {
        for t in &review.tasks {
            if !out.iter().any(|o| o.id == t.id) {
                out.push(t.clone());
            }
        }
    }
    with_own(ctx, out)
}

fn day_of(v: Option<&Json>) -> Option<NaiveDate> {
    let v = v.filter(|v| v.truthy())?;
    NaiveDate::parse_from_str(&v.py_str().chars().take(10).collect::<String>(), "%Y-%m-%d").ok()
}

/// The k-th occurrence after `first`; months and years keep its day, clamped to the month's end.
fn nth(first: NaiveDate, every: &str, k: i64) -> Option<NaiveDate> {
    match every {
        "week" | "2 weeks" => {
            Some(first + Duration::days(k * if every == "week" { 7 } else { 14 }))
        }
        "month" | "year" => {
            let months = first.month0() as i64 + if every == "month" { k } else { 12 * k };
            let (y, m) = (
                first.year() + (months / 12) as i32,
                (months % 12) as u32 + 1,
            );
            let next = if m == 12 {
                NaiveDate::from_ymd_opt(y + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(y, m + 1, 1)
            }?;
            let last = (next - Duration::days(1)).day();
            NaiveDate::from_ymd_opt(y, m, first.day().min(last))
        }
        _ => None,
    }
}

fn done_ids(user_home: &std::path::Path) -> Result<Vec<String>, Unported> {
    let path = user_home.join("data").join("changes-done.json");
    match std::fs::read_to_string(&path) {
        Ok(t) => match pyjson::loads(&t) {
            Some(Json::Obj(items)) => Ok(items.into_iter().map(|(k, _)| k).collect()),
            _ => Err(Unported("changes-done.json is not an object".into())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Unported(format!("changes-done.json: {e}"))),
    }
}

/// The owner's own finance tasks (`[[tasks]]` in profile.toml) join the plan. An entry with a
/// cashcast task's id only changes that task's title, date or notes. An own task once done stays done.
fn with_own(ctx: &Ctx, mut generated: Vec<Task>) -> Result<Vec<Task>, Unported> {
    let own: Vec<&Json> = py::list(&ctx.sit.profile, "tasks")
        .iter()
        .filter(|e| matches!(e, Json::Obj(_)) && e.get("id").is_some_and(Json::truthy))
        .collect();
    if own.is_empty() {
        return Ok(generated);
    }
    let finished = done_ids(&ctx.sit.home)?;
    let mut extra = Vec::new();
    let text = |e: &Json, k: &str| e.get(k).filter(|v| v.truthy()).map(Json::py_str);
    for e in own {
        let mut tid = e.get("id").map_or(String::new(), Json::py_str);
        let mut due = day_of(e.get("due"));
        if let Some(prefix) = tid.strip_suffix('*') {
            for t in generated.iter_mut().filter(|t| t.id.starts_with(prefix)) {
                t.title = text(e, "title").unwrap_or(t.title.clone());
                if let Some(n) = text(e, "notes") {
                    t.notes = format!("{}\n{n}", t.notes).trim().to_string();
                }
            }
            continue;
        }
        if let Some(t) = generated.iter_mut().find(|t| t.id == tid) {
            t.title = text(e, "title").unwrap_or(t.title.clone());
            t.due = due.or(t.due);
            if let Some(n) = text(e, "notes") {
                t.notes = format!("{}\n{n}", t.notes).trim().to_string();
            }
            continue;
        }
        let every = text(e, "every").unwrap_or_default();
        if let (false, Some(first)) = (every.is_empty(), due) {
            let late = ctx.sit.today - Duration::days(7);
            let mut found = None;
            for k in 0..5000 {
                let Some(d) = nth(first, &every, k) else {
                    break;
                };
                if d >= late && !finished.contains(&format!("{tid}-{d}")) {
                    found = Some(d);
                    break;
                }
            }
            let Some(d) = found else { continue };
            due = Some(d);
            tid = format!("{tid}-{d}");
        }
        if !finished.contains(&tid)
            && let Some(title) = text(e, "title")
        {
            let kind = e
                .get("kind")
                .map(Json::py_str)
                .filter(|k| ["read", "settings", "money"].contains(&k.as_str()));
            let mut t = Task::new(tid, title)
                .notes(&e.get("notes").map_or(String::new(), Json::py_str))
                .risk(&kind.unwrap_or("read".into()))
                .recurs(&every);
            t.due = due;
            extra.push(t);
        }
    }
    generated.extend(extra);
    Ok(generated)
}

pub fn document(ctx: &Ctx, only: &[String]) -> Result<String, Unported> {
    let sit = &ctx.sit;
    let name = sit
        .profile
        .get("display_name")
        .map_or(sit.name.clone(), Json::py_str);
    let mut parts = vec![
        format!("# Money strategy — {name} ({})", sit.province()),
        String::new(),
        format!("Made on {}. *{DISCLAIMER}*", sit.today),
        String::new(),
    ];
    let mut shown: Vec<&Section> = ctx.results.iter().collect();
    shown.sort_by_key(|s| s.id != "net_total");
    for s in shown {
        if only.is_empty() || only.contains(&s.id) || s.id == "review" || s.id == "net_total" {
            parts.push(s.markdown());
        }
    }
    parts.push("## Dated checklist".into());
    parts.push(String::new());
    let mut all = tasks(ctx)?;
    all.sort_by(|a, b| {
        (a.due.unwrap_or(NaiveDate::MAX), &a.id).cmp(&(b.due.unwrap_or(NaiveDate::MAX), &b.id))
    });
    for t in all {
        let who = match t.risk.as_str() {
            "money" => " — you do this (moves money)",
            "settings" => " — approve before any agent does it",
            _ => "",
        };
        let due = t.due.map_or("when known".to_string(), |d| d.to_string());
        parts.push(format!("- [ ] {due}: {}{who}", t.title));
    }
    Ok(parts.join("\n") + "\n")
}

/// `engine holdings`, as `rrsp` and `invest-plan` run it first: refreshes
/// `data/holdings.json`. Errors are ignored, as in Python; the plan still
/// runs on the last saved holdings.
pub fn refresh_holdings(home: &std::path::Path) {
    let cfg = match std::fs::read_to_string(home.join("cashcast.toml")) {
        Ok(text) => crate::config::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(crate::config::Config::default()),
        Err(e) => Err(e.to_string()),
    };
    let Ok(cfg) = cfg else { return };
    let Ok(holdings) = crate::ledger::load_dir(&home.join("inbox"), &cfg) else {
        return;
    };
    let data = home.join("data");
    if std::fs::create_dir_all(&data).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(&holdings) {
        let _ = crate::write_private(&data.join("holdings.json"), &json);
    }
}
