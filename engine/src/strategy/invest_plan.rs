//! Investment moves: ranked, each with a gain range, its evidence, its risk
//! and its cost, as `strategy/invest_plan.py`. Gathers aggregates into
//! `data/invest-plan-input.json`; `crate::invest_plan` ranks the moves.

use super::interest::places;
use super::py::{self, getf, gets, list, obj};
use super::reserve::invest_order;
use super::situation::{Situation, Unported};
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::{self, Json};
use chrono::Datelike;

#[derive(Debug)]
pub struct InvestPlanData {
    pub do_nothing_wins: bool,
    pub moves: Vec<String>,
    pub net_options: Vec<Json>,
}

/// `{k.upper(): v for k, v in d.items()}`: later keys win, the first position stays.
fn upper_map(d: &Json) -> Vec<(String, Json)> {
    let mut out: Vec<(String, Json)> = Vec::new();
    if let Json::Obj(items) = d {
        for (k, v) in items {
            let k = k.to_uppercase();
            match out.iter_mut().find(|(t, _)| *t == k) {
                Some(slot) => slot.1 = v.clone(),
                None => out.push((k, v.clone())),
            }
        }
    }
    out
}

/// Common coin tickers. Held outside a registered account, they are crypto even
/// when no exchange balance names them (a hardware wallet, say).
const COINS: &[&str] = &[
    "BTC", "ETH", "LTC", "XMR", "ZEC", "TON", "SOL", "ADA", "DOGE", "XRP", "DOT", "BCH", "XLM",
    "USDT", "USDC", "AVAX", "LINK", "MATIC", "ATOM", "TRX",
];

/// A price in CAD the owner gave: `[invest.prices]`, else the watchlist's own metrics.
fn price(sit: &Situation, symbol: &str) -> Option<f64> {
    let sym = symbol.to_uppercase();
    let prices = upper_map(obj(obj(&sit.profile, "invest"), "prices"));
    let metrics = upper_map(obj(obj(&sit.profile, "watchlist"), "metrics"));
    let base = sym.strip_suffix(".TO").unwrap_or(&sym).to_string();
    for key in [sym.clone(), format!("{sym}.TO"), base] {
        if let Some((_, v)) = prices.iter().find(|(k, _)| *k == key) {
            // A zero, negative or non-number price falls back to the cost.
            return py::num(Some(v)).filter(|x| x.is_finite() && *x > 0.0);
        }
        if let Some((_, m)) = metrics.iter().find(|(k, _)| *k == key)
            && let Some(p) = m.get("price").filter(|p| p.truthy())
        {
            return py::num(Some(p)).filter(|x| x.is_finite() && *x > 0.0);
        }
    }
    None
}

fn cheapest(etfs: &[Json]) -> Json {
    let mut out: Vec<(String, Json)> = Vec::new();
    for e in etfs {
        let domicile = gets(e, "domicile", "");
        if !(domicile == "CA" || domicile == "US") || e.get("mer").is_none() {
            continue;
        }
        let s = |k: &str| e.get(k).map_or("None".to_string(), Json::py_str);
        // A US-listed fund files only under its own key ("equity:US:US-listed"):
        // the RRSP withholding move reads it, the fee move never does.
        let mut keys = if domicile == "US" {
            vec![format!("{}:{}:US-listed", s("asset_class"), s("region"))]
        } else {
            vec![format!("{}:{}", s("asset_class"), s("region"))]
        };
        if domicile == "CA" && gets(e, "region", "") == "global" {
            keys.push(s("asset_class"));
        }
        let mer = getf(e, "mer", 0.0);
        // All-in-one funds also file under their stock share ("mix:80").
        if let Some(share) = py::num(e.get("equity_share")) {
            keys.push(format!("mix:{:.0}", share * 100.0));
        } else if gets(e, "asset_class", "") == "equity" && gets(e, "region", "") == "global" {
            keys.push("mix:100".into());
        }
        for key in keys {
            let entry = Json::Obj(vec![
                (
                    "ticker".into(),
                    e.get("ticker").cloned().unwrap_or(Json::Null),
                ),
                ("mer".into(), Json::Float(mer)),
            ]);
            match out.iter_mut().find(|(k, _)| *k == key) {
                None => out.push((key, entry)),
                Some(slot) if mer < getf(&slot.1, "mer", 0.0) => slot.1 = entry,
                Some(_) => {}
            }
        }
    }
    Json::Obj(out)
}

fn opt(x: Option<f64>) -> Json {
    x.map_or(Json::Null, Json::Float)
}

