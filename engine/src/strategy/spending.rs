//! Spending checkup: "more than usual?",
//! "spend more than I earn?" and "interest and fees paid?". Works on the
//! engine's monthly category totals in cents; never on single payments.

use super::py::{money, pct};
use super::situation::Unported;
use super::{Ctx, Section, Task};
use crate::core::pyjson::Json;

const FEE_WORDS: [&str; 7] = [
    "fee",
    "interest",
    "penalty",
    "overdraft",
    "charge",
    "nsf",
    "late",
];
const HIGHER: f64 = 1.25;
const MIN_USUAL: f64 = 50.0;
const APR_NOTE: &str = "- APR is the yearly interest rate on a balance you carry. At 20.99% APR, a $1,000 card balance \
kept for a year costs about $209.90. Pay the full statement balance each month and it costs $0.";

const CHARGES_NOTE: &str =
    "- Charges billed twice, new subscriptions and price rises: `bin/cashcast charges`.";

fn series(v: &Json) -> Vec<i64> {
    match v {
        Json::List(items) => items
            .iter()
            .map(|x| match x {
                Json::Int(i) => *i,
                Json::Float(f) => *f as i64,
                _ => 0,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn entries(v: Option<&Json>) -> Vec<(String, Vec<i64>)> {
    match v {
        Some(Json::Obj(items)) => items.iter().map(|(k, s)| (k.clone(), series(s))).collect(),
        _ => Vec::new(),
    }
}

fn at(s: &[i64], i: usize) -> i64 {
    s.get(i).copied().unwrap_or(0)
}

fn total_cents(rows: &[(String, Vec<i64>)], window: &[usize]) -> i64 {
    rows.iter()
        .map(|(_, s)| window.iter().map(|&i| at(s, i)).sum::<i64>())
        .sum()
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("spending", "Spending checkup");
    let Some(summary) = sit.summary.as_ref().filter(|s| s.truthy()) else {
        sec.line("- No imported data yet.");
        return Ok(sec);
    };
    let window = sit.months_window();
    let Some(&last) = window.last() else {
        sec.line("- No complete month yet.");
        return Ok(sec);
    };
    let spending = entries(summary.get("spending_cents"));
    let month = match summary.get("months") {
        Some(Json::List(m)) => m.get(last).map(Json::py_str).unwrap_or_default(),
        _ => String::new(),
    };
    let n = window.len();

    sec.line("### Is that more than usual?");
    sec.line("");
    if n < 4 {
        sec.line(format!(
            "- Not enough history yet: {n} complete months; the comparison needs 4."
        ));
    } else {
        let prior = &window[..n - 1];
        let mut rows: Vec<(String, f64, f64)> = Vec::new();
        for (cat, s) in &spending {
            let usual =
                prior.iter().map(|&i| at(s, i)).sum::<i64>() as f64 / prior.len() as f64 / 100.0;
            let now = at(s, last) as f64 / 100.0;
            if usual >= MIN_USUAL && now > usual * HIGHER {
                rows.push((cat.clone(), now, usual));
            }
        }
        rows.sort_by(|a, b| {
            (b.1 - b.2)
                .partial_cmp(&(a.1 - a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if rows.is_empty() {
            sec.line(format!(
                "- {month}: every category was within 25% of its usual month."
            ));
        } else {
            sec.line(format!("{month} against the {} months before it:", n - 1));
            sec.line("");
            sec.line("| Category | This month | Usual month | Change |");
            sec.line("|---|---|---|---|");
            for (cat, now, usual) in rows.iter().take(5) {
                sec.line(format!(
                    "| {cat} | {} | {} | +{} |",
                    money(*now),
                    money(*usual),
                    pct((now - usual) / usual)
                ));
            }
        }
    }

    sec.line("");
    sec.line("### Do I spend more than I earn?");
    sec.line("");
    // Salary rows go to flows_cents, the rest of income to income_cents.
    let salary: Vec<(String, Vec<i64>)> = entries(summary.get("flows_cents"))
        .into_iter()
        .filter(|(k, _)| k == "salary" || k == "salary_off_cycle")
        .collect();
    let earned = (total_cents(&entries(summary.get("income_cents")), &window)
        + total_cents(&salary, &window)) as f64
        / 100.0;
    let spent = total_cents(&spending, &window) as f64 / 100.0;
    let gap = earned - spent;
    if gap >= 0.0 {
        sec.line(format!(
            "- No. In the last {n} months you earned {} and spent {}: {} left over.",
            money(earned),
            money(spent),
            money(gap)
        ));
    } else {
        sec.line(format!(
            "- Yes. In the last {n} months you earned {} and spent {}: {} more than you earned.",
            money(earned),
            money(spent),
            money(-gap)
        ));
        sec.tasks.push(
            Task::new(
                "spending-over-income",
                format!(
                    "Spending was {} above income in {n} months: pick one category to cut",
                    money(-gap)
                ),
            )
            .due(sit.today)
            .recurs("week"),
        );
    }

    sec.line("");
    sec.line("### How much interest and fees did I pay?");
    sec.line("");
    let fees: Vec<(String, Vec<i64>)> = spending
        .iter()
        .filter(|(c, _)| {
            let lower = c.to_lowercase();
            FEE_WORDS.iter().any(|w| lower.contains(w))
        })
        .cloned()
        .collect();
    let paid = total_cents(&fees, &window) as f64 / 100.0;
    if paid > 0.0 {
        let mut names: Vec<&str> = fees
            .iter()
            .filter(|(_, s)| window.iter().map(|&i| at(s, i)).sum::<i64>() > 0)
            .map(|(c, _)| c.as_str())
            .collect();
        names.sort();
        sec.line(format!(
            "- {} in the last {n} months, in: {}.",
            money(paid),
            names.join(", ")
        ));
    } else {
        sec.line(format!("- None found in the last {n} months."));
    }
    sec.line(APR_NOTE);
    sec.line(CHARGES_NOTE);
    Ok(sec)
}
