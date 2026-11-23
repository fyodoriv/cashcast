//! The forecast model and its backtest. Synthetic inputs only.
//! Ported from forecast/test_model.py and forecast/test_backtest.py.

use cashcast_engine::core::pyjson::{Json, loads};
use cashcast_engine::forecast::backtest;
use cashcast_engine::forecast::model::{
    ExtraItem, History, Inputs, add_months, inputs_from, paydays, run,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::os::unix::fs::PermissionsExt;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}
fn n(v: i64) -> Decimal {
    Decimal::from(v)
}

fn inputs() -> Inputs {
    Inputs {
        start: d(2026, 10, 1),
        horizon: 15,
        opening_accounts: vec![("Bank".into(), n(1000))],
        card_balances: vec![],
        schedule: "biweekly".into(),
        net_h1: n(100),
        net_h2: n(200),
        anchor: Some(d(2026, 5, 8)),
        holidays: vec![
            ("New Year".into(), d(2027, 1, 1)),
            ("Good Friday".into(), d(2027, 3, 26)),
        ],
        rent_category: Some("Housing > Rent".into()),
        rent_charged: Some(n(1000)),
        floor_parts: vec![("Bank".into(), n(1500))],
        method: "trailing-12 average".into(),
        baseline: n(0),
        skip_categories: vec![],
        extra_income: vec![],
        taxes: vec![],
        base_currency: "CAD".into(),
        assumptions: vec![],
    }
}

fn history(food: &[i64]) -> History {
    let months: Vec<NaiveDate> = (0..12).map(|i| add_months(d(2025, 10, 1), i)).collect();
    let zero = vec![n(0); 12];
    History {
        months,
        salary: zero.clone(),
        salary_off_cycle: zero.clone(),
        income: vec![],
        spending: vec![
            ("Housing > Rent".into(), zero.clone()),
            ("Food".into(), food.iter().map(|x| n(*x)).collect()),
        ],
        income_tax: zero.clone(),
        invested: zero,
    }
}

fn spend(row: &cashcast_engine::forecast::model::Row, cat: &str) -> Decimal {
    row.spending.iter().find(|(c, _)| c == cat).unwrap().1
}

#[test]
fn new_year_payday_moves_into_december() {
    let days = paydays(&inputs(), 80);
    assert!(days.contains(&d(2026, 12, 31)) && !days.contains(&d(2027, 1, 1)));
    assert!(days.contains(&d(2027, 3, 25)));
    assert_eq!(days[0], d(2026, 10, 9));
}

#[test]
fn cheques_per_month() {
    let rows = run(&inputs(), &history(&[0; 12])).unwrap();
    let c = |m: &str| {
        rows.iter()
            .find(|r| r.month.format("%Y-%m").to_string() == m)
            .unwrap()
            .paycheques
    };
    assert_eq!((c("2026-12"), c("2027-01"), c("2027-07")), (3, 2, 3));
    assert_eq!(rows.iter().map(|r| r.paycheques).sum::<i64>(), 33);
}

#[test]
fn other_schedules() {
    let with = |s: &str| {
        let mut i = inputs();
        i.schedule = s.into();
        run(&i, &history(&[0; 12])).unwrap()
    };
    assert!(with("monthly").iter().all(|r| r.paycheques == 1));
    assert!(with("semimonthly").iter().all(|r| r.paycheques == 2));
    assert!(with("none").iter().all(|r| r.paycheques == 0));
    assert_eq!(with("weekly").iter().map(|r| r.paycheques).sum::<i64>(), 66);
}

#[test]
fn to_invest_is_cash_above_floor() {
    let mut i = inputs();
    i.extra_income = vec![ExtraItem {
        name: "Bonus".into(),
        months: vec![10],
        amount: n(2000),
        ..Default::default()
    }];
    i.horizon = 1;
    let r = &run(&i, &history(&[120; 12])).unwrap()[0];
    assert_eq!(
        (r.net_cash_flow, r.to_invest, r.month_end),
        (n(1280), n(780), n(1500))
    );
}

