//! Reads bank PDF statements: TD credit card, TD chequing, Scotiabank Visa
//! and Scotiabank chequing (decision 0016). E*TRADE stock plan statements
//! have their own reader (`etrade`).
//!
//! `pdftotext -layout` (poppler) turns the PDF into text lines on this
//! computer; nothing leaves it. One reader handles the four layouts: a row
//! is a short date ("JAN 3", "Jan03"), a description and an amount.
//!
//! Safety first: a statement is read only when its own balances add up
//! (previous balance + rows = new balance on a card; the running balance on
//! a chequing account). A file with a statement that does not add up is
//! refused as a whole, so a misread sign never reaches MoneyWiz.
//! `columns` prints each line's masked shape to fix a layout.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;

use crate::export::{AccountKind, ErrorRow, Export, LoadOptions, Txn, file_name};
use crate::text;

mod etrade;
mod scotia_ssi;
pub mod sunlife;

pub const TD_CREDIT: &str = "td-credit-pdf";
pub const TD_CHEQUING: &str = "td-chequing-pdf";
pub const SCOTIA_VISA: &str = "scotiabank-visa-pdf";
pub const SCOTIA_CHEQUING: &str = "scotiabank-chequing-pdf";
/// E*TRADE stock plan statement: cash rows of an investment account.
pub const ETRADE_STOCKPLAN: &str = etrade::FORMAT;
/// Sun Life group retirement statement: units per fund on its last day.
pub const SUNLIFE_STATEMENT: &str = sunlife::FORMAT;
/// Scotia Smart Investor statement: the plan's market value, no rows.
pub const SCOTIA_SSI: &str = scotia_ssi::FORMAT;
/// Every PDF statement format label.
pub const FORMATS: [&str; 7] = [
    TD_CREDIT,
    TD_CHEQUING,
    SCOTIA_VISA,
    SCOTIA_CHEQUING,
    ETRADE_STOCKPLAN,
    SUNLIFE_STATEMENT,
    SCOTIA_SSI,
];

/// True for a format label this module reads.
pub fn is_pdf_format(format: &str) -> bool {
    FORMATS.contains(&format)
}

/// A `.pdf` file, or any file that starts with the PDF magic bytes.
pub fn is_pdf_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "pdf" {
        return true;
    }
    let mut head = [0u8; 5];
    std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
        .is_ok()
        && &head == b"%PDF-"
}

fn tool() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    on_path
        .into_iter()
        .map(|d| d.join("pdftotext"))
        .chain(
            [
                "/opt/homebrew/bin/pdftotext",
                "/usr/local/bin/pdftotext",
                "/usr/bin/pdftotext",
            ]
            .map(PathBuf::from),
        )
        .find(|p| p.is_file())
}

/// True when `pdftotext` is installed (tests skip the PDF round trip without it).
pub fn tool_available() -> bool {
    tool().is_some()
}

/// The PDF's text, laid out in columns. Private: only this module reads it.
pub fn text_of(path: &Path) -> Result<String, String> {
    let tool = tool().ok_or(
        "reading PDF statements needs pdftotext. Install it: brew install poppler".to_string(),
    )?;
    let out = std::process::Command::new(tool)
        .args(["-layout", "-enc", "UTF-8", "-q"])
        .arg(path)
        .arg("-")
        .output()
        .map_err(|e| format!("cannot run pdftotext: {e}"))?;
    if !out.status.success() {
        // stderr is not printed: it may quote the file.
        return Err(format!(
            "pdftotext cannot read this PDF (exit {})",
            out.status.code().unwrap_or(-1)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn load(path: &Path, opts: &LoadOptions) -> Result<Export, String> {
    let text = text_of(path)?;
    parse(&text, &file_name(path), &stem_account(path), opts)
}

/// The file stem without a browser copy suffix (` (1)`): the account name
/// when the statement shows no account number.
fn stem_account(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map_or_else(|| "Statement".into(), |s| s.to_string_lossy().into_owned());
    match stem.rsplit_once(" (") {
        Some((head, tail))
            if tail.ends_with(')')
                && tail[..tail.len() - 1].bytes().all(|b| b.is_ascii_digit()) =>
        {
            head.to_string()
        }
        _ => stem,
    }
}

// ------------------------------------------------------------ layout

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bank {
    Td,
    Scotia,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Card,
    Chequing,
}

fn label(bank: Bank, kind: Kind) -> &'static str {
    match (bank, kind) {
        (Bank::Td, Kind::Card) => TD_CREDIT,
        (Bank::Td, Kind::Chequing) => TD_CHEQUING,
        (Bank::Scotia, Kind::Card) => SCOTIA_VISA,
        (Bank::Scotia, Kind::Chequing) => SCOTIA_CHEQUING,
    }
}

static TD_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bTD\b").unwrap());

/// Which bank and which statement. Brand words count only on lines without
/// an amount, so a payee such as "TD VISA PAYMENT" on a Scotiabank
/// statement never decides it.
fn layout(text: &str) -> Result<(Bank, Kind), String> {
    let (mut td, mut scotia) = (0, 0);
    for line in text.lines() {
        if !amounts(line).is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        for m in [
            "scotiabank",
            "bank of nova scotia",
            "scotia online",
            "scotia momentum",
            "scene+",
        ] {
            scotia += lower.matches(m).count();
        }
        for m in [
            "td canada trust",
            "toronto-dominion",
            "easyweb",
            "td aeroplan",
            "td cash back",
            "td rewards",
            "td first class",
        ] {
            td += lower.matches(m).count();
        }
        td += TD_WORD.find_iter(line).count();
    }
    let bank = match td.cmp(&scotia) {
        std::cmp::Ordering::Greater => Bank::Td,
        std::cmp::Ordering::Less => Bank::Scotia,
        std::cmp::Ordering::Equal => {
            return Err(
                "not a TD or Scotiabank statement (cannot tell the bank). Run bin/cashcast columns"
                    .into(),
            );
        }
    };
    let lower = text.to_lowercase();
    let card = ["posting", "post date", "minimum payment", "credit limit"]
        .iter()
        .filter(|w| lower.contains(*w))
        .count();
    let chequing = if lower.contains("withdraw") && lower.contains("deposit") {
        2
    } else {
        0
    };
    let kind = match card.cmp(&chequing) {
        std::cmp::Ordering::Greater => Kind::Card,
        std::cmp::Ordering::Less => Kind::Chequing,
        std::cmp::Ordering::Equal => {
            return Err(
                "cannot tell a card statement from a chequing one. Run bin/cashcast columns".into(),
            );
        }
    };
    Ok((bank, kind))
}

// ------------------------------------------------------------ tokens

const MONTHS: &str = "jan|feb|mar|apr|may|jun|jul|aug|sept|sep|oct|nov|dec";

static SHORT_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\b({MONTHS})\.?\s?(\d{{1,2}})\b")).unwrap());
static YEAR_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^,?\s*\d{4}\b").unwrap());
static SLASH_YEAR_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/\d{2}\b").unwrap());
static FULL_DATES: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        // Month name, day, year (comma and dot optional).
        Regex::new(&format!(
            r"(?i)\b({MONTHS})[a-z]*\.?\s+(\d{{1,2}}),?\s+(\d{{4}})\b"
        ))
        .unwrap(),
        // Day, month name, year.
        Regex::new(&format!(
            r"(?i)\b(\d{{1,2}})\s+({MONTHS})[a-z]*\.?,?\s+(\d{{4}})\b"
        ))
        .unwrap(),
        // ISO year-month-day.
        Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap(),
        // Month, day, slash, two-digit year (TD chequing).
        Regex::new(&format!(r"(?i)\b({MONTHS})\s?(\d{{1,2}})/(\d{{2}})\b")).unwrap(),
    ]
});
static AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\()?(-)?\s?\$?\s?(-)?(\d{1,3}(?:,\d{3})+\.\d{2}|\d+\.\d{2})(\))?(-|\s?CR\b)?")
        .unwrap()
});

