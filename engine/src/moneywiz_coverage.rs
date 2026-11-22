//! `moneywiz coverage`: how much of MoneyWiz an import would have produced.
//!
//! For each MoneyWiz account that is not cash, it counts the MoneyWiz rows
//! and the ones a bank or card row in the inbox matches (the same match as
//! `moneywiz sync`). Every account that is not fully covered gets one gap:
//! the export to download, the link to add in `cashcast.toml`, or the reader
//! or connector that is missing. The report holds account names, connector
//! ids, dates, counts and shares only: no amounts, payees or row text.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use chrono::NaiveDate;
use serde::Deserialize;

use crate::config::MoneyWiz;
use crate::export::AccountKind;
use crate::inbox::Inbox;
use crate::moneywiz::{self, Store};

/// Formats that read cash rows (bank, card, wallet) into the import. The
/// others in a connector's list are investment and crypto ledgers.
pub const CASH_FORMATS: &[&str] = &[
    "scotiabank-csv",
    "scotiabank-legacy-csv",
    "td-csv",
    "wise-statement-csv",
    "wealthsimple-statement-csv",
    "paypal-activity-csv",
    "tinkoff-csv",
    "ofx",
    "generic-csv",
    crate::pdf::TD_CREDIT,
    crate::pdf::TD_CHEQUING,
    crate::pdf::SCOTIA_VISA,
    crate::pdf::SCOTIA_CHEQUING,
    crate::pdf::ETRADE_STOCKPLAN,
    crate::ledger::SUNLIFE,
    crate::ledger::ETRADE,
    crate::ledger::KRAKEN,
];

/// API calls that return transactions (see `connectors/*.toml` `[api] read`).
const TRANSACTION_CALLS: &[&str] = &[
    "transactions",
    "balance_statements",
    "ledgers",
    "query_ledgers",
    "export_report",
];

#[derive(Deserialize, Default)]
struct ConnectorFile {
    id: String,
    name: String,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    import: Import,
    #[serde(default)]
    api: Api,
}

#[derive(Deserialize, Default)]
struct Import {
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    no_export_reason: String,
}

#[derive(Deserialize, Default)]
struct Api {
    #[serde(default)]
    available: bool,
    #[serde(default)]
    read: Vec<String>,
}

pub struct Connector {
    pub id: String,
    pub name: String,
    pub kinds: Vec<String>,
    pub formats: Vec<String>,
    pub no_export: bool,
    pub api_transactions: bool,
}

impl Connector {
    pub fn cash_formats(&self) -> Vec<&str> {
        self.formats
            .iter()
            .map(String::as_str)
            .filter(|f| CASH_FORMATS.contains(f))
            .collect()
    }
}

/// Reads `connectors/*.toml`. A file that does not parse is skipped.
pub fn load_connectors(dir: &Path) -> Vec<Connector> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|e| e == "toml"));
    paths.sort();
    paths
        .iter()
        .filter_map(|p| toml::from_str::<ConnectorFile>(&std::fs::read_to_string(p).ok()?).ok())
        .filter(|c| c.id != "moneywiz")
        .map(|c| Connector {
            api_transactions: c.api.available
                && c.api
                    .read
                    .iter()
                    .any(|r| TRANSACTION_CALLS.contains(&r.as_str())),
            no_export: !c.import.no_export_reason.is_empty(),
            id: c.id,
            name: c.name,
            kinds: c.kinds,
            formats: c.import.formats,
        })
        .collect()
}

fn tokens(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The connector a MoneyWiz account or export name belongs to: its id, or
/// the first word of its name, is a word of the account name.
fn connector_for<'a>(name: &str, connectors: &'a [Connector]) -> Option<&'a Connector> {
    let words = tokens(name);
    connectors.iter().find(|c| {
        let first = tokens(&c.name).into_iter().next().unwrap_or_default();
        words.iter().any(|w| *w == c.id || *w == first)
    })
}

pub struct AccountCoverage {
    pub name: String,
    pub kind: String,
    pub rows: usize,
    pub covered: usize,
    pub rows_recent: usize,
    pub covered_recent: usize,
    pub first: Option<NaiveDate>,
    pub last: Option<NaiveDate>,
    pub connector: Option<String>,
    pub source: String,
    pub gap: Option<String>,
    /// From `[moneywiz] since` on (0 without it): MoneyWiz rows, those a bank
    /// row matches, and bank rows MoneyWiz lacks.
    pub rows_since: usize,
    pub covered_since: usize,
    pub to_add_since: usize,
    /// What closes the gap from `since` on; None when there is none.
    pub since_gap: Option<String>,
    /// In `[moneywiz.out_of_scope]`: left out of the goal, with this reason.
    pub left_out: Option<String>,
    /// Days the linked export holds (for the gap since `since`).
    pub export_days: Option<(NaiveDate, NaiveDate)>,
    /// MoneyWiz rows from `since` on, by date (for the gap since `since`).
    pub dates_since: Vec<(i64, NaiveDate)>,
    /// Days of a PDF statement missing between two that do not chain.
    pub missing_statements: Vec<String>,
    /// An investment account's last PDF statement day whose closing balance
    /// MoneyWiz's balance equals: with the broker files pairing every share
    /// move, it proves the rows to that day in sum (`stock_plan_checks`).
    pub statement_cash: Option<NaiveDate>,
}

