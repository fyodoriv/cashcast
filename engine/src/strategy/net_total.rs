//! Net total: the main number, as `strategy/net_total.py`. Gathers
//! aggregates into `data/net-total-input.json`; `crate::net_total` adds the
//! owner's own items, saves the trend and ranks the options. It runs last.

use super::banking::title;
use super::cashback::{optimize, rent_service, wallet};
use super::interest::places;
use super::py::{self, getf, gets, list, obj};
use super::situation::{Situation, Unported};
use super::{Ctx, Data, Section};
use crate::core::pyjson::{self, Json};

fn risk_equity(risk: &str) -> f64 {
    match risk {
        "conservative" => 0.4,
        "balanced" => 0.6,
        "all-equity" => 1.0,
        _ => 0.8,
    }
}

fn s(x: &str) -> Json {
    Json::Str(x.into())
}

fn opt(x: Option<f64>) -> Json {
    x.map_or(Json::Null, Json::Float)
}

struct Recorded {
    card: Json,
    owed: f64,
    as_of: Json,
    source: Json,
}

/// Card balances recorded under a wallet card's id (`api record`), as owed amounts.
fn recorded_cards(sit: &Situation) -> Vec<(String, Recorded)> {
    let mut cards: Vec<(String, Json)> = Vec::new();
    for c in list(obj(&sit.profile, "wallet"), "cards") {
        if !c.get("id").is_some_and(Json::truthy) {
            continue;
        }
        let id = c.get("id").map_or(String::new(), Json::py_str);
        match cards.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = c.clone(),
            None => cards.push((id, c.clone())),
        }
    }
    let mut out: Vec<(String, Recorded)> = Vec::new();
    let Some(Json::Obj(entries)) = sit.api_balances.as_ref() else {
        return out;
    };
    for (_, entry) in entries {
        if !matches!(entry, Json::Obj(_)) {
            continue;
        }
        let Some(Json::Obj(balances)) = entry.get("balances") else {
            continue;
        };
        for (name, amount) in balances {
            let Some((_, card)) = cards.iter().find(|(k, _)| k == name) else {
                continue;
            };
            if matches!(amount, Json::Null) {
                continue;
            }
            let r = Recorded {
                card: card.clone(),
                owed: py::num(Some(amount)).unwrap_or(0.0),
                as_of: entry.get("fetched_on").cloned().unwrap_or(Json::Null),
                source: entry.get("source").cloned().unwrap_or(s("api")),
            };
            match out.iter_mut().find(|(k, _)| k == name) {
                Some(slot) => slot.1 = r,
                None => out.push((name.clone(), r)),
            }
        }
    }
    out
}

