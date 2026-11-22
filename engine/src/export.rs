//! Reads one export file: MoneyWiz CSV, a generic CSV with or without a
//! header row, a known bank or app layout (Scotiabank, TD, Wise,
//! Wealthsimple, PayPal), or OFX/QFX (see `ofx`).
//!
//! This is the only code that sees raw rows. Description, payee and memo
//! text go through `text::derive` and are then dropped. Error messages and
//! notes name line numbers and column positions, never field values.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use chrono::{Datelike, NaiveDate};

use crate::money::{detect_decimal, parse_cents};
use crate::text::{self, Hint};

/// Date formats tried when the config does not name one.
pub const DATE_FORMATS: &[&str] = &[
    "%Y-%m-%d",
    "%Y/%m/%d",
    "%Y/%d/%m",
    "%d/%m/%Y",
    "%m/%d/%Y",
    "%d.%m.%Y",
    "%d-%m-%Y",
    "%m-%d-%Y",
    "%b %d, %Y",
    "%d %b %Y",
    "%B %d, %Y",
    "%d %B %Y",
    "%d-%b-%Y",
    "%b %d %Y",
    "%d/%m/%y",
    "%m/%d/%y",
    "%d-%b-%y",
];

/// Category path separators, most specific first.
pub const CATEGORY_SEPARATORS: &[&str] = &["►", "▸", "▶", "→", ">", ":"];

const DATE: &[&str] = &[
    "date",
    "transaction date",
    "trans date",
    "trans. date",
    "posted date",
    "posting date",
    "post date",
    "date posted",
    "booking date",
    "value date",
    "purchase date",
    "transaction posted date",
];
const AMOUNT: &[&str] = &["amount", "transaction amount", "net amount", "value"];
const DEBIT: &[&str] = &[
    "debit",
    "debits",
    "debit amount",
    "withdrawal",
    "withdrawals",
    "withdrawal amount",
    "outflow",
    "money out",
    "paid out",
    "spent",
    "charge",
    "charges",
    "amount debit",
];
const CREDIT: &[&str] = &[
    "credit",
    "credits",
    "credit amount",
    "deposit",
    "deposits",
    "deposit amount",
    "inflow",
    "money in",
    "paid in",
    "received",
    "amount credit",
];
const KIND: &[&str] = &[
    "transaction type",
    "type",
    "debit/credit",
    "dr/cr",
    "cr/dr",
    "credit/debit",
];
const ACCOUNT: &[&str] = &["account", "account name"];
const CATEGORY: &[&str] = &[
    "category",
    "category name",
    "categories",
    "category group/category",
];
const CATEGORY_GROUP: &[&str] = &["category group", "master category", "parent category"];
const TRANSFER: &[&str] = &["transfers", "transfer", "transfer account"];
const CURRENCY: &[&str] = &["currency", "currency code", "ccy"];
const BALANCE: &[&str] = &[
    "balance",
    "running balance",
    "current balance",
    "available balance",
    "account balance",
];
const DESCRIPTION: &[&str] = &[
    "payee",
    "payee name",
    "merchant",
    "merchant name",
    "description",
    "name",
    "transaction description",
    "original description",
    "details",
    "narrative",
    "memo",
    "notes",
    "note",
    "reference",
    "original statement",
    "statement description",
    "description 1",
    "description 2",
];
const PRIVATE: &[&str] = &[
    "time",
    "check #",
    "cheque #",
    "cheque n°",
    "check number",
    "cheque number",
    "tags",
    "labels",
    "flag",
    "cleared",
    "status",
];

#[derive(Default, Clone)]
pub struct LoadOptions {
    pub date_format: Option<String>,
    pub category_separator: Option<String>,
    /// Account name for files without an account column. Default: file stem.
    pub account: Option<String>,
    pub currency: Option<String>,
    /// Some(true) flips signs; Some(false) never flips. None: flip when most
    /// rows are positive (purchases exported as positive amounts).
    pub invert: Option<bool>,
    /// 1-based column positions by role, overriding detection.
    pub columns: BTreeMap<String, usize>,
}

#[derive(Clone)]
pub struct Txn {
    /// Index of the source file in the inbox.
    pub file: usize,
    pub account: String,
    /// The other account of a transfer, or empty.
    pub transfer: String,
    /// Category path parts, for example `["Food", "Groceries"]`.
    pub category: Vec<String>,
    /// True when the category came from the file, false when guessed.
    pub category_from_file: bool,
    pub hint: Hint,
    /// Hashed payee words; 0 when the file has no description.
    pub payee_key: u64,
    pub date: NaiveDate,
    pub cents: i64,
    pub currency: String,
    pub fingerprint: u64,
    /// The row's description and payee text. Private: only `moneywiz`
    /// reads it (to match rows and to add them to MoneyWiz). It is printed
    /// only to the owner: in `out/moneywiz-fixes.md` and the web app's Bank
    /// sync view (`moneywiz_view`). Never hashed into output.
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Bank,
    Card,
}

#[derive(Clone)]
pub struct Balance {
    pub cents: i64,
    /// None means "current". Otherwise rows after this date still apply.
    pub as_of: Option<NaiveDate>,
    pub source: &'static str,
}

/// A statement's printed opening and closing balance, as the account's
/// balance (a card's owed amount is negative). PDF statements only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub account: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// The balance at the end of the day before `start`.
    pub opening: i64,
    /// The balance at the end of `end`.
    pub closing: i64,
}