/// Months that say "invest" while a later month-end is under the floor.
fn contradicting(rows: &[cashcast_engine::forecast::model::Row]) -> usize {
    (0..rows.len())
        .filter(|&k| {
            rows[k].to_invest > Decimal::ZERO && rows[k + 1..].iter().any(|r| r.month_end < r.floor)
        })
        .count()
}

/// A bonus now, then months that spend more than they earn: the bonus stays
/// as cash for the dip instead of being invested (forecast-invest-lookahead).
#[test]
fn to_invest_never_pushes_a_later_month_under_the_floor() {
    let mut i = inputs();
    i.extra_income = vec![ExtraItem {
        name: "Bonus".into(),
        months: vec![10],
        amount: n(2000),
        ..Default::default()
    }];
    let rows = run(&i, &history(&[120; 12])).unwrap();
    assert!(rows.iter().any(|r| r.month_end < r.floor), "the dip exists");
    assert_eq!(contradicting(&rows), 0);
    assert_eq!(rows[0].to_invest, n(0));
}

/// Invest only what the lowest later month can spare; the rest waits.
#[test]
fn to_invest_keeps_what_a_later_dip_needs() {
    let mut i = inputs();
    i.horizon = 3;
    i.rent_charged = Some(n(0));
    let item = |name: &str, month: i64, amount: i64| ExtraItem {
        name: name.into(),
        months: vec![month],
        amount: n(amount),
        ..Default::default()
    };
    i.extra_income = vec![item("Bonus", 10, 2000), item("Repair", 11, -1500)];
    let rows = run(&i, &history(&[0; 12])).unwrap();
    // No investing: 3400 in October, 2300 in November, 2900 in December.
    let invest: Vec<Decimal> = rows.iter().map(|r| r.to_invest).collect();
    let ends: Vec<Decimal> = rows.iter().map(|r| r.month_end).collect();
    assert_eq!(invest, vec![n(800), n(0), n(600)]);
    assert_eq!(ends, vec![n(2600), n(1500), n(1500)]);
    assert_eq!(contradicting(&rows), 0);
}

/// The workbook formulas invest the same look-ahead amounts as the model, so
/// the formula check passes once a month has cash to invest.
#[test]
fn workbook_to_invest_matches_the_model_with_a_later_dip() {
    use cashcast_engine::forecast::{build, verify};
    let mut i = inputs();
    i.horizon = 3;
    i.rent_charged = Some(n(0));
    let item = |name: &str, month: i64, amount: i64| ExtraItem {
        name: name.into(),
        months: vec![month],
        amount: n(amount),
        ..Default::default()
    };
    i.extra_income = vec![item("Bonus", 10, 2000), item("Repair", 11, -1500)];
    let hist = history(&[0; 12]);
    let rows = run(&i, &hist).unwrap();
    let book = build::build(&i, &hist);
    let actual = verify::from_book(&book, i.horizon)
        .unwrap_or_else(|_| panic!("the workbook could not be read"));
    let diffs = verify::compare(&rows, &actual);
    assert!(diffs.is_empty(), "workbook differs from the model: {diffs:?}");
}

#[test]
fn flat_baseline_scales_categories_to_the_baseline() {
    let mut i = inputs();
    i.method = "flat baseline".into();
    i.baseline = n(1060);
    let r = &run(&i, &history(&[120; 12])).unwrap()[0];
    assert_eq!((spend(r, "Food"), r.expenses), (n(60), n(1060)));
}

/// forecast-baseline-ignored: outside "flat baseline" the baseline is only a
/// floor (rent included). A baseline below history changes nothing.
#[test]
fn baseline_is_a_floor_unless_the_method_is_flat_baseline() {
    let h = history(&[120; 12]);
    let mut i = inputs();
    i.baseline = n(1060);
    let r = &run(&i, &h).unwrap()[0];
    assert_eq!(
        (spend(r, "Food"), r.untracked, r.expenses),
        (n(120), n(0), n(1120))
    );
    i.baseline = n(1500);
    let r = &run(&i, &h).unwrap()[0];
    assert_eq!((r.untracked, r.expenses), (n(380), n(1500)));
}