fn month_of(s: &str) -> Option<u32> {
    let s = s.to_lowercase();
    let i = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|m| s.starts_with(m))?;
    Some(i as u32 + 1)
}

#[derive(Clone, Copy, Debug)]
struct Amount {
    start: usize,
    end: usize,
    cents: i64,
    negative: bool,
}

/// Every money amount on a line (two decimals), with its byte span.
fn amounts(line: &str) -> Vec<Amount> {
    let mut out = Vec::new();
    for c in AMOUNT.captures_iter(line) {
        let m = c.get(0).unwrap();
        let num = c.get(4).unwrap();
        // The match may start on spaces; the amount starts after them.
        let start = m.start() + (m.as_str().len() - m.as_str().trim_start().len());
        // Part of a longer token (an id or a date): skip.
        let before = line[..start].chars().next_back();
        if before.is_some_and(|b| b.is_alphanumeric() || b == '.' || b == '/' || b == '_') {
            continue;
        }
        let after = line[m.end()..].chars().next();
        if after.is_some_and(|a| a.is_alphanumeric() || a == '.' || a == '/' || a == '_') {
            continue;
        }
        let Some(cents) = crate::money::parse_cents(num.as_str(), '.') else {
            continue;
        };
        let negative =
            c.get(1).is_some() || c.get(2).is_some() || c.get(3).is_some() || c.get(6).is_some();
        out.push(Amount {
            start,
            end: m.end(),
            cents: cents.abs(),
            negative,
        });
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct Short {
    start: usize,
    end: usize,
    month: u32,
    day: u32,
}

/// Day-and-month dates without a year ("JAN 3", "Jan03"); a full date
/// with its year is not one.
fn short_dates(line: &str) -> Vec<Short> {
    SHORT_DATE
        .captures_iter(line)
        .filter_map(|c| {
            let m = c.get(0).unwrap();
            let rest = &line[m.end()..];
            if YEAR_AFTER.is_match(rest) || SLASH_YEAR_AFTER.is_match(rest) {
                return None;
            }
            let day: u32 = c.get(2)?.as_str().parse().ok()?;
            (1..=31).contains(&day).then_some(Short {
                start: m.start(),
                end: m.end(),
                month: month_of(c.get(1)?.as_str())?,
                day,
            })
        })
        .collect()
}

fn full_dates(line: &str) -> Vec<(usize, NaiveDate)> {
    let mut out = Vec::new();
    for (i, re) in FULL_DATES.iter().enumerate() {
        for c in re.captures_iter(line) {
            let g = |n: usize| c.get(n).map_or("", |m| m.as_str());
            let date = match i {
                0 => g(2)
                    .parse()
                    .ok()
                    .zip(g(3).parse().ok())
                    .and_then(|(d, y)| NaiveDate::from_ymd_opt(y, month_of(g(1))?, d)),
                1 => g(1)
                    .parse()
                    .ok()
                    .zip(g(3).parse().ok())
                    .and_then(|(d, y)| NaiveDate::from_ymd_opt(y, month_of(g(2))?, d)),
                2 => NaiveDate::from_ymd_opt(
                    g(1).parse().unwrap_or(0),
                    g(2).parse().unwrap_or(0),
                    g(3).parse().unwrap_or(0),
                ),
                _ => g(2)
                    .parse()
                    .ok()
                    .zip(g(3).parse::<i32>().ok())
                    .and_then(|(d, y)| NaiveDate::from_ymd_opt(2000 + y, month_of(g(1))?, d)),
            };
            if let Some(d) = date {
                out.push((c.get(0).unwrap().start(), d));
            }
        }
    }
    out.sort_by_key(|(at, _)| *at);
    out.dedup_by_key(|(at, _)| *at);
    out
}

fn has_letter(s: &str) -> bool {
    s.chars().any(char::is_alphabetic)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn char_col(line: &str, byte: usize) -> usize {
    line[..byte].chars().count()
}

// ------------------------------------------------------------ reading

const OPENING: &[&str] = &[
    "opening balance",
    "starting balance",
    "balance forward",
    "balance brought forward",
    "previous balance",
    "previous statement balance",
    "previous account balance",
];
const CLOSING: &[&str] = &[
    "closing balance",
    "ending balance",
    "new balance",
    "new statement balance",
    "closing account balance",
];
/// Lines that are never rows: summaries and notices.
const SKIP: &[&str] = &[
    "total",
    "subtotal",
    "minimum payment",
    "interest rate",
    "payment due",
    "credit limit",
    "available credit",
];

/// How `columns` labels a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    Header,
    Period,
    Balance,
    Row,
    More,
    Rejected(&'static str),
}

struct Row {
    line: usize,
    month: u32,
    day: u32,
    cents: i64,
    text: String,
    date: Option<NaiveDate>,
}

#[derive(Default)]
struct Segment {
    period: Option<(NaiveDate, NaiveDate)>,
    rows: Vec<Row>,
    opening: Option<i64>,
    closing: Option<i64>,
    /// Chequing: rows whose printed balance does not follow the last one.
    mismatches: usize,
    /// Chequing: printed row balances that were checked.
    checked: usize,
    running: Option<i64>,
    /// The dates on the opening and closing balance lines ("Opening balance
    /// on Nov 30, 2023"): the period when no line names one.
    open_on: Option<NaiveDate>,
    close_on: Option<NaiveDate>,
    /// Every printed balance (line, opening?, value), and for the `columns`
    /// hint the amounts on lines that were not read.
    balances: Vec<(usize, bool, i64)>,
    loose: Vec<(usize, i64)>,
}

struct Read {
    bank: Bank,
    kind: Kind,
    segments: Vec<Segment>,
    seen: Vec<(usize, Seen)>,
    bad: Vec<(usize, &'static str)>,
    account_tail: Option<String>,
}

/// Where the withdrawal, deposit and balance columns end (chequing).
#[derive(Default, Clone, Copy)]
struct Heads {
    /// Card statements: where the AMOUNT header cell ends.
    amount: Option<usize>,
    withdraw: Option<usize>,
    deposit: Option<usize>,
    balance: Option<usize>,
}

/// The column where the header word's cell ends (cells are split by two
/// or more spaces).
fn cell_end(line: &str, word: &str) -> Option<usize> {
    let lower = line.to_lowercase();
    let at = lower.find(word)?;
    let rest = &lower[at..];
    let len = rest.find("  ").unwrap_or(rest.len());
    Some(char_col(&lower, at + len))
}

fn is_header(lower: &str, has_amount: bool) -> bool {
    (lower.contains("withdraw") && lower.contains("deposit"))
        || (!has_amount
            && lower.contains("date")
            && ["description", "details", "transactions", "amount"]
                .iter()
                .any(|w| lower.contains(w)))
}

/// Joins a row whose amount wrapped: a dated line with no amount, then
/// (past blank lines and at most one more description line) a line that
/// holds only an amount, printed right of the description. The amount moves
/// up onto the dated line and its own line becomes blank, so line numbers
/// stay the same and the description line still reads as a continuation.
fn join_wrapped(text: &str, card: bool) -> Vec<String> {
    let mut out: Vec<String> = text.lines().map(str::to_string).collect();
    let plain =
        |l: &str| short_dates(l).is_empty() && amounts(l).is_empty() && full_dates(l).is_empty();
    for i in 0..out.len() {
        let a = out[i].trim_end().to_string();
        if short_dates(&a).is_empty()
            || !amounts(&a).is_empty()
            || !full_dates(&a).is_empty()
            || !has_letter(&a)
        {
            continue;
        }
        let next: Vec<usize> = (i + 1..out.len().min(i + 5))
            .filter(|&j| !out[j].trim().is_empty())
            .take(2)
            .collect();
        // A card row of only a reference and dates, with its details and
        // amount on the next line: the next line moves up after the dates.
        if card && let Some(&j) = next.first() {
            let mut rest = a.clone();
            for d in short_dates(&a).iter().rev() {
                rest.replace_range(d.start..d.end, "");
            }
            let b = out[j].trim().to_string();
            let found = amounts(&b);
            if !has_letter(&rest)
                && !found.is_empty()
                && short_dates(&b).is_empty()
                && full_dates(&b).is_empty()
                && has_letter(&b[..found[0].start])
            {
                out[i] = format!("{a}   {b}");
                out[j] = String::new();
                continue;
            }
        }
        let j = match next.as_slice() {
            [j, ..] if !amounts(&out[*j]).is_empty() => *j,
            [d, j]
                if plain(&out[*d])
                    && has_letter(&out[*d])
                    && out[*d].trim().chars().count() <= 60
                    && !amounts(&out[*j]).is_empty() =>
            {
                *j
            }
            _ => continue,
        };
        let b = out[j].trim_end().to_string();
        let found = amounts(&b);
        let lead = b.len() - b.trim_start().len();
        if !short_dates(&b).is_empty()
            || !full_dates(&b).is_empty()
            || has_letter(&b[..found[0].start])
            || char_col(&b, lead) <= a.chars().count()
        {
            continue;
        }
        let pad = char_col(&b, lead) - a.chars().count();
        out[i] = format!("{a}{}{}", " ".repeat(pad), b.trim_start());
        out[j] = String::new();
    }
    out
}

fn read(text: &str) -> Result<Read, String> {
    let (bank, kind) = layout(text)?;
    let joined = join_wrapped(text, kind == Kind::Card);
    let lines: Vec<&str> = joined.iter().map(String::as_str).collect();
    let mut segments: Vec<Segment> = vec![Segment::default()];
    let mut seen = Vec::new();
    let mut bad = Vec::new();
    let mut heads = Heads::default();
    let mut last_row_line: Option<usize> = None;
    for (i, raw) in lines.iter().enumerate() {
        let line_no = i + 1;
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        let found = amounts(line);
        let fulls = full_dates(line);

        // Statement period: two full dates, said to be the statement's (on
        // the line, or on the label line above, past one blank line).
        let prev = (i.saturating_sub(2)..i)
            .rev()
            .find(|&p| !lines[p].trim().is_empty())
            .map(|p| lines[p].to_lowercase())
            .unwrap_or_default();
        let about_period = ["statement", "period", "from"]
            .iter()
            .any(|w| lower.contains(w) || prev.contains(w));
        // An amount right of both dates is a box printed beside the period.
        if fulls.len() >= 2
            && about_period
            && !lower.contains("due")
            && found.iter().all(|a| a.start > fulls[1].0)
        {
            let (a, b) = (fulls[0].1, fulls[1].1);
            let period = (a.min(b), a.max(b));
            let cur = segments.last_mut().unwrap();
            match cur.period {
                None => cur.period = Some(period),
                Some(p) if p == period => {}
                Some(_) => segments.push(Segment {
                    period: Some(period),
                    ..Segment::default()
                }),
            }
            seen.push((line_no, Seen::Period));
            continue;
        }
        if fulls.len() == 1 && lower.contains("statement date") && found.is_empty() {
            let cur = segments.last_mut().unwrap();
            if cur.period.is_none() {
                let end = fulls[0].1;
                cur.period = Some((end - Duration::days(35), end));
            }
            seen.push((line_no, Seen::Period));
            continue;
        }

        // Column headers (chequing signs come from their positions).
        if is_header(&lower, !found.is_empty()) {
            if let Some(w) = cell_end(line, "withdraw") {
                heads.withdraw = Some(w);
            }
            if let Some(d) = cell_end(line, "deposit") {
                heads.deposit = Some(d);
            }
            if let Some(b) = cell_end(line, "balance") {
                heads.balance = Some(b);
            }
            if let Some(a) = cell_end(line, "amount") {
                heads.amount = Some(a);
            }
            seen.push((line_no, Seen::Header));
            continue;
        }

        // Opening and closing balances.
        let opening = OPENING.iter().filter_map(|k| lower.find(k)).min();
        let closing = CLOSING.iter().filter_map(|k| lower.find(k)).min();
        let mut was_balance = false;
        for (at, is_open) in [(opening, true), (closing, false)] {
            let Some(at) = at else { continue };
            let Some(a) = found.iter().find(|a| a.start > at) else {
                continue;
            };
            let v = if a.negative { -a.cents } else { a.cents };
            let cur = segments.last_mut().unwrap();
            cur.balances.push((line_no, is_open, v));
            let on = fulls.iter().find(|(d, _)| *d < a.start).map(|(_, d)| *d);
            if is_open && cur.open_on.is_none() {
                cur.open_on = on;
            } else if !is_open && cur.close_on.is_none() {
                cur.close_on = on;
            }
            if is_open {
                if cur.opening.is_none() {
                    cur.opening = Some(v);
                    if kind == Kind::Chequing && cur.rows.is_empty() {
                        cur.running = Some(v);
                    }
                }
            } else if cur.closing.is_none() {
                cur.closing = Some(v);
            }
            was_balance = true;
        }
        let shorts = short_dates(line);
        // A card statement may print a summary box beside its rows: the
        // row on the left still reads.
        if was_balance && (kind == Kind::Chequing || shorts.is_empty()) {
            seen.push((line_no, Seen::Balance));
            continue;
        }

        if shorts.is_empty() || found.is_empty() {
            // A plain text line right after a row continues its description.
            if shorts.is_empty()
                && found.is_empty()
                && fulls.is_empty()
                && last_row_line == Some(line_no - 1)
                && has_letter(line)
                && !SKIP.iter().any(|k| lower.contains(k))
                && line.trim().chars().count() <= 60
            {
                if let Some(r) = segments.last_mut().unwrap().rows.last_mut() {
                    r.text = squash(&format!("{} {}", r.text, line));
                }
                seen.push((line_no, Seen::More));
                last_row_line = Some(line_no);
            } else if !shorts.is_empty() || !found.is_empty() {
                let cur = segments.last_mut().unwrap();
                for a in &found {
                    cur.loose
                        .push((line_no, if a.negative { -a.cents } else { a.cents }));
                }
                seen.push((line_no, Seen::Rejected("a date or an amount alone")));
            }
            continue;
        }
        // Only the text before the first amount: a summary box printed
        // beside a row does not make the row a summary.
        let head = line[..found[0].start].to_lowercase();
        if SKIP.iter().any(|k| head.contains(k)) {
            seen.push((line_no, Seen::Rejected("a summary line")));
            continue;
        }

        let row = match kind {
            Kind::Card => card_row(line, &shorts, &found, heads.amount),
            Kind::Chequing => {
                chequing_row(line, &shorts, &found, heads, segments.last_mut().unwrap())
            }
        };
        match row {
            Ok((month, day, cents, text)) => {
                segments.last_mut().unwrap().rows.push(Row {
                    line: line_no,
                    month,
                    day,
                    cents,
                    text,
                    date: None,
                });
                seen.push((line_no, Seen::Row));
                last_row_line = Some(line_no);
            }
            Err(reason) => {
                bad.push((line_no, reason));
                seen.push((line_no, Seen::Rejected(reason)));
            }
        }
    }
    for s in &mut segments {
        if let (None, Some(a), Some(b)) = (s.period, s.open_on, s.close_on) {
            s.period = Some((a.min(b), a.max(b)));
        }
    }
    // A period with no rows and no balances is a header line that names two
    // dates (statement date and previous statement), not a statement.
    segments.retain(|s| !s.rows.is_empty() || s.opening.is_some() || s.closing.is_some());
    for s in &mut segments {
        let Some((start, end)) = s.period else {
            for r in &s.rows {
                bad.push((r.line, "no statement period found for this row"));
            }
            continue;
        };
        for r in &mut s.rows {
            r.date = resolve(r.month, r.day, start, end);
            if r.date.is_none() {
                bad.push((r.line, "date not in the statement period"));
            }
        }
    }
    Ok(Read {
        bank,
        kind,
        segments,
        seen,
        bad,
        account_tail: account_tail(text),
    })
}

/// The year of a day-and-month date: the statement end's year, or the year
/// before when that day falls after the end (a December row on a January
/// statement).
fn resolve(month: u32, day: u32, start: NaiveDate, end: NaiveDate) -> Option<NaiveDate> {
    for year in [end.year(), end.year() - 1] {
        if let Some(d) = NaiveDate::from_ymd_opt(year, month, day) {
            if d <= end + Duration::days(7) && d >= start - Duration::days(62) {
                return Some(d);
            }
        }
    }
    None
}

/// Card row: [ref] date [date] description amount. Purchases are money out;
/// a payment or refund (marked -, CR or in brackets) is money in.
fn card_row(
    line: &str,
    shorts: &[Short],
    found: &[Amount],
    amount_col: Option<usize>,
) -> Result<(u32, u32, i64, String), &'static str> {
    let first = shorts[0];
    let lead = line[..first.start].trim();
    // A reference number, and maybe a page-margin number before it.
    let groups: Vec<&str> = lead.split_whitespace().collect();
    let digits = |g: &&str| g.bytes().all(|b| b.is_ascii_digit());
    if groups.len() > 2 || !groups.iter().all(digits) || groups.last().is_some_and(|g| g.len() > 4)
    {
        return Err("text before the date");
    }
    let mut desc_start = first.end;
    if let Some(second) = shorts.get(1) {
        if line[first.end..second.start].trim().is_empty() {
            desc_start = second.end;
        }
    }
    let after: Vec<&Amount> = found.iter().filter(|a| a.start >= desc_start).collect();
    let Some(&first_amount) = after.first() else {
        return Err("no amount after the description");
    };
    // The amount under the AMOUNT header; a foreign amount in the details
    // (USD 4.00) comes first. Without a header, the first amount.
    let a = amount_col
        .and_then(|col| {
            after
                .iter()
                .map(|a| (char_col(line, a.end).abs_diff(col), *a))
                .filter(|(d, _)| *d <= 10)
                .min_by_key(|(d, _)| *d)
                .map(|(_, a)| a)
        })
        .unwrap_or(first_amount);
    let desc = squash(&line[desc_start..a.start]);
    if !has_letter(&desc) {
        return Err("no description");
    }
    let cents = if a.negative { a.cents } else { -a.cents };
    Ok((first.month, first.day, cents, desc))
}

/// Chequing row: one date, a description, the amount and maybe the
/// balance, in any order (TD prints the date after the amounts). The sign
/// comes from the running balance when one is printed, else from the
/// column the amount sits under.
fn chequing_row(
    line: &str,
    shorts: &[Short],
    found: &[Amount],
    heads: Heads,
    seg: &mut Segment,
) -> Result<(u32, u32, i64, String), &'static str> {
    if shorts.len() != 1 {
        return Err("more than one date");
    }
    let date = shorts[0];
    let (amount, balance) = match found {
        [a] => (*a, None),
        [a, b] => (*a, Some(*b)),
        _ => return Err("more than two amounts"),
    };
    let col = char_col(line, amount.end);
    // One amount under the balance column is a balance, not a row.
    if balance.is_none() {
        if let Some(b) = heads.balance {
            let other = [heads.withdraw, heads.deposit]
                .iter()
                .flatten()
                .map(|c| c.abs_diff(col))
                .min();
            if other.is_some_and(|o| b.abs_diff(col) < o) {
                return Err("an amount under the balance column only");
            }
        }
    }
    let mut spans = vec![(date.start, date.end), (amount.start, amount.end)];
    if let Some(b) = balance {
        spans.push((b.start, b.end));
    }
    spans.sort();
    let mut desc = String::new();
    let mut at = 0;
    for (s, e) in spans {
        desc.push_str(&line[at..s.max(at)]);
        desc.push(' ');
        at = e.max(at);
    }
    desc.push_str(&line[at..]);
    let desc = squash(&desc);
    if !has_letter(&desc) {
        return Err("no description");
    }
    let by_column = match (heads.withdraw, heads.deposit) {
        (Some(w), Some(d)) => Some(if w.abs_diff(col) <= d.abs_diff(col) {
            -1
        } else {
            1
        }),
        _ => None,
    };
    let printed = balance.map(|b| if b.negative { -b.cents } else { b.cents });
    let by_balance = match (seg.running, printed) {
        (Some(r), Some(p)) if r - amount.cents == p => Some(-1),
        (Some(r), Some(p)) if r + amount.cents == p => Some(1),
        _ => None,
    };
    let sign = match (by_balance, by_column) {
        (Some(s), _) | (None, Some(s)) => s,
        (None, None) => return Err("cannot tell a withdrawal from a deposit"),
    };
    let cents = sign * amount.cents;
    if let Some(p) = printed {
        if seg.running.is_some() {
            seg.checked += 1;
            if by_balance.is_none() {
                seg.mismatches += 1;
            }
        }
        seg.running = Some(p);
    } else if let Some(r) = seg.running {
        seg.running = Some(r + cents);
    }
    Ok((date.month, date.day, cents, desc))
}

static ACCOUNT_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9Xx*•]{2,}(?:[ -]?[0-9Xx*•]{2,}){1,6}").unwrap());

/// The last four digits of the account or card number, from the first line
/// that names one ("Account number", "Card number"). Phone lines never count.
fn account_tail(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    const NAMES: [&str; 6] = [
        "account no",
        "account number",
        "account #",
        "card no",
        "card number",
        "account:",
    ];
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        let Some(label) = NAMES.iter().filter_map(|k| lower.find(k)).min() else {
            continue;
        };
        if ["call", "phone", "tel", "fax", "1-8"]
            .iter()
            .any(|k| lower.contains(k))
        {
            continue;
        }
        if let Some(t) = account_run(line, None) {
            return Some(t);
        }
        // A boxed header prints the label above its value, in the same
        // column, a line or two below.
        // Measured from the start of the label's cell ("Your account number:").
        let cell = line[..label].rfind("  ").map_or(0, |p| p + 2);
        let col = char_col(line, cell);
        for next in lines.iter().skip(i + 1).take(3) {
            if let Some(t) = account_run(next, Some(col)) {
                return Some(t);
            }
        }
    }
    None
}

