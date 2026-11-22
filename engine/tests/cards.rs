//! Which card now: one test per cap rule in knowledge/data/credit_cards.toml,
//! plus the rent rule, credit left and pending changes. Synthetic rows only.

use std::collections::BTreeMap;

use cashcast_engine::cards::{self, Report, Wallet};
use cashcast_engine::summarize::SpendRow;
use chrono::NaiveDate;

const CATALOG: &str = include_str!("../../knowledge/data/credit_cards.toml");

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn row(account: &str, date: &str, category: &str, dollars: i64) -> SpendRow {
    SpendRow {
        account: account.into(),
        date: d(date),
        category: category.into(),
        cents: dollars * 100,
        foreign: false,
    }
}

fn wallet(profile: &str) -> Wallet {
    cards::parse(profile, CATALOG, None).unwrap()
}

fn run(profile: &str, rows: &[SpendRow], today: &str) -> (Wallet, Report) {
    let w = wallet(profile);
    let r = cards::now(&w, rows, &BTreeMap::new(), &BTreeMap::new(), d(today));
    (w, r)
}

fn pick<'a>(r: &'a Report, cat: &str) -> &'a cards::Pick {
    r.picks.iter().find(|p| p.category == cat).unwrap()
}

const SCOTIA_TD: &str = r#"
[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Card"]
cap_reset = "10-26"

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["Beta Card"]
cap_reset = "02-01"
"#;

#[test]
fn card_year_cap_counts_from_cap_reset_and_moves_on_when_used_up() {
    let rows = [
        // Before this card year: not counted.
        row(
            "Alpha Card",
            "2026-10-20",
            "Food & Dining > Groceries",
            24_000,
        ),
        row("Alpha Card", "2026-10-27", "Bills > Internet", 300),
        row("Alpha Card", "2026-10-28", "Food & Dining > Groceries", 200),
    ];
    let (_, r) = run(SCOTIA_TD, &rows, "2026-11-01");
    let cap = &r.cards[0].caps[0];
    assert_eq!(cap.categories, ["groceries", "recurring", "streaming"]);
    assert_eq!(
        (cap.used, cap.left(), cap.start, cap.resets),
        (50_000, 2_450_000, d("2026-10-26"), d("2027-10-26"))
    );
    assert!(!cap.guessed);
    let g = pick(&r, "groceries");
    assert_eq!(
        (g.card.as_str(), g.rate),
        ("scotia-momentum-visa-infinite", 0.04)
    );
    assert_eq!(g.cap_left, Some((2_450_000, Some(d("2027-10-26")))));

    // The 4 % group used up: groceries go to TD at 3 %.
    let full = [row(
        "Alpha Card",
        "2026-10-30",
        "Food & Dining > Groceries",
        25_000,
    )];
    let (_, r) = run(SCOTIA_TD, &full, "2026-11-01");
    assert_eq!(r.cards[0].caps[0].left(), 0);
    let g = pick(&r, "groceries");
    assert_eq!(
        (g.card.as_str(), g.rate),
        ("td-cash-back-visa-infinite", 0.03)
    );
    assert_eq!(
        g.then,
        Some(("scotia-momentum-visa-infinite".to_string(), 0.01))
    );
}

#[test]
fn unknown_card_year_counts_the_last_twelve_months_and_says_so() {
    let profile = r#"
[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Card"]
"#;
    let rows = [
        row("Alpha Card", "2025-10-01", "Bills > Phone", 9_000),
        row("Alpha Card", "2026-01-15", "Bills > Phone", 100),
    ];
    let (w, r) = run(profile, &rows, "2026-11-01");
    let cap = &r.cards[0].caps[0];
    assert!(cap.guessed);
    assert_eq!((cap.used, cap.start), (10_000, d("2025-11-02")));
    assert_eq!(pick(&r, "recurring").cap_left, Some((2_490_000, None)));
    let text = cards::render(&w, &r);
    assert!(text.contains("2025-11-02 (guess) | unknown"));
    assert!(text.contains("cap reset date unknown"));
    assert!(text.contains("its rows end 2026-01-15"));
}

#[test]
fn per_category_cap_gives_each_category_its_own_limit() {
    let profile = r#"
[[wallet.cards]]
id = "td-cash-back-visa"
accounts = ["Gamma Card"]
cap_reset = "01-01"
"#;
    let rows = [
        row(
            "Gamma Card",
            "2026-03-01",
            "Food & Dining > Groceries",
            5_000,
        ),
        row(
            "Gamma Card",
            "2026-03-02",
            "Personal Transport > Gas/Fuel",
            1_000,
        ),
    ];
    let (_, r) = run(profile, &rows, "2026-11-01");
    let caps = &r.cards[0].caps;
    assert_eq!(caps.len(), 6);
    let groceries = caps.iter().find(|c| c.categories == ["groceries"]).unwrap();
    let gas = caps.iter().find(|c| c.categories == ["gas"]).unwrap();
    assert_eq!((groceries.left(), gas.left()), (0, 400_000));
    assert_eq!(pick(&r, "groceries").rate, 0.005);
    assert_eq!(pick(&r, "gas").rate, 0.01);
}

#[test]
fn calendar_year_cap_starts_january_first() {
    let profile = r#"
[[wallet.cards]]
id = "scotia-passport-visa-infinite"
accounts = ["Delta Card"]
cap_reset = "06-15"
"#;
    let rows = [
        row(
            "Delta Card",
            "2025-12-31",
            "Food & Dining > Dining/Eating Out",
            40_000,
        ),
        row(
            "Delta Card",
            "2026-01-02",
            "Food & Dining > Dining/Eating Out",
            1_000,
        ),
    ];
    let (_, r) = run(profile, &rows, "2026-11-01");
    let cap = &r.cards[0].caps[0];
    assert_eq!(
        (cap.used, cap.start, cap.resets),
        (100_000, d("2026-01-01"), d("2027-01-01"))
    );
}

#[test]
fn monthly_cap_starts_on_the_first() {
    let profile = r#"
[[wallet.cards]]
id = "neo-world-elite"
accounts = ["Neo Card"]
"#;
    let rows = [
        row("Neo Card", "2026-10-31", "Food & Dining > Groceries", 1_000),
        row("Neo Card", "2026-11-03", "Food & Dining > Groceries", 1_000),
    ];
    let (_, r) = run(profile, &rows, "2026-11-04");
    let cap = &r.cards[0].caps[0];
    assert_eq!(
        (cap.used, cap.start, cap.resets),
        (100_000, d("2026-11-01"), d("2026-12-01"))
    );
    assert_eq!(pick(&r, "groceries").rate, 0.01);
    assert!(!cap.guessed);
}

#[test]
fn statement_cap_starts_the_day_after_the_statement_closes() {
    let profile = r#"
[[wallet.cards]]
id = "bmo-cashback-world-elite"
accounts = ["Bmo Card"]
statement_close_day = 18
"#;
    let rows = [
        row("Bmo Card", "2026-10-18", "Food & Dining > Groceries", 400),
        row("Bmo Card", "2026-10-19", "Food & Dining > Groceries", 120),
    ];
    let (_, r) = run(profile, &rows, "2026-11-01");
    let cap = &r.cards[0].caps[0];
    assert_eq!(
        (cap.used, cap.start, cap.resets),
        (12_000, d("2026-10-19"), d("2026-11-19"))
    );
    assert_eq!(r.cards[0].next_close, Some(d("2026-11-18")));
}

#[test]
fn combined_cap_counts_every_purchase_and_ends_the_bonus() {
    let profile = r#"
[[wallet.cards]]
id = "cibc-dividend-visa-infinite"
accounts = ["Cibc Card"]
cap_reset = "01-01"
"#;
    // Clothing earns "other", which only the $50,000 all-purchases cap counts.
    let rows = [
        row("Cibc Card", "2026-02-01", "Clothing > Clothes", 49_000),
        row(
            "Cibc Card",
            "2026-02-02",
            "Food & Dining > Groceries",
            1_000,
        ),
    ];
    let (_, r) = run(profile, &rows, "2026-11-01");
    let (bonus, total) = (&r.cards[0].caps[0], &r.cards[0].caps[1]);
    assert_eq!(
        (bonus.used, total.used, total.left()),
        (100_000, 5_000_000, 0)
    );
    assert_eq!(pick(&r, "groceries").rate, 0.01);
}

#[test]
fn refunds_give_cap_back() {
    let rows = [
        row("Alpha Card", "2026-10-27", "Food & Dining > Groceries", 500),
        row(
            "Alpha Card",
            "2026-10-28",
            "Food & Dining > Groceries",
            -200,
        ),
    ];
    let (_, r) = run(SCOTIA_TD, &rows, "2026-11-01");
    assert_eq!(r.cards[0].caps[0].used, 30_000);
}

#[test]
fn rows_on_other_accounts_and_after_today_are_not_counted() {
    let rows = [
        row(
            "Someone Else",
            "2026-10-27",
            "Food & Dining > Groceries",
            500,
        ),
        row("Alpha Card", "2026-11-05", "Food & Dining > Groceries", 500),
    ];
    let (_, r) = run(SCOTIA_TD, &rows, "2026-11-01");
    assert_eq!(r.cards[0].caps[0].used, 0);
    assert_eq!(r.cards[0].data_until, None);
}

const RENT: &str = r#"
[rent]
monthly = 1000
fee_rate = 0.02
codes_as = "recurring"

[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Card"]
cap_reset = "10-26"
rent_max = 2
# Scotia before Oct 22, 2026: no rent rate, so rent codes as recurring at 4%.
earn = { groceries = 0.04, recurring = 0.04, streaming = 0.04, gas = 0.02, ev_charging = 0.02, transit = 0.02, rideshare = 0.02, delivery = 0.02, other = 0.01 }

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["Beta Card"]
cap_reset = "06-01"
rent_year_start = "02-01"
rent_max = 4

[[wallet.cards]]
id = "wealthsimple-visa-infinite-plus"
"#;

#[test]
fn rent_rule_counts_charges_and_moves_rent_to_the_next_card() {
    let rows = [
        row("Alpha Card", "2026-10-27", "Housing > Rent", 1_020),
        // A car rental under "rent" is not a rent charge.
        row("Alpha Card", "2026-10-28", "Travel > Transport rent", 80),
        row("Beta Card", "2026-01-27", "Housing > Rent", 1_020),
        row("Beta Card", "2026-02-27", "Housing > Rent", 1_020),
    ];
    let (_, r) = run(RENT, &rows, "2026-11-01");
    let scotia = r.cards[0].rent.as_ref().unwrap();
    assert_eq!(
        (scotia.used, scotia.max, scotia.left, scotia.since),
        (1, Some(2), Some(1), d("2026-10-26"))
    );
    // Rent codes as recurring, so it uses Scotia's 4 % group cap.
    assert_eq!(r.cards[0].caps[0].used, 110_000);
    let td = r.cards[1].rent.as_ref().unwrap();
    assert_eq!((td.used, td.since), (1, d("2026-02-01")));
    let p = pick(&r, "rent");
    assert_eq!(p.card, "scotia-momentum-visa-infinite");
    assert!((p.rate - 0.02).abs() < 1e-9);

    let more = [
        rows.to_vec(),
        vec![row("Alpha Card", "2026-10-30", "Housing > Rent", 1_020)],
    ]
    .concat();
    let (_, r) = run(RENT, &more, "2026-11-01");
    assert_eq!(r.cards[0].rent.as_ref().unwrap().left, Some(0));
    let p = pick(&r, "rent");
    assert_eq!(p.card, "td-cash-back-visa-infinite");
    assert!(p.skipped.iter().any(|s| s.contains("rent count used up")));
}

#[test]
fn credit_left_skips_a_card_that_cannot_take_the_charge_and_flags_targets() {
    let profile = r#"
[rent]
monthly = 1000
codes_as = "recurring"

[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Card"]
cap_reset = "10-26"
limit = 5000
target = 150
statement_close_day = 25

[[wallet.cards]]
id = "wealthsimple-visa-infinite-plus"
limit = 9000
"#;
    let w = wallet(profile);
    let balances = BTreeMap::from([("scotia-momentum-visa-infinite".to_string(), 450_000)]);
    let r = cards::now(&w, &[], &balances, &BTreeMap::new(), d("2026-11-01"));
    assert_eq!(r.cards[0].credit_left(), Some(50_000));
    let rent = pick(&r, "rent");
    assert_eq!(rent.card, "wealthsimple-visa-infinite-plus");
    assert!(
        rent.skipped
            .iter()
            .any(|s| s.contains("credit left 500.00"))
    );
    // Groceries fit in the credit left, so Scotia keeps them.
    assert_eq!(pick(&r, "groceries").card, "scotia-momentum-visa-infinite");
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("above its target of 150.00") && n.contains("Wed Nov 25"))
    );
}

#[test]
fn pending_product_change_is_named_and_the_card_stays_as_is() {
    let profile = r#"
[[wallet.cards]]
id = "cibc-dividend-visa-infinite"
accounts = ["Cibc Card"]
cap_reset = "01-01"
"#;
    let w = wallet(profile);
    let pending = BTreeMap::from([(
        "cibc-dividend-visa-infinite".to_string(),
        vec!["downgrade-cibc-dividend-visa-infinite".to_string()],
    )]);
    let r = cards::now(&w, &[], &BTreeMap::new(), &pending, d("2026-11-01"));
    assert_eq!(pick(&r, "groceries").rate, 0.04);
    let text = cards::render(&w, &r);
    assert!(
        text.contains("product change is pending (task `downgrade-cibc-dividend-visa-infinite`)")
    );
}

#[test]
fn profile_overrides_beat_keyword_rules() {
    let profile = r#"
[strategy.categories]
"Travel > Transport rent" = "travel"

[[wallet.cards]]
id = "wealthsimple-visa-infinite-plus"
"#;
    let w = wallet(profile);
    assert_eq!(
        cards::reward_category("Travel > Transport rent", &w.overrides),
        "travel"
    );
    assert_eq!(
        cards::reward_category("Housing > Rent", &w.overrides),
        "rent"
    );
    assert_eq!(
        cards::reward_category("Clothing > Clothes", &w.overrides),
        "other"
    );
}

#[test]
fn bad_input_is_named() {
    let err = cards::parse("[[wallet.cards]]\nid = \"nope\"\n", CATALOG, None).unwrap_err();
    assert!(err.contains("nope is not in the knowledge catalog"));
    let err = cards::parse(
        "[[wallet.cards]]\nid = \"td-cash-back-visa\"\ncap_reset = \"13-40\"\n",
        CATALOG,
        None,
    )
    .unwrap_err();
    assert!(err.contains("must be \"MM-DD\""));
}

#[test]
fn own_catalog_overrides_a_card() {
    let own = "[[cards]]\nid = \"td-cash-back-visa\"\nearn = { groceries = 0.02, other = 0.01 }\ncaps = []\n";
    let w = cards::parse(
        "[[wallet.cards]]\nid = \"td-cash-back-visa\"\n",
        CATALOG,
        Some(own),
    )
    .unwrap();
    assert_eq!(w.cards[0].earn["groceries"], 0.02);
    assert!(w.cards[0].caps.is_empty());
}

/// Rent fills Scotia's shared 4 % cap (7 charges) and TD's recurring cap (4 charges).
const RENT_FILLS_CAPS: &str = r#"
[rent]
monthly = 3500
fee_rate = 0.0175
codes_as = "recurring"

[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["CANARY Alpha Card"]
cap_reset = "11-01"
rent_max = 7
# Scotia before Oct 22, 2026: no rent rate, so rent codes as recurring at 4%.
earn = { groceries = 0.04, recurring = 0.04, streaming = 0.04, gas = 0.02, ev_charging = 0.02, transit = 0.02, rideshare = 0.02, delivery = 0.02, other = 0.01 }

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["CANARY Beta Card"]
cap_reset = "02-01"
rent_max = 4

[[wallet.cards]]
id = "wealthsimple-visa-infinite-plus"
"#;

#[test]
fn a_shared_cap_is_saved_for_rent_when_rent_gains_more_there() {
    // Scotia's 4 % cap just reset. A groceries dollar there gains 1 % over TD;
    // a rent dollar it pushes to Wealthsimple loses 2 %.
    let rows = [
        row(
            "CANARY Beta Card",
            "2026-10-02",
            "Food & Dining > Groceries",
            300,
        ),
        row("CANARY Beta Card", "2026-10-03", "Bills > Internet", 80),
    ];
    let (w, r) = run(RENT_FILLS_CAPS, &rows, "2026-11-01");
    let rent = pick(&r, "rent");
    assert_eq!(rent.card, "scotia-momentum-visa-infinite");
    for cat in ["groceries", "recurring", "streaming"] {
        let p = pick(&r, cat);
        assert_eq!(
            (p.card.as_str(), p.rate),
            ("td-cash-back-visa-infinite", 0.03),
            "{cat}"
        );
        assert!(
            p.skipped.iter().any(|s| s
                .contains("Scotia Momentum Visa Infinite: cap saved for rent (7 charges left)")),
            "{cat}: {:?}",
            p.skipped
        );
    }
    // Scotia's 2 % group does not hold rent: its picks do not change.
    assert_eq!(pick(&r, "gas").card, "td-cash-back-visa-infinite");
    assert_eq!(pick(&r, "transit").card, "td-cash-back-visa-infinite");
    assert!(pick(&r, "rideshare").skipped.is_empty());
    let text = cards::render(&w, &r);
    assert!(text.contains("cap saved for rent"));
    assert!(!text.contains("CANARY"));

    // Rent charges used up on Scotia: groceries may use its 4 % cap again.
    let paid: Vec<_> = (0..7)
        .map(|i| {
            row(
                "CANARY Alpha Card",
                &format!("2026-11-0{}", i + 1),
                "Housing > Rent",
                3_561,
            )
        })
        .collect();
    let (_, r) = run(RENT_FILLS_CAPS, &paid, "2026-11-08");
    assert_eq!(r.cards[0].rent.as_ref().unwrap().left, Some(0));
    let g = pick(&r, "groceries");
    assert_eq!(
        (g.card.as_str(), g.rate),
        ("scotia-momentum-visa-infinite", 0.04)
    );
    assert!(g.skipped.is_empty());
}

#[test]
fn room_beyond_the_rent_charges_stays_usable() {
    // Two rent charges left need 7,122.50 of the 25,000 cap: the rest is shown as cap left.
    let profile = RENT_FILLS_CAPS.replace("rent_max = 7", "rent_max = 2");
    let (_, r) = run(&profile, &[], "2026-11-01");
    let g = pick(&r, "groceries");
    assert_eq!(
        (g.card.as_str(), g.rate),
        ("scotia-momentum-visa-infinite", 0.04)
    );
    assert_eq!(
        g.cap_left,
        Some((2_500_000 - 2 * 356_125, Some(d("2027-11-01"))))
    );
}

#[test]
fn rent_rows_before_a_cards_rent_rate_date_still_use_its_cap() {
    // Scotia's catalog entry: rent earns 1% from 2026-10-22 (`rent_rate_from`).
    // Rent rows posted before that date earned the recurring rate and used
    // the 4% group cap; rows from that date on do not.
    let profile = r#"
[rent]
monthly = 1000
fee_rate = 0.015
codes_as = "recurring"

[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Card"]
cap_reset = "11-27"

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["Beta Card"]
cap_reset = "02-01"
"#;
    let rows = [
        row("Alpha Card", "2026-09-25", "Housing > Rent", 24_900),
        row("Alpha Card", "2026-10-30", "Housing > Rent", 1_000),
    ];
    let (_, r) = run(profile, &rows, "2026-11-15");
    let cap = &r.cards[0].caps[0];
    assert_eq!((cap.used, cap.start), (2_490_000, d("2025-11-27")));
    // Groceries wait for the reset: only 100 left at 4%.
    assert_eq!(
        pick(&r, "groceries").cap_left,
        Some((10_000, Some(d("2026-11-27"))))
    );
    // Rent never picks Scotia now: 1% minus the 1.5% fee.
    assert_eq!(pick(&r, "rent").card, "td-cash-back-visa-infinite");
}
