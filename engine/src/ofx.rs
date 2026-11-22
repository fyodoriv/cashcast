//! Reads OFX and QFX files: bank and credit card statements.
//!
//! Works for SGML (OFX 1.x, leaf tags without closing tags) and XML
//! (OFX 2.x). Account numbers are masked to their last four characters.
//! Payee and memo text go through `text::derive` and are then dropped.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use chrono::NaiveDate;

use crate::export::{AccountKind, Balance, ErrorRow, Export, LoadOptions, Txn, decode, file_name};
use crate::money::parse_cents;
use crate::text;

pub fn load(path: &Path, opts: &LoadOptions) -> Result<Export, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", file_name(path)))?;
    let (text, encoding) = decode(&bytes);
    let mut export = parse(&text, &file_name(path), opts)?;
    export.encoding = encoding;
    Ok(export)
}

pub fn parse(text: &str, name: &str, opts: &LoadOptions) -> Result<Export, String> {
    let upper = text.to_ascii_uppercase();
    let mut export = Export::empty(name, "OFX");
    let mut statements = 0;
    for (open, close, kind) in [
        ("<STMTRS>", "</STMTRS>", AccountKind::Bank),
        ("<CCSTMTRS>", "</CCSTMTRS>", AccountKind::Card),
    ] {
        let mut pos = 0;
        while let Some(found) = upper[pos..].find(open) {
            let start = pos + found;
            let end = upper[start..]
                .find(close)
                .map_or(upper.len(), |e| start + e);
            statement(text, &upper, start, end, kind, opts, &mut export);
            statements += 1;
            pos = end.max(start + open.len());
        }
    }
    if statements == 0 {
        return Err("no bank or credit card statement found in this OFX file".into());
    }
    export.notes.push(format!("{statements} statement(s)"));
    if let Some(org) = value(text, &upper, 0, upper.len(), "ORG") {
        export.notes.push(format!("institution: {org}"));
    }
    Ok(export)
}

/// Returns the text after `<TAG>` up to the next tag or line break.
fn value<'a>(text: &'a str, upper: &str, from: usize, to: usize, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let at = from + upper[from..to].find(&open)? + open.len();
    let end = upper[at..to].find(['<', '\n', '\r']).map_or(to, |e| at + e);
    let v = text[at..end].trim();
    (!v.is_empty()).then_some(v)
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

fn parse_ofx_date(v: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()
}

fn parse_ofx_amount(v: &str) -> Option<i64> {
    let decimal = if v.contains(',') && !v.contains('.') {
        ','
    } else {
        '.'
    };
    parse_cents(v, decimal)
}

fn statement(
    text: &str,
    upper: &str,
    start: usize,
    end: usize,
    kind: AccountKind,
    opts: &LoadOptions,
    export: &mut Export,
) {
    let id = value(text, upper, start, end, "ACCTID").unwrap_or("");
    let tail: String = id
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let acct_type = value(text, upper, start, end, "ACCTTYPE")
        .unwrap_or("")
        .to_ascii_uppercase();
    let (label, kind) = match (kind, acct_type.as_str()) {
        (AccountKind::Card, _) => ("Credit card", AccountKind::Card),
        (_, "SAVINGS") => ("Savings", AccountKind::Bank),
        (_, "MONEYMRKT") => ("Money market", AccountKind::Bank),
        (_, "CREDITLINE") => ("Credit line", AccountKind::Card),
        _ => ("Checking", AccountKind::Bank),
    };
    let account = opts
        .account
        .clone()
        .unwrap_or_else(|| format!("{label} …{tail}"));
    let currency = value(text, upper, start, end, "CURDEF")
        .map_or_else(|| opts.currency.clone().unwrap_or_default(), String::from);
    export.account_kinds.insert(account.clone(), kind);

    if let Some(at) = upper[start..end].find("<LEDGERBAL>") {
        let from = start + at;
        let to = upper[from..end]
            .find("</LEDGERBAL>")
            .map_or(end, |e| from + e);
        if let Some(cents) = value(text, upper, from, to, "BALAMT").and_then(parse_ofx_amount) {
            let as_of = value(text, upper, from, to, "DTASOF").and_then(parse_ofx_date);
            export.balances.insert(
                account.clone(),
                Balance {
                    cents,
                    as_of,
                    source: "statement balance",
                },
            );
        }
    }

    let mut pos = start;
    while let Some(found) = upper[pos..end].find("<STMTTRN>") {
        let from = pos + found;
        let to = upper[from..end]
            .find("</STMTTRN>")
            .map_or(end, |e| from + e);
        pos = to.max(from + 9);
        let line = line_of(text, from);
        let Some(date) = value(text, upper, from, to, "DTPOSTED").and_then(parse_ofx_date) else {
            export.bad_lines.push((line, "date not parseable"));
            continue;
        };
        let Some(cents) = value(text, upper, from, to, "TRNAMT").and_then(parse_ofx_amount) else {
            export.bad_lines.push((line, "amount not parseable"));
            continue;
        };
        let name = value(text, upper, from, to, "NAME").unwrap_or("");
        let memo = value(text, upper, from, to, "MEMO").unwrap_or("");
        let description = format!("{name} {memo}");
        let derived = text::derive(&description, cents);
        if derived.payee_key != 0 {
            export
                .payee_labels
                .entry(derived.payee_key)
                .or_insert_with(|| description.trim().chars().take(40).collect());
        }
        let mut h = DefaultHasher::new();
        account.hash(&mut h);
        match value(text, upper, from, to, "FITID") {
            Some(fitid) => fitid.hash(&mut h),
            None => (date, cents, &description).hash(&mut h),
        }
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
            cents,
            currency: currency.clone(),
            fingerprint: h.finish(),
            text: description.trim().to_string(),
        });
    }
}

/// The first transaction block that fails to read, for `first-error` only.
/// Returns that one block, so a single-line file never shows more.
pub fn error_row(path: &Path, _line: usize, _reason: &'static str) -> Result<ErrorRow, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", file_name(path)))?;
    let (text, _) = decode(&bytes);
    let upper = text.to_ascii_uppercase();
    let (mut pos, mut records_before) = (0, 0);
    while let Some(found) = upper[pos..].find("<STMTTRN>") {
        let from = pos + found;
        let to = upper[from..]
            .find("</STMTTRN>")
            .map_or(upper.len(), |e| from + e);
        pos = to.max(from + 9);
        let date_ok = value(&text, &upper, from, to, "DTPOSTED")
            .and_then(parse_ofx_date)
            .is_some();
        let amount_ok = value(&text, &upper, from, to, "TRNAMT")
            .and_then(parse_ofx_amount)
            .is_some();
        if !(date_ok && amount_ok) {
            let reason = if date_ok {
                "amount not parseable"
            } else {
                "date not parseable"
            };
            let block = text[from..(to + 10).min(text.len())]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            return Ok(ErrorRow {
                line: line_of(&text, from),
                reason,
                records_before,
                fields: vec![("raw block".to_string(), block)],
            });
        }
        records_before += 1;
    }
    Err("no failing transaction block found".to_string())
}