pub struct Export {
    pub file_name: String,
    pub format: String,
    /// Mapping notes: column positions and decisions, never values.
    pub notes: Vec<String>,
    pub encoding: &'static str,
    pub delimiter: char,
    /// 1-based header line; 0 when the file has no header row.
    pub header_line: usize,
    pub columns: Vec<String>,
    pub date_format: String,
    pub date_format_matches: Vec<(String, usize)>,
    pub decimal: char,
    pub category_separator: String,
    /// Count of rows per set of filled key columns, for format diagnosis.
    pub row_shapes: BTreeMap<String, usize>,
    pub balances: BTreeMap<String, Balance>,
    /// Statement balances (PDF statements): `diagnose` checks they chain.
    pub statements: Vec<Statement>,
    pub account_kinds: BTreeMap<String, AccountKind>,
    /// Investment ledger rows: export account to the (plan type, broker
    /// account id) pairs it holds, so `[moneywiz.investments]` can link a
    /// MoneyWiz account to its cash rows. Ids are never printed.
    pub ledger_ids: BTreeMap<String, std::collections::BTreeSet<(String, String)>>,
    pub txns: Vec<Txn>,
    /// Line number and reason. Never the line's content.
    pub bad_lines: Vec<(usize, &'static str)>,
    pub split_candidates: usize,
    /// Payee key to a short description sample. Private: written only to
    /// the owner's review file, never printed.
    pub payee_labels: HashMap<u64, String>,
}

impl Export {
    pub fn empty(file_name: &str, format: &str) -> Self {
        Export {
            file_name: file_name.to_string(),
            format: format.to_string(),
            notes: Vec::new(),
            encoding: "UTF-8",
            delimiter: ',',
            header_line: 0,
            columns: Vec::new(),
            date_format: String::new(),
            date_format_matches: Vec::new(),
            decimal: '.',
            category_separator: String::new(),
            row_shapes: BTreeMap::new(),
            balances: BTreeMap::new(),
            statements: Vec::new(),
            account_kinds: BTreeMap::new(),
            ledger_ids: BTreeMap::new(),
            txns: Vec::new(),
            bad_lines: Vec::new(),
            split_candidates: 0,
            payee_labels: HashMap::new(),
        }
    }
}

#[derive(Default)]
struct Columns {
    moneywiz: bool,
    account: Option<usize>,
    name: Option<usize>,
    balance: Option<usize>,
    transfer: Option<usize>,
    category: Option<usize>,
    category_group: Option<usize>,
    date: usize,
    amount: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    kind: Option<usize>,
    currency: Option<usize>,
    descriptions: Vec<usize>,
    private: Vec<usize>,
    /// Format label of a known layout, such as `wise-statement-csv`.
    label: Option<&'static str>,
    /// Date formats tried in order on every row. Empty: detect one per file.
    date_formats: Vec<&'static str>,
    /// Date format to pick when several match equally well.
    prefer_date: Option<&'static str>,
    /// Keep a row only when this column holds one of these values.
    keep_if: Option<(usize, &'static [&'static str])>,
    /// Skip a row when this column holds one of these values.
    skip_if: Option<(usize, &'static [&'static str])>,
    /// One account per currency when the file holds several currencies.
    split_currency: bool,
    /// Private name columns. They feed the hashed payee key only.
    key_columns: Vec<usize>,
}

