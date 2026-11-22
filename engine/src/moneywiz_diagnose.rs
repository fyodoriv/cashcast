//! `moneywiz diagnose`: why the sync pairs rows the way it does, in counts
//! only. It never prints a date, payee or amount of a single row, so agents
//! may read it (privacy rule 1).

use crate::inbox::Inbox;
use crate::money::fmt_cents;
use crate::moneywiz::{Plan, Store, corrected};
use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.0} %", n as f64 * 100.0 / d as f64)
    }
}

/// Bank rows in this account that another export file also holds (same
/// date, amount and text): overlapping files that the merge did not fold.
fn duplicates(inbox: &Inbox, bank: &[usize]) -> usize {
    let mut per_file: HashMap<(chrono::NaiveDate, i64, &str), HashMap<usize, usize>> =
        HashMap::new();
    for &i in bank {
        let t = &inbox.txns[i];
        *per_file
            .entry((t.date, t.cents, t.text.as_str()))
            .or_default()
            .entry(t.file)
            .or_default() += 1;
    }
    per_file
        .values()
        .map(|files| files.values().sum::<usize>() - files.values().max().copied().unwrap_or(0))
        .sum()
}

pub fn render(store: &Store, inbox: &Inbox, plan: &Plan) -> String {
    let amounts = corrected(store);
    let mut out = String::from(
        "# MoneyWiz diagnose\n\nCounts only. Fix pairs: a bank row paired with a MoneyWiz row of another amount.\n\n",
    );
    out.push_str("| Account | Bank rows | Dup bank rows | Fix pairs | ≤ 1¢ | ≤ $1 | ≤ 5 % | > 5 % | Sign flip | Same day | Payee alike | Only in MW | Only-MW: same amount in export | Only-MW: sum of 2 same-day bank rows | Bank rows with a sign-flipped twin ≤ 3 days |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    let mut adds = String::from(
        "\n## Rows to add: where MoneyWiz may hold them\n\n| Account | To add | Same amount, ≤ 60 days off | Same amount, other account, ≤ 4 days | A bigger same-sign row ≤ 7 days (a sum?) | Nothing alike | Only-MW rows = a run of rows to add |\n|---|---:|---:|---:|---:|---:|---|\n",
    );
    for ap in &plan.accounts {
        let Some(a) = store.account(ap.mw) else {
            continue;
        };
        let bank: Vec<usize> = (0..inbox.txns.len())
            .filter(|&i| inbox.txns[i].account == ap.export_account)
            .filter(|&i| ap.from.is_some_and(|f| inbox.txns[i].date >= f))
            .collect();
        let dup = duplicates(inbox, &bank);
        let (mut cent, mut dollar, mut small, mut big, mut flip, mut sameday, mut alike) =
            (0, 0, 0, 0, 0, 0, 0);
        for &(pk, i) in &ap.fix {
            let b = &inbox.txns[i];
            let m = store.txns.iter().find(|t| t.pk == pk).expect("row exists");
            let amount = amounts[&pk];
            let diff = (amount - b.cents).abs();
            if diff <= 1 {
                cent += 1;
            } else if diff <= 100 {
                dollar += 1;
            } else if diff * 20 <= b.cents.abs() {
                small += 1;
            } else {
                big += 1;
            }
            if amount == -b.cents {
                flip += 1;
            }
            if m.date == b.date {
                sameday += 1;
            }
            let wa: std::collections::HashSet<String> = m
                .text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 3)
                .map(str::to_uppercase)
                .collect();
            if b.text
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w.len() >= 3 && wa.contains(&w.to_uppercase()))
            {
                alike += 1;
            }
        }
        let mut same_amount = 0;
        let mut pair_sum = 0;
        for &pk in &ap.only_in_moneywiz {
            let m = store.txns.iter().find(|t| t.pk == pk).expect("row exists");
            let amount = amounts[&pk];
            if bank.iter().any(|&i| inbox.txns[i].cents == amount) {
                same_amount += 1;
                continue;
            }
            let day: Vec<i64> = bank
                .iter()
                .map(|&i| &inbox.txns[i])
                .filter(|t| (t.date - m.date).num_days().abs() <= 1)
                .map(|t| t.cents)
                .collect();
            if day
                .iter()
                .enumerate()
                .any(|(x, a)| day[x + 1..].iter().any(|b| a + b == amount))
            {
                pair_sum += 1;
            }
        }
        // Where the rows to add might already be in MoneyWiz.
        let (mut later, mut other_account, mut inside_sum, mut nowhere) = (0, 0, 0, 0);
        let mw_here: Vec<&crate::moneywiz::MwTxn> = store
            .txns
            .iter()
            .filter(|t| t.account == ap.mw && t.is_cash())
            .collect();
        for &i in &ap.add {
            let b = &inbox.txns[i];
            let gap = |t: &crate::moneywiz::MwTxn| (t.date - b.date).num_days().abs();
            if mw_here
                .iter()
                .any(|t| amounts[&t.pk] == b.cents && gap(t) <= 60)
            {
                later += 1;
            } else if store.txns.iter().any(|t| {
                t.account != ap.mw && t.is_cash() && amounts[&t.pk] == b.cents && gap(t) <= 4
            }) {
                other_account += 1;
            } else if mw_here.iter().any(|t| {
                let a = amounts[&t.pk];
                gap(t) <= 7 && (a < 0) == (b.cents < 0) && a.abs() > b.cents.abs()
            }) {
                inside_sum += 1;
            } else {
                nowhere += 1;
            }
        }
        // Only-in-MoneyWiz rows that equal a run of consecutive rows to add
        // (by date, same sign, within 31 days of the MoneyWiz row).
        let mut sorted: Vec<usize> = ap.add.clone();
        sorted.sort_by_key(|&i| inbox.txns[i].date);
        let mut used = vec![false; sorted.len()];
        let (mut runs, mut absorbed, mut near) = (0, 0, 0);
        for &pk in &ap.only_in_moneywiz {
            let m = store.txns.iter().find(|t| t.pk == pk).expect("row exists");
            let want = amounts[&pk];
            let ok = |i: usize| {
                let t = &inbox.txns[i];
                (t.cents < 0) == (want < 0) && (t.date - m.date).num_days().abs() <= 31
            };
            let mut found: Option<(usize, usize, bool)> = None;
            'outer: for x in 0..sorted.len() {
                let mut sum = 0;
                for y in x..sorted.len() {
                    if used[y] || !ok(sorted[y]) {
                        break;
                    }
                    sum += inbox.txns[sorted[y]].cents;
                    if y > x && sum == want {
                        found = Some((x, y, true));
                        break 'outer;
                    }
                    if y > x && found.is_none() && (sum - want).abs() * 100 <= want.abs() {
                        found = Some((x, y, false));
                    }
                }
            }
            if let Some((x, y, exact)) = found {
                for u in used.iter_mut().take(y + 1).skip(x) {
                    *u = true;
                }
                runs += exact as usize;
                near += !exact as usize;
                absorbed += y - x + 1;
            }
        }
        adds.push_str(&format!(
            "| {} | {} | {later} | {other_account} | {inside_sum} | {nowhere} | {runs} exact, {near} within 1 % ({absorbed} rows) |\n",
            a.name,
            ap.add.len()
        ));
        let flipped = bank
            .iter()
            .filter(|&&i| {
                let b = &inbox.txns[i];
                mw_here
                    .iter()
                    .any(|t| amounts[&t.pk] == -b.cents && (t.date - b.date).num_days().abs() <= 3)
            })
            .count();
        let n = ap.fix.len();
        let only = ap.only_in_moneywiz.len();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            a.name,
            bank.len(),
            dup,
            n,
            cent,
            dollar,
            small,
            big,
            flip,
            pct(sameday, n),
            pct(alike, n),
            only,
            same_amount,
            pair_sum,
            flipped
        ));
    }
    out.push_str(&adds);
    out.push_str("\n## MoneyWiz rows the matcher skips (not cash entities), in each window\n\n| Account | Entity | Rows | Sum |\n|---|---|---:|---:|\n");
    for ap in &plan.accounts {
        let (Some(a), Some(from), Some(to)) = (store.account(ap.mw), ap.from, ap.to) else {
            continue;
        };
        let mut by: std::collections::BTreeMap<&str, (usize, i64)> = Default::default();
        for t in store
            .txns
            .iter()
            .filter(|t| t.account == ap.mw && t.date >= from && t.date <= to)
        {
            if !t.is_cash() || t.is_correction() {
                let e = by
                    .entry(if t.is_correction() {
                        "cashcast correction"
                    } else {
                        t.entity.as_str()
                    })
                    .or_default();
                e.0 += 1;
                e.1 += t.cents;
            }
        }
        for (entity, (n, sum)) in by {
            // A sum of one or two rows would show a single row's amount.
            let sum = if n >= 3 {
                crate::money::fmt_cents(sum)
            } else {
                "-".into()
            };
            out.push_str(&format!("| {} | {entity} | {n} | {sum} |\n", a.name));
        }
    }
    let mut kept: std::collections::BTreeMap<(String, &str), usize> = Default::default();
    for (account, why) in crate::moneywiz_edit::kept_corrections(store, plan) {
        let name = store
            .account(account)
            .map_or_else(String::new, |a| a.name.clone());
        *kept.entry((name, why)).or_default() += 1;
    }
    if !kept.is_empty() {
        out.push_str("\n## Corrections `--fixes` keeps, and why\n\n| Account | Why | Rows |\n|---|---|---:|\n");
        for ((name, why), n) in kept {
            out.push_str(&format!("| {name} | {why} | {n} |\n"));
        }
    }
    out.push_str(&history_table(store, inbox, plan));
    out.push_str(&chain_table(inbox, plan));
    out.push_str(&ledger_kinds_table(inbox, plan));
    out.push_str(&sides_table(store, inbox, plan));
    out
}

