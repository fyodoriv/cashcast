//! End-to-end checks on a fake export. `sample.csv` holds no real data.

use std::path::Path;

use cashcast_engine::{config, export, inbox::Inbox, inspect, summarize};
use chrono::NaiveDate;

const CONFIG: &str = r#"
[window]
first_month = "2026-05"
last_month = "2026-09"

[accounts]
"Bank A Chequing" = "cash"
"Bank B Chequing" = "cash"
"Card A" = "card"
"TFSA" = "excluded"
"Brokerage" = "excluded"
"Group RRSP" = "excluded"

[categories]
salary = ["Employer > Salary"]
cashback = ["Rewards > Cashback"]
gift = ["Gifts received"]
income_tax = ["Taxes > Income Tax"]

[transfers]
from_excluded = { "Brokerage" = "rsu_espp" }

[pay]
schedule = "biweekly"
anchor = "2026-05-08"
"#;

fn load() -> Inbox {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.csv");
    Inbox::single(export::load(&path, &export::LoadOptions::default()).expect("fixture loads"))
}

fn run() -> (summarize::Summary, String) {
    let cfg = config::parse(CONFIG).expect("config parses");
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let s = summarize::summarize(&load(), &cfg, today).expect("summarizes");
    let text = summarize::render(&s);
    (s, text)
}

fn series<'a>(map: &'a std::collections::BTreeMap<String, Vec<i64>>, key: &str) -> &'a [i64] {
    map.get(key)
        .unwrap_or_else(|| panic!("missing series {key}"))
}

#[test]
fn no_private_text_reaches_any_output() {
    let totals = inspect::totals(&load());
    let totals_json = serde_json::to_string(&totals).unwrap();
    let totals_text = inspect::render_totals(&totals);
    let (s, summary_text) = run();
    let summary_json = serde_json::to_string(&s).unwrap();
    for out in [&totals_json, &totals_text, &summary_json, &summary_text] {
        assert!(!out.contains("CANARY"), "private text leaked into output");
        assert!(!out.contains("09:00"), "time column leaked into output");
    }
}

#[test]
fn reads_moneywiz_shape() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.csv");
    let e = export::load(&path, &export::LoadOptions::default()).unwrap();
    assert_eq!(e.format, "MoneyWiz CSV");
    assert_eq!(e.header_line, 2);
    assert_eq!(e.columns.len(), 14);
    assert_eq!(e.txns.len(), 30);
    assert!(e.bad_lines.is_empty());
    assert_eq!(e.date_format, "%Y-%m-%d");
    assert_eq!(e.category_separator, "►");
    assert_eq!(e.balances.len(), 6);
    let food: Vec<&Vec<String>> = e
        .txns
        .iter()
        .map(|t| &t.category)
        .filter(|c| c.first().is_some_and(|f| f == "Food"))
        .collect();
    assert_eq!(food.len(), 6);
    assert!(
        e.txns
            .iter()
            .all(|t| t.category.is_empty() || t.category_from_file)
    );
}

#[test]
fn paycheques_group_into_cycles_with_half_year_medians() {
    let (s, _) = run();
    assert_eq!(
        s.months,
        ["2026-05", "2026-06", "2026-07", "2026-08", "2026-09"]
    );
    assert_eq!(
        series(&s.flows_cents, "salary"),
        [600_000, 610_000, 1_000_000, 0, 0]
    );
    assert_eq!(
        series(&s.flows_cents, "salary_off_cycle"),
        [0, 0, 0, 50_000, 0]
    );
    let halves: Vec<(&str, usize, i64)> = s
        .pay
        .half_years
        .iter()
        .map(|h| (h.period.as_str(), h.cheques, h.median_cents))
        .collect();
    assert_eq!(halves, [("2026-H1", 4, 300_000), ("2026-H2", 3, 330_000)]);
    let early = s
        .pay
        .cycles
        .iter()
        .find(|c| c.paid_on == "2026-07-02")
        .unwrap();
    assert_eq!(
        (early.nominal.as_str(), early.offset_days),
        ("2026-07-03", -1)
    );
    assert_eq!(s.pay.gap_days.get(&14), Some(&4));
    assert_eq!(s.pay.off_cycle_deposits, 1);
}

