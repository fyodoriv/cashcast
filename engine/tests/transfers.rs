//! Transfer plan: balances in, e-Transfers that bring each bank to its keep
//! amount out. Synthetic profile and balances only.

use std::collections::BTreeMap;

use cashcast_engine::transfers::{self, ease, easy_down, easy_up, runs};

const PROFILE: &str = r#"
[transfers]
hub = "hub-chequing"
daily_limit = 3000
buffer = 0

[[banking.accounts]]
id = "hub-chequing"
keep = 15000

[[banking.accounts]]
id = "alpha-chequing"
keep = 6500

[[banking.accounts]]
id = "beta-chequing"
keep = 4500

[[wallet.cards]]
id = "alpha-visa"
target = 250

[[wallet.cards]]
id = "beta-visa"
target = 50
paid_from = "beta-chequing"

[[wallet.cards]]
id = "hub-visa"
target = 1000
"#;

fn balances(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v * 100))
        .collect()
}

#[test]
fn counts_digit_runs() {
    assert_eq!(runs(999), 1);
    assert_eq!(runs(2000), 2);
    assert_eq!(runs(1555), 2);
    assert_eq!(runs(1234), 4);
    assert_eq!(ease(500), 1);
    assert_eq!(ease(555), 1);
    assert_eq!(ease(522), 2);
}

#[test]
fn rounds_up_to_an_amount_that_is_quick_to_type() {
    assert_eq!(easy_up(1530), 1555);
    assert_eq!(easy_up(980), 999);
    assert_eq!(easy_up(1990), 2000);
    assert_eq!(easy_up(111), 111);
    for n in [1u32, 47, 333, 1530, 2711, 2950] {
        let a = easy_up(n);
        assert!(a >= n && a <= n + (n / 10).max(50), "{n} -> {a}");
        assert!(ease(a) <= 2, "{n} -> {a}");
    }
    assert_eq!(easy_up(1001), 1100);
}

#[test]
fn rounds_surplus_down_so_the_bank_stays_at_its_keep() {
    assert_eq!(easy_down(1530), 1500);
    assert_eq!(easy_down(522), 500);
    for n in [120u32, 999, 2222, 2950] {
        let a = easy_down(n);
        assert!(a <= n && a + (n / 10).max(50) >= n, "{n} -> {a}");
    }
}

#[test]
fn funds_each_bank_to_keep_plus_its_card_paydown() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[
        ("hub-chequing", 40000),
        ("alpha-chequing", 6000),
        ("alpha-visa", 1530),
        ("beta-chequing", 4500),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    // alpha needs 500 (to keep) + 1530 (the card's full balance) = 2030
    let alpha: u32 = out
        .moves
        .iter()
        .filter(|m| m.to == "alpha-chequing")
        .map(|m| m.dollars)
        .sum();
    assert!((2030..=2030 + 203).contains(&alpha), "{alpha}");
    assert_eq!(out.payments.len(), 1);
    assert_eq!(out.payments[0].cents, 1530 * 100);
    assert!(out.moves.iter().all(|m| runs(m.dollars) <= 3));
    assert!(out.moves.iter().all(|m| m.from == "hub-chequing"));
}

#[test]
fn never_sends_more_than_the_daily_limit_from_one_account() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[
        ("hub-chequing", 40000),
        ("alpha-chequing", 1000),
        ("beta-chequing", 0),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    let mut per_day: BTreeMap<(u32, String), u32> = BTreeMap::new();
    for m in &out.moves {
        *per_day.entry((m.day, m.from.clone())).or_default() += m.dollars;
    }
    assert!(per_day.values().all(|v| *v <= 3000), "{per_day:?}");
    let alpha: u32 = out
        .moves
        .iter()
        .filter(|m| m.to == "alpha-chequing")
        .map(|m| m.dollars)
        .sum();
    let beta: u32 = out
        .moves
        .iter()
        .filter(|m| m.to == "beta-chequing")
        .map(|m| m.dollars)
        .sum();
    assert!(alpha >= 5500 && beta >= 4500, "{alpha} {beta}");
    assert!(out.moves.iter().map(|m| m.day).max().unwrap() >= 4);
}

