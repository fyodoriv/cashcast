//! Scotia Smart Investor statements (quarterly PDFs of a mutual fund or
//! GIC plan such as a TFSA): the period and the account's market value at
//! its start and end. Only the year-end statement holds that summary; the
//! other quarters show the account value alone and are read as no
//! statement, so the chequing reader does not take them. The statement lists trades and interest the owner
//! does not keep in MoneyWiz, so no row is read: the closing market value
//! is the account's balance on the period's last day, and coverage counts
//! MoneyWiz's rows to that day when its balance is the same
//! (`moneywiz_coverage`, "statement cash").
//!
//! Safety: opening market value plus deposits, less withdrawals, plus the
//! change in market value must give the closing market value, or the file
//! is refused.

use std::fmt::Write as _;

use chrono::NaiveDate;

use super::{full_dates, line_shape};
use crate::export::{AccountKind, Export, LoadOptions};

pub const FORMAT: &str = "scotiabank-ssi-pdf";

const OPENING: &str = "opening market value";
const CLOSING: &str = "closing market value";
const CHANGE: &str = "change in market value";
const DEPOSITS: &str = "deposits";
const WITHDRAWALS: &str = "withdrawals";
const DETAILS: &str = "details of your transactions";
const ACCOUNT_VALUE: &str = "account value";
/// The holdings table's heading, also in a quarter with no transactions.
const HOLDINGS: &str = "type of investment";
/// A statement covers a quarter; a longer span is a term (a GIC's).
const MAX_PERIOD_DAYS: i64 = 100;

/// True for a year-end statement: it has the market value summary.
fn has_summary(lower: &str) -> bool {
    lower.contains(OPENING) && lower.contains(CLOSING) && lower.contains(CHANGE)
}

/// True for a Smart Investor statement's text.
pub fn is_statement(text: &str) -> bool {
    let lower = text.to_lowercase();
    has_summary(&lower)
        || (lower.contains(ACCOUNT_VALUE) && (lower.contains(DETAILS) || lower.contains(HOLDINGS)))
}

/// What a statement says about its account.
#[derive(Debug, Default)]
pub struct Summary {
    /// The plan and the account number's last four digits ("TFSA …1234").
    pub account: Option<String>,
    pub period: Option<(NaiveDate, NaiveDate)>,
    pub opening: Option<i64>,
    pub deposits: i64,
    pub withdrawals: i64,
    pub change: Option<i64>,
    pub closing: Option<i64>,
}

/// The cells of a line, split by two or more spaces.
fn cells(line: &str) -> Vec<&str> {
    line.split("  ")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect()
}

/// Cents of "1,234.56", "$1,234.56", "1,234.56-" or "(1,234.56)".
fn cents(cell: &str) -> Option<i64> {
    let t = cell.trim();
    if !t.chars().any(|c| c.is_ascii_digit())
        || !t
            .chars()
            .all(|c| c.is_ascii_digit() || ".,$()- ".contains(c))
    {
        return None;
    }
    let negative = t.ends_with('-') || t.starts_with('-') || t.starts_with('(');
    let digits: String = t
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let (whole, frac) = digits.split_once('.').unwrap_or((&digits, "0"));
    let frac = format!("{frac:0<2}");
    let v = whole.parse::<i64>().ok()? * 100 + frac.get(..2)?.parse::<i64>().ok()?;
    Some(if negative { -v } else { v })
}

/// The first amount after a summary line's label (this period; the next
/// column is the year to date).
fn first_amount(line: &str) -> Option<i64> {
    cells(line).iter().skip(1).find_map(|c| cents(c))
}

/// "TFSA   #123456789": a plan word, then the account number alone.
fn account_of(line: &str) -> Option<String> {
    let c = cells(line);
    let (plan, number) = (c.first()?, c.get(1)?);
    let digits = number.strip_prefix('#')?;
    if c.len() != 2
        || digits.len() < 4
        || !digits.chars().all(|ch| ch.is_ascii_digit())
        || !plan.chars().all(|ch| ch.is_ascii_uppercase())
    {
        return None;
    }
    Some(format!("{plan} …{}", &digits[digits.len() - 4..]))
}