/// Each investment account's period, both sides summed by kind: where a
/// gap the matcher does not see lives (buys priced apart from the trade
/// legs, rows before the period).
fn sides_table(store: &Store, inbox: &Inbox, plan: &Plan) -> String {
    let mut out = String::new();
    for ap in plan.accounts.iter().filter(|ap| ap.note.is_none()) {
        let (Some(a), Some(from), Some(to)) = (store.account(ap.mw), ap.from, ap.to) else {
            continue;
        };
        if a.kind != "InvestmentAccount" {
            continue;
        }
        let bank = |trade: bool| -> i64 {
            inbox
                .txns
                .iter()
                .filter(|t| t.account == ap.export_account && t.date >= from && t.date <= to)
                .filter(|t| crate::moneywiz::from_bank(inbox, t))
                .filter(|t| (t.text == "trade" || t.text.starts_with("trade ")) == trade)
                .map(|t| t.cents)
                .sum()
        };
        let mw = |cash: bool| -> i64 {
            store
                .txns
                .iter()
                .filter(|t| t.account == ap.mw && t.date >= from && t.date <= to)
                .filter(|t| t.is_cash() == cash)
                .map(|t| t.cents)
                .sum()
        };
        let before = store.balance_on(ap.mw, from - chrono::Days::new(1));
        if out.is_empty() {
            out.push_str("\n## Investment accounts, both sides summed\n\nIn the bank period: the bank's trade legs and other rows, MoneyWiz's buy and sell rows and other rows, and MoneyWiz's balance the day before.\n\n| Account | Bank trade legs | Bank other | MoneyWiz buys and sells | MoneyWiz other | MoneyWiz before |\n|---|---:|---:|---:|---:|---:|\n");
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            a.name,
            fmt_cents(bank(true)),
            fmt_cents(bank(false)),
            fmt_cents(mw(false)),
            fmt_cents(mw(true)),
            fmt_cents(before)
        ));
    }
    out
}

