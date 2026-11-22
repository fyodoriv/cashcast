//! Credit score: what balance each card should report, and when to pay it
//! down, as `strategy/credit.py`.
//!
//! Bureaus see each card's statement closing balance. About 1–4% of the limit
//! scores best. Paying down two days before the statement closes keeps the
//! reported balance on target while every statement is still paid in full.

use super::py::{self, list, mon_day, money, obj, pct};
use super::situation::{Unported, knowledge};
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::Json;
use chrono::{Datelike, Duration, NaiveDate};

const IDEAL: f64 = 0.03;
const CEILING: f64 = 0.09;

pub fn target(limit: f64, small: bool) -> f64 {
    if small {
        return 50.0;
    }
    let t = py::round0(limit * IDEAL / 50.0) * 50;
    if t == 0 { 50.0 } else { t as f64 }
}

fn last_day(y: i32, m: u32) -> u32 {
    let next = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    };
    next.map_or(28, |d| (d - Duration::days(1)).day())
}

pub fn next_close(today: NaiveDate, day: u32) -> NaiveDate {
    let on = |y: i32, m: u32| {
        NaiveDate::from_ymd_opt(y, m, day.min(last_day(y, m)).max(1)).expect("valid day")
    };
    let d = on(today.year(), today.month());
    if d < today {
        let (y, m) = if today.month() == 12 {
            (today.year() + 1, 1)
        } else {
            (today.year(), today.month() + 1)
        };
        return on(y, m);
    }
    d
}

/// The card's own name, from the profile or else the knowledge catalog.
pub fn label(card: &Json) -> Result<String, Unported> {
    if let Some(n) = card.get("name").filter(|v| v.truthy()) {
        return Ok(n.py_str());
    }
    let catalog = knowledge("credit_cards", None)?;
    let id = card.get("id");
    let entry = list(&catalog, "cards")
        .iter()
        .rev()
        .find(|c| c.get("id") == id);
    Ok(match entry.and_then(|e| e.get("name")) {
        Some(n) => n.py_str(),
        None => id.map_or("card".into(), Json::py_str),
    })
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let mut sec = Section::new(
        "credit",
        "Credit score: statement targets and pay-down dates",
    );
    let sit = &ctx.sit;
    let cards = list(obj(&sit.profile, "wallet"), "cards");
    let with_limits: Vec<&Json> = cards
        .iter()
        .filter(|c| c.get("limit").is_some_and(Json::truthy))
        .collect();
    if with_limits.is_empty() {
        sec.line("- Add `limit` and `statement_close_day` to each card in profile.toml to get targets and dates.");
        return Ok(sec);
    }
    let mut used: Vec<(String, f64)> = Vec::new();
    if let Some(Data::Cashback(plan)) = ctx.result("cashback").map(|s| &s.data) {
        for a in &plan.allocations {
            match used.iter_mut().find(|(k, _)| *k == a.card) {
                Some(slot) => slot.1 += a.amount / 12.0,
                None => used.push((a.card.clone(), a.amount / 12.0)),
            }
        }
    }
    let used_of = |id: Option<&Json>| {
        let id = id.map(Json::py_str);
        used.iter()
            .find(|(k, _)| Some(k) == id.as_ref())
            .map_or(0.0, |(_, v)| *v)
    };
    sec.line("| Card | Limit | Target at close | Keep balance under (9%) | Usual use if not paid early | Pay down by |");
    sec.line("|---|---|---|---|---|---|");
    let (mut total_limit, mut total_target) = (0.0, 0.0);
    for c in with_limits {
        let limit = py::num(c.get("limit")).unwrap_or(0.0);
        let monthly = used_of(c.get("id"));
        let small = monthly < 100.0;
        let tgt = py::num(c.get("target")).unwrap_or_else(|| target(limit, small));
        total_limit += limit;
        total_target += tgt;
        let mut pay_by = "—".to_string();
        if let Some(day) = c.get("statement_close_day").filter(|v| v.truthy()) {
            let close = next_close(sit.today, py::num(Some(day)).unwrap_or(1.0) as u32);
            let pay_on =
                close - Duration::days(py::num(c.get("paydown_days_before")).unwrap_or(2.0) as i64);
            pay_by = format!("{} (closes {})", mon_day(pay_on), mon_day(close));
            sec.tasks.push(
                Task::new(
                    format!(
                        "paydown-{}-{}",
                        c.get("id").map_or("None".into(), Json::py_str),
                        close.format("%Y-%m")
                    ),
                    format!("Pay {} down to {}", label(c)?, money(tgt)),
                )
                .due(pay_on)
                .notes(
                    "Transfer the card's current balance minus the target to its bank, then pay \
                     the card. Autopay still pays the full statement.",
                )
                .risk("money")
                .recurs("month"),
            );
        }
        sec.line(format!(
            "| {} | {} | {} ({}) | {} | {} | {pay_by} |",
            label(c)?,
            money(limit),
            money(tgt),
            pct(tgt / limit),
            money(limit * CEILING),
            if limit != 0.0 {
                pct(monthly / limit)
            } else {
                "—".into()
            }
        ));
    }
    sec.line("");
    sec.line(format!(
        "- Combined: {} reported on {} of limits ({}).",
        money(total_target),
        money(total_limit),
        pct(total_target / total_limit)
    ));
    sec.line("- Every card autopays its **full statement balance** from a chequing account: no interest, perfect history.");
    sec.line("- Never close an old card; product-change it to a no-fee version to keep its age and limit.");
    if let Some(months) = sit
        .goal("mortgage_in_months")
        .filter(|v| !matches!(v, Json::Null))
    {
        sec.line(format!(
            "- Mortgage in about {} months: no new credit or hard pulls from now; pull both bureau \
             reports 3 months before and dispute errors.",
            months.py_str()
        ));
        let m = py::num(Some(months)).unwrap_or(0.0) as i64;
        if m > 3 {
            let due = sit.today + Duration::days(30 * (m - 3));
            sec.tasks.push(
                Task::new(
                    "mortgage-reports",
                    "Pull Equifax and TransUnion reports; dispute errors",
                )
                .due(due),
            );
        }
    }
    sec.tasks.push(
        Task::new(
            "credit-scores-monthly",
            "Check both credit scores (Equifax and TransUnion)",
        )
        .due(next_close(sit.today, 1)),
    );
    Ok(sec)
}