pub(crate) fn header_key(h: &str) -> String {
    h.trim()
        .trim_start_matches('\u{feff}')
        .trim_matches('"')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Columns {
    fn from_header(columns: &[String], moneywiz: bool) -> Result<Self, String> {
        let keys: Vec<String> = columns.iter().map(|c| header_key(c)).collect();
        let mut taken = vec![false; keys.len()];
        let mut take = |names: &[&str], prefix: Option<&str>| -> Option<usize> {
            for name in names {
                if let Some(i) = keys
                    .iter()
                    .enumerate()
                    .position(|(i, k)| !taken[i] && k.as_str() == *name)
                {
                    taken[i] = true;
                    return Some(i);
                }
            }
            let prefix = prefix?;
            let i = keys
                .iter()
                .enumerate()
                .position(|(i, k)| !taken[i] && k.starts_with(prefix))?;
            taken[i] = true;
            Some(i)
        };
        let missing = || {
            format!(
                "need a date column and an amount (or debit and credit) column; found: {}",
                columns.join(", ")
            )
        };
        let date = take(DATE, None).ok_or_else(missing)?;
        let debit = take(DEBIT, Some("debit ("));
        let credit = take(CREDIT, Some("credit ("));
        let amount = take(AMOUNT, Some("amount"));
        if amount.is_none() && debit.is_none() && credit.is_none() {
            return Err(missing());
        }
        let kind = take(KIND, None);
        let name = if moneywiz {
            take(&["name"], None)
        } else {
            None
        };
        let account = take(ACCOUNT, None);
        let category_group = take(CATEGORY_GROUP, None);
        let category = take(CATEGORY, None);
        let transfer = take(TRANSFER, None);
        let currency = take(CURRENCY, None);
        let balance = take(BALANCE, Some("balance ("));
        let mut descriptions = Vec::new();
        while let Some(i) = take(DESCRIPTION, None) {
            descriptions.push(i);
        }
        let mut private = Vec::new();
        while let Some(i) = take(PRIVATE, None) {
            private.push(i);
        }
        Ok(Columns {
            moneywiz,
            account,
            name,
            balance,
            transfer,
            category,
            category_group,
            date,
            amount,
            debit,
            credit,
            kind,
            currency,
            descriptions,
            private,
            ..Columns::default()
        })
    }

    /// Column roles for exports whose header names a known bank or app.
    fn known_layout(keys: &[String]) -> Option<Columns> {
        let at = |name: &str| keys.iter().position(|k| k == name);
        let has = |names: &[&str]| names.iter().all(|n| at(n).is_some());
        let some = |names: &[&str]| names.iter().filter_map(|n| at(n)).collect::<Vec<_>>();
        if has(&[
            "date",
            "description",
            "sub-description",
            "type of transaction",
            "amount",
        ]) {
            return Some(Columns {
                label: Some("scotiabank-csv"),
                date: at("date")?,
                amount: at("amount"),
                kind: at("type of transaction"),
                balance: at("balance"),
                descriptions: some(&["description", "sub-description"]),
                date_formats: vec!["%Y-%m-%d", "%m/%d/%Y"],
                ..Columns::default()
            });
        }
        // RBC online banking "Download transactions" CSV: one file for several
        // accounts, M/D/YYYY dates, and the amount in CAD$ (USD$ for a US-dollar account).
        if has(&[
            "account type",
            "account number",
            "transaction date",
            "description 1",
            "cad$",
        ]) {
            return Some(Columns {
                label: Some("rbc-csv"),
                date: at("transaction date")?,
                amount: at("cad$"),
                descriptions: some(&["description 1", "description 2"]),
                private: some(&["account number", "cheque number"]),
                date_formats: vec!["%m/%d/%Y"],
                ..Columns::default()
            });
        }
        if has(&[
            "transferwise id",
            "date",
            "amount",
            "currency",
            "description",
        ]) {
            return Some(Columns {
                label: Some("wise-statement-csv"),
                date: at("date")?,
                amount: at("amount"),
                currency: at("currency"),
                balance: at("running balance"),
                descriptions: some(&["description", "merchant"]),
                key_columns: some(&["merchant", "payer name", "payee name"]),
                private: some(&["transferwise id"]),
                date_formats: vec!["%d-%m-%Y"],
                split_currency: true,
                ..Columns::default()
            });
        }
        if has(&[
            "дата операции",
            "статус",
            "сумма платежа",
            "валюта платежа",
            "описание",
        ]) {
            return Some(Columns {
                label: Some("tinkoff-csv"),
                date: at("дата операции")?,
                amount: at("сумма платежа"),
                currency: at("валюта платежа"),
                category: at("категория"),
                descriptions: some(&["описание"]),
                private: some(&["номер карты"]),
                date_formats: vec!["%d.%m.%Y"],
                keep_if: Some((at("статус")?, &["ok"])),
                ..Columns::default()
            });
        }
        // Wealthsimple Activity > Download activities, one file per account
        // (2026-11): chequing and credit card. Pending rows are not posted yet.
        const NOT_POSTED: &[&str] = &["pending", "declined", "cancelled", "canceled", "failed"];
        if has(&[
            "transaction_date",
            "account_number",
            "transaction_type",
            "status",
            "amount",
            "currency",
        ]) {
            return Some(Columns {
                label: Some("wealthsimple-activities-csv"),
                date: at("transaction_date")?,
                amount: at("amount"),
                currency: at("currency"),
                descriptions: some(&["transaction_type", "transaction_sub_type", "notes"]),
                key_columns: some(&["payee"]),
                private: some(&["account_number", "payee_email"]),
                date_formats: vec!["%Y-%m-%d"],
                skip_if: Some((at("status")?, NOT_POSTED)),
                ..Columns::default()
            });
        }
        if has(&[
            "transaction_date",
            "transaction_type",
            "status",
            "merchant",
            "amount",
            "currency",
        ]) {
            return Some(Columns {
                label: Some("wealthsimple-activities-csv"),
                date: at("transaction_date")?,
                amount: at("amount"),
                currency: at("currency"),
                category: at("category"),
                descriptions: some(&["transaction_type", "notes"]),
                key_columns: some(&["merchant"]),
                date_formats: vec!["%Y-%m-%d"],
                skip_if: Some((at("status")?, NOT_POSTED)),
                ..Columns::default()
            });
        }
        let mut sorted = keys.to_vec();
        sorted.sort();
        if sorted
            == [
                "amount",
                "balance",
                "currency",
                "date",
                "description",
                "transaction",
            ]
        {
            return Some(Columns {
                label: Some("wealthsimple-statement-csv"),
                date: at("date")?,
                amount: at("amount"),
                balance: at("balance"),
                currency: at("currency"),
                descriptions: some(&["transaction", "description"]),
                date_formats: vec!["%Y-%m-%d"],
                ..Columns::default()
            });
        }
        // The newer "Balance affecting" report: Amount, Fees, Total; dates follow the account locale.
        if has(&[
            "date",
            "amount",
            "fees",
            "total",
            "status",
            "currency",
            "transaction id",
        ]) {
            return Some(Columns {
                label: Some("paypal-activity-csv"),
                date: at("date")?,
                amount: at("total"),
                currency: at("currency"),
                balance: at("balance"),
                descriptions: some(&["type", "name", "item title"]),
                private: some(&["transaction id"]),
                keep_if: Some((at("status")?, &["completed"])),
                split_currency: true,
                ..Columns::default()
            });
        }
        if has(&[
            "date",
            "gross",
            "net",
            "status",
            "currency",
            "transaction id",
        ]) {
            return Some(Columns {
                label: Some("paypal-activity-csv"),
                date: at("date")?,
                amount: at("net"),
                currency: at("currency"),
                balance: at("balance"),
                descriptions: some(&["type", "name", "item title"]),
                private: some(&["transaction id"]),
                keep_if: Some((at("status")?, &["completed"])),
                skip_if: at("balance impact").map(|i| (i, &["memo"][..])),
                prefer_date: Some("%m/%d/%Y"),
                split_currency: true,
                ..Columns::default()
            });
        }
        None
    }

    /// Scotiabank's older headerless export: `date,amount,-,type,description`.
    fn scotiabank_legacy(records: &[(usize, csv::StringRecord)]) -> Option<Columns> {
        let rows: Vec<&csv::StringRecord> = records
            .iter()
            .map(|(_, r)| r)
            .filter(|r| r.iter().any(|f| !f.trim().is_empty()))
            .take(200)
            .collect();
        let dash = rows
            .iter()
            .filter(|r| r.len() == 5 && r.get(2).is_some_and(|f| f.trim() == "-"))
            .count();
        let fits = !rows.is_empty() && dash * 10 >= rows.len() * 8;
        fits.then(|| Columns {
            label: Some("scotiabank-legacy-csv"),
            date: 0,
            amount: Some(1),
            descriptions: vec![3, 4],
            prefer_date: Some("%m/%d/%Y"),
            ..Columns::default()
        })
    }

    /// Applies 1-based overrides such as `date = 1`, `debit = 3`.
    fn apply_overrides(&mut self, overrides: &BTreeMap<String, usize>) -> Result<(), String> {
        for (role, &pos) in overrides {
            let i = pos.checked_sub(1).ok_or("column positions start at 1")?;
            match role.as_str() {
                "date" => self.date = i,
                "amount" => self.amount = Some(i),
                "debit" => self.debit = Some(i),
                "credit" => self.credit = Some(i),
                "balance" => self.balance = Some(i),
                "account" => self.account = Some(i),
                "category" => self.category = Some(i),
                "currency" => self.currency = Some(i),
                "description" => self.descriptions = vec![i],
                other => return Err(format!("unknown column role {other:?} in files.columns")),
            }
        }
        Ok(())
    }

    fn describe(&self) -> String {
        let mut parts = vec![format!("date={}", self.date + 1)];
        for (name, col) in [
            ("amount", self.amount),
            ("debit", self.debit),
            ("credit", self.credit),
            ("balance", self.balance),
            ("account", self.account),
            ("category", self.category),
        ] {
            if let Some(c) = col {
                parts.push(format!("{name}={}", c + 1));
            }
        }
        if let Some(d) = self.descriptions.first() {
            parts.push(format!("description={}", d + 1));
        }
        parts.join(", ")
    }
}

/// The decoded file from its first data or header row on.
struct Body {
    text: String,
    encoding: &'static str,
    /// 1-based header line; 0 when there is none.
    header_line: usize,
    /// 1-based line of the first row the CSV reader sees.
    start_line: usize,
    start_offset: usize,
    delimiter: char,
    moneywiz: bool,
}

impl Body {
    fn read(path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", file_name(path)))?;
        let (text, encoding) = decode(&bytes);
        Body::parse(text, encoding)
    }

    fn parse(text: String, encoding: &'static str) -> Result<Self, String> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut offset = 0;
        let mut sep_hint = None;
        let mut first_data: Option<(usize, usize)> = None;
        for (i, line) in text.split_inclusive('\n').enumerate().take(50) {
            let clean = line.trim().trim_matches('"');
            if let Some(rest) = clean.strip_prefix("sep=") {
                sep_hint = rest.chars().next();
            } else if !clean.is_empty() {
                let delimiter = sep_hint.unwrap_or_else(|| guess_delimiter(line));
                if is_header(line, delimiter) {
                    let lower = line.to_lowercase();
                    let moneywiz = sep_hint.is_some()
                        || (lower.contains("current balance")
                            && lower.contains("\"name\"")
                            && lower.contains("account"));
                    return Ok(Body {
                        text,
                        encoding,
                        header_line: i + 1,
                        start_line: i + 1,
                        start_offset: offset,
                        delimiter,
                        moneywiz,
                    });
                }
                first_data.get_or_insert((i + 1, offset));
            }
            offset += line.len();
        }
        let (start_line, start_offset) = first_data.ok_or("the file has no rows")?;
        let first = text[start_offset..].lines().next().unwrap_or("");
        let delimiter = sep_hint.unwrap_or_else(|| guess_delimiter(first));
        Ok(Body {
            text,
            encoding,
            header_line: 0,
            start_line,
            start_offset,
            delimiter,
            moneywiz: false,
        })
    }

    fn reader(&self) -> csv::Reader<&[u8]> {
        csv::ReaderBuilder::new()
            .delimiter(self.delimiter as u8)
            .flexible(true)
            .has_headers(self.header_line > 0)
            .from_reader(self.text[self.start_offset..].as_bytes())
    }

    /// Converts a reader position to a 1-based line number in the file.
    fn line(&self, position: Option<&csv::Position>) -> usize {
        position.map_or(0, |p| self.start_line + p.line() as usize - 1)
    }
}

fn split_line(line: &str, delimiter: char) -> Vec<String> {
    csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .has_headers(false)
        .flexible(true)
        .from_reader(line.as_bytes())
        .records()
        .next()
        .and_then(Result::ok)
        .map(|r| r.iter().map(String::from).collect())
        .unwrap_or_default()
}

fn is_header(line: &str, delimiter: char) -> bool {
    let keys: Vec<String> = split_line(line, delimiter)
        .iter()
        .map(|c| header_key(c))
        .collect();
    let has_date = keys.iter().any(|k| DATE.contains(&k.as_str()));
    let has_money = keys.iter().any(|k| {
        AMOUNT.contains(&k.as_str())
            || DEBIT.contains(&k.as_str())
            || CREDIT.contains(&k.as_str())
            || k.starts_with("amount")
    });
    (has_date && has_money) || Columns::known_layout(&keys).is_some()
}

/// One failing row, for the `first-error` command only.
pub struct ErrorRow {
    pub line: usize,
    pub reason: &'static str,
    pub records_before: usize,
    /// Column name and value pairs. This is raw data: print it, never store it.
    pub fields: Vec<(String, String)>,
}

/// Returns the first row that `load` rejects, or `None` when every row reads.
pub fn first_error(path: &Path, opts: &LoadOptions) -> Result<Option<ErrorRow>, String> {
    let export = load(path, opts)?;
    let Some(&(line, reason)) = export.bad_lines.iter().min_by_key(|b| b.0) else {
        return Ok(None);
    };
    if crate::pdf::is_pdf_path(path) {
        return Ok(Some(crate::pdf::error_row(path, line, reason)?));
    }
    if is_ofx_path(path) {
        return Ok(Some(crate::ofx::error_row(path, line, reason)?));
    }
    let body = Body::read(path)?;
    let mut reader = body.reader();
    let columns: Vec<String> = if body.header_line > 0 {
        reader
            .headers()
            .map_err(|_| "cannot read the header row")?
            .iter()
            .map(String::from)
            .collect()
    } else {
        Vec::new()
    };
    let mut record = csv::StringRecord::new();
    let mut records_before = 0;
    loop {
        let (at, fields) = match reader.read_record(&mut record) {
            Ok(false) => return Ok(None),
            Ok(true) => {
                let names = (0..record.len()).map(|i| {
                    columns
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("column {}", i + 1))
                });
                (
                    body.line(record.position()),
                    names.zip(record.iter().map(String::from)).collect(),
                )
            }
            Err(e) => {
                let at = body.line(e.position());
                let raw = body
                    .text
                    .lines()
                    .nth(at.saturating_sub(1))
                    .unwrap_or("")
                    .to_string();
                (at, vec![("raw line".to_string(), raw)])
            }
        };
        if at == line {
            return Ok(Some(ErrorRow {
                line,
                reason,
                records_before,
                fields,
            }));
        }
        records_before += 1;
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "Account".to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Scotiabank names a download `<Account>_<last4>_<MMDDYY>.csv`; the date and a
/// browser copy suffix (` (1)`) are dropped so every export of one account shares its name.
fn scotia_account(stem: &str) -> &str {
    let stem = match stem.rsplit_once(" (") {
        Some((head, tail))
            if tail.ends_with(')')
                && tail[..tail.len() - 1].bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => stem,
    };
    match stem.rsplit_once('_') {
        Some((head, date)) if date.len() == 6 && date.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => stem,
    }
}

pub fn is_ofx_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "ofx" | "qfx") {
        return true;
    }
    std::fs::read(path)
        .map(|b| {
            let head = String::from_utf8_lossy(&b[..b.len().min(2048)]).to_uppercase();
            head.contains("OFXHEADER") || head.contains("<OFX>")
        })
        .unwrap_or(false)
}