/// Broker ledgers' rows to add, by activity type: their text is the type
/// only (never a payee), so it can be printed.
fn ledger_kinds_table(inbox: &Inbox, plan: &Plan) -> String {
    let mut by: std::collections::BTreeMap<(String, String), (usize, i64)> = Default::default();
    for ap in &plan.accounts {
        for &i in &ap.add {
            let t = &inbox.txns[i];
            let ledger = inbox
                .files
                .get(t.file)
                .is_some_and(|f| crate::ledger::CASH_LEDGERS.contains(&f.format.as_str()));
            if ledger {
                let e = by
                    .entry((ap.export_account.clone(), t.text.clone()))
                    .or_default();
                e.0 += 1;
                e.1 += t.cents;
            }
        }
    }
    if by.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n## Broker rows to add, by activity type\n\n| Export account | Activity type | Rows | Sum |\n|---|---|---:|---:|\n",
    );
    for ((account, kind), (n, sum)) in by {
        out.push_str(&format!(
            "| {account} | {kind} | {n} | {} |\n",
            fmt_cents(sum)
        ));
    }
    out
}

/// One account's PDF statements: do they chain, and do the bank rows lead
/// from the bank's balance back to each closing?
pub struct Chain {
    pub account: String,
    pub statements: usize,
    pub first: NaiveDate,
    pub last: NaiveDate,
    /// (last day of one statement, first day of the next, next opening
    /// minus last closing): a statement is missing, or a balance differs.
    pub breaks: Vec<(NaiveDate, NaiveDate, i64)>,
    /// Closings the bank rows do not lead to, of those checked, and the
    /// first one: (day, closing minus the balance the rows give).
    pub off: usize,
    pub checked: usize,
    pub first_off: Option<(NaiveDate, i64)>,
}

