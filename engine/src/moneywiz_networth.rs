//! Net worth at market prices, twice: from MoneyWiz (its cash and shares)
//! and from the files (each bank's balance and the broker's shares). Both
//! use the same prices, so a gap is a cash or share difference, never a
//! price. Prices come from a `price` lookup (Yahoo with consent
//! `net:market-data`), else from the broker's last trade.

use std::collections::BTreeMap;

use crate::moneywiz::{AccountPlan, Store};
use crate::moneywiz_invest::Row;

/// One tracked account, in CAD.
pub struct Line {
    pub account: String,
    pub moneywiz: f64,
    pub files: f64,
    /// Symbols valued at the broker's last trade price, not the market.
    pub file_priced: Vec<String>,
}

pub struct Report {
    pub lines: Vec<Line>,
    /// Currencies or symbols with no price at all (left out of both sides).
    pub unpriced: Vec<String>,
}

/// Crypto symbols priced as `SYMBOL-CAD`.
const COINS: [&str; 10] = [
    "BTC", "ETH", "ADA", "DASH", "LTC", "TON", "XMR", "ZEC", "SOL", "DOT",
];

/// The Yahoo ticker for a symbol held in an account of `currency`:
/// `[moneywiz.tickers]` first, a coin as SYMBOL-CAD, a CAD listing as
/// SYMBOL.TO, else the symbol. None for a name with spaces (a fund).
pub fn ticker(symbol: &str, currency: &str, tickers: &BTreeMap<String, String>) -> Option<String> {
    if let Some(t) = tickers.get(symbol) {
        return Some(t.clone()).filter(|t| !t.is_empty());
    }
    if symbol.contains(' ') || symbol.is_empty() {
        return None;
    }
    if COINS.contains(&symbol) {
        return Some(format!("{symbol}-CAD"));
    }
    Some(if currency == "CAD" {
        format!("{symbol}.TO")
    } else {
        symbol.to_string()
    })
}

/// `price(ticker)` gives (price, currency). Tracked accounts: those a sync,
/// an investment mapping or a balance read on a site follows.
pub fn report(
    store: &Store,
    plans: &[AccountPlan],
    rows: &[Row],
    tracked: &[String],
    tickers: &BTreeMap<String, String>,
    price: &mut dyn FnMut(&str) -> Option<(f64, String)>,
) -> Report {
    let mut unpriced = Vec::new();
    let mut fx: BTreeMap<String, Option<f64>> = BTreeMap::new();
    let mut lines = Vec::new();
    for name in tracked {
        let Some(a) = store.accounts.iter().find(|a| &a.name == name) else {
            continue;
        };
        let Some(to_cad) = rate(&mut fx, &a.currency, price) else {
            unpriced.push(a.currency.clone());
            continue;
        };
        let mw_cash = store.balance(a.pk) as f64 / 100.0;
        let bank_cash = plans
            .iter()
            .find(|p| p.mw == a.pk)
            .and_then(|p| p.bank_balance.as_ref())
            // A balance as of a day: MoneyWiz rows after it are not in any
            // file yet, so both sides carry them.
            .map_or(mw_cash, |b| {
                let later = b
                    .as_of
                    .map_or(0, |d| store.balance(a.pk) - store.balance_on(a.pk, d));
                (b.cents + later) as f64 / 100.0
            });
        let mut line = Line {
            account: a.name.clone(),
            moneywiz: mw_cash * to_cad,
            files: bank_cash * to_cad,
            file_priced: Vec::new(),
        };
        for r in rows.iter().filter(|r| r.account == a.name) {
            let mut market = None;
            if let Some(t) = ticker(&r.symbol, &a.currency, tickers)
                && let Some((p, cur)) = price(&t)
                && let Some(x) = rate(&mut fx, &cur, price)
            {
                market = Some(p * x);
            }
            let each = match market {
                Some(p) => p,
                // The broker's last trade, in the account's currency.
                None => match r.last_price {
                    Some((_, p)) => {
                        line.file_priced.push(r.symbol.clone());
                        p * to_cad
                    }
                    None => {
                        unpriced.push(r.symbol.clone());
                        continue;
                    }
                },
            };
            line.moneywiz += r.moneywiz_shares * each;
            line.files += r.broker_shares * each;
        }
        lines.push(line);
    }
    unpriced.sort();
    unpriced.dedup();
    Report { lines, unpriced }
}

