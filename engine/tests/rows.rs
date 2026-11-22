//! Row-level reading rules, one synthetic row per case. No real data.

use std::path::PathBuf;

use cashcast_engine::export::{self, LoadOptions};

const HEADER: &str = "\"Name\",\"Current balance\",\"Account\",\"Transfers\",\"Description\",\"Payee\",\"Category\",\"Date\",\"Time\",\"Memo\",\"Amount\",\"Currency\",\"Check #\",\"Tags\"";

/// Writes `rows` under a MoneyWiz header to a temp file named after `case`.
fn csv_file(case: &str, rows: &[&str]) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("cashcast-test-{}-{case}.csv", std::process::id()));
    let text = std::iter::once(HEADER)
        .chain(rows.iter().copied())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_row_with_no_key_columns_is_an_error_not_a_silent_skip() {
    let path = csv_file(
        "no-key",
        &[
            r#""","","Bank A","","x","p","Food","2026-01-02","","","-1.00","CAD","","""#,
            r#""","","","","orphan text","","","","","","","","","""#,
        ],
    );
    let e = export::load(&path, &LoadOptions::default()).unwrap();
    assert_eq!(e.txns.len(), 1);
    assert_eq!(
        e.bad_lines,
        [(3, "neither a transaction nor an account row")]
    );
}

#[test]
fn blank_rows_are_skipped_quietly() {
    let path = csv_file(
        "blank",
        &[
            r#""","","Bank A","","x","p","Food","2026-01-02","","","-1.00","CAD","","""#,
            r#""","","","","","","","","","","","","","""#,
        ],
    );
    let e = export::load(&path, &LoadOptions::default()).unwrap();
    assert!(e.bad_lines.is_empty());
}

#[test]
fn first_error_returns_only_the_first_failing_row() {
    let path = csv_file(
        "first",
        &[
            r#""","","Bank A","","x","p","Food","2026-01-02","","","-1.00","CAD","","""#,
            r#""","","Bank A","","x","p","Food","2026-01-03","","","not money","CAD","","""#,
            r#""","","Bank A","","x","p","Food","2026-01-04","","","also bad","CAD","","""#,
        ],
    );
    let row = export::first_error(&path, &LoadOptions::default())
        .unwrap()
        .expect("one error");
    assert_eq!(
        (row.line, row.reason, row.records_before),
        (3, "amount not parseable", 1)
    );
    assert!(
        row.fields
            .contains(&("Amount".to_string(), "not money".to_string()))
    );
}

#[test]
fn year_day_month_dates_are_detected() {
    let path = csv_file(
        "ydm",
        &[
            r#""","","Card A","","x","p","Food","2026/30/09","","","-5.00","CAD","","""#,
            r#""","","Card A","","x","p","Food","2026/05/10","","","-6.00","CAD","","""#,
        ],
    );
    let e = export::load(&path, &LoadOptions::default()).unwrap();
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.date_format, "%Y/%d/%m");
    let dates: Vec<String> = e.txns.iter().map(|t| t.date.to_string()).collect();
    assert_eq!(dates, ["2026-09-30", "2026-10-05"]);
}