/// An export account's PDF statements, by period, each once.
pub fn statements<'a>(inbox: &'a Inbox, account: &str) -> Vec<&'a crate::export::Statement> {
    let mut sts: Vec<&crate::export::Statement> = inbox
        .files
        .iter()
        .flat_map(|f| f.statements.iter())
        .filter(|s| s.account == account)
        .collect();
    sts.sort_by_key(|s| (s.start, s.end));
    sts.dedup_by(|a, b| a == b);
    sts
}

/// Where the statements do not chain: (last day of one, first day of the
/// next, next opening minus last closing). Days between two statements
/// whose balance carries over (a weekend) are no break.
pub fn breaks(sts: &[&crate::export::Statement]) -> Vec<(NaiveDate, NaiveDate, i64)> {
    sts.windows(2)
        .filter(|w| w[1].opening != w[0].closing)
        .map(|w| (w[0].end, w[1].start, w[1].opening - w[0].closing))
        .collect()
}

/// The breaks MoneyWiz does not account for. When the bank never issued a
/// month, the owner's own rows may stand in for it: from the day before the
/// statement ahead of the break to the end of the one after it, MoneyWiz
/// then moves as the statements do, within max(100.00, 5 % of the closing).
/// `balance_on` is MoneyWiz's balance at the end of a day.
pub fn unexplained_breaks(
    sts: &[&crate::export::Statement],
    balance_on: impl Fn(NaiveDate) -> i64,
) -> Vec<(NaiveDate, NaiveDate, i64)> {
    break_spans(sts, balance_on)
        .into_iter()
        .filter(|&(_, explained)| !explained)
        .map(|(w, _)| (w[0].end, w[1].start, w[1].opening - w[0].closing))
        .collect()
}

/// The days MoneyWiz's own rows stand in for a month the bank never issued:
/// the statement before each break it accounts for, through the one after.
/// The sync leaves both sides' rows alone there, since a bank row near the
/// gap would pair with a row typed for the missing month.
pub fn stand_in_spans(
    sts: &[&crate::export::Statement],
    balance_on: impl Fn(NaiveDate) -> i64,
) -> Vec<(NaiveDate, NaiveDate)> {
    break_spans(sts, balance_on)
        .into_iter()
        .filter(|&(_, explained)| explained)
        .map(|(w, _)| (w[0].start, w[1].end))
        .collect()
}

