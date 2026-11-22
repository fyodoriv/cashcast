//! Compares workbook values with the model, month by month, to the cent,
//! as `forecast/verify.py`.

use super::book::{Book, col_letter};
use super::formula::{Eval, Val, dec_of};
use super::model::{Failure, Row};
use rust_decimal::{Decimal, RoundingStrategy};
use std::path::Path;

pub const REQUIRED: [&str; 4] = ["Net cash flow", "Month start", "To invest", "Month end"];
pub const EXTRA: [&str; 4] = ["Paycheques", "Net salary", "Extra income", "Expenses"];

fn names() -> impl Iterator<Item = &'static str> {
    REQUIRED.into_iter().chain(EXTRA)
}

/// Python `cents(value)`: half up.
pub fn cents(v: Decimal) -> i64 {
    (v * Decimal::from(100))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .try_into()
        .unwrap_or(0)
}

/// One row per month: each column's value in cents (None: not a number).
pub type Actual = Vec<Vec<(&'static str, Option<i64>)>>;

/// Evaluates the Forecast sheet's formulas, as pycel does.
pub fn from_book(book: &Book, months: i64) -> Result<Actual, Failure> {
    let sheet = book
        .sheet("Forecast")
        .ok_or_else(|| Failure("no Forecast sheet".into()))?;
    let header: Vec<String> = (1..)
        .map_while(|c| match sheet.get(1, c) {
            Some(super::book::V::Str(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let e = Eval::new(book);
    let mut rows = Vec::new();
    for r in 2..2 + months as u32 {
        let mut row = Vec::new();
        for name in names() {
            let col = header
                .iter()
                .position(|h| h == name)
                .ok_or_else(|| Failure(format!("no column {name}")))?;
            let v = match e.cell("Forecast", &format!("{}{r}", col_letter(col as u32 + 1))) {
                Val::Num(x) => cents(dec_of(x)),
                Val::Empty => 0,
                // pycel would fail here with a traceback; Python handles it.
                other => return Err(Failure(format!("{other:?}"))),
            };
            row.push((name, Some(v)));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Reads the values Numbers computed (written by to_numbers.applescript).
pub fn from_numbers(path: &Path, months: i64) -> Result<Actual, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure(e.to_string()))?;
    let lines: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').collect())
        .collect();
    let header = lines.first().ok_or_else(|| Failure("IndexError".into()))?;
    let mut rows = Vec::new();
    for line in lines.iter().skip(1).take(months as usize) {
        let mut row = Vec::new();
        for name in names() {
            let i = header
                .iter()
                .position(|h| *h == name)
                .ok_or_else(|| Failure("ValueError".into()))?;
            let raw = line.get(i).ok_or_else(|| Failure("IndexError".into()))?;
            let v = match raw.strip_prefix("c:") {
                Some(n) => Some(n.parse::<i64>().map_err(|_| Failure("ValueError".into()))?),
                None => None,
            };
            row.push((name, v));
        }
        rows.push(row);
    }
    Ok(rows)
}

fn model_value(r: &Row, name: &str) -> i64 {
    match name {
        "Paycheques" => r.paycheques * 100,
        "Net salary" => cents(r.net_salary),
        "Extra income" => cents(r.extra_income),
        "Expenses" => cents(r.expenses),
        "Net cash flow" => cents(r.net_cash_flow),
        "Month start" => cents(r.month_start),
        "To invest" => cents(r.to_invest),
        _ => cents(r.month_end),
    }
}

/// One line per difference; an empty list means a match. Python's `zip(strict=True)`
/// fails on a length mismatch; that cannot happen with the same horizon.
pub fn compare(expected: &[Row], actual: &Actual) -> Vec<String> {
    let mut out = Vec::new();
    for (exp, act) in expected.iter().zip(actual) {
        for (name, got) in act {
            let want = model_value(exp, name);
            if *got != Some(want) {
                let level = if REQUIRED.contains(name) {
                    "MISMATCH"
                } else {
                    "differs"
                };
                let shown = got.map_or("None".to_string(), |v| v.to_string());
                out.push(format!(
                    "{level} {} {name}: workbook {shown} vs model {want} (cents)",
                    exp.month.format("%Y-%m")
                ));
            }
        }
    }
    out
}
