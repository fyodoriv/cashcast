//! Reads investment and crypto ledgers and computes holdings.
//!
//! Formats: Kraken ledger, Wealthsimple activity, E*TRADE transactions,
//! E*TRADE stock plan, Sun Life group plan transactions, and generic crypto
//! wallet CSVs (Casa and others). These files are not cash transactions.
//! The inbox keeps them out of the cash import, and the `holdings` command
//! reads them here.
//!
//! This module reads raw rows. Only asset symbols, account types (such as
//! TFSA), years, quantities, CAD sums and line numbers leave it. Account
//! ids, security names, notes, descriptions, emails and grant numbers are
//! never read past the header. One exception: Sun Life group funds have no
//! ticker, so the fund name (a public product name) is the symbol.
//!
//! Rules (Canada): the adjusted cost base (ACB) is the average cost in CAD,
//! fees included, pooled per asset across all non-registered accounts.
//! Registered accounts (TFSA, RRSP, FHSA, RRIF, RESP, LIRA, LIF, RDSP, and
//! a workplace DPSP or pension plan) are tracked per account type, without
//! taxable gains. Costs that the files
//! cannot show are reported as "cost unknown", never guessed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;
use std::path::Path;

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use crate::config::Config;
use crate::export::{decode, file_name, header_key, is_ofx_path, parse_date};
use crate::money::fmt_cents;

pub const KRAKEN: &str = "kraken-ledger-csv";
pub const WEALTHSIMPLE: &str = "wealthsimple-activity-csv";
pub const ETRADE: &str = "etrade-transactions-csv";
pub const ETRADE_PLAN: &str = "etrade-stockplan-csv";
pub const CRYPTO: &str = "crypto-generic-csv";
pub const ETRADE_GL: &str = "etrade-gains-losses";
pub const ETRADE_RS: &str = "etrade-benefit-rs";
pub const ETRADE_ESPP: &str = "etrade-benefit-espp";
pub const CASA: &str = "casa-csv";
pub const SUNLIFE: &str = "sunlife-transactions-csv";
/// Date formats in E*TRADE stock plan files (and .xlsx dates).
const PLAN_DATES: &[&str] = &["%m/%d/%Y", "%d-%b-%Y", "%Y-%m-%d", "%m/%d/%y"];
pub const NON_REGISTERED: &str = "non-registered";

const REGISTERED: &[(&str, &str)] = &[
    ("tfsa", "TFSA"),
    ("rrsp", "RRSP"),
    ("fhsa", "FHSA"),
    ("rrif", "RRIF"),
    ("resp", "RESP"),
    ("lira", "LIRA"),
    ("lif", "LIF"),
    ("rdsp", "RDSP"),
];
const FIAT: &[&str] = &["CAD", "USD", "EUR", "GBP", "AUD", "CHF", "JPY", "NZD"];
const SUPERFICIAL_DAYS: i64 = 30;
const EPS: f64 = 1e-9;

const NEED_DEPOSIT: &str = "deposit with unknown cost";
const NEED_REWARD: &str = "reward without a CAD value";
const NEED_PURCHASE: &str = "purchase without a CAD value";
const NEED_SWAP: &str = "crypto-to-crypto trade without a CAD value (cost carried over)";
const NEED_OVERSOLD: &str = "sold more than the files show bought";
const NEED_SENT: &str = "sent coins the files never show arriving";

/// The account bucket for an account type: a registered plan, or one
/// pooled non-registered bucket.
pub fn bucket_of(account_type: &str) -> &'static str {
    let lower = account_type.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    REGISTERED
        .iter()
        .find(|(key, _)| words.contains(key))
        .map_or(NON_REGISTERED, |(_, name)| name)
}

fn is_fiat(asset: &str) -> bool {
    FIAT.contains(&asset)
}

// ---------------------------------------------------------------- reports

#[derive(Serialize, Default)]
pub struct FileReport {
    pub name: String,
    pub format: String,
    /// Data rows, blank rows not counted.
    pub rows: usize,
    pub used_rows: usize,
    /// Cash moves, currency conversions and summary rows.
    pub ignored_rows: usize,
    pub unhandled_rows: usize,
    /// Vendor transaction types that were not handled, with counts.
    pub unhandled_types: BTreeMap<String, usize>,
    /// Line numbers of rows that could not be read. Never their content.
    pub bad_lines: Vec<usize>,
    pub error: Option<String>,
    /// Shares the vendor says are sellable now, per symbol (E*TRADE
    /// Benefit History). Compared with the computed holding.
    pub reported_sellable: BTreeMap<String, f64>,
    /// New money paid into a registered plan, one entry per row. Summed into
    /// `Holdings::contributions`; never serialized per row.
    #[serde(skip)]
    pub contrib: Vec<Contrib>,
}

/// One contribution row: date, account bucket, "member" or "employer", CAD
/// (negative for a reversal).
pub struct Contrib {
    pub date: NaiveDate,
    pub bucket: &'static str,
    pub source: &'static str,
    pub cad: f64,
}

/// Contributions summed per account bucket, deduction year, month and
/// source. For an RRSP, the first 60 days of a year count for the year
/// before (the CRA contribution window); other plans use the calendar year.
#[derive(Serialize)]
pub struct ContributionSum {
    pub account_bucket: String,
    pub deduction_year: i32,
    /// "YYYY-MM".
    pub month: String,
    /// "member" (you) or "employer".
    pub source: String,
    pub cad: f64,
    pub rows: usize,
}

/// The year whose RRSP deduction limit a contribution on `date` uses.
pub fn deduction_year(bucket: &str, date: NaiveDate) -> i32 {
    if bucket == "RRSP" && date.ordinal() <= 60 {
        date.year() - 1
    } else {
        date.year()
    }
}

#[derive(Serialize)]
pub struct AssetHolding {
    pub symbol: String,
    pub account_bucket: String,
    pub quantity: f64,
    pub acb_cad: f64,
    pub acb_per_unit_cad: Option<f64>,
    /// True when part of the quantity has no known cost.
    pub cost_unknown: bool,
    /// Quantity sent out of the files' wallets and not seen arriving again.
    /// It still counts as held.
    pub held_outside_files: f64,
}

#[derive(Serialize)]
pub struct YearGains {
    pub year: i32,
    pub gains_cad: f64,
    /// Zero or negative.
    pub losses_cad: f64,
    pub net_cad: f64,
    pub sales: usize,
    /// Sales left out of the sums: cost or proceeds unknown.
    pub incomplete_sales: usize,
    pub net_by_asset_cad: BTreeMap<String, f64>,
}

#[derive(Serialize)]
pub struct YearIncome {
    pub year: i32,
    /// "dividend", "interest", "staking" or "other".
    pub kind: String,
    /// False for registered accounts.
    pub taxable: bool,
    pub cad: f64,
    pub rows: usize,
    pub rows_without_value: usize,
}

#[derive(Serialize)]
pub struct SuperficialFlag {
    pub asset: String,
    pub year: i32,
    /// Loss sales with a purchase of the same asset within 30 days.
    pub count: usize,
}

#[derive(Serialize)]
pub struct NeedsCost {
    pub asset: String,
    pub account_bucket: String,
    pub reason: String,
    pub rows: usize,
    pub quantity: f64,
}

#[derive(Serialize)]
pub struct Holdings {
    pub files: Vec<FileReport>,
    pub assets: Vec<AssetHolding>,
    pub realized_by_year: Vec<YearGains>,
    pub income_by_year: Vec<YearIncome>,
    pub superficial_loss_flags: Vec<SuperficialFlag>,
    pub needs_cost: Vec<NeedsCost>,
    /// Currencies without a rate under `[currencies]`.
    pub missing_rates: Vec<String>,
    /// Computed holdings compared with counts the vendor reports.
    pub checks: Vec<String>,
    pub notes: Vec<String>,
    /// Registered-plan contributions (new money in), for contribution room.
    pub contributions: Vec<ContributionSum>,
    /// Every buy and sell, oldest first: what `moneywiz` writes to the
    /// investment accounts (date, shares, price). Not in the JSON report.
    #[serde(skip)]
    pub trades: Vec<Trade>,
}

/// One buy (shares above zero) or sell (below zero).
#[derive(Clone, Debug)]
pub struct Trade {
    pub date: NaiveDate,
    pub symbol: String,
    /// The account type: TFSA, RRSP, FHSA, non-registered and so on.
    pub bucket: &'static str,
    /// The ledger format the row came from (wealthsimple-activity-csv, ...).
    pub format: String,
    /// The broker's account id ("" when the file has none). Private.
    pub account: String,
    pub shares: f64,
    /// Cost of a buy or proceeds of a sell, in CAD, when the file says.
    pub cad: Option<f64>,
    /// The cash a buy or sell moved: Some("") for the account's own
    /// currency, Some("USD") for another. None for coins that moved in, out
    /// or swapped for another coin: no cash moved.
    pub cash: Option<String>,
}

// ---------------------------------------------------------------- events

#[derive(Debug)]
enum Action {
    Buy {
        qty: f64,
        cost: Option<f64>,
    },
    Sell {
        qty: f64,
        proceeds: Option<f64>,
        fees: f64,
    },
    /// Coins arriving from outside the files' wallets.
    In {
        qty: f64,
    },
    /// Coins sent to a wallet outside the files. Still owned.
    Out {
        qty: f64,
    },
    /// Coins paid as a fee.
    Fee {
        qty: f64,
    },
    Income {
        qty: f64,
        cad: Option<f64>,
        kind: &'static str,
    },
    Swap {
        to: String,
        qty: f64,
        qty_to: f64,
    },
}

struct Ev {
    date: NaiveDate,
    line: usize,
    asset: String,
    bucket: &'static str,
    action: Action,
    /// The broker's account id, when the file names one (Wealthsimple).
    acct: String,
    /// The cash that paid a buy or took a sell, when the file names it
    /// (Kraken: CAD, USD, EUR). None: the account's own currency.
    paid: Option<String>,
}

struct Event {
    acct: String,
    date: NaiveDate,
    rank: u8,
    file: usize,
    seq: usize,
    asset: String,
    bucket: &'static str,
    action: Action,
    paid: Option<String>,
}

/// Converts amounts to CAD with the `[currencies]` rates.
struct Fx {
    base: String,
    rates: BTreeMap<String, f64>,
    missing: BTreeSet<String>,
}

impl Fx {
    fn new(cfg: &Config) -> Fx {
        let base = cfg
            .currencies
            .base
            .clone()
            .unwrap_or_else(|| "CAD".into())
            .to_uppercase();
        let rates = cfg
            .currencies
            .rates
            .iter()
            .map(|(k, v)| (k.to_uppercase(), *v))
            .collect();
        Fx {
            base,
            rates,
            missing: BTreeSet::new(),
        }
    }

    fn rate(&mut self, cur: &str) -> Option<f64> {
        if cur == self.base {
            return Some(1.0);
        }
        let r = self.rates.get(cur).copied().filter(|r| *r > 0.0);
        if r.is_none() {
            self.missing.insert(cur.to_string());
        }
        r
    }

    fn cad(&mut self, amount: f64, cur: &str) -> Option<f64> {
        let cur = if cur.is_empty() { "CAD" } else { cur };
        if cur == "CAD" {
            return Some(amount);
        }
        Some(amount * self.rate(cur)? / self.rate("CAD")?)
    }
}

// ---------------------------------------------------------------- reading

struct Header {
    label: &'static str,
    /// 1-based line of the header row.
    line: usize,
    offset: usize,
    delimiter: u8,
}

fn guess_delimiter(line: &str) -> u8 {
    b",;\t"
        .iter()
        .copied()
        .max_by_key(|d| (line.matches(*d as char).count(), *d == b','))
        .unwrap_or(b',')
}

fn split(line: &str, delimiter: u8) -> Vec<String> {
    csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(line.as_bytes())
        .records()
        .next()
        .and_then(Result::ok)
        .map(|r| r.iter().map(header_key).collect())
        .unwrap_or_default()
}