#[test]
fn same_month_last_year_uses_that_calendar_month() {
    let mut food = [0; 12];
    food[0] = 300;
    let mut i = inputs();
    i.method = "same month last year".into();
    let rows = run(&i, &history(&food)).unwrap();
    assert_eq!(
        (spend(&rows[0], "Food"), spend(&rows[1], "Food")),
        (n(300), n(0))
    );
}

#[test]
fn without_rent_the_history_stays() {
    let mut h = history(&[120; 12]);
    h.spending[0].1 = vec![n(900); 12];
    let mut i = inputs();
    i.rent_category = None;
    i.rent_charged = None;
    let r = &run(&i, &h).unwrap()[0];
    assert_eq!(
        (spend(r, "Housing > Rent"), spend(r, "Food")),
        (n(900), n(120))
    );
}

fn summary() -> Json {
    let months: Vec<String> = (0..12)
        .map(|i| add_months(d(2025, 10, 1), i).format("%Y-%m").to_string())
        .collect();
    let z = "[0,0,0,0,0,0,0,0,0,0,0,0]";
    loads(&format!(
        r#"{{"base_currency": "CAD", "months": {months:?},
        "flows_cents": {{"salary": {z}, "salary_off_cycle": {z}, "income_tax": {z}, "invested": {z}}},
        "income_cents": {{}}, "spending_cents": {{"Food": [50000,50000,50000,50000,50000,50000,50000,50000,50000,50000,50000,50000]}},
        "accounts": [
          {{"name": "Chequing", "class": "cash", "balance_cents": 123400, "balance_source": "running balance", "sum_balance_cents": 1}},
          {{"name": "Savings", "class": "cash", "balance_cents": null, "balance_source": null, "sum_balance_cents": 1}},
          {{"name": "Visa", "class": "card", "balance_cents": -5000, "balance_source": "statement balance", "sum_balance_cents": 1}}],
        "pay": {{"schedule": "biweekly", "anchor": "2026-09-25", "half_years": [
          {{"period": "2026-H1", "median_cents": 300000}}, {{"period": "2026-H2", "median_cents": 310000}}]}},
        "suggested_extra_income": [{{"name": "Deposits #2", "pattern": "yearly", "months": [8], "amount_cents": 100000}}]}}"#
    ))
    .unwrap()
}

#[test]
fn missing_balance_is_a_clear_question() {
    let e = inputs_from(&toml::Table::new(), &summary(), d(2026, 10, 2))
        .unwrap()
        .unwrap_err();
    assert_eq!(e.0, vec!["balance needed: Savings"]);
}

#[test]
fn defaults_fill_everything_else_and_are_listed() {
    let cfg: toml::Table = toml::from_str("[balances]\nSavings = 100\n").unwrap();
    let i = inputs_from(&cfg, &summary(), d(2026, 10, 2))
        .unwrap()
        .unwrap();
    assert_eq!(i.opening_cash(), n(1334));
    assert_eq!(
        (i.schedule.as_str(), i.net_h1, i.net_h2, i.anchor),
        ("biweekly", n(3000), n(3100), Some(d(2026, 9, 25)))
    );
    assert_eq!(i.floor(), n(1500));
    assert_eq!(i.card_balances, vec![("Visa".to_string(), n(-50))]);
    assert_eq!(i.start, d(2026, 10, 1));
    assert!(
        i.assumptions
            .iter()
            .any(|a| a.contains("Cash floor defaults"))
    );
    assert!(i.extra_income.is_empty());
    let cfg: toml::Table =
        toml::from_str("[balances]\nSavings = 100\n[forecast]\nuse_suggestions = true\n").unwrap();
    let s = inputs_from(&cfg, &summary(), d(2026, 10, 2))
        .unwrap()
        .unwrap();
    assert_eq!(s.extra_income.len(), 1);
    let x = &s.extra_income[0];
    assert_eq!(
        (x.name.as_str(), x.months.clone(), x.amount),
        ("Deposits #2", vec![8], n(1000))
    );
}

// ---------- backtest ----------