pub struct Coverage {
    pub accounts: Vec<AccountCoverage>,
    /// (MoneyWiz account, export account) pairs the owner can add to
    /// `[moneywiz.accounts]`: one export of the same bank and kind.
    pub suggestions: Vec<(String, String)>,
    /// Connector id and why it has no cash reader.
    pub reader_gaps: Vec<(String, String)>,
    /// Rows covered (all time, last 12 months) with the suggested links.
    pub with_links: Option<(usize, usize)>,
    /// Per linked export (with the suggested links): how its rows pair up.
    pub matches: Vec<MatchRow>,
    /// `[moneywiz] since`: the day coverage must be complete from.
    pub since: Option<NaiveDate>,
    /// What the "Open MoneyWiz rows" list shows of a row from `since` on
    /// that no bank file matched, by row: never its payee or text.
    pub open_rows: BTreeMap<i64, OpenRow>,
}

/// One MoneyWiz row with no bank row: enough for the owner to decide it.
pub struct OpenRow {
    pub cents: i64,
    /// The row's kind without "Transaction" ("Withdraw", "TransferDeposit").
    pub kind: String,
    /// The other account of a transfer.
    pub other: Option<String>,
}

fn export_kind(inbox: &Inbox, account: &str) -> AccountKind {
    inbox
        .files
        .iter()
        .find_map(|f| f.account_kinds.get(account).copied())
        .unwrap_or_else(|| {
            let t = tokens(account);
            if t.iter()
                .any(|w| ["visa", "mastercard", "amex", "card", "credit"].contains(&w.as_str()))
            {
                AccountKind::Card
            } else {
                AccountKind::Bank
            }
        })
}

/// The connector of an export account: from its file's format, else its name.
fn export_connector<'a>(
    inbox: &Inbox,
    account: &str,
    connectors: &'a [Connector],
) -> Option<&'a Connector> {
    let file = inbox
        .txns
        .iter()
        .find(|t| t.account == account)
        .and_then(|t| inbox.files.get(t.file))?;
    let described = format!("{} {}", file.format, file.notes.join(" "));
    connectors
        .iter()
        .filter(|c| !c.cash_formats().is_empty())
        .find(|c| {
            c.cash_formats()
                .iter()
                .any(|f| *f != "ofx" && *f != "generic-csv" && described.contains(f))
        })
        .or_else(|| connector_for(account, connectors))
        .or_else(|| connector_for(&file.file_name, connectors))
}

