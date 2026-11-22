//! Investment recommender: synthetic inputs only. Covers the honesty rules
//! of decision 0013: ranges, evidence, do-nothing as a ranked option,
//! risk moves with no expected gain, and overrides.

use cashcast_engine::invest_plan::{self, Input, plan, render};
use serde_json::json;

fn facts() -> serde_json::Value {
    let f = |value: f64, conf: &str| json!({"value": value, "text": "synthetic fact", "source": "https://example.com", "as_of": "2026-01-01", "review_by": "2027-04-30", "confidence": conf});
    json!({
        "pag-equity": f(0.066, "verified"),
        "pag-fixed-income": f(0.032, "verified"),
        "pag-short-term": f(0.024, "verified"),
        "pag-inflation": f(0.021, "verified"),
        "return-band": f(0.02, "heuristic"),
        "one-year-drop": f(-0.42, "estimate"), "crypto-drawdown": f(-0.7, "estimate"), "fhsa": f(8000.0, "verified"),
        "concentration-limit": f(0.10, "heuristic"),
        "rebalance": f(0.05, "heuristic"),
        "us-withholding": f(0.15, "verified"),
        "us-dividend-yield": {"value": 0.012, "low": 0.010, "high": 0.015, "text": "t", "source": "https://example.com", "as_of": "2026-01-01", "review_by": "2027-04-30", "confidence": "estimate"},
        "fees-predict": f(0.0, "verified"), "etf-mers": f(0.0019, "verified"), "broker-trade-fee": f(0.0, "verified"), "single-stock-drop": f(0.5, "heuristic"), "cash-rate-capture": f(0.5, "heuristic"), "gain-deferral-share": f(0.5, "heuristic"), "mer-lag": f(0.8, "heuristic"), "high-interest-debt-rate": f(0.08, "heuristic"),
        "single-stock-risk": f(0.0, "verified"),
        "spiva-canada": f(0.988, "verified"),
        "superficial-loss": f(30.0, "verified"),
        "harvest-defers": f(0.0, "heuristic"),
        "tfsa-shelter": f(0.0, "verified"),
        "in-kind-loss": f(0.0, "verified"),
        "norbert": f(0.001, "estimate"),
        "broker-fx-fee": f(0.015, "verified"),
        "short-horizon": f(60.0, "heuristic"),
    })
}

fn input(extra: serde_json::Value) -> Input {
    let mut base = json!({
        "today": "2026-11-01",
        "horizon_months": 360.0,
        "marginal": 0.43,
        "inclusion": 0.5,
        "target_equity": 0.8,
        "target_source": "your risk level (growth)",
        "simplicity": 50.0,
        "high_interest_debt": 0.0,
        "cash_rate": 0.02,
        "room": [{"account": "TFSA", "room": 7000.0, "why": "tax-free"}, {"account": "RRSP", "room": 20000.0}],
        "cheapest": {"equity": {"ticker": "XEQT", "mer": 0.0019}, "bond": {"ticker": "ZAG", "mer": 0.0009}},
        "facts": facts(),
    });
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).unwrap()
}

#[test]
fn do_nothing_wins_when_there_is_nothing_to_do() {
    let i = input(json!({"cash": 1000.0, "reserve": 5000.0}));
    let p = plan(&i);
    assert!(p.do_nothing_wins);
    assert_eq!(p.ranked[0].id, "do-nothing");
    assert!(render(&i, &p).contains("no move needed now"));
}

#[test]
fn do_nothing_wins_over_moves_below_the_simplicity_threshold() {
    // 300 above the reserve gains well under 50 a year even at the high end.
    let i = input(json!({"cash": 5300.0, "reserve": 5000.0}));
    let p = plan(&i);
    assert!(p.do_nothing_wins, "{:?}", p.ranked);
    let drag = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(drag.task.is_none());
    assert!(render(&i, &p).contains("Below your simplicity threshold"));
}

#[test]
fn cash_above_the_reserve_is_a_range_with_risk_and_evidence() {
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    let p = plan(&i);
    assert!(!p.do_nothing_wins);
    let m = &p.ranked[0];
    assert_eq!(m.id, "cash-drag");
    assert!(m.gain_low < m.gain_high);
    assert!(m.gain_low > 0.0);
    assert!(m.risk.contains("bad year"));
    assert!(m.task.as_deref().unwrap().contains("into TFSA"));
    let text = render(&i, &p);
    assert!(text.contains("not a licensed financial advisor"));
    assert!(text.contains("not a forecast"));
    assert!(text.contains("`return-band` (heuristic)"));
    assert!(text.contains("`one-year-drop` (estimate)"));
    for word in ["guaranteed", "will return", "will grow", "certain to"] {
        assert!(
            !text.to_lowercase().contains(word),
            "claims certainty: {word}"
        );
    }
}

#[test]
fn a_costly_fund_becomes_a_fee_move_and_a_cheap_one_does_not() {
    let i = input(json!({
        "buckets": {"TFSA": 50000.0},
        "positions": [
            {"symbol": "PRICEY", "bucket": "TFSA", "value": 40000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "domicile": "CA", "mer": 0.02},
            {"symbol": "XEQT", "bucket": "TFSA", "value": 10000.0, "basis": "market", "acb": 9000.0, "asset_class": "equity", "domicile": "CA", "mer": 0.0019}
        ]
    }));
    let p = plan(&i);
    let fee = p.ranked.iter().find(|m| m.id == "fee:PRICEY").unwrap();
    assert!((fee.gain_high - 40000.0 * (0.02 - 0.0019)).abs() < 0.01);
    assert_eq!(fee.cost_once, 0.0, "no tax in a TFSA");
    assert!(!p.ranked.iter().any(|m| m.id == "fee:XEQT"));
}

#[test]
fn selling_in_a_taxable_account_counts_the_tax() {
    let i = input(json!({
        "buckets": {"Non-registered": 40000.0},
        "positions": [{"symbol": "PRICEY", "bucket": "Non-registered", "value": 40000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}]
    }));
    let p = plan(&i);
    let fee = p.ranked.iter().find(|m| m.id == "fee:PRICEY").unwrap();
    let tax = 20000.0 * 0.5 * 0.43;
    assert!(
        fee.cost_once > 0.0 && fee.cost_once < tax * 0.2,
        "{}",
        fee.cost_once
    );
    assert!(fee.cost_note.contains("$4,300"), "{}", fee.cost_note);
}

#[test]
fn a_us_listed_fund_in_a_tfsa_loses_withholding() {
    let i = input(json!({
        "simplicity": 0.0,
        "buckets": {"TFSA": 50000.0, "RRSP": 60000.0},
        "positions": [{"symbol": "USFUND", "bucket": "TFSA", "value": 50000.0, "basis": "market", "acb": 40000.0, "asset_class": "equity", "domicile": "US", "mer": 0.0003}]
    }));
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "location:USFUND").unwrap();
    assert!((m.gain_low - 50000.0 * 0.010 * 0.15).abs() < 0.01);
    assert!((m.gain_high - 50000.0 * 0.015 * 0.15).abs() < 0.01);
    assert!(m.evidence.contains(&"norbert".to_string()));
    assert!(m.cost_once > 0.0, "currency conversion is free");
    assert!(m.task.as_deref().unwrap().contains("Norbert"));
}

#[test]
fn employer_concentration_is_a_risk_move_with_no_expected_gain() {
    let i = input(json!({
        "buckets": {"Non-registered": 60000.0, "TFSA": 40000.0},
        "employer": {"symbol": "EMPL", "value": 50000.0, "basis": "etrade-total", "acb": 45000.0}
    }));
    let p = plan(&i);
    assert!(p.ranked.iter().all(|m| !m.id.starts_with("concentration")));
    let m = &p.risk[0];
    assert_eq!(m.id, "concentration:EMPL");
    assert_eq!((m.gain_low, m.gain_high), (0.0, 0.0));
    assert!(m.why.contains("Expected gain about $0"));
    assert!(m.task.as_deref().unwrap().starts_with("Decide whether"));
    assert!(m.cost_once > 0.0);
}

#[test]
fn an_override_is_respected_with_its_cost_and_no_task() {
    let i = input(json!({
        "cash": 25000.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 60000.0, "TFSA": 40000.0},
        "employer": {"symbol": "EMPL", "value": 50000.0, "basis": "market", "acb": 45000.0},
        "overrides": [
            {"move": "concentration:EMPL", "reason": "CANARY-reason I accept the risk"},
            {"move": "cash-drag", "reason": "saving for a home in 2 years"}
        ]
    }));
    let p = plan(&i);
    assert!(p.risk.is_empty());
    assert_eq!(p.overridden.len(), 2);
    assert!(p.overridden.iter().all(|m| m.task.is_none()));
    let text = render(&i, &p);
    assert!(text.contains("## Your choices"));
    assert!(text.contains("CANARY-reason I accept the risk"));
    assert!(text.contains("forgone gain"));
    // No nagging: an overridden move appears once in the choices list, not in the ranking.
    assert_eq!(text.matches("`cash-drag`").count(), 1);
}

