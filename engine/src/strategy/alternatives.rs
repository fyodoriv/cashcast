//! Alternatives finder: cheaper or better-paying options than what you use
//! now, as `strategy/alternatives.py`.

use super::banking::find_connector;
use super::cashback::{Card, optimize, rent_service, to_card, wallet};
use super::py::{self, getf, gets, list, money, obj, pct};
use super::situation::Unported;
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::Json;

/// `re.search(r"\b(rent|mortgage|housing)\b", s, re.I)`.
fn housing(s: &str) -> bool {
    let lower = s.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    for w in ["rent", "mortgage", "housing"] {
        let wc: Vec<char> = w.chars().collect();
        for i in 0..chars.len().saturating_sub(wc.len() - 1) {
            if chars[i..i + wc.len()] == wc[..]
                && (i == 0 || !word(chars[i - 1]))
                && chars.get(i + wc.len()).is_none_or(|c| !word(*c))
            {
                return true;
            }
        }
    }
    false
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("alternatives", "Alternatives finder");
    let spend = sit.annual_by_reward_category();
    let cards = wallet(sit)?;
    if spend.is_empty() {
        sec.line("- Needs spending data or `[spending]` estimates.");
        return Ok(sec);
    }
    let service = rent_service(sit)?;
    let base = optimize(&spend, &cards, service.as_ref());
    let income = py::sum(match obj(&sit.profile, "income") {
        Json::Obj(items) => items
            .iter()
            .filter_map(|(_, v)| match v {
                Json::Int(i) => Some(*i as f64),
                Json::Float(f) => Some(*f),
                Json::Bool(b) => Some(*b as i64 as f64),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    });
    let hold: Vec<&str> = cards.iter().map(|c| c.id.as_str()).collect();
    let no_new_credit = sit.goal("no_new_credit").is_some_and(Json::truthy)
        || sit
            .goal("mortgage_in_months")
            .is_some_and(|m| py::num(Some(m)).unwrap_or(0.0) as i64 <= 12);
    let threshold = py::num(sit.goal("simplicity_threshold")).unwrap_or(50.0);
    let catalog = sit.knowledge("credit_cards")?;
    let mut adds: Vec<(f64, &Json, bool)> = Vec::new();
    for entry in list(&catalog, "cards") {
        if hold.contains(&gets(entry, "id", "").as_str()) {
            continue;
        }
        let card = to_card(entry, Some(&spend))?;
        let mut with: Vec<Card> = cards.clone();
        with.push(card);
        let gain = optimize(&spend, &with, service.as_ref()).net() - base.net();
        let eligible = getf(entry, "min_income_personal", 0.0) <= income || income == 0.0;
        if gain > threshold {
            adds.push((gain, entry, eligible));
        }
    }
    adds.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if no_new_credit && !adds.is_empty() {
        sec.line("**Verdict: wait.** Your goals say no new credit now (mortgage or credit-building), so skip these for now; the top two are listed for later.");
        sec.line("");
    }
    sec.line(format!(
        "**Cards that would add the most net cash back a year** (only gains above your \
         {} simplicity threshold; smaller ones are not worth the complexity):",
        money(threshold)
    ));
    if adds.is_empty() {
        sec.line("- None.");
    }
    // Under a wait verdict, two cards are enough to plan with later.
    let shown = if no_new_credit { 2 } else { 5 };
    for (gain, entry, eligible) in adds.iter().take(shown) {
        let mut notes = Vec::new();
        if !eligible {
            notes.push(format!(
                "needs {} personal income",
                money(getf(entry, "min_income_personal", 0.0))
            ));
        }
        if entry
            .get("confidence")
            .is_some_and(|c| c.py_str() == "estimate")
        {
            notes.push("terms are estimates; verify".into());
        }
        sec.line(format!(
            "- {}: +{} a year (fee {}){}",
            entry.get("name").map_or("None".into(), Json::py_str),
            money(*gain),
            money(getf(entry, "annual_fee", 0.0)),
            if notes.is_empty() {
                String::new()
            } else {
                format!(" — {}", notes.join("; "))
            }
        ));
    }
    sec.line("");
    sec.line("**Cards that cost more than they earn you:**");
    let mut weak = Vec::new();
    for card in &cards {
        let others: Vec<Card> = cards.iter().filter(|c| c.id != card.id).cloned().collect();
        let contribution = base.net() - optimize(&spend, &others, service.as_ref()).net();
        if card.net_fee() > 0.0 && contribution < 0.0 {
            weak.push((card, contribution));
        }
    }
    if weak.is_empty() {
        sec.line("- None.");
    }
    for (card, contribution) in weak {
        sec.line(format!(
            "- {}: {} a year after its {} fee → product-change to a \
             no-fee card from the same bank (keeps the account age and limit); redeem its cash back first.",
            card.name,
            money(contribution),
            money(card.net_fee())
        ));
        let mut t = Task::new(
            format!("downgrade-{}", card.id),
            format!(
                "Product-change {} to a no-fee card before its annual fee posts",
                card.name
            ),
        )
        .notes("Redeem the cash back first. Call the issuer; keep the same limit.")
        .risk("settings");
        t.service = find_connector(&gets(&card.raw, "issuer", ""))?;
        t.action = "product-change-card".into();
        sec.tasks.push(t);
    }
    let get = |k: &str| spend.iter().find(|(c, _)| c == k).map(|(_, v)| *v);
    if let Some(svc) = &service
        && svc
            .get("membership_rate")
            .is_some_and(|v| !matches!(v, Json::Null))
        && let Some(rent) = get("rent").filter(|r| *r != 0.0)
    {
        let (std, mem) = (
            getf(svc, "fee_rate", 0.0),
            getf(svc, "membership_rate", 0.0),
        );
        let member_fee = getf(svc, "membership_fee", 0.0);
        let saving = rent * (std - mem) - member_fee;
        sec.line("");
        sec.line(format!(
            "**{} plan:** the membership costs {} a year and saves {} in fees, so you {} {} a year (before any perks like credit reporting).",
            svc.get("name").or_else(|| svc.get("id")).map_or("None".into(), Json::py_str),
            money(member_fee),
            money(rent * (std - mem)),
            if saving >= 0.0 { "come out ahead by" } else { "lose" },
            money(saving.abs())
        ));
    }
    let foreign = get("foreign").unwrap_or(0.0);
    if foreign != 0.0 {
        let fx = cards
            .iter()
            .map(|c| c.fx_fee)
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
            .unwrap_or(0.025);
        if fx > 0.0 {
            sec.line("");
            sec.line(format!(
                "**Foreign currency:** your best card charges {} FX → a 0% FX card or Wise saves about {} a year on {} of foreign spending.",
                pct(fx),
                money(foreign * fx),
                money(foreign)
            ));
        }
    }
    let pay = match ctx.result("payments").map(|s| &s.data) {
        Some(Data::Payments(p)) => Some(p),
        _ => None,
    };
    if let Some(p) = pay.filter(|p| !p.rows.is_empty()) {
        let fees: Vec<_> = p.rows.iter().filter(|r| r.avoidable).collect();
        if !fees.is_empty() {
            sec.line("");
            sec.line(format!(
                "**Fees:** {} — about {} a year that a no-fee account or on-time payments would remove.",
                fees.iter().map(|r| fee_label(&r.category)).collect::<Vec<_>>().join(", "),
                money(py::sum(fees.iter().map(|r| r.avg)) * 12.0)
            ));
        }
    }
    let recurring: Vec<&Json> = match pay.and_then(|p| p.recurring.as_ref()) {
        Some(Json::List(items)) => items
            .iter()
            .filter(|r| !housing(&gets(r, "category", "")))
            .collect(),
        _ => Vec::new(),
    };
    if !recurring.is_empty() {
        let cents = |r: &Json| py::num(r.get("yearly_cents")).unwrap_or(0.0);
        let total = recurring.iter().map(|r| cents(r) as i64).sum::<i64>() as f64 / 100.0;
        sec.line("");
        sec.line(format!(
            "**Recurring payments to review** ({} a year in total):",
            money(total)
        ));
        let s = |r: &Json, k: &str| r.get(k).map_or("None".into(), Json::py_str);
        for r in &recurring {
            sec.line(format!(
                "- Review {}: {}, {}, {} a year. Still used? Keep it, move to a cheaper or shared plan, or cancel it.",
                s(r, "label"),
                s(r, "category"),
                s(r, "cadence"),
                money(cents(r) / 100.0)
            ));
        }
        sec.tasks.push(
            Task::new(
                format!("recurring-review-{}", sit.today.format("%Y-%m")),
                format!(
                    "Review {} recurring payments (about {} a year) and cancel the ones you no longer use",
                    recurring.len(),
                    money(total)
                ),
            )
            .due(sit.today)
            .notes("Your private data/review.md shows which payee each 'Payments #n' is. You cancel with each merchant.")
            .risk("read")
            .recurs("month"),
        );
    }
    Ok(sec)
}

/// "Fees > Overdraft" reads as "Overdraft fees"; other paths keep their last part.
fn fee_label(category: &str) -> String {
    match category.rsplit_once(" > ") {
        Some(("Fees", leaf)) => format!("{leaf} fees"),
        _ => category.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::fee_label;

    #[test]
    fn fee_label_drops_the_repeated_fees_group() {
        assert_eq!(fee_label("Fees > Overdraft"), "Overdraft fees");
        assert_eq!(fee_label("Bank > Service"), "Bank > Service");
        assert_eq!(fee_label("Fees"), "Fees");
    }
}