fn long_history(months: i32) -> History {
    let start = add_months(d(2026, 10, 1), -months);
    let len = months as usize;
    let zero = vec![n(0); len];
    History {
        months: (0..months).map(|i| add_months(start, i)).collect(),
        salary: vec![n(400); len],
        salary_off_cycle: zero.clone(),
        income: vec![("gift".into(), vec![n(10); len])],
        spending: vec![
            ("Housing > Rent".into(), vec![n(1000); len]),
            (
                "Food".into(),
                (0..len as i64).map(|i| n(100 + 20 * i)).collect(),
            ),
        ],
        income_tax: vec![n(-5); len],
        invested: zero,
    }
}

fn misses_from(h: &History, origin: NaiveDate) -> Vec<backtest::Miss> {
    backtest::backtest(&inputs(), h)
        .unwrap()
        .into_iter()
        .filter(|m| m.origin == origin)
        .collect()
}

#[test]
fn origins_need_twelve_months_before() {
    let h = long_history(20);
    assert_eq!(backtest::origins(&h), h.months[12..].to_vec());
}

#[test]
fn truncate_keeps_only_the_past() {
    let h = long_history(30);
    let past = backtest::truncate(&h, h.months[12]);
    assert_eq!(past.months, h.months[..12].to_vec());
    assert_eq!(*past.spend("Food").unwrap().last().unwrap(), n(320));
}

#[test]
fn misses_are_forecast_minus_actual_without_looking_ahead() {
    let h = long_history(30);
    let all = misses_from(&h, h.months[12]);
    let food: Vec<_> = all.iter().filter(|m| m.line == "cat:Food").collect();
    assert_eq!(
        food.iter().map(|m| m.ahead).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        (food[0].forecast, food[0].actual, food[0].error()),
        (n(210), n(340), n(-130))
    );
    assert!(food.iter().all(|m| m.error() < n(0)));
}

#[test]
fn lines_count_extra_income_taxes_and_the_running_month_end() {
    let h = long_history(30);
    let all = misses_from(&h, h.months[12]);
    let lines = backtest::by_line(&all);
    let line = |k: &str| lines.iter().find(|(l, _)| l == k).unwrap().1.clone();
    assert_eq!(line("extra_income")[0].actual, n(10));
    assert_eq!(line("taxes")[0].actual, n(-5));
    let net: Vec<Decimal> = line("net_cash_flow").iter().map(|m| m.error()).collect();
    let want: Vec<Decimal> = (0..6).map(|k| net[..=k].iter().copied().sum()).collect();
    assert_eq!(
        line("month_end")
            .iter()
            .map(|m| m.error())
            .collect::<Vec<_>>(),
        want
    );
}

#[test]
fn score_sizes_and_signs() {
    let o = d(2026, 1, 1);
    let m = |ahead, f| backtest::Miss {
        origin: o,
        ahead,
        line: "x".into(),
        forecast: n(f),
        actual: n(100),
    };
    let (a, b) = (m(1, 110), m(2, 70));
    let sc = backtest::score(&[&a, &b]);
    assert_eq!(
        (sc.mean_abs, sc.bias, sc.wape),
        (n(20), n(-10), Some(Decimal::new(2, 1)))
    );
    assert!(backtest::score(&[]).wape.is_none());
}

#[test]
fn saved_forecast_is_scored_against_months_imported_later() {
    let h = long_history(30);
    let origin = h.months[h.months.len() - 3];
    let before = backtest::truncate(&h, origin);
    let mut inp = inputs();
    inp.start = origin;
    inp.horizon = 6;
    let home = std::env::temp_dir().join(format!("cashcast-forecast-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    assert!(
        backtest::latest_snapshot(&home, &h, d(2026, 10, 2))
            .unwrap()
            .is_none()
    );
    let path =
        backtest::save_snapshot(&home, &inp, &run(&inp, &before).unwrap(), d(2026, 7, 2)).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode(&path), mode(path.parent().unwrap())), (0o600, 0o700));
    assert!(
        backtest::latest_snapshot(&home, &h, d(2026, 7, 2))
            .unwrap()
            .is_none(),
        "a snapshot made today is not scored"
    );
    let snap = backtest::latest_snapshot(&home, &h, d(2026, 10, 2))
        .unwrap()
        .unwrap();
    let rows = backtest::compare_snapshot(&snap, &h).unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.month.as_str()).collect::<Vec<_>>(),
        vec!["2026-07", "2026-08", "2026-09"]
    );
    let food = rows[0]
        .categories
        .iter()
        .find(|(c, _)| c == "Food")
        .unwrap()
        .1;
    assert_eq!(food, (n(510), n(640)));
    let cum: Decimal = rows
        .iter()
        .map(|r| r.forecast("net_cash_flow") - r.actual("net_cash_flow"))
        .sum();
    assert_eq!(
        rows[2].forecast("month_end") - rows[2].actual("month_end"),
        cum
    );
}

