//! MoneyWiz investment accounts against the broker ledgers: per account
//! and symbol, the shares MoneyWiz holds from its buy and sell rows and the
//! shares the broker files show, and the broker trades MoneyWiz lacks.
//! Reads only what `moneywiz::open` and `ledger::load_dir` already read.

use std::collections::BTreeMap;

use crate::ledger::Trade;
use crate::moneywiz::Store;

pub struct Row {
    pub account: String,
    pub symbol: String,
    pub moneywiz_shares: f64,
    pub broker_shares: f64,
    /// Broker trades with no MoneyWiz row of that day, symbol and shares.
    pub missing: usize,
    /// Those trades.
    pub missing_trades: Vec<Trade>,
    /// The price of the latest broker trade, and its day.
    pub last_price: Option<(chrono::NaiveDate, f64)>,
    /// MoneyWiz trade rows no broker trade pairs with: row pk and shares.
    pub extra: Vec<(i64, f64)>,
    /// Every MoneyWiz trade row of the symbol (pk) and every broker trade.
    pub all_rows: Vec<i64>,
    pub all_trades: Vec<Trade>,
    /// Paired rows whose day, shares or cash differ from the broker's trade
    /// (hand-typed rounding): their cost base, and so the gain, is off.
    pub off_rows: usize,
}

/// One row per investment account and symbol, from both sides.
/// Fund names differ in spacing and case between the two sides.
pub fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// One broker account: format, plan type and, when the file has several
/// accounts of one type, the end of the account id ("" for any).
struct Spec<'a> {
    format: &'a str,
    bucket: &'a str,
    id_end: &'a str,
}

impl Spec<'_> {
    fn takes(&self, t: &Trade) -> bool {
        t.format == self.format
            && t.bucket.eq_ignore_ascii_case(self.bucket)
            && t.account.ends_with(self.id_end)
    }
}

/// The "<format>:<plan type>[:<id end>]" parts of one mapping (comma-separated).
fn specs(v: &str) -> Vec<Spec<'_>> {
    v.split(',')
        .filter_map(|x| {
            let mut p = x.trim().splitn(3, ':');
            Some(Spec {
                format: p.next()?,
                bucket: p.next()?,
                id_end: p.next().unwrap_or(""),
            })
        })
        .collect()
}

/// Whether one `[moneywiz.investments]` value takes a broker account: its
/// file format, plan type and account id. A stock plan PDF statement holds
/// the cash of the plan that the benefit and gains files describe, so a
/// mapping of those takes it too.
pub fn spec_takes(mapping: &str, format: &str, bucket: &str, account: &str) -> bool {
    const PLAN_FILES: &[&str] = &[
        crate::ledger::ETRADE_PLAN,
        crate::ledger::ETRADE_RS,
        crate::ledger::ETRADE_ESPP,
        crate::ledger::ETRADE_GL,
    ];
    specs(mapping).iter().any(|s| {
        let same = s.format == format
            || (format == crate::pdf::ETRADE_STOCKPLAN && PLAN_FILES.contains(&s.format));
        same && s.bucket.eq_ignore_ascii_case(bucket) && account.ends_with(s.id_end)
    })
}

/// The last four characters of an account id, the only part shown.
fn id_end(id: &str) -> String {
    let c: Vec<char> = id.chars().collect();
    c[c.len().saturating_sub(4)..].iter().collect()
}