fn layout(keys: &[String]) -> Option<&'static str> {
    let has = |names: &[&str]| names.iter().all(|n| keys.iter().any(|k| k == n));
    if has(&["txid", "refid", "time", "type", "asset", "amount", "fee"]) {
        Some(KRAKEN)
    } else if has(&["account_type", "activity_type", "symbol", "quantity"])
        && (has(&["transaction_date"]) || has(&["effective_date"]))
    {
        Some(WEALTHSIMPLE)
    } else if has(&[
        "record type",
        "symbol",
        "quantity",
        "date sold",
        "total proceeds",
    ]) {
        Some(ETRADE_GL)
    } else if has(&["record type", "symbol", "vest date", "released qty"]) {
        Some(ETRADE_RS)
    } else if has(&[
        "record type",
        "symbol",
        "purchase date",
        "net shares",
        "purchase date fmv",
    ]) {
        Some(ETRADE_ESPP)
    } else if has(&["timestamp", "coin type", "amount", "casa transaction id"]) {
        Some(CASA)
    } else if has(&[
        "transactiondate",
        "transactiontype",
        "symbol",
        "quantity",
        "amount",
    ]) {
        Some(ETRADE)
    } else if has(&["record type", "symbol", "vest date", "vest date fmv"])
        || has(&["record type", "symbol", "qty", "grant number"])
    {
        Some(ETRADE_PLAN)
    } else if SunLifeCols::from_keys(keys).is_some() {
        Some(SUNLIFE)
    } else if CryptoCols::from_keys(keys).is_some() {
        Some(CRYPTO)
    } else {
        None
    }
}

fn find_header(text: &str) -> Option<Header> {
    let mut offset = 0;
    for (i, line) in text.split_inclusive('\n').enumerate().take(30) {
        if !line.trim().is_empty() {
            let delimiter = guess_delimiter(line);
            if let Some(label) = layout(&split(line, delimiter)) {
                return Some(Header {
                    label,
                    line: i + 1,
                    offset,
                    delimiter,
                });
            }
        }
        offset += line.len();
    }
    None
}

pub fn is_xlsx_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("xlsx"))
}

/// Every worksheet as (sheet name, CSV text).
fn xlsx_sheets(path: &Path) -> Result<Vec<(String, String)>, String> {
    use calamine::{Data, Reader, open_workbook_auto};
    let fail = |e: &dyn std::fmt::Display| format!("cannot read {}: {e}", file_name(path));
    let mut book = open_workbook_auto(path).map_err(|e| fail(&e))?;
    let mut sheets = Vec::new();
    for name in book.sheet_names() {
        let Ok(range) = book.worksheet_range(&name) else {
            continue;
        };
        let mut w = csv::WriterBuilder::new()
            .flexible(true)
            .from_writer(Vec::new());
        for row in range.rows() {
            w.write_record(row.iter().map(|c| {
                match c {
                    Data::DateTime(d) => d
                        .as_datetime()
                        .map(|t| t.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                    Data::Error(_) => String::new(),
                    other => other.to_string(),
                }
            }))
            .map_err(|e| fail(&e))?;
        }
        let bytes = w.into_inner().map_err(|e| fail(&e))?;
        let text = String::from_utf8(bytes).map_err(|e| fail(&e))?;
        sheets.push((name, text));
    }
    Ok(sheets)
}

// ---------------------------------------------------------------- columns

/// How many lines of each file or sheet `columns` describes.
const COLUMN_LINES: usize = 12;

/// The layout of a file without its values, to fix a parser: header-like
/// lines print their cell text; every other line prints one shape per cell
/// (`date`, `num`, `empty`, or text masked to `a`/`9` runs, such as `a-9-9`).
pub fn columns(path: &Path) -> String {
    if crate::pdf::is_pdf_path(path) {
        return crate::pdf::columns(path);
    }
    let name = file_name(path);
    let parts = if is_xlsx_path(path) {
        xlsx_sheets(path).map(|s| {
            s.into_iter()
                .map(|(sheet, t)| (format!("sheet {}", header_text(&sheet)), t))
                .collect()
        })
    } else {
        read_text(path).map(|t| vec![(String::new(), t)])
    };
    let parts = match parts {
        Ok(p) => p,
        Err(e) => return format!("- {name}: {e}\n"),
    };
    let mut out = String::new();
    for (label, text) in parts {
        let format = find_header(&text).map_or("not recognized", |h| h.label);
        let sep = if label.is_empty() { "" } else { ", " };
        let _ = writeln!(out, "- {name}{sep}{label} ({format}):");
        let lines: Vec<(usize, Vec<String>)> = text
            .lines()
            .enumerate()
            .take(COLUMN_LINES + 1)
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| (i + 1, raw_cells(l, guess_delimiter(l))))
            .collect();
        let mut header_seen = false;
        for (k, (line, cells)) in lines.iter().enumerate() {
            if *line > COLUMN_LINES {
                break;
            }
            let next_len = lines.get(k + 1).map(|(_, c)| c.len());
            let (shown, header) = shape(cells, !header_seen && next_len == Some(cells.len()));
            header_seen |= header;
            let _ = writeln!(out, "  line {line}: {} cells | {shown}", cells.len());
        }
    }
    out
}

fn raw_cells(line: &str, delimiter: u8) -> Vec<String> {
    csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(line.as_bytes())
        .records()
        .next()
        .and_then(Result::ok)
        .map(|r| r.iter().map(|c| c.trim().to_string()).collect())
        .unwrap_or_default()
}

/// A header name: starts with a letter or digit, holds a letter, has at
/// most 2 digits (so ids never pass), and uses only letters, digits and
/// common punctuation. Not a number or a date.
fn is_header_word(cell: &str) -> bool {
    cell.len() <= 64
        && cell
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && cell.chars().any(|c| c.is_ascii_alphabetic())
        && cell.chars().filter(char::is_ascii_digit).count() <= 2
        && cell
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " ()/_.#%&*'=-?:$".contains(c))
        && !looks_like_date(cell)
}

fn header_text(cell: &str) -> String {
    if is_header_word(cell) {
        cell.to_string()
    } else {
        mask(cell)
    }
}

/// The first header line of a block (3+ cells, all names, and the next
/// line has as many cells) prints its text. Other lines print shapes.
fn shape(cells: &[String], may_be_header: bool) -> (String, bool) {
    let filled: Vec<&String> = cells.iter().filter(|c| !c.is_empty()).collect();
    let header = may_be_header && filled.len() >= 3 && filled.iter().all(|c| is_header_word(c));
    let text = cells
        .iter()
        .map(|c| {
            if header {
                c.clone()
            } else if c.is_empty() {
                "empty".into()
            } else if num(c).is_some()
                && !c.chars().any(|ch| ch.is_alphabetic())
                && !looks_like_date(c)
            {
                "num".into()
            } else if looks_like_date(c) {
                "date".into()
            } else {
                mask(c)
            }
        })
        .collect::<Vec<_>>()
        .join(" | ");
    (text, header)
}

fn looks_like_date(c: &str) -> bool {
    const FORMATS: &[&str] = &[
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%d/%m/%Y",
        "%m/%d/%y",
        "%d-%b-%Y",
        "%b %d, %Y",
        "%Y/%m/%d",
    ];
    let head = c.split(['T', ' ']).next().unwrap_or(c);
    FORMATS
        .iter()
        .any(|f| parse_date(c, f).is_some() || parse_date(head, f).is_some())
}

/// Letters become `a`, digits `9`, runs collapse: `Mar-15-2025` → `a-9-9`.
fn mask(c: &str) -> String {
    let mut out = String::new();
    for ch in c.chars().take(40) {
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

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", file_name(path)))?;
    let text = decode(&bytes).0.replace("\r\n", "\n").replace('\r', "\n");
    Ok(etrade_json_csv(&text).unwrap_or(text))
}

/// A page of the E*TRADE transactions API (`api download etrade`) as the
/// E*TRADE transactions CSV, so one reader serves both. Descriptions and
/// account ids are left out. `None` for any other text.
fn etrade_json_csv(text: &str) -> Option<String> {
    if !text.trim_start().starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let list = v.get("TransactionListResponse")?;
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record([
        "TransactionDate",
        "TransactionType",
        "SecurityType",
        "Symbol",
        "Quantity",
        "Amount",
        "Price",
        "Commission",
        "Description",
    ])
    .ok()?;
    let empty = Vec::new();
    let txns = list
        .get("Transaction")
        .and_then(|t| t.as_array())
        .unwrap_or(&empty);
    let num = |v: Option<&serde_json::Value>| {
        v.and_then(serde_json::Value::as_f64)
            .map_or_else(String::new, |n| n.to_string())
    };
    for t in txns {
        let b = t.get("brokerage");
        let product = b.and_then(|b| b.get("product"));
        let s = |v: Option<&serde_json::Value>, k: &str| {
            v.and_then(|v| v.get(k))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string()
        };
        let date = t
            .get("transactionDate")
            .and_then(serde_json::Value::as_i64)
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map_or_else(String::new, |d| d.format("%m/%d/%Y").to_string());
        w.write_record([
            date,
            s(Some(t), "transactionType"),
            s(product, "securityType"),
            s(product, "symbol"),
            num(b.and_then(|b| b.get("quantity"))),
            num(t.get("amount")),
            num(b.and_then(|b| b.get("price"))),
            num(b.and_then(|b| b.get("fee"))),
            String::new(),
        ])
        .ok()?;
    }
    String::from_utf8(w.into_inner().ok()?).ok()
}

/// The ledger format of a file, or `None` for cash exports and OFX.
pub fn sniff(path: &Path) -> Option<&'static str> {
    if is_ofx_path(path) {
        return None;
    }
    if is_xlsx_path(path) {
        return xlsx_sheets(path)
            .ok()?
            .iter()
            .find_map(|(_, t)| find_header(t).map(|h| h.label));
    }
    find_header(&read_text(path).ok()?).map(|h| h.label)
}

struct Rows {
    keys: Vec<String>,
    records: Vec<(usize, csv::StringRecord)>,
}

impl Rows {
    fn read(text: &str, h: &Header, report: &mut FileReport) -> Rows {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(h.delimiter)
            .flexible(true)
            .has_headers(true)
            .from_reader(&text.as_bytes()[h.offset..]);
        let keys = reader
            .headers()
            .map(|r| r.iter().map(header_key).collect())
            .unwrap_or_default();
        let line = |p: Option<&csv::Position>| p.map_or(0, |p| h.line + p.line() as usize - 1);
        let mut records = Vec::new();
        for result in reader.records() {
            match result {
                Ok(r) if r.iter().all(|f| f.trim().is_empty()) => {}
                Ok(r) => records.push((line(r.position()), r)),
                Err(e) => report.bad_lines.push(line(e.position())),
            }
        }
        report.rows = records.len() + report.bad_lines.len();
        Rows { keys, records }
    }

    /// The last column with this name. Benefit History repeats names: the
    /// first copy is the grant summary, the last the vest schedule.
    fn col_last(&self, name: &str) -> Option<usize> {
        self.keys.iter().rposition(|k| k == name)
    }

    fn col(&self, names: &[&str]) -> Option<usize> {
        names
            .iter()
            .find_map(|n| self.keys.iter().position(|k| k == n))
    }
}

fn get(r: &csv::StringRecord, i: Option<usize>) -> &str {
    i.and_then(|i| r.get(i)).map_or("", str::trim)
}

/// Parses `1,234.56`, `$140.00`, `-0.5` or `(12.00)`. Not for comma decimals.
fn num(raw: &str) -> Option<f64> {
    let t = raw.trim();
    let cleaned: String = t
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-'))
        .collect();
    if !cleaned.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: f64 = cleaned.parse().ok()?;
    Some(if t.starts_with('(') && v > 0.0 { -v } else { v })
}

fn date_of(raw: &str, formats: &[&str]) -> Option<NaiveDate> {
    formats.iter().find_map(|f| parse_date(raw, f))
}

fn kraken_asset(raw: &str) -> String {
    let mut a = raw.trim().trim_matches('"').to_uppercase();
    if let Some(dot) = a.find('.') {
        a.truncate(dot);
    }
    const PREFIXED: &[&str] = &[
        "XXBT", "XETH", "XLTC", "XXRP", "XXLM", "XXMR", "XZEC", "XETC", "XREP", "XMLN", "XXDG",
        "ZCAD", "ZUSD", "ZEUR", "ZGBP", "ZJPY", "ZAUD", "ZCHF",
    ];
    if PREFIXED.contains(&a.as_str()) {
        a.remove(0);
    }
    match a.as_str() {
        "XBT" => "BTC".into(),
        "XDG" => "DOGE".into(),
        "ETH2" => "ETH".into(),
        _ => a,
    }
}