pub fn read(text: &str) -> Summary {
    let mut s = Summary::default();
    // The summary's header line names its last day.
    let mut dated: Option<(usize, NaiveDate)> = None;
    let mut summary_end: Option<NaiveDate> = None;
    for (i, line) in text.lines().enumerate() {
        let lower = line.trim().to_lowercase();
        if let Some((_, d)) = full_dates(line).last() {
            dated = Some((i, *d));
        }
        let label = lower.trim_start_matches(['+', '-', '=', ' ']);
        if s.period.is_none() {
            let days: Vec<NaiveDate> = full_dates(line).into_iter().map(|(_, d)| d).collect();
            if let [start, end] = days[..]
                && start < end
                && (end - start).num_days() <= MAX_PERIOD_DAYS
            {
                s.period = Some((start, end));
            }
        }
        if s.account.is_none() {
            s.account = account_of(line);
        }
        if label.starts_with(OPENING) && s.opening.is_none() {
            s.opening = first_amount(line);
            summary_end = dated.filter(|(at, _)| i - at <= 3).map(|(_, d)| d);
        } else if label.starts_with(CLOSING) && s.closing.is_none() {
            s.closing = first_amount(line);
        } else if label.starts_with(CHANGE) && s.change.is_none() {
            s.change = first_amount(line);
        } else if label.starts_with(DEPOSITS) && lower.starts_with('+') {
            s.deposits += first_amount(line).unwrap_or(0);
        } else if label.starts_with(WITHDRAWALS) && lower.starts_with('-') {
            s.withdrawals += first_amount(line).unwrap_or(0).abs();
        }
    }
    // The first statement has no period line: it runs from the start of
    // its quarter, before the account opened (opening 0).
    if let Some(end) = summary_end {
        match s.period {
            Some((_, e)) if e == end => {}
            _ => s.period = quarter_start(end).map(|start| (start, end)),
        }
    }
    s
}

fn quarter_start(day: NaiveDate) -> Option<NaiveDate> {
    use chrono::Datelike;
    NaiveDate::from_ymd_opt(day.year(), (day.month() - 1) / 3 * 3 + 1, 1)
}

/// The statement as an export: no rows, one statement and the closing
/// market value as the balance.
pub fn parse(text: &str, name: &str, opts: &LoadOptions) -> Result<Export, String> {
    if !has_summary(&text.to_lowercase()) {
        let mut export = Export::empty(name, FORMAT);
        export
            .notes
            .push("a quarter statement without the market value summary: nothing to read".into());
        return Ok(export);
    }
    let s = read(text);
    let (Some((start, end)), Some(opening), Some(closing), Some(change)) =
        (s.period, s.opening, s.closing, s.change)
    else {
        return Err(format!(
            "{FORMAT}: no period or market value summary found. Run bin/cashcast columns"
        ));
    };
    if opening + s.deposits - s.withdrawals + change != closing {
        return Err(format!(
            "{FORMAT}: the market value summary does not add up, so nothing was read. Run bin/cashcast columns"
        ));
    }
    let account = opts
        .account
        .clone()
        .or(s.account)
        .unwrap_or_else(|| "Smart Investor".into());
    let mut export = Export::empty(name, FORMAT);
    export
        .account_kinds
        .insert(account.clone(), AccountKind::Bank);
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
    export.notes.push(format!(
        "the market value summary adds up; closing market value on {end}"
    ));
    Ok(export)
}

/// What was read, and the masked shapes of the period, account and
/// summary lines.
pub fn columns_text(text: &str, name: &str) -> String {
    let result = match parse(text, name, &LoadOptions::default()) {
        Ok(e) => match e.statements.first() {
            Some(st) => format!(
                "1 statement {} to {}; account {}; the summary adds up",
                st.start, st.end, st.account
            ),
            None => e.notes.join("; "),
        },
        Err(e) => format!("refused: {e}"),
    };
    let mut out = format!("- {name} ({FORMAT}): {result}\n");
    for (i, line) in text.lines().enumerate() {
        let lower = line.trim().to_lowercase();
        let label = lower.trim_start_matches(['+', '-', '=', ' ']);
        let summary = [OPENING, CLOSING, CHANGE, DEPOSITS, WITHDRAWALS]
            .iter()
            .any(|k| label.starts_with(k));
        if summary || !full_dates(line).is_empty() || account_of(line).is_some() {
            let _ = writeln!(out, "  line {}: {}", i + 1, line_shape(line));
        }
    }
    out
}