pub fn coverage(
    store: &Store,
    inbox: &Inbox,
    cfg: Option<&MoneyWiz>,
    connectors: &[Connector],
    today: NaiveDate,
) -> Result<Coverage, String> {
    let plan = moneywiz::plan(store, inbox, cfg)?;
    let days = cfg.and_then(|c| c.match_days).unwrap_or(4).clamp(0, 31);
    let recent = today - chrono::Duration::days(365);
    let since = cfg
        .and_then(|c| c.since.as_deref())
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let mut covered: HashSet<i64> = HashSet::new();
    let mut linked: BTreeMap<i64, &moneywiz::AccountPlan> = BTreeMap::new();
    for ap in &plan.accounts {
        covered.extend(ap.matched_mw.iter().copied());
        covered.extend(ap.stood_in.iter().copied());
        linked.insert(ap.mw, ap);
    }
    let by_transfer = transfer_pairs(store, &covered, &linked);
    covered.extend(by_transfer.iter().copied());
    // Rows the owner accepts as MoneyWiz's own record (no bank file).
    let accepted = accepted_days(cfg)?;
    for t in &store.txns {
        // A 0.00 cash row changes no balance, and no bank file lists one.
        if t.cents == 0 && t.is_cash() {
            covered.insert(t.pk);
        }
        let name = store.account(t.account).map(|a| a.name.as_str());
        if name
            .and_then(|n| accepted.get(n))
            .is_some_and(|day| t.date <= *day)
        {
            covered.insert(t.pk);
        }
    }

    // Exports with no MoneyWiz account, by connector and kind.
    let mut suggestions = Vec::new();
    // (export account, connector id, kind, from an investment ledger)
    let mut open: Vec<(&str, &str, AccountKind, bool)> = Vec::new();
    for e in &plan.unlinked_exports {
        if let Some(c) = export_connector(inbox, e, connectors) {
            let ledger = inbox
                .txns
                .iter()
                .find(|t| t.account == *e)
                .and_then(|t| inbox.files.get(t.file))
                .is_some_and(|f| {
                    crate::ledger::CASH_LEDGERS.contains(&f.format.as_str())
                        || f.format == crate::pdf::ETRADE_STOCKPLAN
                });
            open.push((e.as_str(), c.id.as_str(), export_kind(inbox, e), ledger));
        }
    }
    let named = |account: &str| {
        cfg.and_then(|c| c.connectors.get(account))
            .and_then(|id| connectors.iter().find(|c| c.id == *id))
    };

    let mut accounts = Vec::new();
    for a in &store.accounts {
        if a.kind == "CashAccount" {
            continue;
        }
        let rows: Vec<_> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && !t.is_correction())
            .collect();
        let count = |only_recent: bool, only_covered: bool| {
            rows.iter()
                .filter(|t| !only_recent || t.date >= recent)
                .filter(|t| !only_covered || covered.contains(&t.pk))
                .count()
        };
        let connector = named(&a.name).or_else(|| connector_for(&a.name, connectors));
        let out_of_scope = cfg.and_then(|c| c.out_of_scope.get(&a.name));
        // Only bank and card accounts get a suggested link: an investment
        // or crypto account at the same bank is not its chequing export.
        let kind = match a.kind.as_str() {
            "CreditCardAccount" => Some(AccountKind::Card),
            "BankChequeAccount" | "BankSavingAccount" => Some(AccountKind::Bank),
            _ => None,
        };
        // An investment account takes the cash rows of its broker's ledger.
        let investment = matches!(a.kind.as_str(), "InvestmentAccount" | "ForexAccount");
        let mut ac = AccountCoverage {
            name: a.name.clone(),
            kind: a.kind.trim_end_matches("Account").to_string(),
            rows: rows.len(),
            covered: count(false, true),
            rows_recent: count(true, false),
            covered_recent: count(true, true),
            first: rows.iter().map(|t| t.date).min(),
            last: rows.iter().map(|t| t.date).max(),
            connector: connector.map(|c| c.id.clone()),
            source: "-".into(),
            gap: None,
            rows_since: 0,
            covered_since: 0,
            to_add_since: 0,
            since_gap: None,
            left_out: out_of_scope.cloned(),
            export_days: None,
            dates_since: Vec::new(),
            missing_statements: Vec::new(),
            statement_cash: None,
        };
        if let Some(s) = since {
            ac.dates_since = rows
                .iter()
                .filter(|t| t.date >= s && !covered.contains(&t.pk))
                .map(|t| (t.pk, t.date))
                .collect();
            ac.rows_since = rows.iter().filter(|t| t.date >= s).count();
            ac.covered_since = ac.rows_since - ac.dates_since.len();
        }
        if !linked.contains_key(&a.pk) && rows.iter().any(|t| by_transfer.contains(&t.pk)) {
            ac.source = "transfers a bank row matches".into();
        }
        let accepted_to = accepted
            .get(&a.name)
            .filter(|day| rows.iter().any(|t| t.date <= **day));
        if let Some(ap) = linked.get(&a.pk) {
            ac.to_add_since = if since.is_some() { ap.add.len() } else { 0 };
            ac.export_days = ap.from.zip(ap.to);
            let sts = crate::moneywiz_diagnose::statements(inbox, &ap.export_account);
            ac.missing_statements =
                crate::moneywiz_diagnose::unexplained_breaks(&sts, |d| store.balance_on(a.pk, d))
                    .into_iter()
                    .map(crate::moneywiz_diagnose::missing_days)
                    .collect();
            if investment {
                ac.statement_cash = sts
                    .iter()
                    .filter(|s| s.closing == store.balance_on(a.pk, s.end))
                    .map(|s| s.end)
                    .max();
            }
            let formats: Vec<String> = {
                let mut f: Vec<String> = inbox
                    .txns
                    .iter()
                    .filter(|t| t.account == ap.export_account)
                    .filter_map(|t| inbox.files.get(t.file).map(|f| f.format.clone()))
                    // A statement-only file (no rows) names its format too.
                    .chain(sts_formats(inbox, &ap.export_account))
                    .collect();
                f.sort();
                f.dedup();
                f
            };
            ac.source = format!("{} ({})", ap.export_account, formats.join(", "));
            if let Some(day) = accepted_to {
                ac.source = format!("{} + owner accepted MoneyWiz to {day}", ac.source);
            }
            if ac.covered < ac.rows {
                ac.gap = Some(match (ap.from, ap.to, ap.note.as_deref()) {
                    (_, _, Some(note)) if note.starts_with("skipped") => note.to_string(),
                    (Some(from), Some(to), _) => {
                        let mut want = Vec::new();
                        if ac.first.is_some_and(|f| f < from) {
                            want.push(format!(
                                "older rows (MoneyWiz starts {})",
                                ac.first.unwrap()
                            ));
                        }
                        if ac.last.is_some_and(|l| l > to) {
                            want.push(format!("newer rows (MoneyWiz ends {})", ac.last.unwrap()));
                        }
                        if want.is_empty() {
                            format!(
                                "the export ({from} to {to}) lacks some MoneyWiz rows: see `moneywiz sync --dry-run`"
                            )
                        } else {
                            format!(
                                "the export holds {from} to {to}; download {}",
                                want.join(" and ")
                            )
                        }
                    }
                    _ => "the export has no rows in the period".into(),
                });
            }
        } else if let (Some(why), true) = (out_of_scope, ac.rows > 0) {
            ac.gap = Some(format!("out of scope: {why}"));
        } else if ac.covered < ac.rows {
            ac.gap = Some(match connector {
                None => "no connector for this bank yet".into(),
                Some(c) => {
                    let candidates: Vec<&str> = open
                        .iter()
                        .filter(|(_, id, k, ledger)| {
                            *id == c.id
                                && if investment {
                                    *ledger
                                } else {
                                    !*ledger && Some(*k) == kind
                                }
                        })
                        .map(|(e, _, _, _)| *e)
                        .collect();
                    let cash = c.cash_formats();
                    if let [one] = candidates.as_slice() {
                        suggestions.push((a.name.clone(), one.to_string()));
                        "an export of this bank is in the inbox but not linked: see the suggested links".to_string()
                    } else if candidates.len() > 1 {
                        format!(
                            "{} exports of this bank and kind are in the inbox: link one in [moneywiz.accounts]",
                            candidates.len()
                        )
                    } else if !cash.is_empty() {
                        format!(
                            "no export in the inbox: download one ({}), see `connectors show {}`",
                            cash.join(", "),
                            c.id
                        )
                    } else if c.no_export {
                        format!(
                            "{} has no export; its rows come through a card export",
                            c.id
                        )
                    } else if c.formats.is_empty() {
                        format!("reader missing: {} has no reader", c.id)
                    } else {
                        format!(
                            "reader missing: {} has only investment and crypto ledger readers ({}); none makes MoneyWiz rows",
                            c.id,
                            c.formats.join(", ")
                        )
                    }
                }
            });
        }
        accounts.push(ac);
    }

    let reader_gaps = connectors
        .iter()
        .filter(|c| c.cash_formats().is_empty())
        .filter(|c| {
            c.api_transactions
                || c.kinds.iter().any(|k| {
                    [
                        "bank",
                        "card",
                        "payments",
                        "crypto",
                        "broker",
                        "retirement",
                        "fx",
                    ]
                    .contains(&k.as_str())
                })
        })
        .map(|c| {
            let why = if c.no_export {
                "no export (its rows come through a card export)".to_string()
            } else if c.formats.is_empty() {
                "no reader".to_string()
            } else {
                format!("only ledger readers ({})", c.formats.join(", "))
            };
            let api = if c.api_transactions {
                "; its API has transactions"
            } else {
                ""
            };
            (c.id.clone(), format!("{why}{api}"))
        })
        .collect();

    // An export that fits more than one MoneyWiz account is not suggested.
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, e) in &suggestions {
        *uses.entry(e.as_str()).or_insert(0) += 1;
    }
    let ambiguous: HashSet<String> = uses
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(e, _)| e.to_string())
        .collect();
    suggestions.retain(|(_, e)| !ambiguous.contains(e));
    for ac in &mut accounts {
        if ac
            .gap
            .as_deref()
            .is_some_and(|g| g.ends_with("see the suggested links"))
            && !suggestions.iter().any(|(mw, _)| *mw == ac.name)
        {
            ac.gap = Some("an export of this bank is in the inbox but fits more than one account: link it in [moneywiz.accounts]".into());
        }
    }

    // What the same inbox would cover with the suggested links.
    let mut matches = match_rows(store, inbox, &plan, days);
    let with_links = if suggestions.is_empty() {
        None
    } else {
        let mut cfg = cfg.cloned().unwrap_or_default();
        for (mw, e) in &suggestions {
            cfg.accounts.insert(mw.clone(), e.clone());
        }
        let plan = moneywiz::plan(store, inbox, Some(&cfg))?;
        matches = match_rows(store, inbox, &plan, days);
        let covered: HashSet<i64> = plan
            .accounts
            .iter()
            .flat_map(|ap| ap.matched_mw.iter().chain(&ap.stood_in).copied())
            .collect();
        let pks: Vec<(i64, NaiveDate)> = store
            .txns
            .iter()
            .filter(|t| !t.is_correction())
            .filter(|t| {
                store
                    .account(t.account)
                    .is_some_and(|a| a.kind != "CashAccount")
            })
            .map(|t| (t.pk, t.date))
            .collect();
        let all = pks.iter().filter(|(pk, _)| covered.contains(pk)).count();
        let recent_n = pks
            .iter()
            .filter(|(pk, d)| *d >= recent && covered.contains(pk))
            .count();
        Some((all, recent_n))
    };

    let mut c = Coverage {
        accounts,
        suggestions,
        reader_gaps,
        with_links,
        matches,
        since,
        open_rows: open_rows(store, since, &covered),
    };
    since_gaps(&mut c, today);
    Ok(c)
}