#[test]
fn an_override_by_kind_covers_every_move_of_that_kind() {
    let i = input(json!({
        "buckets": {"TFSA": 50000.0},
        "positions": [
            {"symbol": "AAA", "bucket": "TFSA", "value": 20000.0, "basis": "market", "acb": 1.0, "asset_class": "equity", "mer": 0.02},
            {"symbol": "BBB", "bucket": "TFSA", "value": 20000.0, "basis": "market", "acb": 1.0, "asset_class": "equity", "mer": 0.02}
        ],
        "overrides": [{"move": "fee", "reason": "I like these managers"}]
    }));
    let p = plan(&i);
    assert_eq!(p.overridden.len(), 2);
    assert!(!p.ranked.iter().any(|m| m.id.starts_with("fee:")));
}

#[test]
fn harvesting_employer_stock_warns_about_vests_within_30_days() {
    let i = input(json!({
        "buckets": {"Non-registered": 30000.0},
        "positions": [{"symbol": "EMPL", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": ""}],
        "employer": {"symbol": "EMPL", "value": 20000.0, "basis": "market", "acb": 30000.0}
    }));
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "harvest:EMPL").unwrap();
    assert!(m.unknowns.iter().any(|u| u.contains("RSU vest")));
    assert!(m.gain_low < m.gain_high);
    assert!(m.task.as_deref().unwrap().contains("30 days"));
    assert!(m.task.as_deref().unwrap().contains("RSU vest"));
}

#[test]
fn unknown_mix_and_rates_are_said_plainly() {
    let mut i = input(json!({
        "buckets": {"TFSA": 50000.0},
        "positions": [{"symbol": "ODD", "bucket": "TFSA", "value": 10000.0, "basis": "cost", "acb": 10000.0}]
    }));
    i.marginal = None;
    let p = plan(&i);
    let text = render(&i, &p);
    assert!(text.contains("What this plan does not know"));
    assert!(text.contains("marginal tax rate"));
    assert!(text.contains("stock/bond mix"));
    assert!(text.contains("[invest.prices]"));
}

#[test]
fn rebalancing_flags_drift_beyond_the_threshold() {
    let i = input(json!({
        "target_equity": 0.6,
        "buckets": {"RRSP": 100000.0},
        "positions": [{"symbol": "XEQT", "bucket": "RRSP", "value": 100000.0, "basis": "market", "acb": 80000.0, "asset_class": "equity", "mer": 0.0019}]
    }));
    let p = plan(&i);
    assert_eq!(p.risk[0].id, "rebalance");
    assert_eq!(p.risk[0].gain_high, 0.0);
}

#[test]
fn stale_or_missing_evidence_is_flagged() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    i.facts.get_mut("pag-equity").unwrap().review_by = "2026-01-01".into();
    i.facts.remove("one-year-drop");
    let text = render(&i, &plan(&i));
    assert!(text.contains("past its review date"));
    assert!(text.contains("`one-year-drop`: missing"));
}

#[test]
fn run_prints_json_with_markdown_and_moves() {
    let raw =
        serde_json::to_string(&json!({"target_equity": 0.8, "cash": 1.0, "reserve": 2.0})).unwrap();
    let out: serde_json::Value = serde_json::from_str(&invest_plan::run(&raw).unwrap()).unwrap();
    assert_eq!(out["do_nothing_wins"], true);
    assert!(
        out["markdown"]
            .as_str()
            .unwrap()
            .contains("Investment moves")
    );
    assert_eq!(out["moves"][0]["id"], "do-nothing");
}

#[test]
fn money_needed_soon_stays_out_of_stocks() {
    let mut i = input(json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01}));
    i.horizon_months = Some(10.0);
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(m.title.contains("safe"), "{}", m.title);
    assert!((m.gain_high - 40000.0 * (0.024 - 0.01) * (1.0 - 0.43) * (10.0 / 12.0)).abs() < 0.01);
    assert!(m.evidence.contains(&"short-horizon".to_string()));
    assert!(!m.task.as_deref().unwrap().to_lowercase().contains("invest"));
}

#[test]
fn harvest_and_fee_on_one_fund_are_one_sale() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "positions": [{"symbol": "PRICEY", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "mer": 0.02}]
    }));
    let p = plan(&i);
    let h = p.ranked.iter().find(|m| m.id == "harvest:PRICEY").unwrap();
    assert!(h.assumptions.iter().any(|a| a.contains("one sale")));
    assert_eq!(render(&i, &p).matches("$0 (baseline)").count(), 2);
}

#[test]
fn veto_blocks_investing_while_high_interest_debt_is_open() {
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": 3000.0}));
    let p = plan(&i);
    assert!(p.ranked.iter().all(|m| m.id != "cash-drag"));
    let m = p.vetoed.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(m.vetoed.as_deref().unwrap().starts_with("debt first"));
    assert!(m.task.is_none());
    let text = render(&i, &p);
    assert!(text.contains("## Blocked by a safety rule"));
    assert!(text.contains("**Blocked:** debt first"));
}

#[test]
fn veto_blocks_a_move_whose_evidence_is_past_review() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    i.facts.get_mut("pag-equity").unwrap().review_by = "2026-01-01".into();
    let p = plan(&i);
    let m = p.vetoed.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(
        m.vetoed.as_deref().unwrap().contains("`pag-equity`"),
        "{:?}",
        m.vetoed
    );
}

#[test]
fn every_move_carries_its_strongest_objection() {
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    let p = plan(&i);
    for m in p.ranked.iter().chain(&p.risk) {
        assert!(!m.objection.is_empty(), "{}", m.id);
    }
    assert!(render(&i, &p).contains("**Strongest objection:**"));
}

#[test]
fn a_cited_fact_missing_from_knowledge_blocks_the_move() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    i.facts.remove("pag-equity");
    let p = plan(&i);
    let m = p.vetoed.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(m.vetoed.as_deref().unwrap().starts_with("missing evidence"));
}

#[test]
fn a_review_date_that_does_not_parse_counts_as_stale() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    // "2026-9-1" sorts after "2026-11-08" as text but is long past.
    i.facts.get_mut("pag-equity").unwrap().review_by = "2026-9-1".into();
    let p = plan(&i);
    assert!(p.vetoed.iter().any(|m| m.id == "cash-drag"));
}

#[test]
fn a_nan_or_zero_horizon_keeps_money_out_of_stocks() {
    for h in [f64::NAN, 0.0, -5.0] {
        let mut i = input(json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01}));
        i.horizon_months = Some(h);
        let p = plan(&i);
        let m = p
            .ranked
            .iter()
            .chain(&p.vetoed)
            .find(|m| m.id == "cash-drag")
            .unwrap();
        assert!(!m.title.starts_with("Invest"), "{h}: {}", m.title);
    }
}

#[test]
fn a_stale_risk_warning_stays_visible() {
    let mut i = input(json!({
        "buckets": {"Non-registered": 60000.0, "TFSA": 40000.0},
        "employer": {"symbol": "EMPL", "value": 50000.0, "basis": "market", "acb": 45000.0}
    }));
    for f in i.facts.values_mut() {
        f.review_by = "2026-01-01".into();
    }
    let p = plan(&i);
    assert!(p.risk.iter().any(|m| m.id == "concentration:EMPL"));
    assert!(p.vetoed.iter().all(|m| m.kind != "risk"));
}

#[test]
fn the_answer_comes_first_and_each_move_says_what_to_do() {
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    let p = plan(&i);
    let text = render(&i, &p);
    let answer = text.find("**Answer:").unwrap();
    assert!(answer < text.find("not personal financial advice").unwrap());
    assert!(answer < text.find("| # |").unwrap());
    if let Some(t) = p.ranked.iter().find_map(|m| m.task.clone()) {
        assert!(text.contains(&format!("- **Do:** {t}")));
    }
    assert!(text.contains("Long-run average gain"));
}

#[test]
fn disclosure_names_incentives_and_ai_and_harvest_warns_about_spouse_and_drip() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "positions": [{"symbol": "LOSS", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "mer": 0.002}]
    }));
    let p = plan(&i);
    let text = render(&i, &p);
    assert!(text.contains("No provider pays"));
    assert!(text.contains("AI model"));
    let h = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "harvest:LOSS")
        .unwrap();
    let task = h.task.as_deref().unwrap_or_default();
    assert!(task.contains("spouse") && task.contains("DRIP"), "{task}");
}

#[test]
fn a_symbol_override_never_hides_a_risk_warning_and_overrides_expire() {
    let base = json!({
        "buckets": {"Non-registered": 60000.0, "TFSA": 40000.0},
        "employer": {"symbol": "EMPL", "value": 50000.0, "basis": "market", "acb": 45000.0}
    });
    assert!(
        plan(&input(base.clone()))
            .risk
            .iter()
            .any(|m| m.id == "concentration:EMPL")
    );
    let mut i = input(base.clone());
    i.overrides = serde_json::from_value(json!([{"move": "EMPL", "reason": ""}])).unwrap();
    assert!(plan(&i).risk.iter().any(|m| m.id == "concentration:EMPL"));

    let mut i = input(base.clone());
    i.overrides = serde_json::from_value(json!([{"move": "concentration:EMPL", "reason": "selling after vest", "until": "2026-01-01"}])).unwrap();
    assert!(
        plan(&i).risk.iter().any(|m| m.id == "concentration:EMPL"),
        "expired"
    );

    let mut i = input(base);
    i.overrides = serde_json::from_value(json!([{"move": "concentration:EMPL", "reason": "selling after vest", "until": "2099-01-01"}])).unwrap();
    assert!(
        plan(&i)
            .overridden
            .iter()
            .any(|m| m.id == "concentration:EMPL")
    );
}