/// The last 4 digits of an account-number run on `line`; with `col`, only a
/// run that starts within 8 columns of it.
fn account_run(line: &str, col: Option<usize>) -> Option<String> {
    for m in ACCOUNT_RUN.find_iter(line) {
        let run = m.as_str();
        let lead = run.len() - run.trim_start().len();
        if col.is_some_and(|c| char_col(line, m.start() + lead).abs_diff(c) > 8) {
            continue;
        }
        let digits = run.chars().filter(char::is_ascii_digit).count();
        let symbols = run.chars().filter(|c| !matches!(c, ' ' | '-')).count();
        // The last 4 digits may span groups ("01234 00024 68").
        let tail: Vec<char> = run
            .chars()
            .rev()
            .filter(|c| !matches!(c, ' ' | '-'))
            .take(4)
            .collect();
        if symbols >= 7 && digits >= 4 && tail.iter().all(char::is_ascii_digit) {
            return Some(tail.into_iter().rev().collect());
        }
    }
    None
}

// ------------------------------------------------------------ export

/// Reads statement text into an export. Refuses the whole file when a
/// statement's balances do not add up.
pub fn parse(text: &str, name: &str, stem: &str, opts: &LoadOptions) -> Result<Export, String> {
    if etrade::is_statement(text) {
        return etrade::parse(text, name, opts);
    }
    if sunlife::is_statement(text) {
        return sunlife::parse(text, name);
    }
    if scotia_ssi::is_statement(text) {
        return scotia_ssi::parse(text, name, opts);
    }
    let r = read(text)?;
    let format = label(r.bank, r.kind);
    let (kind, word) = match r.kind {
        Kind::Card => (AccountKind::Card, "Credit card"),
        Kind::Chequing => (AccountKind::Bank, "Checking"),
    };
    let account = opts
        .account
        .clone()
        .unwrap_or_else(|| match &r.account_tail {
            Some(t) => format!("{word} …{t}"),
            None => stem.to_string(),
        });
    let mut export = Export::empty(name, format);
    export.date_format = "%b %d".into();
    export.account_kinds.insert(account.clone(), kind);
    let currency = opts.currency.clone().unwrap_or_else(|| "CAD".into());

    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;
    for s in &r.segments {
        let Some((start, end)) = s.period else {
            continue;
        };
        let sum: i64 = s
            .rows
            .iter()
            .filter(|r| r.date.is_some())
            .map(|r| r.cents)
            .sum();
        let ok = match r.kind {
            // Owed amounts: new = previous + purchases - payments. A card
            // statement may print more than one "previous balance" (a cash
            // back box has its own): one printed pair must add up.
            Kind::Card => s.opening.zip(s.closing).map(|_| {
                let opens = s.balances.iter().filter(|b| b.1);
                opens
                    .flat_map(|o| s.balances.iter().filter(|c| !c.1).map(move |c| (o.2, c.2)))
                    .any(|(p, n)| p - sum == n)
            }),
            Kind::Chequing => {
                let ends = s.opening.zip(s.closing).map(|(o, c)| o + sum == c);
                if ends == Some(false) || s.mismatches > 0 {
                    Some(false)
                } else if ends == Some(true) || s.checked > 0 {
                    Some(true)
                } else {
                    None
                }
            }
        };
        match ok {
            Some(true) => checked += 1,
            Some(false) => failed.push(format!("{start} to {end}")),
            None => {}
        }
    }
    if !failed.is_empty() {
        return Err(format!(
            "{format}: the balances do not add up on {} statement(s) ({}), so no row was read. Run bin/cashcast columns and fix the reader, or mark these months as a known gap",
            failed.len(),
            failed.join(", ")
        ));
    }
    let statements = r.segments.iter().filter(|s| s.period.is_some()).count();

    let mut nth: HashMap<(NaiveDate, i64), usize> = HashMap::new();
    for s in &r.segments {
        for row in &s.rows {
            let Some(date) = row.date else { continue };
            let n = nth.entry((date, row.cents)).or_insert(0);
            *n += 1;
            let derived = text::derive(&row.text, row.cents);
            if derived.payee_key != 0 {
                export
                    .payee_labels
                    .entry(derived.payee_key)
                    .or_insert_with(|| row.text.chars().take(40).collect());
            }
            // The same row in two copies of one statement (a yearly and a
            // monthly download) gets the same fingerprint, so the inbox
            // keeps one.
            let mut h = DefaultHasher::new();
            ("pdf", &account, date, row.cents, *n).hash(&mut h);
            export.txns.push(Txn {
                file: 0,
                account: account.clone(),
                transfer: String::new(),
                category: derived
                    .category
                    .map(|c| vec![c.to_string()])
                    .unwrap_or_default(),
                category_from_file: false,
                hint: derived.hint,
                payee_key: derived.payee_key,
                date,
                cents: row.cents,
                currency: currency.clone(),
                fingerprint: h.finish(),
                text: row.text.clone(),
            });
        }
    }
    // Each statement's own balances, so `diagnose` can check that one
    // statement's closing is the next one's opening (a missing month
    // breaks the chain) and that the rows lead to them.
    for s in &r.segments {
        let (Some((start, end)), Some(open), Some(close)) = (s.period, s.opening, s.closing) else {
            continue;
        };
        let sum: i64 = s
            .rows
            .iter()
            .filter(|r| r.date.is_some())
            .map(|r| r.cents)
            .sum();
        let (opening, closing) = match r.kind {
            Kind::Chequing if open + sum == close => (open, close),
            // Owed amounts: the account's balance is the negative.
            Kind::Card if open - sum == close => (-open, -close),
            _ => continue,
        };
        let st = crate::export::Statement {
            account: account.clone(),
            start,
            end,
            opening,
            closing,
        };
        if !export.statements.contains(&st) {
            export.statements.push(st);
        }
    }
    export.bad_lines = r.bad.clone();
    export.bad_lines.sort();
    if export.txns.is_empty() && export.bad_lines.is_empty() {
        return Err(format!(
            "{format}: no transaction rows found. Run bin/cashcast columns"
        ));
    }
    export.notes.push(format!(
        "{statements} statement(s), balances checked on {checked}"
    ));
    if r.account_tail.is_none() && opts.account.is_none() {
        export
            .notes
            .push("no account number found: the account is the file name".into());
    }
    Ok(export)
}