/// Reads one file of any supported format.
pub fn load(path: &Path, opts: &LoadOptions) -> Result<Export, String> {
    if crate::pdf::is_pdf_path(path) {
        return crate::pdf::load(path, opts);
    }
    if is_ofx_path(path) {
        return crate::ofx::load(path, opts);
    }
    let body = Body::read(path)?;
    load_csv(&body, &file_name(path), &file_stem(path), opts)
}

struct Raw {
    line: usize,
    account: String,
    transfer: String,
    group: String,
    category: String,
    date: String,
    amount: String,
    debit: String,
    credit: String,
    kind: String,
    currency: String,
    balance: String,
    description: String,
    /// Private name text. Only its hash is kept, as the payee key.
    key_text: String,
    fingerprint: u64,
}

fn load_csv(body: &Body, name: &str, stem: &str, opts: &LoadOptions) -> Result<Export, String> {
    let mut export = Export::empty(name, if body.moneywiz { "MoneyWiz CSV" } else { "CSV" });
    export.encoding = body.encoding;
    export.delimiter = body.delimiter;
    export.header_line = body.header_line;

    let mut reader = body.reader();
    let mut records: Vec<(usize, csv::StringRecord)> = Vec::new();
    if body.header_line > 0 {
        export.columns = reader
            .headers()
            .map_err(|_| "cannot read the header row".to_string())?
            .iter()
            .map(|h| h.trim().to_string())
            .collect();
    }
    for result in reader.records() {
        match result {
            Ok(r) => records.push((body.line(r.position()), r)),
            Err(e) => export
                .bad_lines
                .push((body.line(e.position()), "unreadable CSV record")),
        }
    }

    let mut cols = if body.header_line > 0 {
        let keys: Vec<String> = export.columns.iter().map(|c| header_key(c)).collect();
        match Columns::known_layout(&keys) {
            Some(cols) => cols,
            None => Columns::from_header(&export.columns, body.moneywiz)?,
        }
    } else if let Some(cols) = Columns::scotiabank_legacy(&records) {
        cols
    } else {
        export.format = "CSV (no header row)".to_string();
        let mut cols = infer_columns(&records)?;
        export.notes.push(format!(
            "no header row; guessed columns: {}",
            cols.describe()
        ));
        let width = records.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
        let td = width == 5
            && cols.date == 0
            && cols.descriptions == [1]
            && (cols.debit, cols.credit, cols.balance) == (Some(2), Some(3), Some(4));
        if td {
            cols.prefer_date = Some("%m/%d/%Y");
            export.notes.push(
                "layout matches td-csv (TD EasyWeb: date, description, debit, credit, balance)"
                    .to_string(),
            );
        }
        cols
    };
    if let Some(label) = cols.label {
        export.format = label.to_string();
    }
    let stem = if cols.label == Some("scotiabank-csv") {
        scotia_account(stem)
    } else {
        stem
    };
    if !opts.columns.is_empty() {
        cols.apply_overrides(&opts.columns)?;
        export
            .notes
            .push(format!("columns set in config: {}", cols.describe()));
    }

    let wanted = |record: &csv::StringRecord| {
        let value = |i: usize| record.get(i).unwrap_or("").trim().to_lowercase();
        cols.keep_if
            .is_none_or(|(i, keep)| keep.contains(&value(i).as_str()))
            && cols
                .skip_if
                .is_none_or(|(i, skip)| !skip.contains(&value(i).as_str()))
    };
    let multi_currency = cols.split_currency
        && cols.currency.is_some_and(|c| {
            let mut seen: Vec<&str> = records
                .iter()
                .filter(|(_, r)| wanted(r))
                .filter_map(|(_, r)| r.get(c).map(str::trim))
                .filter(|s| !s.is_empty())
                .collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len() > 1
        });
    if multi_currency {
        export
            .notes
            .push("several currencies: one account per currency".to_string());
    }

    let mut header_raw: Vec<(String, String)> = Vec::new();
    let mut raws: Vec<Raw> = Vec::new();
    let mut current_account = String::new();
    let mut skipped = 0;
    for (line, record) in &records {
        if !wanted(record) {
            skipped += 1;
            continue;
        }
        let get = |i: Option<usize>| i.and_then(|i| record.get(i)).map_or("", str::trim);
        let account = get(cols.account);
        let name = get(cols.name);
        let date = get(Some(cols.date));
        let (amount, debit, credit) = (get(cols.amount), get(cols.debit), get(cols.credit));
        let has_money = !amount.is_empty() || !debit.is_empty() || !credit.is_empty();

        let shape: Vec<&str> = [
            ("account", account),
            ("name", name),
            ("balance", get(cols.balance)),
            ("transfer", get(cols.transfer)),
            ("category", get(cols.category)),
            ("date", date),
            ("amount", if has_money { "x" } else { "" }),
        ]
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, _)| *k)
        .collect();
        let blank = shape.is_empty() && record.iter().all(|f| f.trim().is_empty());
        let shape = if shape.is_empty() {
            "(no key columns)".to_string()
        } else {
            shape.join("+")
        };
        *export
            .row_shapes
            .entry(if blank { "(blank)".to_string() } else { shape })
            .or_insert(0) += 1;

        if !date.is_empty() && has_money {
            let account = if !account.is_empty() {
                account.to_string()
            } else if !current_account.is_empty() {
                current_account.clone()
            } else {
                opts.account.clone().unwrap_or_else(|| stem.to_string())
            };
            let account = match get(cols.currency) {
                cur if multi_currency && !cur.is_empty() => format!("{account} {cur}"),
                _ => account,
            };
            let description: Vec<&str> = cols
                .descriptions
                .iter()
                .map(|&i| get(Some(i)))
                .filter(|s| !s.is_empty())
                .collect();
            let key_text: Vec<&str> = cols
                .key_columns
                .iter()
                .map(|&i| get(Some(i)))
                .filter(|s| !s.is_empty())
                .collect();
            let mut transfer = get(cols.transfer).to_string();
            if transfer.is_empty()
                && let Some(rest) = description
                    .first()
                    .and_then(|d| d.strip_prefix("Transfer : "))
            {
                transfer = rest.trim().to_string();
            }
            let mut h = DefaultHasher::new();
            account.hash(&mut h);
            for i in [
                Some(cols.date),
                cols.amount,
                cols.debit,
                cols.credit,
                cols.category,
                cols.transfer,
            ]
            .into_iter()
            .flatten()
            .chain(cols.descriptions.iter().copied())
            .chain(cols.private.iter().copied())
            {
                record.get(i).unwrap_or("").trim().hash(&mut h);
            }
            raws.push(Raw {
                line: *line,
                account,
                transfer,
                group: get(cols.category_group).to_string(),
                category: get(cols.category).to_string(),
                date: date.to_string(),
                amount: amount.to_string(),
                debit: debit.to_string(),
                credit: credit.to_string(),
                kind: get(cols.kind).to_lowercase(),
                currency: get(cols.currency).to_string(),
                balance: get(cols.balance).to_string(),
                description: description.join(" "),
                key_text: key_text.join(" "),
                fingerprint: h.finish(),
            });
        } else if cols.moneywiz && !name.is_empty() && date.is_empty() {
            current_account = name.to_string();
            header_raw.push((name.to_string(), get(cols.balance).to_string()));
        } else if !blank {
            export
                .bad_lines
                .push((*line, "neither a transaction nor an account row"));
        }
    }
    if skipped > 0 {
        export.notes.push(format!(
            "{skipped} rows skipped: not completed, or memo rows that do not change the balance"
        ));
    }

    export.date_format_matches = DATE_FORMATS
        .iter()
        .map(|f| {
            (
                f.to_string(),
                raws.iter()
                    .filter(|r| parse_date(&r.date, f).is_some())
                    .count(),
            )
        })
        .collect();
    export.date_format = match &opts.date_format {
        Some(f) => f.clone(),
        None if !cols.date_formats.is_empty() => cols.date_formats.join(" or "),
        None if raws.is_empty() => DATE_FORMATS[0].to_string(),
        None => match pick_date_format(&export.date_format_matches) {
            Ok(f) => f,
            Err(e) => {
                let best = export
                    .date_format_matches
                    .iter()
                    .map(|m| m.1)
                    .max()
                    .unwrap_or(0);
                let preferred = export
                    .date_format_matches
                    .iter()
                    .find(|m| Some(m.0.as_str()) == cols.prefer_date && m.1 == best && best > 0);
                preferred.map(|m| m.0.clone()).ok_or(e)?
            }
        },
    };
    let row_formats: Vec<String> = match &opts.date_format {
        None if !cols.date_formats.is_empty() => {
            cols.date_formats.iter().map(|f| f.to_string()).collect()
        }
        _ => vec![export.date_format.clone()],
    };
    export.decimal = detect_decimal(
        raws.iter()
            .flat_map(|r| [r.amount.as_str(), r.debit.as_str(), r.credit.as_str()])
            .filter(|s| !s.is_empty()),
    );
    export.category_separator = match &opts.category_separator {
        Some(s) => s.clone(),
        None => detect_separator(raws.iter().map(|r| r.category.as_str())),
    };
    for (name, raw) in header_raw {
        if let Some(cents) = parse_cents(&raw, export.decimal) {
            export.balances.insert(
                name,
                Balance {
                    cents,
                    as_of: None,
                    source: "account row",
                },
            );
        }
    }

    // Signs: a type column wins; otherwise flip when purchases look positive.
    let single_amount = cols.amount.is_some()
        && cols.debit.is_none()
        && cols.credit.is_none()
        && cols.kind.is_none();
    let parsed: Vec<Option<i64>> = raws.iter().map(|r| amount_of(r, export.decimal)).collect();
    let positive = parsed.iter().flatten().filter(|c| **c > 0).count();
    let nonzero = parsed.iter().flatten().filter(|c| **c != 0).count();
    let mostly_positive =
        !body.moneywiz && single_amount && nonzero >= 5 && positive * 10 > nonzero * 7;
    let card_like = {
        let mut names: Vec<&str> = raws.iter().map(|r| r.account.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        !names.is_empty()
            && names.iter().all(|n| {
                crate::detect::guess_class(n, None, "", "").0 == crate::detect::Class::Card
            })
    };
    let invert = match opts.invert {
        Some(v) => v,
        None => mostly_positive && card_like,
    };
    if invert {
        export
            .notes
            .push("signs flipped: purchases were exported as positive amounts".to_string());
    } else if mostly_positive && opts.invert.is_none() {
        export.notes.push(format!(
            "most amounts are positive; if purchases show as positive, set invert = true under [files.\"{name}\"]"
        ));
    }

    let mut running: BTreeMap<String, Vec<(NaiveDate, usize, i64)>> = BTreeMap::new();
    for (index, (r, cents)) in raws.into_iter().zip(parsed).enumerate() {
        let Some(date) = row_formats.iter().find_map(|f| parse_date(&r.date, f)) else {
            export.bad_lines.push((r.line, "date not parseable"));
            continue;
        };
        let Some(mut cents) = cents else {
            export.bad_lines.push((r.line, "amount not parseable"));
            continue;
        };
        if invert {
            cents = -cents;
        }
        if r.category.contains(';') || (r.category.contains('|') && body.delimiter != '|') {
            export.split_candidates += 1;
        }
        let mut derived = text::derive(&r.description, cents);
        let named = text::derive(&r.key_text, cents).payee_key;
        if named != 0 {
            derived.payee_key = named;
        }
        if derived.payee_key != 0 {
            export
                .payee_labels
                .entry(derived.payee_key)
                .or_insert_with(|| r.description.chars().take(40).collect());
        }
        let mut category = split_category(&r.category, &export.category_separator);
        if !r.group.is_empty() && !category.is_empty() {
            category.insert(0, r.group.clone());
        }
        let category_from_file = !category.is_empty();
        if category.is_empty()
            && r.transfer.is_empty()
            && let Some(guess) = derived.category
        {
            category = vec![guess.to_string()];
        }
        if let Some(balance) = parse_cents(&r.balance, export.decimal) {
            running
                .entry(r.account.clone())
                .or_default()
                .push((date, index, balance));
        }
        export.txns.push(Txn {
            file: 0,
            account: r.account,
            transfer: r.transfer,
            category,
            category_from_file,
            hint: derived.hint,
            payee_key: derived.payee_key,
            date,
            cents,
            currency: if r.currency.is_empty() {
                opts.currency.clone().unwrap_or_default()
            } else {
                r.currency
            },
            fingerprint: r.fingerprint,
            text: r.description,
        });
    }

    for (account, rows) in running {
        let ascending = rows.first().map(|f| f.0) <= rows.last().map(|l| l.0);
        let latest = rows.iter().map(|r| r.0).max().expect("non-empty");
        let pick = rows
            .iter()
            .filter(|r| r.0 == latest)
            .max_by_key(|r| if ascending { r.1 as i64 } else { -(r.1 as i64) })
            .expect("non-empty");
        export.balances.entry(account).or_insert(Balance {
            cents: pick.2,
            as_of: Some(latest),
            source: "running balance",
        });
    }
    Ok(export)
}

