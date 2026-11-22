//! Crypto at market value: what the exchange balances are worth in CAD, and
//! what selling would cost in tax, as `strategy/crypto.py`.

use super::banking::title;
use super::py::{self, fmt_g, getf, list, money, obj, pct};
use super::situation::Unported;
use super::{Ctx, DISCLAIMER, Data, Section};
use crate::core::pyjson::Json;
use chrono::NaiveDate;

#[derive(Default, Clone)]
struct Cost {
    acb: f64,
    qty: f64,
    unknown: bool,
}

/// Average cost per unit across every wallet in the holdings, or None when any unit's cost is unknown.
fn unit_cost(c: Option<&Cost>) -> Option<f64> {
    let c = c?;
    if c.unknown || c.qty <= 0.0 {
        return None;
    }
    Some(c.acb / c.qty)
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("crypto", "Crypto: market value from your exchanges");
    let rows = sit.rows_of("crypto")?;
    if rows.is_empty() {
        sec.line("- No crypto balances found. Run `bin/cashcast api balances kraken` to read your Kraken balances.");
        return Ok(sec);
    }
    let mut cost: Vec<(String, Cost)> = Vec::new();
    let empty = Json::Obj(Vec::new());
    for a in list(sit.holdings.as_ref().unwrap_or(&empty), "assets") {
        let sym = a
            .get("symbol")
            .map_or(String::new(), Json::py_str)
            .to_uppercase();
        let i = match cost.iter().position(|(k, _)| *k == sym) {
            Some(i) => i,
            None => {
                cost.push((sym, Cost::default()));
                cost.len() - 1
            }
        };
        let c = &mut cost[i].1;
        c.acb += getf(a, "acb_cad", 0.0);
        c.qty += getf(a, "quantity", 0.0);
        c.unknown = c.unknown || a.get("cost_unknown").is_some_and(Json::truthy);
    }
    let cost_of = |code: &str| cost.iter().find(|(k, _)| k == code).map(|(_, c)| c);
    sec.line(format!("*{DISCLAIMER}*"));
    sec.line("");
    sec.line("| Asset | Amount | CAD price (date) | Value | Cost (holdings) |");
    sec.line("|---|---|---|---|---|");
    let (mut total, mut unpriced) = (0.0, Vec::new());
    for r in &rows {
        let name = format!("{} {}", title(&r.service), r.currency);
        let Some(cad) = r.cad else {
            sec.line(format!(
                "| {name} | {} | no price | — | — |",
                fmt_g(r.amount, 8, true)
            ));
            unpriced.push(name);
            continue;
        };
        total += cad;
        let acb =
            unit_cost(cost_of(&r.currency)).map_or("unknown".to_string(), |u| money(u * r.amount));
        sec.line(format!(
            "| {name} | {} | {} ({}) | {} | {acb} |",
            fmt_g(r.amount, 8, true),
            money(r.rate.unwrap_or(0.0)),
            r.rate_date.py_str(),
            money(cad)
        ));
    }
    sec.line("");
    sec.line(format!(
        "- Crypto at market: **{}**. It is never counted as cash in the reserve.",
        money(total)
    ));
    if !unpriced.is_empty() {
        sec.line(format!(
            "- No price for {}: not in the total.",
            unpriced.join(", ")
        ));
    }
    let max_age =
        py::num(sit.knowledge("foreign_cash")?.get("crypto_max_age_days")).unwrap_or(7.0) as i64;
    let fetched = rows
        .iter()
        .filter(|r| r.fetched_on.truthy())
        .filter_map(|r| NaiveDate::parse_from_str(&r.fetched_on.py_str(), "%Y-%m-%d").ok())
        .min();
    if let Some(oldest) = fetched
        && (sit.today - oldest).num_days() > max_age
    {
        sec.line(format!(
            "- Prices are older than {max_age} days: run `bin/cashcast api balances kraken` again."
        ));
    }
    let cash = sit.cash_balance()?.unwrap_or(0.0);
    if cash + total > 0.0 {
        sec.line(format!(
            "- Share of cash plus crypto: {}.",
            pct(total / (cash + total))
        ));
    }
    let tax = sit.knowledge("tax_ca_2026")?;
    let inclusion = getf(obj(&tax, "limits"), "capital_gains_inclusion", 0.5);
    let known: Vec<_> = rows
        .iter()
        .filter(|r| r.cad.is_some() && unit_cost(cost_of(&r.currency)).is_some())
        .collect();
    if !known.is_empty() {
        let gain = py::sum(known.iter().map(|r| {
            r.cad.unwrap_or(0.0) - unit_cost(cost_of(&r.currency)).unwrap_or(0.0) * r.amount
        }));
        sec.line(format!(
            "- Unrealized gain on assets with a known cost: {}. Selling or swapping crypto is a disposition; {} of a capital gain is taxable.",
            money(gain),
            pct(inclusion)
        ));
    }
    sec.data = Data::Crypto(total);
    Ok(sec)
}