/// What each account lacks from `[moneywiz] since` on: the days to download
/// (before or after what its export holds), bank rows to add, and MoneyWiz
/// rows no bank row matches. Called again after `count_trades`.
/// Transfer rows of accounts with no linked export whose other side a bank
/// row matched: the bank row proves the amount and the day. The other side
/// is the opposite transfer between the same two accounts within 4 days,
/// within 2 % of the amount (a wire fee, or a bank amount `edit --fixes`
/// wrote on that side only); any amount between accounts in two
/// currencies. Each other side counts once.
fn transfer_pairs(
    store: &Store,
    covered: &HashSet<i64>,
    linked: &BTreeMap<i64, &moneywiz::AccountPlan>,
) -> HashSet<i64> {
    let currency = |pk: i64| {
        store
            .accounts
            .iter()
            .find(|a| a.pk == pk)
            .map(|a| a.currency.as_str())
    };
    let mut used: HashSet<i64> = HashSet::new();
    let mut out = HashSet::new();
    for t in &store.txns {
        let Some(other) = t.other_account else {
            continue;
        };
        if linked.contains_key(&t.account) || covered.contains(&t.pk) || t.is_correction() {
            continue;
        }
        let same_currency = currency(t.account) == currency(other);
        let pair = store
            .txns
            .iter()
            .filter(|o| {
                o.account == other
                    && o.other_account == Some(t.account)
                    && o.cents.signum() == -t.cents.signum()
                    && (o.date - t.date).num_days().abs() <= 4
                    && (!same_currency || (o.cents + t.cents).abs() * 50 <= t.cents.abs())
                    && covered.contains(&o.pk)
                    && !used.contains(&o.pk)
            })
            .min_by_key(|o| {
                (
                    (o.date - t.date).num_days().abs(),
                    (o.cents + t.cents).abs(),
                )
            });
        if let Some(o) = pair {
            used.insert(o.pk);
            out.insert(t.pk);
        }
    }
    out
}