/// `map`: MoneyWiz account name to "<format>:<plan type>", several
/// separated by commas.
pub fn compare(store: &Store, trades: &[Trade], map: &BTreeMap<String, String>) -> Vec<Row> {
    let mut out = Vec::new();
    for a in &store.accounts {
        let Some(want) = map.get(&a.name).map(|v| specs(v)) else {
            continue;
        };
        let theirs: Vec<&Trade> = trades
            .iter()
            .filter(|t| want.iter().any(|w| w.takes(t)))
            .collect();
        let ours: Vec<(chrono::NaiveDate, String, f64)> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk)
            .filter_map(|t| {
                let tr = store.trades.get(&t.pk)?;
                let sign = if t.entity.contains("Sell") { -1.0 } else { 1.0 };
                Some((t.date, norm(&tr.symbol), sign * tr.shares.abs()))
            })
            .collect();
        let our_pks: Vec<i64> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && store.trades.contains_key(&t.pk))
            .map(|t| t.pk)
            .collect();
        // "WEALTHSIMPLE GOLD" in MoneyWiz is "GOLD" at the broker.
        let broker_syms: Vec<String> = theirs.iter().map(|t| norm(&t.symbol)).collect();
        let ours: Vec<(chrono::NaiveDate, String, f64)> = ours
            .into_iter()
            .map(|(d, s, sh)| {
                let words: Vec<&str> = s.split(' ').collect();
                let alias = broker_syms
                    .iter()
                    .find(|b| **b != s && !b.contains(' ') && words.contains(&b.as_str()));
                (d, alias.cloned().unwrap_or(s), sh)
            })
            .collect();
        let mut symbols: BTreeMap<String, Row> = BTreeMap::new();
        let row = |s: &str| Row {
            account: a.name.clone(),
            symbol: s.to_string(),
            moneywiz_shares: 0.0,
            broker_shares: 0.0,
            missing: 0,
            missing_trades: Vec::new(),
            last_price: None,
            extra: Vec::new(),
            all_rows: Vec::new(),
            all_trades: Vec::new(),
            off_rows: 0,
        };
        for (_, s, sh) in &ours {
            symbols
                .entry(s.clone())
                .or_insert_with(|| row(s))
                .moneywiz_shares += sh;
        }
        // MoneyWiz values a holding at its opening shares plus its rows.
        for o in store
            .holdings
            .iter()
            .filter(|o| o.account == a.pk && o.shares != 0.0)
        {
            let s = norm(&o.symbol);
            symbols
                .entry(s.clone())
                .or_insert_with(|| row(&s))
                .moneywiz_shares += o.shares;
        }
        for (i, (_, s, _)) in ours.iter().enumerate() {
            if let Some(r) = symbols.get_mut(s) {
                r.all_rows.push(our_pks[i]);
            }
        }
        let mut used = vec![false; ours.len()];
        let cents_of = |i: usize| {
            store
                .txns
                .iter()
                .find(|x| x.pk == our_pks[i])
                .map_or(0, |x| x.cents.abs())
        };
        let want_cents = |t: &Trade| (trade_cash(t, &a.currency).abs() * 100.0).round() as i64;
        let live: Vec<&&Trade> = theirs.iter().filter(|t| t.shares != 0.0).collect();
        // First the exact pairs (day, shares and cash), so a loose pair never
        // takes the row that is exactly another trade's.
        let mut pair: Vec<Option<usize>> = vec![None; live.len()];
        for (k, t) in live.iter().enumerate() {
            let s = norm(&t.symbol);
            pair[k] = ours.iter().enumerate().position(|(i, (d, os, sh))| {
                !used[i]
                    && *os == s
                    && *d == t.date
                    && (sh - t.shares).abs() <= 1e-4
                    && (t.cad.is_none() || (cents_of(i) - want_cents(t)).abs() <= 2)
            });
            if let Some(i) = pair[k] {
                used[i] = true;
            }
        }
        for (k, t) in live.iter().enumerate() {
            let s = norm(&t.symbol);
            let r = symbols.entry(s.clone()).or_insert_with(|| row(&s));
            r.broker_shares += t.shares;
            r.all_trades.push((**t).clone());
            // Rows typed by hand: rounded shares, a few days off. Each
            // MoneyWiz row stands for one broker trade at most.
            let exact = pair[k].is_some();
            let found = pair[k].or_else(|| {
                ours.iter().enumerate().position(|(i, (d, os, sh))| {
                    !used[i]
                        && *os == s
                        && (*d - t.date).num_days().abs() <= 3
                        && (sh.signum() == t.shares.signum())
                        && (sh - t.shares).abs() <= (t.shares.abs() * 0.02).max(0.01)
                })
            });
            if let Some(i) = found {
                used[i] = true;
                if !exact {
                    r.off_rows += 1;
                }
            } else {
                r.missing += 1;
                r.missing_trades.push((**t).clone());
            }
            if let Some(cad) = t.cad.filter(|c| t.shares != 0.0 && *c != 0.0) {
                r.last_price = Some((t.date, (cad / t.shares).abs()));
            }
        }
        for (i, (_, s, sh)) in ours.iter().enumerate() {
            if !used[i] {
                if let Some(r) = symbols.get_mut(s) {
                    r.extra.push((our_pks[i], *sh));
                }
            }
        }
        out.extend(symbols.into_values());
    }
    out
}

