//! Cash back routing: which card to use for each kind of spending, as
//! `strategy/cashback.py`.
//!
//! The optimizer is greedy by regret: each step serves the category that
//! loses the most if it misses its best card, up to each card's caps, then
//! sends the rest to the best rate after caps. Net rates include FX fees on
//! foreign spending and the rent service's fee on rent.

use super::py::{self, getf, gets, list, money, obj, pct};
use super::situation::{Situation, Unported};
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::Json;

#[derive(Clone, Debug)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub earn: Vec<(String, f64)>,
    pub caps: Vec<Json>,
    pub annual_fee: f64,
    pub fee_rebated: bool,
    pub fx_fee: f64,
    pub raw: Json,
}

impl Card {
    fn has(&self, cat: &str) -> bool {
        self.earn.iter().any(|(c, _)| c == cat)
    }
    pub fn rate(&self, cat: &str) -> f64 {
        let get = |k: &str| self.earn.iter().find(|(c, _)| c == k).map(|(_, v)| *v);
        get(cat).or_else(|| get("other")).unwrap_or(0.0)
    }
    pub fn net_fee(&self) -> f64 {
        if self.fee_rebated {
            0.0
        } else {
            self.annual_fee
        }
    }
    fn cap_cats(&self, i: usize) -> Vec<String> {
        list(&self.caps[i], "categories")
            .iter()
            .map(Json::py_str)
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct Allocation {
    pub category: String,
    pub card: String,
    pub amount: f64,
    pub rate: f64,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub allocations: Vec<Allocation>,
    pub gross: f64,
    pub card_fees: f64,
    pub service_fees: f64,
    pub membership_fees: f64,
    pub bank_paid: Vec<(String, f64)>,
    /// Card id → name, as the web app shows the routes (set by `build`).
    pub names: Vec<(String, String)>,
}

impl Plan {
    pub fn net(&self) -> f64 {
        self.gross - self.card_fees - self.service_fees - self.membership_fees
    }
}

/// Python `repr(str)` for the simple ids a profile holds.
fn repr(v: Option<&Json>) -> String {
    match v {
        None | Some(Json::Null) => "None".into(),
        Some(Json::Str(s)) if s.contains('\'') && !s.contains('"') => format!("\"{s}\""),
        Some(Json::Str(s)) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Some(other) => other.py_str(),
    }
}

/// `dict.update`: later keys replace earlier ones in place, new ones go last.
pub fn update(base: &mut Vec<(String, Json)>, extra: &Json) {
    if let Json::Obj(items) = extra {
        for (k, v) in items {
            match base.iter_mut().find(|(x, _)| x == k) {
                Some(slot) => slot.1 = v.clone(),
                None => base.push((k.clone(), v.clone())),
            }
        }
    }
}

pub fn wallet(sit: &Situation) -> Result<Vec<Card>, Unported> {
    let catalog = sit.knowledge("credit_cards")?;
    let mut cards = Vec::new();
    for entry in list(obj(&sit.profile, "wallet"), "cards") {
        let id = entry.get("id");
        let mut base: Vec<(String, Json)> = match list(&catalog, "cards")
            .iter()
            .rev()
            .find(|c| c.get("id") == id)
        {
            Some(Json::Obj(items)) => items.clone(),
            _ => Vec::new(),
        };
        update(&mut base, entry);
        let base = Json::Obj(base);
        if base.get("earn").is_none() {
            sit.note(format!(
                "card {} is not in the knowledge catalog; add its earn rates in profile.toml",
                repr(id)
            ));
            continue;
        }
        let picks = base.get("pick_categories").is_some_and(Json::truthy);
        let spend = if picks {
            Some(sit.annual_by_reward_category())
        } else {
            None
        };
        cards.push(to_card(&base, spend.as_deref())?);
    }
    Ok(cards)
}

pub fn to_card(d: &Json, spend: Option<&[(String, f64)]>) -> Result<Card, Unported> {
    let mut earn: Vec<(String, f64)> = match d.get("earn") {
        Some(Json::Obj(items)) => items
            .iter()
            .map(|(k, v)| (k.clone(), py::num(Some(v)).unwrap_or(0.0)))
            .collect(),
        _ => return Err(Unported("earn is not a table".into())),
    };
    let picks = py::num(d.get("pick_categories")).unwrap_or(0.0) as usize;
    if picks > 0
        && let Some(spend) = spend
    {
        let base = earn
            .iter()
            .find(|(c, _)| c == "other")
            .map_or(0.0, |(_, v)| *v);
        let get = |c: &str| spend.iter().find(|(k, _)| k == c).map_or(0.0, |(_, v)| *v);
        let mut bonus: Vec<(String, f64)> = earn
            .iter()
            .filter(|(c, v)| c != "other" && *v > base)
            .cloned()
            .collect();
        let key = |c: &(String, f64)| -get(&c.0) * c.1;
        bonus.sort_by(|a, b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (c, _) in bonus.iter().skip(picks) {
            earn.retain(|(k, _)| k != c);
        }
    }
    let id = gets(d, "id", "");
    Ok(Card {
        name: d.get("name").map_or(id.clone(), Json::py_str),
        id,
        earn,
        caps: list(d, "caps").to_vec(),
        annual_fee: getf(d, "annual_fee", 0.0),
        fee_rebated: d.get("fee_rebated").is_some_and(Json::truthy),
        fx_fee: getf(d, "fx_fee", 0.0),
        raw: d.clone(),
    })
}

pub fn rent_service(sit: &Situation) -> Result<Option<Json>, Unported> {
    let rent = obj(&sit.profile, "rent");
    let Some(sid) = rent.get("service").filter(|v| v.truthy()) else {
        return Ok(None);
    };
    let pick = |keys: &[&str]| -> Json {
        match rent {
            Json::Obj(items) => Json::Obj(
                items
                    .iter()
                    .filter(|(k, _)| keys.contains(&k.as_str()))
                    .cloned()
                    .collect(),
            ),
            _ => Json::Obj(Vec::new()),
        }
    };
    let services = sit.knowledge("payment_services")?;
    for s in list(&services, "services") {
        if s.get("id") == Some(sid) {
            let mut out = match s {
                Json::Obj(items) => items.clone(),
                _ => Vec::new(),
            };
            update(
                &mut out,
                &pick(&["fee_rate", "membership_fee", "membership_rate", "codes_as"]),
            );
            return Ok(Some(Json::Obj(out)));
        }
    }
    let mut own = vec![
        ("id".to_string(), sid.clone()),
        ("fee_rate".into(), Json::Float(0.0)),
        ("membership_fee".into(), Json::Float(0.0)),
        ("codes_as".into(), Json::Str("other".into())),
    ];
    update(
        &mut own,
        &pick(&[
            "fee_rate",
            "membership_fee",
            "membership_rate",
            "codes_as",
            "name",
        ]),
    );
    Ok(Some(Json::Obj(own)))
}

type CapKey = (String, usize, String);

/// Routes annual `spend` per reward category across `cards`.
pub fn optimize(spend: &[(String, f64)], cards: &[Card], service: Option<&Json>) -> Plan {
    let service_rate = service.map_or(0.0, |s| getf(s, "fee_rate", 0.0));
    let codes_as = service.map_or("other".to_string(), |s| gets(s, "codes_as", "other"));
    let membership = service.map_or(0.0, |s| getf(s, "membership_fee", 0.0));

    let effective = |card: &Card, cat: &str| -> (String, f64) {
        if cat == "rent" {
            let counted = if card.has("rent") {
                "rent".to_string()
            } else {
                codes_as.clone()
            };
            let r = card.rate(&counted) - service_rate;
            return (counted, r);
        }
        if cat == "foreign" {
            return ("foreign".into(), card.rate("foreign") - card.fx_fee);
        }
        (cat.to_string(), card.rate(cat))
    };
    let cap_key = |card: &Card, i: usize, cat: &str| -> CapKey {
        let per = card.caps[i].get("per_category").is_some_and(Json::truthy);
        (
            card.id.clone(),
            i,
            if per { cat.to_string() } else { "*".into() },
        )
    };
    let mut room: Vec<(CapKey, f64)> = Vec::new();
    for card in cards {
        for i in 0..card.caps.len() {
            for cat in card.cap_cats(i) {
                let k = cap_key(card, i, &cat);
                if !room.iter().any(|(x, _)| *x == k) {
                    room.push((k, getf(&card.caps[i], "limit", 0.0)));
                }
            }
        }
    }
    let caps_for = |card: &Card, counted: &str| -> Vec<usize> {
        (0..card.caps.len())
            .filter(|&i| card.cap_cats(i).iter().any(|c| c == counted))
            .collect()
    };
    let after_cap = |cat: &str| -> (Option<&Card>, f64) {
        let (mut best, mut best_rate) = (None, 0.0);
        for card in cards {
            let (counted, mut rate) = effective(card, cat);
            let idx = caps_for(card, &counted);
            if !idx.is_empty() {
                let after = idx
                    .iter()
                    .map(|&i| getf(&card.caps[i], "after_rate", card.rate("other")))
                    .fold(f64::INFINITY, f64::min);
                rate = after
                    - if cat == "rent" {
                        service_rate
                    } else if cat == "foreign" {
                        card.fx_fee
                    } else {
                        0.0
                    };
            }
            if rate > best_rate {
                best = Some(card);
                best_rate = rate;
            }
        }
        (best, best_rate)
    };
    let room_of = |room: &[(CapKey, f64)], card: &Card, counted: &str| -> f64 {
        let idx = caps_for(card, counted);
        if idx.is_empty() {
            return f64::INFINITY;
        }
        idx.iter()
            .map(|&i| {
                let k = cap_key(card, i, counted);
                room.iter().find(|(x, _)| *x == k).map_or(0.0, |(_, v)| *v)
            })
            .fold(f64::INFINITY, f64::min)
    };

    let mut left: Vec<(String, f64)> = spend
        .iter()
        .filter(|(c, v)| c != "bank_only" && *v > 0.0)
        .cloned()
        .collect();
    let mut allocations: Vec<Allocation> = Vec::new();
    loop {
        let mut step: Option<((f64, f64), String, &Card, String, f64)> = None;
        let mut cats: Vec<String> = left.iter().map(|(c, _)| c.clone()).collect();
        cats.sort();
        for cat in cats {
            let want = left
                .iter()
                .find(|(c, _)| *c == cat)
                .map_or(0.0, |(_, v)| *v);
            if want <= 0.0 {
                continue;
            }
            let mut options: Vec<(f64, &str, &Card, String)> = Vec::new();
            for card in cards {
                let (counted, rate) = effective(card, &cat);
                if rate > 0.0 && room_of(&room, card, &counted) > 0.0 {
                    options.push((rate, card.id.as_str(), card, counted));
                }
            }
            if options.is_empty() {
                continue;
            }
            options.sort_by(|a, b| {
                b.0.partial_cmp(&a.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.cmp(b.1))
            });
            let (rate, _, card, counted) = options[0].clone();
            let (mut need, mut land) = (want, after_cap(&cat).1);
            for (r, _, c, cc) in &options[1..] {
                let rm = room_of(&room, c, cc);
                if rm >= need {
                    land = r.max(land);
                    break;
                }
                need -= rm;
            }
            let key = (rate - land, rate);
            let better = match &step {
                None => true,
                Some((k, ..)) => key.0 > k.0 || (key.0 == k.0 && key.1 > k.1),
            };
            if better {
                step = Some((key, cat.clone(), card, counted, rate));
            }
        }
        let Some((_, cat, card, counted, rate)) = step else {
            break;
        };
        let slot = left
            .iter_mut()
            .find(|(c, _)| *c == cat)
            .expect("category in left");
        let amount = slot.1.min(room_of(&room, card, &counted));
        for i in caps_for(card, &counted) {
            let k = cap_key(card, i, &counted);
            if let Some(r) = room.iter_mut().find(|(x, _)| *x == k) {
                r.1 -= amount;
            }
        }
        let slot = left
            .iter_mut()
            .find(|(c, _)| *c == cat)
            .expect("category in left");
        slot.1 -= amount;
        allocations.push(Allocation {
            category: cat,
            card: card.id.clone(),
            amount,
            rate,
        });
    }
    for i in 0..left.len() {
        let (cat, amount) = left[i].clone();
        if amount <= 0.0 {
            continue;
        }
        let (best, best_rate) = after_cap(&cat);
        if let Some(best) = best {
            allocations.push(Allocation {
                category: cat,
                card: best.id.clone(),
                amount,
                rate: best_rate,
            });
            left[i].1 = 0.0;
        }
    }
    let mut bank_paid: Vec<(String, f64)> =
        left.iter().filter(|(_, v)| *v > 0.0).cloned().collect();
    if let Some((_, v)) = spend
        .iter()
        .find(|(c, _)| c == "bank_only")
        .filter(|(_, v)| *v != 0.0)
    {
        bank_paid.push(("bank_only".into(), *v));
    }
    let rent_on_cards = py::sum(
        allocations
            .iter()
            .filter(|a| a.category == "rent")
            .map(|a| a.amount),
    );
    Plan {
        gross: py::sum(allocations.iter().map(|a| {
            a.amount
                * (a.rate
                    + if a.category == "rent" {
                        service_rate
                    } else {
                        0.0
                    })
        })),
        card_fees: py::sum(cards.iter().map(Card::net_fee)),
        service_fees: rent_on_cards * service_rate,
        membership_fees: if rent_on_cards != 0.0 {
            membership
        } else {
            0.0
        },
        allocations,
        bank_paid,
        names: Vec::new(),
    }
}

/// How many monthly rent charges each card takes, in the order to use them.
pub fn rent_charges(plan: &Plan, monthly_charge: f64) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for a in &plan.allocations {
        if a.category == "rent" && monthly_charge > 0.0 {
            let n = py::round0(a.amount / monthly_charge);
            if n != 0 {
                out.push((a.card.clone(), n));
            }
        }
    }
    out
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("cashback", "Cash back: where to pay");
    let cards = wallet(sit)?;
    let spend = sit.annual_by_reward_category();
    if cards.is_empty() {
        sec.line(
            "- No cards in `[wallet]` of profile.toml yet. Add the cards you hold (catalog ids are in `knowledge/data/credit_cards.toml`).",
        );
        return Ok(sec);
    }
    if spend.is_empty() {
        sec.line(
            "- No spending yet: import exports or add `[spending]` estimates to profile.toml.",
        );
        return Ok(sec);
    }
    let service = rent_service(sit)?;
    let mut plan = optimize(&spend, &cards, service.as_ref());
    plan.names = cards
        .iter()
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect();
    let name_of = |id: &str| {
        cards
            .iter()
            .rev()
            .find(|c| c.id == id)
            .map_or(String::new(), |c| c.name.clone())
    };
    let single = cards
        .iter()
        .map(|c| optimize(&spend, std::slice::from_ref(c), service.as_ref()).net())
        .fold(f64::NEG_INFINITY, f64::max);
    if single > 0.0 {
        ctx.events.push((
            "cashback_scored".into(),
            vec![
                (
                    "ratio".into(),
                    Json::Float(py::round(plan.net() / single, 2)),
                ),
                ("cards".into(), Json::Int(cards.len() as i64)),
            ],
        ));
    }
    let total = py::sum(
        spend
            .iter()
            .filter(|(k, _)| k != "bank_only")
            .map(|(_, v)| *v),
    );
    sec.line("| Spending | Per year | Card (in order) | Net rate |");
    sec.line("|---|---|---|---|");
    let mut order: Vec<&(String, f64)> = spend.iter().collect();
    order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (cat, amount) in order {
        if cat == "bank_only" {
            continue;
        }
        let parts: Vec<&Allocation> = plan
            .allocations
            .iter()
            .filter(|a| a.category == *cat)
            .collect();
        let route = parts
            .iter()
            .map(|a| {
                if parts.len() > 1 {
                    format!("{} ({})", name_of(&a.card), money(a.amount))
                } else {
                    name_of(&a.card)
                }
            })
            .collect::<Vec<_>>()
            .join(" → ");
        let route = if route.is_empty() {
            "bank transfer".to_string()
        } else {
            route
        };
        let rates = parts
            .iter()
            .map(|a| pct(a.rate))
            .collect::<Vec<_>>()
            .join(" / ");
        let rates = if rates.is_empty() {
            "0%".to_string()
        } else {
            rates
        };
        sec.line(format!(
            "| {cat} | {} | {route} | {rates} |",
            money(*amount)
        ));
    }
    sec.line("");
    sec.line(format!(
        "- Gross cash back: **{}** a year ({} of card-eligible spend).",
        money(plan.gross),
        if total != 0.0 {
            pct(plan.gross / total)
        } else {
            "0%".into()
        }
    ));
    sec.line(format!(
        "- Card fees not rebated: {}. Rent service fees: {}. Memberships: {}.",
        money(plan.card_fees),
        money(plan.service_fees),
        money(plan.membership_fees)
    ));
    sec.line(format!("- **Net: {} a year.**", money(plan.net())));
    if !plan.bank_paid.is_empty() {
        let mut keys: Vec<&String> = plan.bank_paid.iter().map(|(k, _)| k).collect();
        keys.sort();
        let labels: Vec<String> = keys
            .iter()
            .map(|c| {
                if *c == "bank_only" {
                    "taxes, transfers and other payments cards cannot take".into()
                } else {
                    c.to_string()
                }
            })
            .collect();
        sec.line(format!("- Paid by bank transfer: {}.", labels.join(", ")));
    }
    let rent = obj(&sit.profile, "rent");
    if let (Some(monthly), Some(service)) = (rent.get("monthly").filter(|v| v.truthy()), &service) {
        let charge = py::num(Some(monthly)).unwrap_or(0.0) * (1.0 + getf(service, "fee_rate", 0.0));
        let order = rent_charges(&plan, charge);
        if !order.is_empty() {
            let parts: Vec<String> = order
                .iter()
                .map(|(c, n)| format!("{} × {n}", name_of(c)))
                .collect();
            sec.line(format!(
                "- Rent rule (count charges, no math): {} per cap year, starting at each card's cap reset.",
                parts.join(" → ")
            ));
        }
    }
    for card in &cards {
        let used = py::sum(
            plan.allocations
                .iter()
                .filter(|a| a.card == card.id)
                .map(|a| a.amount),
        );
        if used == 0.0 {
            sec.tasks.push(
                Task::new(
                    format!("keep-active-{}", card.id),
                    format!(
                        "Put one small subscription on {} so it never sits idle",
                        card.name
                    ),
                )
                .notes(
                    "Long-idle cards can be closed by the issuer, which shortens credit history.",
                )
                .risk("settings"),
            );
        }
    }
    for n in sit.notes.borrow().iter() {
        sec.line(format!("- Note: {n}"));
    }
    sec.data = Data::Cashback(plan);
    Ok(sec)
}
