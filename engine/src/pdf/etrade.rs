//! E*TRADE stock plan statements (Morgan Stanley at Work): the cash rows of
//! "CASH FLOW ACTIVITY BY DATE" (sales, wires, fees, dividends, tax).
//!
//! A row is a `M/DD` date, an optional settlement date, the activity type,
//! more cells, and the credit or (debit) last. The row text is the activity
//! type only; descriptions and comments are never read. A sale is a cash row
//! ("sold"), not a trade leg the sync skips: plan shares arrive by vest or
//! payroll purchase, never paid from this cash, so the sale's proceeds are
//! the only cash a trade brings (MoneyWiz holds them as a deposit). Safety: each
//! section's rows must add up to its "NET CREDITS/(DEBITS)" line, or the
//! whole file is refused. The money market sweep and share transfers move
//! no cash and are skipped.
//!
//! The statement's "OPENING CASH, BDP, MMFs" and "CLOSING CASH, BDP, MMFs"
//! (cash, bank deposits and money market funds together, so a sweep does
//! not change them) give the account's balance on the period's last day,
//! when the opening plus the rows makes the closing.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

use super::{amounts, line_shape, month_of, resolve};
use crate::export::{AccountKind, Export, LoadOptions, Txn};

pub const FORMAT: &str = "etrade-stockplan-pdf";

const SECTION: &str = "cash flow activity by date";
const NET: &str = "net credits/(debits)";
const OPENING: &str = "opening cash, bdp, mmfs";
const CLOSING: &str = "closing cash, bdp, mmfs";

static PERIOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)for the period\s+([a-z]+)\.?\s+(\d{1,2})(?:,\s*(\d{4}))?\s*[-–]\s*(?:([a-z]+)\.?\s+)?(\d{1,2}),\s*(\d{4})",
    )
    .unwrap()
});
static ACCOUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\d{3})-(\d{6})-(\d{3})\b").unwrap());
static ROW_DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap());

/// True for an E*TRADE stock plan statement's text.
pub fn is_statement(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains(SECTION) && lower.contains(NET)
}

/// The cells of a line, split by two or more spaces.
fn cells(line: &str) -> Vec<&str> {
    line.split("  ")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect()
}

fn period(line: &str) -> Option<(NaiveDate, NaiveDate)> {
    let c = PERIOD.captures(line)?;
    let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u32>().ok());
    // One month ("January 1-31, 2025") names its month once.
    let m1 = month_of(&c[1])?;
    let m2 = c.get(4).map_or(Some(m1), |m| month_of(m.as_str()))?;
    let (d1, d2) = (num(2)?, num(5)?);
    let y2 = num(6)? as i32;
    let y1 = num(3).map_or(if m1 > m2 { y2 - 1 } else { y2 }, |y| y as i32);
    Some((
        NaiveDate::from_ymd_opt(y1, m1, d1)?,
        NaiveDate::from_ymd_opt(y2, m2, d2)?,
    ))
}

/// The cents of a cell that is one whole amount.
fn whole_amount(cell: &str) -> Option<i64> {
    let a = *amounts(cell).first()?;
    (a.start == 0 && a.end == cell.len()).then_some(if a.negative { -a.cents } else { a.cents })
}

