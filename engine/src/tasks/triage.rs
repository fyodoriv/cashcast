//! Goes through the open finance tasks: resolves what cashcast can do and
//! names who must do the rest, as `src/cashcast/triage.py`.

use super::{changes, connectors};
use crate::core::events;
use crate::core::paths::{AppError, UserHome};
use crate::core::pyjson::Json;
use crate::strategy::Task;
use crate::strategy::situation::Unported;
use chrono::{Duration, NaiveDate};

const AUTO_WINDOW: i64 = 7;
/// Task id prefix → the report section that does the task.
const RESOLVERS: [(&str, &str); 3] = [
    ("monthly-review-", "review"),
    ("use-cashcast", "review"),
    ("update-priorities-finances-", "review"),
];
const CHECKS: [&str; 6] = ["confirm", "check", "ensure", "verify", "research", "review"];
const REASON_MAX: usize = 120;

pub struct Row {
    pub task: Task,
    pub lane: &'static str,
    pub step: String,
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `^(confirm|check|...)\b`, ignoring case.
fn is_check(title: &str) -> bool {
    let low = title.to_lowercase();
    CHECKS
        .iter()
        .any(|w| low.starts_with(w) && !low[w.len()..].chars().next().is_some_and(word_char))
}

/// The AMOUNT pattern: a currency sign, a decimal, or 3+ digits that are not a 4-digit year.
pub fn has_amount(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let digit = |i: usize| c.get(i).is_some_and(|x| x.is_ascii_digit());
    for i in 0..c.len() {
        if "$€£¥".contains(c[i]) {
            return true;
        }
        if digit(i) && matches!(c.get(i + 1), Some('.') | Some(',')) && digit(i + 2) {
            return true;
        }
    }
    for i in 0..c.len() {
        if !digit(i) || (i > 0 && word_char(c[i - 1])) {
            continue;
        }
        let run = (i..c.len()).take_while(|j| digit(*j)).count();
        if run < 3 {
            continue;
        }
        let year = (c[i] == '1' && c[i + 1] == '9' || c[i] == '2' && c[i + 1] == '0')
            && run >= 4
            && !c.get(i + 4).is_some_and(|x| word_char(*x));
        if !year {
            return true;
        }
    }
    false
}

fn where_(task: &Task, specs: &[(String, Json)]) -> String {
    let service = task
        .title
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .find_map(|w| connectors::find_by_name(specs, w));
    let mut place = "`bin/cashcast payments`, `bin/cashcast status` (re-import fresh exports: `bin/cashcast fetch plan`)".to_string();
    let mut site = "the service's site".to_string();
    if let Some(s) = service {
        place = format!("`bin/cashcast connectors show {s}` (API or export), then {place}");
        site = format!("the {s} site");
    }
    format!(
        "Agent checks it from evidence: {place}. If that cannot show it: after the owner runs \
         `bin/cashcast consent grant agent:browser-read`, read {site} in the agent's browser once the owner signs in, \
         and save any statement into the private inbox (then `bin/cashcast import`). \
         Record it: `bin/cashcast tasks verdict {} pass|fail --reason \"<what it showed, no amounts>\" [--source browser]`.",
        task.id
    )
}

fn evidence(task: &Task, today: NaiveDate, specs: &[(String, Json)]) -> String {
    let mut step = where_(task, specs);
    if let Some(due) = task.due.filter(|d| *d > today) {
        step = format!("Not before {due}: the evidence may not exist yet. {step}");
    }
    if !task.notes.is_empty() {
        step += &format!(" Your rule: {}", task.notes.trim());
    }
    step
}

fn resolver(id: &str) -> Option<&'static str> {
    RESOLVERS
        .iter()
        .find(|(p, _)| id.starts_with(p))
        .map(|(_, s)| *s)
}