pub fn render(rows: &[Row]) -> String {
    let mut out = String::from(
        "\n## Investments against the broker files\n\nShares per symbol from MoneyWiz's buy and sell rows and from the broker's trades.\n\n| Account | Symbol | MoneyWiz shares | Broker shares | Broker trades missing | Rows off (day, shares or cash) | Last trade price | On |\n|---|---|---:|---:|---:|---:|---:|---|\n",
    );
    if rows.is_empty() {
        out.push_str("| (none) | | | | | | | |\n");
    }
    let mut notes = String::new();
    for r in rows {
        let no_cash = r.missing_trades.iter().filter(|t| t.cad.is_none()).count();
        if no_cash > 0 {
            notes.push_str(&format!(
                "- {} {}: {no_cash} missing broker trades have no CAD amount (no price or no USD rate that day), so none are written.\n",
                r.account, r.symbol
            ));
        }
        let (d, p) = r
            .last_price
            .map_or((String::new(), String::new()), |(d, p)| {
                (d.to_string(), format!("{p:.4}"))
            });
        out.push_str(&format!(
            "| {} | {} | {:.4} | {:.4} | {} | {} | {p} | {d} |\n",
            r.account, r.symbol, r.moneywiz_shares, r.broker_shares, r.missing, r.off_rows
        ));
    }
    if !notes.is_empty() {
        out.push('\n');
        out.push_str(&notes);
    }
    out
}

/// Broker accounts ("<format>:<plan type>", trade count) no MoneyWiz
/// account is mapped to yet.
pub fn unmapped(trades: &[Trade], map: &BTreeMap<String, String>) -> Vec<(String, usize)> {
    let mut n: BTreeMap<String, usize> = BTreeMap::new();
    for t in trades {
        if map.values().any(|v| specs(v).iter().any(|w| w.takes(t))) {
            continue;
        }
        let key = if t.account.is_empty() {
            format!("{}:{}", t.format, t.bucket)
        } else {
            format!("{}:{}:{}", t.format, t.bucket, id_end(&t.account))
        };
        *n.entry(key).or_default() += 1;
    }
    n.into_iter().collect()
}

/// Mappings whose specs take trades from more than one broker account id
/// (two accounts can share the last four characters): each with the
/// shortest id ends that tell its accounts apart, and their trade counts.
pub fn ambiguous(
    trades: &[Trade],
    map: &BTreeMap<String, String>,
) -> Vec<(String, Vec<(String, usize)>)> {
    let mut out = Vec::new();
    for (name, v) in map {
        let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
        for t in trades.iter().filter(|t| !t.account.is_empty()) {
            if specs(v).iter().any(|w| w.takes(t)) {
                *ids.entry(t.account.as_str()).or_default() += 1;
            }
        }
        if ids.len() < 2 {
            continue;
        }
        let keys: Vec<&str> = ids.keys().copied().collect();
        let ends = keys
            .iter()
            .map(|id| {
                let c: Vec<char> = id.chars().collect();
                let n = (4..=c.len())
                    .find(|&n| {
                        let end: String = c[c.len() - n..].iter().collect();
                        keys.iter().filter(|k| k.ends_with(&end)).count() == 1
                    })
                    .unwrap_or(c.len());
                (c[c.len() - n..].iter().collect(), ids[id])
            })
            .collect();
        out.push((name.clone(), ends));
    }
    out
}

pub fn render_ambiguous(list: &[(String, Vec<(String, usize)>)]) -> String {
    let mut out = String::new();
    for (name, ends) in list {
        let parts: Vec<String> = ends
            .iter()
            .map(|(e, n)| format!("{e} ({n} trades)"))
            .collect();
        out.push_str(&format!(
            "\n\"{name}\" takes trades from {} broker accounts: {}. Add the id end that names one (\"<format>:<plan type>:<id end>\").\n",
            ends.len(),
            parts.join(", ")
        ));
    }
    out
}

pub fn render_unmapped(list: &[(String, usize)]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\nBroker accounts with no MoneyWiz account (map one in cashcast.toml [moneywiz.investments] \"MoneyWiz name\" = \"<format>:<plan type>[:<id end>]\"):\n",
    );
    for (k, n) in list {
        out.push_str(&format!("- {k}: {n} trades\n"));
    }
    out
}

/// The missing broker trades as rows for the writer. A trade with no
/// known cash amount, or an account with no sync id, is left out.
pub fn trade_adds(store: &Store, rows: &[Row]) -> Vec<crate::moneywiz_edit::TradeAdd> {
    trade_fixes(store, rows, false).0
}