/// The engine's input: aggregates only, never a raw row.
pub fn gather(ctx: &Ctx) -> Result<Json, Unported> {
    let sit = &ctx.sit;
    let invest = obj(&sit.profile, "invest");
    let marginal = match ctx.result("tax").map(|s| &s.data) {
        Some(Data::Tax(t)) => Some(t.marginal),
        _ => None,
    };
    let risk = sit.goal("risk").map_or("growth".to_string(), Json::py_str);
    // A different stock mix needs a reason, like any choice that hides a warning.
    let valid_target = py::num(invest.get("target_equity")).is_some_and(|x| x.is_finite() && x >= 0.0);
    let (target, source) = if valid_target
        && !gets(invest, "target_reason", "").trim().is_empty()
    {
        let why = gets(invest, "target_reason", "");
        let source = if why.is_empty() {
            "your override in [invest]".to_string()
        } else {
            format!("your override in [invest] ({why})")
        };
        let raw = getf(invest, "target_equity", 0.0);
        // 80 means 80%: a share above 1 is read as a percent, then kept in 0-1.
        let share = if raw > 1.0 { raw / 100.0 } else { raw };
        (
            if share.is_finite() {
                share.clamp(0.0, 1.0)
            } else {
                0.8
            },
            source,
        )
    } else {
        // Stock share per risk level: the `risk-equity-shares` fact.
        let ev = sit.knowledge("invest_evidence")?;
        let shares = list(&ev, "facts")
            .iter()
            .find(|f| f.get("id").is_some_and(|i| i.py_str() == "risk-equity-shares"))
            .cloned()
            .unwrap_or(Json::Obj(Vec::new()));
        let known = ["conservative", "balanced", "growth", "all-equity"].contains(&risk.as_str());
        let level = if known { risk.as_str() } else { "growth" };
        let default = match level {
            "conservative" => 0.40,
            "balanced" => 0.60,
            "all-equity" => 1.0,
            _ => 0.80,
        };
        let share = py::num(obj(&shares, "shares").get(level)).unwrap_or(default);
        let risk = if known {
            format!("{risk}, a rule of thumb from `risk-equity-shares`")
        } else {
            format!("'{risk}' is not a known risk level (conservative, balanced, growth, all-equity), so growth is used")
        };
        let note = if invest.get("target_equity").is_some() && !valid_target {
            "; your target_equity is not a number of 0 or more, so it is ignored"
        } else if invest.get("target_equity").is_some() {
            "; your target_equity is ignored until you add target_reason"
        } else {
            ""
        };
        (share, format!("your risk level ({risk}){note}"))
    };
    let knowledge = sit.knowledge("investing")?;
    let mut etfs: Vec<(String, Json)> = Vec::new();
    for e in list(&knowledge, "etfs") {
        let k = gets(e, "ticker", "").to_uppercase();
        match etfs.iter_mut().find(|(t, _)| *t == k) {
            Some(slot) => slot.1 = e.clone(),
            None => etfs.push((k, e.clone())),
        }
    }
    let mut buckets: Vec<(String, f64)> = Vec::new();
    let add = |b: &mut Vec<(String, f64)>, k: &str, v: f64| match b.iter_mut().find(|(t, _)| t == k)
    {
        Some(slot) => slot.1 += v,
        None => b.push((k.to_string(), v)),
    };
    let mut etrade = 0.0;
    for r in sit.rows_of("investment")? {
        let Some(cad) = r.cad else { continue };
        add(
            &mut buckets,
            &r.bucket.clone().unwrap_or_else(|| "None".into()),
            cad,
        );
        if r.service == "etrade" {
            etrade += cad;
        }
    }
    let crypto_rows = sit.rows_of("crypto")?;
    let crypto = py::sum(crypto_rows.iter().filter_map(|r| r.cad));
    if crypto != 0.0 {
        add(&mut buckets, "Crypto", crypto);
    }
    let employer_symbol = obj(&sit.profile, "employer_stock")
        .get("symbol")
        .filter(|s| s.truthy())
        .map_or(String::new(), Json::py_str)
        .to_uppercase();
    // The owner's own kind per symbol: `[invest.classes]` AAPL = "equity".
    let classes = upper_map(obj(invest, "classes"));
    let coins: Vec<&str> = crypto_rows.iter().map(|r| r.currency.as_str()).collect();
    let mut positions = Vec::new();
    // (value, basis, acb) of the employer stock, in Python's dict order: symbol, value, basis, acb.
    let mut employer: Option<(f64, String, Option<f64>)> = None;
    let empty = Json::Obj(Vec::new());
    let holdings = sit.holdings.as_ref().unwrap_or(&empty);
    let mut used_price = false;
    for a in list(holdings, "assets") {
        let sym = a
            .get("symbol")
            .map_or(String::new(), Json::py_str)
            .to_uppercase();
        let qty = getf(a, "quantity", 0.0);
        if !(qty.is_finite() && qty > 0.0) {
            continue;
        }
        let acb = if a.get("cost_unknown").is_some_and(Json::truthy) {
            None
        } else {
            Some(getf(a, "acb_cad", 0.0))
        };
        // A coin with no price of its own uses the exchange's CAD price.
        let coin_rate = || {
            crypto_rows
                .iter()
                .find(|r| r.currency.eq_ignore_ascii_case(&sym))
                .and_then(|r| r.rate)
                .filter(|r| r.is_finite() && *r > 0.0)
        };
        if price(sit, &sym).is_some() {
            used_price = true;
        }
        let (value, basis) = match price(sit, &sym).or_else(coin_rate) {
            Some(p) => (qty * p, "market"),
            None => (acb.filter(|v| *v != 0.0).unwrap_or(0.0), "cost"),
        };
        let find = |k: &str| {
            etfs.iter()
                .find(|(t, _)| t == k)
                .map(|(_, e)| e)
                .filter(|e| e.truthy())
        };
        let meta = find(&sym)
            .or_else(|| find(sym.strip_suffix(".TO").unwrap_or(&sym)))
            .unwrap_or(&empty);
        let s = |k: &str| meta.get(k).cloned().unwrap_or(Json::Str(String::new()));
        positions.push(Json::Obj(vec![
            ("symbol".into(), Json::Str(sym.clone())),
            (
                "bucket".into(),
                a.get("account_bucket")
                    .cloned()
                    .unwrap_or(Json::Str("non-registered".into())),
            ),
            ("value".into(), Json::Float(value)),
            ("basis".into(), Json::Str(basis.into())),
            ("acb".into(), opt(acb)),
            (
                "asset_class".into(),
                match classes.iter().find(|(k, _)| *k == sym) {
                    Some((_, v)) => Json::Str(v.py_str().to_lowercase()),
                    None if coins.contains(&sym.as_str())
                        || (COINS.contains(&sym.as_str())
                            && a.get("account_bucket")
                                .map_or(true, |b| b.py_str() == "non-registered")) =>
                    {
                        Json::Str("crypto".into())
                    }
                    None => s("asset_class"),
                },
            ),
            ("region".into(), s("region")),
            ("domicile".into(), s("domicile")),
            (
                "equity_share".into(),
                meta.get("equity_share").cloned().unwrap_or(Json::Null),
            ),
            (
                "mer".into(),
                if meta.get("mer").is_some() {
                    Json::Float(getf(meta, "mer", 0.0))
                } else {
                    Json::Null
                },
            ),
        ]));
        if sym == employer_symbol {
            let e = employer.get_or_insert((0.0, basis.to_string(), Some(0.0)));
            e.0 += value;
            e.2 = match (acb, e.2) {
                (Some(a), Some(b)) => Some(b + a),
                _ => None,
            };
            if basis != "market" {
                e.1 = basis.to_string();
            }
        }
    }
    match &mut employer {
        Some(e) if e.1 != "market" && etrade > 0.0 => {
            e.0 = etrade;
            e.1 = "etrade-total".into();
        }
        None if !employer_symbol.is_empty() && etrade > 0.0 => {
            employer = Some((etrade, "etrade-total".into(), None));
        }
        _ => {}
    }
    let cad_rates: Vec<f64> = places(sit)?
        .iter()
        .filter(|p| p.currency == "CAD")
        .map(|p| p.rate)
        .collect();
    let cash_rate = match sit.goal("cash_rate") {
        // A rate written as a percent (4.5) or text that is not a number
        // falls back to the best account rate, never to 0%.
        // 1 or more is a percent (no cash account pays 100%); above 25% after
        // that is a typo and falls back too.
        Some(v)
            if py::num(Some(v)).is_some_and(|r| {
                let r = if r >= 1.0 { r / 100.0 } else { r };
                r.is_finite() && (0.0..=0.25).contains(&r)
            }) =>
        {
            let r = py::num(Some(v)).unwrap_or(0.0);
            if r >= 1.0 { r / 100.0 } else { r }
        }
        _ => cad_rates
            .iter()
            .copied()
            .fold(None, |m: Option<f64>, r| {
                Some(m.map_or(r, |m| if r > m { r } else { m }))
            })
            .unwrap_or(0.0),
    };
    let evidence = sit.knowledge("invest_evidence")?;
    let facts: Vec<(String, Json)> = list(&evidence, "facts")
        .iter()
        .map(|f| (f.get("id").map_or("None".into(), Json::py_str), f.clone()))
        .fold(Vec::new(), |mut out, (k, v)| {
            match out.iter_mut().find(|(t, _): &&mut (String, Json)| *t == k) {
                Some(slot) => slot.1 = v,
                None => out.push((k, v)),
            }
            out
        });
    let mut facts = facts;
    // Live Bank of Canada facts (`knowledge refresh`, decision 0015), as cited facts.
    for (id, f) in crate::live_facts::as_facts(sit.today) {
        facts.retain(|(k, _)| *k != id);
        facts.push((id, f));
    }
    // This year's tax file when there is one; else the newest we ship, whose
    // review date then blocks the moves that read it (the `tax-rules` fact).
    let this_year = sit.knowledge(&format!("tax_ca_{}", sit.today.year()))?;
    // investing.toml (fund fees) carries its own review date.
    let inv_meta = obj(&knowledge, "meta");
    if let Some(rb) = inv_meta.get("review_by") {
        facts.retain(|(k, _)| k != "investing-data");
        facts.push((
            "investing-data".into(),
            Json::Obj(vec![
                ("id".into(), Json::Str("investing-data".into())),
                ("text".into(), Json::Str("Fund fees (MERs) and tickers in knowledge/data/investing.toml.".into())),
                ("source".into(), list(inv_meta, "sources").first().cloned().unwrap_or(Json::Null)),
                ("as_of".into(), inv_meta.get("as_of").cloned().unwrap_or(Json::Null)),
                ("review_by".into(), rb.clone()),
                ("confidence".into(), Json::Str("verified".into())),
            ]),
        ));
    }
    // Prices you typed are good for about 30 days from `prices_as_of`.
    if used_price {
        let as_of = invest
            .get("prices_as_of")
            .map(Json::py_str)
            .and_then(|d| chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok());
        let review_by = as_of.map_or("2000-01-01".to_string(), |d| (d + chrono::Days::new(30)).to_string());
        facts.retain(|(k, _)| k != "prices");
        facts.push((
            "prices".into(),
            Json::Obj(vec![
                ("id".into(), Json::Str("prices".into())),
                ("text".into(), Json::Str("Prices you gave in [invest.prices] or the watchlist. Update them and set `prices_as_of = YYYY-MM-DD` under [invest]; they are good for 30 days.".into())),
                ("source".into(), Json::Str("your profile".into())),
                ("as_of".into(), Json::Str(as_of.map_or("unknown".into(), |d| d.to_string()))),
                ("review_by".into(), Json::Str(review_by)),
                ("confidence".into(), Json::Str("owner".into())),
                ("judgment".into(), Json::Bool(true)),
            ]),
        ));
    }
    let tax = if obj(&this_year, "meta").get("review_by").is_some() {
        this_year
    } else {
        sit.knowledge("tax_ca_2026")?
    };
    let limits = obj(&tax, "limits");
    let meta = obj(&tax, "meta");
    if let Some(rb) = meta.get("review_by") {
        facts.retain(|(k, _)| k != "tax-rules");
        facts.push((
            "tax-rules".into(),
            Json::Obj(vec![
                ("id".into(), Json::Str("tax-rules".into())),
                ("text".into(), Json::Str(format!("{}: capital gains inclusion rate and TFSA, RRSP and FHSA limits.", gets(meta, "title", "Canada tax rules")))),
                ("source".into(), list(meta, "sources").first().cloned().unwrap_or(Json::Null)),
                ("as_of".into(), meta.get("as_of").cloned().unwrap_or(Json::Null)),
                ("review_by".into(), rb.clone()),
                ("confidence".into(), Json::Str("verified".into())),
            ]),
        ));
    }
    // A home bought soon wins over a longer horizon: the down payment is
    // needed first.
    let home = (sit.goal("first_time_home_buyer").is_some_and(Json::truthy)
        && sit.goal("mortgage_in_months").is_some_and(Json::truthy))
    .then(|| py::num(sit.goal("mortgage_in_months")))
    .flatten();
    let horizon = match (invest.get("horizon_years"), home) {
        // A value that is not a number is unknown, not "now".
        (Some(h), Some(m)) => py::num(Some(h)).map(|y| (y * 12.0).min(m)).or(Some(m)),
        (Some(h), None) => py::num(Some(h)).map(|y| y * 12.0),
        (None, m) => m,
    };
    let reserve = match ctx.result("reserve").map(|s| &s.data) {
        Some(Data::Reserve(r)) => Some(r.threshold),
        _ => None,
    };
    let year = sit.today.year();
    Ok(Json::Obj(vec![
        ("today".into(), Json::Str(sit.today.to_string())),
        ("horizon_months".into(), opt(horizon)),
        (
            "high_interest_debt".into(),
            opt(invest
                .get("high_interest_debt")
                .and_then(|v| py::num(Some(v)))),
        ),
        (
            "crypto_max".into(),
            opt(invest.get("crypto_max").and_then(|v| py::num(Some(v)))),
        ),
        ("marginal".into(), opt(marginal)),
        (
            "inclusion".into(),
            Json::Float(getf(limits, "capital_gains_inclusion", 0.5)),
        ),
        ("target_equity".into(), Json::Float(target)),
        ("target_source".into(), Json::Str(source)),
        (
            "simplicity".into(),
            Json::Float(py::num(sit.goal("simplicity_threshold")).unwrap_or(50.0)),
        ),
        ("cash".into(), opt(sit.cash_balance()?)),
        ("reserve".into(), opt(reserve)),
        ("cash_rate".into(), Json::Float(cash_rate)),
        (
            "room".into(),
            Json::List(
                invest_order(&sit.profile, marginal)
                    .into_iter()
                    .filter(|(_, room, _)| room.is_finite())
                    .map(|(a, room, why)| {
                        Json::Obj(vec![
                            ("account".into(), Json::Str(a)),
                            ("room".into(), Json::Float(room)),
                            ("why".into(), Json::Str(why)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "buckets".into(),
            Json::Obj(
                buckets
                    .into_iter()
                    .map(|(k, v)| (k, Json::Float(v)))
                    .collect(),
            ),
        ),
        ("positions".into(), Json::List(positions)),
        (
            "employer".into(),
            employer.map_or(Json::Null, |(value, basis, acb)| {
                Json::Obj(vec![
                    ("symbol".into(), Json::Str(employer_symbol.clone())),
                    ("value".into(), Json::Float(value)),
                    ("basis".into(), Json::Str(basis)),
                    ("acb".into(), opt(acb)),
                ])
            }),
        ),
        (
            "cheapest".into(),
            cheapest(&etfs.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>()),
        ),
        (
            "superficial".into(),
            Json::List(
                list(holdings, "superficial_loss_flags")
                    .iter()
                    .filter(|f| py::num(f.get("year")).unwrap_or(0.0) as i32 == year)
                    .map(|f| {
                        let s = |k: &str| f.get(k).map_or("None".into(), Json::py_str);
                        Json::Str(format!("{} ×{}", s("asset"), s("count")))
                    })
                    .collect(),
            ),
        ),
        ("facts".into(), Json::Obj(facts)),
        (
            "overrides".into(),
            Json::List(
                list(invest, "overrides")
                    .iter()
                    .map(|o| {
                        Json::Obj(vec![
                            ("move".into(), Json::Str(gets(o, "move", ""))),
                            ("reason".into(), Json::Str(gets(o, "reason", ""))),
                            // A bare TOML date or a quoted one; the first 10 chars are YYYY-MM-DD.
                            (
                                "until".into(),
                                Json::Str(o.get("until").map_or(String::new(), |u| {
                                    let t = u.py_str();
                                    t.get(0..10)
                                        .filter(|d| {
                                            d.len() == 10
                                                && t.len() > 10
                                                && t.as_bytes()[10] == b'T'
                                        })
                                        .map_or(t.clone(), str::to_string)
                                })),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]))
}

/// Gain moves as net total options: the middle of the range a year, the known cost once.
pub fn net_options(moves: &[Json]) -> Vec<Json> {
    moves
        .iter()
        .filter(|m| {
            gets(m, "kind", "") == "gain"
                && !m.get("overridden").is_some_and(Json::truthy)
                && !m.get("vetoed").is_some_and(Json::truthy)
                // Below the simplicity bar the plan gives no task; neither does the net total.
                && m.get("task").is_some_and(Json::truthy)
        })
        .map(|m| {
            Json::Obj(vec![
                (
                    "id".into(),
                    Json::Str(format!("invest-plan:{}", gets(m, "id", ""))),
                ),
                (
                    "title".into(),
                    Json::Str(format!(
                        "{} (range {} to {} a year; long-run, not a forecast)",
                        gets(m, "title", ""),
                        crate::invest_plan::dollars(getf(m, "gain_low", 0.0)),
                        crate::invest_plan::dollars(getf(m, "gain_high", 0.0))
                    )),
                ),
                (
                    "effect_year".into(),
                    Json::Float(py::round(
                        (getf(m, "gain_low", 0.0) + getf(m, "gain_high", 0.0)) / 2.0,
                        2,
                    )),
                ),
                (
                    "cost_once".into(),
                    m.get("cost_once").cloned().unwrap_or(Json::Float(0.0)),
                ),
                ("tags".into(), Json::List(vec![Json::Str("invest".into())])),
                (
                    "why".into(),
                    m.get("why").cloned().unwrap_or(Json::Str(String::new())),
                ),
            ])
        })
        .collect()
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let mut sec = Section::new("invest_plan", "Investment moves: ranked, with evidence");
    let input = pyjson::dumps(&gather(ctx)?, Some(1), false);
    let sit = &ctx.sit;
    let data = sit.home.join("data");
    let path = data.join("invest-plan-input.json");
    let write = std::fs::create_dir_all(&data)
        .and_then(|_| std::fs::write(&path, &input))
        .and_then(|_| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        });
    write.map_err(|e| Unported(format!("invest-plan-input.json: {e}")))?;
    let Some(result) = crate::invest_plan::run(&input)
        .ok()
        .and_then(|out| pyjson::loads(&out))
    else {
        sec.line("- The engine could not rank the moves. Run `bin/cashcast doctor`.");
        return Ok(sec);
    };
    let moves = list(&result, "moves");
    let markdown = gets(&result, "markdown", "");
    let body: Vec<&str> = markdown.lines().collect();
    let body = if body.first().is_some_and(|l| l.starts_with("## ")) {
        &body[2.min(body.len())..]
    } else {
        &body[..]
    };
    sec.lines.extend(body.iter().map(|l| l.to_string()));
    // One decision first: only the top move is due today; the rest follow it.
    let mut first: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    for m in moves {
        if let Some(task) = m.get("task").filter(|t| t.truthy()) {
            let id = gets(m, "id", "");
            // One Reminder per run: the rest wait in its notes until it is done.
            if first.is_some() {
                rest.push(task.py_str().split(';').next().unwrap_or("").trim().to_string());
                continue;
            }
            first = Some(task.py_str());
            let (due, after) = (sit.today, String::new());
            sec.tasks.push(
                Task::new(format!("invest-{}", id.replace(':', "-").to_lowercase()), task.py_str())
                    .due(due)
                    .notes(&format!(
                        "{after}From `bin/cashcast invest-plan` ({id}). Gain {} to {} a year, a long-run range, not a forecast. Cost: {} Risk: {} Not personal advice: you decide. You make the trade. Question it with the invest-challenge skill.",
                        crate::invest_plan::dollars(getf(m, "gain_low", 0.0)),
                        crate::invest_plan::dollars(getf(m, "gain_high", 0.0)),
                        {
                            let once = getf(m, "cost_once", 0.0);
                            let note = gets(m, "cost_note", "");
                            match (once > 0.0, note.trim().is_empty()) {
                                (true, true) => format!("about {} once.", crate::invest_plan::dollars(once)),
                                (true, false) => format!("about {} once. {note}", crate::invest_plan::dollars(once)),
                                (false, false) => note,
                                (false, true) => "none known.".into(),
                            }
                        },
                        gets(m, "risk", "see the plan.")
                    ))
                    .risk("money")
                    // re-planned every week, so it lives in the weekly session
                    .recurs("week"),
            );
        }
    }
    if !rest.is_empty()
        && let Some(t) = sec.tasks.iter_mut().rev().find(|t| t.id.starts_with("invest-"))
    {
        t.notes = format!(
            "{} Next, once this is done, run the plan again for: {}.",
            t.notes,
            rest.join("; ")
        );
    }
    sec.data = Data::InvestPlan(InvestPlanData {
        do_nothing_wins: result.get("do_nothing_wins").is_some_and(Json::truthy),
        moves: moves.iter().map(|m| gets(m, "id", "")).collect(),
        net_options: net_options(moves),
    });
    Ok(sec)
}