pub fn classify(
    tasks: &[Task],
    browser_ok: bool,
    change_ids: &[String],
    today: NaiveDate,
) -> Result<Vec<Row>, Unported> {
    let specs = connectors::load_all()?;
    let mut sorted: Vec<&Task> = tasks.iter().collect();
    sorted.sort_by(|a, b| {
        (a.due.unwrap_or(NaiveDate::MAX), &a.id).cmp(&(b.due.unwrap_or(NaiveDate::MAX), &b.id))
    });
    let row = |t: &Task, lane, step: String| Row {
        task: t.clone(),
        lane,
        step,
    };
    Ok(sorted
        .into_iter()
        .map(|t| {
            if t.risk == "money" {
                row(t, "you", "It moves money. cashcast never moves money: pay or transfer it yourself, then tick it.".into())
            } else if let Some(section) = resolver(&t.id) {
                row(
                    t,
                    "auto",
                    format!("cashcast runs `bin/cashcast {section}` and marks it done (`bin/cashcast tasks resolve`)."),
                )
            } else if t.risk == "read" && is_check(&t.title) {
                row(t, "agent", evidence(t, today, &specs))
            } else if t.risk == "settings" && change_ids.contains(&t.id) {
                let need = if browser_ok {
                    ""
                } else {
                    "First run in Terminal: bin/cashcast consent grant actions:browser. "
                };
                row(
                    t,
                    "browser",
                    format!(
                        "{need}An agent does it in your browser after you sign in (bin/cashcast changes show {}).",
                        t.id
                    ),
                )
            } else if t.risk == "settings" {
                row(t, "you", "No connector does this change. Do it in the issuer's app or by phone, then tick it.".into())
            } else {
                row(t, "you", "It needs your sign-in, a check or a decision. Do it, then tick it.".into())
            }
        })
        .collect())
}

/// Runs each due `auto` task through `run(section)` and marks it done when that works.
pub fn resolve<'a>(
    user: &UserHome,
    rows: &'a [Row],
    today: NaiveDate,
    mut run: impl FnMut(&str) -> Result<bool, AppError>,
) -> Result<(Vec<&'a Row>, Vec<&'a Row>), AppError> {
    let (mut resolved, mut blocked) = (Vec::new(), Vec::new());
    for r in rows {
        let due_soon = r
            .task
            .due
            .is_none_or(|d| d <= today + Duration::days(AUTO_WINDOW));
        if r.lane == "auto" && due_soon && run(resolver(&r.task.id).unwrap_or_default())? {
            changes::mark(user, &r.task.id, today, false)?;
            resolved.push(r);
        } else if r.lane == "browser" || r.lane == "you" {
            blocked.push(r);
        }
    }
    Ok((resolved, blocked))
}

/// Records an agent check. A pass marks the task done; a fail keeps it open.
/// The inner Err is a refusal, printed as `error: …` with exit 1.
#[allow(clippy::too_many_arguments)]
pub fn verdict(
    user: &UserHome,
    rows: &[Row],
    task_id: &str,
    result: &str,
    reason: &str,
    source: &str,
    today: NaiveDate,
    browser_ok: bool,
) -> Result<Result<String, String>, AppError> {
    let Some(row) = rows
        .iter()
        .find(|r| r.task.id == task_id)
        .filter(|r| r.lane == "agent")
    else {
        return Ok(Err(format!(
            "{task_id} is not a check an agent answers (see bin/cashcast tasks)."
        )));
    };
    if let Some(due) = row.task.due.filter(|d| *d > today) {
        return Ok(Err(format!(
            "{task_id}: not before {due}; the evidence may not exist yet."
        )));
    }
    if !matches!(result, "pass" | "fail") || !matches!(source, "local" | "browser") {
        return Ok(Err(
            "result is pass or fail; source is local or browser.".into()
        ));
    }
    let reason = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if reason.is_empty() || reason.chars().count() > REASON_MAX || has_amount(&reason) {
        return Ok(Err(format!(
            "the reason says what the evidence showed in at most {REASON_MAX} characters, with no amount, balance or score."
        )));
    }
    if source == "browser" && !browser_ok {
        return Ok(Err(
            "a browser check needs consent. The owner runs in Terminal: bin/cashcast consent grant agent:browser-read".into(),
        ));
    }
    let io = AppError::io;
    std::fs::create_dir_all(user.data()).map_err(io)?;
    let path = user.data().join("verdicts.jsonl");
    let line = Json::Obj(vec![
        ("task".into(), Json::Str(task_id.into())),
        ("on".into(), Json::Str(today.to_string())),
        ("result".into(), Json::Str(result.into())),
        ("source".into(), Json::Str(source.into())),
        ("reason".into(), Json::Str(reason.clone())),
    ]);
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(io)?;
        writeln!(f, "{}", crate::core::pyjson::dumps(&line, None, false)).map_err(io)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
    }
    events::log(
        user,
        "task_verdict",
        &[
            ("result", Json::Str(result.into())),
            ("source", Json::Str(source.into())),
        ],
    );
    if result == "pass" {
        return Ok(Ok(changes::mark(user, task_id, today, false)?));
    }
    Ok(Ok(format!(
        "{task_id} stays open: the evidence did not pass ({reason}). Tell the owner what to fix, then check again."
    )))
}