/// Signed cents from an amount column, debit/credit columns or a type column.
fn amount_of(r: &Raw, decimal: char) -> Option<i64> {
    if !r.amount.is_empty() {
        let cents = parse_cents(&r.amount, decimal)?;
        let debit_kind = ["debit", "dr", "withdrawal", "purchase", "payment", "sale"]
            .iter()
            .any(|k| r.kind.starts_with(k));
        return Some(if debit_kind && cents > 0 {
            -cents
        } else {
            cents
        });
    }
    let out = if r.debit.is_empty() {
        Some(0)
    } else {
        parse_cents(&r.debit, decimal).map(i64::abs)
    };
    let inn = if r.credit.is_empty() {
        Some(0)
    } else {
        parse_cents(&r.credit, decimal).map(i64::abs)
    };
    Some(inn? - out?)
}

/// Guesses roles for files without a header row from the cell contents.
fn infer_columns(records: &[(usize, csv::StringRecord)]) -> Result<Columns, String> {
    let sample: Vec<&csv::StringRecord> = records.iter().take(200).map(|(_, r)| r).collect();
    let width = sample.iter().map(|r| r.len()).max().unwrap_or(0);
    let is_number = |s: &str| {
        let t = s.trim();
        !t.is_empty()
            && t.chars().any(|c| c.is_ascii_digit())
            && t.chars()
                .all(|c| c.is_ascii_digit() || " .,-+()$€£¥\u{2212}".contains(c))
    };
    #[derive(Clone, Copy, PartialEq)]
    enum Cell {
        Empty,
        Date,
        Number,
        Text,
    }
    let mut date = None;
    let mut kinds = Vec::with_capacity(width);
    for i in 0..width {
        let cells: Vec<&str> = sample
            .iter()
            .map(|r| r.get(i).unwrap_or("").trim())
            .filter(|s| !s.is_empty())
            .collect();
        let dates = cells
            .iter()
            .filter(|c| DATE_FORMATS.iter().any(|f| parse_date(c, f).is_some()))
            .count();
        let kind = if cells.is_empty() {
            Cell::Empty
        } else if dates * 10 >= cells.len() * 8 {
            // The first mostly-filled date column is the transaction date;
            // later date columns (posting dates) are ignored.
            if date.is_none() && cells.len() * 10 >= sample.len() * 8 {
                date = Some(i);
            }
            Cell::Date
        } else if cells.iter().all(|c| is_number(c)) {
            Cell::Number
        } else {
            Cell::Text
        };
        kinds.push(kind);
    }
    // An empty column beside a number column is a debit or credit column
    // with no rows (for example withdrawals in a deposits-only account).
    let last_data = kinds.iter().rposition(|k| *k != Cell::Empty).unwrap_or(0);
    let numbers: Vec<usize> = (0..width)
        .filter(|&i| match kinds[i] {
            Cell::Number => true,
            Cell::Empty => {
                i < last_data
                    && (kinds.get(i + 1) == Some(&Cell::Number)
                        || (i > 0 && kinds[i - 1] == Cell::Number))
            }
            _ => false,
        })
        .collect();
    let texts: Vec<usize> = (0..width).filter(|&i| kinds[i] == Cell::Text).collect();
    let date = date.ok_or("no header row, and no column looks like dates")?;
    let exclusive = |a: usize, b: usize| {
        let both = sample
            .iter()
            .filter(|r| {
                !r.get(a).unwrap_or("").trim().is_empty()
                    && !r.get(b).unwrap_or("").trim().is_empty()
            })
            .count();
        both * 10 < sample.len()
    };
    let mut cols = Columns {
        date,
        descriptions: texts.first().copied().into_iter().collect(),
        ..Columns::default()
    };
    match numbers.as_slice() {
        [] => return Err("no header row, and no column looks like amounts".to_string()),
        [a] => cols.amount = Some(*a),
        [a, b] if exclusive(*a, *b) => (cols.debit, cols.credit) = (Some(*a), Some(*b)),
        [a, b] => (cols.amount, cols.balance) = (Some(*a), Some(*b)),
        [a, b, c, ..] if exclusive(*a, *b) => {
            (cols.debit, cols.credit, cols.balance) = (Some(*a), Some(*b), Some(*c))
        }
        [a, b, ..] => (cols.amount, cols.balance) = (Some(*a), Some(*b)),
    }
    cols.private = texts.iter().skip(1).copied().collect();
    Ok(cols)
}

