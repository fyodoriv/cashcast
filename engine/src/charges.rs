//! Unexpected charges: duplicate subscriptions, new recurring charges, price
//! rises and charges after an apparent cancel, across every account in the
//! inbox. Also `charges find WORD`: every charge series of one merchant.
//!
//! The report names merchants, accounts and amounts, so it is for the owner
//! only: the CLI sends it through the output gate (`agent:strategy`).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use chrono::{Duration, NaiveDate};

use crate::detect::{self, Cadence};
use crate::export::Txn;

/// A series that started this many days before today is new.
pub const NEW_DAYS: i64 = 60;
/// A recurring amount up more than this percent is a price rise.
pub const RISE_PCT: i64 = 10;
/// A charge is in the same amount group within this percent of its median,
/// so two plans on one card stay apart while a price rise stays together.
const GROUP_PCT: i64 = 25;
/// Two copies of one charge (a bank export and MoneyWiz) post within this
/// many days of each other.
const MIRROR_DAYS: i64 = 3;

/// One merchant and the billing names it uses.
#[derive(Debug, Clone)]
pub struct Alias {
    pub name: String,
    pub patterns: Vec<String>,
}

/// Reads `[[merchants]]` from knowledge/data/merchants.toml.
pub fn parse_aliases(text: &str) -> Result<Vec<Alias>, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("merchants.toml: {e}"))?;
    let mut out = Vec::new();
    for m in v
        .get("merchants")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
    {
        let name = m.get("name").and_then(|n| n.as_str()).unwrap_or_default();
        let patterns: Vec<String> = m
            .get("patterns")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(words))
            .filter(|p| !p.is_empty())
            .collect();
        if !name.is_empty() && !patterns.is_empty() {
            out.push(Alias {
                name: name.to_string(),
                patterns,
            });
        }
    }
    Ok(out)
}