fn kraken(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    struct K {
        line: usize,
        refid: String,
        date: NaiveDate,
        kind: String,
        sub: String,
        asset: String,
        amount: f64,
        fee: f64,
    }
    let c = |n: &str| rows.col(&[n]);
    let (refid, time, kind, sub, asset, amount, fee) = (
        c("refid"),
        c("time"),
        c("type"),
        c("subtype"),
        c("asset"),
        c("amount"),
        c("fee"),
    );
    let mut ks = Vec::new();
    for (line, r) in &rows.records {
        let (Some(date), Some(amount)) =
            (date_of(get(r, time), &["%Y-%m-%d"]), num(get(r, amount)))
        else {
            rep.bad_lines.push(*line);
            continue;
        };
        ks.push(K {
            line: *line,
            refid: get(r, refid).to_string(),
            date,
            kind: get(r, kind).to_lowercase(),
            sub: get(r, sub).to_lowercase(),
            asset: kraken_asset(get(r, asset)),
            amount,
            fee: num(get(r, fee)).unwrap_or(0.0).abs(),
        });
    }
    let mut out = Vec::new();
    let mut groups: Vec<Vec<&K>> = Vec::new();
    let mut group_of: HashMap<&str, usize> = HashMap::new();
    let ev = |k: &K, asset: &str, action| Ev {
        acct: String::new(),
        paid: None,
        date: k.date,
        line: k.line,
        asset: asset.to_string(),
        bucket: NON_REGISTERED,
        action,
    };
    for k in &ks {
        let crypto = !is_fiat(&k.asset);
        match k.kind.as_str() {
            "trade" | "spend" | "receive" => {
                let g = *group_of.entry(k.refid.as_str()).or_insert_with(|| {
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[g].push(k);
                continue;
            }
            "deposit" if crypto => out.push(ev(
                k,
                &k.asset,
                Action::In {
                    qty: k.amount.abs() - k.fee,
                },
            )),
            "withdrawal" if crypto => {
                out.push(ev(
                    k,
                    &k.asset,
                    Action::Out {
                        qty: k.amount.abs(),
                    },
                ));
                if k.fee > 0.0 {
                    out.push(ev(k, &k.asset, Action::Fee { qty: k.fee }));
                }
            }
            "staking" if crypto && k.amount > 0.0 => out.push(ev(
                k,
                &k.asset,
                Action::Income {
                    qty: k.amount - k.fee,
                    cad: None,
                    kind: "staking",
                },
            )),
            "earn" if crypto && k.sub == "reward" && k.amount > 0.0 => out.push(ev(
                k,
                &k.asset,
                Action::Income {
                    qty: k.amount - k.fee,
                    cad: None,
                    kind: "staking",
                },
            )),
            "transfer"
                if crypto
                    && !["staking", "spot", "allocation", "migration"]
                        .iter()
                        .any(|s| k.sub.contains(s)) =>
            {
                let action = if k.amount > 0.0 {
                    Action::In { qty: k.amount }
                } else {
                    Action::Out { qty: -k.amount }
                };
                out.push(ev(k, &k.asset, action));
            }
            "deposit" | "withdrawal" | "staking" | "earn" | "transfer" => {
                rep.ignored_rows += 1;
                continue;
            }
            other => {
                *rep.unhandled_types.entry(other.to_string()).or_insert(0) += 1;
                rep.unhandled_rows += 1;
                continue;
            }
        }
        rep.used_rows += 1;
    }
    for legs in groups {
        let pair = match legs.as_slice() {
            [a, b] if a.amount < 0.0 && b.amount > 0.0 => Some((*a, *b)),
            [a, b] if b.amount < 0.0 && a.amount > 0.0 => Some((*b, *a)),
            _ => None,
        };
        let Some((out_leg, in_leg)) = pair else {
            *rep.unhandled_types
                .entry("trade (not a pair)".into())
                .or_insert(0) += legs.len();
            rep.unhandled_rows += legs.len();
            continue;
        };
        let first = if out_leg.line < in_leg.line {
            out_leg
        } else {
            in_leg
        };
        match (is_fiat(&out_leg.asset), is_fiat(&in_leg.asset)) {
            (true, true) => {
                rep.ignored_rows += 2;
                continue;
            }
            (true, false) => {
                let cost = fx.cad(-out_leg.amount + out_leg.fee, &out_leg.asset);
                let mut e = ev(
                    first,
                    &in_leg.asset,
                    Action::Buy {
                        qty: in_leg.amount - in_leg.fee,
                        cost,
                    },
                );
                e.paid = Some(out_leg.asset.clone());
                out.push(e);
            }
            (false, true) => {
                let proceeds = fx.cad(in_leg.amount, &in_leg.asset);
                let fees = fx.cad(in_leg.fee, &in_leg.asset).unwrap_or(0.0);
                let mut e = ev(
                    first,
                    &out_leg.asset,
                    Action::Sell {
                        qty: -out_leg.amount,
                        proceeds,
                        fees,
                    },
                );
                e.paid = Some(in_leg.asset.clone());
                out.push(e);
                if out_leg.fee > 0.0 {
                    out.push(ev(first, &out_leg.asset, Action::Fee { qty: out_leg.fee }));
                }
            }
            (false, false) => out.push(ev(
                first,
                &out_leg.asset,
                Action::Swap {
                    to: in_leg.asset.clone(),
                    qty: -out_leg.amount + out_leg.fee,
                    qty_to: in_leg.amount - in_leg.fee,
                },
            )),
        }
        rep.used_rows += 2;
    }
    out
}

fn wealthsimple(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let c = |n: &str| rows.col(&[n]);
    let account_id = c("account_id");
    let (date, account_type, kind, sub, direction) = (
        c("transaction_date").or(c("effective_date")),
        c("account_type"),
        c("activity_type"),
        c("activity_sub_type"),
        c("direction"),
    );
    let (symbol, currency, quantity, price, commission, net) = (
        c("symbol"),
        c("currency"),
        c("quantity"),
        c("unit_price"),
        c("commission"),
        c("net_cash_amount"),
    );
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let Some(day) = date_of(get(r, date), &["%Y-%m-%d", "%m/%d/%Y"]) else {
            rep.bad_lines.push(*line);
            continue;
        };
        let kind_raw = get(r, kind).to_lowercase();
        let side = format!("{} {} {}", get(r, sub), get(r, direction), kind_raw).to_lowercase();
        let asset = get(r, symbol).to_uppercase();
        let cur = get(r, currency).to_uppercase();
        let qty = num(get(r, quantity)).map(f64::abs);
        let price = num(get(r, price)).filter(|p| *p > 0.0);
        let comm = num(get(r, commission)).unwrap_or(0.0).abs();
        let net = num(get(r, net)).filter(|n| *n != 0.0);
        let bucket = bucket_of(get(r, account_type));
        let mut push = |asset: &str, action| {
            out.push(Ev {
                date: day,
                line: *line,
                asset: asset.to_string(),
                bucket,
                action,
                acct: get(r, account_id).to_string(),
                paid: None,
            })
        };
        if kind_raw == "trade" || kind_raw.contains("buy") || kind_raw.contains("sell") {
            let Some(qty) = qty.filter(|q| *q > 0.0 && !asset.is_empty()) else {
                rep.bad_lines.push(*line);
                continue;
            };
            if side.contains("sell") {
                let gross = price.map(|p| p * qty).or(net.map(|n| n.abs() + comm));
                let proceeds = gross.and_then(|g| fx.cad(g, &cur));
                let fees = fx.cad(comm, &cur).unwrap_or(0.0);
                push(
                    &asset,
                    Action::Sell {
                        qty,
                        proceeds,
                        fees,
                    },
                );
            } else if side.contains("buy") {
                let total = net.map(f64::abs).or(price.map(|p| p * qty + comm));
                push(
                    &asset,
                    Action::Buy {
                        qty,
                        cost: total.and_then(|t| fx.cad(t, &cur)),
                    },
                );
            } else {
                *rep.unhandled_types
                    .entry("trade (no side)".into())
                    .or_insert(0) += 1;
                rep.unhandled_rows += 1;
                continue;
            }
        } else if kind_raw.contains("dividend") || kind_raw.contains("interest") {
            let kind = if kind_raw.contains("dividend") {
                "dividend"
            } else {
                "interest"
            };
            push(
                &asset,
                Action::Income {
                    qty: 0.0,
                    cad: net.and_then(|n| fx.cad(n, &cur)),
                    kind,
                },
            );
        } else if kind_raw.contains("staking") || kind_raw.contains("reward") {
            let value = net.map(f64::abs).or(price.zip(qty).map(|(p, q)| p * q));
            push(
                &asset,
                Action::Income {
                    qty: qty.unwrap_or(0.0),
                    cad: value.and_then(|v| fx.cad(v, &cur)),
                    kind: "staking",
                },
            );
        } else if !asset.is_empty()
            && !is_fiat(&asset)
            && qty.is_some_and(|q| q > 0.0)
            && ["deposit", "withdraw", "transfer"]
                .iter()
                .any(|k| kind_raw.contains(k))
        {
            let qty = qty.unwrap_or(0.0);
            let out_bound = ["out", "withdraw", "send", "sent"]
                .iter()
                .any(|k| side.contains(k))
                || num(get(r, quantity)).is_some_and(|q| q < 0.0);
            push(
                &asset,
                if out_bound {
                    Action::Out { qty }
                } else {
                    Action::In { qty }
                },
            );
        } else if asset.is_empty() || is_fiat(&asset) {
            // Cash paid into a registered account is a contribution (room), not a holding.
            let contribution = bucket != NON_REGISTERED
                && (side.contains("contribution") || kind_raw.contains("deposit"))
                && !side.contains("transfer")
                && !side.contains("withdraw");
            let cad = net
                .filter(|n| *n > 0.0)
                .and_then(|n| fx.cad(n, if cur.is_empty() { "CAD" } else { &cur }));
            if let (true, Some(cad)) = (contribution, cad) {
                rep.contrib.push(Contrib {
                    date: day,
                    bucket,
                    source: "member",
                    cad,
                });
            }
            rep.ignored_rows += 1;
            continue;
        } else {
            *rep.unhandled_types.entry(kind_raw).or_insert(0) += 1;
            rep.unhandled_rows += 1;
            continue;
        }
        rep.used_rows += 1;
    }
    out
}

fn etrade(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let c = |n: &str| rows.col(&[n]);
    let (date, kind, symbol, quantity, amount, price, commission) = (
        c("transactiondate"),
        c("transactiontype"),
        c("symbol"),
        c("quantity"),
        c("amount"),
        c("price"),
        c("commission"),
    );
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let Some(day) = date_of(get(r, date), &["%m/%d/%Y", "%m/%d/%y", "%Y-%m-%d"]) else {
            rep.bad_lines.push(*line);
            continue;
        };
        let kind_raw = get(r, kind).to_lowercase();
        let asset = get(r, symbol).to_uppercase();
        let qty = num(get(r, quantity)).map(f64::abs).unwrap_or(0.0);
        let amount = num(get(r, amount)).unwrap_or(0.0);
        let price = num(get(r, price)).filter(|p| *p > 0.0);
        let comm = num(get(r, commission)).unwrap_or(0.0).abs();
        let action = if kind_raw.contains("bought") || kind_raw == "buy" {
            let total = if amount != 0.0 {
                Some(amount.abs())
            } else {
                price.map(|p| p * qty + comm)
            };
            Some(Action::Buy {
                qty,
                cost: total.and_then(|t| fx.cad(t, "USD")),
            })
        } else if kind_raw.contains("sold") || kind_raw == "sell" {
            let gross = price
                .map(|p| p * qty)
                .or((amount != 0.0).then(|| amount.abs() + comm));
            let fees = fx.cad(comm, "USD").unwrap_or(0.0);
            Some(Action::Sell {
                qty,
                proceeds: gross.and_then(|g| fx.cad(g, "USD")),
                fees,
            })
        } else if kind_raw.contains("dividend") || kind_raw.contains("interest") {
            let kind = if kind_raw.contains("dividend") {
                "dividend"
            } else {
                "interest"
            };
            Some(Action::Income {
                qty: 0.0,
                cad: fx.cad(amount, "USD"),
                kind,
            })
        } else {
            None
        };
        match action {
            Some(Action::Buy { qty, .. } | Action::Sell { qty, .. })
                if qty <= 0.0 || asset.is_empty() =>
            {
                rep.bad_lines.push(*line);
            }
            Some(action) => {
                out.push(Ev {
                    acct: String::new(),
                    paid: None,
                    date: day,
                    line: *line,
                    asset,
                    bucket: NON_REGISTERED,
                    action,
                });
                rep.used_rows += 1;
            }
            None if asset.is_empty() => rep.ignored_rows += 1,
            None => {
                *rep.unhandled_types.entry(kind_raw).or_insert(0) += 1;
                rep.unhandled_rows += 1;
            }
        }
    }
    out
}