/// The first failing row, for `first-error` only: that one raw line.
pub fn error_row(path: &Path, line: usize, reason: &'static str) -> Result<ErrorRow, String> {
    let text = text_of(path)?;
    let raw = text.lines().nth(line.saturating_sub(1)).unwrap_or("");
    let records_before = if etrade::is_statement(&text) {
        etrade::rows_before(&text, line)
    } else {
        read(&text)
            .map(|r| {
                r.segments
                    .iter()
                    .flat_map(|s| &s.rows)
                    .filter(|row| row.line < line)
                    .count()
            })
            .unwrap_or(0)
    };
    Ok(ErrorRow {
        line,
        reason,
        records_before,
        fields: vec![("raw line".to_string(), squash(raw))],
    })
}

/// True when the file is a statement this module reads (for `fetch
/// collect`, which leaves other PDFs in Downloads).
pub fn sniff(path: &Path) -> bool {
    text_of(path).is_ok_and(|t| {
        etrade::is_statement(&t)
            || sunlife::is_statement(&t)
            || scotia_ssi::is_statement(&t)
            || layout(&t).is_ok()
    })
}

// ------------------------------------------------------------ columns

/// Words a shape may print as they are: layout words, never values.
const LAYOUT_WORDS: &[&str] = &[
    "DATE",
    "DATES",
    "TRANSACTION",
    "TRANSACTIONS",
    "TRANS",
    "POSTING",
    "POST",
    "REF",
    "ACTIVITY",
    "DESCRIPTION",
    "DETAILS",
    "AMOUNT",
    "AMOUNTS",
    "WITHDRAWALS",
    "WITHDRAWAL",
    "WITHDRAWN",
    "DEPOSITS",
    "DEPOSIT",
    "DEPOSITED",
    "BALANCE",
    "PREVIOUS",
    "NEW",
    "STATEMENT",
    "PERIOD",
    "FROM",
    "TO",
    "OPENING",
    "CLOSING",
    "STARTING",
    "ENDING",
    "FORWARD",
    "TOTAL",
    "ACCOUNT",
    "CARD",
    "NUMBER",
    "NO",
    "PAGE",
    "OF",
    "VISA",
    "SCOTIABANK",
    "TD",
    "CANADA",
    "TRUST",
    "CHEQUING",
    "CREDIT",
    "PAYMENT",
    "PAYMENTS",
    "PURCHASES",
    "INTEREST",
    "FEES",
    "CONTINUED",
    "SUMMARY",
    "YOUR",
    "CASH",
    "FLOW",
    "BY",
    "NET",
    "CREDITS",
    "DEBITS",
    "SETTLEMENT",
    "TYPE",
    "COMMENTS",
    "QUANTITY",
    "PRICE",
    // Group retirement statements (Sun Life): words, never values.
    "SUN",
    "LIFE",
    "GROUP",
    "RETIREMENT",
    "SAVINGS",
    "REGISTERED",
    "RRSP",
    "TFSA",
    "DPSP",
    "PLAN",
    "MEMBER",
    "EMPLOYER",
    "EMPLOYEE",
    "CONTRIBUTION",
    "CONTRIBUTIONS",
    "MONEY",
    "IN",
    "OUT",
    "ON",
    "AT",
    "AS",
    "AND",
    "THE",
    "FOR",
    "CHANGE",
    "VALUE",
    "MARKET",
    "UNIT",
    "UNITS",
    "FUND",
    "FUNDS",
    "INVESTMENT",
    "INVESTMENTS",
    "TRANSFER",
    "TRANSFERS",
    "FEE",
    "RATE",
    "RETURN",
    "RETURNS",
    "PERSONAL",
    "BEGINNING",
    "END",
    "JANUARY",
    "FEBRUARY",
    "MARCH",
    "APRIL",
    "MAY",
    "JUNE",
    "JULY",
    "AUGUST",
    "SEPTEMBER",
    "OCTOBER",
    "NOVEMBER",
    "DECEMBER",
];

