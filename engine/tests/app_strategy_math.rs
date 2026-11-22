//! Strategy arithmetic on synthetic inputs: tax (US-07), cash back routing
//! (US-07), the RRSP limit (US-07), the stock optimizer (US-18), currency
//! conversion (US-25) and the bank fee verdict (US-28). Ported from
//! tests/unit/test_strategy_tax.py, test_strategy_cashback.py, test_rrsp.py,
//! test_optimize.py and parts of test_strategy_foreign.py / test_interest.py.

mod app;

use app::{approx, d, repo};
use cashcast_engine::core::pyjson::{Json, loads};
use cashcast_engine::strategy::cashback::{self, Card, optimize as route, rent_charges, to_card};
use cashcast_engine::strategy::optimize::{self as opt, Plan as OptPlan};
use cashcast_engine::strategy::{banking, foreign, py, rrsp, tax};

fn j(s: &str) -> Json {
    loads(s).unwrap_or_else(|| panic!("bad json: {s}"))
}

fn knowledge(name: &str) -> Json {
    let path = repo().join(format!("knowledge/data/{name}.toml"));
    py::table(&toml::from_str(&app::read(&path)).unwrap())
}

// ---------- tax ----------

const TAX: &str = r#"{
 "federal": {"brackets": [{"up_to": 50000, "rate": 0.15}, {"rate": 0.30}], "basic_personal_amount": 10000},
 "provinces": {
  "ON": {"brackets": [{"up_to": 50000, "rate": 0.05}, {"rate": 0.10}], "basic_personal_amount": 10000,
         "surtax": [{"over": 4000, "rate": 0.20}]},
  "QC": {"brackets": [{"rate": 0.15}], "basic_personal_amount": 0, "federal_abatement": 0.165}},
 "payroll": {"cpp_ympe": 70000, "cpp_basic_exemption": 3500, "cpp_rate": 0.0595, "cpp2_yampe": 80000,
             "cpp2_rate": 0.04, "ei_max_insurable": 65000, "ei_rate": 0.0164, "ei_rate_qc": 0.0131}}"#;

#[test]
fn tax_progressive() {
    let data = j(TAX);
    let brackets = py::list(py::obj(&data, "federal"), "brackets");
    assert!(approx(
        tax::progressive(60000.0, brackets),
        50000.0 * 0.15 + 10000.0 * 0.30
    ));
}

#[test]
fn tax_income_tax_with_credit_and_surtax() {
    let data = j(TAX);
    let t = tax::income_tax(100000.0, "ON", &data);
    let fed = 50000.0 * 0.15 + 50000.0 * 0.30 - 10000.0 * 0.15;
    let prov = 50000.0 * 0.05 + 50000.0 * 0.10 - 10000.0 * 0.05;
    assert!(approx(t.federal, fed));
    assert!(approx(t.provincial, prov + 0.20 * (prov - 4000.0)));
    assert!(approx(
        tax::marginal(100000.0, "ON", &data),
        0.30 + 0.10 * 1.2
    ));
}

#[test]
fn tax_quebec_abatement_and_rrsp_refund() {
    let data = j(TAX);
    assert!(approx(
        tax::income_tax(40000.0, "QC", &data).federal,
        (40000.0 * 0.15 - 1500.0) * (1.0 - 0.165)
    ));
    assert!(approx(
        tax::rrsp_refund(60000.0, 10000.0, "ON", &data),
        tax::income_tax(60000.0, "ON", &data).total - tax::income_tax(50000.0, "ON", &data).total
    ));
}

#[test]
fn tax_payroll() {
    let (cpp, cpp2, ei) = tax::payroll(90000.0, "ON", &j(TAX)).unwrap();
    assert!(approx(cpp, (70000.0 - 3500.0) * 0.0595));
    assert!(approx(cpp2, 10000.0 * 0.04));
    assert!(approx(ei, 65000.0 * 0.0164));
}

#[test]
fn tax_real_data_covers_every_province() {
    let data = knowledge("tax_ca_2026");
    for (code, _) in cashcast_engine::core::users::PROVINCES {
        assert!(
            py::obj(py::obj(&data, "provinces"), code).truthy(),
            "{code}"
        );
        let t = tax::income_tax(80000.0, code, &data);
        assert!(0.12 < t.average && t.average < 0.35, "{code}");
        let m = tax::marginal(80000.0, code, &data);
        assert!(0.2 < m && m < 0.5, "{code}");
    }
}

// ---------- cash back ----------

fn card(json: &str) -> Card {
    to_card(&j(json), None).unwrap()
}

fn flat2() -> Card {
    card(r#"{"id": "flat2", "name": "Flat 2%", "earn": {"other": 0.02}, "fx_fee": 0.0}"#)
}

fn grocery4() -> Card {
    card(
        r#"{"id": "grocery4", "name": "Grocery 4%", "earn": {"groceries": 0.04, "recurring": 0.04, "other": 0.01},
        "caps": [{"categories": ["groceries", "recurring"], "limit": 25000, "period": "card_year", "after_rate": 0.01}],
        "annual_fee": 120, "fee_rebated": true, "fx_fee": 0.025}"#,
    )
}