/// A span MoneyWiz stands in for: the statement before a break through the
/// one after, the bank's own change across it, and the correction (on the
/// last missing day) that makes MoneyWiz move by as much.
pub struct StandIn {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub bank: i64,
    pub fix: (NaiveDate, i64),
}

pub fn stand_ins(
    sts: &[&crate::export::Statement],
    balance_on: impl Fn(NaiveDate) -> i64,
) -> Vec<StandIn> {
    break_spans(sts, &balance_on)
        .into_iter()
        .filter(|&(_, explained)| explained)
        .map(|(w, _)| {
            let bank = w[1].closing - w[0].opening;
            let moneywiz = balance_on(w[1].end) - balance_on(w[0].start - chrono::Days::new(1));
            let day = (w[1].start - chrono::Days::new(1)).max(w[0].end);
            StandIn {
                start: w[0].start,
                end: w[1].end,
                bank,
                fix: (day, bank - moneywiz),
            }
        })
        .collect()
}

/// An account the bank gives statements for and never a row (a Scotia Smart
/// Investor plan): the balance MoneyWiz must hold the day before each
/// statement starts (its opening) and on its last day (its closing), from
/// `since` on, in date order.
pub fn statement_targets(
    sts: &[&crate::export::Statement],
    since: Option<NaiveDate>,
) -> Vec<(NaiveDate, i64)> {
    let mut out: Vec<(NaiveDate, i64)> = Vec::new();
    for s in sts {
        for (day, cents) in [
            (s.start - chrono::Days::new(1), s.opening),
            (s.end, s.closing),
        ] {
            if since.is_some_and(|f| day < f) {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.0 == day => last.1 = cents,
                _ => out.push((day, cents)),
            }
        }
    }
    out
}

/// The corrections, one per statement day MoneyWiz is off, that make its
/// balance equal each of `statement_targets`.
pub fn statement_fixes(
    targets: &[(NaiveDate, i64)],
    balance_on: impl Fn(NaiveDate) -> i64,
) -> Vec<(NaiveDate, i64)> {
    let mut added = 0;
    let mut out = Vec::new();
    for &(day, cents) in targets {
        let need = cents - (balance_on(day) + added);
        if need != 0 {
            out.push((day, need));
            added += need;
        }
    }
    out
}