fn token_shape(tok: &str) -> String {
    let t = tok.trim();
    if let Some(a) = amounts(t).first() {
        if a.start == 0 && a.end == t.len() {
            return if a.negative {
                "num-".into()
            } else {
                "num".into()
            };
        }
    }
    if let Some(s) = short_dates(t).first() {
        if s.start == 0 && s.end == t.len() {
            let case = if t[..3].chars().all(char::is_uppercase) {
                "MON"
            } else {
                "Mon"
            };
            let sep = if t[3..].starts_with(|c: char| c.is_ascii_digit()) {
                ""
            } else {
                " "
            };
            return format!("{case}{sep}9");
        }
    }
    if full_dates(t).first().is_some_and(|(at, _)| *at == 0) {
        return "fulldate".into();
    }
    let words: Vec<&str> = t
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if !words.is_empty()
        && words
            .iter()
            .all(|w| LAYOUT_WORDS.contains(&w.to_uppercase().as_str()))
    {
        return t.to_string();
    }
    let mut out = String::new();
    for ch in t.chars().take(40) {
        let m = if ch.is_alphabetic() {
            'a'
        } else if ch.is_ascii_digit() {
            '9'
        } else if ch.is_whitespace() {
            ' '
        } else {
            ch
        };
        if !out.ends_with(m) {
            out.push(m);
        }
    }
    out
}