#[test]
fn sends_surplus_back_to_the_hub_rounded_down() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[
        ("hub-chequing", 20000),
        ("alpha-chequing", 9321),
        ("beta-chequing", 4500),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    let back: Vec<_> = out
        .moves
        .iter()
        .filter(|m| m.from == "alpha-chequing")
        .collect();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].to, "hub-chequing");
    assert!(
        (2539..=2821).contains(&back[0].dollars),
        "{}",
        back[0].dollars
    );
}

#[test]
fn warns_when_the_hub_would_fall_below_its_keep() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[("hub-chequing", 15500), ("alpha-chequing", 1000)]);
    let out = transfers::plan(&p, &b).unwrap();
    let text = transfers::render(&p, &b, &out);
    assert!(text.contains("below its keep"), "{text}");
    assert!(text.contains("beta-chequing: no balance given"), "{text}");
}

#[test]
fn a_card_paid_from_the_hub_needs_no_transfer() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[
        ("hub-chequing", 40000),
        ("hub-visa", 5000),
        ("alpha-chequing", 6500),
        ("beta-chequing", 4500),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    assert!(out.moves.is_empty(), "{:?}", out.moves);
    assert_eq!(out.payments[0].card, "hub-visa");
    let text = transfers::render(&p, &b, &out);
    assert!(text.contains("full current balance of hub-visa"), "{text}");
    assert!(text.contains("| hub-chequing | 40,000.00 | 35,000.00 |"), "{text}");
}

#[test]
fn a_bank_above_keep_gets_no_top_up_and_a_card_in_credit_is_left() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[
        ("hub-chequing", 40000),
        ("alpha-chequing", 6550),
        ("alpha-visa", -165),
        ("beta-chequing", 4500),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    assert!(out.moves.is_empty(), "{:?}", out.moves);
    assert!(out.payments.is_empty());
    let text = transfers::render(&p, &b, &out);
    assert!(text.contains("alpha-visa has a credit of $165.00"), "{text}");
}

#[test]
fn a_bank_with_surplus_funds_a_short_bank_directly() {
    let p = transfers::parse(PROFILE).unwrap();
    let b = balances(&[
        ("hub-chequing", 15000),
        ("alpha-chequing", 9000),
        ("beta-chequing", 4000),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    // beta needs 500; alpha holds 2500 extra: one move alpha → beta, the rest to the hub
    assert!(
        out.moves
            .iter()
            .any(|m| m.from == "alpha-chequing" && m.to == "beta-chequing"),
        "{:?}",
        out.moves
    );
    assert!(!out.moves.iter().any(|m| m.from == "hub-chequing"));
}

#[test]
fn rejects_an_unknown_balance_id() {
    let p = transfers::parse(PROFILE).unwrap();
    let err = transfers::plan(&p, &balances(&[("nope", 1)])).unwrap_err();
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn a_buffer_keeps_a_small_margin_above_keep_and_is_capped() {
    let with = |pct: u32| PROFILE.replace("buffer = 0", &format!("buffer = {pct}"));
    let p = transfers::parse(&with(10)).unwrap();
    let b = balances(&[
        ("hub-chequing", 40000),
        ("alpha-chequing", 6000),
        ("beta-chequing", 4500),
    ]);
    let out = transfers::plan(&p, &b).unwrap();
    let alpha: u32 = out
        .moves
        .iter()
        .filter(|m| m.to == "alpha-chequing")
        .map(|m| m.dollars)
        .sum();
    // below keep: up to keep plus 10 %
    assert!((1150..=1150 + 115).contains(&alpha), "{alpha}");
    // above keep: no top-up, however far from keep plus the buffer
    let b = balances(&[
        ("hub-chequing", 40000),
        ("alpha-chequing", 7100),
        ("beta-chequing", 4950),
    ]);
    assert!(transfers::plan(&p, &b).unwrap().moves.is_empty());
    let err = transfers::parse(&with(25)).unwrap_err();
    assert!(err.contains("0 to 20"), "{err}");
    assert_eq!(
        transfers::parse(PROFILE.replace("buffer = 0\n", "").as_str())
            .unwrap()
            .buffer,
        5
    );
}
