//! Cash reserve, float and the order new money goes into accounts, as
//! `strategy/reserve.py`.

use super::banking::title;
use super::py::{self, commas, getf, money, obj, pct};
use super::situation::Unported;
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::Json;

const HIGH_MARGINAL: f64 = 0.30;

/// `f"{x:g}"` for the small numbers a profile holds.
pub fn g(x: f64) -> String {
    let s = crate::core::pyjson::float_repr(x);
    s.strip_suffix(".0").map_or(s.clone(), str::to_string)
}

pub fn invest_order(profile: &Json, marginal: Option<f64>) -> Vec<(String, f64, String)> {
    let room = obj(profile, "room");
    let goals = obj(profile, "goals");
    let mut order = Vec::new();
    if goals.get("first_time_home_buyer").is_some_and(Json::truthy) && getf(room, "fhsa", 0.0) > 0.0
    {
        order.push((
            "FHSA".to_string(),
            getf(room, "fhsa", 0.0),
            "deductible now, tax-free out for a first home".to_string(),
        ));
    }
    let (rrsp, tfsa) = (getf(room, "rrsp", 0.0), getf(room, "tfsa", 0.0));
    let rrsp_first = marginal.is_some_and(|m| m >= HIGH_MARGINAL);
    let rrsp_why = match marginal {
        Some(m) if m != 0.0 => format!("deduction at your {} marginal rate", pct(m)),
        _ => "deduction now, taxed later".to_string(),
    };
    let mut pair = vec![
        ("RRSP".to_string(), rrsp, rrsp_why),
        (
            "TFSA".to_string(),
            tfsa,
            "tax-free growth, withdraw any time".to_string(),
        ),
    ];
    if !rrsp_first {
        pair.reverse();
    }
    order.extend(pair.into_iter().filter(|p| p.1 > 0.0));
    order.push((
        "Non-registered".into(),
        f64::INFINITY,
        "after registered room is full".into(),
    ));
    order
}

#[derive(Debug)]
pub struct ReserveData {
    pub emergency: f64,
    pub threshold: f64,
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("reserve", "Cash reserve and where new money goes");
    let spend = sit.monthly_spending();
    if spend <= 0.0 {
        sec.line("- Spending unknown yet: import exports or set `goals.monthly_spending`.");
        return Ok(sec);
    }
    let months = py::num(sit.goal("emergency_months")).unwrap_or(3.0);
    let (emergency, float_) = (spend * months, spend);
    let threshold = emergency + float_;
    sec.line(format!("- Monthly spending: {}.", money(spend)));
    sec.line(format!(
        "- Emergency fund: **{}** ({} months). Keep it in cash or a high-interest account.",
        money(emergency),
        g(months)
    ));
    sec.line(format!(
        "- Monthly float: {} so card payments never bounce.",
        money(float_)
    ));
    sec.line(format!(
        "- Invest anything above **{}** of total cash.",
        money(threshold)
    ));
    if let Some(cash) = sit.cash_balance()? {
        let extra = cash - threshold;
        if extra > 0.0 {
            sec.line(format!(
                "- Cash now: {} → **{} can be invested**.",
                money(cash),
                money(extra)
            ));
            sec.tasks.push(
                Task::new(
                    "sweep-cash",
                    format!("Invest {} of cash above the reserve", money(extra)),
                )
                .due(sit.today)
                .notes("Follow the account order in your strategy. You make the transfer.")
                .risk("money"),
            );
        } else {
            sec.line(format!(
                "- Cash now: {}, {} below reserve + float: keep building cash first.",
                money(cash),
                money(-extra)
            ));
        }
        for row in sit.rows_of("cash")? {
            let name = format!("{} {}", title(&row.service), row.currency);
            if row.covered_by_summary {
                sec.line(format!(
                    "  - {name}: already in your MoneyWiz accounts, not counted twice."
                ));
            } else if row.cad.is_none() {
                sec.line(format!(
                    "  - {name}: no CAD rate in knowledge/data/foreign_cash.toml, not counted."
                ));
            } else if row.currency == "CAD" {
                sec.line(format!(
                    "  - Includes {name}: {}.",
                    money(row.cad.unwrap_or(0.0))
                ));
            } else {
                sec.line(format!(
                    "  - Includes {name}: {} {} = {} CAD (Bank of Canada {:.4}, {}).",
                    commas(row.amount, 2),
                    row.currency,
                    money(row.cad.unwrap_or(0.0)),
                    row.rate.unwrap_or(0.0),
                    row.rate_date.py_str()
                ));
            }
        }
    }
    let marginal = match ctx.result("tax").map(|s| &s.data) {
        Some(Data::Tax(t)) => Some(t.marginal),
        _ => None,
    };
    sec.line("");
    sec.line("Order for new money:");
    for (i, (acct, room, why)) in invest_order(&sit.profile, marginal).iter().enumerate() {
        let room_text = if room.is_infinite() {
            String::new()
        } else {
            format!(" (room {})", money(*room))
        };
        sec.line(format!("{}. {acct}{room_text}: {why}.", i + 1));
    }
    if sit.goal("first_time_home_buyer").is_some_and(Json::truthy) {
        sec.line(
            "- Down-payment money needed within about 2 years stays in cash or GICs, not stocks. \
             The RRSP Home Buyers' Plan needs contributions to sit 90 days before withdrawal.",
        );
    }
    sec.data = Data::Reserve(ReserveData {
        emergency,
        threshold,
    });
    Ok(sec)
}
