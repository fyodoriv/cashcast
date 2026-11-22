//! The monthly review: five minutes that keep every dollar on plan, as
//! `strategy/review.py` (with `journal.compare`).

use super::py::{self, list, money, pct};
use super::situation::Unported;
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::{self, Json};
use chrono::{Datelike, Duration, NaiveDate};
use std::path::Path;

pub const THRESHOLD: f64 = 0.15;

fn first_monday(year: i32, month: u32) -> NaiveDate {
    let d = NaiveDate::from_ymd_opt(year, month, 1).expect("valid");
    d + Duration::days((7 - d.weekday().num_days_from_monday() as i64) % 7)
}

pub fn next_review(today: NaiveDate) -> NaiveDate {
    let d = first_monday(today.year(), today.month());
    if d >= today {
        return d;
    }
    let (y, m) = if today.month() == 12 {
        (today.year() + 1, 1)
    } else {
        (today.year(), today.month() + 1)
    };
    first_monday(y, m)
}

pub struct Scored {
    pub month: String,
    pub predicted: f64,
    pub actual: f64,
    pub error: f64,
}

/// `journal.compare`: scored predictions (oldest first) and whether to pivot.
pub fn compare(home: &Path, summary: Option<&Json>) -> Result<(Vec<Scored>, bool), Unported> {
    let Some(summary) = summary.filter(|s| s.truthy()) else {
        return Ok((Vec::new(), false));
    };
    let path = home.join("data").join("journal.jsonl");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Unported(format!("journal: {e}"))),
    };
    let months: Vec<String> = list(summary, "months").iter().map(Json::py_str).collect();
    let mut scored = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let e = pyjson::loads(line).ok_or_else(|| Unported("journal line is not JSON".into()))?;
        let month = e.get("month").map_or(String::new(), Json::py_str);
        if e.get("metric").map(Json::py_str).as_deref() != Some("spending") {
            continue;
        }
        let Some(i) = months.iter().position(|m| *m == month) else {
            continue;
        };
        let total: i64 = match summary.get("spending_cents") {
            Some(Json::Obj(items)) => items
                .iter()
                .map(|(_, s)| match s {
                    Json::List(v) => py::num(v.get(i)).unwrap_or(0.0) as i64,
                    _ => 0,
                })
                .sum(),
            _ => 0,
        };
        let actual = total as f64 / 100.0;
        let predicted = py::num(e.get("predicted")).unwrap_or(0.0);
        let error = if predicted != 0.0 {
            (actual - predicted) / predicted
        } else {
            0.0
        };
        scored.push(Scored {
            month,
            predicted,
            actual: py::round(actual, 2),
            error,
        });
    }
    scored.sort_by(|a, b| a.month.cmp(&b.month));
    let misses: Vec<bool> = scored.iter().map(|e| e.error.abs() > THRESHOLD).collect();
    let pivot = misses.len() >= 2 && misses[misses.len() - 2..].iter().all(|m| *m);
    Ok((scored, pivot))
}

#[derive(Debug)]
pub struct ReviewData {
    pub pivot: bool,
    pub next_review: NaiveDate,
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("review", "Monthly review (5 minutes)");
    let when = next_review(sit.today);
    let steps = [
        "Import: add last month's exports to your inbox, run `bin/cashcast import`.",
        "Statements: every card closed at or under its target and was paid in full.",
        "Bank minimums: no monthly fee was charged; top accounts back up to their keep amounts.",
        "Rent charges: count this cap year's charges per card; switch the rent card when a count is used up.",
        "Credit scores: check Equifax and TransUnion; any drop → find the cause.",
        "Cash sweep: invest cash above the reserve, in the account order of your strategy.",
        "Cash back: redeem balances of $25+ as a statement credit.",
        "Budget: look at the categories over budget; adjust the budget or the habit.",
    ];
    sec.line(format!("Next review: **{}**.", when.format("%a, %b %-d")));
    sec.line("");
    for (i, s) in steps.iter().enumerate() {
        sec.line(format!("{}. {s}", i + 1));
    }
    let mut signals = Vec::new();
    if let Some(summary) = sit.summary.as_ref().filter(|s| s.truthy()) {
        let last = list(summary, "months")
            .last()
            .map_or(String::new(), Json::py_str);
        let parsed = last
            .split_once('-')
            .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<u32>().ok()?)));
        let Some((y, m)) = parsed else {
            return Err(Unported(format!("bad month {last}")));
        };
        let next = if m == 12 {
            NaiveDate::from_ymd_opt(y + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(y, m + 1, 1)
        };
        let end = next.ok_or_else(|| Unported(format!("bad month {last}")))? - Duration::days(1);
        let age = (sit.today - end).num_days();
        if age > 35 {
            signals.push(format!(
                "Data is {age} days old (last month imported: {last}). Import new exports first."
            ));
        }
    }
    let (scored, pivot) = compare(&sit.home, sit.summary.as_ref())?;
    if !scored.is_empty() {
        let misses = scored.iter().filter(|e| e.error.abs() > THRESHOLD).count();
        ctx.events.push((
            "forecast_scored".into(),
            vec![
                ("checked".into(), Json::Int(scored.len() as i64)),
                ("misses".into(), Json::Int(misses as i64)),
            ],
        ));
    }
    for e in scored.iter().skip(scored.len().saturating_sub(3)) {
        signals.push(format!(
            "Spending {}: predicted {}, actual {} ({}).",
            e.month,
            money(e.predicted),
            money(e.actual),
            pct(e.error)
        ));
    }
    if pivot {
        signals.push(
            "**Pivot:** the last two months missed by more than 15%. Re-check the spending method, the budget and \
             any life change, then re-run the strategy."
                .into(),
        );
    }
    let k = crate::knowledge::check(sit.today, &crate::knowledge::data_dir())
        .map_err(|e| Unported(format!("knowledge: {}", e.0.display())))?;
    if !k.stale.is_empty() {
        signals.push(format!(
            "Knowledge past its review date: {}. Re-check those facts before relying on them.",
            k.stale.join(", ")
        ));
    }
    if !signals.is_empty() {
        sec.line("");
        sec.line("**Signals this month:**");
        for s in &signals {
            sec.line(format!("- {s}"));
        }
    }
    sec.tasks.push(
        Task::new(
            format!("monthly-review-{}", when.format("%Y-%m")),
            "Monthly money review (5 minutes)",
        )
        .due(when)
        .notes(&format!(
            "Run: bin/cashcast review. {}",
            steps[..3].join(" ")
        ))
        .recurs("month"),
    );
    for other in &ctx.results {
        if other.id != "review" {
            sec.tasks.extend(other.tasks.iter().cloned());
        }
    }
    sec.data = Data::Review(ReviewData {
        pivot,
        next_review: when,
    });
    Ok(sec)
}