/// One line as masked cells with their columns: `@0 MON 9 | @14 a a | @60 num`.
/// Cells are split by two or more spaces.
fn line_shape(line: &str) -> String {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut cells = Vec::new();
    let mut start: Option<usize> = None;
    for k in 0..=chars.len() {
        let gap =
            k == chars.len() || (chars[k].1 == ' ' && chars.get(k + 1).is_none_or(|n| n.1 == ' '));
        match start {
            None if !gap && chars[k].1 != ' ' => start = Some(k),
            Some(s) if gap => {
                let to = chars.get(k).map_or(line.len(), |c| c.0);
                cells.push(format!("@{s} {}", token_shape(&line[chars[s].0..to])));
                start = None;
            }
            _ => {}
        }
    }
    cells.join(" | ")
}

/// How many rows `columns` prints per file; other lines print up to
/// `SHOWN_LINES`.
const SHOWN_ROWS: usize = 6;
const SHOWN_LINES: usize = 60;

/// The layout of a PDF statement without its values: each header, period,
/// balance and row line as masked cells, rejected lines with the reason,
/// and what the reader made of the file.
pub fn columns(path: &Path) -> String {
    let name = file_name(path);
    let text = match text_of(path) {
        Ok(t) => t,
        Err(e) => return format!("- {name}: {e}\n"),
    };
    columns_text(&text, &name, &stem_account(path))
}