#[test]
fn crypto_is_its_own_part_of_the_mix_and_is_checked_against_a_cap() {
    let positions = json!([
        {"symbol": "XEQT", "bucket": "TFSA", "value": 62000.0, "basis": "market", "asset_class": "equity"},
        {"symbol": "ZAG", "bucket": "RRSP", "value": 18000.0, "basis": "market", "asset_class": "bond"},
        {"symbol": "BTC", "bucket": "non-registered", "value": 20000.0, "basis": "market", "asset_class": "crypto"},
    ]);
    // No cap set: the whole mix is shown and the missing cap is an unknown.
    let i = input(json!({"positions": positions}));
    let p = plan(&i);
    let md = render(&i, &p);
    assert!(
        md.contains("Your whole mix: stocks 62% · bonds 18% · crypto 20% (BTC)"),
        "{md}"
    );
    assert!(
        p.unknowns.iter().any(|u| u.contains("crypto_max")),
        "{:?}",
        p.unknowns
    );
    assert!(!p.risk.iter().any(|m| m.id == "crypto"));
    // Stocks share leaves crypto out: 62 / 80 = 77.5%, inside the 80% band.
    assert!(!p.risk.iter().any(|m| m.id == "rebalance"));
    // A cap below the share gives a risk move with a task and no gain.
    let i = input(json!({"positions": positions, "crypto_max": 0.10}));
    let p = plan(&i);
    let m = p
        .risk
        .iter()
        .find(|m| m.id == "crypto")
        .expect("crypto risk move");
    assert!(
        m.title.contains("20%") && m.title.contains("10%"),
        "{}",
        m.title
    );
    assert!(m.task.as_deref().unwrap_or("").contains("new money"));
    assert!(m.cost_note.contains("disposition"));
}

#[test]
fn holdings_of_unknown_kind_are_named() {
    let i = input(json!({"positions": [
        {"symbol": "XEQT", "bucket": "TFSA", "value": 1000.0, "basis": "market", "asset_class": "equity"},
        {"symbol": "ACME", "bucket": "TFSA", "value": 1000.0, "basis": "market", "asset_class": ""},
    ]}));
    let p = plan(&i);
    assert!(
        p.unknowns
            .iter()
            .any(|u| u.contains("The kind of ACME") && u.contains("[invest.classes]")),
        "{:?}",
        p.unknowns
    );
    assert!(render(&i, &p).contains("of unknown kind 50%"));
}

#[test]
fn money_needed_soon_names_the_fhsa_when_there_is_room() {
    let mut i = input(
        json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01,
        "room": [{"account": "FHSA", "room": 8000.0}, {"account": "TFSA", "room": 7000.0}]}),
    );
    i.horizon_months = Some(10.0);
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(
        m.why.contains("FHSA") && m.why.contains("$3,440"),
        "{}",
        m.why
    );
    assert!(
        m.task
            .as_deref()
            .unwrap()
            .contains("up to $8,000 of it in your FHSA")
    );
    assert!(!m.task.as_deref().unwrap().to_lowercase().contains("invest"));
    assert!(
        !m.task.as_deref().unwrap().contains("GIC"),
        "a GIC would mature after the money is needed"
    );
    assert!(m.evidence.contains(&"fhsa".to_string()));
}

#[test]
fn the_plan_says_when_it_was_made_and_how_old_its_evidence_is() {
    let i = input(json!({}));
    let md = render(&i, &plan(&i));
    assert!(
        md.contains("Made on 2026-11-01; oldest evidence as of 2026-01-01."),
        "{md}"
    );
}

#[test]
fn jargon_is_explained_the_first_time_only() {
    let mut i = input(
        json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01,
        "room": [{"account": "FHSA", "room": 8000.0}]}),
    );
    i.horizon_months = Some(10.0);
    let md = render(&i, &plan(&i));
    assert_eq!(
        md.matches("FHSA (First Home Savings Account)").count(),
        1,
        "{md}"
    );
    assert!(
        md.contains("GIC (a deposit with a fixed rate and term)"),
        "{md}"
    );
    assert!(
        !md.lines()
            .any(|l| l.starts_with('#') && l.contains("(a deposit"))
    );
}

#[test]
fn missing_live_rates_are_named_as_an_unknown() {
    let i = input(json!({}));
    assert!(
        plan(&i)
            .unknowns
            .iter()
            .any(|u| u.contains("knowledge refresh"))
    );
    let mut f = facts();
    f["live-gic-1y"] = json!({"value": 0.03, "text": "t", "source": "https://example.com", "as_of": "2026-10-31", "review_by": "2026-11-14", "confidence": "verified"});
    let i = input(json!({"facts": f}));
    assert!(
        !plan(&i)
            .unknowns
            .iter()
            .any(|u| u.contains("knowledge refresh"))
    );
}

#[test]
fn round_two_judge_fixes_hold() {
    let base = json!({
        "cash": 25000.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 60000.0, "TFSA": 40000.0},
        "employer": {"symbol": "EMPL", "value": 50000.0, "basis": "market", "acb": 45000.0}
    });
    // A bare symbol never hides a risk warning, even with a reason.
    let mut i = input(base.clone());
    i.overrides = serde_json::from_value(json!([{"move": "EMPL", "reason": "fee only"}])).unwrap();
    assert!(plan(&i).risk.iter().any(|m| m.id == "concentration:EMPL"));
    // An unreadable until date is named, and the override is ignored.
    let mut i = input(base.clone());
    i.overrides = serde_json::from_value(
        json!([{"move": "concentration:EMPL", "reason": "r", "until": "next March"}]),
    )
    .unwrap();
    let p = plan(&i);
    assert!(p.unknowns.iter().any(|u| u.contains("next March")));
    assert!(p.risk.iter().any(|m| m.id == "concentration:EMPL"));
    // A bad today: expiring overrides fail closed.
    let mut i = input(base.clone());
    i.today = "soon".into();
    i.overrides = serde_json::from_value(
        json!([{"move": "concentration:EMPL", "reason": "r", "until": "2099-01-01"}]),
    )
    .unwrap();
    assert!(plan(&i).risk.iter().any(|m| m.id == "concentration:EMPL"));
    // A veto beats an override.
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": 3000.0}));
    i.overrides = serde_json::from_value(json!([{"move": "cash-drag", "reason": "r"}])).unwrap();
    assert!(plan(&i).vetoed.iter().any(|m| m.id == "cash-drag"));
    // No horizon: investing spare cash is blocked until it is set.
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    i.horizon_months = None;
    let p = plan(&i);
    assert!(p.vetoed.iter().any(|m| {
        m.vetoed
            .as_deref()
            .unwrap_or("")
            .starts_with("horizon first")
    }));
    // A percent written as 5 is refused, not read as 500%.
    let i = input(json!({"crypto_max": 5.0, "positions": [
        {"symbol": "BTC", "bucket": "non-registered", "value": 50.0, "basis": "market", "asset_class": "crypto"},
        {"symbol": "XEQT", "bucket": "TFSA", "value": 50.0, "basis": "market", "asset_class": "equity"}]}));
    assert!(
        plan(&i)
            .unknowns
            .iter()
            .any(|u| u.contains("not a fraction"))
    );
    // Gold and crypto count as classified for the mix check.
    let i = input(json!({"positions": [
        {"symbol": "BTC", "bucket": "non-registered", "value": 60.0, "basis": "market", "asset_class": "crypto"},
        {"symbol": "XEQT", "bucket": "TFSA", "value": 40.0, "basis": "market", "asset_class": "equity"}]}));
    assert!(
        !plan(&i)
            .unknowns
            .iter()
            .any(|u| u.contains("have a known asset class"))
    );
    // The headline says "a year" once, carries the caveat, and never prints -0%.
    let mut i = input(json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01}));
    i.horizon_months = Some(10.0);
    let md = render(&i, &plan(&i));
    let first = md.lines().find(|l| l.starts_with("**Answer")).unwrap();
    assert!(
        !md.contains("a year a year") && first.contains("Not personal advice"),
        "{first}"
    );
    assert!(!md.contains("-0%"));
}

#[test]
fn a_broad_override_says_what_it_matched() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    i.overrides = serde_json::from_value(json!([{"move": "cash-drag", "reason": "r"}])).unwrap();
    assert!(!render(&i, &plan(&i)).contains("matched by"));
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "positions": [{"symbol": "PRICEY", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "mer": 0.02}],
        "overrides": [{"move": "PRICEY", "reason": "keep it"}]
    }));
    let p = plan(&i);
    assert!(p.overridden.len() >= 2, "{:?}", p.overridden.len());
    assert!(p.overridden.iter().all(|m| {
        m.overridden
            .as_deref()
            .unwrap()
            .contains("matched by `PRICEY`")
    }));
}

