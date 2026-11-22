//! Unexpected charges and `charges find`. Synthetic data only; every
//! description carries a canary, and the leak test checks the canaries'
//! digits and store numbers never reach the report.

use cashcast_engine::{charges, config, inbox};
use chrono::{Duration, NaiveDate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

const TODAY: &str = "2026-10-20";
const ALIASES: &str = "[[merchants]]\nname = \"Anthropic\"\npatterns = [\"anthropic\", \"claude ai\"]\n";

fn monthly(from: &str, n: i64, text: &'static str, cents: i64) -> Vec<(NaiveDate, &'static str, i64)> {
    (0..n)
        .map(|k| (d(from) + Duration::days(30 * k), text, -cents))
        .collect()
}

fn csv(mut events: Vec<(NaiveDate, &str, i64)>) -> String {
    events.sort_by_key(|e| e.0);
    let mut rows = vec!["Date,Description,Amount".to_string()];
    for (date, text, cents) in events {
        rows.push(format!("{date},{text},{:.2}", cents as f64 / 100.0));
    }
    rows.join("\n")
}

fn load(case: &str, files: &[(&str, String)]) -> Vec<charges::Series> {
    let dir = std::env::temp_dir().join(format!("cashcast-charges-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let cfg = config::parse("").unwrap();
    let inbox = inbox::load_dir(&dir, &cfg).unwrap();
    charges::series(&inbox.txns, &charges::parse_aliases(ALIASES).unwrap())
}

/// Card A: Anthropic monthly since 2025 (CANARY91), a gym that paused for
/// four months, a streaming price rise. Card B: a second Anthropic plan
/// under another billing name (CANARY92), since July 2026.
fn owner(case: &str) -> Vec<charges::Series> {
    let mut a = monthly("2025-10-05", 13, "ANTHROPIC PBC CANARY91 #8812", 2_800);
    a.extend(monthly("2025-10-10", 4, "GYMCO CANARY93", 4_000));
    a.extend(monthly("2026-06-10", 5, "GYMCO CANARY93", 4_000));
    a.extend(monthly("2025-11-01", 11, "STREAMCO CANARY94", 1_699));
    a.push((d("2026-10-01"), "STREAMCO CANARY94", -2_099));
    a.push((d("2026-10-18"), "COFFEE CANARY95", -450));
    let b = monthly("2026-07-03", 4, "CLAUDE.AI SUBSCRIPTION CANARY92", 14_000);
    load(case, &[("visa-a.csv", csv(a)), ("visa-b.csv", csv(b))])
}

#[test]
fn aliases_group_billing_names() {
    let a = charges::parse_aliases(ALIASES).unwrap();
    for text in ["ANTHROPIC", "Anthropic PBC", "CLAUDE.AI SUBSCRIPTION"] {
        assert_eq!(charges::merchant(text, &a), "Anthropic", "{text}");
    }
    assert_eq!(charges::merchant("NETFLIX.COM #4411", &a), "netflix com");
}

#[test]
fn finds_a_duplicate_subscription_across_cards() {
    let all = owner("owner1");
    let found = charges::findings(&all, d(TODAY));
    let dup = found
        .iter()
        .find_map(|f| match f {
            charges::Finding::Duplicate { merchant, series } => Some((merchant, series)),
            _ => None,
        })
        .expect("a duplicate");
    assert_eq!(dup.0, "Anthropic");
    let mut accounts: Vec<&str> = dup.1.iter().map(|&i| all[i].account.as_str()).collect();
    accounts.sort();
    assert_eq!(accounts.len(), 2);
    assert_ne!(accounts[0], accounts[1]);
}

#[test]
fn finds_two_amounts_on_one_card() {
    let mut a = monthly("2026-01-05", 10, "ANTHROPIC CANARY91", 2_800);
    a.extend(monthly("2026-03-12", 8, "ANTHROPIC CANARY91", 14_000));
    let all = load("two-plans", &[("visa.csv", csv(a))]);
    let found = charges::findings(&all, d(TODAY));
    assert!(found.iter().any(|f| matches!(f, charges::Finding::Duplicate { series, .. } if series.len() == 2)));
}

#[test]
fn a_mirrored_copy_is_not_a_duplicate() {
    let a = monthly("2026-01-05", 10, "ANTHROPIC CANARY91", 2_800);
    let copy: Vec<_> = a.iter().map(|e| (e.0 + Duration::days(1), e.1, e.2)).collect();
    let all = load("mirror", &[("bank.csv", csv(a)), ("moneywiz.csv", csv(copy))]);
    let found = charges::findings(&all, d(TODAY));
    assert!(!found.iter().any(|f| matches!(f, charges::Finding::Duplicate { .. })), "{found:?}");
}

#[test]
fn finds_new_rise_and_resumed() {
    let all = owner("owner2");
    let found = charges::findings(&all, d(TODAY));
    let name = |i: usize| all[i].merchant.clone();
    assert!(found.iter().any(|f| matches!(f, charges::Finding::PriceRise { series, from: 1_699, to: 2_099 } if name(*series) == "streamco")));
    assert!(found.iter().any(|f| matches!(f, charges::Finding::Resumed { series, .. } if name(*series) == "gymco")));
    // The card-B plan started 2026-07-03, more than 60 days ago: not new.
    assert!(!found.iter().any(|f| matches!(f, charges::Finding::New(_))));
    let fresh = load("new", &[("visa.csv", csv(monthly("2026-08-25", 3, "ANTHROPIC CANARY91", 2_800)))]);
    let found = charges::findings(&fresh, d(TODAY));
    assert!(found.iter().any(|f| matches!(f, charges::Finding::New(_))), "{found:?}");
}

#[test]
fn find_lists_every_series_per_account() {
    let all = owner("owner3");
    let text = charges::render_find(&all, "anthropic");
    assert!(text.contains("2 account(s)"), "{text}");
    assert!(text.contains("Anthropic (active): monthly 140.00"), "{text}");
    assert!(text.contains("Anthropic (active): monthly 28.00"), "{text}");
    assert!(!text.contains("gymco"), "{text}");
    assert!(charges::render_find(&all, "nothing-here").contains("No charges found."));
}

#[test]
fn reports_never_leak_raw_text() {
    let all = owner("owner4");
    let found = charges::findings(&all, d(TODAY));
    let text = charges::render(&all, &found, d(TODAY)) + &charges::render_find(&all, "a");
    for canary in ["CANARY", "canary", "8812", "PBC", "SUBSCRIPTION"] {
        assert!(!text.contains(canary), "{canary} leaked:\n{text}");
    }
}
