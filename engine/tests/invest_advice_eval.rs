//! Advice-quality judge (task invest-advice-eval): synthetic Canadian investor
//! profiles scored on a fixed rubric, so advice quality does not rest on the
//! user spotting bad advice (Takayanagi et al. 2025, arXiv 2504.05862; profiles
//! after Fieberg et al. 2025, CESifo WP 11666). Deterministic, no model.

use cashcast_engine::invest_plan::{Input, Plan, plan, render};
use serde_json::{Value, json};

fn fact(value: f64, conf: &str) -> Value {
    json!({"value": value, "text": "synthetic fact", "source": "https://example.com", "as_of": "2026-01-01", "review_by": "2027-04-30", "confidence": conf})
}

fn profile(extra: Value) -> Input {
    let mut base = json!({
        "today": "2026-11-01", "marginal": 0.30, "inclusion": 0.5,
        "target_equity": 0.8, "target_source": "your risk level (growth)",
        "simplicity": 50.0, "cash_rate": 0.015, "high_interest_debt": 0.0,
        "room": [{"account": "TFSA", "room": 7000.0}, {"account": "RRSP", "room": 10000.0}],
        "cheapest": {"equity": {"ticker": "XEQT", "mer": 0.0019}, "bond": {"ticker": "ZAG", "mer": 0.0009}},
        "facts": {
            "pag-equity": fact(0.066, "verified"), "pag-fixed-income": fact(0.032, "verified"),
            "pag-short-term": fact(0.024, "verified"), "pag-inflation": fact(0.021, "verified"),
            "return-band": fact(0.02, "heuristic"), "one-year-drop": fact(-0.42, "estimate"), "crypto-drawdown": fact(-0.7, "estimate"), "fhsa": fact(8000.0, "verified"),
            "concentration-limit": fact(0.10, "heuristic"), "rebalance": fact(0.05, "heuristic"),
            "short-horizon": fact(60.0, "heuristic"), "fees-predict": fact(0.0, "verified"), "etf-mers": fact(0.0019, "verified"), "broker-trade-fee": fact(0.0, "verified"), "single-stock-drop": fact(0.5, "heuristic"), "cash-rate-capture": fact(0.5, "heuristic"), "gain-deferral-share": fact(0.5, "heuristic"), "mer-lag": fact(0.8, "heuristic"), "high-interest-debt-rate": fact(0.08, "heuristic"),
            "single-stock-risk": fact(0.0, "verified"), "tfsa-shelter": fact(0.0, "verified"),
            "spiva-canada": fact(0.988, "verified")
        }
    });
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).unwrap()
}

/// The 12 profiles: (name, extra input, expectation).
fn profiles() -> Vec<(&'static str, Value)> {
    vec![
        (
            "student, no savings",
            json!({"cash": 800.0, "reserve": 3000.0}),
        ),
        (
            "new grad, cash above reserve",
            json!({"cash": 20000.0, "reserve": 6000.0}),
        ),
        (
            "card debt and cash",
            json!({"cash": 20000.0, "reserve": 6000.0, "high_interest_debt": 4000.0}),
        ),
        (
            "home in 18 months",
            json!({"cash": 40000.0, "reserve": 8000.0, "horizon_months": 18.0}),
        ),
        (
            "home in 10 years",
            json!({"cash": 40000.0, "reserve": 8000.0, "horizon_months": 120.0}),
        ),
        (
            "conservative retiree",
            json!({"cash": 30000.0, "reserve": 20000.0, "target_equity": 0.4, "target_source": "your risk level (conservative)"}),
        ),
        (
            "employer stock heavy",
            json!({"cash": 5000.0, "reserve": 5000.0, "employer": {"symbol": "EMPL", "value": 60000.0, "basis": "market", "acb": 20000.0}, "buckets": {"Non-registered": 70000.0}}),
        ),
        (
            "all-equity, cash at reserve",
            json!({"cash": 10000.0, "reserve": 10000.0, "target_equity": 1.0}),
        ),
        (
            "no tax rate known",
            json!({"cash": 15000.0, "reserve": 5000.0, "marginal": null}),
        ),
        (
            "no room left",
            json!({"cash": 15000.0, "reserve": 5000.0, "room": []}),
        ),
        (
            "high simplicity bar",
            json!({"cash": 7000.0, "reserve": 5000.0, "simplicity": 500.0}),
        ),
        (
            "debt, no spare cash",
            json!({"cash": 2000.0, "reserve": 5000.0, "high_interest_debt": 9000.0}),
        ),
    ]
}