#[derive(Debug)]
struct Row {
    line: usize,
    date: NaiveDate,
    cents: i64,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Seen {
    Header,
    Period,
    Row,
    Net,
    Balance,
    Rejected(&'static str),
}

#[derive(Default, Debug)]
struct Read {
    account: Option<String>,
    rows: Vec<Row>,
    bad: Vec<(usize, &'static str)>,
    /// Sections read, and the periods whose rows miss their net.
    sections: usize,
    failed: Vec<String>,
    seen: Vec<(usize, Seen)>,
    /// The statement period, and its opening and closing cash.
    period: Option<(NaiveDate, NaiveDate)>,
    opening: Option<i64>,
    closing: Option<i64>,
}

/// The cash printed after a label: its first amount ("—" is none, so 0).
fn cash_after(lower: &str, label: &str) -> Option<i64> {
    let rest = &lower[lower.find(label)? + label.len()..];
    match amounts(rest).first() {
        Some(a) => Some(if a.negative { -a.cents } else { a.cents }),
        None => rest.trim_start().starts_with('—').then_some(0),
    }
}

fn read(text: &str) -> Read {
    let mut r = Read::default();
    let mut at: Option<(NaiveDate, NaiveDate)> = None;
    // In a cash flow section: the sum of its rows so far.
    let mut open: Option<i64> = None;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let lower = line.to_lowercase();
        if r.account.is_none() {
            if let Some(c) = ACCOUNT.captures(line) {
                r.account = Some(format!("{}{}{}", &c[1], &c[2], &c[3]));
            }
        }
        if let Some(p) = period(line) {
            at = Some(p);
            r.period.get_or_insert(p);
            r.seen.push((n, Seen::Period));
            continue;
        }
        for (label, slot) in [(OPENING, &mut r.opening), (CLOSING, &mut r.closing)] {
            if lower.contains(label) && slot.is_none() {
                *slot = cash_after(&lower, label);
                r.seen.push((n, Seen::Balance));
            }
        }
        if lower.contains("for the period") {
            r.seen.push((n, Seen::Rejected("period")));
            continue;
        }
        if lower.contains(SECTION) {
            open.get_or_insert(0);
            r.seen.push((n, Seen::Header));
            continue;
        }
        let Some(sum) = open else { continue };
        let c = cells(line);
        if lower.trim_start().starts_with(NET) {
            let net = c.last().and_then(|c| whole_amount(c));
            let label = at.map_or("a statement".into(), |(a, b)| format!("{a} to {b}"));
            if net != Some(sum) {
                r.failed.push(label);
            }
            r.sections += 1;
            open = None;
            r.seen.push((n, Seen::Net));
            continue;
        }
        let Some(first) = c.first() else { continue };
        let Some(d) = ROW_DATE.captures(first) else {
            if lower.contains("activity type") || lower.contains("credits/(debits)") {
                r.seen.push((n, Seen::Header));
            }
            continue;
        };
        let Some((start, end)) = at else {
            r.bad.push((n, "no statement period before this row"));
            r.seen.push((n, Seen::Rejected("no period")));
            continue;
        };
        let (month, day) = (d[1].parse().unwrap_or(0), d[2].parse().unwrap_or(0));
        let Some(date) = resolve(month, day, start, end) else {
            r.bad.push((n, "date not parseable"));
            r.seen.push((n, Seen::Rejected("date")));
            continue;
        };
        let kind_at = if c.get(1).is_some_and(|s| ROW_DATE.is_match(s)) {
            2
        } else {
            1
        };
        let (Some(kind), Some(cents)) = (
            c.get(kind_at).filter(|_| c.len() > kind_at + 1),
            c.last().and_then(|l| whole_amount(l)),
        ) else {
            r.bad.push((n, "amount not parseable"));
            r.seen.push((n, Seen::Rejected("amount")));
            continue;
        };
        let text = kind.to_lowercase();
        open = Some(sum + cents);
        r.rows.push(Row {
            line: n,
            date,
            cents,
            text,
        });
        r.seen.push((n, Seen::Row));
    }
    if open.is_some() {
        r.failed
            .push("a section with no NET CREDITS/(DEBITS) line".into());
    }
    r
}

fn account_name(id: Option<&str>) -> String {
    match id {
        Some(id) => format!("E*TRADE Stock Plan …{}", &id[id.len().saturating_sub(4)..]),
        None => "E*TRADE Stock Plan".into(),
    }
}

pub fn parse(text: &str, name: &str, opts: &LoadOptions) -> Result<Export, String> {
    let r = read(text);
    if !r.failed.is_empty() {
        return Err(format!(
            "{FORMAT}: the rows do not add up to the net on {} section(s) ({}), so no row was read. Run bin/cashcast columns and fix the reader",
            r.failed.len(),
            r.failed.join(", ")
        ));
    }
    if r.rows.is_empty() && r.bad.is_empty() {
        return Err(format!(
            "{FORMAT}: no cash flow rows found. Run bin/cashcast columns"
        ));
    }
    let account = opts
        .account
        .clone()
        .unwrap_or_else(|| account_name(r.account.as_deref()));
    let currency = opts.currency.clone().unwrap_or_else(|| "USD".into());
    let mut export = Export::empty(name, FORMAT);
    export.date_format = "%m/%d".into();
    export
        .account_kinds
        .insert(account.clone(), AccountKind::Bank);
    if let Some(id) = &r.account {
        export
            .ledger_ids
            .entry(account.clone())
            .or_default()
            .insert((crate::ledger::NON_REGISTERED.to_string(), id.clone()));
    }
    let mut nth: HashMap<(NaiveDate, i64), usize> = HashMap::new();
    for row in &r.rows {
        let n = nth.entry((row.date, row.cents)).or_insert(0);
        *n += 1;
        let derived = crate::text::derive(&row.text, row.cents);
        let mut h = DefaultHasher::new();
        ("etrade-pdf", &account, row.date, row.cents, *n).hash(&mut h);
        export.txns.push(Txn {
            file: 0,
            account: account.clone(),
            transfer: String::new(),
            category: Vec::new(),
            category_from_file: false,
            hint: derived.hint,
            payee_key: derived.payee_key,
            date: row.date,
            cents: row.cents,
            currency: currency.clone(),
            fingerprint: h.finish(),
            text: row.text.clone(),
        });
    }
    export.bad_lines = r.bad.clone();
    export.notes.push(format!(
        "{} cash flow section(s), each adds up to its net",
        r.sections
    ));
    // The closing cash is the balance on the period's last day, once the
    // opening and the rows lead to it.
    if let (Some((start, end)), Some(opening), Some(closing)) = (r.period, r.opening, r.closing) {
        let sum: i64 = r.rows.iter().map(|row| row.cents).sum();
        if opening + sum == closing {
            export.statements.push(crate::export::Statement {
                account: account.clone(),
                start,
                end,
                opening,
                closing,
            });
            export.balances.insert(
                account.clone(),
                crate::export::Balance {
                    cents: closing,
                    as_of: Some(end),
                    source: "statement balance",
                },
            );
            export.notes.push(format!("closing cash on {end}"));
        } else {
            export
                .notes
                .push("the opening cash and the rows miss the closing cash".into());
        }
    }
    Ok(export)
}

/// Rows before a line, for `first-error`.
pub fn rows_before(text: &str, line: usize) -> usize {
    read(text).rows.iter().filter(|r| r.line < line).count()
}

/// Masked shapes of the period, header, row and net lines.
pub fn columns_text(text: &str, name: &str) -> String {
    let r = read(text);
    let result = match parse(text, name, &LoadOptions::default()) {
        Ok(e) => format!(
            "{} rows, {} unread, {}; account {}",
            e.txns.len(),
            e.bad_lines.len(),
            e.notes.join("; "),
            e.account_kinds.keys().next().map_or("?", String::as_str)
        ),
        Err(e) => format!("refused: {e}"),
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = format!("- {name} ({FORMAT}): {result}\n");
    let mut rows = 0;
    for (n, seen) in r.seen.iter().take(super::SHOWN_LINES) {
        let tag = match seen {
            Seen::Row => {
                rows += 1;
                if rows > super::SHOWN_ROWS {
                    continue;
                }
                "row".to_string()
            }
            Seen::Header => "header".into(),
            Seen::Period => "period".into(),
            Seen::Net => "net".into(),
            Seen::Balance => "cash".into(),
            Seen::Rejected(why) => format!("not read: {why}"),
        };
        let _ = writeln!(out, "  line {n} [{tag}]: {}", line_shape(lines[n - 1]));
    }
    out
}