/// The missing broker trades to add and, when the bank wins, the MoneyWiz
/// trade rows no broker trade pairs with, to delete. A symbol is fixed only
/// when the result makes MoneyWiz's shares the broker's.
/// The cash a broker trade moved in an account of this currency: below
/// zero for a buy, 0 when it moved none here.
pub fn trade_cash(t: &Trade, currency: &str) -> f64 {
    let here = match t.cash.as_deref() {
        Some("") => true,
        Some(c) => c.eq_ignore_ascii_case(currency),
        None => false,
    };
    match t.cad {
        Some(cad) if here => {
            if t.shares < 0.0 {
                cad.abs()
            } else {
                -cad.abs()
            }
        }
        _ => 0.0,
    }
}

pub fn trade_fixes(
    store: &Store,
    rows: &[Row],
    bank_wins: bool,
) -> (
    Vec<crate::moneywiz_edit::TradeAdd>,
    Vec<crate::moneywiz_edit::RowEdit>,
) {
    let mut out = Vec::new();
    let mut deletes = Vec::new();
    // A negative broker total means the broker file starts partway through
    // the account's history: nothing is written to that account.
    let partial: std::collections::BTreeSet<&str> = rows
        .iter()
        .filter(|r| r.broker_shares < -0.01)
        .map(|r| r.account.as_str())
        .collect();
    for r in rows {
        if partial.contains(r.account.as_str()) {
            continue;
        }
        let Some(a) = store.accounts.iter().find(|a| a.name == r.account) else {
            continue;
        };
        // MoneyWiz holds trades only in an investment account.
        if a.gid.is_empty() || a.kind != "InvestmentAccount" {
            continue;
        }
        // Only when the trades make MoneyWiz's shares the broker's: a
        // partial broker history (12 months) never gets written.
        let adding: f64 = r.missing_trades.iter().map(|t| t.shares).sum();
        let removing: f64 = if bank_wins {
            r.extra.iter().map(|(_, sh)| sh).sum()
        } else {
            0.0
        };
        // A symbol only MoneyWiz holds, in an account the broker covers, is a
        // stale hand entry: the bank wins and its rows go.
        let stale = bank_wins
            && r.all_trades.is_empty()
            && r.broker_shares.abs() <= 0.01
            && rows
                .iter()
                .any(|o| o.account == r.account && !o.all_trades.is_empty());
        if stale {
            for pk in &r.all_rows {
                let Some(t) = store.txns.iter().find(|t| t.pk == *pk) else {
                    continue;
                };
                if t.gid.is_empty() {
                    continue;
                }
                deletes.push(crate::moneywiz_edit::RowEdit {
                    entity: t.entity.clone(),
                    gid: t.gid.clone(),
                    expect_amount: t.cents as f64 / 100.0,
                    expect_date: t.date.to_string(),
                    amount: None,
                    category: None,
                    category_kind: None,
                    payee: None,
                    memo: None,
                    date: None,
                    delete: true,
                    account: None,
                    unlink: false,
                });
            }
            continue;
        }
        let pick_up = (r.moneywiz_shares - removing + adding - r.broker_shares).abs() <= 0.01;
        // The cash the broker's trades moved in this account's currency,
        // against what MoneyWiz's rows hold: coins moved in, out or swapped,
        // and trades paid in another currency, move no cash here.
        // Only files that name the paying currency (Kraken) are checked: the
        // others do not say which cash a move of shares took.
        let cash_wrong = bank_wins
            && r.all_trades
                .iter()
                .any(|t| t.cash.as_deref().is_some_and(|c| !c.is_empty()))
            && r.all_trades.iter().all(|t| t.cad.is_some())
            && {
                let want: i64 = r
                    .all_trades
                    .iter()
                    .map(|t| (trade_cash(t, &a.currency) * 100.0).round() as i64)
                    .sum();
                let have: i64 = r
                    .all_rows
                    .iter()
                    .filter_map(|pk| store.txns.iter().find(|t| t.pk == *pk))
                    .map(|t| t.cents)
                    .sum();
                (want - have).abs() > 100
            };
        // A paired row off by a little (within the pairing room) leaves the
        // shares apart even when no broker trade is missing.
        // Every row must equal its broker trade, so the cost base and the
        // gain match the broker's.
        let rows_off = bank_wins && r.off_rows > 0 && !r.all_trades.is_empty();
        // Rows pair within 0.0001 shares each, so their sum can still differ
        // at four decimals: any gap above float noise is off.
        let shares_off = bank_wins
            && (r.moneywiz_shares - r.broker_shares).abs() > 1e-5
            && !r.all_trades.is_empty();
        if !cash_wrong
            && !shares_off
            && !rows_off
            && (r.missing_trades.is_empty() || r.missing_trades.iter().any(|t| t.cad.is_none()))
        {
            continue;
        }
        // When the bank wins and patching cannot make the shares equal (a
        // paired row off by a little), every row of the symbol is replaced
        // by the broker's trades, when they all have a cash amount.
        let replace_all = (!pick_up || cash_wrong || shares_off || rows_off)
            && bank_wins
            && r.all_trades.iter().all(|t| t.cad.is_some())
            && r.broker_shares.abs() >= 0.0;
        if (!pick_up || cash_wrong || shares_off || rows_off) && !replace_all {
            continue;
        }
        let (to_delete, to_add): (Vec<i64>, Vec<Trade>) = if replace_all {
            (r.all_rows.clone(), r.all_trades.clone())
        } else {
            (
                if bank_wins {
                    r.extra.iter().map(|(pk, _)| *pk).collect()
                } else {
                    Vec::new()
                },
                r.missing_trades.clone(),
            )
        };
        {
            for pk in &to_delete {
                let Some(t) = store.txns.iter().find(|t| t.pk == *pk) else {
                    continue;
                };
                if t.gid.is_empty() {
                    continue;
                }
                deletes.push(crate::moneywiz_edit::RowEdit {
                    entity: t.entity.clone(),
                    gid: t.gid.clone(),
                    expect_amount: t.cents as f64 / 100.0,
                    expect_date: t.date.to_string(),
                    amount: None,
                    category: None,
                    category_kind: None,
                    payee: None,
                    memo: None,
                    date: None,
                    delete: true,
                    account: None,
                    unlink: false,
                });
            }
        }
        for t in &to_add {
            let Some(cad) = t.cad.filter(|_| t.shares != 0.0) else {
                continue;
            };
            let sell = t.shares < 0.0;
            out.push(crate::moneywiz_edit::TradeAdd {
                account_gid: a.gid.clone(),
                sell,
                date: t.date.to_string(),
                symbol: r.symbol.clone(),
                shares: t.shares.abs(),
                price: (cad / t.shares).abs(),
                amount: trade_cash(t, &a.currency),
            });
        }
    }
    (out, deletes)
}