#[test]
fn spending_income_and_transfers_follow_the_rules() {
    let (s, _) = run();
    assert_eq!(
        series(&s.spending_cents, "Food"),
        [20_000, 8_000, 0, 0, 9_134]
    );
    assert_eq!(series(&s.spending_cents, "Housing"), [0, 0, 250_000, 0, 0]);
    assert_eq!(
        series(&s.spending_cents, "(Uncategorized)"),
        [0, 0, 0, 0, 3_000]
    );
    assert_eq!(series(&s.income_cents, "cashback"), [0, 0, 2_500, 0, 0]);
    assert_eq!(series(&s.income_cents, "gift"), [0, 0, 0, 0, 10_000]);
    assert_eq!(series(&s.income_cents, "rsu_espp"), [0, 0, 0, 250_000, 0]);
    assert_eq!(
        series(&s.income_by_category_cents, "Transfer from Brokerage"),
        [0, 0, 0, 250_000, 0]
    );
    assert_eq!(series(&s.flows_cents, "invested"), [0, 0, 100_000, 0, 0]);
    assert_eq!(series(&s.flows_cents, "income_tax"), [80_000, 0, 0, 0, 0]);
    assert_eq!(
        series(&s.flows_cents, "uncategorized_in"),
        [0, 0, 0, 0, 5_000]
    );
    assert_eq!(
        series(&s.flows_cents, "card_payments"),
        [0, 20_000, 0, 0, 0]
    );
    assert_eq!(
        series(&s.flows_cents, "card_spend"),
        [20_000, 8_000, 250_000, 0, 12_134]
    );
    // Card payments and own-account moves are neither income nor spending.
    assert!(!s.income_cents.contains_key("other"));
}

#[test]
fn excluded_accounts_never_count_and_checks_fire() {
    let (s, _) = run();
    let count = |name: &str| {
        s.checks
            .iter()
            .find(|c| c.name.starts_with(name))
            .unwrap()
            .count
    };
    assert_eq!(count("future-dated rows"), 1);
    assert_eq!(count("rows before the window"), 1);
    assert_eq!(count("possible duplicate groups inside one file"), 1);
    assert_eq!(count("own-account transfer pairs"), 0);
    let bank_a = s
        .accounts
        .iter()
        .find(|a| a.name == "Bank A Chequing")
        .unwrap();
    assert_eq!(
        (bank_a.balance_cents, bank_a.sum_balance_cents),
        (Some(500_000), Some(2_255_000))
    );
    assert_eq!(bank_a.balance_source.as_deref(), Some("account row"));
    let visa = s.accounts.iter().find(|a| a.name == "Card A").unwrap();
    assert_eq!(visa.sum_balance_cents, Some(-267_634));
    let sun = s.accounts.iter().find(|a| a.name == "Group RRSP").unwrap();
    assert_eq!((sun.balance_cents, sun.sum_balance_cents), (None, None));
}

#[test]
fn unlisted_accounts_are_guessed_and_flagged() {
    let cfg = config::parse(&CONFIG.replace(r#""Card A" = "card""#, "")).expect("config parses");
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let s = summarize::summarize(&load(), &cfg, today).unwrap();
    let card = s.accounts.iter().find(|a| a.name == "Card A").unwrap();
    assert!(card.guessed);
    assert_eq!(card.class, summarize_class("card"));
}

fn summarize_class(name: &str) -> cashcast_engine::detect::Class {
    cashcast_engine::detect::Class::parse(name).unwrap()
}

#[test]
fn totals_sum_by_account_and_category_without_private_text() {
    let t = inspect::totals(&load());
    let bank_a = t
        .accounts
        .iter()
        .find(|a| a.name == "Bank A Chequing")
        .unwrap();
    assert_eq!((bank_a.currency.as_str(), bank_a.rows), ("CAD", 14));
    assert_eq!(bank_a.transfers.get("Card A"), Some(&-20_000));
    assert_eq!(bank_a.transfers.get("Brokerage"), Some(&250_000));
    let groceries: Vec<(i64, i64, usize)> = t
        .categories
        .iter()
        .filter(|c| c.path == "Food > Groceries")
        .map(|c| (c.in_cents, c.out_cents, c.rows))
        .collect();
    assert_eq!(groceries, [(2_000, -30_134, 5)]);
    let json = serde_json::to_string(&t).unwrap();
    let text = inspect::render_totals(&t);
    assert!(!json.contains("CANARY") && !text.contains("CANARY"));
}
