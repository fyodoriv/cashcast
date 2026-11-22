//! Cash rows from crypto ledgers, for `moneywiz sync` and `moneywiz coverage`.

use std::path::PathBuf;

use cashcast_engine::ledger;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn kraken_fiat_legs_become_cash_rows_net_of_fees() {
    let e = ledger::cash_export(&fixture("ledger-kraken.csv")).expect("a Kraken ledger");
    let rows = |a: &str| -> Vec<i64> {
        e.txns
            .iter()
            .filter(|t| t.account == a)
            .map(|t| t.cents)
            .collect()
    };
    assert_eq!(rows("Kraken CAD"), [1_000_000, -602_400, 398_400, -140_000]);
    assert_eq!(rows("Kraken USD"), [149_400, 100_000, -100]);
    // Crypto legs are not cash: no row for BTC, ETH or DOT.
    assert!(
        e.txns
            .iter()
            .all(|t| t.account == "Kraken CAD" || t.account == "Kraken USD")
    );
    assert!(
        e.txns
            .iter()
            .all(|t| t.currency == t.account["Kraken ".len()..])
    );
    // The row text is the type and asset only, never the ledger ids.
    assert!(
        e.txns
            .iter()
            .all(|t| !t.text.contains("LA") && !t.text.contains("TA"))
    );
}

#[test]
fn etrade_api_json_reads_like_the_csv() {
    let path = fixture("ledger-etrade-api.json");
    assert_eq!(ledger::sniff(&path), Some("etrade-transactions-csv"));
    let e = ledger::cash_export(&path).expect("an E*TRADE API page");
    let rows: Vec<(String, i64)> = e
        .txns
        .iter()
        .map(|t| (t.date.to_string(), t.cents))
        .collect();
    assert_eq!(
        rows,
        [
            ("2025-01-15".to_string(), -150_025),
            ("2025-02-15".to_string(), 1_250),
            ("2025-01-10".to_string(), 200_000)
        ]
    );
    assert!(e.txns.iter().all(|t| t.account == "E*TRADE"));
    // Descriptions and account ids are never read.
    assert!(e.txns.iter().all(|t| !t.text.contains("CANARY")));
    assert_eq!(e.txns[0].text, "bought VTI");
}

#[test]
fn wealthsimple_spending_and_investment_rows_become_cash_rows() {
    let e = ledger::cash_export(&fixture("ledger-wealthsimple-cash.csv"))
        .expect("a Wealthsimple activity file");
    let rows = |a: &str| -> Vec<i64> {
        e.txns
            .iter()
            .filter(|t| t.account == a)
            .map(|t| t.cents)
            .collect()
    };
    assert_eq!(rows("Wealthsimple Cash"), [250_000, -180_000]);
    assert_eq!(rows("Wealthsimple Credit"), [-4_210, 30_000]);
    // An investment account is named by its type and the end of its id, and
    // keeps the full id for `[moneywiz.investments]` links (never printed).
    // The trade leg has text "trade buy": a mapped account leaves it to the
    // buy row.
    let tfsa = "Wealthsimple TFSA …Y-WSA1";
    assert_eq!(rows(tfsa), [-10_000, 50_000]);
    assert_eq!(e.txns.len(), 6);
    assert_eq!(
        e.ledger_ids[tfsa].iter().collect::<Vec<_>>(),
        [&("TFSA".to_string(), "CANARY-WSA1".to_string())]
    );
    assert!(e.ledger_ids.get("Wealthsimple Cash").is_none());
    assert!(e.txns.iter().any(|t| t.text == "trade buy"));
    // The description, ids and names are private: the text is the type only.
    assert!(e.txns.iter().all(|t| !t.text.contains("CANARY")));
}

#[test]
fn kraken_ledger_balance_is_the_last_fiat_row() {
    let e = ledger::cash_export(&fixture("ledger-kraken.csv")).expect("a Kraken ledger");
    let b = |a: &str| {
        e.balances
            .get(a)
            .map(|b| (b.cents, b.as_of.map(|d| d.to_string())))
    };
    assert_eq!(b("Kraken CAD"), Some((656_000, Some("2025-09-01".into()))));
    assert_eq!(b("Kraken USD"), Some((249_300, Some("2025-09-02".into()))));
    assert_eq!(b("Kraken BTC"), None, "crypto is not cash");
}