#[test]
fn round_four_judge_fixes_hold() {
    // Money needed within a year is not sent to a 1-year GIC.
    let mut i = input(json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01}));
    i.horizon_months = Some(6.0);
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(!m.task.as_deref().unwrap().contains("GIC"), "{:?}", m.task);
    // A loss already flagged as superficial is not harvested.
    let pos = json!([{"symbol": "PRICEY", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity"}]);
    let i = input(json!({"buckets": {"Non-registered": 20000.0}, "positions": pos.clone()}));
    assert!(plan(&i).ranked.iter().any(|m| m.id == "harvest:PRICEY"));
    let i = input(
        json!({"buckets": {"Non-registered": 20000.0}, "positions": pos, "superficial": ["PRICEY ×2"]}),
    );
    let p = plan(&i);
    assert!(
        !p.ranked
            .iter()
            .chain(&p.vetoed)
            .any(|m| m.id == "harvest:PRICEY")
    );
    // No marginal rate: the safe-cash gain says it is before tax.
    let mut i = input(json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.01}));
    i.horizon_months = Some(10.0);
    i.marginal = None;
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(
        m.assumptions[0].starts_with("Before tax"),
        "{:?}",
        m.assumptions
    );
}

#[test]
fn debt_first_also_blocks_parking_cash_needed_soon() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": 8000.0}));
    i.horizon_months = Some(24.0);
    let p = plan(&i);
    let m = p
        .vetoed
        .iter()
        .find(|m| m.id == "cash-drag")
        .expect("vetoed");
    assert!(m.vetoed.as_deref().unwrap().starts_with("debt first"));
}

#[test]
fn moving_taxable_funds_to_the_tfsa_ignores_crypto_in_the_same_bucket() {
    let i = input(json!({
        "buckets": {"Non-registered": 10000.0, "Crypto": 60000.0},
        "positions": [
            {"symbol": "XEQT", "bucket": "non-registered", "value": 10000.0, "basis": "market", "acb": 10000.0, "asset_class": "equity"},
            {"symbol": "BTC", "bucket": "non-registered", "value": 60000.0, "basis": "market", "acb": 10000.0, "asset_class": "crypto"}
        ]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    assert!(
        m.cost_once < 1.0,
        "BTC gain taxed in a fund move: {}",
        m.cost_once
    );
}

#[test]
fn moving_taxable_funds_to_the_tfsa_is_not_blocked_by_a_low_reserve() {
    let i = input(json!({
        "cash": 500.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 40000.0},
        "positions": [
            {"symbol": "XEQT", "bucket": "non-registered", "value": 40000.0, "basis": "market", "acb": 40000.0, "asset_class": "equity"}
        ]
    }));
    let p = plan(&i);
    let blocked: Vec<_> = p.vetoed.iter().map(|m| (&m.id, &m.vetoed)).collect();
    assert!(p.ranked.iter().any(|m| m.id == "fill-room"), "{blocked:#?}");
}

#[test]
fn a_taxable_bond_swap_counts_the_stock_tax_and_the_sale_tax() {
    let i = input(json!({
        "buckets": {"Non-registered": 100000.0},
        "positions": [
            {"symbol": "ZAG", "bucket": "non-registered", "value": 100000.0, "basis": "market", "acb": 80000.0, "asset_class": "bond"}
        ]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .chain(&p.risk)
        .find(|m| m.id == "location:ZAG")
        .expect("location:ZAG");
    let mr = i.marginal.unwrap();
    assert!(m.cost_once > 0.0, "sale tax missing");
    assert!(
        m.cost_once < 20000.0 * 0.5 * mr * 0.2,
        "full tax counted: {}",
        m.cost_once
    );
    assert!(m.cost_note.contains("due anyway"), "{}", m.cost_note);
    assert!(
        m.gain_high < 100000.0 * (0.032 + 0.02) * mr,
        "stock tax ignored: {}",
        m.gain_high
    );
    assert!(m.gain_low <= m.gain_high);
}

#[test]
fn filling_the_tfsa_with_a_taxable_bond_is_one_sale_not_two() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 30000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "ZAG", "bucket": "non-registered", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "bond"}
        ]
    }));
    let p = plan(&i);
    let all: Vec<_> = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .chain(&p.risk)
        .map(|m| m.id.as_str())
        .collect();
    assert!(all.contains(&"fill-room"), "{all:?}");
    assert!(!all.contains(&"location:ZAG"), "double-counted: {all:?}");
}

#[test]
fn a_skipped_tfsa_move_leaves_the_fee_swap_its_own_sale_tax() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 30000.0}],
        "positions": [
            {"symbol": "XYZ", "bucket": "non-registered", "value": 20000.0, "basis": "market", "acb": 10000.0, "asset_class": "equity", "mer": 0.02}
        ],
        "overrides": [{"move": "fill-room", "reason": "keep it liquid"}]
    }));
    let p = plan(&i);
    let all: Vec<_> = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .chain(&p.risk)
        .chain(&p.overridden)
        .collect();
    let fee = all.iter().find(|m| m.id == "fee:XYZ").expect("fee:XYZ");
    assert!(fee.cost_once > 0.0, "sale tax vanished: {}", fee.cost_note);
}

fn every_id(p: &cashcast_engine::invest_plan::Plan) -> Vec<String> {
    p.ranked
        .iter()
        .chain(&p.vetoed)
        .chain(&p.risk)
        .chain(&p.overridden)
        .map(|m| m.id.clone())
        .collect()
}

#[test]
fn filling_the_tfsa_keeps_the_tfsa_withholding_move_and_drops_the_taxable_loss_sale() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0, "TFSA": 50000.0, "RRSP": 60000.0},
        "room": [{"account": "TFSA", "room": 30000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "USFUND", "bucket": "TFSA", "value": 50000.0, "basis": "market", "acb": 40000.0, "asset_class": "equity", "domicile": "US", "mer": 0.0003},
            {"symbol": "USFUND", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 25000.0, "asset_class": "equity", "domicile": "US", "mer": 0.0003}
        ]
    }));
    let ids = every_id(&plan(&i));
    assert!(ids.contains(&"fill-room".to_string()), "{ids:?}");
    assert!(
        ids.iter().any(|x| x.starts_with("location:USFUND")),
        "TFSA move lost: {ids:?}"
    );
    assert!(
        !ids.iter().any(|x| x.starts_with("harvest:")),
        "loss counted twice: {ids:?}"
    );
}

#[test]
fn the_same_fund_in_two_accounts_gets_two_ids() {
    let i = input(json!({
        "buckets": {"TFSA": 20000.0, "RRSP": 20000.0},
        "positions": [
            {"symbol": "PRICEY", "bucket": "TFSA", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02},
            {"symbol": "PRICEY", "bucket": "RRSP", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}
        ]
    }));
    let ids: Vec<String> = every_id(&plan(&i))
        .into_iter()
        .filter(|x| x.starts_with("fee:"))
        .collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn an_override_still_skips_a_fund_held_in_two_accounts() {
    for target in ["PRICEY", "fee:PRICEY", "fee"] {
        let i = input(json!({
            "buckets": {"TFSA": 20000.0, "RRSP": 20000.0},
            "positions": [
                {"symbol": "PRICEY", "bucket": "TFSA", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02},
                {"symbol": "PRICEY", "bucket": "RRSP", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}
            ],
            "overrides": [{"move": target, "reason": "keeping it"}]
        }));
        let p = plan(&i);
        assert_eq!(
            p.overridden
                .iter()
                .filter(|m| m.id.starts_with("fee:"))
                .count(),
            2,
            "{target}"
        );
    }
    let i = input(json!({
        "buckets": {"TFSA": 20000.0, "RRSP": 20000.0},
        "positions": [
            {"symbol": "PRICEY", "bucket": "TFSA", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02},
            {"symbol": "PRICEY", "bucket": "RRSP", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}
        ],
        "overrides": [{"move": "fee:PRICEY:tfsa", "reason": "keeping it"}]
    }));
    assert_eq!(plan(&i).overridden.len(), 1);
}

#[test]
fn a_per_account_override_holds_after_the_other_account_sells() {
    let i = input(json!({
        "buckets": {"TFSA": 20000.0},
        "positions": [
            {"symbol": "PRICEY", "bucket": "TFSA", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}
        ],
        "overrides": [{"move": "fee:PRICEY:tfsa", "reason": "keeping it"}]
    }));
    assert_eq!(plan(&i).overridden.len(), 1);
}

#[test]
fn a_us_fund_swap_is_capped_by_what_the_rrsp_can_take() {
    let i = input(json!({
        "buckets": {"TFSA": 60000.0, "RRSP": 5000.0},
        "room": [{"account": "RRSP", "room": 0.0}],
        "simplicity": 0.0,
        "positions": [{"symbol": "USFUND", "bucket": "TFSA", "value": 60000.0, "basis": "market", "acb": 40000.0, "asset_class": "equity", "domicile": "US", "mer": 0.0003}]
    }));
    let p = plan(&i);
    let all: Vec<_> = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .chain(&p.risk)
        .chain(&p.overridden)
        .collect();
    let m = all
        .iter()
        .find(|m| m.id == "location:USFUND")
        .expect("move");
    assert!(
        m.gain_high <= 5000.0 * 0.015 * 0.15 + 0.01,
        "sized on the whole holding: {}",
        m.gain_high
    );
    let task = m.task.as_deref().unwrap();
    assert!(
        task.contains("$5,000 of USFUND"),
        "no partial amount: {task}"
    );

    let none = input(json!({
        "buckets": {"TFSA": 60000.0},
        "room": [],
        "positions": [{"symbol": "USFUND", "bucket": "TFSA", "value": 60000.0, "basis": "market", "acb": 40000.0, "asset_class": "equity", "domicile": "US", "mer": 0.0003}]
    }));
    let p = plan(&none);
    assert!(
        !p.ranked
            .iter()
            .chain(&p.vetoed)
            .any(|m| m.id == "location:USFUND"),
        "impossible swap offered"
    );
}

#[test]
fn a_stale_fact_blocks_every_move_whose_math_reads_it() {
    let base = json!({
        "cash": 5000.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 30000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "XEQT", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 10000.0, "asset_class": "equity"},
            {"symbol": "LOSER", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity"}
        ]
    });
    for (fact, id) in [
        ("pag-short-term", "harvest:LOSER"),
        ("pag-fixed-income", "fill-room"),
    ] {
        let mut i = input(base.clone());
        i.room.clear();
        if id == "fill-room" {
            i = input(base.clone());
        }
        i.facts.get_mut(fact).unwrap().review_by = "2026-01-01".into();
        let p = plan(&i);
        let ok = p.vetoed.iter().any(|m| m.id == id)
            || !p.ranked.iter().chain(&p.risk).any(|m| m.id == id);
        assert!(ok, "{id} still ranked on stale {fact}");
    }
}

