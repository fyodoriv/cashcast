//! The `totals` report: sums per account and per category. No rows.

use std::collections::BTreeMap;
use std::fmt::Write;

use serde::Serialize;

use crate::inbox::Inbox;

/// Sums per account and per category, in each row's own currency.
/// Used to classify accounts and categories. Sums only, no rows.
#[derive(Serialize)]
pub struct Totals {
    pub accounts: Vec<AccountTotals>,
    pub categories: Vec<CategoryTotals>,
}

#[derive(Serialize)]
pub struct AccountTotals {
    pub name: String,
    /// The account's most common currency.
    pub currency: String,
    pub rows: usize,
    pub in_cents: i64,
    pub out_cents: i64,
    /// Net transfer amount per counterpart account.
    pub transfers: BTreeMap<String, i64>,
}

#[derive(Serialize)]
pub struct CategoryTotals {
    pub path: String,
    pub currency: String,
    pub rows: usize,
    pub in_cents: i64,
    pub out_cents: i64,
}

pub fn totals(inbox: &Inbox) -> Totals {
    let mut accounts: BTreeMap<&str, (BTreeMap<&str, usize>, AccountTotals)> = BTreeMap::new();
    let mut categories: BTreeMap<(String, &str), CategoryTotals> = BTreeMap::new();
    for t in &inbox.txns {
        let (currencies, a) = accounts.entry(t.account.as_str()).or_insert_with(|| {
            let blank = AccountTotals {
                name: t.account.clone(),
                currency: String::new(),
                rows: 0,
                in_cents: 0,
                out_cents: 0,
                transfers: BTreeMap::new(),
            };
            (BTreeMap::new(), blank)
        });
        *currencies.entry(t.currency.as_str()).or_insert(0) += 1;
        a.rows += 1;
        if t.cents > 0 {
            a.in_cents += t.cents
        } else {
            a.out_cents += t.cents
        }
        if !t.transfer.is_empty() {
            *a.transfers.entry(t.transfer.clone()).or_insert(0) += t.cents;
            continue;
        }
        let path = if t.category.is_empty() {
            "(Uncategorized)".to_string()
        } else {
            t.category.join(" > ")
        };
        let c = categories
            .entry((path.clone(), t.currency.as_str()))
            .or_insert_with(|| CategoryTotals {
                path,
                currency: t.currency.clone(),
                rows: 0,
                in_cents: 0,
                out_cents: 0,
            });
        c.rows += 1;
        if t.cents > 0 {
            c.in_cents += t.cents
        } else {
            c.out_cents += t.cents
        }
    }
    let accounts = accounts
        .into_values()
        .map(|(currencies, mut a)| {
            a.currency = currencies
                .iter()
                .max_by_key(|(_, n)| **n)
                .map_or("", |(c, _)| *c)
                .to_string();
            a
        })
        .collect();
    Totals {
        accounts,
        categories: categories.into_values().collect(),
    }
}

pub fn render_totals(t: &Totals) -> String {
    use crate::money::fmt_cents;
    let mut s = String::new();
    let _ = writeln!(s, "# Totals per account (own currency, whole export)\n");
    let _ = writeln!(
        s,
        "| Account | Cur | Rows | In | Out | Net transfers by counterpart |"
    );
    let _ = writeln!(s, "|---|---|---:|---:|---:|---|");
    for a in &t.accounts {
        let transfers: Vec<String> = a
            .transfers
            .iter()
            .map(|(k, v)| format!("{k} {}", fmt_cents(*v)))
            .collect();
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} |",
            a.name,
            a.currency,
            a.rows,
            fmt_cents(a.in_cents),
            fmt_cents(a.out_cents),
            transfers.join("; ")
        );
    }
    let _ = writeln!(s, "\n# Totals per category (non-transfer rows)\n");
    let _ = writeln!(s, "| Category | Cur | Rows | In | Out |");
    let _ = writeln!(s, "|---|---|---:|---:|---:|");
    for c in &t.categories {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} |",
            c.path,
            c.currency,
            c.rows,
            fmt_cents(c.in_cents),
            fmt_cents(c.out_cents)
        );
    }
    s
}