/// Each vested lot is an acquisition at the vest-date fair market value,
/// which is its ACB under Canadian rules.
fn etrade_plan(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let (record, symbol) = (rows.col(&["record type"]), rows.col(&["symbol"]));
    let (qty_col, vest, acquired, fmv) = (
        rows.col(&["qty", "quantity"]),
        rows.col(&["vest date"]),
        rows.col(&["date acquired"]),
        rows.col(&["vest date fmv"]),
    );
    const FORMATS: &[&str] = &["%m/%d/%Y", "%d-%b-%Y", "%Y-%m-%d", "%m/%d/%y"];
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let kind = get(r, record).to_lowercase();
        let asset = get(r, symbol).to_uppercase();
        let qty = num(get(r, qty_col)).filter(|q| *q > 0.0);
        let date_text = if get(r, vest).is_empty() {
            get(r, acquired)
        } else {
            get(r, vest)
        };
        if kind.contains("total")
            || kind.contains("summary")
            || asset.is_empty()
            || qty.is_none()
            || date_text.is_empty()
        {
            rep.ignored_rows += 1;
            continue;
        }
        let (Some(day), Some(qty)) = (date_of(date_text, FORMATS), qty) else {
            rep.bad_lines.push(*line);
            continue;
        };
        let cost = num(get(r, fmv))
            .filter(|f| *f > 0.0)
            .and_then(|f| fx.cad(f * qty, "USD"));
        out.push(Ev {
            paid: None,
            acct: String::new(),
            date: day,
            line: *line,
            asset,
            bucket: NON_REGISTERED,
            action: Action::Buy { qty, cost },
        });
        rep.used_rows += 1;
    }
    out
}

/// Column roles for a generic crypto wallet or exchange export.
struct CryptoCols {
    date: usize,
    kind: usize,
    asset: Option<usize>,
    /// Asset named in a header such as `Amount (BTC)`.
    header_asset: Option<String>,
    qty: usize,
    value: Option<usize>,
    value_cur: Option<String>,
    /// Per-row fiat currency of the value, when the asset has its own column.
    value_cur_col: Option<usize>,
    fee: Option<usize>,
    fee_cur: Option<String>,
    account_type: Option<usize>,
}

/// `value (cad)` → `CAD`.
fn suffix_currency(key: &str) -> Option<String> {
    let open = key.find('(')?;
    let close = key[open..].find(')')? + open;
    let inner = key[open + 1..close].trim();
    (!inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| inner.to_uppercase())
}

impl CryptoCols {
    fn from_keys(keys: &[String]) -> Option<CryptoCols> {
        if keys
            .iter()
            .any(|k| matches!(k.as_str(), "gross" | "net" | "balance impact"))
        {
            return None;
        }
        let exact = |names: &[&str]| names.iter().find_map(|n| keys.iter().position(|k| k == n));
        let prefix = |p: &str| keys.iter().position(|k| k.starts_with(p));
        let date = exact(&[
            "date",
            "timestamp",
            "time",
            "datetime",
            "date (utc)",
            "timestamp (utc)",
            "created at",
            "created_at",
            "transaction date",
            "date/time",
        ])
        .or_else(|| prefix("date"))?;
        let kind = exact(&[
            "type",
            "transaction type",
            "transaction_type",
            "kind",
            "side",
            "action",
            "operation",
        ])?;
        let asset = exact(&[
            "asset",
            "coin",
            "symbol",
            "ticker",
            "crypto",
            "cryptocurrency",
            "asset symbol",
            "asset_symbol",
        ]);
        let strict_qty = exact(&["quantity", "qty", "quantity transacted"])
            .or_else(|| prefix("quantity ("))
            .or_else(|| prefix("qty ("));
        let qty = strict_qty
            .or_else(|| exact(&["amount"]))
            .or_else(|| prefix("amount ("))?;
        let header_asset = suffix_currency(&keys[qty]).filter(|c| !is_fiat(c));
        let currency = exact(&["currency", "currency code"]);
        let (asset, value_cur_col) = match (asset, currency) {
            (Some(a), cur) => (Some(a), cur),
            (None, Some(cur)) if strict_qty.is_some() => (Some(cur), None),
            (None, _) if header_asset.is_some() => (None, None),
            _ => return None,
        };
        let value = exact(&[
            "cad_value",
            "cad value",
            "value_cad",
            "cad total",
            "value",
            "total",
            "subtotal",
            "cost",
            "native amount",
        ])
        .or_else(|| prefix("value ("))
        .or_else(|| prefix("total ("))
        .or_else(|| prefix("native amount ("));
        let value_cur = value.and_then(|v| {
            suffix_currency(&keys[v]).or_else(|| keys[v].contains("cad").then(|| "CAD".to_string()))
        });
        let fee = exact(&["fee", "fees", "network fee"]).or_else(|| prefix("fee"));
        let fee_cur = fee.and_then(|f| suffix_currency(&keys[f]));
        let account_type = exact(&["account_type", "account type"]);
        Some(CryptoCols {
            date,
            kind,
            asset,
            header_asset,
            qty,
            value,
            value_cur,
            value_cur_col,
            fee,
            fee_cur,
            account_type,
        })
    }
}

/// E*TRADE Gains & Losses: one row per sold lot. The buys come from
/// Benefit History (vests and ESPP purchases), so these rows are sales.
fn etrade_gl(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let c = |n: &str| rows.col(&[n]);
    let (record, symbol, quantity, sold, proceeds) = (
        c("record type"),
        c("symbol"),
        c("quantity"),
        c("date sold"),
        c("total proceeds"),
    );
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let kind = get(r, record).to_lowercase();
        if kind.contains("summary") || kind.contains("total") {
            rep.ignored_rows += 1;
            continue;
        }
        let asset = get(r, symbol).to_uppercase();
        let qty = num(get(r, quantity)).map(f64::abs).filter(|q| *q > 0.0);
        let (Some(day), Some(qty), false) =
            (date_of(get(r, sold), PLAN_DATES), qty, asset.is_empty())
        else {
            rep.bad_lines.push(*line);
            continue;
        };
        let proceeds = num(get(r, proceeds)).and_then(|p| fx.cad(p.abs(), "USD"));
        out.push(Ev {
            paid: None,
            acct: String::new(),
            date: day,
            line: *line,
            asset,
            bucket: NON_REGISTERED,
            action: Action::Sell {
                qty,
                proceeds,
                fees: 0.0,
            },
        });
        rep.used_rows += 1;
    }
    out
}

/// E*TRADE Benefit History, Restricted Stock sheet. A "Shares released"
/// event is a buy of the net shares (after the shares withheld for tax).
/// Its cost is the vest-date value per share, from the vest row of the
/// same grant and date: the released amount / released qty, else the
/// taxable gain of that vest period / vested qty. Grant rows give the
/// sellable count E*TRADE reports. Only grant rows name the symbol, so
/// it carries over to the rows below.
fn etrade_rs(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let c = |n: &str| rows.col(&[n]);
    let (record, symbol, sellable, grant) = (
        c("record type"),
        c("symbol"),
        c("sellable qty."),
        c("grant number"),
    );
    let (event_date, event_type, event_qty) = (c("date"), c("event type"), c("qty. or amount"));
    let (period, vest_date, vested, released, amount, gain) = (
        rows.col_last("vest period"),
        rows.col_last("vest date"),
        rows.col_last("vested qty."),
        rows.col_last("released qty"),
        rows.col_last("released amount"),
        rows.col_last("taxable gain"),
    );
    let key = |r: &csv::StringRecord, col| (get(r, grant).to_string(), get(r, col).to_string());
    let mut gains: HashMap<(String, String), f64> = HashMap::new();
    for (_, r) in &rows.records {
        if let Some(g) = num(get(r, gain)).filter(|g| *g > 0.0) {
            *gains.entry(key(r, period)).or_default() += g;
        }
    }
    // USD value per share of each vest, by (grant, vest date).
    let mut per_share: HashMap<(String, NaiveDate), f64> = HashMap::new();
    for (_, r) in &rows.records {
        let Some(day) = date_of(get(r, vest_date), PLAN_DATES) else {
            continue;
        };
        let by_amount = num(get(r, amount))
            .filter(|a| *a > 0.0)
            .zip(num(get(r, released)).filter(|q| *q > 0.0))
            .map(|(a, q)| a / q);
        let by_gain = num(get(r, vested))
            .filter(|v| *v > 0.0)
            .zip(gains.get(&key(r, period)))
            .map(|(v, g)| g / v);
        if let Some(p) = by_amount.or(by_gain) {
            per_share.insert((get(r, grant).to_string(), day), p);
        }
    }
    let mut current = String::new();
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let kind = get(r, record).to_lowercase();
        let sym = get(r, symbol).to_uppercase();
        if !sym.is_empty() {
            current = sym;
        }
        if kind == "grant" {
            let q = num(get(r, sellable)).unwrap_or(0.0).max(0.0);
            *rep.reported_sellable.entry(current.clone()).or_default() += q;
            rep.ignored_rows += 1;
            continue;
        }
        if kind != "event" || !get(r, event_type).to_lowercase().contains("released") {
            rep.ignored_rows += 1;
            continue;
        }
        let day = date_of(get(r, event_date), PLAN_DATES);
        let qty = num(get(r, event_qty)).map(f64::abs).filter(|q| *q > 0.0);
        let (Some(day), Some(qty), false) = (day, qty, current.is_empty()) else {
            rep.bad_lines.push(*line);
            continue;
        };
        let cost = per_share
            .get(&(get(r, grant).to_string(), day))
            .and_then(|p| fx.cad(p * qty, "USD"));
        out.push(Ev {
            paid: None,
            acct: String::new(),
            date: day,
            line: *line,
            asset: current.clone(),
            bucket: NON_REGISTERED,
            action: Action::Buy { qty, cost },
        });
        rep.used_rows += 1;
    }
    out
}

/// E*TRADE Benefit History, ESPP sheet. A purchase row is a buy of the net
/// shares at the purchase-date market value: in Canada the discount is
/// employment income, so the ACB is the market value.
fn etrade_espp(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let c = |n: &str| rows.col(&[n]);
    let (record, symbol, date, net, purchased, fmv, sellable) = (
        c("record type"),
        c("symbol"),
        c("purchase date"),
        c("net shares"),
        c("purchased qty."),
        c("purchase date fmv"),
        c("sellable qty."),
    );
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let asset = get(r, symbol).to_uppercase();
        let day = date_of(get(r, date), PLAN_DATES);
        let qty = num(get(r, net))
            .or(num(get(r, purchased)))
            .filter(|q| *q > 0.0);
        let (Some(day), Some(qty), false) = (day, qty, asset.is_empty()) else {
            rep.ignored_rows += 1;
            continue;
        };
        if get(r, record).to_lowercase().contains("summary") {
            rep.ignored_rows += 1;
            continue;
        }
        let q = num(get(r, sellable)).unwrap_or(0.0).max(0.0);
        *rep.reported_sellable.entry(asset.clone()).or_default() += q;
        let cost = num(get(r, fmv))
            .filter(|f| *f > 0.0)
            .and_then(|f| fx.cad(f * qty, "USD"));
        out.push(Ev {
            paid: None,
            acct: String::new(),
            date: day,
            line: *line,
            asset,
            bucket: NON_REGISTERED,
            action: Action::Buy { qty, cost },
        });
        rep.used_rows += 1;
    }
    out
}