/// One trade per `[moneywiz.holdings]` entry ("SYMBOL:SHARES@YYYY-MM-DD")
/// that moves the account's broker shares to the number read on the site:
/// coins with no cash (network fees the files never show). Problems name
/// the entry, never an amount.
pub fn holding_adjustments(
    trades: &[Trade],
    map: &BTreeMap<String, String>,
    holdings: &BTreeMap<String, String>,
) -> (Vec<Trade>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (name, value) in holdings {
        let parsed = value.split_once('@').and_then(|(left, day)| {
            let (sym, shares) = left.split_once(':')?;
            Some((
                norm(sym.trim()),
                shares.trim().parse::<f64>().ok()?,
                chrono::NaiveDate::parse_from_str(day.trim(), "%Y-%m-%d").ok()?,
            ))
        });
        let Some((sym, want, day)) = parsed else {
            problems.push(format!(
                "holdings \"{name}\": write \"SYMBOL:SHARES@YYYY-MM-DD\", such as \"BTC:0.5@2026-11-08\""
            ));
            continue;
        };
        let Some(spec) = map.get(name.trim()).map(|v| specs(v)) else {
            problems.push(format!(
                "holdings \"{name}\": map the account in [moneywiz.investments] first"
            ));
            continue;
        };
        let theirs: Vec<&Trade> = trades
            .iter()
            .filter(|t| spec.iter().any(|w| w.takes(t)) && norm(&t.symbol) == sym)
            .collect();
        let Some(like) = theirs.first() else {
            problems.push(format!(
                "holdings \"{name}\": the broker files have no {sym} trade for it"
            ));
            continue;
        };
        let have: f64 = theirs
            .iter()
            .filter(|t| t.date <= day)
            .map(|t| t.shares)
            .sum();
        let gap = want - have;
        // Priced at the last broker trade with a price, so MoneyWiz (which
        // values a holding at its newest trade) keeps a real price. No cash
        // moves: `cash` stays None.
        let price = theirs
            .iter()
            .filter(|t| t.date <= day && t.shares.abs() > 0.0)
            .filter_map(|t| t.cad.map(|c| (t.date, (c / t.shares).abs())))
            .filter(|(_, p)| *p > 0.0)
            .max_by_key(|(d, _)| *d)
            .map_or(0.0, |(_, p)| p);
        if gap.abs() > 1e-8 {
            out.push(Trade {
                date: day,
                symbol: like.symbol.clone(),
                bucket: like.bucket,
                format: like.format.clone(),
                account: like.account.clone(),
                shares: gap,
                cad: Some(gap.abs() * price),
                cash: None,
            });
        }
    }
    (out, problems)
}

