//! Balance forecast: the original cashcast model, summarized for the
//! strategy, as `strategy/forecast.py`.

use super::py::{self, money};
use super::situation::Unported;
use super::{Ctx, Data, Section};
use crate::forecast::model::{self, MissingInputs};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

#[derive(Debug)]
pub struct ForecastData {
    pub first_month: Option<NaiveDate>,
    pub low: f64,
    pub to_invest: f64,
    pub next_month_spending: f64,
    /// The monthly rows, for the web app's chart and table.
    pub rows: Vec<model::Row>,
    /// 80 % range per row as (below, above) offsets to month-end cash, from the backtest.
    pub ranges: Vec<(f64, f64)>,
    /// Backtest misses (WAPE, 0..1): month-end cash, net per month, net per 3 months.
    pub accuracy: Option<(f64, f64, f64)>,
}

pub fn f(d: Decimal) -> f64 {
    d.to_string()
        .parse()
        .unwrap_or_else(|_| d.to_f64().unwrap_or(0.0))
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("forecast", "Balance forecast");
    let path = sit.home.join("data").join("summary.json");
    let Some(summary) = sit.summary.as_ref().filter(|s| s.truthy() && path.exists()) else {
        sec.line(
            "- No imported data yet: put exports in your inbox and run `bin/cashcast import`.",
        );
        return Ok(sec);
    };
    let fail = |e: model::Failure| Unported(format!("forecast: {}", e.0));
    let cfg = crate::forecast::read_toml(&sit.home.join("cashcast.toml")).map_err(fail)?;
    let inputs = match model::inputs_from(&cfg, summary, sit.today).map_err(fail)? {
        Ok(i) => i,
        Err(MissingInputs(problems)) => {
            sec.line(format!(
                "- The forecast needs answers first: {}. Run `bin/cashcast status`.",
                problems.join("; ")
            ));
            return Ok(sec);
        }
    };
    let hist = model::load_history(summary).map_err(fail)?;
    let rows = model::run(&inputs, &hist).map_err(fail)?;
    let Some(low) = rows.iter().fold(None, |m: Option<&model::Row>, r| match m {
        Some(b) if b.month_end <= r.month_end => Some(b),
        _ => Some(r),
    }) else {
        return Err(Unported("forecast: no rows".into()));
    };
    let to_invest = py::sum(rows.iter().map(|r| f(r.to_invest)));
    sec.line(format!(
        "- {} months from {}: income {}, expenses {}.",
        inputs.horizon,
        inputs.start.format("%b %Y"),
        money(py::sum(rows.iter().map(|r| f(r.income)))),
        money(py::sum(rows.iter().map(|r| f(r.expenses))))
    ));
    sec.line(format!(
        "- **Can invest {}** while keeping the {} floor.",
        money(to_invest),
        money(f(inputs.floor()))
    ));
    sec.line(format!(
        "- Lowest month-end cash: {} in {}.",
        money(f(low.month_end)),
        low.month.format("%b %Y")
    ));
    let misses = crate::forecast::backtest::backtest(&inputs, &hist).unwrap_or_default();
    let ranges = crate::forecast::backtest::month_end_ranges(&misses, rows.len());
    let accuracy = {
        use crate::forecast::backtest::{by_line, quarters, score};
        let lines = by_line(&misses);
        let get = |k: &str| {
            lines
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let (me, net) = (get("month_end"), get("net_cash_flow"));
        let q = quarters(&net);
        match (
            score(&me).wape,
            score(&net).wape,
            score(&q.iter().collect::<Vec<_>>()).wape,
        ) {
            (Some(a), Some(b), Some(c)) => Some((f(a), f(b), f(c))),
            _ => None,
        }
    };
    cap_reserve_sweep(ctx, rows.first().map_or(0.0, |r| f(r.to_invest)));
    sec.line("- Full table and editable workbook: `bin/cashcast forecast`.");
    sec.data = Data::Forecast(ForecastData {
        first_month: rows.first().map(|r| r.month),
        low: f(low.month_end),
        to_invest,
        next_month_spending: rows.first().map_or(0.0, |r| f(r.expenses)),
        rows: rows.clone(),
        ranges,
        accuracy,
    });
    Ok(sec)
}

/// The reserve section runs first and only knows cash now. Cap its "invest now"
/// task at what the forecast can invest this month, so later dips stay covered.
fn cap_reserve_sweep(ctx: &mut Ctx, safe_now: f64) {
    let Some(reserve) = ctx.results.iter_mut().find(|s| s.id == "reserve") else {
        return;
    };
    let Some(i) = reserve.tasks.iter().position(|t| t.id == "sweep-cash") else {
        return;
    };
    let asked: f64 = reserve.tasks[i]
        .title
        .split('$')
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .and_then(|s| s.replace(',', "").parse().ok())
        .unwrap_or(0.0);
    if asked <= safe_now + 0.5 {
        return;
    }
    let kept = asked - safe_now;
    if safe_now < 1.0 {
        reserve.tasks.remove(i);
    } else {
        reserve.tasks[i].title = format!(
            "Invest {} of the {} above the reserve now; keep {} for later months",
            money(safe_now),
            money(asked),
            money(kept)
        );
    }
    reserve.line(format!(
        "- The forecast keeps {} of that for later months that dip: invest {} now.",
        money(kept),
        money(safe_now.max(0.0))
    ));
    // The safe-account move must name only the kept part, or both tasks spend the same cash.
    for t in ctx.results.iter_mut().flat_map(|s| s.tasks.iter_mut()) {
        if let Some(rest) = t.title.strip_prefix("Move $")
            && let Some((_, tail)) = rest.split_once(" above your reserve")
        {
            t.title = format!("Move the {} set aside for later months{tail}", money(kept));
        }
    }
    // The invest plan runs before the cap is known: its "keep safe" step must name only the kept part too.
    let whole = format!("Keep the {} above your reserve safe", crate::invest_plan::dollars(asked));
    let part = format!("Keep the {} set aside for later months in a safe place", money(kept));
    for s in ctx.results.iter_mut() {
        for l in s.lines.iter_mut() {
            if l.contains(&whole) {
                *l = l.replace(&whole, &part);
            }
        }
        for t in s.tasks.iter_mut() {
            if t.title.contains(&whole) {
                t.title = t.title.replace(&whole, &part);
            }
        }
    }
}