/// Casa vault history. `Amount` carries its unit (`-0.01 BTC`): positive
/// coins arrive, negative coins leave to another wallet (still owned).
/// `Fees` on a send is coins paid as a fee. There is no CAD value.
fn casa(rows: &Rows, rep: &mut FileReport) -> Vec<Ev> {
    let c = |n: &str| rows.col(&[n]);
    let (time, coin, amount, fees, status) = (
        c("timestamp"),
        c("coin type"),
        c("amount"),
        c("fees"),
        c("status"),
    );
    const FORMATS: &[&str] = &["%Y-%m-%d", "%m/%d/%Y", "%Y/%m/%d", "%d/%m/%Y"];
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let state = get(r, status).to_lowercase();
        if ["fail", "cancel", "reject", "pending"]
            .iter()
            .any(|s| state.contains(s))
        {
            rep.ignored_rows += 1;
            continue;
        }
        let raw = get(r, time);
        let head = raw.split(['T', ' ']).next().unwrap_or(raw);
        let unit = get(r, amount)
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_uppercase();
        let mut asset = get(r, coin).to_uppercase();
        if asset.is_empty() || asset == "BITCOIN" {
            asset = if unit.is_empty() { "BTC".into() } else { unit };
        }
        let (Some(day), Some(qty)) = (
            date_of(head, FORMATS),
            num(get(r, amount)).filter(|q| *q != 0.0),
        ) else {
            rep.bad_lines.push(*line);
            continue;
        };
        let fee = num(get(r, fees)).map_or(0.0, f64::abs);
        let mut push = |action| {
            out.push(Ev {
                paid: None,
                acct: String::new(),
                date: day,
                line: *line,
                asset: asset.clone(),
                bucket: NON_REGISTERED,
                action,
            })
        };
        if qty > 0.0 {
            push(Action::In { qty });
        } else {
            push(Action::Out { qty: -qty });
            if fee > 0.0 {
                push(Action::Fee { qty: fee });
            }
        }
        rep.used_rows += 1;
    }
    out
}

/// Column roles in a Sun Life group plan transaction history. Sun Life does
/// not publish a download layout, so these are the usual group-plan names
/// (an estimate): date, transaction type, fund, units, and a unit price or
/// an amount. `plan` is optional; without it the file is a group RRSP.
struct SunLifeCols {
    date: usize,
    kind: usize,
    fund: usize,
    units: usize,
    price: Option<usize>,
    amount: Option<usize>,
    plan: Option<usize>,
}

impl SunLifeCols {
    fn from_keys(keys: &[String]) -> Option<SunLifeCols> {
        let find = |names: &[&str]| names.iter().find_map(|n| keys.iter().position(|k| k == n));
        let price = find(&[
            "unit price",
            "unit value",
            "unit price ($)",
            "unit value ($)",
            "price per unit",
            "price",
        ]);
        let amount = find(&[
            "amount",
            "amount ($)",
            "amount (cad)",
            "dollar amount",
            "transaction amount",
            "gross amount",
        ]);
        if price.is_none() && amount.is_none() {
            return None;
        }
        Some(SunLifeCols {
            date: find(&[
                "date",
                "transaction date",
                "effective date",
                "trade date",
                "process date",
                "processed date",
                "valuation date",
            ])?,
            kind: find(&[
                "transaction type",
                "transaction",
                "type",
                "activity",
                "transaction description",
                "description",
            ])?,
            fund: find(&[
                "fund",
                "fund name",
                "investment option",
                "investment option name",
                "investment",
                "investment name",
                "fund code",
            ])?,
            units: find(&[
                "units",
                "number of units",
                "units transacted",
                "unit quantity",
            ])?,
            price,
            amount,
            plan: find(&[
                "plan",
                "plan type",
                "plan name",
                "product",
                "product type",
                "product name",
                "account type",
            ]),
        })
    }
}

/// Date formats seen on Sun Life pages. Day-first dates are read only when
/// month-first cannot fit (the day is above 12).
const SUNLIFE_DATES: &[&str] = &[
    "%Y-%m-%d",
    "%b %d, %Y",
    "%B %d, %Y",
    "%d-%b-%Y",
    "%m/%d/%Y",
    "%d/%m/%Y",
    "%Y/%m/%d",
];

/// The account bucket of a Sun Life plan or product name. No name means the
/// group RRSP, but only in a file named for Sun Life (`named`): another
/// provider's fund ledger with the same columns stays non-registered, so its
/// gains are never hidden as registered. Spelled-out names count: "Tax-Free Savings Account" is a
/// TFSA, a deferred profit sharing plan is a DPSP, a pension plan is an
/// RPP. Anything else follows `bucket_of` (non-registered when unknown).
fn sunlife_bucket(plan: &str, named: bool) -> &'static str {
    let p = plan.to_lowercase();
    let words: Vec<&str> = p.split(|c: char| !c.is_alphanumeric()).collect();
    let any = |keys: &[&str]| keys.iter().any(|k| p.contains(k));
    if p.trim().is_empty() {
        if named { "RRSP" } else { NON_REGISTERED }
    } else if any(&["non-reg", "non reg", "nonreg", "unregistered"]) {
        NON_REGISTERED
    } else if bucket_of(&p) != NON_REGISTERED {
        bucket_of(&p)
    } else if any(&["tax-free", "tax free"]) {
        "TFSA"
    } else if any(&["deferred profit"]) || words.contains(&"dpsp") {
        "DPSP"
    } else if any(&["pension"]) || words.contains(&"rpp") {
        "RPP"
    } else if any(&["retirement"]) || words.contains(&"rsp") {
        "RRSP"
    } else {
        NON_REGISTERED
    }
}

/// A fund name as the holding's symbol: group funds have no ticker. Spaces
/// collapse; only letters, digits and plain punctuation stay.
pub(crate) fn fund_label(raw: &str) -> String {
    let name: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| c.is_alphanumeric() || " &-.()/'%+".contains(*c))
        .take(64)
        .collect();
    name.trim().to_string()
}

/// Sun Life group plan transaction history (layout estimated, see
/// `SunLifeCols`). Contributions (yours or your employer's), transfers in
/// and switches into a fund are buys of its units at the dollar amount.
/// Withdrawals, transfers out and switches out are sales. Fees redeem
/// units. Interest or distributions credited in units are income.
/// Reversals, corrections, fee rebates and plain fund transfers follow the
/// sign of the units (or amount). All amounts are CAD. Registered plans
/// carry no taxable gains. Unknown types are counted under one label,
/// because the type column may hold free text.
/// True when a file name says Sun Life ("sunlife-2026.csv", "Sun Life history.csv").
fn named_sunlife(name: &str) -> bool {
    let squashed: String = name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    squashed.contains("sunlife")
}

fn sunlife(rows: &Rows, rep: &mut FileReport) -> Vec<Ev> {
    let named = named_sunlife(&rep.name);
    let Some(c) = SunLifeCols::from_keys(&rows.keys) else {
        rep.error = Some("the header changed while reading".into());
        return Vec::new();
    };
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let kind = get(r, Some(c.kind)).to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| kind.contains(w));
        // An interfund transfer names the fund "From X" or "To X".
        let raw = get(r, Some(c.fund)).trim();
        let raw = ["from ", "to "]
            .iter()
            .find(|p| {
                raw.len() > p.len()
                    && raw
                        .get(..p.len())
                        .is_some_and(|h| h.eq_ignore_ascii_case(p))
            })
            .map_or(raw, |p| &raw[p.len()..]);
        let asset = fund_label(raw);
        let summary = has(&[
            "opening balance",
            "closing balance",
            "beginning balance",
            "ending balance",
            "balance forward",
            "subtotal",
        ]) || kind.starts_with("total");
        if asset.is_empty() || summary {
            rep.ignored_rows += 1;
            continue;
        }
        let Some(day) = date_of(get(r, Some(c.date)), SUNLIFE_DATES) else {
            rep.bad_lines.push(*line);
            continue;
        };
        let units = num(get(r, Some(c.units))).filter(|u| *u != 0.0);
        let price = num(get(r, c.price)).map(f64::abs).filter(|p| *p > 0.0);
        let amount = num(get(r, c.amount)).filter(|a| *a != 0.0);
        let qty = units
            .map(f64::abs)
            .or(amount.zip(price).map(|(a, p)| a.abs() / p));
        let value = amount.map(f64::abs).or(qty.zip(price).map(|(q, p)| q * p));
        let negative = units.or(amount).is_some_and(|v| v < 0.0);
        let buy = |qty| Action::Buy { qty, cost: value };
        let sell = |qty| Action::Sell {
            qty,
            proceeds: value,
            fees: 0.0,
        };
        let by_sign = |q| if negative { sell(q) } else { buy(q) };
        let fee = has(&["fee", "charge"]);
        let undo = has(&["rebate", "reversal", "reverse", "correction", "adjustment"]);
        let bucket = sunlife_bucket(get(r, c.plan), named);
        // New money in: not a switch, transfer or rollover between plans or funds,
        // and not money out or income, whose type may also name the member or employer.
        let moved = has(&[
            "transfer",
            "switch",
            "rollover",
            "roll-over",
            "realloc",
            "rebalanc",
            "withdraw",
            "redemption",
            "redeem",
            "payout",
            "payment",
            "refund",
            "terminat",
            "interest",
            "dividend",
            "distribution",
            "income",
        ]);
        let contribution = !fee
            && !moved
            && bucket != NON_REGISTERED
            && has(&[
                "contribut",
                "payroll",
                "match",
                "member",
                "employee",
                "employer",
                "required",
                "voluntary",
                "lump sum",
                "lump-sum",
                "deposit",
            ]);
        if let (true, Some(v)) = (contribution, value) {
            rep.contrib.push(Contrib {
                date: day,
                bucket,
                source: if has(&["employer", "match", "company"]) {
                    "employer"
                } else {
                    "member"
                },
                cad: if negative { -v } else { v },
            });
        }
        let action = if fee && !undo && !has(&["refund", "credit"]) {
            qty.map(|qty| Action::Fee { qty })
        } else if undo || fee {
            qty.map(by_sign)
        } else if has(&[
            "withdraw",
            "redemption",
            "redeem",
            "transfer out",
            "switch out",
            "refund",
            "payout",
            "payment",
            "terminat",
            "sell",
            "sold",
        ]) {
            qty.map(sell)
        } else if has(&["interest", "dividend", "distribution", "income"]) {
            let kind = if has(&["dividend"]) {
                "dividend"
            } else if has(&["interest"]) {
                "interest"
            } else {
                "other"
            };
            Some(Action::Income {
                qty: qty.unwrap_or(0.0),
                cad: value,
                kind,
            })
        } else if has(&[
            "contribut",
            "deposit",
            "transfer in",
            "switch in",
            "purchase",
            "buy",
            "rollover",
            "roll-over",
            "lump sum",
            "lump-sum",
            "payroll",
            "match",
            "member",
            "employee",
            "employer",
            "required",
            "voluntary",
        ]) {
            qty.map(buy)
        } else if has(&[
            "transfer",
            "switch",
            "rebalanc",
            "realloc",
            "exchange",
            "interfund",
        ]) {
            qty.map(by_sign)
        } else {
            *rep.unhandled_types
                .entry("unknown type".into())
                .or_insert(0) += 1;
            rep.unhandled_rows += 1;
            continue;
        };
        let Some(action) = action else {
            rep.bad_lines.push(*line);
            continue;
        };
        out.push(Ev {
            paid: None,
            acct: String::new(),
            date: day,
            line: *line,
            asset,
            bucket,
            action,
        });
        rep.used_rows += 1;
    }
    out
}