/// Which change would make a refused card statement add up: another
/// printed opening or closing balance, one row left out or sign-flipped,
/// every row from one line on left out, or an unread amount added. Names
/// line numbers only, never an amount.
fn card_hint(s: &Segment) -> Option<String> {
    let rows: Vec<&Row> = s.rows.iter().filter(|r| r.date.is_some()).collect();
    let sum: i64 = rows.iter().map(|r| r.cents).sum();
    let opens: Vec<(usize, i64)> = s
        .balances
        .iter()
        .filter(|b| b.1)
        .map(|b| (b.0, b.2))
        .collect();
    let closes: Vec<(usize, i64)> = s
        .balances
        .iter()
        .filter(|b| !b.1)
        .map(|b| (b.0, b.2))
        .collect();
    if opens.is_empty() || closes.is_empty() {
        return Some("no opening or closing balance found".into());
    }
    let mut found = Vec::new();
    for &(ol, p) in &opens {
        for &(cl, n) in &closes {
            let pair = format!("balances on lines {ol} and {cl}");
            if p - sum == n {
                found.push(pair.clone());
            }
            for r in &rows {
                if p - (sum - r.cents) == n {
                    found.push(format!("{pair}, line {} left out", r.line));
                }
                if p - (sum - 2 * r.cents) == n {
                    found.push(format!("{pair}, line {} sign flipped", r.line));
                }
            }
            for k in 1..rows.len() {
                let head: i64 = rows[..k].iter().map(|r| r.cents).sum();
                if p - head == n {
                    found.push(format!(
                        "{pair}, rows from line {} on left out",
                        rows[k].line
                    ));
                }
            }
            for &(ll, v) in &s.loose {
                if p - (sum - v) == n || p - (sum + v) == n {
                    found.push(format!("{pair}, the amount on line {ll} added"));
                }
            }
        }
    }
    found.dedup();
    Some(if found.is_empty() {
        format!(
            "{} rows read; no simple change explains the gap",
            rows.len()
        )
    } else {
        format!("the sum matches with: {}", found.join("; "))
    })
}