/// Rubric failures for one plan; empty means it passes.
fn judge(name: &str, i: &Input, p: &Plan) -> Vec<String> {
    let text = render(i, p).to_lowercase();
    let mut bad = vec![];
    let mut fail = |ok: bool, what: &str| {
        if !ok {
            bad.push(format!("{name}: {what}"));
        }
    };
    fail(
        p.ranked.iter().any(|m| m.kind == "baseline"),
        "do nothing is ranked",
    );
    fail(
        text.contains("not a licensed financial advisor"),
        "disclosure present",
    );
    fail(
        text.lines()
            .filter(|l| l.contains("Answer: do move"))
            .all(|l| l.contains("Not personal advice")),
        "a headline that names a move carries the caveat",
    );
    for w in [
        "guaranteed",
        "will return",
        "time the market",
        "rrsp is always",
        "buy now before",
    ] {
        fail(
            !text.contains(w),
            &format!("no promise or misconception ({w})"),
        );
    }
    for m in p.ranked.iter().chain(&p.risk).chain(&p.vetoed) {
        fail(
            m.gain_low <= m.gain_high,
            &format!("{} gain is an ordered range", m.id),
        );
        fail(
            !m.objection.is_empty(),
            &format!("{} has an objection", m.id),
        );
        fail(
            m.kind == "baseline" || !m.evidence.is_empty(),
            &format!("{} cites evidence", m.id),
        );
    }
    let debt = i.high_interest_debt.unwrap_or(0.0) > 0.0;
    let invests = |m: &&cashcast_engine::invest_plan::Move| {
        m.id == "fill-room" || (m.id == "cash-drag" && m.title.starts_with("Invest"))
    };
    fail(
        !(debt && p.ranked.iter().any(|m| invests(&m))),
        "debt blocks new investing",
    );
    if i.horizon_months.is_some_and(|h| h < 60.0) {
        fail(
            !p.ranked.iter().any(|m| invests(&m)),
            "near money never goes to stocks",
        );
    }
    bad
}

#[test]
fn advice_passes_the_rubric_on_every_synthetic_profile() {
    let all = profiles();
    let mut failures = vec![];
    for (name, extra) in &all {
        let i = profile(extra.clone());
        failures.extend(judge(name, &i, &plan(&i)));
    }
    let failed: std::collections::BTreeSet<_> = failures
        .iter()
        .map(|f| f.split(':').next().unwrap().to_string())
        .collect();
    let rate = 1.0 - failed.len() as f64 / all.len() as f64;
    println!(
        "advice rubric: {:.0}% of {} profiles pass",
        rate * 100.0,
        all.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Negative controls: the judge must catch each kind of bad advice, or its
/// 100% pass rate proves nothing.
#[test]
fn the_judge_fails_bad_advice() {
    let i = profile(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": 3000.0}));
    let good = plan(&i);
    assert!(judge("good", &i, &good).is_empty());
    let blocked = good.vetoed.iter().find(|m| m.id == "cash-drag").cloned();
    let blocked = blocked.expect("debt profile blocks investing");

    let mut p = good.clone();
    p.ranked.insert(0, blocked.clone());
    assert!(
        judge("bad", &i, &p)
            .iter()
            .any(|f| f.contains("debt blocks"))
    );

    let mut p = good.clone();
    p.ranked[0].why = "This is guaranteed.".into();
    assert!(
        judge("bad", &i, &p)
            .iter()
            .any(|f| f.contains("guaranteed"))
    );

    let mut p = good.clone();
    p.ranked[0].gain_low = p.ranked[0].gain_high + 1.0;
    assert!(
        judge("bad", &i, &p)
            .iter()
            .any(|f| f.contains("ordered range"))
    );

    let mut p = good.clone();
    p.ranked.retain(|m| m.kind != "baseline");
    assert!(
        judge("bad", &i, &p)
            .iter()
            .any(|f| f.contains("do nothing"))
    );

    let mut p = good;
    p.vetoed[0].objection.clear();
    assert!(judge("bad", &i, &p).iter().any(|f| f.contains("objection")));
}

/// Expected outcomes for the profiles where the right answer is known.
#[test]
fn known_profiles_get_the_known_answer() {
    let debt = profile(json!({"cash": 25000.0, "reserve": 5000.0, "high_interest_debt": 3000.0}));
    let p = plan(&debt);
    let m = p.vetoed.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(m.vetoed.as_deref().unwrap().starts_with("debt first"));

    let mut soon = profile(json!({"cash": 45000.0, "reserve": 5000.0}));
    soon.horizon_months = Some(12.0);
    let p = plan(&soon);
    let m = p.ranked.iter().find(|m| m.id == "cash-drag").unwrap();
    assert!(!m.title.starts_with("Invest"), "{}", m.title);

    let idle = profile(json!({}));
    assert!(plan(&idle).do_nothing_wins);
}