fn since_gaps(c: &mut Coverage, today: NaiveDate) {
    let Some(since) = c.since else {
        return;
    };
    let day = chrono::Duration::days(1);
    for ac in &mut c.accounts {
        ac.since_gap = None;
        let open = ac.dates_since.len();
        if open == 0 && ac.to_add_since == 0 && ac.missing_statements.is_empty() {
            continue;
        }
        let mut want: Vec<String> = ac
            .missing_statements
            .iter()
            .map(|m| format!("a statement is missing: download {m}"))
            .collect();
        match ac.export_days {
            Some((from, to)) => {
                let first = ac.dates_since.iter().map(|(_, d)| *d).min();
                let last = ac.dates_since.iter().map(|(_, d)| *d).max();
                if first.is_some_and(|f| f < from) {
                    want.push(format!("download {since} to {}", from - day));
                }
                if last.is_some_and(|l| l > to) {
                    want.push(format!("download {} to {today}", to + day));
                }
                if ac.to_add_since > 0 {
                    want.push(format!(
                        "{} bank row{} to add: `moneywiz sync`",
                        ac.to_add_since,
                        if ac.to_add_since == 1 { "" } else { "s" }
                    ));
                }
                let inside = ac
                    .dates_since
                    .iter()
                    .filter(|(_, d)| *d >= from && *d <= to)
                    .count();
                if inside > 0 {
                    want.push(format!(
                        "{inside} MoneyWiz row{} with no bank row: `moneywiz sync --dry-run`, then `moneywiz edit --fixes`",
                        if inside == 1 { "" } else { "s" }
                    ));
                }
            }
            None => {
                let first = ac.dates_since.iter().map(|(_, d)| *d).min();
                let last = ac.dates_since.iter().map(|(_, d)| *d).max();
                let what = ac.gap.clone().unwrap_or_else(|| "no export linked".into());
                match (first, last) {
                    (Some(f), Some(l)) => want.push(format!("{what}; rows {f} to {l}")),
                    _ => want.push(what),
                }
            }
        }
        ac.since_gap = Some(want.join("; "));
    }
}

/// Why bank rows found no MoneyWiz row: (opposite sign, the same amount on
/// a date further than `match_days`, an amount within 1.00 with another
/// payee, nothing alike within 30 days).
fn unmatched(
    store: &Store,
    inbox: &Inbox,
    ap: &moneywiz::AccountPlan,
    days: i64,
) -> (usize, usize, usize, usize) {
    let rows: Vec<_> = store
        .txns
        .iter()
        .filter(|t| t.account == ap.mw && t.is_cash() && !t.is_correction())
        .collect();
    let (mut flipped, mut far, mut close, mut none) = (0, 0, 0, 0);
    for &i in &ap.add {
        let b = &inbox.txns[i];
        let within = |cents: i64, max: i64| {
            rows.iter()
                .any(|m| m.cents == cents && (m.date - b.date).num_days().abs() <= max)
        };
        let rounded = rows.iter().any(|m| {
            (m.cents - b.cents).abs() <= 100
                && (m.cents < 0) == (b.cents < 0)
                && (m.date - b.date).num_days().abs() <= days
        });
        if b.cents != 0 && within(-b.cents, days) {
            flipped += 1;
        } else if within(b.cents, 30) {
            far += 1;
        } else if rounded {
            close += 1;
        } else {
            none += 1;
        }
    }
    (flipped, far, close, none)
}

/// How the bank rows of each linked export pair with MoneyWiz rows.
fn match_rows(store: &Store, inbox: &Inbox, plan: &moneywiz::Plan, days: i64) -> Vec<MatchRow> {
    plan.accounts
        .iter()
        .map(|ap| MatchRow {
            unmatched: unmatched(store, inbox, ap, days),
            account: store
                .account(ap.mw)
                .map_or_else(String::new, |a| a.name.clone()),
            export: ap.export_account.clone(),
            bank_rows: ap.bank_rows,
            matched: ap.matched,
            differ: ap.fix.len(),
            conflicts: ap.conflicts.iter().map(|c| c.rows.len()).sum(),
            to_add: ap.add.len(),
            only_in_moneywiz: ap.only_in_moneywiz.len(),
            note: ap.note.clone(),
        })
        .collect()
}

pub struct MatchRow {
    pub account: String,
    pub export: String,
    pub bank_rows: usize,
    pub matched: usize,
    pub differ: usize,
    pub conflicts: usize,
    pub to_add: usize,
    pub only_in_moneywiz: usize,
    pub note: Option<String>,
    /// Rows to add by why they found no match (see `unmatched`).
    pub unmatched: (usize, usize, usize, usize),
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        "-".into()
    } else {
        format!("{:.1} %", part as f64 * 100.0 / whole as f64)
    }
}