#[test]
fn a_stale_short_horizon_blocks_investing_cash() {
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    i.facts.get_mut("short-horizon").unwrap().review_by = "2026-01-01".into();
    let p = plan(&i);
    assert!(
        p.vetoed.iter().any(|m| m.id == "cash-drag"),
        "cash-drag still ranked on a stale short-horizon"
    );
}

#[test]
fn filling_the_tfsa_keeps_a_coin_loss_sale() {
    let i = input(json!({
        "buckets": {"Non-registered": 30000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "XEQT", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 10000.0, "asset_class": "equity"},
            {"symbol": "BTC", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 40000.0, "asset_class": "crypto"}
        ]
    }));
    let p = plan(&i);
    let ids: Vec<_> = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .chain(&p.risk)
        .map(|m| m.id.as_str())
        .collect();
    assert!(ids.contains(&"fill-room"), "{ids:?}");
    assert!(
        ids.contains(&"harvest:BTC"),
        "coin loss sale dropped: {ids:?}"
    );
}

#[test]
fn the_tfsa_task_says_to_sell_losers_first_not_in_kind() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "XEQT", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 10000.0, "asset_class": "equity"},
            {"symbol": "LOSER", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity"}
        ]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    let task = m.task.as_deref().unwrap_or("");
    assert!(
        task.contains("LOSER") && task.contains("not in kind"),
        "{task}"
    );
    assert!(m.assumptions.iter().any(|a| a.contains("before or after")));
}

#[test]
fn money_needed_soon_is_not_called_a_long_run_gain() {
    let mut i =
        input(json!({"cash": 45000.0, "reserve": 5000.0, "cash_rate": 0.0, "simplicity": 0.0}));
    i.horizon_months = Some(8.0);
    let text = render(&i, &plan(&i));
    let answer = text
        .lines()
        .find(|l| l.contains("Answer: do move"))
        .unwrap_or("");
    assert!(answer.contains("until you need it"), "{answer}");
}

#[test]
fn the_crypto_risk_move_cites_a_dated_fact() {
    let i = input(json!({
        "crypto_max": 0.05,
        "buckets": {"Non-registered": 20000.0},
        "positions": [
            {"symbol": "XEQT", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 10000.0, "asset_class": "equity"},
            {"symbol": "BTC", "bucket": "Non-registered", "value": 10000.0, "basis": "market", "acb": 5000.0, "asset_class": "crypto"}
        ]
    }));
    let p = plan(&i);
    let m = p
        .risk
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "crypto")
        .expect("crypto");
    assert!(m.evidence.contains(&"crypto-drawdown".to_string()));
}

#[test]
fn filling_the_tfsa_taxes_stock_gains_at_the_inclusion_rate() {
    let i = input(json!({
        "buckets": {"Non-registered": 5000.0},
        "room": [{"account": "TFSA", "room": 30000.0}],
        "simplicity": 0.0,
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    // 80% stocks return under 10% a year even at the high end of the band.
    let cap = 5000.0 * 0.10 * 0.5 * 0.43;
    assert!(
        m.gain_high <= cap,
        "full-rate tax on gains: {}",
        m.gain_high
    );
}

#[test]
fn filling_the_tfsa_with_a_loss_holding_names_the_30_day_rule_in_the_task() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "LOSER", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 25000.0, "asset_class": "equity"}
        ]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    let task = m.task.as_deref().unwrap();
    assert!(task.contains("30 days"), "no superficial-loss rule: {task}");
}

#[test]
fn the_skill_example_override_carries_an_until_date() {
    let skill = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../skills/invest-challenge/SKILL.md"
    ))
    .unwrap();
    let ex = skill.split("## Example").nth(1).unwrap();
    assert!(ex.contains("until ="), "example hides the move forever");
}

#[test]
fn a_fee_swap_inside_the_tfsa_move_says_to_buy_in_the_tfsa() {
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 30000.0}],
        "simplicity": 0.0,
        "positions": [
            {"symbol": "PRICEY", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}
        ]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id.starts_with("fee:PRICEY"))
        .expect("fee move");
    let task = m.task.as_deref().unwrap();
    assert!(
        task.contains("in the TFSA"),
        "buys in the taxable account: {task}"
    );
}

#[test]
fn a_blocked_cash_move_leaves_the_tfsa_room_for_the_taxable_account() {
    let i = input(json!({
        "cash": 25000.0, "reserve": 5000.0,
        "horizon_months": null,
        "buckets": {"Non-registered": 15000.0},
        "room": [{"account": "TFSA", "room": 20000.0}],
        "simplicity": 0.0,
    }));
    let p = plan(&i);
    assert!(
        p.ranked
            .iter()
            .chain(&p.vetoed)
            .any(|m| m.id == "fill-room"),
        "fill-room lost to a blocked cash move"
    );
}

#[test]
fn the_invest_cash_task_names_the_mix() {
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    let p = plan(&i);
    let t = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    let task = t.task.as_deref().unwrap();
    assert!(task.contains("80% stocks"), "no mix: {task}");
}