fn crypto(rows: &Rows, rep: &mut FileReport, fx: &mut Fx) -> Vec<Ev> {
    let Some(c) = CryptoCols::from_keys(&rows.keys) else {
        rep.error = Some("the header changed while reading".into());
        return Vec::new();
    };
    const FORMATS: &[&str] = &[
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%d/%m/%Y",
        "%Y/%m/%d",
        "%b %d, %Y",
        "%d %b %Y",
    ];
    let mut out = Vec::new();
    for (line, r) in &rows.records {
        let day = date_of(get(r, Some(c.date)), FORMATS);
        let asset = c
            .asset
            .map(|i| get(r, Some(i)).to_uppercase())
            .or(c.header_asset.clone())
            .unwrap_or_default();
        let qty = num(get(r, Some(c.qty))).map(f64::abs);
        let (Some(day), Some(qty)) = (day, qty) else {
            rep.bad_lines.push(*line);
            continue;
        };
        if asset.is_empty() {
            rep.bad_lines.push(*line);
            continue;
        }
        let kind = get(r, Some(c.kind)).to_lowercase();
        let any = |words: &[&str]| words.iter().any(|w| kind.contains(w));
        let transfer = any(&[
            "receive", "deposit", "incoming", "send", "sent", "withdraw", "outgoing",
        ]) || kind == "in"
            || kind == "out";
        let value_cur = c
            .value_cur
            .clone()
            .or(c.value_cur_col.map(|i| get(r, Some(i)).to_uppercase()))
            .unwrap_or_else(|| "CAD".into());
        let value = num(get(r, c.value)).map(f64::abs).filter(|v| *v > 0.0);
        let fee = num(get(r, c.fee)).map(f64::abs).unwrap_or(0.0);
        let fee_in_asset = match &c.fee_cur {
            Some(cur) => cur == &asset,
            None => c.value.is_none() || transfer,
        };
        let fee_cad = if fee_in_asset {
            0.0
        } else {
            fx.cad(fee, c.fee_cur.as_deref().unwrap_or(&value_cur))
                .unwrap_or(0.0)
        };
        let bucket = c
            .account_type
            .map_or(NON_REGISTERED, |i| bucket_of(get(r, Some(i))));
        let action = if any(&["sell", "sold"]) {
            Action::Sell {
                qty,
                proceeds: value.and_then(|v| fx.cad(v, &value_cur)),
                fees: fee_cad,
            }
        } else if any(&["buy", "bought", "purchase"]) {
            Action::Buy {
                qty,
                cost: value
                    .and_then(|v| fx.cad(v, &value_cur))
                    .map(|v| v + fee_cad),
            }
        } else if any(&[
            "reward", "staking", "stake", "interest", "income", "airdrop", "earn",
        ]) {
            let kind = if any(&["interest"]) {
                "interest"
            } else if any(&["airdrop", "income"]) {
                "other"
            } else {
                "staking"
            };
            Action::Income {
                qty,
                cad: value.and_then(|v| fx.cad(v, &value_cur)),
                kind,
            }
        } else if any(&["receive", "deposit", "incoming"]) || kind == "in" || kind == "transfer in"
        {
            Action::In { qty }
        } else if any(&["send", "sent", "withdraw", "outgoing"])
            || kind == "out"
            || kind == "transfer out"
        {
            Action::Out { qty }
        } else {
            *rep.unhandled_types
                .entry("unknown type".into())
                .or_insert(0) += 1;
            rep.unhandled_rows += 1;
            continue;
        };
        out.push(Ev {
            paid: None,
            acct: String::new(),
            date: day,
            line: *line,
            asset: asset.clone(),
            bucket,
            action,
        });
        if fee_in_asset && fee > 0.0 {
            out.push(Ev {
                paid: None,
                acct: String::new(),
                date: day,
                line: *line,
                asset,
                bucket,
                action: Action::Fee { qty: fee },
            });
        }
        rep.used_rows += 1;
    }
    out
}

/// Reads one file, or each sheet of an .xlsx, that has a ledger header.
fn read_file(path: &Path, fx: &mut Fx) -> Vec<(FileReport, Vec<Ev>)> {
    let name = file_name(path);
    let texts = if is_xlsx_path(path) {
        xlsx_sheets(path).map(|sheets| {
            let multi = sheets.len() > 1;
            sheets
                .into_iter()
                .map(|(sheet, t)| {
                    let label = if multi {
                        format!("{name} [{}]", header_text(&sheet))
                    } else {
                        name.clone()
                    };
                    (label, t)
                })
                .collect::<Vec<_>>()
        })
    } else {
        read_text(path).map(|t| vec![(name.clone(), t)])
    };
    let texts = match texts {
        Ok(t) => t,
        Err(e) => {
            return vec![(
                FileReport {
                    name,
                    format: "unreadable".into(),
                    error: Some(e),
                    ..FileReport::default()
                },
                Vec::new(),
            )];
        }
    };
    let mut out = Vec::new();
    for (label, text) in texts {
        let Some(header) = find_header(&text) else {
            continue;
        };
        let mut rep = FileReport {
            name: label,
            format: header.label.to_string(),
            ..FileReport::default()
        };
        let rows = Rows::read(&text, &header, &mut rep);
        let events = match header.label {
            KRAKEN => kraken(&rows, &mut rep, fx),
            WEALTHSIMPLE => wealthsimple(&rows, &mut rep, fx),
            ETRADE => etrade(&rows, &mut rep, fx),
            ETRADE_PLAN => etrade_plan(&rows, &mut rep, fx),
            ETRADE_GL => etrade_gl(&rows, &mut rep, fx),
            ETRADE_RS => etrade_rs(&rows, &mut rep, fx),
            ETRADE_ESPP => etrade_espp(&rows, &mut rep, fx),
            CASA => casa(&rows, &mut rep),
            SUNLIFE => sunlife(&rows, &mut rep),
            _ => crypto(&rows, &mut rep, fx),
        };
        rep.bad_lines.sort_unstable();
        out.push((rep, events));
    }
    out
}

/// Reads every ledger file in `dir` and computes holdings.
pub fn load_dir(dir: &Path, cfg: &Config) -> Result<Holdings, String> {
    let mut fx = Fx::new(cfg);
    let mut files = Vec::new();
    let mut events = Vec::new();
    // An export downloaded twice (or two overlapping exports) repeats rows.
    // A row counts as often as the one file holding it most often does.
    let mut most: HashMap<String, usize> = HashMap::new();
    let mut repeated = 0;
    for path in crate::inbox::inbox_files(dir)? {
        if is_ofx_path(&path) {
            continue;
        }
        for (mut report, evs) in read_file(&path, &mut fx) {
            let mut here: HashMap<String, usize> = HashMap::new();
            let contrib = std::mem::take(&mut report.contrib);
            for c in contrib {
                let key = format!(
                    "contrib|{}|{}|{}|{}|{:?}",
                    report.format, c.date, c.bucket, c.source, c.cad
                );
                let n = here.entry(key.clone()).or_default();
                *n += 1;
                if *n <= most.get(&key).copied().unwrap_or(0) {
                    repeated += 1;
                } else {
                    report.contrib.push(c);
                }
            }
            let mut keep = Vec::new();
            for e in evs {
                let key = format!(
                    "{}|{}|{}|{}|{:?}",
                    report.format, e.date, e.asset, e.bucket, e.action
                );
                let n = here.entry(key.clone()).or_default();
                *n += 1;
                if *n <= most.get(&key).copied().unwrap_or(0) {
                    repeated += 1;
                } else {
                    keep.push(e);
                }
            }
            for (key, n) in here {
                let m = most.entry(key).or_default();
                *m = (*m).max(n);
            }
            let file = files.len();
            files.push(report);
            events.extend(order(file, keep));
        }
    }
    if files.is_empty() {
        return Err(format!(
            "no ledger files in {}. Supported: {KRAKEN}, {WEALTHSIMPLE}, {ETRADE}, {ETRADE_PLAN}, {ETRADE_GL}, {ETRADE_RS}, {ETRADE_ESPP}, {CASA}, {SUNLIFE}, {CRYPTO}.",
            dir.display()
        ));
    }
    // The same count in two copies of one format is one count.
    let mut per_format: BTreeMap<(String, String), f64> = BTreeMap::new();
    for f in &files {
        for (symbol, qty) in &f.reported_sellable {
            let m = per_format
                .entry((f.format.clone(), symbol.clone()))
                .or_default();
            *m = m.max(*qty);
        }
    }
    let mut reported: BTreeMap<String, f64> = BTreeMap::new();
    for ((_, symbol), qty) in per_format {
        *reported.entry(symbol).or_default() += qty;
    }
    let mut sums: BTreeMap<(&'static str, i32, String, &'static str), (f64, usize)> =
        BTreeMap::new();
    for c in files.iter().flat_map(|f| &f.contrib) {
        let key = (
            c.bucket,
            deduction_year(c.bucket, c.date),
            c.date.format("%Y-%m").to_string(),
            c.source,
        );
        let e = sums.entry(key).or_insert((0.0, 0));
        e.0 += c.cad;
        e.1 += 1;
    }
    let mut h = compute(files, events, cfg, fx);
    price_coin_moves(&mut h.trades, &dir.join("prices"));
    h.contributions = sums
        .into_iter()
        .map(
            |((bucket, year, month, source), (cad, rows))| ContributionSum {
                account_bucket: bucket.to_string(),
                deduction_year: year,
                month,
                source: source.to_string(),
                cad: r2(cad),
                rows,
            },
        )
        .collect();
    for (symbol, want) in reported {
        let have: f64 = h
            .assets
            .iter()
            .filter(|a| a.symbol == symbol && a.account_bucket == NON_REGISTERED)
            .map(|a| a.quantity)
            .sum();
        let verdict = if (have - want).abs() < 0.001 {
            "match"
        } else {
            "MISMATCH"
        };
        h.checks.push(format!(
            "{symbol}: E*TRADE reports {} sellable shares; the files give {} non-registered ({verdict}).",
            r4(want) + 0.0,
            r4(have) + 0.0
        ));
    }
    if repeated > 0 {
        h.checks.push(format!(
            "{repeated} rows also appear in another file of the same format (an export downloaded twice?) and count once."
        ));
    }
    Ok(h)
}

/// Orders one file's events by date. Files listed newest first are read
/// in reverse so same-day rows keep their real order.
fn order(file: usize, mut evs: Vec<Ev>) -> Vec<Event> {
    evs.sort_by_key(|e| e.line);
    let descending = evs
        .first()
        .zip(evs.last())
        .is_some_and(|(a, b)| a.date > b.date);
    let max_line = evs.last().map_or(0, |e| e.line);
    evs.into_iter()
        .map(|e| Event {
            acct: e.acct,
            date: e.date,
            rank: u8::from(!matches!(e.action, Action::Out { .. })),
            file,
            seq: if descending {
                max_line - e.line
            } else {
                e.line
            },
            asset: e.asset,
            bucket: e.bucket,
            action: e.action,
            paid: e.paid,
        })
        .collect()
}

// ---------------------------------------------------------------- compute

#[derive(Default)]
struct Pos {
    qty: f64,
    acb: f64,
    outside: f64,
    cost_unknown: bool,
}

impl Pos {
    /// Removes `qty` units and returns (removed, their ACB).
    fn take(&mut self, qty: f64) -> (f64, f64) {
        let removed = qty.min(self.qty.max(0.0));
        let acb = if self.qty > EPS {
            self.acb * removed / self.qty
        } else {
            0.0
        };
        self.qty -= removed;
        self.acb -= acb;
        if self.qty <= EPS {
            *self = Pos::default();
        }
        self.outside = self.outside.min(self.qty);
        (removed, acb)
    }
}

#[derive(Default)]
struct Year {
    gains: f64,
    losses: f64,
    sales: usize,
    incomplete: usize,
    by_asset: BTreeMap<String, f64>,
}

fn r4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

fn r2(x: f64) -> f64 {
    let v = (x * 100.0).round() / 100.0;
    if v == 0.0 { 0.0 } else { v }
}

fn r8(x: f64) -> f64 {
    let v = (x * 1e8).round() / 1e8;
    if v == 0.0 { 0.0 } else { v }
}

