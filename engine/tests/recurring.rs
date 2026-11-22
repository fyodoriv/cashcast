//! Recurring payments: payees that charge on a schedule, as numbered groups.
//! Synthetic data only; every description carries a canary.

use cashcast_engine::{config, inbox, inspect, summarize};
use chrono::{Duration, NaiveDate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

const CONFIG: &str = "[window]\nfirst_month = \"2024-01\"\nlast_month = \"2026-09\"\n";

fn run(case: &str, files: &[(&str, String)]) -> (summarize::Summary, String, inbox::Inbox) {
    let dir =
        std::env::temp_dir().join(format!("cashcast-recurring-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let cfg = config::parse(CONFIG).unwrap();
    let inbox = inbox::load_dir(&dir, &cfg).unwrap();
    let s = summarize::summarize(&inbox, &cfg, d("2026-10-02")).unwrap();
    let text = summarize::render(&s);
    (s, text, inbox)
}

fn csv(mut events: Vec<(NaiveDate, &str, i64)>) -> String {
    events.sort_by_key(|e| e.0);
    let mut rows = vec!["Date,Description,Amount".to_string()];
    for (date, text, cents) in events {
        rows.push(format!("{date},{text},{:.2}", cents as f64 / 100.0));
    }
    rows.join("\n")
}

/// Three subscriptions (monthly, yearly, weekly) and noise that must not count.
fn chequing() -> String {
    let mut e: Vec<(NaiveDate, &str, i64)> = Vec::new();
    for k in 0..12 {
        let (y, m) = if k < 3 { (2025, 10 + k) } else { (2026, k - 2) };
        e.push((
            d(&format!("{y}-{m:02}-12")),
            "NETFLIX.COM CANARY71 #4411",
            -1_699,
        ));
        e.push((d(&format!("{y}-{m:02}-25")), "PAYROLL CANARY78", 300_000));
        e.push((
            d(&format!("{y}-{m:02}-20")),
            "BANK VISA PAYMENT CANARY79",
            -50_000,
        ));
        e.push((
            d(&format!("{y}-{m:02}-21")),
            "ONLINE TRANSFER TO SAVINGS CANARY80",
            -20_000,
        ));
    }
    for date in ["2024-03-02", "2025-03-04", "2026-03-03"] {
        e.push((d(date), "DROPBOX ANNUAL CANARY72", -11_900));
    }
    for k in 0..18 {
        e.push((
            d("2026-06-01") + Duration::days(7 * k),
            "MEAL KIT CANARY73",
            -7_900,
        ));
    }
    // Noise: weekly shopping with varying amounts, irregular coffee, a
    // two-time purchase, and a gym that stopped in April.
    for (k, cents) in (0..30).zip([4_000, 12_000, 7_500, 21_000, 3_000].into_iter().cycle()) {
        e.push((
            d("2026-03-02") + Duration::days(7 * k),
            "GROCER CANARY74",
            -cents,
        ));
    }
    for date in [
        "2026-01-03",
        "2026-01-20",
        "2026-03-09",
        "2026-03-11",
        "2026-07-01",
        "2026-09-30",
    ] {
        e.push((d(date), "CAFE CANARY75", -550));
    }
    for date in ["2026-02-14", "2026-03-14"] {
        e.push((d(date), "GADGET STORE CANARY76", -30_000));
    }
    for m in 1..=4 {
        e.push((d(&format!("2026-{m:02}-05")), "OLDGYM CANARY77", -4_500));
    }
    csv(e)
}

#[test]
fn three_subscriptions_become_three_numbered_payments() {
    let (s, text, _) = run("three", &[("chequing.csv", chequing())]);
    let rows: Vec<(&str, &str, &str, i64, usize, &str, i64)> = s
        .recurring_payments
        .iter()
        .map(|p| {
            (
                p.label.as_str(),
                p.cadence.as_str(),
                p.category.as_str(),
                p.median_cents,
                p.count,
                p.last_seen.as_str(),
                p.yearly_cents,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "Payments #1",
                "weekly",
                "(Uncategorized)",
                7_900,
                18,
                "2026-09-28",
                7_900 * 52
            ),
            (
                "Payments #2",
                "monthly",
                "Subscriptions",
                1_699,
                12,
                "2026-09-12",
                1_699 * 12
            ),
            (
                "Payments #3",
                "yearly",
                "Subscriptions",
                11_900,
                3,
                "2026-03-03",
                11_900
            ),
        ]
    );
    assert!(text.contains("## Recurring payments found"));
    assert!(text.contains("- Payments #2: monthly, Subscriptions, 12 charges, usually 16.99, last 2026-09-12; about 203.88 a year"), "{text}");
}

#[test]
fn no_payee_text_reaches_any_output_but_the_private_review_names_them() {
    let (s, text, inbox) = run("leak", &[("chequing.csv", chequing())]);
    let json = serde_json::to_string(&s).unwrap();
    let totals = inspect::render_totals(&inspect::totals(&inbox));
    for out in [&json, &text, &totals] {
        assert!(!out.contains("CANARY"), "payee text leaked");
        assert!(
            !out.contains("NETFLIX") && !out.contains("DROPBOX") && !out.contains("MEAL KIT"),
            "payee name leaked"
        );
    }
    assert!(
        !json.contains("payee_key"),
        "the hashed payee key stays inside the engine"
    );
    let review = summarize::render_review(&inbox, &s);
    assert!(
        review.contains("- Payments #1: MEAL KIT CANARY73"),
        "{review}"
    );
    assert!(
        review.contains("- Payments #3: DROPBOX ANNUAL CANARY72"),
        "{review}"
    );
}

#[test]
fn an_older_card_export_does_not_hide_its_subscriptions() {
    // The card export ends in May; its monthly charge is still active.
    let card: Vec<(NaiveDate, &str, i64)> = (1..=5)
        .map(|m| (d(&format!("2026-{m:02}-15")), "SPOTIFY CANARY81", -1_199))
        .collect();
    let (s, _, _) = run(
        "older",
        &[("chequing.csv", chequing()), ("card.csv", csv(card))],
    );
    assert_eq!(s.recurring_payments.len(), 4);
    let spotify = s
        .recurring_payments
        .iter()
        .find(|p| p.median_cents == 1_199)
        .unwrap();
    assert_eq!(
        (spotify.cadence.as_str(), spotify.last_seen.as_str()),
        ("monthly", "2026-05-15")
    );
}