/// (items, missing): every source of the net total as one signed CAD value, and what is not counted yet.
fn items(sit: &Situation) -> Result<(Vec<Json>, Vec<String>), Unported> {
    let mut out = Vec::new();
    let mut missing = Vec::new();
    let given = obj(&sit.config, "balances");
    let recorded = recorded_cards(sit);
    let by_record: Vec<String> = recorded
        .iter()
        .flat_map(|(_, r)| list(&r.card, "accounts").iter().map(Json::py_str))
        .collect();
    let empty = Json::Obj(Vec::new());
    let summary = sit.summary.as_ref().unwrap_or(&empty);
    for a in list(summary, "accounts") {
        let cls = a.get("class").map(Json::py_str).unwrap_or_default();
        let name = a.get("name").map_or(String::new(), Json::py_str);
        if (cls != "cash" && cls != "card") || (cls == "card" && by_record.contains(&name)) {
            continue;
        }
        let cad = if let Some(v) = given.get(&name) {
            py::num(Some(v)).unwrap_or(0.0)
        } else if let Some(c) = a.get("balance_cents").filter(|c| !matches!(c, Json::Null)) {
            let currency = a
                .get("currency")
                .filter(|c| c.truthy())
                .map_or("CAD".to_string(), Json::py_str);
            match sit.to_cad(py::num(Some(c)).unwrap_or(0.0) / 100.0, &currency)? {
                Some(v) => v,
                None => {
                    let cur = a.get("currency").map_or("None".into(), Json::py_str);
                    missing.push(format!("a bank or card account in {cur}: no CAD rate"));
                    continue;
                }
            }
        } else {
            missing.push("a bank or card account: no balance in the export".into());
            continue;
        };
        let cash = cls == "cash";
        out.push(Json::Obj(vec![
            (
                "label".into(),
                s(if cash { "Bank accounts" } else { "Cards" }),
            ),
            ("kind".into(), s(if cash { "cash" } else { "debt" })),
            ("cad".into(), Json::Float(py::round(cad, 2))),
            (
                "as_of".into(),
                a.get("last_date").cloned().unwrap_or(Json::Null),
            ),
            (
                "source".into(),
                a.get("balance_source")
                    .filter(|v| v.truthy())
                    .cloned()
                    .unwrap_or(s("summary")),
            ),
        ]));
    }
    for (_, r) in &recorded {
        out.push(Json::Obj(vec![
            ("label".into(), s("Cards")),
            ("kind".into(), s("debt")),
            ("cad".into(), Json::Float(-r.owed)),
            ("as_of".into(), r.as_of.clone()),
            ("source".into(), r.source.clone()),
        ]));
    }
    let rows = sit.source_balances()?;
    for r in &rows {
        let label = format!(
            "{} {}",
            title(&r.service),
            if r.kind == "investment" {
                r.bucket.clone().unwrap_or_else(|| "None".into())
            } else {
                r.currency.clone()
            }
        );
        if r.kind == "cash" && !r.counted {
            if r.cad.is_none() {
                missing.push(format!("{label}: no CAD rate"));
            }
            continue;
        }
        let Some(cad) = r.cad else {
            missing.push(format!("{label}: no CAD price"));
            continue;
        };
        out.push(Json::Obj(vec![
            ("label".into(), Json::Str(label)),
            ("kind".into(), Json::Str(r.kind.clone())),
            ("cad".into(), Json::Float(cad)),
            ("as_of".into(), r.fetched_on.clone()),
            ("source".into(), r.source.clone()),
        ]));
    }
    if !rows.iter().any(|r| r.kind == "investment") {
        let coins: Vec<&str> = rows
            .iter()
            .filter(|r| r.kind == "crypto")
            .map(|r| r.currency.as_str())
            .collect();
        let holdings = sit.holdings.as_ref().unwrap_or(&empty);
        let cost = py::sum(
            list(holdings, "assets")
                .iter()
                .filter(|a| {
                    getf(a, "quantity", 0.0) > 0.0
                        && !coins.contains(
                            &a.get("symbol")
                                .map_or(String::new(), Json::py_str)
                                .to_uppercase()
                                .as_str(),
                        )
                })
                .map(|a| getf(a, "acb_cad", 0.0)),
        );
        if cost != 0.0 {
            out.push(Json::Obj(vec![
                (
                    "label".into(),
                    s("Investments at cost (no market value recorded)"),
                ),
                ("kind".into(), s("investment")),
                ("cad".into(), Json::Float(py::round(cost, 2))),
                ("source".into(), s("cost")),
            ]));
            missing.push("investment market values: record them with `bin/cashcast api record wealthsimple --set TFSA=…`".into());
        } else {
            missing.push(
                "investments: record them with `bin/cashcast api record SERVICE --set NAME=AMOUNT`"
                    .into(),
            );
        }
    }
    if !sit.summary.as_ref().is_some_and(Json::truthy) {
        missing.push("bank and card balances: import your exports (`bin/cashcast import`)".into());
    }
    Ok((out, missing))
}

/// Expected long-run return a year of the owner's mix (`[expected_returns]` in investing.toml).
fn mix_return(sit: &Situation, risk: &str) -> Result<f64, Unported> {
    let k = sit.knowledge("investing")?;
    let er = obj(&k, "expected_returns");
    let equity = risk_equity(risk);
    Ok(equity * getf(er, "equity", 0.0) + (1.0 - equity) * getf(er, "fixed_income", 0.0))
}