/// Each break's two statements, and whether MoneyWiz accounts for it.
fn break_spans<'a>(
    sts: &[&'a crate::export::Statement],
    balance_on: impl Fn(NaiveDate) -> i64,
) -> Vec<([&'a crate::export::Statement; 2], bool)> {
    sts.windows(2)
        .filter(|w| w[1].opening != w[0].closing)
        .map(|w| {
            let bank = w[1].closing - w[0].opening;
            let moneywiz = balance_on(w[1].end) - balance_on(w[0].start - chrono::Days::new(1));
            let room = (w[1].closing.abs() / 20).max(10_000);
            ([w[0], w[1]], (moneywiz - bank).abs() <= room)
        })
        .collect()
}

/// The statement days a break leaves out, in words: "2025-01-01 to
/// 2025-01-30", or the two statements' days when no day is between them.
pub fn missing_days((end, start, _): (NaiveDate, NaiveDate, i64)) -> String {
    let (a, b) = (end + chrono::Days::new(1), start - chrono::Days::new(1));
    if a <= b {
        format!("{a} to {b}")
    } else {
        format!("between the statements ending {end} and starting {start}")
    }
}

pub fn chain(inbox: &Inbox, plan: &Plan) -> Vec<Chain> {
    let mut out = Vec::new();
    for ap in &plan.accounts {
        let sts = statements(inbox, &ap.export_account);
        let (Some(first), Some(last)) = (sts.first(), sts.last()) else {
            continue;
        };
        let breaks = breaks(&sts);
        let (mut off, mut checked, mut first_off) = (0, 0, None);
        // With no bank row at all (a statement-only plan), no row leads
        // back to a closing: "Balance over time" checks MoneyWiz instead.
        if let Some(bank) = ap
            .bank_balance
            .as_ref()
            .filter(|_| ap.statement_days.is_empty())
        {
            let end = bank.as_of.or(ap.to);
            let rows: Vec<&crate::export::Txn> = inbox
                .txns
                .iter()
                .filter(|t| t.account == ap.export_account)
                .filter(|t| crate::moneywiz::from_bank(inbox, t))
                .collect();
            // The balance at the end of a day: the bank's, plus the rows up
            // to that day, less the rows up to the bank balance's day.
            let upto = |d: Option<NaiveDate>| -> i64 {
                rows.iter()
                    .filter(|t| d.is_none_or(|d| t.date <= d))
                    .map(|t| t.cents)
                    .sum()
            };
            let at_end = upto(end);
            for s in &sts {
                let diff = s.closing - (bank.cents + upto(Some(s.end)) - at_end);
                checked += 1;
                if diff != 0 {
                    off += 1;
                    first_off.get_or_insert((s.end, diff));
                }
            }
        }
        out.push(Chain {
            account: ap.export_account.clone(),
            statements: sts.len(),
            first: first.start,
            last: last.end,
            breaks,
            off,
            checked,
            first_off,
        });
    }
    out
}

fn chain_table(inbox: &Inbox, plan: &Plan) -> String {
    let rows = chain(inbox, plan);
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n## Statement chain\n\nPDF statements: each closing balance should be the next opening, with no days between, and the bank rows should lead from the bank's balance back to each closing.\n\n| Export account | Statements | First | Last | Breaks | Closings off the rows | First off | Off by |\n|---|---:|---|---|---:|---:|---|---:|\n",
    );
    for c in &rows {
        let (day, by) = c.first_off.map_or(("-".into(), "-".into()), |(d, v)| {
            (d.to_string(), fmt_cents(v))
        });
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} of {} | {day} | {by} |\n",
            c.account,
            c.statements,
            c.first,
            c.last,
            c.breaks.len(),
            c.off,
            c.checked
        ));
    }
    for c in rows.iter().filter(|c| !c.breaks.is_empty()) {
        let list: Vec<String> = c
            .breaks
            .iter()
            .map(|(a, b, d)| format!("{a} → {b} (opening minus closing {})", fmt_cents(*d)))
            .collect();
        out.push_str(&format!(
            "\nBreaks in {}: {}.\n",
            c.account,
            list.join("; ")
        ));
    }
    out
}

/// One account's balance over the bank period: MoneyWiz against the bank.
pub struct History {
    pub account: String,
    /// The day MoneyWiz is furthest from the bank, and by how much.
    pub worst: Option<(NaiveDate, i64)>,
    /// MoneyWiz's lowest balance in the period, and its day.
    pub low: Option<(NaiveDate, i64)>,
    pub is_card: bool,
    /// Bank days on which MoneyWiz is within $1 of the bank, of all days.
    pub close_days: (usize, usize),
    /// The latest bank day on which MoneyWiz is more than $1 off.
    pub last_off: Option<NaiveDate>,
    /// The biggest day-to-day moves of the gap (over $100): where MoneyWiz
    /// and the bank part ways, and where they meet again.
    pub steps: Vec<(NaiveDate, i64)>,
    /// The gap on the last bank day of each month.
    pub monthly: Vec<(NaiveDate, i64)>,
}