#[test]
fn cash_that_goes_to_the_rrsp_first_leaves_the_tfsa_room_free() {
    let i = input(json!({
        "cash": 25000.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 15000.0},
        "room": [{"account": "RRSP", "room": 20000.0}, {"account": "TFSA", "room": 20000.0}],
        "simplicity": 0.0,
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fill-room")
        .expect("fill-room lost to cash bound for the RRSP");
    assert!(
        m.task.as_deref().unwrap().contains("$15,000"),
        "{:?}",
        m.task
    );
}

#[test]
fn a_large_gain_does_not_hide_the_tfsa_move() {
    let i = input(json!({
        "buckets": {"Non-registered": 50000.0},
        "room": [{"account": "TFSA", "room": 60000.0}],
        "positions": [
            {"symbol": "XEQT", "bucket": "Non-registered", "value": 50000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "mer": 0.0019}
        ]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    assert!(
        m.task.is_some(),
        "tax due anyway hid the move: net {}",
        m.net()
    );
    assert!(m.cost_note.contains("due anyway"));
}

#[test]
fn a_fee_swap_to_another_regions_fund_says_to_check() {
    let i = input(json!({
        "buckets": {"TFSA": 50000.0},
        "simplicity": 0.0,
        "positions": [{"symbol": "USEQ", "bucket": "TFSA", "value": 50000.0, "basis": "market", "acb": 40000.0, "asset_class": "equity", "region": "us", "domicile": "CA", "mer": 0.0025}]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .find(|m| m.id.starts_with("fee:USEQ"))
        .expect("fee");
    assert!(
        m.task.as_deref().unwrap().contains("same region"),
        "{:?}",
        m.task
    );
    assert!(m.evidence.contains(&"etf-mers".to_string()));
}

#[test]
fn cash_below_earlier_room_leaves_the_whole_tfsa_room() {
    let i = input(json!({
        "cash": 10000.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "RRSP", "room": 20000.0}, {"account": "TFSA", "room": 20000.0}],
        "simplicity": 0.0,
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    assert!(
        m.task.as_deref().unwrap().contains("$20,000"),
        "{:?}",
        m.task
    );
}

#[test]
fn a_balanced_fund_counts_at_its_stated_stock_share() {
    let i = input(json!({
        "buckets": {"TFSA": 50000.0},
        "positions": [{"symbol": "VGRO", "bucket": "TFSA", "value": 50000.0, "basis": "market", "acb": 40000.0, "asset_class": "balanced", "equity_share": 0.8, "domicile": "CA", "mer": 0.0024}]
    }));
    let p = plan(&i);
    assert!(
        !p.risk.iter().chain(&p.ranked).any(|m| m.id == "rebalance"),
        "an 80/20 fund at an 80% target was told to rebalance"
    );
    assert!(
        render(&i, &p).contains("stocks 80% · bonds 20%"),
        "{}",
        render(&i, &p)
    );
}

#[test]
fn tfsa_tasks_warn_about_room_used_this_year() {
    let i = input(json!({
        "cash": 25000.0, "reserve": 5000.0,
        "buckets": {"Non-registered": 5000.0},
        "room": [{"account": "TFSA", "room": 60000.0}],
        "simplicity": 0.0,
    }));
    let p = plan(&i);
    for id in ["cash-drag", "fill-room"] {
        let m = p.ranked.iter().find(|m| m.id == id).expect(id);
        assert!(
            m.task.as_deref().unwrap().contains("this year"),
            "{id}: {:?}",
            m.task
        );
    }
}

#[test]
fn the_trade_fee_fact_is_cited_only_for_registered_swaps() {
    let i = input(json!({
        "buckets": {"TFSA": 50000.0, "Non-registered": 50000.0},
        "simplicity": 0.0,
        "cheapest": {"equity:global": {"ticker": "XEQT", "mer": 0.0019}, "equity": {"ticker": "XEQT", "mer": 0.0019}, "bond": {"ticker": "ZAG", "mer": 0.0009}},
        "positions": [
            {"symbol": "PRICEY", "bucket": "TFSA", "value": 50000.0, "basis": "market", "acb": 50000.0, "asset_class": "equity", "region": "global", "mer": 0.02},
            {"symbol": "PRICEY", "bucket": "Non-registered", "value": 50000.0, "basis": "market", "acb": 50000.0, "asset_class": "equity", "region": "global", "mer": 0.02}
        ]
    }));
    let p = plan(&i);
    let fees: Vec<_> = p
        .ranked
        .iter()
        .filter(|m| m.id.starts_with("fee:"))
        .collect();
    assert_eq!(
        fees.len(),
        2,
        "{:?}",
        p.ranked.iter().map(|m| &m.id).collect::<Vec<_>>()
    );
    for m in fees {
        let has = m.evidence.contains(&"broker-trade-fee".to_string());
        assert_eq!(
            has,
            !m.bucket.eq_ignore_ascii_case("non-registered"),
            "{}",
            m.id
        );
        assert!(
            !m.task.as_deref().unwrap_or("").contains("same region"),
            "{:?}",
            m.task
        );
    }
}

#[test]
fn panel_three_fixes_hold() {
    // A capped bond swap names its amount.
    let i = input(json!({
        "buckets": {"Non-registered": 50000.0},
        "room": [{"account": "TFSA", "room": 10000.0}],
        "simplicity": 0.0,
        "positions": [{"symbol": "XBB", "bucket": "Non-registered", "value": 50000.0, "basis": "market", "acb": 50000.0, "asset_class": "bond"}]
    }));
    let p = plan(&i);
    let all: Vec<_> = p.ranked.iter().chain(&p.vetoed).collect();
    if let Some(m) = all.iter().find(|m| m.id.starts_with("location:XBB")) {
        assert!(
            m.task.as_deref().unwrap_or("").contains("$10,000 of XBB"),
            "{:?}",
            m.task
        );
    }
    // A taxable fee swap cites the returns its cost reads.
    let i = input(json!({
        "buckets": {"Non-registered": 40000.0},
        "positions": [{"symbol": "PRICEY", "bucket": "Non-registered", "value": 40000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}]
    }));
    let p = plan(&i);
    let fee = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id.starts_with("fee:PRICEY"))
        .unwrap();
    assert!(fee.evidence.contains(&"pag-equity".to_string()));
    // Missing cash is an unknown, not a quiet "no move".
    let i = input(json!({}));
    let p = plan(&i);
    assert!(
        p.unknowns
            .iter()
            .any(|u| u.contains("Cash above your reserve")),
        "{:?}",
        p.unknowns
    );
    // The answer line points to the steps it cut.
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "positions": [{"symbol": "LOSER", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 25000.0, "asset_class": "equity"}]
    }));
    let p = plan(&i);
    let text = render(&i, &p);
    let answer = text.lines().find(|l| l.contains("**Answer")).unwrap_or("");
    assert!(
        !answer.contains("**Answer: do move 1") || answer.contains("first do the steps"),
        "{answer}"
    );
}

#[test]
fn panel_four_fixes_hold() {
    // The concentration fall comes from a dated fact.
    let i = input(json!({
        "buckets": {"Non-registered": 60000.0, "TFSA": 40000.0},
        "employer": {"symbol": "EMPL", "value": 50000.0, "basis": "etrade-total", "acb": 45000.0}
    }));
    let p = plan(&i);
    assert!(
        p.risk[0]
            .evidence
            .contains(&"single-stock-drop".to_string())
    );
    // Money needed soon gets no "toward more stocks" task.
    let i = input(json!({
        "horizon_months": 12.0,
        "simplicity": 0.0,
        "buckets": {"TFSA": 100000.0},
        "positions": [{"symbol": "ZAG", "bucket": "TFSA", "value": 100000.0, "basis": "market", "acb": 100000.0, "asset_class": "bond"}]
    }));
    let p = plan(&i);
    let all: Vec<_> = p.ranked.iter().chain(&p.risk).chain(&p.vetoed).collect();
    assert!(all.iter().any(|m| m.id == "rebalance"), "{all:?}");
    assert!(
        all.iter()
            .filter(|m| m.id == "rebalance")
            .all(|m| m.task.as_deref().is_none_or(|t| t.contains("down to"))),
        "{all:?}"
    );
    // A loser that is the employer stock names the vest rule in the TFSA task.
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "employer": {"symbol": "EMPL", "value": 1000.0, "basis": "market", "acb": 1000.0},
        "positions": [{"symbol": "EMPL", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 25000.0, "asset_class": "equity"}]
    }));
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fill-room")
        .expect("fill-room");
    assert!(
        m.task.as_deref().unwrap_or("").contains("RSU vest"),
        "{:?}",
        m.task
    );
    // A blocked move is named in the answer, not "no move needed".
    let i = input(
        json!({"cash": 50000.0, "reserve": 10000.0, "horizon_months": 6.0, "simplicity": 0.0, "buckets": {"Non-registered": 20000.0},
        "positions": [{"symbol": "PRICEY", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 20000.0, "asset_class": "equity", "mer": 0.02}]}),
    );
    let p = plan(&i);
    let text = render(&i, &p);
    if !p.vetoed.is_empty() && p.do_nothing_wins {
        assert!(
            text.contains("no move until a safety rule clears"),
            "{text}"
        );
    }
}

#[test]
fn panel_five_fixes_hold() {
    // Unknown debt blocks investing cash.
    let mut i = input(json!({"cash": 25000.0, "reserve": 5000.0, "simplicity": 0.0}));
    i.high_interest_debt = None;
    let p = plan(&i);
    let m = p
        .vetoed
        .iter()
        .find(|m| m.id == "cash-drag")
        .expect("vetoed");
    assert!(m.vetoed.as_deref().unwrap().starts_with("debt unknown"));
    // A stale tax year blocks the moves that read it.
    let mut i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "positions": [{"symbol": "XEQT", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 15000.0, "asset_class": "equity"}]
    }));
    i.facts.insert(
        "tax-rules".into(),
        serde_json::from_value(json!({"value": null, "confidence": "verified", "review_by": "2026-01-31", "as_of": "2025-11-01", "source": "x", "text": "t"})).unwrap(),
    );
    let p = plan(&i);
    let m = p
        .vetoed
        .iter()
        .find(|m| m.id == "fill-room")
        .expect("fill-room vetoed");
    assert!(
        m.vetoed.as_deref().unwrap().contains("tax-rules"),
        "{:?}",
        m.vetoed
    );
}