pub fn columns_text(text: &str, name: &str, stem: &str) -> String {
    if etrade::is_statement(text) {
        return etrade::columns_text(text, name);
    }
    if sunlife::is_statement(text) {
        return sunlife::columns_text(text, name);
    }
    if scotia_ssi::is_statement(text) {
        return scotia_ssi::columns_text(text, name);
    }
    let r = match read(text) {
        Ok(r) => r,
        Err(e) => {
            let mut out = format!("- {name} (PDF, not recognized: {e}):\n");
            for (i, l) in text
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.trim().is_empty())
                .take(30)
            {
                let _ = writeln!(out, "  line {}: {}", i + 1, line_shape(l));
            }
            return out;
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let result = match parse(text, name, stem, &LoadOptions::default()) {
        Ok(e) => format!(
            "{} rows, {} unread, {}; account {}",
            e.txns.len(),
            e.bad_lines.len(),
            e.notes.join("; "),
            e.account_kinds.keys().next().map_or("?", String::as_str)
        ),
        Err(e) => format!("refused: {e}"),
    };
    // A refused file shows every row: the one that broke the sum is in there.
    let (shown_rows, shown_lines) = if result.starts_with("refused") {
        (usize::MAX, 400)
    } else {
        (SHOWN_ROWS, SHOWN_LINES)
    };
    let mut out = format!("- {name} ({}): {result}\n", label(r.bank, r.kind));
    if result.starts_with("refused") && r.kind == Kind::Card {
        for s in &r.segments {
            if let Some(h) = card_hint(s) {
                let _ = writeln!(out, "  hint: {h}");
            }
        }
    }
    let (mut rows, mut printed) = (0, 0);
    for (n, seen) in &r.seen {
        if printed >= shown_lines {
            break;
        }
        let tag = match seen {
            Seen::Row => {
                rows += 1;
                if rows > shown_rows {
                    continue;
                }
                "row".to_string()
            }
            Seen::More if rows > shown_rows => continue,
            Seen::More => "more".into(),
            Seen::Header => "header".into(),
            Seen::Period => "period".into(),
            Seen::Balance => "balance".into(),
            Seen::Rejected(why) => format!("not read: {why}"),
        };
        let _ = writeln!(out, "  line {n} [{tag}]: {}", line_shape(lines[n - 1]));
        printed += 1;
        // After a line it could not read, the next unclassified lines show
        // why (an amount that wrapped below, a box printed in between).
        if matches!(seen, Seen::Rejected(_)) {
            for m in *n + 1..(*n + 3).min(lines.len() + 1) {
                if lines[m - 1].trim().is_empty() || r.seen.iter().any(|(k, _)| *k == m) {
                    continue;
                }
                let _ = writeln!(out, "  line {m} [skipped]: {}", line_shape(lines[m - 1]));
                printed += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_mask_values_and_keep_layout_words() {
        let s = line_shape("JAN 3     JAN 4     CANARY COFFEE SHOP        $45.67");
        assert_eq!(s, "@0 MON 9 | @10 MON 9 | @20 a a a | @46 num");
        assert_eq!(
            line_shape("DATE   DESCRIPTION   AMOUNT($)"),
            "@0 DATE | @7 DESCRIPTION | @21 AMOUNT($)"
        );
        assert_eq!(token_shape("500.00-"), "num-");
        assert_eq!(token_shape("FEB01"), "MON9");
    }

    #[test]
    fn amounts_read_signs_and_skip_ids() {
        // A foreign amount glued to its currency (20.00_USD) is description.
        let a = amounts("PAYMENT  -$1,500.00  12.50-  3.00 CR  (4.00)  ID12.34  20.00_USD");
        let got: Vec<(i64, bool)> = a.iter().map(|a| (a.cents, a.negative)).collect();
        assert_eq!(
            got,
            [(150_000, true), (1_250, true), (300, true), (400, true)]
        );
    }

    #[test]
    fn a_short_date_is_not_part_of_a_full_date() {
        assert_eq!(short_dates("Jan 11, 2024").len(), 0);
        assert_eq!(short_dates("JAN 31/24").len(), 0);
        assert_eq!(short_dates("MARKET 12 JAN03").len(), 1);
    }
}
