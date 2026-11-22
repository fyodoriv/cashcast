//! Net total: sums, trend, boundary checks and ranking. Synthetic values only.

use cashcast_engine::net_total::{self, Input, Item, Opt, Point};

fn item(label: &str, kind: &str, cad: f64) -> Item {
    Item {
        label: label.into(),
        kind: kind.into(),
        cad,
        as_of: Some("2026-10-30".into()),
        source: "summary".into(),
    }
}

fn input() -> Input {
    Input {
        today: "2026-11-01".into(),
        items: vec![
            item("CANARY-CHEQUING", "cash", 20_000.0),
            item("CANARY-TFSA", "investment", 50_000.0),
            item("CANARY-BTC", "crypto", 5_000.0),
            item("CANARY-CARD", "debt", -3_000.0),
        ],
        reserve: Some(10_000.0),
        risk: Some("growth".into()),
        ..Input::default()
    }
}

fn opt(id: &str, effect: f64) -> Opt {
    Opt {
        id: id.into(),
        title: format!("Option {id}"),
        section: "test".into(),
        effect_year: effect,
        ..Opt::default()
    }
}

#[test]
fn total_is_assets_minus_debts() {
    let r = net_total::compute(&input(), "", Vec::new()).unwrap();
    assert_eq!(r.total, 72_000.0);
    assert_eq!(r.by_kind["debt"], -3_000.0);
    assert_eq!(r.history.len(), 1);
    assert!(r.changes.is_empty());
}

#[test]
fn own_items_count_and_a_debt_is_subtracted() {
    let profile = r#"
[[net_total.items]]
label = "CANARY-CAR"
kind = "other"
cad = 8000
[[net_total.items]]
label = "CANARY-LOAN"
kind = "debt"
cad = 6000
"#;
    let r = net_total::compute(&input(), profile, Vec::new()).unwrap();
    assert_eq!(r.total, 72_000.0 + 8_000.0 - 6_000.0);
    assert_eq!(r.by_kind["debt"], -9_000.0);
}

#[test]
fn trend_compares_with_30_and_365_days_ago() {
    let history = vec![
        Point {
            date: "2025-10-01".into(),
            total: 40_000.0,
            ..Point::default()
        },
        Point {
            date: "2026-09-20".into(),
            total: 70_000.0,
            ..Point::default()
        },
        Point {
            date: "2026-10-25".into(),
            total: 71_000.0,
            ..Point::default()
        },
    ];
    let r = net_total::compute(&input(), "", history).unwrap();
    let c: Vec<(i64, f64)> = r.changes.iter().map(|c| (c.days, c.change)).collect();
    assert_eq!(c, vec![(30, 2_000.0), (365, 32_000.0)]);
    assert_eq!(r.history.len(), 4);
}

#[test]
fn a_second_run_the_same_day_replaces_the_point() {
    let first = net_total::compute(&input(), "", Vec::new()).unwrap();
    let mut again = input();
    again.items.pop();
    let r = net_total::compute(&again, "", first.history).unwrap();
    assert_eq!(r.history.len(), 1);
    assert_eq!(r.history[0].total, 75_000.0);
}

#[test]
fn options_rank_by_first_year_effect() {
    let mut i = input();
    let mut fee = opt("fee", 900.0);
    fee.cost_once = 600.0;
    i.options = vec![opt("small", 100.0), fee, opt("big", 2_000.0)];
    let r = net_total::compute(&i, "", Vec::new()).unwrap();
    let ids: Vec<&str> = r.ranked.iter().map(|x| x.option.id.as_str()).collect();
    assert_eq!(ids, vec!["big", "fee", "small"]);
    assert_eq!(r.ranked[1].net_effect, 300.0);
}

#[test]
fn burnout_limit_refuses_extra_work() {
    let mut i = input();
    let mut gig = opt("gig", 20_000.0);
    gig.work_hours_week = 10.0;
    let mut light = opt("light", 2_000.0);
    light.work_hours_week = 3.0;
    i.options = vec![gig, light];
    let profile = "[boundaries]\nwork_hours_week = 40\nmax_work_hours_week = 45\n";
    let r = net_total::compute(&i, profile, Vec::new()).unwrap();
    assert_eq!(r.ranked.len(), 1);
    assert_eq!(r.ranked[0].option.id, "light");
    assert!(
        r.refused[0].refused[0].contains("no burnout"),
        "{:?}",
        r.refused
    );
}

#[test]
fn unknown_hours_refuse_extra_work() {
    let mut i = input();
    let mut gig = opt("gig", 1.0);
    gig.work_hours_week = 1.0;
    i.options = vec![gig];
    let r = net_total::compute(&i, "", Vec::new()).unwrap();
    assert!(r.refused[0].refused[0].contains("work_hours_week"));
}

