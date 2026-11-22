//! Foreign-currency cash: what the Wise USD (and other) balances are worth in
//! CAD and what fits them, as `strategy/foreign.py`.

use super::banking::title;
use super::py::{self, commas, getf, gets, list, money, obj, pct};
use super::situation::Unported;
use super::{Ctx, DISCLAIMER, Section, Task};
use crate::core::pyjson::Json;
use chrono::{Datelike, NaiveDate};

pub struct Conversion {
    pub wise: f64,
    pub norbert: f64,
    pub breakeven: f64,
    pub cheaper: &'static str,
}

/// Cost of converting `cad` worth of foreign cash at Wise vs. with Norbert's gambit.
pub fn conversion(cad: f64, wise_rate: f64, norbert: &Json) -> Conversion {
    let (fixed, spread) = (
        getf(norbert, "fixed_cad", 0.0),
        getf(norbert, "spread_rate", 0.0),
    );
    let (wise, gambit) = (cad * wise_rate, fixed + cad * spread);
    let breakeven = if wise_rate > spread {
        fixed / (wise_rate - spread)
    } else {
        f64::INFINITY
    };
    Conversion {
        wise,
        norbert: gambit,
        breakeven,
        cheaper: if gambit < wise {
            "Norbert's gambit"
        } else {
            "Wise"
        },
    }
}

pub fn day(v: &Json) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&v.py_str(), "%Y-%m-%d").ok()
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("foreign", "Foreign cash: Wise USD and other currencies");
    let rows = sit.rows_of("cash")?;
    if rows.is_empty() {
        sec.line("- No foreign-currency cash found. Run `bin/cashcast api balances wise` to read your Wise balances.");
        return Ok(sec);
    }
    let facts = sit.knowledge("foreign_cash")?;
    sec.line(format!("*{DISCLAIMER}*"));
    sec.line("");
    sec.line("| Balance | Amount | CAD per unit | In CAD | Counted in cash |");
    sec.line("|---|---|---|---|---|");
    let mut foreign_cad = 0.0;
    for r in &rows {
        let rate = match r.rate {
            Some(x) => format!("{x:.4} ({})", r.rate_date.py_str()),
            None => "no rate".into(),
        };
        let cad = r.cad.map_or("—".to_string(), money);
        let counted = if r.counted {
            "yes"
        } else if r.covered_by_summary {
            "in MoneyWiz already"
        } else {
            "no"
        };
        sec.line(format!(
            "| {} {} | {} {} | {rate} | {cad} | {counted} |",
            title(&r.service),
            r.currency,
            commas(r.amount, 2),
            r.currency
        ));
        if r.currency != "CAD"
            && let Some(c) = r.cad
        {
            foreign_cad += c;
        }
    }
    let max_age = py::num(facts.get("max_age_days")).unwrap_or(45.0) as i64;
    if let Some(oldest) = rows.iter().filter_map(|r| day(&r.rate_date)).min()
        && (sit.today - oldest).num_days() > max_age
    {
        sec.line(format!(
            "\n- The rates are older than {max_age} days: refresh `knowledge/data/foreign_cash.toml` from the Bank of Canada."
        ));
    }
    if foreign_cad == 0.0 {
        return Ok(sec);
    }
    sec.line("");
    sec.line("**What fits US-dollar cash:**");
    sec.line(format!(
        "- Keep it in USD for USD spending. {}",
        gets(obj(&facts, "keep_usd"), "why", "")
    ));
    let rrsp_room = getf(obj(&sit.profile, "room"), "rrsp", 0.0);
    let room = if rrsp_room != 0.0 {
        format!(" You have {} of RRSP room.", money(rrsp_room))
    } else {
        String::new()
    };
    sec.line(format!(
        "- US-listed ETFs in an RRSP can be bought with these dollars directly, with no conversion. {}{room}",
        gets(obj(&facts, "rrsp_us_withholding"), "why", "")
    ));
    let services = sit.knowledge("payment_services")?;
    let wise_fee = list(&services, "services")
        .iter()
        .find(|s| s.get("id").is_some_and(|i| i.py_str() == "wise"))
        .map_or(0.0, |s| getf(s, "fee_rate", 0.0));
    let norbert = obj(&facts, "norbert");
    let c = conversion(foreign_cad, wise_fee, norbert);
    sec.line("");
    sec.line(format!(
        "**If you convert it to CAD** ({}):",
        money(foreign_cad)
    ));
    sec.line(format!(
        "- Wise: about {} ({} fee), same day.",
        money(c.wise),
        pct(wise_fee)
    ));
    sec.line(format!(
        "- Norbert's gambit: about {} (trades and journal fee about {}, spread {}), {} business days. {}",
        money(c.norbert),
        money(getf(norbert, "fixed_cad", 0.0)),
        pct(getf(norbert, "spread_rate", 0.0)),
        norbert.get("days").map_or("3".to_string(), Json::py_str),
        gets(norbert, "why", "")
    ));
    sec.line(format!(
        "- Break-even: Norbert's gambit is cheaper above about {}. For this amount: **{}**.",
        money(c.breakeven),
        c.cheaper
    ));
    let t1135 = obj(&facts, "t1135");
    let threshold = getf(t1135, "threshold_cad", 100000.0);
    let empty = Json::Obj(Vec::new());
    let nonreg = py::sum(
        list(sit.holdings.as_ref().unwrap_or(&empty), "assets")
            .iter()
            .filter(|a| gets(a, "account_bucket", "non-registered") == "non-registered")
            .map(|a| getf(a, "acb_cad", 0.0)),
    );
    sec.line("");
    sec.line("**Tax checks:**");
    sec.line(format!(
        "- T1135: foreign cash is {} of the {} threshold ({}). {} {}",
        money(foreign_cad),
        money(threshold),
        pct(foreign_cad / threshold),
        gets(t1135, "why", ""),
        gets(t1135, "check", "")
    ));
    if nonreg != 0.0 {
        sec.line(format!(
            "  - With non-registered holdings at cost ({}, an upper bound if all were foreign): {}.",
            money(nonreg),
            money(foreign_cad + nonreg)
        ));
    }
    if foreign_cad + nonreg > threshold {
        sec.tasks.push(
            Task::new("check-t1135", "Check whether Form T1135 is due (foreign property may be above $100,000 CAD cost)")
                .due(NaiveDate::from_ymd_opt(sit.today.year() + 1, 4, 30).expect("valid"))
                .notes("Count only specified foreign property: foreign cash and foreign shares outside RRSP, RRIF and TFSA."),
        );
    }
    sec.line(format!(
        "- Interest: {}",
        gets(obj(&facts, "foreign_interest"), "why", "")
    ));
    Ok(sec)
}
