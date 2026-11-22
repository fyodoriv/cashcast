//! Long-term investing: a low-fee core, where each asset goes, and a screen,
//! as `strategy/invest.py`. With `--market` (and consent `net:market-data`)
//! the watchlist adds live prices from `crate::api::market`.

use super::banking::title;
use super::py::{self, getf, gets, list, money, obj, pct};
use super::situation::{Situation, Unported};
use super::{Ctx, DISCLAIMER, Section};
use crate::core::pyjson::Json;
use chrono::NaiveDate;

const ETF_MAX_MER: f64 = 0.0025;
const CONCENTRATION: f64 = 0.10;

fn core(risk: &str) -> Option<(&'static str, f64)> {
    match risk {
        "conservative" => Some(("balanced", 0.40)),
        "balanced" => Some(("balanced", 0.60)),
        "growth" => Some(("equity", 0.80)),
        "all-equity" => Some(("equity", 1.0)),
        _ => None,
    }
}

type Rule = (&'static str, fn(&Json) -> bool);

fn m(metrics: &Json, k: &str) -> Option<f64> {
    metrics
        .get(k)
        .filter(|v| !matches!(v, Json::Null))
        .and_then(|v| py::num(Some(v)))
}

const STOCK_RULES: [Rule; 5] = [
    ("P/E between 0 and 30", |x| {
        m(x, "pe").is_some_and(|v| 0.0 < v && v <= 30.0)
    }),
    ("Profit margin ≥ 10%", |x| {
        m(x, "profit_margin").is_some_and(|v| v >= 0.10)
    }),
    ("Return on equity ≥ 12%", |x| {
        m(x, "roe").is_some_and(|v| v >= 0.12)
    }),
    ("Revenue growing year over year", |x| {
        m(x, "revenue_growth").is_some_and(|v| v > 0.0)
    }),
    ("Beta ≤ 1.5", |x| m(x, "beta").is_some_and(|v| v <= 1.5)),
];

pub fn screen(metrics: &Json, etf: Option<&Json>) -> (usize, usize, Vec<String>) {
    if let Some(e) = etf {
        let mer = getf(e, "mer", 1.0);
        let ok = mer <= ETF_MAX_MER;
        return (
            ok as usize,
            1,
            vec![format!(
                "MER {} {} {}",
                pct(mer),
                if ok { "≤" } else { ">" },
                pct(ETF_MAX_MER)
            )],
        );
    }
    let passed: Vec<String> = STOCK_RULES
        .iter()
        .filter(|(_, r)| r(metrics))
        .map(|(n, _)| n.to_string())
        .collect();
    (passed.len(), STOCK_RULES.len(), passed)
}

pub fn concentration(holdings: Option<&Json>, symbol: &str) -> Option<f64> {
    let h = holdings.filter(|h| h.truthy())?;
    if symbol.is_empty() {
        return None;
    }
    let assets = list(h, "assets");
    let total = py::sum(assets.iter().map(|a| getf(a, "acb_cad", 0.0)));
    let own = py::sum(
        assets
            .iter()
            .filter(|a| {
                a.get("symbol")
                    .map_or(String::new(), Json::py_str)
                    .to_uppercase()
                    == symbol.to_uppercase()
            })
            .map(|a| getf(a, "acb_cad", 0.0)),
    );
    if total != 0.0 {
        Some(own / total)
    } else {
        None
    }
}

/// Set by `--market`: fetch live prices for the watchlist.
pub static LIVE_MARKET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Investment accounts at market value, as recorded from each service's pages.
pub fn market_value(sit: &Situation) -> Result<Vec<String>, Unported> {
    let rows = sit.rows_of("investment")?;
    if rows.is_empty() {
        return Ok(vec![
            String::new(),
            "- Market value of your accounts: not recorded yet. Read them in the browser (playbook `read-investment-balances`), \
             then `bin/cashcast api record wealthsimple --set TFSA=…,RRSP=…`."
                .into(),
        ]);
    }
    let mut by_bucket: Vec<(String, f64)> = Vec::new();
    let mut out: Vec<String> = vec![
        String::new(),
        "**Market value of your accounts** (recorded from their pages):".into(),
        String::new(),
        "| Account | Bucket | Value | Recorded |".into(),
        "|---|---|---|---|".into(),
    ];
    for r in &rows {
        let value = r.cad.map_or(format!("no {} rate", r.currency), money);
        let bucket = r.bucket.clone().unwrap_or_else(|| "None".into());
        out.push(format!(
            "| {} {} | {bucket} | {value} | {} |",
            title(&r.service),
            r.account,
            r.fetched_on.py_str()
        ));
        if let Some(c) = r.cad {
            match by_bucket.iter_mut().find(|(b, _)| *b == bucket) {
                Some((_, v)) => *v += c,
                None => by_bucket.push((bucket, c)),
            }
        }
    }
    let total = py::sum(by_bucket.iter().map(|(_, v)| *v));
    let mut sorted = by_bucket.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    out.push(String::new());
    out.push(format!(
        "- Total: **{}** ({}).",
        money(total),
        sorted
            .iter()
            .map(|(b, v)| format!("{b} {}", money(*v)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let crypto = py::sum(sit.rows_of("crypto")?.iter().filter_map(|r| r.cad));
    if crypto != 0.0 {
        out.push(format!(
            "- With crypto from your exchanges ({}): {}.",
            money(crypto),
            money(total + crypto)
        ));
    }
    let max_age = py::num(
        sit.knowledge("foreign_cash")?
            .get("investments_max_age_days"),
    )
    .unwrap_or(31.0) as i64;
    let oldest = rows
        .iter()
        .filter(|r| r.fetched_on.truthy())
        .filter_map(|r| NaiveDate::parse_from_str(&r.fetched_on.py_str(), "%Y-%m-%d").ok())
        .min();
    if let Some(d) = oldest
        && (sit.today - d).num_days() > max_age
    {
        out.push(format!(
            "- Recorded more than {max_age} days ago: read the accounts again and record them."
        ));
    }
    Ok(out)
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("invest", "Long-term investing: core, placement and screen");
    let data = sit.knowledge("investing")?;
    let mut etfs: Vec<(String, &Json)> = Vec::new();
    for e in list(&data, "etfs") {
        let k = gets(e, "ticker", "").to_uppercase();
        match etfs.iter_mut().find(|(t, _)| *t == k) {
            Some(slot) => slot.1 = e,
            None => etfs.push((k, e)),
        }
    }
    let etf = |k: &str| etfs.iter().find(|(t, _)| t == k).map(|(_, e)| *e);
    let risk = sit.goal("risk").map_or("growth".to_string(), Json::py_str);
    let (kind, equity) = core(&risk).unwrap_or(("equity", 0.80));
    sec.line(format!("*{DISCLAIMER}*\n"));
    sec.line(format!(
        "**Core ({risk}, about {}% stocks):** one diversified, low-fee fund does most of the work.",
        py::round0(equity * 100.0)
    ));
    let mut candidates: Vec<&Json> = etfs
        .iter()
        .map(|(_, e)| *e)
        .filter(|e| gets(e, "asset_class", "") == kind && gets(e, "domicile", "") == "CA")
        .collect();
    candidates.sort_by(|a, b| {
        getf(a, "mer", 1.0)
            .partial_cmp(&getf(b, "mer", 1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for e in candidates.iter().take(3) {
        sec.line(format!(
            "- {}: {}, MER {}. {}",
            gets(e, "ticker", ""),
            gets(e, "name", ""),
            pct(getf(e, "mer", 0.0)),
            gets(e, "why", "")
        ));
    }
    let rules = list(&data, "asset_location");
    if !rules.is_empty() {
        sec.line("");
        sec.line("**Where each asset goes:**");
        for r in rules {
            sec.line(format!(
                "- {} → {}: {}",
                gets(r, "asset", ""),
                gets(r, "best_account", ""),
                gets(r, "why", "")
            ));
        }
    }
    let stock = obj(&sit.profile, "employer_stock")
        .get("symbol")
        .map(Json::py_str)
        .unwrap_or_default();
    if let Some(share) = concentration(sit.holdings.as_ref(), &stock) {
        let flag = if share > CONCENTRATION {
            "above"
        } else {
            "within"
        };
        sec.line(format!(
            "\n- Employer stock {stock}: {} of invested cost, {flag} the {} guideline.",
            pct(share),
            pct(CONCENTRATION)
        ));
        if share > CONCENTRATION {
            sec.line(
                "  Selling down over time (RSUs right at vest add no gain, since their cost is the vest value) \
                 cuts the risk of losing job and savings at once.",
            );
        }
    }
    sec.lines.extend(market_value(sit)?);
    let watchlist = obj(&sit.profile, "watchlist");
    let watch: Vec<String> = list(watchlist, "symbols")
        .iter()
        .map(|s| s.py_str().to_uppercase())
        .collect();
    if !watch.is_empty() {
        sec.line("");
        sec.line(
            "**Watchlist screen** (rules pass / total; a high score is not a recommendation):",
        );
        sec.line("");
        sec.line("| Symbol | Score | Passing rules | Price |");
        sec.line("|---|---|---|---|");
        let mut manual: Vec<(String, &Json)> = Vec::new();
        if let Json::Obj(items) = obj(watchlist, "metrics") {
            for (k, v) in items {
                let k = k.to_uppercase();
                match manual.iter_mut().find(|(t, _)| *t == k) {
                    Some(slot) => slot.1 = v,
                    None => manual.push((k, v)),
                }
            }
        }
        let user = crate::core::paths::UserHome::new(&sit.name, sit.home.clone());
        let live = LIVE_MARKET.load(std::sync::atomic::Ordering::Relaxed)
            && crate::core::consent::allowed(&user, "net:market-data", sit.today).unwrap_or(false);
        for sym in &watch {
            let mut metrics = manual
                .iter()
                .find(|(k, _)| k == sym)
                .map_or(Json::Obj(Vec::new()), |(_, v)| (*v).clone());
            if live {
                match crate::api::market(&user, sym, sit.today) {
                    Ok(Json::Obj(items)) => {
                        for (k, v) in items {
                            if v != Json::Null {
                                metrics.set(&k, v);
                            }
                        }
                    }
                    Ok(_) => {}
                    // As Python: only API and consent errors go in the row; others stop the run.
                    Err(e) if !matches!(e.kind, "apierror" | "consenterror") => {
                        return Err(Unported(format!("market data: {}", e.message)));
                    }
                    Err(e) => {
                        let head = e.message.split(':').next().unwrap_or("").to_string();
                        metrics.set("error", Json::Str(head));
                    }
                }
            }
            let metrics = &metrics;
            let e = etf(sym).or_else(|| etf(sym.strip_suffix(".TO").unwrap_or(sym)));
            let (score, total, passed) = screen(metrics, e);
            let price = metrics
                .get("price")
                .filter(|p| p.truthy())
                .map_or("—".to_string(), |p| {
                    money(py::num(Some(p)).unwrap_or(0.0))
                });
            let joined = if passed.is_empty() {
                "—".to_string()
            } else {
                passed.join("; ")
            };
            sec.line(format!("| {sym} | {score}/{total} | {joined} | {price} |"));
        }
        if !live {
            sec.line(
                "\nLive prices and fundamentals: run `bin/cashcast strategy --market` after the owner grants \
                 `net:market-data` (only ticker symbols are sent).",
            );
        }
    }
    Ok(sec)
}
