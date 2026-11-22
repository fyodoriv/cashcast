//! Interest on cash: what each balance earns a year, where idle cash earns
//! the most, and the tax on it, as `strategy/interest.py`.

use super::banking::{accounts, title};
use super::py::{getf, gets, list, money, obj, pct};
use super::situation::{Situation, Unported};
use super::{Ctx, Data, Section};
use crate::core::pyjson::Json;

pub struct Place {
    pub name: String,
    pub cad: f64,
    pub rate: f64,
    pub keep: bool,
    pub currency: String,
    pub id: Option<Json>,
}

/// Every place cash sits, with its CAD balance and annual rate. API rows replace a bank account of the same service.
pub fn places(sit: &Situation) -> Result<Vec<Place>, Unported> {
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for r in sit.rows_of("cash")? {
        let Some(cad) = r.cad else { continue };
        if r.covered_by_summary {
            continue;
        }
        seen.push(r.service.clone());
        out.push(Place {
            name: format!("{} {}", title(&r.service), r.currency),
            cad,
            rate: sit.interest_rate(&r.service, &r.currency)?,
            keep: false,
            currency: r.currency.clone(),
            id: None,
        });
    }
    for a in accounts(sit)? {
        if seen.contains(&gets(&a, "bank", "").to_lowercase())
            || !a.get("keep").is_some_and(Json::truthy)
        {
            continue;
        }
        let name = match a.get("name") {
            Some(n) => n.py_str(),
            None => a.get("id").map_or("None".into(), Json::py_str),
        };
        out.push(Place {
            name: format!("{} {name}", gets(&a, "bank", ""))
                .trim()
                .to_string(),
            cad: getf(&a, "keep", 0.0),
            rate: getf(&a, "interest_rate", 0.0),
            keep: true,
            currency: "CAD".into(),
            id: Some(a.get("id").cloned().unwrap_or(Json::Null)),
        });
    }
    Ok(out)
}

/// Python's `max(..., key=...)`: the first item with the largest key.
fn first_max<T>(items: impl IntoIterator<Item = T>, key: impl Fn(&T) -> f64) -> Option<T> {
    let mut best: Option<(f64, T)> = None;
    for it in items {
        let k = key(&it);
        if best.as_ref().is_none_or(|(b, _)| k > *b) {
            best = Some((k, it));
        }
    }
    best.map(|(_, t)| t)
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("interest", "Interest on cash: what each balance earns");
    let rows = places(sit)?;
    if rows.is_empty() {
        sec.line(
            "- No cash places yet: add `[[banking.accounts]]` with a `keep` to profile.toml, or run `bin/cashcast api balances wise`.",
        );
        return Ok(sec);
    }
    sec.line("| Where | Balance (CAD) | Rate | Interest a year |");
    sec.line("|---|---|---|---|");
    let mut total = 0.0;
    for r in &rows {
        let yearly = r.cad * r.rate;
        total += yearly;
        let basis = if r.keep { " (keep)" } else { "" };
        sec.line(format!(
            "| {} | {}{basis} | {} | {} |",
            r.name,
            money(r.cad),
            pct(r.rate),
            money(yearly)
        ));
    }
    sec.line("");
    sec.line(format!(
        "- Interest a year at today's rates: **{}**.",
        money(total)
    ));
    let cad: Vec<&Place> = rows.iter().filter(|r| r.currency == "CAD").collect();
    let best = first_max(cad.iter().copied(), |r| r.rate);
    if let Some(b) = best.filter(|b| b.rate > 0.0) {
        sec.line(format!(
            "- Highest CAD rate you hold: **{} at {}**. Idle cash above each keep earns the most there.",
            b.name,
            pct(b.rate)
        ));
        if let Some(Data::Reserve(r)) = ctx.result("reserve").map(|s| &s.data)
            && r.emergency != 0.0
        {
            sec.line(format!(
                "- Your emergency fund ({}) there earns about {} a year.",
                money(r.emergency),
                money(r.emergency * b.rate)
            ));
        }
        let hub_id = obj(&sit.profile, "transfers")
            .get("hub")
            .cloned()
            .unwrap_or(Json::Null);
        let hub = cad
            .iter()
            .find(|r| r.id.clone().unwrap_or(Json::Null) == hub_id);
        if let Some(h) = hub.filter(|h| h.rate < b.rate) {
            sec.line(format!(
                "- Your transfer hub, {}, earns {}. {} earns {}: consider it as the hub, or move idle cash there (advice only; you move the money).",
                h.name,
                pct(h.rate),
                b.name,
                pct(b.rate)
            ));
        }
    }
    let banks = sit.knowledge("banks")?;
    let catalog = first_max(
        list(&banks, "accounts")
            .iter()
            .filter(|a| getf(a, "waive_min_balance", 0.0) == 0.0),
        |a| getf(a, "interest_rate", 0.0),
    );
    if let (Some(c), Some(b)) = (catalog, best)
        && getf(c, "interest_rate", 0.0) > b.rate
    {
        sec.line(format!(
            "- Best no-minimum rate in the catalog: {} {} at {}. {}",
            c.get("bank").map_or("None".into(), Json::py_str),
            c.get("name").map_or("None".into(), Json::py_str),
            pct(getf(c, "interest_rate", 0.0)),
            gets(c, "notes", "")
        ));
    }
    let facts = sit.knowledge("foreign_cash")?;
    if rows.iter().any(|r| !r.keep && r.rate != 0.0) {
        sec.line(format!(
            "- {}",
            gets(obj(obj(&facts, "interest"), "wise"), "why", "")
        ));
    }
    sec.line(format!(
        "- Tax: {} CAD interest goes on the same line.",
        gets(obj(&facts, "foreign_interest"), "why", "")
    ));
    sec.data = Data::Interest(total);
    Ok(sec)
}