#[test]
fn panel_six_fixes_hold() {
    // A stock-ward rebalance waits for debt and horizon, like investing cash.
    let bonds = json!({
        "simplicity": 0.0,
        "buckets": {"TFSA": 100000.0},
        "positions": [{"symbol": "ZAG", "bucket": "TFSA", "value": 100000.0, "basis": "market", "acb": 100000.0, "asset_class": "bond"}]
    });
    for (debt, horizon) in [
        (Some(5000.0), Some(360.0)),
        (None, Some(360.0)),
        (Some(0.0), None),
    ] {
        let mut i = input(bonds.clone());
        i.high_interest_debt = debt;
        i.horizon_months = horizon;
        let p = plan(&i);
        let all: Vec<_> = p.ranked.iter().chain(&p.risk).chain(&p.vetoed).collect();
        assert!(
            all.iter()
                .filter(|m| m.id == "rebalance")
                .all(|m| m.task.as_deref().is_none_or(|t| t.contains("down to"))),
            "{debt:?} {horizon:?}"
        );
    }
    // A harvest claims no sure saving: gains to offset are unknown.
    let i = input(json!({
        "simplicity": 0.0,
        "buckets": {"Non-registered": 20000.0},
        "room": [],
        "positions": [{"symbol": "LOSER", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity"}]
    }));
    let p = plan(&i);
    if let Some(m) = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "harvest:LOSER")
    {
        assert_eq!(m.gain_low, 0.0);
        assert!(m.why.contains("not other income"), "{}", m.why);
    }
    // The blocked answer line is two sentences.
    let i = input(
        json!({"cash": 1000.0, "reserve": 5000.0, "high_interest_debt": 3000.0, "simplicity": 0.0}),
    );
    let p = plan(&i);
    let text = render(&i, &p);
    assert!(
        !text.contains("a year Fix that first") && !text.contains(") Fix that first"),
        "{text}"
    );
}

#[test]
fn panel_seven_fixes_hold() {
    // A stock-ward rebalance waits while cash is below the reserve.
    let i = input(json!({
        "cash": 1000.0, "reserve": 5000.0, "simplicity": 0.0,
        "buckets": {"TFSA": 100000.0},
        "positions": [{"symbol": "ZAG", "bucket": "TFSA", "value": 100000.0, "basis": "market", "acb": 100000.0, "asset_class": "bond"}]
    }));
    let p = plan(&i);
    let all: Vec<_> = p.ranked.iter().chain(&p.risk).chain(&p.vetoed).collect();
    assert!(
        all.iter()
            .filter(|m| m.id == "rebalance")
            .all(|m| m.task.is_none())
    );
    // A superficial loss note names both outcomes.
    let i = input(json!({"superficial": ["XYZ"]}));
    let p = plan(&i);
    assert!(
        p.unknowns.iter().any(|u| u.contains("gone for good")),
        "{:?}",
        p.unknowns
    );
    // The debt rule shows the fact's rate.
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": 3000.0}));
    let p = plan(&i);
    let m = p.vetoed.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(
        m.vetoed
            .as_deref()
            .unwrap()
            .contains("high-interest-debt-rate")
    );
}

#[test]
fn a_tiny_blocked_move_does_not_take_the_headline() {
    let i = input(
        json!({"cash": 5100.0, "reserve": 5000.0, "high_interest_debt": 3000.0, "simplicity": 1000000.0}),
    );
    let p = plan(&i);
    assert!(!p.vetoed.is_empty());
    let text = render(&i, &p);
    assert!(text.contains("no move needed now"), "{text}");
}

#[test]
fn the_cash_move_cites_its_fee_facts() {
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(m.evidence.contains(&"etf-mers".to_string()));
    assert!(m.evidence.contains(&"broker-trade-fee".to_string()));
}

#[test]
fn panel_twelve_fixes_hold() {
    // Short horizon with too many stocks: no "toward target" task.
    let i = input(json!({
        "horizon_months": 18.0, "simplicity": 0.0,
        "buckets": {"TFSA": 100000.0},
        "positions": [{"symbol": "XEQT", "bucket": "TFSA", "value": 100000.0, "basis": "market", "acb": 100000.0, "asset_class": "equity"}]
    }));
    let p = plan(&i);
    let all: Vec<_> = p.ranked.iter().chain(&p.risk).chain(&p.vetoed).collect();
    assert!(
        all.iter()
            .filter(|m| m.id == "rebalance")
            .all(|m| m.task.as_deref().is_none_or(|t| t.contains("down to"))),
        "{all:?}"
    );
    // A mixed target never names the all-stock fund.
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0, "target_equity": 0.6}));
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(
        !m.task.as_deref().unwrap_or("").contains("XEQT"),
        "{:?}",
        m.task
    );
    // Old prices block a sell move.
    let mut i = input(json!({
        "buckets": {"Non-registered": 20000.0}, "room": [], "simplicity": 0.0,
        "positions": [{"symbol": "LOSER", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity"}]
    }));
    i.facts.insert("prices".into(), serde_json::from_value(json!({"value": null, "confidence": "owner", "review_by": "2000-01-01", "as_of": "unknown", "source": "your profile", "text": "t", "judgment": true})).unwrap());
    let p = plan(&i);
    assert!(
        p.ranked.iter().all(|m| !m.id.starts_with("harvest:")),
        "{:?}",
        p.ranked
    );
    // A judgment call shows no source link.
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0}));
    let mut i2 = i.clone();
    i2.facts.get_mut("return-band").unwrap().judgment = true;
    let text = render(&i2, &plan(&i2));
    assert!(
        text.contains("No external source: a judgment call"),
        "{text}"
    );
}

#[test]
fn panel_thirteen_fixes_hold() {
    // Saving money needed soon is not blocked as "investing cash".
    let mut i = input(
        json!({"cash": 25000.0, "reserve": 5000.0, "horizon_months": 12.0, "simplicity": 0.0, "cash_rate": 0.0}),
    );
    i.high_interest_debt = None;
    let p = plan(&i);
    assert!(
        p.vetoed.iter().all(|m| !m
            .vetoed
            .as_deref()
            .unwrap_or("")
            .contains("before investing cash")),
        "{:?}",
        p.vetoed
    );
    // A zero-gain move keeps no task at a zero threshold.
    let i = input(
        json!({"cash": 25000.0, "reserve": 5000.0, "horizon_months": 12.0, "simplicity": 0.0, "cash_rate": 0.5}),
    );
    let p = plan(&i);
    assert!(
        p.ranked
            .iter()
            .filter(|m| m.net() <= 0.0 && m.kind == "gain")
            .all(|m| m.task.is_none())
    );
    // An all-stock target says so.
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0, "target_equity": 1.0}));
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(
        !m.task.as_deref().unwrap().contains("bonds"),
        "{:?}",
        m.task
    );
}

#[test]
fn panel_fourteen_fixes_hold() {
    // A negative debt is unknown, not "no debt".
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": -5000.0}));
    let p = plan(&i);
    assert!(
        p.vetoed.iter().any(|m| m.id == "cash-drag"),
        "{:?}",
        p.ranked
    );
    assert!(p.unknowns.iter().any(|u| u.contains("negative")));
    // Stale prices block fill-room; the taxable harvest is not dropped for it.
    let mut i = input(json!({
        "buckets": {"Non-registered": 20000.0},
        "room": [{"account": "TFSA", "room": 50000.0}],
        "simplicity": 0.0,
        "positions": [{"symbol": "LOSER", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "mer": 0.02}]
    }));
    i.facts.insert("prices".into(), serde_json::from_value(json!({"value": null, "confidence": "owner", "review_by": "2000-01-01", "as_of": "unknown", "source": "your profile", "text": "t", "judgment": true})).unwrap());
    let p = plan(&i);
    assert!(p.vetoed.iter().any(|m| m.id == "fill-room"));
    let all: Vec<_> = p.ranked.iter().chain(&p.vetoed).collect();
    assert!(all.iter().any(|m| m.id == "harvest:LOSER"), "{all:?}");
    assert!(
        all.iter().filter(|m| m.id.starts_with("fee:")).all(|m| !m
            .task
            .as_deref()
            .unwrap_or("")
            .contains("fill-room"))
    );
    // A taxable swap with an unknown gain says to check it.
    let i = input(json!({
        "buckets": {"Non-registered": 40000.0}, "simplicity": 0.0, "room": [],
        "positions": [{"symbol": "PRICEY", "bucket": "Non-registered", "value": 40000.0, "basis": "cost", "acb": 40000.0, "asset_class": "equity", "mer": 0.02}]
    }));
    let p = plan(&i);
    if let Some(m) = p.ranked.iter().find(|m| m.id.starts_with("fee:PRICEY")) {
        assert!(
            m.task
                .as_deref()
                .unwrap()
                .contains("check the capital gain"),
            "{:?}",
            m.task
        );
    }
}