/// Units a statement shows before the broker's history starts.
///
/// A plan's download can start after its first statement (Sun Life keeps
/// about 12 months). The history's first day then buys every unit held
/// before as one lump. For the latest statement before that day, each
/// fund's statement units move out of the lump into a buy on the statement's
/// day. Those buys cost what was paid in up to then: MoneyWiz's cash that
/// day without the account's own buy and sell rows (the plan holds no
/// cash), split by the lump's price per unit. The lump's other cost pays for
/// the units left in it, so the account's cash stays the same. The result
/// does not depend on buys already written, so a second run changes nothing.
/// Returns problems; an account with one is left as it was.
pub fn statement_lots(
    store: &Store,
    trades: &mut Vec<Trade>,
    snapshots: &[crate::pdf::sunlife::Snapshot],
    map: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut gone = Vec::new();
    let mut added = Vec::new();
    for a in &store.accounts {
        let Some(want) = map.get(&a.name) else {
            continue;
        };
        let spec = specs(want);
        let mine: Vec<usize> = (0..trades.len())
            .filter(|&i| spec.iter().any(|w| w.takes(&trades[i])))
            .collect();
        let Some(first) = mine.iter().map(|&i| trades[i].date).min() else {
            continue;
        };
        let Some(s) = snapshots
            .iter()
            .filter(|s| {
                s.as_of < first
                    && spec.iter().any(|w| {
                        w.format == crate::ledger::SUNLIFE && w.bucket.eq_ignore_ascii_case(&s.plan)
                    })
            })
            .max_by_key(|s| s.as_of)
        else {
            continue;
        };
        // The lump: each symbol's largest priced buy on the first day.
        let mut lump: BTreeMap<String, usize> = BTreeMap::new();
        for &i in &mine {
            let t = &trades[i];
            if t.date != first || t.shares <= 0.0 || t.cad.is_none_or(|c| c <= 0.0) {
                continue;
            }
            let e = lump.entry(norm(&t.symbol)).or_insert(i);
            if trades[*e].shares < t.shares {
                *e = i;
            }
        }
        let symbols: Vec<String> = lump.keys().cloned().collect();
        let mut used: Vec<String> = Vec::new();
        let mut lots: Vec<(usize, f64)> = Vec::new();
        let mut bad = Vec::new();
        for f in s.funds.iter().filter(|f| f.units > 0.0) {
            let left: Vec<String> = symbols
                .iter()
                .filter(|x| !used.contains(x))
                .cloned()
                .collect();
            match pair_symbol(&f.name, &left) {
                Some(x) if trades[lump[&x]].shares + 1e-6 >= f.units => {
                    lots.push((lump[&x], f.units.min(trades[lump[&x]].shares)));
                    used.push(x);
                }
                Some(_) => bad.push(format!(
                    "statement {} lists more {} units than the broker's first day ({}) buys",
                    s.as_of, f.name, first
                )),
                None => bad.push(format!(
                    "statement {}: no buy of {} on the broker's first day ({})",
                    s.as_of, f.name, first
                )),
            }
        }
        // What was paid in up to the statement's day, in cents.
        let traded: i64 = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && t.date <= s.as_of && store.trades.contains_key(&t.pk))
            .map(|t| t.cents)
            .sum();
        let paid = store.balance_on(a.pk, s.as_of) - traded;
        let cost = |i: usize| (trades[i].cad.unwrap_or(0.0) * 100.0).round() as i64;
        let price = |i: usize| trades[i].cad.unwrap_or(0.0) / trades[i].shares;
        let lump_cost: i64 = lots.iter().map(|&(i, _)| cost(i)).sum();
        let rest = lump_cost - paid;
        let left = |i: usize, u: f64| {
            let r = trades[i].shares - u;
            if r < 1e-6 { 0.0 } else { r }
        };
        let rest_weight: f64 = lots.iter().map(|&(i, u)| left(i, u) * price(i)).sum();
        if bad.is_empty() && paid <= 0 {
            bad.push(format!(
                "statement {}: MoneyWiz holds no money paid in by then",
                s.as_of
            ));
        }
        if bad.is_empty() && (rest < 0 || (rest > 0 && rest_weight <= 0.0)) {
            bad.push(format!(
                "statement {}: the money paid in by then does not fit the broker's first day ({})",
                s.as_of, first
            ));
        }
        if !bad.is_empty() || lots.is_empty() {
            problems.extend(bad.into_iter().map(|p| format!("{}: {p}", a.name)));
            continue;
        }
        // Split `total` cents by `weights`; the last share takes the rounding.
        let split = |total: i64, weights: &[f64]| -> Vec<i64> {
            let sum: f64 = weights.iter().sum();
            let mut out: Vec<i64> = weights
                .iter()
                .map(|w| {
                    if sum > 0.0 {
                        (total as f64 * w / sum).round() as i64
                    } else {
                        0
                    }
                })
                .collect();
            if let Some(k) = weights.iter().rposition(|w| *w > 0.0) {
                out[k] += total - out.iter().sum::<i64>();
            }
            out
        };
        let at_lump: Vec<f64> = lots.iter().map(|&(i, u)| u * price(i)).collect();
        let rest_w: Vec<f64> = lots.iter().map(|&(i, u)| left(i, u) * price(i)).collect();
        let lot_cents = split(paid, &at_lump);
        let rest_cents = split(rest, &rest_w);
        let rests: Vec<f64> = lots.iter().map(|&(i, u)| left(i, u)).collect();
        for (k, &(i, u)) in lots.iter().enumerate() {
            let mut lot = trades[i].clone();
            lot.date = s.as_of;
            lot.shares = u;
            lot.cad = Some(lot_cents[k] as f64 / 100.0);
            let r = rests[k];
            if r == 0.0 {
                gone.push(i);
            } else {
                trades[i].shares = r;
                trades[i].cad = Some(rest_cents[k] as f64 / 100.0);
            }
            added.push(lot);
        }
    }
    gone.sort_unstable();
    for i in gone.into_iter().rev() {
        trades.remove(i);
    }
    // In date order: `compare` prices a symbol at its last trade.
    for lot in added {
        let at = trades
            .iter()
            .position(|t| t.date > lot.date)
            .unwrap_or(trades.len());
        trades.insert(at, lot);
    }
    problems
}