/// Rows from `since` on that no bank row matched (later checks may still
/// prove some; the list shows only those left in `dates_since`).
fn open_rows(
    store: &Store,
    since: Option<NaiveDate>,
    covered: &HashSet<i64>,
) -> BTreeMap<i64, OpenRow> {
    let Some(since) = since else {
        return BTreeMap::new();
    };
    store
        .txns
        .iter()
        .filter(|t| t.date >= since && !covered.contains(&t.pk))
        .map(|t| {
            let kind = t.entity.strip_suffix("Transaction").unwrap_or(&t.entity);
            let other = t
                .other_account
                .and_then(|o| store.account(o))
                .map(|a| a.name.clone());
            (
                t.pk,
                OpenRow {
                    cents: t.cents,
                    kind: kind.to_string(),
                    other,
                },
            )
        })
        .collect()
}

/// Up to this many open rows are listed; the rest are counted.
const OPEN_ROWS_SHOWN: usize = 30;

/// The open rows, one table line each, oldest first within an account.
fn render_open_rows(c: &Coverage, goal: &[&AccountCoverage], day: NaiveDate) -> String {
    let mut lines = Vec::new();
    for a in goal {
        let mut rows = a.dates_since.clone();
        rows.sort_by_key(|(pk, d)| (*d, *pk));
        for (pk, d) in rows {
            let (amount, kind, other) = match c.open_rows.get(&pk) {
                Some(r) => (
                    crate::money::fmt_cents(r.cents),
                    r.kind.as_str(),
                    r.other.as_deref().unwrap_or("-"),
                ),
                None => ("-".to_string(), "-", "-"),
            };
            lines.push(format!(
                "| {} | {d} | {amount} | {kind} | {other} |",
                a.name
            ));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "Open MoneyWiz rows since {day} ({}): no bank row proves them. For each, download the bank's file for that day, fix the row with `moneywiz edit`, or accept MoneyWiz to that day in `[moneywiz.accepted]`.\n| MoneyWiz account | Day | Amount | Kind | Other account |\n|---|---|---|---|---|\n",
        lines.len()
    );
    for l in lines.iter().take(OPEN_ROWS_SHOWN) {
        out.push_str(l);
        out.push('\n');
    }
    if lines.len() > OPEN_ROWS_SHOWN {
        out.push_str(&format!("…and {} more.\n", lines.len() - OPEN_ROWS_SHOWN));
    }
    out
}

/// The goal from `[moneywiz] since` on: every MoneyWiz row matched and no
/// bank row missing. One "History gaps since" line names what is left (the
/// weekly run lists it under "Not up to date").
fn render_since(c: &Coverage, day: NaiveDate) -> String {
    let goal: Vec<&AccountCoverage> = c.accounts.iter().filter(|a| a.left_out.is_none()).collect();
    let rows: usize = goal.iter().map(|a| a.rows_since).sum();
    let covered: usize = goal.iter().map(|a| a.covered_since).sum();
    let to_add: usize = goal.iter().map(|a| a.to_add_since).sum();
    let mut out = format!(
        "Coverage since {day}: {covered} of {rows} MoneyWiz rows ({}), {to_add} bank rows not in MoneyWiz. Goal: 100 % and 0.\n",
        share(covered, rows)
    );
    let mut open: Vec<&&AccountCoverage> = goal.iter().filter(|a| a.since_gap.is_some()).collect();
    open.sort_by_key(|a| {
        (
            std::cmp::Reverse(a.rows_since - a.covered_since + a.to_add_since),
            a.name.clone(),
        )
    });
    if open.is_empty() {
        out.push_str(&format!("History gaps since {day}: none.\n"));
    } else {
        let names: Vec<String> = open
            .iter()
            .map(|a| {
                format!(
                    "{} ({})",
                    a.name,
                    a.since_gap.as_deref().unwrap_or_default()
                )
            })
            .collect();
        out.push_str(&format!(
            "History gaps since {day}: {}. Download each range (`bin/cashcast connectors show <bank>`), then `moneywiz sync` and `moneywiz edit --fixes`.\n",
            names.join(", ")
        ));
    }
    let left: Vec<String> = c
        .accounts
        .iter()
        .filter(|a| a.rows_since > 0)
        .filter_map(|a| Some(format!("{} ({})", a.name, a.left_out.as_deref()?)))
        .collect();
    if !left.is_empty() {
        out.push_str(&format!(
            "Left out on purpose: {}. `[moneywiz.out_of_scope]` in cashcast.toml.\n",
            left.join(", ")
        ));
    }
    out.push_str(&render_open_rows(c, &goal, day));
    out.push('\n');
    out
}

pub fn render(c: &Coverage, today: NaiveDate) -> String {
    let rows: usize = c.accounts.iter().map(|a| a.rows).sum();
    let covered: usize = c.accounts.iter().map(|a| a.covered).sum();
    let recent: usize = c.accounts.iter().map(|a| a.rows_recent).sum();
    let covered_recent: usize = c.accounts.iter().map(|a| a.covered_recent).sum();
    let since = today - chrono::Duration::days(365);
    let mut out = String::from(
        "# MoneyWiz coverage\n\nThe share of MoneyWiz rows (accounts that are not cash) that a bank or card row in the inbox matches: what an import would have produced. Counts only.\n\n",
    );
    out.push_str(&format!(
        "Coverage: {covered} of {rows} rows ({}), all time.\nCoverage, last 12 months (since {since}): {covered_recent} of {recent} rows ({}).\n\n",
        share(covered, rows),
        share(covered_recent, recent)
    ));
    if let Some(day) = c.since {
        out.push_str(&render_since(c, day));
    }
    out.push_str("| MoneyWiz account | Type | Rows | Covered | Last 12 months | Covered | First | Last | Connector | Source | Gap |\n|---|---|---:|---:|---:|---:|---|---|---|---|---|\n");
    let mut sorted: Vec<&AccountCoverage> = c.accounts.iter().filter(|a| a.rows > 0).collect();
    sorted.sort_by_key(|a| {
        (
            std::cmp::Reverse(a.rows_recent - a.covered_recent),
            std::cmp::Reverse(a.rows - a.covered),
            a.name.clone(),
        )
    });
    for a in &sorted {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            a.name,
            a.kind,
            a.rows,
            share(a.covered, a.rows),
            a.rows_recent,
            share(a.covered_recent, a.rows_recent),
            a.first.map_or("-".into(), |d| d.to_string()),
            a.last.map_or("-".into(), |d| d.to_string()),
            a.connector.as_deref().unwrap_or("-"),
            a.source,
            a.gap.as_deref().unwrap_or("none"),
        ));
    }
    let gaps: Vec<&&AccountCoverage> = sorted.iter().filter(|a| a.gap.is_some()).collect();
    out.push_str(&format!(
        "\n{} of {} accounts with rows have a gap; {} rows of the last 12 months are not covered.\n",
        gaps.len(),
        sorted.len(),
        recent - covered_recent
    ));
    if !c.suggestions.is_empty() {
        out.push_str("\nSuggested links (same bank, same kind; check each before you add it to cashcast.toml):\n\n```toml\n[moneywiz.accounts]\n");
        for (mw, export) in &c.suggestions {
            out.push_str(&format!("{mw:?} = {export:?}\n"));
        }
        out.push_str("```\n");
        if let Some((all, recent_n)) = c.with_links {
            out.push_str(&format!(
                "\nWith these links: {all} of {rows} rows ({}), last 12 months {recent_n} of {recent} ({}).\n",
                share(all, rows),
                share(recent_n, recent)
            ));
        }
    }
    if !c.matches.is_empty() {
        out.push_str("\nMatch rate per export (bank rows that pair with a MoneyWiz row; with the suggested links):\n\n| MoneyWiz account | Export account | Bank rows | Same amount | Amount differs | Conflicts | To add | Of those: sign flipped / date further / within 1.00 / nothing alike | Only in MoneyWiz | Match rate |\n|---|---|---:|---:|---:|---:|---:|---|---:|---:|\n");
        for m in &c.matches {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} / {} / {} / {} | {} | {} |\n",
                m.account,
                m.export,
                m.bank_rows,
                m.matched,
                m.differ,
                m.conflicts,
                m.to_add,
                m.unmatched.0,
                m.unmatched.1,
                m.unmatched.2,
                m.unmatched.3,
                m.only_in_moneywiz,
                m.note
                    .clone()
                    .unwrap_or_else(|| share(m.matched + m.differ, m.bank_rows))
            ));
        }
    }
    if !c.reader_gaps.is_empty() {
        out.push_str("\nConnectors with no cash reader:\n");
        for (id, why) in &c.reader_gaps {
            out.push_str(&format!("- {id}: {why}\n"));
        }
    }
    out
}

