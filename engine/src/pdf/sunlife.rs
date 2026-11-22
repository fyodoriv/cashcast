//! Sun Life group retirement statements (half-year PDFs): the units each
//! fund holds on the statement's last day, from the "Your investments"
//! table (fund, number of units, price on, value on), which may run over a
//! page break. The statement lists no transactions, so it checks MoneyWiz's
//! units per fund on that day (`moneywiz_invest::statement_checks`); the
//! transaction history CSV gives the rows, for 12 months only.
//!
//! Safety: each fund's units times price must give its value (within a
//! cent per unit of rounding), and the values must add up to "Total
//! investments", or the file is refused.

use std::fmt::Write as _;
use std::path::Path;

use chrono::NaiveDate;

use super::{full_dates, line_shape};
use crate::export::Export;

pub const FORMAT: &str = "sunlife-statement-pdf";

const HEADER: &str = "number of units";
const TOTAL: &str = "total investments";

/// One fund on the statement's day.
#[derive(Clone, Debug, PartialEq)]
pub struct Fund {
    /// The fund name as `ledger` labels a Sun Life fund.
    pub name: String,
    pub units: f64,
    pub value_cents: i64,
}

/// What a statement holds on its last day.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub file: String,
    /// The plan type, as `ledger` names a Sun Life bucket ("RRSP").
    pub plan: String,
    pub as_of: NaiveDate,
    pub funds: Vec<Fund>,
    pub total_cents: i64,
}

/// True for a Sun Life statement's text.
pub fn is_statement(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("sun life") && lower.contains(HEADER) && lower.contains(TOTAL)
}

/// The cells of a line, split by two or more spaces.
fn cells(line: &str) -> Vec<&str> {
    line.split("  ")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect()
}

/// A plain number: "1,234.5678", "$12.34" or "($5.00)".
fn number(cell: &str) -> Option<f64> {
    let t = cell.trim();
    let negative = t.starts_with('(') && t.ends_with(')') || t.starts_with('-');
    let digits: String = t
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty()
        || !t
            .chars()
            .all(|c| c.is_ascii_digit() || ".,$()- ".contains(c))
    {
        return None;
    }
    let v: f64 = digits.parse().ok()?;
    Some(if negative { -v } else { v })
}

fn cents(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

/// The plan type a statement is for: the plan its title lines name (a line
/// that is only the plan's name), else the one it mentions most. Notes may
/// name other plans in passing. Ties and none: RRSP.
fn plan_of(text: &str) -> &'static str {
    const PLANS: [(&str, &[&str]); 3] = [
        ("RRSP", &["retirement savings", "rrsp"]),
        ("TFSA", &["tax-free savings", "tfsa"]),
        ("DPSP", &["deferred profit", "dpsp"]),
    ];
    let mut score = [0usize; 3];
    for line in text.lines() {
        let l = line.trim().to_lowercase();
        for (i, (_, keys)) in PLANS.iter().enumerate() {
            let n: usize = keys.iter().map(|k| l.matches(k).count()).sum();
            // A title: the plan's name alone on its line.
            let title = n > 0 && l.split_whitespace().count() <= 5 && !l.contains('.');
            score[i] += if title { 10 * n } else { n };
        }
    }
    let best = (0..3).max_by_key(|&i| (score[i], i == 0)).unwrap_or(0);
    if score[best] == 0 {
        "RRSP"
    } else {
        PLANS[best].0
    }
}

/// The investments table: one snapshot per statement.
pub fn read(text: &str, file: &str) -> Result<Snapshot, String> {
    let mut as_of: Option<NaiveDate> = None;
    let mut funds: Vec<Fund> = Vec::new();
    let mut total: Option<i64> = None;
    let mut inside = false;
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains(HEADER) {
            let days: Vec<NaiveDate> = full_dates(line).iter().map(|(_, d)| *d).collect();
            if days.windows(2).any(|w| w[0] != w[1]) {
                return Err("an investments header with two dates".into());
            }
            let day = days.last().copied();
            match (as_of, day) {
                (_, None) => return Err("an investments header without its date".into()),
                (Some(a), Some(d)) if a != d => {
                    return Err("two investments tables with different dates".into());
                }
                _ => as_of = day,
            }
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        let c = cells(line);
        if lower.trim_start().starts_with(TOTAL) {
            total = c.last().and_then(|v| number(v)).map(cents);
            break;
        }
        // A fund: its name, then units, price and value.
        if c.len() >= 4 {
            let n = c.len();
            let (units, price, value) = (number(c[n - 3]), number(c[n - 2]), number(c[n - 1]));
            if let (Some(units), Some(price), Some(value)) = (units, price, value) {
                if !c[n - 2].contains('$') {
                    return Err("a fund row without its price".into());
                }
                let name = crate::ledger::fund_label(&c[..n - 3].join(" "));
                // Each unit's price is rounded: allow a cent per unit, at least a dollar.
                if (cents(units * price) - cents(value)).abs() > (units.abs() as i64).max(100) {
                    return Err(format!("{name}: units times price is not its value"));
                }
                funds.push(Fund {
                    name,
                    units,
                    value_cents: cents(value),
                });
            }
        }
    }
    let as_of = as_of.ok_or("no investments table")?;
    let total = total.ok_or("no total investments line")?;
    if funds.is_empty() {
        return Err("no funds in the investments table".into());
    }
    let sum: i64 = funds.iter().map(|f| f.value_cents).sum();
    if (sum - total).abs() > funds.len() as i64 {
        return Err("the funds do not add up to total investments".into());
    }
    Ok(Snapshot {
        file: file.to_string(),
        plan: plan_of(text).to_string(),
        as_of,
        funds,
        total_cents: total,
    })
}

/// The statement as an export: no rows (the plan holds no cash), a note
/// with the day and the count of funds.
pub fn parse(text: &str, name: &str) -> Result<Export, String> {
    let s = read(text, name)?;
    let mut export = Export::empty(name, FORMAT);
    export.notes.push(format!(
        "{} fund(s) on {} ({}): checks MoneyWiz units (moneywiz diagnose)",
        s.funds.len(),
        s.as_of,
        s.plan
    ));
    Ok(export)
}

/// Every Sun Life statement in a folder, oldest first. Files that fail
/// to read are left out (`columns` says why).
pub fn snapshots(dir: &Path) -> Vec<Snapshot> {
    let Ok(files) = crate::inbox::inbox_files(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Snapshot> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        .filter_map(|p| {
            let text = super::text_of(p).ok()?;
            if !is_statement(&text) {
                return None;
            }
            read(&text, &crate::export::file_name(p)).ok()
        })
        .collect();
    out.sort_by_key(|s| s.as_of);
    out.dedup_by_key(|s| (s.as_of, s.plan.clone()));
    out
}

/// The layout without values: what was read, and each table line's shape.
pub fn columns_text(text: &str, name: &str) -> String {
    let result = match read(text, name) {
        Ok(s) => format!("{} fund(s) on {} ({})", s.funds.len(), s.as_of, s.plan),
        Err(e) => format!("refused: {e}"),
    };
    let mut out = format!("- {name} ({FORMAT}): {result}\n");
    let mut inside = false;
    for (i, line) in text.lines().enumerate() {
        let lower = line.to_lowercase();
        if lower.contains(HEADER) {
            inside = true;
        }
        if inside && !line.trim().is_empty() {
            let _ = writeln!(out, "  line {}: {}", i + 1, line_shape(line));
        }
        if lower.trim_start().starts_with(TOTAL) {
            break;
        }
    }
    out
}