fn bills3() -> Card {
    card(
        r#"{"id": "bills3", "name": "Bills 3%", "earn": {"groceries": 0.03, "recurring": 0.03, "other": 0.01},
        "caps": [{"categories": ["groceries", "recurring"], "limit": 15000, "per_category": true, "after_rate": 0.01}],
        "annual_fee": 139, "fx_fee": 0.025}"#,
    )
}

fn spend(items: &[(&str, f64)]) -> Vec<(String, f64)> {
    items.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn bank(plan: &cashback::Plan, cat: &str) -> Option<f64> {
    plan.bank_paid
        .iter()
        .find(|(k, _)| k == cat)
        .map(|(_, v)| *v)
}

#[test]
fn cashback_best_rate_wins_until_the_cap() {
    let plan = route(
        &spend(&[("groceries", 30000.0), ("other", 10000.0)]),
        &[flat2(), grocery4()],
        None,
    );
    let g: Vec<(String, f64)> = plan
        .allocations
        .iter()
        .filter(|a| a.category == "groceries")
        .map(|a| (a.card.clone(), a.amount))
        .collect();
    assert_eq!(
        g,
        vec![("grocery4".into(), 25000.0), ("flat2".into(), 5000.0)]
    );
    assert!(approx(
        plan.gross,
        25000.0 * 0.04 + 5000.0 * 0.02 + 10000.0 * 0.02
    ));
    assert_eq!(plan.card_fees, 0.0);
}

#[test]
fn cashback_rent_through_a_service_uses_caps_and_counts_charges() {
    let service = j(
        r#"{"id": "rent-service", "fee_rate": 0.015, "membership_fee": 150, "codes_as": "recurring"}"#,
    );
    let rent = 3600.0 * 12.0;
    let plan = route(
        &spend(&[("rent", rent)]),
        &[flat2(), grocery4(), bills3()],
        Some(&service),
    );
    let order: Vec<(String, f64)> = plan
        .allocations
        .iter()
        .map(|a| (a.card.clone(), a.amount.round()))
        .collect();
    assert_eq!(order[0], ("grocery4".into(), 25000.0));
    assert_eq!(order[1], ("bills3".into(), 15000.0));
    assert!(approx(plan.service_fees, rent * 0.015));
    assert_eq!(plan.membership_fees, 150.0);
    assert_eq!(
        rent_charges(&plan, 3600.0 * 1.015)[0],
        ("grocery4".into(), 7)
    );
}

#[test]
fn cashback_rent_goes_to_the_bank_when_no_card_beats_the_fee() {
    let one = card(r#"{"id": "one", "name": "One", "earn": {"other": 0.01}}"#);
    let service = j(r#"{"id": "s", "fee_rate": 0.0175, "codes_as": "other"}"#);
    let plan = route(&spend(&[("rent", 12000.0)]), &[one], Some(&service));
    assert_eq!(plan.bank_paid, spend(&[("rent", 12000.0)]));
    assert_eq!(plan.service_fees, 0.0);
}

#[test]
fn cashback_foreign_spending_avoids_fx_fees() {
    let plan = route(&spend(&[("foreign", 5000.0)]), &[grocery4(), flat2()], None);
    let cards: Vec<&str> = plan.allocations.iter().map(|a| a.card.as_str()).collect();
    assert_eq!(cards, vec!["flat2"]);
}

#[test]
fn cashback_bank_only_spending_never_goes_on_a_card() {
    let plan = route(
        &spend(&[("bank_only", 9000.0), ("other", 100.0)]),
        &[flat2()],
        None,
    );
    assert_eq!(bank(&plan, "bank_only"), Some(9000.0));
    assert_eq!(plan.bank_paid.len(), 1);
    assert_eq!(
        plan.allocations.iter().map(|a| a.amount).sum::<f64>(),
        100.0
    );
}

#[test]
fn cashback_pick_your_categories_cards_get_only_their_best_picks() {
    let entry = j(
        r#"{"id": "pick2", "name": "Pick 2", "earn": {"groceries": 0.02, "gas": 0.02, "dining": 0.02, "other": 0.005},
            "pick_categories": 2}"#,
    );
    let c = to_card(
        &entry,
        Some(&spend(&[
            ("groceries", 6000.0),
            ("dining", 3000.0),
            ("gas", 100.0),
        ])),
    )
    .unwrap();
    let mut earn = c.earn.clone();
    earn.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        earn,
        spend(&[("dining", 0.02), ("groceries", 0.02), ("other", 0.005)])
    );
}

/// The catalog cards `ids`; `old_scotia` drops Scotia's rent rate, as it
/// was before Oct 22, 2026 (rent then coded as recurring at 4%).
fn catalog_cards(ids: &[&str], old_scotia: bool) -> Vec<Card> {
    let catalog = knowledge("credit_cards");
    ids.iter()
        .map(|id| {
            let mut entry = py::list(&catalog, "cards")
                .iter()
                .find(|c| py::gets(c, "id", "") == *id)
                .unwrap()
                .clone();
            if old_scotia && *id == "scotia-momentum-visa-infinite" {
                if let Some(Json::Obj(earn)) = entry.get("earn").cloned() {
                    let kept = earn.into_iter().filter(|(k, _)| k != "rent").collect();
                    entry.set("earn", Json::Obj(kept));
                }
            }
            to_card(&entry, None).unwrap()
        })
        .collect()
}

#[test]
fn cashback_scotia_takes_groceries_not_rent_since_its_rent_rate_dropped() {
    let ids = [
        "scotia-momentum-visa-infinite",
        "td-cash-back-visa-infinite",
        "wealthsimple-visa-infinite-plus",
    ];
    let cards = catalog_cards(&ids, false);
    let service = j(r#"{"id": "rent-service", "fee_rate": 0.015, "codes_as": "recurring"}"#);
    let s = spend(&[("groceries", 8400.0), ("rent", 3500.0 * 12.0)]);
    let plan = route(&s, &cards, Some(&service));
    let on = |cat: &str| -> Vec<String> {
        plan.allocations
            .iter()
            .filter(|a| a.category == cat)
            .map(|a| a.card.clone())
            .collect()
    };
    assert_eq!(on("groceries"), vec!["scotia-momentum-visa-infinite"]);
    assert!(
        !on("rent")
            .iter()
            .any(|c| c == "scotia-momentum-visa-infinite")
    );
}

#[test]
fn cashback_a_shared_cap_goes_to_rent_when_rent_loses_more_without_it() {
    let ids = [
        "scotia-momentum-visa-infinite",
        "td-cash-back-visa-infinite",
        "wealthsimple-visa-infinite-plus",
    ];
    let cards = catalog_cards(&ids, true);
    let service = j(r#"{"id": "rent-service", "fee_rate": 0.0175, "codes_as": "recurring"}"#);
    let s = spend(&[
        ("groceries", 8400.0),
        ("recurring", 1200.0),
        ("streaming", 240.0),
        ("rent", 3500.0 * 12.0),
    ]);
    let plan = route(&s, &cards, Some(&service));
    for cat in ["groceries", "recurring", "streaming"] {
        let on: Vec<&str> = plan
            .allocations
            .iter()
            .filter(|a| a.category == cat)
            .map(|a| a.card.as_str())
            .collect();
        assert_eq!(on, vec!["td-cash-back-visa-infinite"], "{cat}");
    }
    assert_eq!(
        rent_charges(&plan, 3500.0 * 1.0175),
        vec![
            ("scotia-momentum-visa-infinite".into(), 7),
            ("td-cash-back-visa-infinite".into(), 4),
            ("wealthsimple-visa-infinite-plus".into(), 1)
        ]
    );
    let greedy = 9840.0 * 0.04 + 15160.0 * 0.0225 + 15000.0 * 0.0125 + 11840.0 * 0.0025;
    let best =
        8400.0 * 0.03 + 1440.0 * 0.03 + 25000.0 * 0.0225 + 13560.0 * 0.0125 + 3440.0 * 0.0025;
    let reward: f64 = plan.allocations.iter().map(|a| a.amount * a.rate).sum();
    assert!(approx(reward, best), "{reward} vs {best}");
    assert!(best > greedy);
}

// ---------- RRSP ----------

const TODAY_RRSP: &str = "2026-10-25";
const MONTHS: [&str; 7] = [
    "2026-03", "2026-04", "2026-05", "2026-06", "2026-07", "2026-08", "2026-09",
];

fn contrib(month: &str, cad: f64, source: &str, year: i32, bucket: &str) -> String {
    format!(
        r#"{{"account_bucket": "{bucket}", "deduction_year": {year}, "month": "{month}", "source": "{source}", "cad": {cad:?}, "rows": 1}}"#
    )
}

fn payroll(months: &[&str], member: f64, employer: f64, year: i32) -> Vec<String> {
    let mut v: Vec<String> = months
        .iter()
        .map(|m| contrib(m, member, "member", year, "RRSP"))
        .collect();
    v.extend(
        months
            .iter()
            .map(|m| contrib(m, employer, "employer", year, "RRSP")),
    );
    v
}

fn holdings(rows: &[String]) -> Json {
    j(&format!(r#"{{"contributions": [{}]}}"#, rows.join(",")))
}

fn profile(limit: f64, year: i32, extra: &str) -> Json {
    j(&format!(
        r#"{{"room": {{"rrsp": {limit:?}, "rrsp_limit_year": {year}{extra}}}}}"#
    ))
}

fn limits() -> Json {
    py::obj(&knowledge("tax_ca_2026"), "limits").clone()
}

fn check(p: &Json, h: Option<&Json>, today: &str) -> rrsp::Check {
    rrsp::check(p, h, d(today), &limits())
}

fn data(c: &rrsp::Check, key: &str) -> Json {
    c.data
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or(Json::Null)
}

fn num(c: &rrsp::Check, key: &str) -> f64 {
    py::num(Some(&data(c, key))).unwrap_or(f64::NAN)
}

#[test]
fn rrsp_the_first_60_days_count_for_the_year_before() {
    for (day, year) in [
        ("2026-03-01", 2025),
        ("2026-03-02", 2026),
        ("2024-02-29", 2023),
        ("2024-03-01", 2024),
    ] {
        assert_eq!(rrsp::deduction_year(d(day)), year, "{day}");
    }
}

#[test]
fn rrsp_window_runs_to_day_60_of_next_year() {
    assert_eq!(rrsp::window(2026), (d("2026-03-02"), d("2027-03-01")));
    assert_eq!(rrsp::window(2027), (d("2027-03-02"), d("2028-02-29")));
}

#[test]
fn rrsp_within_the_limit_at_the_current_pace() {
    let res = check(
        &profile(20000.0, 2026, ""),
        Some(&holdings(&payroll(&MONTHS, 500.0, 250.0, 2026))),
        TODAY_RRSP,
    );
    assert_eq!((res.status.as_str(), res.year), ("ok", 2026));
    assert_eq!(num(&res, "contributed"), 5250.0);
    assert_eq!(num(&res, "left"), 20000.0 - 5250.0);
    assert_eq!(num(&res, "monthly_pace"), 750.0);
    assert!(res.tasks.is_empty());
}

#[test]
fn rrsp_other_buckets_and_other_years_do_not_count() {
    let mut rows = payroll(&MONTHS, 500.0, 250.0, 2026);
    rows.push(contrib("2026-02", 9999.0, "member", 2025, "RRSP"));
    rows.push(contrib("2026-05", 9999.0, "member", 2026, "TFSA"));
    let res = check(
        &profile(20000.0, 2026, ""),
        Some(&holdings(&rows)),
        TODAY_RRSP,
    );
    assert_eq!(num(&res, "contributed"), 5250.0);
}

#[test]
fn rrsp_projected_over_makes_a_settings_reminder_before_the_crossing() {
    let res = check(
        &profile(20000.0, 2026, r#", "rrsp_unused": 2000"#),
        Some(&holdings(&payroll(&MONTHS, 1500.0, 500.0, 2026))),
        TODAY_RRSP,
    );
    assert_eq!(res.status, "projected-over");
    assert_eq!(
        (num(&res, "available"), num(&res, "left")),
        (18000.0, 4000.0)
    );
    let crosses = d(&data(&res, "crosses_on").py_str());
    assert!(d("2026-12-15") <= crosses && crosses <= d("2027-01-05"));
    assert_eq!(res.tasks.len(), 1);
    let t = &res.tasks[0];
    assert_eq!(
        (t.id.as_str(), t.risk.as_str(), t.service.as_str()),
        ("rrsp-pause-2026", "settings", "sunlife")
    );
    let due = t.due.unwrap();
    assert!(d(TODAY_RRSP) <= due && due < crosses);
}

#[test]
fn rrsp_over_inside_the_buffer_warns_without_a_penalty() {
    let res = check(
        &profile(5000.0, 2026, ""),
        Some(&holdings(&payroll(&MONTHS, 500.0, 250.0, 2026))),
        TODAY_RRSP,
    );
    assert_eq!(res.status, "over");
    assert!(!res.data.iter().any(|(k, _)| k == "penalty_per_month"));
    assert!(res.lines.iter().any(|l| l.contains("buffer")));
    assert_eq!(
        res.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["rrsp-pause-2026"]
    );
}

#[test]
fn rrsp_over_the_buffer_shows_the_monthly_tax_and_a_money_reminder() {
    let res = check(
        &profile(1000.0, 2026, ""),
        Some(&holdings(&payroll(&MONTHS, 500.0, 250.0, 2026))),
        TODAY_RRSP,
    );
    assert_eq!(res.status, "penalty");
    assert!(approx(
        num(&res, "penalty_per_month"),
        (5250.0 - 1000.0 - 2000.0) * 0.01
    ));
    assert_eq!(res.tasks.len(), 1);
    assert_eq!(res.tasks[0].risk, "money");
    assert!(res.tasks[0].notes.contains("T1-OVP"));
}

#[test]
fn rrsp_other_contributions_from_the_profile_count() {
    let res = check(
        &profile(20000.0, 2026, r#", "rrsp_other": 1000"#),
        Some(&holdings(&payroll(&MONTHS, 500.0, 250.0, 2026))),
        TODAY_RRSP,
    );
    assert_eq!(num(&res, "contributed"), 6250.0);
}

#[test]
fn rrsp_a_missing_or_old_limit_asks_for_the_noa_figure() {
    for room in [
        "{}",
        r#"{"rrsp": 20000}"#,
        r#"{"rrsp": 20000, "rrsp_limit_year": 2025}"#,
    ] {
        let p = j(&format!(r#"{{"room": {room}}}"#));
        let res = check(
            &p,
            Some(&holdings(&payroll(&MONTHS, 500.0, 250.0, 2026))),
            TODAY_RRSP,
        );
        assert_eq!(res.status, "no-limit", "{room}");
        assert_eq!(res.tasks.len(), 1);
        let t = &res.tasks[0];
        assert_eq!(t.id, "rrsp-limit-2026");
        assert!(t.notes.contains("CRA My Account") && t.notes.contains("rrsp_limit_year = 2026"));
        assert_eq!(num(&res, "contributed"), 5250.0);
    }
}

#[test]
fn rrsp_no_exports_says_where_to_get_them() {
    let res = check(&profile(20000.0, 2026, ""), None, TODAY_RRSP);
    assert_eq!(res.status, "ok");
    assert!(res.lines.iter().any(|l| l.contains("sunlife")));
}

#[test]
fn rrsp_in_january_the_window_is_still_last_year() {
    let mut rows = payroll(&MONTHS, 500.0, 250.0, 2026);
    rows.extend(payroll(
        &["2026-10", "2026-11", "2026-12"],
        500.0,
        250.0,
        2026,
    ));
    let res = check(
        &profile(20000.0, 2026, ""),
        Some(&holdings(&rows)),
        "2027-01-20",
    );
    assert_eq!(res.year, 2026);
    assert_eq!(num(&res, "contributed"), 7500.0);
}

#[test]
fn rrsp_one_lump_sum_does_not_set_the_pace() {
    let rows = vec![
        contrib("2027-01", 15000.0, "member", 2026, "RRSP"),
        contrib("2026-04", 500.0, "member", 2026, "RRSP"),
    ];
    let res = check(
        &profile(30000.0, 2027, ""),
        Some(&holdings(&rows)),
        "2027-03-20",
    );
    assert_eq!(num(&res, "monthly_pace"), 0.0);
    assert_ne!(res.status, "projected-over");
}

#[test]
fn rrsp_an_old_export_says_the_pace_is_unknown() {
    let rows = payroll(&["2026-03", "2026-04", "2026-05"], 500.0, 250.0, 2026);
    let res = check(
        &profile(20000.0, 2026, ""),
        Some(&holdings(&rows)),
        TODAY_RRSP,
    );
    assert_eq!(data(&res, "newest_month").py_str(), "2026-05");
    assert!(
        res.lines
            .iter()
            .any(|l| l.contains("newest contribution") && l.contains("export"))
    );
    assert_eq!(res.status, "ok");
}

#[test]
fn rrsp_over_in_february_points_to_the_new_years_limit() {
    let h = holdings(&payroll(&MONTHS, 500.0, 250.0, 2026));
    let res = check(&profile(1000.0, 2026, ""), Some(&h), "2027-02-10");
    assert_eq!(res.status, "penalty");
    assert!(
        res.lines
            .iter()
            .any(|l| l.contains("first 60 days of 2027"))
    );
    let now = check(&profile(1000.0, 2026, ""), Some(&h), TODAY_RRSP);
    assert!(!now.lines.iter().any(|l| l.contains("first 60 days")));
}

#[test]
fn rrsp_an_old_export_asks_for_a_fresh_one_with_a_task() {
    let rows = payroll(&["2026-03", "2026-04", "2026-05"], 500.0, 250.0, 2026);
    let res = check(
        &profile(20000.0, 2026, ""),
        Some(&holdings(&rows)),
        TODAY_RRSP,
    );
    let t: Vec<_> = res
        .tasks
        .iter()
        .filter(|t| t.id.starts_with("rrsp-refresh"))
        .collect();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].id, "rrsp-refresh-2026-05");
    assert_eq!(
        (t[0].risk.as_str(), t[0].service.as_str()),
        ("read", "sunlife")
    );
    assert_eq!(t[0].due, Some(d(TODAY_RRSP)));
}

#[test]
fn rrsp_a_fresh_export_needs_no_rrsp_task() {
    let res = check(
        &profile(30000.0, 2026, ""),
        Some(&holdings(&payroll(&MONTHS, 500.0, 250.0, 2026))),
        TODAY_RRSP,
    );
    assert_eq!(res.status, "ok");
    assert!(res.tasks.is_empty());
}

// ---------- stock optimizer ----------

const TARGETS: &str = r#"[{"symbol": "EQCA", "weight": 0.5, "asset_class": "equity", "listing": "CA"},
 {"symbol": "EQUS", "weight": 0.3, "asset_class": "equity", "listing": "US"},
 {"symbol": "BOND", "weight": 0.2, "asset_class": "bond"}]"#;

fn run(spec: &str) -> OptPlan {
    match opt::optimize(&j(spec), None) {
        Ok(p) => p,
        Err(e) => panic!("{}", e.0),
    }
}

fn run_err(spec: &str) -> String {
    match opt::optimize(&j(spec), None) {
        Ok(_) => panic!("expected an error for {spec}"),
        Err(e) => e.0,
    }
}

fn trade(p: &OptPlan, account: &str, symbol: &str) -> Option<f64> {
    p.trades
        .iter()
        .find(|t| t.account == account && t.symbol == symbol)
        .map(|t| t.amount)
}

fn w(m: &[(String, f64)], k: &str) -> f64 {
    m.iter().find(|(s, _)| s == k).map_or(0.0, |(_, v)| *v)
}

#[test]
fn optimize_water_fill_tops_up_the_most_underweight_first() {
    let close = |a: Vec<f64>, b: &[f64]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
    assert!(close(
        opt::water_fill(&[100.0, 50.0, -20.0], 60.0),
        &[55.0, 5.0, 0.0]
    ));
    assert!(close(
        opt::water_fill(&[100.0, 50.0, -20.0], 20.0),
        &[20.0, 0.0, 0.0]
    ));
    assert!(approx(
        opt::water_fill(&[10.0, 5.0], 100.0).iter().sum(),
        100.0
    ));
    assert_eq!(opt::water_fill(&[10.0, 5.0], 0.0), vec![0.0, 0.0]);
}

#[test]
fn optimize_enough_cash_reaches_the_targets_exactly() {
    let p = run(&format!(
        r#"{{"targets": {TARGETS}, "cash": {{"tfsa": 6000, "rrsp": 4000}}}}"#
    ));
    assert!(p.drift(&p.after).abs() < 1e-9);
    assert!(approx(trade(&p, "rrsp", "BOND").unwrap(), 2000.0));
    assert!(approx(trade(&p, "rrsp", "EQUS").unwrap(), 2000.0));
    assert!(approx(trade(&p, "tfsa", "EQUS").unwrap(), 1000.0));
    assert!(approx(trade(&p, "tfsa", "EQCA").unwrap(), 5000.0));
}

#[test]
fn optimize_no_sell_mode_never_sells_and_lowers_the_drift() {
    let p = run(&format!(
        r#"{{"targets": {TARGETS}, "cash": {{"tfsa": 1000}}, "holdings": [{{"account": "tfsa", "symbol": "EQCA", "value": 9000}},
           {{"account": "Non-Registered", "symbol": "OLD", "value": 500}}]}}"#
    ));
    assert!(p.trades.iter().all(|t| t.amount > 0.0));
    assert!(p.drift(&p.after) < p.drift(&p.before));
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("OLD is held but has no target"))
    );
}

#[test]
fn optimize_sell_mode_sells_registered_first_and_reaches_targets() {
    let p = run(
        r#"{"targets": [{"symbol": "A", "weight": 0.5}, {"symbol": "B", "weight": 0.5}], "allow_sell": true,
            "holdings": [{"account": "tfsa", "symbol": "A", "value": 600}, {"account": "non-registered", "symbol": "A", "value": 400}]}"#,
    );
    let sells: Vec<(&str, f64)> = p
        .trades
        .iter()
        .filter(|t| t.amount < 0.0)
        .map(|t| (t.account.as_str(), t.amount))
        .collect();
    assert_eq!(sells.len(), 1);
    assert_eq!(sells[0].0, "tfsa");
    assert!(approx(sells[0].1, -500.0));
    assert!(approx(w(&p.after, "A"), 0.5) && approx(w(&p.after, "B"), 0.5));
}

