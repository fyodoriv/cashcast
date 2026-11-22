//! Bank and app export layouts, one synthetic fixture each in
//! `tests/fixtures/format-*.csv`. Every free-text and private field holds a
//! CANARY string, and no canary may reach a report.

use std::path::PathBuf;

use cashcast_engine::export::{self, Export, LoadOptions};
use cashcast_engine::{config, inbox, inspect, summarize, text};
use chrono::NaiveDate;

const FORMATS: &[&str] = &[
    "scotiabank.csv",
    "scotiabank-legacy.csv",
    "td.csv",
    "wise.csv",
    "wealthsimple-chequing.csv",
    "paypal.csv",
    "paypal-balance.csv",
    "google-sheets.csv",
];

/// Fixtures sit flat in `tests/fixtures/` (the .gitignore allowlist covers
/// only that level) as `format-<name>`.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("format-{name}"))
}

/// Loads a fixture under its short name, which becomes the account name.
fn load(name: &str) -> Export {
    let dir = std::env::temp_dir().join(format!("cashcast-formats-{}-one", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::copy(fixture(name), &path).unwrap();
    export::load(&path, &LoadOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn cents(e: &Export) -> Vec<i64> {
    e.txns.iter().map(|t| t.cents).collect()
}

fn dates(e: &Export) -> Vec<String> {
    e.txns.iter().map(|t| t.date.to_string()).collect()
}

fn accounts(e: &Export) -> Vec<&str> {
    e.txns.iter().map(|t| t.account.as_str()).collect()
}

#[test]
fn scotiabank_csv_signs_from_type_column_and_both_date_styles() {
    let e = load("scotiabank.csv");
    assert_eq!(e.format, "scotiabank-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(
        dates(&e),
        ["2026-09-02", "2026-09-05", "2026-09-08", "2026-09-12"]
    );
    // Debit with a positive amount is money out; a signed debit stays negative.
    assert_eq!(cents(&e), [-4_510, 200_000, -8_000, -2_000]);
    let b = &e.balances["scotiabank"];
    assert_eq!(
        (b.cents, b.as_of),
        (285_490, NaiveDate::from_ymd_opt(2026, 9, 12))
    );
}

#[test]
fn scotiabank_legacy_headerless_rows_read_type_and_description() {
    let e = load("scotiabank-legacy.csv");
    assert_eq!(e.format, "scotiabank-legacy-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.date_format, "%m/%d/%Y");
    assert_eq!(
        dates(&e),
        ["2026-09-01", "2026-09-03", "2026-09-10", "2026-09-15"]
    );
    assert_eq!(cents(&e), [-1_250, 150_000, -6_000, -20_000]);
    // The description column (5), not the "-" column (3), feeds the payee key.
    assert!(e.txns.iter().all(|t| t.payee_key != 0));
    assert_eq!(e.txns[2].category, ["Bills"]);
}

#[test]
fn td_easyweb_headerless_rows_prefer_month_first_dates() {
    let e = load("td.csv");
    // The generic headerless label stays (an existing test pins it); a note names the layout.
    assert_eq!(e.format, "CSV (no header row)");
    assert!(
        e.notes.iter().any(|n| n.contains("td-csv")),
        "{:?}",
        e.notes
    );
    // Every day is 12 or less, so only the TD preference settles the format.
    assert_eq!(e.date_format, "%m/%d/%Y");
    assert_eq!(
        dates(&e),
        ["2026-09-02", "2026-09-05", "2026-09-08", "2026-09-12"]
    );
    assert_eq!(cents(&e), [-4_510, 200_000, -8_000, -325]);
    assert_eq!(e.balances["td"].cents, 287_165);
}

#[test]
fn wise_statement_day_first_dates_one_account_per_currency_names_hashed_only() {
    let e = load("wise.csv");
    assert_eq!(e.format, "wise-statement-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(
        dates(&e),
        ["2026-09-03", "2026-09-04", "2026-09-10", "2026-09-11"]
    );
    assert_eq!(cents(&e), [250_000, -1_240, -30_000, -2_000]);
    assert_eq!(
        accounts(&e),
        ["wise CAD", "wise CAD", "wise CAD", "wise USD"]
    );
    assert_eq!(e.txns[3].currency, "USD");
    // Payer and payee names feed only the hashed key.
    assert_eq!(
        e.txns[0].payee_key,
        text::derive("CANARY-WP01", 1).payee_key
    );
    assert_eq!(
        e.txns[2].payee_key,
        text::derive("CANARY-WE01", 1).payee_key
    );
    assert_eq!(e.txns[1].category, ["Dining"]);
    let labels: Vec<&String> = e.payee_labels.values().collect();
    for private in [
        "CANARY-WA01",
        "CANARY-W4D1",
        "CANARY-WH01",
        "CANARY-WN01",
        "CANARY-WR02",
    ] {
        assert!(
            !labels.iter().any(|l| l.contains(private)),
            "{private} reached the review samples"
        );
    }
}

#[test]
fn wealthsimple_statement_reads_signed_amounts_and_balance() {
    let e = load("wealthsimple-chequing.csv");
    assert_eq!(e.format, "wealthsimple-statement-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(cents(&e), [180_000, -6_420, 210, -15_000]);
    assert_eq!(e.balances["wealthsimple-chequing"].cents, 168_790);
}

#[test]
fn paypal_balance_affecting_report_uses_total_and_day_first_dates() {
    let e = load("paypal-balance.csv");
    assert_eq!(e.format, "paypal-activity-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(dates(&e), ["2025-02-14", "2025-02-20", "2025-03-03"]);
    assert_eq!(cents(&e), [-4_000, 4_000, -1_300]);
    assert_eq!(
        accounts(&e),
        ["paypal-balance CAD", "paypal-balance CAD", "paypal-balance USD"]
    );
    assert!(e.txns.iter().all(|t| !t.text.contains("PB-CANARY")));
}

#[test]
fn paypal_uses_net_and_keeps_only_completed_balance_rows() {
    let e = load("paypal.csv");
    assert_eq!(e.format, "paypal-activity-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert!(
        e.notes.iter().any(|n| n.starts_with("2 rows skipped")),
        "{:?}",
        e.notes
    );
    assert_eq!(dates(&e), ["2026-09-02", "2026-09-05", "2026-09-06"]);
    assert_eq!(cents(&e), [-1_599, 9_680, -1_000]);
    assert_eq!(accounts(&e), ["paypal CAD", "paypal CAD", "paypal USD"]);
    assert_eq!(e.txns[0].category, ["Subscriptions"]);
    let labels: Vec<&String> = e.payee_labels.values().collect();
    for private in [
        "CANARY-PE01",
        "CANARY-PA01",
        "CANARY-PH01",
        "CANARY-PN01",
        "CANARY-PS01",
    ] {
        assert!(
            !labels.iter().any(|l| l.contains(private)),
            "{private} reached the review samples"
        );
    }
}

#[test]
fn google_sheets_download_reads_as_generic_csv() {
    let e = load("google-sheets.csv");
    assert_eq!(e.format, "CSV");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(cents(&e), [-8_215, 240_000, -475]);
    assert_eq!(
        accounts(&e),
        ["Sheets Chequing", "Sheets Chequing", "Sheets Card"]
    );
    assert_eq!(e.txns[2].category, ["Dining"]);
}

#[test]
fn no_canary_from_any_format_reaches_reports() {
    let dir = std::env::temp_dir().join(format!("cashcast-formats-{}-leak", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in FORMATS {
        std::fs::copy(fixture(name), dir.join(name)).unwrap();
    }
    let cfg = config::parse(
        "[window]\nfirst_month = \"2026-01\"\nlast_month = \"2026-09\"\n[currencies]\nbase = \"CAD\"\nrates = { USD = 1.4 }\n",
    )
    .unwrap();
    let inbox = inbox::load_dir(&dir, &cfg).unwrap();
    assert_eq!(inbox.files.len(), FORMATS.len());
    assert!(inbox.failed.is_empty(), "{:?}", inbox.failed);
    let s =
        summarize::summarize(&inbox, &cfg, NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()).unwrap();
    let json = serde_json::to_string(&s).unwrap();
    let text = summarize::render(&s);
    let totals = inspect::render_totals(&inspect::totals(&inbox));
    for out in [&json, &text, &totals] {
        assert!(!out.contains("CANARY"), "private text leaked");
        assert!(!out.contains("example.com"), "an email leaked");
    }
    for label in [
        "scotiabank-csv",
        "scotiabank-legacy-csv",
        "wise-statement-csv",
        "wealthsimple-statement-csv",
        "paypal-activity-csv",
    ] {
        assert!(
            text.contains(label),
            "{label} missing from the import report"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scotiabank_downloads_of_one_account_share_its_name_across_dates() {
    for name in [
        "Ultimate_Package_9084_103026.csv",
        "Ultimate_Package_9084_110426.csv",
        "Ultimate_Package_9084_110426 (1).csv",
    ] {
        let dir =
            std::env::temp_dir().join(format!("cashcast-formats-{}-scotia", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::copy(fixture("scotiabank.csv"), &path).unwrap();
        let e = export::load(&path, &LoadOptions::default()).unwrap();
        assert!(
            e.txns.iter().all(|t| t.account == "Ultimate_Package_9084"),
            "{name}"
        );
    }
}

#[test]
fn tinkoff_windows_1251_comma_decimals_skips_failed_and_uses_payment_amount() {
    let e = load("tinkoff.csv");
    assert_eq!(e.format, "tinkoff-csv");
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(dates(&e), ["2026-10-31", "2026-10-30", "2026-10-28"]);
    // The payment amount is in the account's currency, not the purchase's.
    assert_eq!(cents(&e), [-123_456, 5_000_000, -189_040]);
    assert!(e.txns.iter().all(|t| t.currency == "RUB"));
    // The card number is private: never in the row text.
    assert!(e.txns.iter().all(|t| !t.text.contains("CANARY-TK1")));
}