/// One fund of a statement against MoneyWiz on the statement's day.
pub struct StatementFund {
    pub fund: String,
    pub statement_units: f64,
    /// The MoneyWiz symbol it pairs with, and its units on that day.
    pub moneywiz: Option<(String, f64)>,
}

/// A statement's units per fund against a MoneyWiz investment account.
pub struct StatementCheck {
    pub account: String,
    pub file: String,
    pub as_of: chrono::NaiveDate,
    pub funds: Vec<StatementFund>,
    /// MoneyWiz symbols holding units the statement does not list.
    pub extra: Vec<(String, f64)>,
    /// MoneyWiz's cash in the account that day (the plan holds none).
    pub cash_cents: i64,
    /// Every fund within max(0.01, 0.1 %) units, nothing extra, no cash.
    pub matches: bool,
}

fn units_agree(a: f64, b: f64) -> bool {
    (a - b).abs() <= (b.abs() * 0.001).max(0.01)
}

/// The symbol a statement fund names: the same name, else the one symbol
/// sharing most of its words (at least half; ties go to the one with the
/// fewest other words).
fn pair_symbol(fund: &str, symbols: &[String]) -> Option<String> {
    let f = norm(fund);
    if let Some(s) = symbols.iter().find(|s| **s == f) {
        return Some(s.clone());
    }
    let words = |s: &str| -> std::collections::BTreeSet<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let fw = words(&f);
    // Most shared words (at least half), then fewest words in all.
    let mut best: Vec<((usize, usize), &String)> = symbols
        .iter()
        .map(|s| {
            let sw = words(s);
            let n = fw.intersection(&sw).count();
            ((n, fw.union(&sw).count()), s)
        })
        .filter(|((n, _), s)| *n * 2 >= fw.len().max(words(s).len()) && *n > 0)
        .collect();
    best.sort_by(|a, b| b.0.0.cmp(&a.0.0).then(a.0.1.cmp(&b.0.1)));
    match best.as_slice() {
        [(k, s), rest @ ..] if rest.first().is_none_or(|r| r.0 != *k) => Some((*s).clone()),
        _ => None,
    }
}