#[test]
fn optimize_whole_units_and_minimum_trade() {
    let spec = r#"{"targets": [{"symbol": "A", "weight": 0.5, "price": 30}, {"symbol": "B", "weight": 0.5}], "cash": {"tfsa": 100}, "min_trade": MIN}"#;
    assert!(run(&spec.replace("MIN", "60")).trades.is_empty());
    let p = run(&spec.replace("MIN", "0"));
    let a = p.trades.iter().find(|t| t.symbol == "A").unwrap();
    assert_eq!((a.units, a.amount), (Some(1), 30.0));
    assert!(approx(w(&p.left, "tfsa"), 20.0));
}

#[test]
fn optimize_a_targets_own_accounts_win() {
    let p = run(
        r#"{"targets": [{"symbol": "A", "weight": 1, "accounts": ["Non-Registered"]}], "cash": {"tfsa": 100, "non-registered": 50}}"#,
    );
    let t: Vec<(&str, f64)> = p
        .trades
        .iter()
        .map(|t| (t.account.as_str(), t.amount))
        .collect();
    assert_eq!(t, vec![("non-registered", 50.0)]);
    assert!(p.notes.iter().any(|n| n.contains("no cash left")));
}

#[test]
fn optimize_an_unknown_account_takes_only_the_targets_that_name_it() {
    let p = run(
        r#"{"targets": [{"symbol": "XEQT", "weight": 0.6}, {"symbol": "PLANFUND", "weight": 0.4, "accounts": ["group-rrsp"]}],
            "cash": {"tfsa": 600, "group-rrsp": 400}}"#,
    );
    assert_eq!(p.trades.len(), 2);
    assert!(approx(trade(&p, "tfsa", "XEQT").unwrap(), 600.0));
    assert!(approx(trade(&p, "group-rrsp", "PLANFUND").unwrap(), 400.0));
    let p = run(r#"{"targets": [{"symbol": "XEQT", "weight": 1}], "cash": {"group-rrsp": 400}}"#);
    assert!(p.trades.is_empty() && p.notes.iter().any(|n| n.contains("no target may go there")));
}