/// MoneyWiz's balance at the end of each bank day against the bank's
/// (its balance less the bank rows after that day). Accounts with no bank
/// balance up to the period's end are left out.
pub fn history(store: &Store, inbox: &Inbox, plan: &Plan) -> Vec<History> {
    let mut out = Vec::new();
    for ap in &plan.accounts {
        // Statements and no rows: MoneyWiz against each statement day.
        if let (Some(a), false) = (store.account(ap.mw), ap.statement_days.is_empty()) {
            let gaps: Vec<(NaiveDate, i64)> = ap
                .statement_days
                .iter()
                .map(|&(d, cents)| (d, store.balance_on(a.pk, d) - cents))
                .collect();
            let off: Vec<&(NaiveDate, i64)> = gaps.iter().filter(|g| g.1.abs() > 100).collect();
            let mut monthly: Vec<(NaiveDate, i64)> = Vec::new();
            for &(d, gap) in &gaps {
                match monthly.last_mut() {
                    Some(m) if (m.0.year(), m.0.month()) == (d.year(), d.month()) => *m = (d, gap),
                    _ => monthly.push((d, gap)),
                }
            }
            out.push(History {
                account: a.name.clone(),
                worst: gaps.iter().copied().max_by_key(|g| g.1.abs()),
                low: ap
                    .statement_days
                    .iter()
                    .map(|&(d, _)| (d, store.balance_on(a.pk, d)))
                    .min_by_key(|&(_, c)| c),
                is_card: a.is_card(),
                close_days: (gaps.len() - off.len(), gaps.len()),
                last_off: off.last().map(|g| g.0),
                steps: Vec::new(),
                monthly,
            });
            continue;
        }
        let (Some(a), Some(from), Some(to), Some(bank)) = (
            store.account(ap.mw),
            ap.from,
            ap.to,
            ap.bank_balance.as_ref(),
        ) else {
            continue;
        };
        // A balance dated before the last row still compares up to its day.
        if ap.note.is_some() || bank.as_of.is_some_and(|d| d < from) {
            continue;
        }
        let end = bank.as_of.unwrap_or(to);
        // Around a month the bank never issued, MoneyWiz's rows stand in
        // (the sync leaves them alone): only the statements' change across
        // the span counts, and its days are left out.
        let spans = stand_ins(&statements(inbox, &ap.export_account), |d| {
            store.balance_on(a.pk, d)
        });
        let inside = |d: NaiveDate| spans.iter().any(|s| d >= s.start && d <= s.end);
        let all_rows: Vec<&crate::export::Txn> = inbox
            .txns
            .iter()
            .filter(|t| t.account == ap.export_account && t.date <= end)
            .filter(|t| crate::moneywiz::from_bank(inbox, t))
            .collect();
        let rows: Vec<&crate::export::Txn> =
            all_rows.into_iter().filter(|t| !inside(t.date)).collect();
        let mut days: Vec<NaiveDate> = rows
            .iter()
            .map(|t| t.date)
            .filter(|d| *d >= from)
            .chain(std::iter::once(end).filter(|d| !inside(*d)))
            .collect();
        days.sort();
        days.dedup();
        let mut worst: Option<(NaiveDate, i64)> = None;
        let mut close = 0;
        let mut last_off = None;
        let mut steps = Vec::new();
        let mut prev: Option<i64> = None;
        let mut monthly: Vec<(NaiveDate, i64)> = Vec::new();
        // Rows the matcher skips (fund buys, not cash) are in the bank's
        // balance but never in its export, so they count on its side too.
        // A buy the export lists itself is already in `rows`: each export
        // row stands for one MoneyWiz buy of its day and amount.
        let mut listed: Vec<bool> = vec![false; rows.len()];
        let skipped: Vec<_> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && !t.is_cash())
            // Only the fund buy a same-day export row paid for (the export
            // lists the contribution, not the buy).
            .filter(|t| rows.iter().any(|b| b.date == t.date && b.cents == -t.cents))
            .filter(|t| {
                let own = rows
                    .iter()
                    .enumerate()
                    .position(|(i, b)| !listed[i] && b.date == t.date && b.cents == t.cents);
                own.inspect(|&i| listed[i] = true).is_none()
            })
            .collect();
        for &d in &days {
            let later: i64 = rows
                .iter()
                .filter(|t| t.date > d)
                .map(|t| t.cents)
                .sum::<i64>()
                + skipped
                    .iter()
                    .filter(|t| t.date > d)
                    .map(|t| t.cents)
                    .sum::<i64>()
                + spans
                    .iter()
                    .filter(|s| s.start > d)
                    .map(|s| s.bank)
                    .sum::<i64>();
            let gap = store.balance_on(a.pk, d) - (bank.cents - later);
            if let Some(p) = prev.filter(|p| (gap - p).abs() > 10_000) {
                steps.push((d, gap - p));
            }
            prev = Some(gap);
            match monthly.last_mut() {
                Some(m) if (m.0.year(), m.0.month()) == (d.year(), d.month()) => *m = (d, gap),
                _ => monthly.push((d, gap)),
            }
            close += usize::from(gap.abs() <= 100);
            if gap.abs() > 100 {
                last_off = Some(d);
            }
            if worst.is_none_or(|(_, w)| gap.abs() > w.abs()) {
                worst = Some((d, gap));
            }
        }
        let mut mw_days: Vec<NaiveDate> = store
            .txns
            .iter()
            .filter(|t| t.account == a.pk && t.date >= from && t.date <= end)
            .map(|t| t.date)
            .collect();
        mw_days.sort();
        mw_days.dedup();
        let low = mw_days
            .iter()
            .map(|&d| (d, store.balance_on(a.pk, d)))
            .min_by_key(|&(_, c)| c);
        out.push(History {
            account: a.name.clone(),
            worst,
            low,
            is_card: a.is_card(),
            close_days: (close, days.len()),
            last_off,
            steps: {
                steps.sort_by_key(|s| std::cmp::Reverse(s.1.abs()));
                steps.truncate(5);
                steps.sort();
                steps
            },
            monthly,
        });
    }
    out
}