/// Counts as covered the MoneyWiz buy and sell rows that a broker trade
/// pairs with (`[moneywiz.investments]`, the pairing `diagnose` uses): the
/// broker files hold those rows, as an export holds a bank's.
pub fn add_trades(
    c: &mut Coverage,
    store: &Store,
    inbox_dir: &Path,
    cfg: &crate::config::Config,
    today: NaiveDate,
) {
    use crate::moneywiz_invest as inv;
    let Some(m) = cfg.moneywiz.as_ref().filter(|m| !m.investments.is_empty()) else {
        return;
    };
    let Ok(h) = crate::ledger::load_dir(inbox_dir, cfg) else {
        return;
    };
    let (adjust, _) = inv::holding_adjustments(&h.trades, &m.investments, &m.holdings);
    let mut trades = h.trades;
    trades.extend(adjust);
    let snaps = crate::pdf::sunlife::snapshots(inbox_dir);
    let _ = inv::statement_lots(store, &mut trades, &snaps, &m.investments);
    let rows = inv::compare(store, &trades, &m.investments);
    count_trades(c, store, &rows, today);
    let mut checks = inv::statement_checks(store, &snaps, &m.investments);
    checks.extend(stock_plan_checks(store, c, &rows));
    count_statements(c, store, &rows, &checks, today);
}

/// `[moneywiz.accepted]`: each account's last accepted day.
fn accepted_days(cfg: Option<&MoneyWiz>) -> Result<BTreeMap<String, NaiveDate>, String> {
    let mut out = BTreeMap::new();
    for (name, value) in cfg.map(|c| &c.accepted).into_iter().flatten() {
        let day = value
            .split(':')
            .next()
            .and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok())
            .ok_or_else(|| {
                format!("[moneywiz.accepted] \"{name}\": write \"YYYY-MM-DD: reason\"")
            })?;
        out.insert(name.clone(), day);
    }
    Ok(out)
}

