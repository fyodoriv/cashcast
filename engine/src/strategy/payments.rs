//! Payments analysis: where the money goes and what changed, as
//! `strategy/payments.py`. Works on the engine's monthly category totals;
//! never on single payments.

use super::py::{self, mean, money, pct, pstdev};
use super::situation::Unported;
use super::{Ctx, Data, Section, Task};
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

#[derive(Debug, Clone)]
pub struct Row {
    pub category: String,
    pub avg: f64,
    pub trend: Option<f64>,
    pub yoy: Option<f64>,
    pub fixed: bool,
    pub avoidable: bool,
}

#[derive(Debug)]
pub struct PaymentsData {
    pub rows: Vec<Row>,
    pub recurring: Option<Json>,
}

fn cadence(c: &str) -> &str {
    match c {
        "weekly" => "week",
        "monthly" => "month",
        "quarterly" => "quarter",
        "yearly" => "year",
        other => other,
    }
}

fn cents(r: &Json, k: &str) -> f64 {
    py::num(r.get(k)).unwrap_or(0.0)
}

/// The "Recurring payments" sub-section: one row per payee and a yearly total.
pub fn recurring_lines(items: &[Json]) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        "### Recurring payments".into(),
        String::new(),
    ];
    if items.is_empty() {
        lines.push(
            "- None found: no payee charged you at least 3 times on a regular schedule.".into(),
        );
        return lines;
    }
    let total = items
        .iter()
        .map(|r| cents(r, "yearly_cents") as i64)
        .sum::<i64>() as f64
        / 100.0;
    lines.push(format!(
        "{} {} you on a schedule: {} a year in total. Your private `data/review.md` shows which payee each one is.",
        items.len(),
        if items.len() == 1 { "payee charges" } else { "payees charge" },
        money(total)
    ));
    lines.push(String::new());
    lines.push(
        "| Payment | Category | Every | Usual charge | Charges | Last seen | Per year |".into(),
    );
    lines.push("|---|---|---|---|---|---|---|".into());
    let s = |r: &Json, k: &str| r.get(k).map_or("None".into(), Json::py_str);
    for r in items {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            s(r, "label"),
            s(r, "category"),
            cadence(&s(r, "cadence")),
            money(cents(r, "median_cents") / 100.0),
            s(r, "count"),
            s(r, "last_seen"),
            money(cents(r, "yearly_cents") / 100.0)
        ));
    }
    lines
}

pub fn analyze(spending: &Json, window: &[usize]) -> Vec<Row> {
    let mut rows = Vec::new();
    let Json::Obj(items) = spending else {
        return rows;
    };
    for (cat, series) in items {
        let at = |i: usize| match series {
            Json::List(v) => py::num(v.get(i)).unwrap_or(0.0) / 100.0,
            _ => 0.0,
        };
        let vals: Vec<f64> = window.iter().map(|&i| at(i)).collect();
        if vals.is_empty() || py::sum(vals.iter().copied()) <= 0.0 {
            continue;
        }
        let avg = mean(&vals);
        let split = vals.len().saturating_sub(3);
        let (prior, last3) = vals.split_at(split);
        let trend = if !prior.is_empty() && mean(prior) > 0.0 {
            Some((mean(last3) - mean(prior)) / mean(prior))
        } else {
            None
        };
        let mut yoy = None;
        if let Some(&last) = window.last()
            && last >= 12
        {
            let (a, b) = (at(last), at(last - 12));
            yoy = if b > 0.0 { Some((a - b) / b) } else { None };
        }
        let cv = if avg != 0.0 { pstdev(&vals) / avg } else { 0.0 };
        let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let lower = cat.to_lowercase();
        rows.push(Row {
            category: cat.clone(),
            avg,
            trend,
            yoy,
            fixed: cv < 0.15 && min > 0.0,
            avoidable: FEE_WORDS.iter().any(|w| lower.contains(w)),
        });
    }
    rows.sort_by(|a, b| {
        b.avg
            .partial_cmp(&a.avg)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("payments", "Payments analysis");
    let Some(summary) = sit.summary.as_ref().filter(|s| s.truthy()) else {
        sec.line("- No imported data yet.");
        return Ok(sec);
    };
    let window = sit.months_window();
    let rows = analyze(
        summary.get("spending_cents").unwrap_or(&Json::Null),
        &window,
    );
    if rows.is_empty() {
        sec.line("- No spending in the last 12 complete months.");
        return Ok(sec);
    }
    let total = py::sum(rows.iter().map(|r| r.avg));
    sec.line(format!(
        "Last {} complete months; {} per month in total.",
        window.len(),
        money(total)
    ));
    sec.line("");
    sec.line("| Category | Per month | Share | Trend (3 vs 9 months) | Year over year | Kind |");
    sec.line("|---|---|---|---|---|---|");
    for r in rows.iter().take(15) {
        let kind = if r.avoidable {
            "avoidable fee"
        } else if r.fixed {
            "fixed"
        } else {
            "variable"
        };
        // A rise reads "+200%" here as in "Rising fast" below.
        let signed = |x: f64| if (x * 10000.0).round() > 0.0 { format!("+{}", pct(x)) } else { pct(x) };
        let trend = r.trend.map_or("—".to_string(), signed);
        let yoy = r.yoy.map_or("—".to_string(), signed);
        sec.line(format!(
            "| {} | {} | {} | {trend} | {yoy} | {kind} |",
            r.category,
            money(r.avg),
            pct(r.avg / total)
        ));
    }
    let rising: Vec<&Row> = rows
        .iter()
        .filter(|r| r.trend.is_some_and(|t| t > 0.25) && r.avg > 50.0)
        .collect();
    if !rising.is_empty() {
        let parts: Vec<String> = rising
            .iter()
            .take(5)
            .map(|r| format!("{} (+{})", r.category, pct(r.trend.unwrap_or(0.0))))
            .collect();
        sec.line(format!("\n- Rising fast: {}.", parts.join(", ")));
    }
    let fees: Vec<&Row> = rows.iter().filter(|r| r.avoidable).collect();
    if !fees.is_empty() {
        let yearly = py::sum(fees.iter().map(|r| r.avg)) * 12.0;
        sec.line(format!(
            "- Avoidable fees and interest: about {} a year. Find each one's cause in your bank app.",
            money(yearly)
        ));
        sec.tasks.push(
            Task::new(
                "fees-review",
                format!(
                    "Find and stop the fees and interest (about {} a year)",
                    money(yearly)
                ),
            )
            .due(sit.today),
        );
    }
    let fixed: Vec<&Row> = rows.iter().filter(|r| r.fixed).collect();
    if !fixed.is_empty() {
        sec.line(format!(
            "- Fixed-like costs: {} per month. These are the ones worth renegotiating once a year.",
            money(py::sum(fixed.iter().map(|r| r.avg)))
        ));
    }
    let recurring = summary
        .get("recurring_payments")
        .filter(|r| !matches!(r, Json::Null))
        .cloned();
    if let Some(r) = &recurring {
        sec.lines.extend(recurring_lines(match r {
            Json::List(v) => v,
            _ => &[],
        }));
    }
    sec.data = Data::Payments(PaymentsData { rows, recurring });
    Ok(sec)
}