/// Each Sun Life statement against the MoneyWiz account that
/// `[moneywiz.investments]` maps to Sun Life files of its plan type: units
/// per fund from the opening shares and the buy and sell rows up to the
/// statement's day.
pub fn statement_checks(
    store: &Store,
    snapshots: &[crate::pdf::sunlife::Snapshot],
    map: &BTreeMap<String, String>,
) -> Vec<StatementCheck> {
    let mut out = Vec::new();
    for s in snapshots {
        for a in &store.accounts {
            let Some(want) = map.get(&a.name) else {
                continue;
            };
            if !specs(want).iter().any(|w| {
                w.format == crate::ledger::SUNLIFE && w.bucket.eq_ignore_ascii_case(&s.plan)
            }) {
                continue;
            }
            let mut units: BTreeMap<String, f64> = BTreeMap::new();
            for t in store
                .txns
                .iter()
                .filter(|t| t.account == a.pk && t.date <= s.as_of)
            {
                if let Some(tr) = store.trades.get(&t.pk) {
                    let sign = if t.entity.contains("Sell") { -1.0 } else { 1.0 };
                    *units.entry(norm(&tr.symbol)).or_default() += sign * tr.shares.abs();
                }
            }
            for o in store.holdings.iter().filter(|o| o.account == a.pk) {
                *units.entry(norm(&o.symbol)).or_default() += o.shares;
            }
            let symbols: Vec<String> = units.keys().cloned().collect();
            let mut used: Vec<String> = Vec::new();
            let funds: Vec<StatementFund> = s
                .funds
                .iter()
                .map(|f| {
                    // Symbols an earlier fund took are out of the running.
                    let left: Vec<String> = symbols
                        .iter()
                        .filter(|x| !used.contains(x))
                        .cloned()
                        .collect();
                    let sym = pair_symbol(&f.name, &left);
                    if let Some(x) = &sym {
                        used.push(x.clone());
                    }
                    StatementFund {
                        fund: f.name.clone(),
                        statement_units: f.units,
                        moneywiz: sym.map(|x| {
                            let u = units[&x];
                            (x, u)
                        }),
                    }
                })
                .collect();
            let extra: Vec<(String, f64)> = units
                .iter()
                .filter(|(k, v)| !used.contains(k) && v.abs() > 0.01)
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            let cash_cents = store.balance_on(a.pk, s.as_of);
            let matches = extra.is_empty()
                && cash_cents.abs() <= 100
                && funds.iter().all(|f| {
                    f.moneywiz
                        .as_ref()
                        .is_some_and(|(_, u)| units_agree(*u, f.statement_units))
                });
            out.push(StatementCheck {
                account: a.name.clone(),
                file: s.file.clone(),
                as_of: s.as_of,
                funds,
                extra,
                cash_cents,
                matches,
            });
        }
    }
    out
}

/// The checks as a `diagnose` section.
pub fn render_statement_checks(checks: &[StatementCheck]) -> String {
    if checks.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n## Statements against MoneyWiz units\n\nUnits per fund on each statement's last day (Sun Life statements list no rows). A match proves the account's rows up to that day; `coverage` counts them.\n\n| Account | On | Fund | Statement units | MoneyWiz symbol | MoneyWiz units | Agrees |\n|---|---|---|---:|---|---:|---|\n",
    );
    for c in checks {
        for f in &c.funds {
            let (sym, u, ok) = match &f.moneywiz {
                Some((s, u)) => (
                    s.as_str(),
                    format!("{u:.4}"),
                    if units_agree(*u, f.statement_units) {
                        "yes"
                    } else {
                        "no"
                    },
                ),
                None => ("-", "-".into(), "no symbol"),
            };
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "| {} | {} | {} | {:.4} | {} | {} | {} |\n",
                    c.account, c.as_of, f.fund, f.statement_units, sym, u, ok
                ),
            );
        }
        for (s, u) in &c.extra {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "| {} | {} | (not on the statement) | 0.0000 | {s} | {u:.4} | no |\n",
                    c.account, c.as_of
                ),
            );
        }
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "| {} | {} | MoneyWiz cash | 0.00 | - | {} | {} |\n",
                c.account,
                c.as_of,
                crate::money::fmt_cents(c.cash_cents),
                if c.cash_cents.abs() <= 100 {
                    "yes"
                } else {
                    "no"
                }
            ),
        );
    }
    out
}