/// Formats of the files holding a statement of `account`.
fn sts_formats(inbox: &Inbox, account: &str) -> Vec<String> {
    inbox
        .files
        .iter()
        .filter(|f| f.statements.iter().any(|s| s.account == account))
        .map(|f| f.format.clone())
        .collect()
}

/// A check per investment account whose statement cash MoneyWiz matches
/// (`statement_cash`) when the broker files pair every share move: no
/// broker trade missing, no MoneyWiz trade row unpaired or off, and the
/// same shares on both sides. Such a check proves the rows to that day.
pub fn stock_plan_checks(
    store: &Store,
    c: &Coverage,
    rows: &[crate::moneywiz_invest::Row],
) -> Vec<crate::moneywiz_invest::StatementCheck> {
    c.accounts
        .iter()
        .filter_map(|ac| {
            let day = ac.statement_cash?;
            let a = store.accounts.iter().find(|a| a.name == ac.name)?;
            let mine: Vec<_> = rows.iter().filter(|r| r.account == ac.name).collect();
            let paired = mine.iter().all(|r| {
                r.missing == 0
                    && r.off_rows == 0
                    && r.extra.is_empty()
                    && (r.moneywiz_shares - r.broker_shares).abs()
                        <= (r.broker_shares.abs() * 0.001).max(0.0001)
            });
            Some(crate::moneywiz_invest::StatementCheck {
                account: ac.name.clone(),
                file: String::new(),
                as_of: day,
                funds: Vec::new(),
                extra: Vec::new(),
                cash_cents: store.balance_on(a.pk, day),
                matches: paired,
            })
        })
        .collect()
}

/// MoneyWiz trade rows the broker files pair.
fn paired_rows(rows: &[crate::moneywiz_invest::Row]) -> HashSet<i64> {
    let mut paired: HashSet<i64> = HashSet::new();
    for r in rows {
        let extra: HashSet<i64> = r.extra.iter().map(|(pk, _)| *pk).collect();
        paired.extend(r.all_rows.iter().filter(|pk| !extra.contains(pk)));
    }
    paired
}

/// Statement units prove the rows before them in sum: when every fund on a
/// statement holds the same units in MoneyWiz (and no cash), each MoneyWiz
/// row of that account up to the statement's day counts as covered. The
/// latest matching statement wins; rows the broker files pair are not
/// counted twice.
pub fn count_statements(
    c: &mut Coverage,
    store: &Store,
    rows: &[crate::moneywiz_invest::Row],
    checks: &[crate::moneywiz_invest::StatementCheck],
    today: NaiveDate,
) {
    let recent = today - chrono::Duration::days(365);
    let paired = paired_rows(rows);
    for ac in &mut c.accounts {
        let Some(best) = checks
            .iter()
            .filter(|k| k.matches && k.account == ac.name)
            .max_by_key(|k| k.as_of)
        else {
            continue;
        };
        let day = best.as_of;
        // A stock plan check holds no funds: its statement proves the cash.
        let what = if best.funds.is_empty() {
            "statement cash"
        } else {
            "statement units"
        };
        let Some(a) = store.accounts.iter().find(|a| a.name == ac.name) else {
            continue;
        };
        let proven: HashSet<i64> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && !t.is_correction() && t.date <= day)
            .filter(|t| !paired.contains(&t.pk))
            .map(|t| t.pk)
            .collect();
        if proven.is_empty() {
            continue;
        }
        let dates: Vec<NaiveDate> = store
            .txns
            .iter()
            .filter(|t| proven.contains(&t.pk))
            .map(|t| t.date)
            .collect();
        // A row a bank file already matched is in `covered` too: cap.
        ac.covered = (ac.covered + dates.len()).min(ac.rows);
        ac.covered_recent = (ac.covered_recent + dates.iter().filter(|d| **d >= recent).count())
            .min(ac.rows_recent);
        let before = ac.dates_since.len();
        ac.dates_since.retain(|(pk, _)| !proven.contains(pk));
        ac.covered_since += before - ac.dates_since.len();
        ac.source = if ac.source == "-" {
            format!("{what} to {day}")
        } else {
            format!("{} + {what} to {day}", ac.source)
        };
        if ac.covered >= ac.rows {
            ac.gap = None;
        }
    }
    since_gaps(c, today);
}

/// `add_trades` on rows `moneywiz_invest::compare` made.
pub fn count_trades(
    c: &mut Coverage,
    store: &Store,
    rows: &[crate::moneywiz_invest::Row],
    today: NaiveDate,
) {
    let recent = today - chrono::Duration::days(365);
    let paired = paired_rows(rows);
    for ac in &mut c.accounts {
        let Some(a) = store.accounts.iter().find(|a| a.name == ac.name) else {
            continue;
        };
        let mine: Vec<NaiveDate> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && !t.is_correction() && paired.contains(&t.pk))
            .map(|t| t.date)
            .collect();
        if mine.is_empty() {
            continue;
        }
        ac.covered += mine.len();
        ac.covered_recent += mine.iter().filter(|d| **d >= recent).count();
        let before = ac.dates_since.len();
        ac.dates_since.retain(|(pk, _)| !paired.contains(pk));
        ac.covered_since += before - ac.dates_since.len();
        ac.source = if ac.source == "-" {
            "broker trades".into()
        } else {
            format!("{} + broker trades", ac.source)
        };
        if ac.covered >= ac.rows {
            ac.gap = None;
        }
    }
    since_gaps(c, today);
}