/// Lower-case letters and digits, every other run of characters one space.
fn words(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The merchant a row belongs to: an alias name when a pattern matches its
/// words, else its first two words without digits ("NETFLIX.COM #4411" →
/// "netflix com").
pub fn merchant(text: &str, aliases: &[Alias]) -> String {
    let w = format!(" {} ", words(text));
    for a in aliases {
        if a.patterns.iter().any(|p| w.contains(&format!(" {p} "))) {
            return a.name.clone();
        }
    }
    let plain: Vec<&str> = w
        .split_whitespace()
        .filter(|t| t.chars().any(|c| c.is_alphabetic()) && t.chars().all(|c| !c.is_ascii_digit()))
        .take(2)
        .collect();
    plain.join(" ")
}

/// One merchant's charges on one account, at one usual amount.
#[derive(Debug, Clone)]
pub struct Series {
    pub merchant: String,
    pub account: String,
    pub currency: String,
    /// (date, cents spent), oldest first; same-day charges add up.
    pub charges: Vec<(NaiveDate, i64)>,
    /// None when the charges have no schedule.
    pub cadence: Option<Cadence>,
    pub median_cents: i64,
    /// Still charging by the last date its account has data for.
    pub active: bool,
}

impl Series {
    pub fn first(&self) -> NaiveDate {
        self.charges[0].0
    }
    pub fn last(&self) -> NaiveDate {
        self.charges[self.charges.len() - 1].0
    }
}

/// Splits sorted positive amounts into groups within `GROUP_PCT` of the
/// group's first amount.
fn amount_groups(rows: &[(NaiveDate, i64)]) -> Vec<Vec<(NaiveDate, i64)>> {
    let mut sorted: Vec<i64> = rows.iter().map(|r| r.1).collect();
    sorted.sort_unstable();
    let mut bounds: Vec<(i64, i64)> = Vec::new();
    for a in sorted {
        match bounds.last_mut() {
            Some((lo, hi)) if (a - *lo) * 100 <= *lo * GROUP_PCT => *hi = a,
            _ => bounds.push((a, a)),
        }
    }
    bounds
        .iter()
        .map(|(lo, hi)| {
            rows.iter()
                .copied()
                .filter(|r| r.1 >= *lo && r.1 <= *hi)
                .collect()
        })
        .collect()
}

fn by_day(rows: &[(NaiveDate, i64)]) -> Vec<(NaiveDate, i64)> {
    let mut m: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for (d, c) in rows {
        *m.entry(*d).or_insert(0) += c;
    }
    m.into_iter().collect()
}

fn median(rows: &[(NaiveDate, i64)]) -> i64 {
    let mut a: Vec<i64> = rows.iter().map(|r| r.1).collect();
    a.sort_unstable();
    a[a.len() / 2]
}

/// Every charge series in `txns`: outflows that are not transfers, grouped
/// by merchant, account, currency and usual amount. When one amount group
/// has no schedule, the account's whole merchant group is tried as one
/// series, so a price rise over `GROUP_PCT` still reads as one subscription.
pub fn series(txns: &[Txn], aliases: &[Alias]) -> Vec<Series> {
    let mut until: HashMap<&str, NaiveDate> = HashMap::new();
    for t in txns {
        let e = until.entry(t.account.as_str()).or_insert(t.date);
        *e = (*e).max(t.date);
    }
    let mut groups: BTreeMap<(String, String, String), Vec<(NaiveDate, i64)>> = BTreeMap::new();
    for t in txns {
        if t.cents >= 0 || !t.transfer.is_empty() || t.text.trim().is_empty() {
            continue;
        }
        let m = merchant(&t.text, aliases);
        if m.is_empty() {
            continue;
        }
        groups
            .entry((m, t.account.clone(), t.currency.clone()))
            .or_default()
            .push((t.date, -t.cents));
    }
    let mut out = Vec::new();
    for ((m, account, currency), rows) in groups {
        let data_until = until[account.as_str()];
        let make = |rows: Vec<(NaiveDate, i64)>| {
            let rows = by_day(&rows);
            let last = rows[rows.len() - 1].0;
            // Judged against its own last charge, so a stopped series still
            // shows its cadence; `active` says whether it stopped.
            let found = detect::detect_recurring(&rows, last);
            let active = found.is_some() && detect::detect_recurring(&rows, data_until).is_some();
            Series {
                merchant: m.clone(),
                account: account.clone(),
                currency: currency.clone(),
                median_cents: found.map_or_else(|| median(&rows), |r| r.median_cents),
                cadence: found.map(|r| r.cadence),
                active,
                charges: rows,
            }
        };
        let parts: Vec<Series> = amount_groups(&rows).into_iter().map(make).collect();
        let whole = make(rows.clone());
        let scheduled = parts.iter().filter(|s| s.cadence.is_some()).count();
        if parts.len() > 1 && scheduled == 0 && whole.cadence.is_some() {
            out.push(whole);
        } else {
            out.extend(parts);
        }
    }
    out
}

/// True when `b` is a copy of `a` from another file (MoneyWiz and the bank
/// export): almost every charge of the shorter one has a twin of the same
/// amount within `MIRROR_DAYS`.
fn mirrored(a: &Series, b: &Series) -> bool {
    let (short, long) = if a.charges.len() <= b.charges.len() {
        (a, b)
    } else {
        (b, a)
    };
    let twins = short
        .charges
        .iter()
        .filter(|(d, c)| {
            long.charges.iter().any(|(d2, c2)| {
                (*d - *d2).num_days().abs() <= MIRROR_DAYS && (c - c2).abs() * 50 <= *c
            })
        })
        .count();
    twins * 4 >= short.charges.len() * 3
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// One merchant charging on the same cadence from two or more series.
    Duplicate {
        merchant: String,
        series: Vec<usize>,
    },
    /// A recurring series whose first charge is within `NEW_DAYS`.
    New(usize),
    /// The latest charge is up more than `RISE_PCT` on a steady price.
    PriceRise { series: usize, from: i64, to: i64 },
    /// A gap of more than two periods, then charges again.
    Resumed {
        series: usize,
        gap_from: NaiveDate,
        gap_to: NaiveDate,
    },
}

/// The unexpected charges among `all` (from `series`).
pub fn findings(all: &[Series], today: NaiveDate) -> Vec<Finding> {
    let mut out = Vec::new();
    let active: Vec<usize> = (0..all.len())
        .filter(|&i| all[i].active && all[i].cadence.is_some())
        .collect();
    let mut by_merchant: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for &i in &active {
        let s = &all[i];
        by_merchant
            .entry((s.merchant.as_str(), s.cadence.unwrap().name()))
            .or_default()
            .push(i);
    }
    for ((m, _), ids) in by_merchant {
        // Keep one of each mirrored pair: the same charge in two files.
        let mut kept: Vec<usize> = Vec::new();
        for i in ids {
            if !kept.iter().any(|&k| mirrored(&all[k], &all[i])) {
                kept.push(i);
            }
        }
        if kept.len() > 1 {
            out.push(Finding::Duplicate {
                merchant: m.to_string(),
                series: kept,
            });
        }
    }
    for &i in &active {
        let s = &all[i];
        if s.first() >= today - Duration::days(NEW_DAYS) {
            out.push(Finding::New(i));
        }
        // Only a fixed price can rise: the 3 charges before the latest agree
        // within 2%, which leaves out bills that vary (power, phone usage).
        let n = s.charges.len();
        if n >= 4 {
            let to = s.charges[n - 1].1;
            let from = s.charges[n - 2].1;
            let steady = s.charges[n - 4..n - 1]
                .iter()
                .all(|c| (c.1 - from).abs() * 50 <= from);
            if steady && (to - from) * 100 > from * RISE_PCT {
                out.push(Finding::PriceRise {
                    series: i,
                    from,
                    to,
                });
            }
        }
        let (_, hi) = s.cadence.unwrap().gap_days();
        if let Some(w) = s
            .charges
            .windows(2)
            .rev()
            .find(|w| (w[1].0 - w[0].0).num_days() > 2 * hi)
        {
            out.push(Finding::Resumed {
                series: i,
                gap_from: w[0].0,
                gap_to: w[1].0,
            });
        }
    }
    out
}

fn amount(cents: i64, currency: &str) -> String {
    let n = format!("{}.{:02}", cents / 100, cents % 100);
    if currency.is_empty() {
        n
    } else {
        format!("{n} {currency}")
    }
}

fn describe(s: &Series) -> String {
    format!(
        "{} on {}: {} {}, {} charge(s) {} to {}{}",
        s.merchant,
        s.account,
        s.cadence.map_or("irregular", Cadence::name),
        amount(s.median_cents, &s.currency),
        s.charges.len(),
        s.first(),
        s.last(),
        if s.active || s.cadence.is_none() {
            ""
        } else {
            " (stopped)"
        }
    )
}

/// The unexpected-charges report.
pub fn render(all: &[Series], found: &[Finding], today: NaiveDate) -> String {
    let mut o = format!(
        "# Unexpected charges ({today})\n\nFor the owner only: it names merchants, accounts and amounts.\n"
    );
    let recurring = all
        .iter()
        .filter(|s| s.active && s.cadence.is_some())
        .count();
    let _ = writeln!(o, "\nActive recurring series checked: {recurring}.");
    let section = |o: &mut String, title: &str, lines: Vec<String>| {
        let _ = writeln!(o, "\n## {title}\n");
        if lines.is_empty() {
            let _ = writeln!(o, "None found.");
        }
        for l in lines {
            let _ = writeln!(o, "- {l}");
        }
    };
    let mut dup = Vec::new();
    let mut new = Vec::new();
    let mut rise = Vec::new();
    let mut back = Vec::new();
    for f in found {
        match f {
            Finding::Duplicate { merchant, series } => {
                let parts: Vec<String> = series.iter().map(|&i| describe(&all[i])).collect();
                dup.push(format!(
                    "{merchant}: {} series\n  - {}",
                    series.len(),
                    parts.join("\n  - ")
                ));
            }
            Finding::New(i) => new.push(describe(&all[*i])),
            Finding::PriceRise { series, from, to } => {
                let s = &all[*series];
                rise.push(format!(
                    "{}: {} → {} (+{}%)",
                    describe(s),
                    amount(*from, &s.currency),
                    amount(*to, &s.currency),
                    (to - from) * 100 / (*from).max(1)
                ));
            }
            Finding::Resumed {
                series,
                gap_from,
                gap_to,
            } => back.push(format!(
                "{}: no charge from {gap_from} to {gap_to}, then charged again",
                describe(&all[*series])
            )),
        }
    }
    section(&mut o, "Duplicate subscriptions", dup);
    section(
        &mut o,
        &format!("New recurring charges (last {NEW_DAYS} days)"),
        new,
    );
    section(&mut o, &format!("Price rises over {RISE_PCT}%"), rise);
    section(&mut o, "Charged again after a pause", back);
    o
}

/// `charges find WORD`: every series whose merchant or alias name holds
/// WORD, grouped per account.
pub fn render_find(all: &[Series], query: &str) -> String {
    let q = words(query);
    let hits: Vec<&Series> = all
        .iter()
        .filter(|s| !q.is_empty() && words(&s.merchant).contains(&q))
        .collect();
    let mut o = format!(
        "# Charges matching \"{query}\"\n\nFor the owner only: it names merchants, accounts and amounts.\n"
    );
    if hits.is_empty() {
        let _ = writeln!(o, "\nNo charges found.");
        return o;
    }
    let mut accounts: BTreeMap<&str, Vec<&Series>> = BTreeMap::new();
    for s in hits {
        accounts.entry(s.account.as_str()).or_default().push(s);
    }
    let active = accounts
        .values()
        .flatten()
        .filter(|s| s.active && s.cadence.is_some())
        .count();
    let _ = writeln!(
        o,
        "\n{} account(s), {active} active recurring series.",
        accounts.len()
    );
    for (account, list) in accounts {
        let _ = writeln!(o, "\n## {account}\n");
        for s in list {
            let state = match (s.cadence, s.active) {
                (Some(_), true) => "active",
                (Some(_), false) => "stopped",
                _ => "one-off",
            };
            let _ = writeln!(
                o,
                "- {} ({state}): {} {}, {} charge(s) {} to {}, last {}",
                s.merchant,
                s.cadence.map_or("irregular", Cadence::name),
                amount(s.median_cents, &s.currency),
                s.charges.len(),
                s.first(),
                s.last(),
                amount(s.charges[s.charges.len() - 1].1, &s.currency)
            );
        }
    }
    o
}