fn options(ctx: &Ctx) -> Result<Vec<Json>, Unported> {
    let sit = &ctx.sit;
    let mut out = Vec::new();
    let risk = sit.goal("risk").map_or("growth".to_string(), Json::py_str);
    let threshold = match ctx.result("reserve").map(|s| &s.data) {
        Some(Data::Reserve(r)) => Some(r.threshold),
        _ => None,
    };
    let cash = sit.cash_balance()?;
    let has_cash_move = match ctx.result("invest_plan").map(|s| &s.data) {
        // Every state counts: a blocked or skipped cash move must not come back here.
        Some(Data::InvestPlan(p)) => p.moves.iter().any(|m| m == "cash-drag"),
        _ => false,
    };
    if threshold.is_some_and(|t| t != 0.0)
        && let Some(cash) = cash.filter(|c| *c != 0.0)
        && !has_cash_move
    {
        let cash_rate = places(sit)?
            .iter()
            .filter(|p| p.currency == "CAD")
            .map(|p| p.rate)
            .fold(None, |m: Option<f64>, r| {
                Some(m.map_or(r, |m| if r > m { r } else { m }))
            })
            .unwrap_or(0.0);
        let expected = mix_return(sit, &risk)?;
        let gain = expected - cash_rate;
        if gain > 0.0 {
            out.push(Json::Obj(vec![
                ("id".into(), s("invest-cash-above-floor")),
                (
                    "title".into(),
                    Json::Str(format!(
                        "Invest cash above your floor in your {risk} mix (account order in the reserve section; about {:.1}% to {:.1}% a year, long-run, not a forecast; stocks can fall 40% in a bad year)",
                        (expected - 0.02 - cash_rate) * 100.0,
                        (expected + 0.02 - cash_rate) * 100.0
                    )),
                ),
                ("section".into(), s("reserve")),
                ("per_dollar".into(), Json::Float(py::round(gain, 4))),
                ("cash_out".into(), Json::Float(cash)),
                ("risk".into(), Json::Str(risk.clone())),
                ("tags".into(), Json::List(vec![s("invest")])),
                (
                    "why".into(),
                    Json::Str(format!(
                        "Expected {:.1}% a year vs {:.1}% on cash; a long-run planning assumption.",
                        expected * 100.0,
                        cash_rate * 100.0
                    )),
                ),
            ]));
        }
    }
    if let Some(Data::Cashback(plan)) = ctx.result("cashback").map(|s| &s.data) {
        let cards = wallet(sit)?;
        let spend = sit.annual_by_reward_category();
        let service = rent_service(sit)?;
        let single = cards
            .iter()
            .map(|c| optimize(&spend, std::slice::from_ref(c), service.as_ref()).net())
            .fold(None, |m: Option<f64>, v| {
                Some(m.map_or(v, |m| if v > m { v } else { m }))
            })
            .unwrap_or(0.0);
        let gain = plan.net() - single;
        if gain > 0.0 {
            out.push(Json::Obj(vec![
                ("id".into(), s("cashback-routing")),
                (
                    "title".into(),
                    s("Pay each spending category with the card the cash-back section names"),
                ),
                ("section".into(), s("cashback")),
                ("effect_year".into(), Json::Float(py::round(gain, 2))),
                (
                    "why".into(),
                    s("Compared with putting everything on your best single card."),
                ),
            ]));
        }
    }
    // A capped reserve sweep invests part now; the safe-keeping option covers only the rest.
    let kept = ctx
        .results
        .iter()
        .flat_map(|s| s.tasks.iter())
        .find(|t| t.id == "sweep-cash")
        .and_then(|t| t.title.split("; keep ").nth(1))
        .and_then(|s| s.strip_suffix(" for later months"))
        .map(str::to_string);
    for sec in &ctx.results {
        if let Data::InvestPlan(p) = &sec.data {
            for o in &p.net_options {
                let mut items = vec![("section".to_string(), Json::Str(sec.id.clone()))];
                if let Json::Obj(fields) = o {
                    items.extend(fields.iter().cloned().map(|(k, v)| match (&k[..], &v, &kept) {
                        ("title", Json::Str(t), Some(kept)) => {
                            let t = match t
                                .strip_prefix("Keep the $")
                                .and_then(|r| r.split_once(" above your reserve safe"))
                            {
                                Some((_, tail)) => {
                                    format!("Keep the {kept} set aside for later months safe{tail}")
                                }
                                None => t.clone(),
                            };
                            (k, Json::Str(t))
                        }
                        _ => (k, v),
                    }));
                }
                out.push(Json::Obj(items));
            }
        }
    }
    Ok(out)
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let mut sec = Section::new("net_total", "Net total: the main number");
    let (rows, missing) = items(&ctx.sit)?;
    let opts: Vec<Json> = options(ctx)?;
    let sit = &ctx.sit;
    let reserve = match ctx.result("reserve").map(|s| &s.data) {
        Some(Data::Reserve(r)) => Some(r.threshold),
        _ => None,
    };
    let input = Json::Obj(vec![
        ("today".into(), Json::Str(sit.today.to_string())),
        ("items".into(), Json::List(rows)),
        (
            "missing".into(),
            Json::List(missing.into_iter().map(Json::Str).collect()),
        ),
        ("options".into(), Json::List(opts)),
        ("reserve".into(), opt(reserve)),
        (
            "risk".into(),
            Json::Str(sit.goal("risk").map_or("growth".to_string(), Json::py_str)),
        ),
    ]);
    let text = pyjson::dumps(&input, Some(1), false);
    let data = sit.home.join("data");
    let path = data.join("net-total-input.json");
    std::fs::create_dir_all(&data).map_err(|e| Unported(format!("data: {e}")))?;
    crate::write_private(&path, &text).map_err(Unported)?;
    let profile = std::fs::read_to_string(sit.home.join("profile.toml"));
    let history_path = data.join("net-total-history.json");
    let history = std::fs::read_to_string(&history_path).ok();
    let result = match profile {
        Err(e) => Err(format!(
            "cannot read {}: {e}",
            sit.home.join("profile.toml").display()
        )),
        Ok(p) => crate::net_total::run(&text, &p, history.as_deref()),
    };
    let report = match result {
        Ok((out, hist)) => {
            crate::write_private(&history_path, &hist).map_err(Unported)?;
            pyjson::loads(&out).ok_or_else(|| Unported("net-total output is not JSON".into()))?
        }
        Err(e) => {
            let last = format!("error: {e}");
            let last = last.trim().lines().last().unwrap_or("unknown").to_string();
            sec.line(format!("- The net total could not be computed: {last}. Fix `[boundaries]` in your profile.toml."));
            return Ok(sec);
        }
    };
    let markdown = gets(&report, "markdown", "");
    let body: Vec<&str> = markdown.lines().collect();
    if let Some(head) = body.first().and_then(|l| l.strip_prefix("## ")) {
        sec.title = head.to_string();
        sec.lines.extend(body.iter().skip(2).map(|l| l.to_string()));
    } else {
        sec.lines.extend(body.iter().map(|l| l.to_string()));
    }
    sec.data = Data::NetTotal(report);
    Ok(sec)
}