#[test]
fn optimize_a_symbol_listed_twice_is_an_error() {
    let e = run_err(
        r#"{"targets": [{"symbol": "A", "weight": 0.5}, {"symbol": "a", "weight": 0.5}], "cash": {"tfsa": 1}}"#,
    );
    assert!(e.contains("listed twice"), "{e}");
}

#[test]
fn optimize_cash_left_by_rounding_buys_the_unit_that_helps_most() {
    let spec = r#"{"targets": [{"symbol": "A", "weight": 0.5, "price": PA}, {"symbol": "B", "weight": 0.5, "price": PB}], "cash": {"tfsa": 200}}"#;
    let p = run(&spec.replace("PA", "40").replace("PB", "70"));
    let units = |p: &OptPlan, s: &str| {
        p.trades
            .iter()
            .find(|t| t.symbol == s)
            .and_then(|t| t.units)
    };
    assert_eq!((units(&p, "A"), units(&p, "B")), (Some(2), Some(1)));
    assert!(approx(w(&p.left, "tfsa"), 50.0));
    let p = run(&spec.replace("PA", "60").replace("PB", "60"));
    assert_eq!(
        p.trades.iter().map(|t| t.units.unwrap_or(0)).sum::<i64>(),
        3
    );
    assert!(approx(w(&p.left, "tfsa"), 20.0));
}