fn compute(files: Vec<FileReport>, mut events: Vec<Event>, cfg: &Config, fx: Fx) -> Holdings {
    // Every share movement in the account, so its share count adds up.
    // Only buys and sells carry a cash amount; the rest move coins with
    // no known price, and the writer leaves those to the owner.
    let mut trades: Vec<Trade> = events
        .iter()
        .flat_map(|e| {
            let trade = matches!(e.action, Action::Buy { .. } | Action::Sell { .. });
            let moves: Vec<(String, f64, Option<f64>)> = match &e.action {
                Action::Buy { qty, cost } => vec![(e.asset.clone(), *qty, *cost)],
                // A file that names the paying cash (Kraken) holds the sale
                // net of its fee: that is the cash the sale moved.
                Action::Sell {
                    qty,
                    proceeds,
                    fees,
                } => {
                    let cash = proceeds.map(|p| if e.paid.is_some() { p - fees } else { p });
                    vec![(e.asset.clone(), -qty, cash)]
                }
                Action::In { qty } | Action::Income { qty, .. } => {
                    vec![(e.asset.clone(), *qty, None)]
                }
                Action::Out { qty } | Action::Fee { qty } => vec![(e.asset.clone(), -qty, None)],
                Action::Swap { to, qty, qty_to } => {
                    vec![(e.asset.clone(), -qty, None), (to.clone(), *qty_to, None)]
                }
            };
            let format = files
                .get(e.file)
                .map(|f| f.format.clone())
                .unwrap_or_default();
            moves
                .into_iter()
                // Cash and rows with no symbol are not shares.
                .filter(move |(s, _, _)| trade || (!s.is_empty() && !is_fiat(s)))
                .map(move |(symbol, shares, cad)| Trade {
                    date: e.date,
                    symbol,
                    bucket: e.bucket,
                    format: format.clone(),
                    account: e.acct.clone(),
                    shares,
                    cad,
                    cash: trade.then(|| e.paid.clone().unwrap_or_default()),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    trades.sort_by_key(|t| t.date);
    let mut pos: BTreeMap<(&'static str, String), Pos> = BTreeMap::new();
    let mut needs: BTreeMap<(String, &'static str, &'static str), (usize, f64)> = BTreeMap::new();
    let mut need = |asset: &str, bucket: &'static str, reason: &'static str, qty: f64| {
        let e = needs
            .entry((asset.to_string(), bucket, reason))
            .or_insert((0, 0.0));
        e.0 += 1;
        e.1 += qty;
    };
    for (asset, o) in &cfg.ledger.opening {
        let p = pos
            .entry((
                bucket_of(o.bucket.as_deref().unwrap_or("")),
                asset.to_uppercase(),
            ))
            .or_default();
        p.qty += o.quantity;
        p.acb += o.acb;
        p.outside += o.quantity;
    }
    events.sort_by_key(|e| (e.date, e.rank, e.file, e.seq));
    let mut purchases: BTreeMap<String, Vec<NaiveDate>> = BTreeMap::new();
    for e in &events {
        match &e.action {
            Action::Buy { .. } => purchases.entry(e.asset.clone()).or_default().push(e.date),
            Action::Swap { to, .. } => purchases.entry(to.clone()).or_default().push(e.date),
            _ => {}
        }
    }
    let mut years: BTreeMap<i32, Year> = BTreeMap::new();
    let mut income: BTreeMap<(i32, &'static str, bool), (f64, usize, usize)> = BTreeMap::new();
    let mut superficial: BTreeMap<(String, i32), usize> = BTreeMap::new();

    for e in &events {
        let taxable = e.bucket == NON_REGISTERED;
        let key = (e.bucket, e.asset.clone());
        match &e.action {
            Action::Buy { qty, cost } => {
                let p = pos.entry(key).or_default();
                p.qty += qty;
                match cost {
                    Some(c) => p.acb += c,
                    None => {
                        p.cost_unknown = true;
                        need(&e.asset, e.bucket, NEED_PURCHASE, *qty);
                    }
                }
            }
            Action::Sell {
                qty,
                proceeds,
                fees,
            } => {
                let p = pos.entry(key).or_default();
                let unknown_before = p.cost_unknown;
                let (sold, acb) = p.take(*qty);
                let short = *qty - sold > EPS.max(qty * 1e-9);
                if short {
                    need(&e.asset, e.bucket, NEED_OVERSOLD, qty - sold);
                }
                if !taxable {
                    continue;
                }
                let y = years.entry(e.date.year()).or_default();
                y.sales += 1;
                match proceeds {
                    Some(proceeds) if !short && !unknown_before => {
                        let gain = proceeds - fees - acb;
                        if gain >= 0.0 {
                            y.gains += gain;
                        } else {
                            y.losses += gain;
                        }
                        *y.by_asset.entry(e.asset.clone()).or_insert(0.0) += gain;
                        let near = purchases.get(&e.asset).is_some_and(|ds| {
                            ds.iter()
                                .any(|d| (*d - e.date).num_days().abs() <= SUPERFICIAL_DAYS)
                        });
                        if gain < -0.005 && near {
                            *superficial
                                .entry((e.asset.clone(), e.date.year()))
                                .or_insert(0) += 1;
                        }
                    }
                    _ => y.incomplete += 1,
                }
            }
            Action::In { qty } => {
                let p = pos.entry(key).or_default();
                let matched = qty.min(p.outside);
                p.outside -= matched;
                let rest = qty - matched;
                if rest > EPS {
                    p.qty += rest;
                    p.cost_unknown = true;
                    need(&e.asset, e.bucket, NEED_DEPOSIT, rest);
                }
            }
            Action::Out { qty } => {
                let p = pos.entry(key).or_default();
                let in_files = p.qty - p.outside;
                if qty - in_files > EPS {
                    p.qty += qty - in_files;
                    p.cost_unknown = true;
                    need(&e.asset, e.bucket, NEED_SENT, qty - in_files);
                }
                p.outside += qty;
            }
            Action::Fee { qty } => {
                pos.entry(key).or_default().take(*qty);
            }
            Action::Income { qty, cad, kind } => {
                let entry = income
                    .entry((e.date.year(), kind, taxable))
                    .or_insert((0.0, 0, 0));
                entry.1 += 1;
                match cad {
                    Some(v) => entry.0 += v,
                    None => entry.2 += 1,
                }
                if *qty > 0.0 {
                    let p = pos.entry(key).or_default();
                    p.qty += qty;
                    match cad {
                        Some(v) => p.acb += v.abs(),
                        None => {
                            p.cost_unknown = true;
                            need(&e.asset, e.bucket, NEED_REWARD, *qty);
                        }
                    }
                }
            }
            Action::Swap { to, qty, qty_to } => {
                let (sold, acb) = pos.entry(key).or_default().take(*qty);
                if qty - sold > EPS {
                    need(&e.asset, e.bucket, NEED_OVERSOLD, qty - sold);
                }
                let p = pos.entry((e.bucket, to.clone())).or_default();
                p.qty += qty_to;
                p.acb += acb;
                p.cost_unknown = true;
                need(to, e.bucket, NEED_SWAP, *qty_to);
                if taxable {
                    let y = years.entry(e.date.year()).or_default();
                    y.sales += 1;
                    y.incomplete += 1;
                }
            }
        }
    }

    if cfg.ledger.sent_out_is_gone {
        // The owner holds coins only where the files show them: what left
        // and never came back is gone, so it leaves the holding at its cost.
        for p in pos.values_mut() {
            if p.outside > EPS && p.qty > EPS {
                let share = (p.outside / p.qty).min(1.0);
                p.acb -= p.acb * share;
                p.qty -= p.outside;
                p.outside = 0.0;
            }
        }
    }
    for (symbol, want) in &cfg.ledger.held {
        let have: f64 = pos
            .iter()
            .filter(|((_, s), _)| s == symbol)
            .map(|(_, p)| p.qty)
            .sum();
        if have > EPS {
            let scale = want / have;
            for ((_, s), p) in pos.iter_mut() {
                if s == symbol {
                    p.qty *= scale;
                    p.acb *= scale;
                    p.outside = (p.outside * scale).min(p.qty);
                }
            }
        }
    }
    let assets = pos
        .into_iter()
        .filter(|(_, p)| p.qty > EPS || p.acb.abs() >= 0.005)
        .map(|((bucket, symbol), p)| AssetHolding {
            symbol,
            account_bucket: bucket.to_string(),
            quantity: r8(p.qty),
            acb_cad: r2(p.acb),
            acb_per_unit_cad: (p.qty > EPS && !p.cost_unknown).then(|| r2(p.acb / p.qty)),
            cost_unknown: p.cost_unknown,
            held_outside_files: r8(p.outside),
        })
        .collect();
    let realized_by_year = years
        .into_iter()
        .map(|(year, y)| YearGains {
            year,
            gains_cad: r2(y.gains),
            losses_cad: r2(y.losses),
            net_cad: r2(y.gains + y.losses),
            sales: y.sales,
            incomplete_sales: y.incomplete,
            net_by_asset_cad: y.by_asset.into_iter().map(|(k, v)| (k, r2(v))).collect(),
        })
        .collect();
    let income_by_year = income
        .into_iter()
        .map(|((year, kind, taxable), (cad, rows, without))| YearIncome {
            year,
            kind: kind.to_string(),
            taxable,
            cad: r2(cad),
            rows,
            rows_without_value: without,
        })
        .collect();
    let superficial_loss_flags = superficial
        .into_iter()
        .map(|((asset, year), count)| SuperficialFlag { asset, year, count })
        .collect();
    let needs_cost = needs
        .into_iter()
        .map(|((asset, bucket, reason), (rows, qty))| NeedsCost {
            asset,
            account_bucket: bucket.to_string(),
            reason: reason.to_string(),
            rows,
            quantity: r8(qty),
        })
        .collect();
    Holdings {
        files,
        assets,
        realized_by_year,
        income_by_year,
        superficial_loss_flags,
        needs_cost,
        missing_rates: fx.missing.into_iter().collect(),
        checks: Vec::new(),
        contributions: Vec::new(),
        trades,
        notes: vec![
            "ACB is the average cost in CAD with fees, pooled per asset across non-registered accounts.".into(),
            "Registered accounts are tracked without taxable gains or losses.".into(),
            "Other currencies use the single [currencies] rate, not the rate of each trade date.".into(),
            "Crypto sent out and not seen arriving again still counts as held (held_outside_files).".into(),
        ],
    }
}

// ---------------------------------------------------------------- report

fn cad(x: f64) -> String {
    fmt_cents((x * 100.0).round() as i64)
}

/// The holdings report printed to the terminal. Aggregates only.
pub fn render(h: &Holdings) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "# Holdings report\n\n## Ledger files\n");
    for f in &h.files {
        if let Some(e) = &f.error {
            let _ = writeln!(o, "- {}: NOT READ: {e}", f.name);
            continue;
        }
        let mut parts = vec![format!("{} rows", f.rows), format!("{} used", f.used_rows)];
        if f.ignored_rows > 0 {
            parts.push(format!("{} cash or summary rows ignored", f.ignored_rows));
        }
        if f.unhandled_rows > 0 {
            let types: Vec<String> = f
                .unhandled_types
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect();
            parts.push(format!(
                "{} not handled ({})",
                f.unhandled_rows,
                types.join(", ")
            ));
        }
        if !f.bad_lines.is_empty() {
            let lines: Vec<String> = f.bad_lines.iter().take(10).map(usize::to_string).collect();
            parts.push(format!(
                "{} unreadable (lines {})",
                f.bad_lines.len(),
                lines.join(", ")
            ));
        }
        let _ = writeln!(o, "- {} ({}): {}.", f.name, f.format, parts.join(", "));
    }
    let _ = writeln!(o, "\n## Holdings (CAD, average cost)\n");
    let _ = writeln!(
        o,
        "| Asset | Account | Quantity | ACB | ACB per unit | Notes |"
    );
    let _ = writeln!(o, "|---|---|---:|---:|---:|---|");
    for a in &h.assets {
        let mut notes = Vec::new();
        if a.cost_unknown {
            notes.push("cost unknown".to_string());
        }
        if a.held_outside_files > 0.0 {
            notes.push(format!("{} held outside the files", a.held_outside_files));
        }
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} | {} |",
            a.symbol,
            a.account_bucket,
            a.quantity,
            cad(a.acb_cad),
            a.acb_per_unit_cad.map_or("-".into(), cad),
            notes.join("; ")
        );
    }
    if !h.realized_by_year.is_empty() {
        let _ = writeln!(o, "\n## Realized gains and losses (non-registered)\n");
        let _ = writeln!(o, "| Year | Gains | Losses | Net | Sales | Incomplete |");
        let _ = writeln!(o, "|---|---:|---:|---:|---:|---:|");
        for y in &h.realized_by_year {
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} | {} | {} |",
                y.year,
                cad(y.gains_cad),
                cad(y.losses_cad),
                cad(y.net_cad),
                y.sales,
                y.incomplete_sales
            );
        }
    }
    if !h.income_by_year.is_empty() {
        let _ = writeln!(o, "\n## Income\n");
        let _ = writeln!(
            o,
            "| Year | Kind | Taxable | CAD | Rows | Rows without a value |"
        );
        let _ = writeln!(o, "|---|---|---|---:|---:|---:|");
        for i in &h.income_by_year {
            let taxable = if i.taxable { "yes" } else { "no (registered)" };
            let _ = writeln!(
                o,
                "| {} | {} | {taxable} | {} | {} | {} |",
                i.year,
                i.kind,
                cad(i.cad),
                i.rows,
                i.rows_without_value
            );
        }
    }
    if !h.superficial_loss_flags.is_empty() {
        let _ = writeln!(o, "\n## Possible superficial losses\n");
        for s in &h.superficial_loss_flags {
            let _ = writeln!(
                o,
                "- {} {}: {} loss sale(s) with a purchase within 30 days",
                s.asset, s.year, s.count
            );
        }
    }
    if !h.needs_cost.is_empty() {
        let _ = writeln!(o, "\n## Needs a cost\n");
        for n in &h.needs_cost {
            let _ = writeln!(
                o,
                "- {} ({}): {}: {} row(s), quantity {}",
                n.asset,
                n.account_bucket,
                n.reason,
                n.rows,
                r8(n.quantity)
            );
        }
        let _ = writeln!(
            o,
            "\nAdd known costs in cashcast.toml, for example [ledger.opening.BTC] quantity = 0.5, acb = 20000."
        );
    }
    if !h.missing_rates.is_empty() {
        let _ = writeln!(o, "\n## Missing rates\n");
        let _ = writeln!(
            o,
            "Add these under [currencies] rates: {}",
            h.missing_rates.join(", ")
        );
    }
    if !h.checks.is_empty() {
        let _ = writeln!(o, "\n## Checks\n");
        for c in &h.checks {
            let _ = writeln!(o, "- {c}");
        }
    }
    let _ = writeln!(o, "\n## Rules\n");
    for n in &h.notes {
        let _ = writeln!(o, "- {n}");
    }
    o
}