/// The market price of one share of `symbol` in `currency` (the holding's
/// account currency), or None with no ticker or no quote.
pub fn market_price(
    symbol: &str,
    currency: &str,
    tickers: &BTreeMap<String, String>,
    price: &mut dyn FnMut(&str) -> Option<(f64, String)>,
) -> Option<f64> {
    let t = ticker(symbol, currency, tickers)?;
    let (p, cur) = price(&t)?;
    let mut fx = BTreeMap::new();
    let to_cad = rate(&mut fx, &cur, price)?;
    let from_cad = rate(&mut fx, currency, price)?;
    Some(p * to_cad / from_cad).filter(|p| *p > 0.0)
}

/// CAD for one unit of `cur`, cached per run.
fn rate(
    fx: &mut BTreeMap<String, Option<f64>>,
    cur: &str,
    price: &mut dyn FnMut(&str) -> Option<(f64, String)>,
) -> Option<f64> {
    let cur = cur.to_uppercase();
    if cur == "CAD" || cur.is_empty() {
        return Some(1.0);
    }
    *fx.entry(cur.clone())
        .or_insert_with(|| price(&format!("{cur}CAD=X")).map(|(p, _)| p))
}

pub fn render(r: &Report) -> String {
    let mw: f64 = r.lines.iter().map(|l| l.moneywiz).sum();
    let files: f64 = r.lines.iter().map(|l| l.files).sum();
    let mut out = format!(
        "\n## Net worth at market prices\n\nEach tracked account valued twice at the same prices: MoneyWiz's cash and shares, and the files' balances and shares.\n\n| | CAD |\n|---|---:|\n| MoneyWiz | {} |\n| Files | {} |\n| Gap | {} |\n\n| Account | MoneyWiz | Files | Gap | Priced at the last trade |\n|---|---:|---:|---:|---|\n",
        money(mw),
        money(files),
        money(mw - files)
    );
    for l in &r.lines {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            l.account,
            money(l.moneywiz),
            money(l.files),
            money(l.moneywiz - l.files),
            l.file_priced.join(", ")
        ));
    }
    if !r.unpriced.is_empty() {
        out.push_str(&format!(
            "\nNo price, left out: {}.\n",
            r.unpriced.join(", ")
        ));
    }
    out
}

fn money(x: f64) -> String {
    crate::money::fmt_cents((x * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticker_picks_override_coin_listing_or_none() {
        let mut t = BTreeMap::new();
        t.insert("ABC".to_string(), "ABC.NE".to_string());
        t.insert("SKIP".to_string(), String::new());
        assert_eq!(ticker("ABC", "CAD", &t).as_deref(), Some("ABC.NE"));
        assert_eq!(ticker("SKIP", "CAD", &t), None);
        assert_eq!(ticker("BTC", "USD", &t).as_deref(), Some("BTC-CAD"));
        assert_eq!(ticker("XEQT", "CAD", &t).as_deref(), Some("XEQT.TO"));
        assert_eq!(ticker("VTI", "USD", &t).as_deref(), Some("VTI"));
        assert_eq!(ticker("Some Fund", "CAD", &t), None);
    }

    #[test]
    fn rate_is_one_for_cad_and_cached_otherwise() {
        let mut fx = BTreeMap::new();
        let mut calls = 0;
        let mut price = |t: &str| {
            calls += 1;
            assert_eq!(t, "USDCAD=X");
            Some((1.25, "CAD".to_string()))
        };
        assert_eq!(rate(&mut fx, "CAD", &mut price), Some(1.0));
        assert_eq!(rate(&mut fx, "usd", &mut price), Some(1.25));
        assert_eq!(rate(&mut fx, "USD", &mut price), Some(1.25));
        drop(price);
        assert_eq!(calls, 1);
    }

    #[test]
    fn render_sums_both_sides_and_the_gap() {
        let r = Report {
            lines: vec![Line {
                account: "Test account".into(),
                moneywiz: 100.0,
                files: 99.5,
                file_priced: vec!["FUND".into()],
            }],
            unpriced: vec!["ZZZ".into()],
        };
        let out = render(&r);
        assert!(out.contains("| Gap | "), "{out}");
        assert!(out.contains("FUND"), "{out}");
        assert!(out.contains("No price, left out: ZZZ."), "{out}");
    }
}