/// Splits a category into trimmed path parts. An empty separator means flat.
pub fn split_category(raw: &str, separator: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    if separator.is_empty() {
        return vec![raw.trim().to_string()];
    }
    raw.split(separator)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// Parses a date and rejects implausible years, so `%Y` does not swallow
/// a two-digit year.
pub fn parse_date(raw: &str, format: &str) -> Option<NaiveDate> {
    let (date, rest) = NaiveDate::parse_and_remainder(raw.trim(), format).ok()?;
    let ok = (rest.is_empty() || rest.starts_with([' ', 'T', ',']))
        && (1970..=2100).contains(&date.year());
    ok.then_some(date)
}

fn pick_date_format(matches: &[(String, usize)]) -> Result<String, String> {
    let best = matches.iter().map(|m| m.1).max().unwrap_or(0);
    if best == 0 {
        return Err(
            "no known date format matches the date column; set date_format for this file".into(),
        );
    }
    let winners: Vec<&str> = matches
        .iter()
        .filter(|m| m.1 == best)
        .map(|m| m.0.as_str())
        .collect();
    match winners.as_slice() {
        [one] => Ok((*one).to_string()),
        many => Err(format!(
            "dates match several formats equally ({}); set date_format for this file",
            many.join(" or ")
        )),
    }
}

fn detect_separator<'a>(categories: impl Iterator<Item = &'a str>) -> String {
    let categories: Vec<&str> = categories.filter(|c| !c.is_empty()).collect();
    let mut best = ("", 0usize);
    for sep in CATEGORY_SEPARATORS {
        let n = categories.iter().filter(|c| c.contains(sep)).count();
        if n > best.1 {
            best = (sep, n);
        }
    }
    best.0.to_string()
}

fn guess_delimiter(line: &str) -> char {
    [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|d| (line.matches(*d).count(), *d == ','))
        .unwrap_or(',')
}

pub fn decode(bytes: &[u8]) -> (String, &'static str) {
    fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| word([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), "UTF-8 with BOM");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (utf16(rest, u16::from_le_bytes), "UTF-16 LE");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (utf16(rest, u16::from_be_bytes), "UTF-16 BE");
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), "UTF-8"),
        // Russian banks (Tinkoff) export in Windows-1251.
        Err(_) if cp1251(bytes).to_lowercase().contains("дата") => {
            (cp1251(bytes), "Windows-1251")
        }
        Err(_) => (
            bytes.iter().map(|&b| b as char).collect(),
            "Latin-1 (fallback)",
        ),
    }
}

/// Windows-1251: Cyrillic letters, Ё and ё; other bytes as Latin-1.
fn cp1251(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0xC0..=0xFF => char::from_u32(0x0410 + u32::from(b - 0xC0)).unwrap_or('?'),
            0xA8 => 'Ё',
            0xB8 => 'ё',
            0xB9 => '№',
            _ => b as char,
        })
        .collect()
}