#[test]
fn optimize_sales_are_whole_units() {
    let p = run(
        r#"{"targets": [{"symbol": "A", "weight": 0.5, "price": 30}, {"symbol": "B", "weight": 0.5}], "allow_sell": true,
            "holdings": [{"account": "tfsa", "symbol": "A", "value": 1000}]}"#,
    );
    let sale = p.trades.iter().find(|t| t.amount < 0.0).unwrap();
    assert!(approx(-sale.amount, 16.0 * 30.0));
}

#[test]
fn optimize_holdings_from_the_engine_use_the_given_prices() {
    let engine = j(
        r#"{"assets": [{"symbol": "A", "account_bucket": "TFSA", "quantity": 10}, {"symbol": "ZZ", "account_bucket": "RRSP", "quantity": 1}]}"#,
    );
    let p = match opt::optimize(
        &j(r#"{"targets": [{"symbol": "A", "weight": 1, "price": 10}], "cash": {}}"#),
        Some(&engine),
    ) {
        Ok(p) => p,
        Err(e) => panic!("{}", e.0),
    };
    assert_eq!(p.before, vec![("A".to_string(), 1.0)]);
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("ZZ") && n.contains("no price"))
    );
}

#[test]
fn optimize_bad_inputs_explain() {
    for (spec, m) in [
        (r#"{"targets": []}"#, "at least one"),
        (
            r#"{"targets": [{"symbol": "A", "weight": 0.7}]}"#,
            "add up to 1",
        ),
        (
            r#"{"targets": [{"symbol": "A", "weight": 1}], "cash": {"tfsa": -1}}"#,
            "0 or more",
        ),
        (
            r#"{"targets": [{"symbol": "A", "weight": 1}]}"#,
            "nothing to invest",
        ),
    ] {
        let e = run_err(spec);
        assert!(e.contains(m), "{spec}: {e}");
    }
}

#[test]
fn optimize_missing_inputs_point_at_the_profile() {
    let e = opt::load(&j("{}"), None)
        .err()
        .map(|e| e.0)
        .unwrap_or_default();
    assert!(e.contains("profile.toml"), "{e}");
}

/// A small deterministic generator for the property test (no extra crates).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn int(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() * (hi - lo + 1) as f64) as usize % (hi - lo + 1)
    }
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.int(0, xs.len() - 1)]
    }
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[test]
fn optimize_properties_hold_for_random_portfolios() {
    for seed in 0..300u64 {
        let mut rng = Rng(seed.wrapping_mul(2654435761).wrapping_add(17));
        let n = rng.int(1, 6);
        let syms: Vec<String> = (0..n).map(|i| format!("S{i}")).collect();
        let raw: Vec<f64> = (0..n).map(|_| rng.next() + 0.01).collect();
        let total: f64 = raw.iter().sum();
        let mut weights: Vec<f64> = raw.iter().map(|x| x / total).collect();
        let head: f64 = weights[..n - 1].iter().sum();
        weights[n - 1] = 1.0 - head;
        let priced = rng.next() < 0.4;
        let mut pool = vec!["tfsa", "rrsp", "fhsa", "non-registered", "group-rrsp"];
        let k = rng.int(1, 4);
        let mut accts = Vec::new();
        for _ in 0..k {
            let i = rng.int(0, pool.len() - 1);
            accts.push(pool.remove(i));
        }
        let mut targets = Vec::new();
        let mut allowed: Vec<(String, Vec<String>)> = Vec::new();
        for (s, wt) in syms.iter().zip(&weights) {
            let class = rng.pick(&["equity", "bond"]);
            let listing = rng.pick(&["CA", "US"]);
            let mut t = format!(
                r#"{{"symbol": "{s}", "weight": {wt:?}, "asset_class": "{class}", "listing": "{listing}""#
            );
            if priced {
                t += &format!(r#", "price": {:?}"#, r2(rng.uniform(5.0, 300.0)));
            }
            let named = rng.next() < 0.15;
            if named {
                t += r#", "accounts": ["group-rrsp"]"#;
            }
            t += "}";
            targets.push(t);
            allowed.push((
                s.clone(),
                if named {
                    vec!["group-rrsp".into()]
                } else {
                    vec![]
                },
            ));
        }
        let cash: Vec<(String, f64)> = accts
            .iter()
            .map(|a| (a.to_string(), r2(rng.uniform(0.0, 5000.0))))
            .collect();
        let nh = rng.int(0, 6);
        let mut hold: Vec<(String, String, f64)> = Vec::new();
        for _ in 0..nh {
            let a = rng.pick(&accts).to_string();
            let s = syms[rng.int(0, n - 1)].clone();
            hold.push((a, s, r2(rng.uniform(0.0, 8000.0))));
        }
        let allow_sell = rng.next() < 0.3;
        let min_trade = [0, 0, 25][rng.int(0, 2)];
        let cash_sum: f64 = cash.iter().map(|c| c.1).sum();
        if cash_sum + hold.iter().map(|h| h.2).sum::<f64>() <= 0.0 {
            continue;
        }
        let spec = format!(
            r#"{{"targets": [{}], "cash": {{{}}}, "holdings": [{}], "allow_sell": {allow_sell}, "min_trade": {min_trade}}}"#,
            targets.join(","),
            cash.iter()
                .map(|(a, v)| format!(r#""{a}": {v:?}"#))
                .collect::<Vec<_>>()
                .join(","),
            hold.iter()
                .map(|(a, s, v)| format!(
                    r#"{{"account": "{a}", "symbol": "{s}", "value": {v:?}}}"#
                ))
                .collect::<Vec<_>>()
                .join(",")
        );
        let p = match opt::optimize(&j(&spec), None) {
            Ok(p) => p,
            Err(e) => panic!("seed {seed}: {}\n{spec}", e.0),
        };
        let held = |a: &str, s: &str| {
            hold.iter()
                .filter(|h| h.0 == a && h.1 == s)
                .map(|h| h.2)
                .sum::<f64>()
        };
        let names = |s: &str| {
            allowed
                .iter()
                .find(|(x, _)| x == s)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let mut spent: Vec<(String, f64)> = Vec::new();
        for t in &p.trades {
            match spent.iter_mut().find(|(a, _)| *a == t.account) {
                Some(s) => s.1 += t.amount,
                None => spent.push((t.account.clone(), t.amount)),
            }
            if t.amount < 0.0 {
                assert!(
                    allow_sell && -t.amount <= held(&t.account, &t.symbol) + 1e-6,
                    "seed {seed}"
                );
            } else {
                let named = names(&t.symbol);
                assert!(
                    t.account != "group-rrsp" || named.contains(&"group-rrsp".to_string()),
                    "seed {seed}"
                );
                assert!(
                    named.is_empty() || named.contains(&t.account),
                    "seed {seed}"
                );
            }
        }
        for (a, used) in &spent {
            assert!(*used <= w(&cash, a) + 1e-6, "seed {seed}: {a} spent {used}");
        }
        let simple = !priced
            && min_trade == 0
            && !accts.contains(&"group-rrsp")
            && allowed.iter().all(|(_, v)| v.is_empty());
        if simple {
            assert!(
                p.drift(&p.after) <= p.drift(&p.before) + 1e-9 || p.before.is_empty(),
                "seed {seed}"
            );
            if !allow_sell {
                let placed: f64 = spent.iter().map(|s| s.1).sum();
                assert!(
                    (placed - cash_sum).abs() <= 0.05,
                    "seed {seed}: {placed} of {cash_sum}"
                );
            }
        }
        let after: f64 = p.after.iter().map(|x| x.1).sum();
        assert!(
            p.after.is_empty() || (after - 1.0).abs() < 1e-6,
            "seed {seed}"
        );
    }
}

// ---------- conversion and bank fees ----------

#[test]
fn foreign_conversion_picks_the_cheaper_way() {
    let norbert = j(r#"{"fixed_cad": 20, "spread_rate": 0.0005}"#);
    let small = foreign::conversion(1000.0, 0.0046, &norbert);
    let large = foreign::conversion(50000.0, 0.0046, &norbert);
    assert_eq!((small.cheaper, large.cheaper), ("Wise", "Norbert's gambit"));
    assert!(approx(small.breakeven, 20.0 / 0.0041));
}

#[test]
fn banking_holding_cost_subtracts_what_the_account_pays() {
    assert!(approx(
        banking::verdict(10.0, 10000.0, 0.0, 0.03, 0.02).hold_cost,
        100.0
    ));
    assert_eq!(
        banking::verdict(10.0, 10000.0, 0.0, 0.02, 0.03).hold_cost,
        0.0
    );
}

#[test]
fn banking_no_fee_account_is_not_told_to_pay_the_fee() {
    assert_eq!(banking::verdict(0.0, 0.0, 0.0, 0.02, 0.0).choice, "no fee");
}