fn line(r: &Row) -> String {
    format!(
        "- `{}` ({}): {}\n  - {}",
        r.task.id,
        r.task.due.map_or("no date".to_string(), |d| d.to_string()),
        r.task.title,
        r.step
    )
}

/// The list, or with `done` = (resolved, blocked) the `tasks resolve` report.
pub fn render(rows: &[Row], done: Option<(&[&Row], &[&Row])>) -> String {
    let Some((resolved, blocked)) = done else {
        let mut lines = vec![
            format!("# Open finance tasks ({})", rows.len()),
            String::new(),
        ];
        for (lane, head) in [
            ("auto", "cashcast resolves"),
            ("agent", "Agent checks"),
            ("browser", "Agent in your browser"),
            ("you", "Blocked: you do these"),
        ] {
            let group: Vec<&Row> = rows.iter().filter(|r| r.lane == lane).collect();
            if !group.is_empty() {
                lines.push(format!("## {head} ({})", group.len()));
                lines.push(String::new());
                lines.extend(group.iter().map(|r| line(r)));
                lines.push(String::new());
            }
        }
        lines.push("Resolve what cashcast can: bin/cashcast tasks resolve".into());
        return lines.join("\n");
    };
    let agent: Vec<&Row> = rows.iter().filter(|r| r.lane == "agent").collect();
    let mut lines = vec![
        format!("# Resolved {} of {} open tasks", resolved.len(), rows.len()),
        String::new(),
    ];
    if resolved.is_empty() {
        lines.push("- Nothing cashcast can resolve is due this week.".into());
    }
    lines.extend(
        resolved
            .iter()
            .map(|r| format!("- Done: `{}`: {}", r.task.id, r.task.title)),
    );
    if !agent.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "## Agent checks next ({}): answer each from evidence",
            agent.len()
        ));
        lines.push(String::new());
        lines.extend(agent.iter().map(|r| line(r)));
    }
    lines.push(String::new());
    lines.push(format!(
        "## BLOCKED ({}): cashcast cannot finish these",
        blocked.len()
    ));
    lines.push(String::new());
    lines.extend(blocked.iter().map(|r| line(r)));
    lines.push(String::new());
    lines.push(
        "Update Reminders, Calendar and Notion: run in Terminal: bin/cashcast sync all".into(),
    );
    lines.join("\n")
}

pub fn as_json(rows: &[Row]) -> Json {
    Json::List(
        rows.iter()
            .map(|r| {
                Json::Obj(vec![
                    ("id".into(), Json::Str(r.task.id.clone())),
                    (
                        "due".into(),
                        r.task.due.map_or(Json::Null, |d| Json::Str(d.to_string())),
                    ),
                    ("lane".into(), Json::Str(r.lane.into())),
                    ("step".into(), Json::Str(r.step.clone())),
                    ("risk".into(), Json::Str(r.task.risk.clone())),
                ])
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_and_scores_are_refused_but_years_pass() {
        for bad in ["paid $5", "1.5 left", "score 742", "20261 rows", "2026a"] {
            assert!(has_amount(bad), "{bad}");
        }
        for ok in ["paid in 2026", "statement shows it", "12 days"] {
            assert!(!has_amount(ok), "{ok}");
        }
    }

    #[test]
    fn a_check_starts_with_a_check_word() {
        assert!(is_check("Confirm the rent landed"));
        assert!(!is_check("Checkout the card"));
    }
}