#[test]
fn cash_floor_with_margin_sizes_a_sweep() {
    let mut i = input();
    let mut sweep = opt("sweep", 0.0);
    sweep.per_dollar = Some(0.03);
    sweep.cash_out = 15_000.0;
    i.options = vec![sweep];
    // Cash 20,000; floor 10,000 + 5 % = 10,500; so at most 9,500 moves.
    let r = net_total::compute(&i, "", Vec::new()).unwrap();
    assert_eq!(r.ranked[0].option.cash_out, 9_500.0);
    assert_eq!(r.ranked[0].net_effect, 285.0);
}

#[test]
fn margin_is_capped_at_20_percent() {
    let err = net_total::compute(&input(), "[boundaries]\nmargin = 25\n", Vec::new()).unwrap_err();
    assert!(err.contains("0 to 20"), "{err}");
}

#[test]
fn a_fixed_cash_move_below_the_floor_is_refused() {
    let mut i = input();
    let mut down = opt("down-payment", 4_000.0);
    down.cash_out = 15_000.0;
    i.options = vec![down];
    let r = net_total::compute(
        &i,
        "[boundaries]\nmin_cash = 12000\nmargin = 10\n",
        Vec::new(),
    )
    .unwrap();
    assert!(r.ranked.is_empty());
    assert!(
        r.refused[0].refused[0].contains("above your floor"),
        "{:?}",
        r.refused
    );
}

#[test]
fn home_debt_risk_and_tags_are_checked() {
    let mut i = input();
    let mut sell = opt("sell-home", 50_000.0);
    sell.sells_home = true;
    let mut loan = opt("car-loan", 100.0);
    loan.new_debt = 20_000.0;
    let mut coin = opt("coin", 3_000.0);
    coin.risk = Some("speculative".into());
    let mut lever = opt("lever", 5_000.0);
    lever.tags = vec!["Leverage".into()];
    i.options = vec![sell, loan, coin, lever];
    let r = net_total::compute(&i, "[boundaries]\nrefuse = [\"leverage\"]\n", Vec::new()).unwrap();
    assert!(r.ranked.is_empty());
    let reasons: Vec<String> = r.refused.iter().map(|x| x.refused.join(" ")).collect();
    assert!(reasons.iter().any(|s| s.contains("sells your home")));
    assert!(reasons.iter().any(|s| s.contains("of debt")));
    assert!(
        reasons
            .iter()
            .any(|s| s.contains("above your limit (growth)"))
    );
    assert!(reasons.iter().any(|s| s.contains("\"Leverage\"")));
    // Refused ones still rank by effect, so the owner sees what each boundary costs.
    assert_eq!(r.refused[0].option.id, "sell-home");
}

#[test]
fn owner_ideas_join_the_options() {
    let profile = r#"
[boundaries]
work_hours_week = 38
[[net_total.ideas]]
id = "evening-course"
title = "CANARY-IDEA"
effect_year = 4000
work_hours_week = 5
"#;
    let r = net_total::compute(&input(), profile, Vec::new()).unwrap();
    assert_eq!(r.ranked[0].option.title, "CANARY-IDEA");
    assert_eq!(r.ranked[0].option.section, "your idea");
}

#[test]
fn render_and_headline_show_the_total() {
    let mut i = input();
    let mut gig = opt("gig", 9_000.0);
    gig.work_hours_week = 20.0;
    i.options = vec![opt("ok", 120.0), gig];
    i.items[0].as_of = Some("2026-08-01".into());
    let (out, history) = net_total::run(
        &serde_json::to_string(&serde_json::json!({
            "today": i.today, "items": i.items, "options": i.options, "reserve": 10000.0
        }))
        .unwrap(),
        "[boundaries]\nwork_hours_week = 40\n",
        None,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["total"], 72_000.0);
    let md = v["markdown"].as_str().unwrap();
    assert!(md.starts_with("## Net total: $72,000"), "{md}");
    assert!(md.contains("| 1 | Option ok | +$120 | test |"), "{md}");
    assert!(md.contains("~~Option gig~~"), "{md}");
    assert!(md.contains("CANARY-CHEQUING"), "{md}");
    let head = v["headline"].as_str().unwrap();
    assert!(head.starts_with("Net total: $72,000"), "{head}");
    assert!(head.contains("1 source(s) stale"), "{head}");
    let saved: Vec<Point> = serde_json::from_str(&history).unwrap();
    assert_eq!(saved.len(), 1);
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(net_total::run("{}", "", None).is_err());
    assert!(
        net_total::run(
            "{\"today\":\"2026-11-01\"}",
            "[boundaries]\nmax_risk = \"yolo\"\n",
            None
        )
        .is_err()
    );
}