#[test]
fn median_ignores_a_one_off_spike_month() {
    // Eleven ordinary months and one one-off spike (10x normal).
    let mut food = [100; 12];
    food[4] = 1000;
    let mut i = inputs();
    i.method = "trailing-12 median".into();
    let r = &run(&i, &history(&food)).unwrap()[0];
    assert_eq!(spend(r, "Food"), n(100));
    // The average carries the spike into every month: 175.
    i.method = "trailing-12 average".into();
    let r = &run(&i, &history(&food)).unwrap()[0];
    assert_eq!(spend(r, "Food"), n(175));
}

#[test]
fn median_of_an_even_window_averages_the_middle_two() {
    let food = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
    let mut i = inputs();
    i.method = "trailing-12 median".into();
    let r = &run(&i, &history(&food)).unwrap()[0];
    assert_eq!(spend(r, "Food"), n(65));
}

#[test]
fn median_is_an_allowed_config_method() {
    use cashcast_engine::forecast::model::METHODS;
    assert!(METHODS.contains(&"trailing-12 median"));
}

#[test]
fn skipped_one_offs_leave_the_backtest_actuals_too() {
    let mut h = long_history(30);
    h.spending.push((
        "Moving".into(),
        (0..30).map(|i| n(if i == 14 { 9000 } else { 0 })).collect(),
    ));
    let mut inp = inputs();
    inp.skip_categories = vec!["Moving".into()];
    let all: Vec<_> = backtest::backtest(&inp, &h)
        .unwrap()
        .into_iter()
        .filter(|m| m.origin == h.months[12])
        .collect();
    assert!(all.iter().all(|m| m.line != "cat:Moving"));
    let spend = all
        .iter()
        .find(|m| m.line == "spending_excl_rent" && m.ahead == 3)
        .unwrap();
    assert!(
        spend.actual < n(1000),
        "the one-off stays out of actual spending"
    );
}

#[test]
fn dated_extra_income_counts_only_inside_its_dates_once_known() {
    let item = |from, until, known| ExtraItem {
        name: "Vest".into(),
        months: (1..=12).collect(),
        amount: n(500),
        from,
        until,
        known,
    };
    let extra = |x: ExtraItem| {
        let mut i = inputs();
        i.extra_income = vec![x];
        run(&i, &history(&[0; 12]))
            .unwrap()
            .iter()
            .map(|r| r.extra_income)
            .collect::<Vec<_>>()
    };
    // The forecast starts 2026-10.
    let e = extra(item(Some(d(2026, 12, 1)), Some(d(2027, 2, 1)), None));
    assert_eq!(e[..6], [n(0), n(0), n(500), n(500), n(500), n(0)]);
    // Not yet known at the forecast start: left out.
    let e = extra(item(None, None, Some(d(2026, 11, 1))));
    assert!(e.iter().all(|v| v.is_zero()));
}

#[test]
fn quarters_sum_full_three_month_blocks_per_origin() {
    let o = d(2026, 1, 1);
    let m = |ahead, f, a| backtest::Miss {
        origin: o,
        ahead,
        line: "net_cash_flow".into(),
        forecast: n(f),
        actual: n(a),
    };
    // A payment one month late: big monthly misses, none over the quarter.
    let ms = vec![m(1, 100, 0), m(2, 0, 100), m(3, 50, 50), m(4, 10, 10)];
    let q = backtest::quarters(&ms.iter().collect::<Vec<_>>());
    assert_eq!(q.len(), 1, "the partial second quarter is dropped");
    assert_eq!(
        (q[0].forecast, q[0].actual, q[0].error()),
        (n(150), n(150), n(0))
    );
}