fn history_table(store: &Store, inbox: &Inbox, plan: &Plan) -> String {
    let rows = history(store, inbox, plan);
    let mut out = String::from(
        "\n## Balance over time\n\nMoneyWiz now, at the end of each bank day, against the bank that day.\n\n| Account | Days within $1 | Last day off | Worst day | MoneyWiz minus bank | Lowest MoneyWiz balance | On |\n|---|---:|---|---|---:|---:|---|\n",
    );
    if rows.is_empty() {
        out.push_str("| (none) | | | | | | |\n");
    }
    for h in &rows {
        let (wd, wg) = h.worst.map_or((String::new(), String::new()), |(d, g)| {
            (d.to_string(), fmt_cents(g))
        });
        let (ld, lc) = h.low.map_or((String::new(), String::new()), |(d, c)| {
            let flag = if !h.is_card && c < 0 {
                " (below zero)"
            } else {
                ""
            };
            (d.to_string(), format!("{}{flag}", fmt_cents(c)))
        });
        let (c, n) = h.close_days;
        out.push_str(&format!(
            "| {} | {c} of {n} | {} | {wd} | {wg} | {lc} | {ld} |\n",
            h.account,
            h.last_off.map_or("none".to_string(), |d| d.to_string())
        ));
    }
    let moved: Vec<&History> = rows.iter().filter(|h| !h.steps.is_empty()).collect();
    if !moved.is_empty() {
        out.push_str("\n### Gap steps\n\nDays the gap to the bank moved by more than $100 (the five biggest per account).\n\n| Account | Day | Gap moved by |\n|---|---|---:|\n");
        for h in moved {
            for (d, c) in &h.steps {
                out.push_str(&format!("| {} | {d} | {} |\n", h.account, fmt_cents(*c)));
            }
        }
    }
    let off: Vec<&History> = rows
        .iter()
        .filter(|h| h.close_days.0 < h.close_days.1)
        .collect();
    if !off.is_empty() {
        out.push_str("\n### Gap by month\n\nMoneyWiz minus the bank on the last bank day of each month, for accounts off on some day.\n\n");
        for h in off {
            let list: Vec<String> = h
                .monthly
                .iter()
                .map(|(d, g)| format!("{}: {}", d.format("%Y-%m"), fmt_cents(*g)))
                .collect();
            out.push_str(&format!("- {}: {}\n", h.account, list.join(", ")));
        }
    }
    out
}