#[test]
fn panel_fifteen_fixes_hold() {
    // Extra cash above the first account's room is split, never over-contributed.
    let i = input(
        json!({"cash": 25000.0, "reserve": 5000.0, "room": [{"account": "RRSP", "room": 3000.0}, {"account": "TFSA", "room": 50000.0}]}),
    );
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "cash-drag")
        .unwrap();
    let t = m.task.as_deref().unwrap_or("");
    assert!(t.contains("into RRSP") && t.contains("into TFSA"), "{t}");
    // A taxable fee swap at a loss warns about the superficial-loss rule.
    let i = input(json!({
        "buckets": {"Non-registered": 20000.0}, "simplicity": 0.0, "room": [],
        "positions": [{"symbol": "LOSER", "bucket": "Non-registered", "value": 20000.0, "basis": "market", "acb": 30000.0, "asset_class": "equity", "mer": 0.02}]
    }));
    let p = plan(&i);
    if let Some(m) = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "fee:LOSER")
    {
        assert!(m.evidence.iter().any(|e| e == "superficial-loss"));
        assert!(m.task.as_deref().unwrap_or("").contains("30 days") || m.vetoed.is_some());
    }
    // Above target with a short horizon still gets a de-risking task.
    let mut i = input(json!({
        "buckets": {"TFSA": 100000.0}, "target_equity": 0.6,
        "positions": [
            {"symbol": "EQ", "bucket": "TFSA", "value": 90000.0, "basis": "market", "asset_class": "equity"},
            {"symbol": "BD", "bucket": "TFSA", "value": 10000.0, "basis": "market", "asset_class": "bond"}
        ]
    }));
    i.horizon_months = Some(24.0);
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.risk)
        .chain(&p.vetoed)
        .find(|m| m.id == "rebalance")
        .unwrap();
    assert!(
        m.task.as_deref().unwrap_or("").contains("down to"),
        "{:?}",
        m.task
    );
    // A blocked gain move never says it is too small.
    let i = input(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": null}));
    let p = plan(&i);
    assert!(!p.vetoed.is_empty());
    assert!(
        !render(&i, &p).contains("Below your simplicity threshold"),
        "{}",
        render(&i, &p)
    );
}

#[test]
fn panel_sixteen_fixes_hold() {
    // Two small rooms: the leftover goes to Non-registered, never over a room.
    let i = input(
        json!({"cash": 25000.0, "reserve": 5000.0, "room": [{"account": "TFSA", "room": 3000.0}, {"account": "FHSA", "room": 2000.0}]}),
    );
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .chain(&p.vetoed)
        .find(|m| m.id == "cash-drag")
        .unwrap();
    let t = m.task.as_deref().unwrap_or("");
    assert!(
        t.contains("$2,000 into FHSA") && t.contains("$15,000 in Non-registered"),
        "{t}"
    );
    // Crypto over its cap with unknown debt never says to buy funds.
    let crypto = json!([{"symbol": "BTC", "bucket": "Non-registered", "value": 30000.0, "basis": "market", "asset_class": "crypto"},
        {"symbol": "XEQT", "bucket": "TFSA", "value": 70000.0, "basis": "market", "asset_class": "equity"}]);
    let i = input(json!({"crypto_max": 0.05, "positions": crypto, "high_interest_debt": null}));
    let p = plan(&i);
    let all: Vec<_> = p.ranked.iter().chain(&p.risk).chain(&p.vetoed).collect();
    let m = all.iter().find(|m| m.id == "crypto").unwrap();
    assert!(
        !m.task.as_deref().unwrap().contains("other funds"),
        "{:?}",
        m.task
    );
    // Short horizon: the excess goes to safe holdings.
    let mut i = input(json!({"crypto_max": 0.05, "positions": crypto}));
    i.horizon_months = Some(24.0);
    let p = plan(&i);
    let all: Vec<_> = p.ranked.iter().chain(&p.risk).chain(&p.vetoed).collect();
    let m = all.iter().find(|m| m.id == "crypto").unwrap();
    assert!(
        m.task.as_deref().unwrap().contains("safe holdings"),
        "{:?}",
        m.task
    );
}

fn rrsp_with_vfv(value: f64) -> Input {
    input(json!({
        "buckets": {"RRSP": value},
        "room": [{"account": "RRSP", "room": 0.0}],
        "cheapest": {"equity": {"ticker": "XEQT", "mer": 0.0019}, "equity:US:US-listed": {"ticker": "VTI", "mer": 0.0003}},
        "positions": [{"symbol": "VFV", "bucket": "RRSP", "value": value, "basis": "market", "acb": value, "asset_class": "equity", "region": "US", "domicile": "CA", "mer": 0.0009}]
    }))
}

#[test]
fn rrsp_us_listed_a_large_rrsp_gets_a_ranged_move_with_cost() {
    let i = rrsp_with_vfv(200000.0);
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .find(|m| m.id == "location:VFV")
        .expect("no RRSP US-listed move");
    assert!((m.gain_low - 200000.0 * 0.010 * 0.15).abs() < 0.01);
    assert!((m.gain_high - 200000.0 * 0.015 * 0.15).abs() < 0.01);
    assert!(m.gain_low < m.gain_high);
    assert!((m.cost_once - 200000.0 * 0.001).abs() < 0.01, "{}", m.cost_once);
    assert!(m.evidence.contains(&"norbert".to_string()));
    let task = m.task.as_deref().expect("a large RRSP gets a task");
    assert!(task.contains("VTI") && task.contains("Norbert"), "{task}");
}

#[test]
fn rrsp_us_listed_a_small_rrsp_gets_no_task() {
    let i = rrsp_with_vfv(20000.0);
    let p = plan(&i);
    let m = p
        .ranked
        .iter()
        .find(|m| m.id == "location:VFV")
        .expect("the move is listed, below the threshold");
    assert!(m.net() < i.simplicity);
    assert!(m.task.is_none(), "{:?}", m.task);
}

fn rrsp_case(bucket: &str, region: &str, class: &str) -> Input {
    input(json!({
        "buckets": {bucket: 200000.0},
        "room": [{"account": "RRSP", "room": 0.0}],
        "cheapest": {
            "equity": {"ticker": "XEQT", "mer": 0.0019},
            "equity:US:US-listed": {"ticker": "VTI", "mer": 0.0003},
            "equity:international:US-listed": {"ticker": "VXUS", "mer": 0.0005},
            "equity:CA:US-listed": {"ticker": "EWC", "mer": 0.005},
            "bond:US:US-listed": {"ticker": "BND", "mer": 0.0003}
        },
        "positions": [{"symbol": "FUND", "bucket": bucket, "value": 200000.0, "basis": "market", "acb": 200000.0, "asset_class": class, "region": region, "domicile": "CA", "mer": 0.0009}]
    }))
}

fn has_rrsp_us_move(i: &Input) -> bool {
    let p = plan(i);
    p.ranked
        .iter()
        .chain(&p.risk)
        .chain(&p.vetoed)
        .any(|m| m.id == "location:FUND")
}

#[test]
fn rrsp_us_listed_the_baseline_case_gets_a_move() {
    assert!(has_rrsp_us_move(&rrsp_case("RRSP", "US", "equity")));
}

#[test]
fn rrsp_us_listed_international_gets_no_move() {
    assert!(!has_rrsp_us_move(&rrsp_case("RRSP", "international", "equity")));
}

#[test]
fn rrsp_us_listed_canadian_region_gets_no_move() {
    assert!(!has_rrsp_us_move(&rrsp_case("RRSP", "CA", "equity")));
}

#[test]
fn rrsp_us_listed_a_bond_gets_no_move() {
    assert!(!has_rrsp_us_move(&rrsp_case("RRSP", "US", "bond")));
}

#[test]
fn rrsp_us_listed_a_tfsa_gets_no_move() {
    assert!(!has_rrsp_us_move(&rrsp_case("TFSA", "US", "equity")));
}

#[test]
fn rrsp_us_listed_non_registered_gets_no_move() {
    assert!(!has_rrsp_us_move(&rrsp_case("Non-registered", "US", "equity")));
}

#[test]
fn rrsp_us_listed_names_the_currency_exposure_of_a_hedged_fund() {
    let i = rrsp_with_vfv(200000.0);
    let p = plan(&i);
    let m = p.ranked.iter().find(|m| m.id == "location:VFV").unwrap();
    assert!(
        m.unknowns
            .iter()
            .any(|u| u.contains("CAD-hedged") && u.contains("currency")),
        "{:?}",
        m.unknowns
    );
}

#[test]
fn rrsp_us_listed_drops_the_fee_move_for_the_same_symbol() {
    let mut i = rrsp_with_vfv(200000.0);
    i.positions[0].mer = Some(0.005);
    i.cheapest.insert(
        "equity:US".into(),
        serde_json::from_value(json!({"ticker": "XUU", "mer": 0.0007})).unwrap(),
    );
    let p = plan(&i);
    let all: Vec<_> = p
        .ranked
        .iter()
        .chain(&p.risk)
        .chain(&p.vetoed)
        .chain(&p.overridden)
        .collect();
    assert!(all.iter().any(|m| m.id == "location:VFV"));
    assert!(
        !all.iter().any(|m| m.id == "fee:VFV"),
        "two sell tasks for VFV"
    );
}

#[test]
fn rrsp_us_listed_a_rrif_gets_a_move() {
    assert!(has_rrsp_us_move(&rrsp_case("RRIF", "US", "equity")));
}

#[test]
fn rrsp_us_listed_a_lira_gets_a_move() {
    assert!(has_rrsp_us_move(&rrsp_case("LIRA", "US", "equity")));
}

#[test]
fn rrsp_us_listed_a_lif_gets_a_move() {
    assert!(has_rrsp_us_move(&rrsp_case("LIF", "US", "equity")));
}