#[test]
fn month_end_ranges_use_backtest_percentiles_then_widen() {
    use backtest::{Miss, month_end_ranges};
    let o = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let miss = |ahead, err: i64| Miss {
        origin: o,
        ahead,
        line: "month_end".into(),
        forecast: Decimal::ZERO,
        actual: Decimal::from(err),
    };
    let mut ms: Vec<Miss> = (0..=10).map(|e| miss(1, e * 100 - 500)).collect();
    ms.push(miss(2, -2000));
    ms.push(miss(2, 2000));
    let r = month_end_ranges(&ms, 8);
    assert_eq!(r.len(), 8);
    assert!((r[0].0 + 400.0).abs() < 1e-9 && (r[0].1 - 400.0).abs() < 1e-9);
    assert!((r[1].0 + 1600.0).abs() < 1e-9);
    assert!(
        (r[7].1 - 3200.0).abs() < 1e-9,
        "8 ahead is sqrt(4) x the 2-ahead range"
    );
    assert!(month_end_ranges(&[], 3).is_empty());
}

#[test]
fn by_method_scores_every_method_on_the_same_history() {
    let h = long_history(30);
    let all = backtest::by_method(&inputs(), &h);
    assert_eq!(
        all.iter().map(|(m, _)| *m).collect::<Vec<_>>(),
        cashcast_engine::forecast::model::METHODS.to_vec()
    );
    let now = backtest::backtest(&inputs(), &h).unwrap();
    let net: Vec<_> = now.iter().filter(|m| m.line == "net_cash_flow").collect();
    let current = inputs().method;
    let row = all.iter().find(|(m, _)| *m == current).unwrap();
    assert_eq!(row.1, backtest::score(&net).wape);
    assert!(all.iter().all(|(_, w)| w.is_some()));
}

#[test]
fn if_exact_shows_which_line_loses_the_most_accuracy() {
    let m = |month: u32, line: &str, f: i64, a: i64| backtest::Miss {
        origin: d(2026, month, 1),
        ahead: 1,
        line: line.into(),
        forecast: n(f),
        actual: n(a),
    };
    // March: net misses by +30 (income 10 too low: -10; spending 40 too low: +40).
    // May: net misses by +20 (taxes forecast 10 too high: +10; rent 10 too low: +10).
    let misses = vec![
        m(3, "net_cash_flow", 130, 100),
        m(3, "extra_income", 90, 100),
        m(3, "spending_excl_rent", 160, 200),
        m(3, "cat:Travel", 60, 100),
        m(3, "month_end", 130, 100),
        m(5, "net_cash_flow", 120, 100),
        m(5, "taxes", 10, 0),
        m(5, "rent", 90, 100),
    ];
    let got: Vec<(String, Option<Decimal>)> = backtest::if_exact(&misses);
    let at = |k: &str| got.iter().find(|(x, _)| x == k).unwrap().1.unwrap();
    let pct = |p: i64| Decimal::new(p, 2);
    assert_eq!(at("extra_income"), pct(30), "40 + 20 of 200; May keeps its own miss");
    assert_eq!(at("spending_excl_rent"), pct(15), "10 + 20 of 200");
    assert_eq!(at("cat:Travel"), pct(15));
    assert_eq!(at("taxes"), pct(20), "taxes add to net: 30 + 10 of 200");
    assert_eq!(at("rent"), pct(20), "rent takes from net: 30 + 10 of 200");
    assert!(got.iter().all(|(k, _)| k != "net_cash_flow" && k != "month_end"));
}

#[test]
fn history_that_ends_before_last_month_still_gives_twelve_months() {
    // The import left out September (the data did not cover it): the history ends in August.
    let mut h = history(&[120; 12]);
    h.months = (0..12).map(|i| add_months(d(2025, 9, 1), i)).collect();
    let mut i = inputs();
    let rows = run(&i, &h).unwrap();
    assert_eq!(spend(&rows[0], "Food"), n(120), "average over 12 months, not 11");
    i.method = "same month last year".into();
    let rows = run(&i, &h).unwrap();
    let sept = rows.iter().find(|r| r.month == d(2027, 9, 1)).unwrap();
    assert_eq!(spend(sept, "Food"), n(120), "September 2025 is in the lookup");
}