// ------------------------------------------------------- cash rows for MoneyWiz

/// The end of a broker account id shown in an export account name: six
/// characters, enough to tell apart ids that differ only before their
/// currency code ("…W06CAD", "…V06CAD").
fn id_tail(id: &str) -> String {
    let c: Vec<char> = id.chars().collect();
    c[c.len().saturating_sub(6)..].iter().collect()
}

/// Ledger formats that also make cash rows for `moneywiz sync` and
/// `moneywiz coverage`: the money that moves in or out of the account.
pub const CASH_LEDGERS: &[&str] = &[SUNLIFE, ETRADE, KRAKEN, WEALTHSIMPLE];

/// The cash rows of a Sun Life or E*TRADE transactions file, a Wealthsimple
/// activity file (every account) or a Kraken ledger (fiat legs, net of fees), as an export
/// that `moneywiz` can match against an investment account. Contributions,
/// deposits, withdrawals, dividends, interest, fees, buys and sells become
/// rows with their signed amount. Switches between funds and share-only
/// transfers move no money and are skipped. The row text is the
/// fund (Sun Life) or the type and symbol (E*TRADE) only: descriptions and
/// names are never read. A Wealthsimple account id only names its export
/// account (its last six characters) and links it (`ledger_ids`). `None`
/// for other files and for .xlsx sheets.
pub fn cash_export(path: &Path) -> Option<crate::export::Export> {
    if is_xlsx_path(path) {
        return None;
    }
    let text = read_text(path).ok()?;
    let header = find_header(&text)?;
    if !CASH_LEDGERS.contains(&header.label) {
        return None;
    }
    let name = file_name(path);
    let mut rep = FileReport::default();
    let rows = Rows::read(&text, &header, &mut rep);
    let mut export = crate::export::Export::empty(&name, header.label);
    export.header_line = header.line;
    let mut push = |_line: usize,
                    date: NaiveDate,
                    cents: i64,
                    account: String,
                    currency: &str,
                    text: String| {
        let derived = crate::text::derive(&text, cents);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // No line number: overlapping downloads hold a row on other lines, and
        // `Inbox::merge` keeps the copies of one file (see its doc).
        std::hash::Hash::hash(&(&account, date, cents, &text), &mut h);
        export
            .account_kinds
            .insert(account.clone(), crate::export::AccountKind::Bank);
        export.txns.push(crate::export::Txn {
            file: 0,
            account,
            transfer: String::new(),
            category: Vec::new(),
            category_from_file: false,
            hint: derived.hint,
            payee_key: derived.payee_key,
            date,
            cents,
            currency: currency.to_string(),
            fingerprint: std::hash::Hasher::finish(&h),
            text,
        });
    };
    let cents_of = |v: f64| (v * 100.0).round() as i64;
    match header.label {
        SUNLIFE => {
            let c = SunLifeCols::from_keys(&rows.keys)?;
            let named = named_sunlife(&name);
            for (line, r) in &rows.records {
                let kind = get(r, Some(c.kind)).to_lowercase();
                let has = |words: &[&str]| words.iter().any(|w| kind.contains(w));
                let fund = fund_label(get(r, Some(c.fund)));
                let moved_inside = has(&[
                    "switch",
                    "interfund",
                    "fund transfer",
                    "transfer in",
                    "transfer out",
                    "realloc",
                    "rebalanc",
                ]);
                if fund.is_empty()
                    || moved_inside
                    || kind.contains("balance")
                    || kind.starts_with("total")
                {
                    continue;
                }
                let Some(day) = date_of(get(r, Some(c.date)), SUNLIFE_DATES) else {
                    export.bad_lines.push((*line, "date not parseable"));
                    continue;
                };
                let units = num(get(r, Some(c.units))).filter(|u| *u != 0.0);
                let price = num(get(r, c.price)).map(f64::abs);
                let amount = num(get(r, c.amount))
                    .filter(|a| *a != 0.0)
                    .or(units.zip(price).map(|(u, p)| u * p));
                let Some(amount) = amount else {
                    export.bad_lines.push((*line, "amount not parseable"));
                    continue;
                };
                // A fee or withdrawal written as a positive amount still takes money out.
                let negative = units.is_some_and(|u| u < 0.0)
                    || amount < 0.0
                    || (has(&["fee", "charge", "withdraw", "redemption", "payout"])
                        && !has(&["rebate", "refund", "reversal"]));
                let cents = if negative {
                    -cents_of(amount.abs())
                } else {
                    cents_of(amount.abs())
                };
                let bucket = sunlife_bucket(get(r, c.plan), named);
                // The type may name the member or employer: the text is the fund only.
                push(*line, day, cents, format!("Sun Life {bucket}"), "CAD", fund);
            }
        }
        WEALTHSIMPLE => {
            // Spending accounts (Cash, Credit) are named by their type.
            // Investment accounts (TFSA, FHSA, non-registered, …) get the end
            // of their account id too, since one file can hold several of a
            // type; `ledger_ids` lets `[moneywiz.investments]` link them. The
            // text is the activity type only: the description names payees.
            let c = |n: &str| rows.col(&[n]);
            let (date, account_type, kind, sub, currency, net, account_id) = (
                c("transaction_date").or(c("effective_date")),
                c("account_type"),
                c("activity_type"),
                c("activity_sub_type"),
                c("currency"),
                c("net_cash_amount"),
                c("account_id"),
            );
            for (line, r) in &rows.records {
                let kind_of = get(r, account_type).trim().to_string();
                let lower = kind_of.to_lowercase();
                let spending = ["cash", "chequing", "credit", "card", "spend"]
                    .iter()
                    .any(|w| lower.contains(w));
                let id = get(r, account_id).trim();
                let name = if spending || id.is_empty() {
                    format!("Wealthsimple {kind_of}")
                } else {
                    format!("Wealthsimple {kind_of} …{}", id_tail(id))
                };
                if !spending {
                    export
                        .ledger_ids
                        .entry(name.clone())
                        .or_default()
                        .insert((bucket_of(&kind_of).to_string(), id.to_string()));
                }
                let Some(cents) = num(get(r, net)).map(cents_of).filter(|c| *c != 0) else {
                    continue;
                };
                let Some(day) = date_of(get(r, date), &["%Y-%m-%d", "%m/%d/%Y"]) else {
                    export.bad_lines.push((*line, "date not parseable"));
                    continue;
                };
                let cur = get(r, currency).to_uppercase();
                let cur = if cur.is_empty() {
                    "CAD".to_string()
                } else {
                    cur
                };
                let text = format!("{} {}", get(r, kind), get(r, sub))
                    .trim()
                    .to_lowercase();
                push(*line, day, cents, name, &cur, text);
            }
        }
        KRAKEN => {
            // Only the fiat legs are cash; crypto amounts do not fit in cents.
            let c = |n: &str| rows.col(&[n]);
            let (time, kind, asset, amount, fee, balance) = (
                c("time"),
                c("type"),
                c("asset"),
                c("amount"),
                c("fee"),
                c("balance"),
            );
            // The balance after the latest row per fiat asset: the bank
            // balance `moneywiz sync` checks against.
            let mut last: BTreeMap<String, (String, NaiveDate, i64)> = BTreeMap::new();
            for (line, r) in &rows.records {
                let cur = kraken_asset(get(r, asset));
                if !is_fiat(&cur) {
                    continue;
                }
                let Some(day) = date_of(get(r, time), &["%Y-%m-%d"]) else {
                    export.bad_lines.push((*line, "date not parseable"));
                    continue;
                };
                if let Some(b) = num(get(r, balance)).map(cents_of) {
                    let at = get(r, time).to_string();
                    let e = last.entry(cur.clone()).or_insert((at.clone(), day, b));
                    if at >= e.0 {
                        *e = (at, day, b);
                    }
                }
                let Some(net) = num(get(r, amount))
                    .map(|a| a - num(get(r, fee)).unwrap_or(0.0).abs())
                    .map(cents_of)
                    .filter(|c| *c != 0)
                else {
                    continue;
                };
                let text = format!("{} {cur}", get(r, kind).to_lowercase());
                push(*line, day, net, format!("Kraken {cur}"), &cur, text);
            }
            for (cur, (_, day, cents)) in last {
                export.balances.insert(
                    format!("Kraken {cur}"),
                    crate::export::Balance {
                        cents,
                        as_of: Some(day),
                        source: "kraken-ledger",
                    },
                );
            }
        }
        _ => {
            let date = rows.col(&["transactiondate"]);
            let kind = rows.col(&["transactiontype"]);
            let symbol = rows.col(&["symbol"]);
            let amount = rows.col(&["amount"]);
            for (line, r) in &rows.records {
                let Some(cents) = num(get(r, amount)).map(cents_of).filter(|c| *c != 0) else {
                    continue;
                };
                let Some(day) = date_of(get(r, date), &["%m/%d/%Y", "%m/%d/%y", "%Y-%m-%d"]) else {
                    export.bad_lines.push((*line, "date not parseable"));
                    continue;
                };
                let text = format!(
                    "{} {}",
                    get(r, kind).to_lowercase(),
                    get(r, symbol).to_uppercase()
                );
                push(
                    *line,
                    day,
                    cents,
                    "E*TRADE".to_string(),
                    "USD",
                    text.trim().to_string(),
                );
            }
        }
    }
    Some(export)
}

/// Daily closes from Kraken's public OHLCVT files (`<PAIR>_1440.csv`: unix
/// time, open, high, low, close, volume, trades; no header) in `dir`.
fn load_closes(dir: &Path) -> HashMap<String, BTreeMap<NaiveDate, f64>> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_uppercase();
        let Some(pair) = name.strip_suffix("_1440.CSV") else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let mut m = BTreeMap::new();
        for line in text.lines() {
            let c: Vec<&str> = line.split(',').collect();
            let (Some(t), Some(close)) = (
                c.first().and_then(|t| t.trim().parse::<i64>().ok()),
                c.get(4).and_then(|v| v.trim().parse::<f64>().ok()),
            ) else {
                continue;
            };
            if let Some(d) = chrono::DateTime::from_timestamp(t, 0) {
                m.insert(d.date_naive(), close);
            }
        }
        out.insert(pair.to_string(), m);
    }
    out
}

/// The close on `day`, or the last one before it within a week.
fn close_on(m: &BTreeMap<NaiveDate, f64>, day: NaiveDate) -> Option<f64> {
    m.range(..=day)
        .next_back()
        .filter(|(d, _)| (day - **d).num_days() <= 7)
        .map(|(_, v)| *v)
}

/// The CAD price of one coin on a day: its CAD pair, or its USD or EUR pair
/// times that currency's CAD rate.
pub fn coin_cad(
    closes: &HashMap<String, BTreeMap<NaiveDate, f64>>,
    coin: &str,
    day: NaiveDate,
) -> Option<f64> {
    let k = if coin == "BTC" { "XBT" } else { coin };
    let get = |pair: String| closes.get(&pair).and_then(|m| close_on(m, day));
    get(format!("{k}CAD")).or_else(|| {
        ["USD", "EUR"]
            .iter()
            .find_map(|q| Some(get(format!("{k}{q}"))? * get(format!("{q}CAD"))?))
    })
}

/// Gives a CAD value to coin moves that have none (deposits, withdrawals,
/// rewards, swaps), from the daily price files, so MoneyWiz can show them.
fn price_coin_moves(trades: &mut [Trade], dir: &Path) {
    let closes = load_closes(dir);
    if closes.is_empty() {
        return;
    }
    for t in trades
        .iter_mut()
        .filter(|t| t.cad.is_none() && !is_fiat(&t.symbol))
    {
        t.cad = coin_cad(&closes, &t.symbol, t.date).map(|p| r2(p * t.shares.abs()));
    }
}
