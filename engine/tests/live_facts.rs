//! `knowledge refresh`: live Bank of Canada facts, parsed safely and offline.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

/// Tests that set CASHCAST_PRIVATE hold this: the environment is process-wide.
static ENV: Mutex<()> = Mutex::new(());

use cashcast_engine::core::http::{Response, set_transport};
use cashcast_engine::live_facts::{self, Kind, SERIES, parse, render, url};

fn series(id: &str) -> &'static live_facts::Series {
    SERIES.iter().find(|s| s.id == id).unwrap()
}

fn reply(id: &str, rows: &[(&str, &str)]) -> String {
    let obs: Vec<String> = rows
        .iter()
        .map(|(d, v)| format!("{{\"d\": \"{d}\", \"{id}\": {{\"v\": \"{v}\"}}}}"))
        .collect();
    format!("{{\"observations\": [{}]}}", obs.join(", "))
}

#[test]
fn urls_hold_only_fixed_series_ids() {
    for s in SERIES {
        let u = url(s);
        assert!(u.starts_with("https://www.bankofcanada.ca/valet/observations/"));
        assert!(!u.contains("private") && !u.contains('@'));
        let recent = if s.kind == Kind::YearOverYear {
            "13"
        } else {
            "1"
        };
        assert!(u.ends_with(&format!("/json?recent={recent}")), "{u}");
    }
}

#[test]
fn percent_becomes_a_fraction() {
    let f = parse(
        series("V39079"),
        &reply("V39079", &[("2026-11-05", "2.50")]),
    );
    assert_eq!(f.value, Some(0.025));
    assert_eq!(f.date, "2026-11-05");
}

#[test]
fn cpi_is_a_twelve_month_change() {
    let body = reply(
        "V41690973",
        &[
            ("2025-09-01", "160.0"),
            ("2026-08-01", "167.0"),
            ("2026-09-01", "164.8"),
        ],
    );
    let f = parse(series("V41690973"), &body);
    let v = f.value.unwrap();
    assert!((v - 0.03).abs() < 1e-9, "{v}");
}

#[test]
fn bad_or_absurd_replies_never_become_facts() {
    let s = series("V39079");
    assert_eq!(parse(s, "not json CANARY_LEAK").value, None);
    assert_eq!(parse(s, "{}").value, None);
    let absurd = parse(s, &reply("V39079", &[("2026-11-05", "95")]));
    assert_eq!(absurd.value, None);
    assert!(absurd.note.contains("sane range"));
    let cpi = parse(
        series("V41690973"),
        &reply("V41690973", &[("2026-09-01", "170")]),
    );
    assert_eq!(cpi.value, None);
}

#[test]
fn render_flags_a_gap_against_the_hand_fact() {
    let evidence: toml::Table =
        toml::from_str("[[facts]]\nid = \"pag-inflation\"\nvalue = 0.021\nas_of = 2026-04-16\n")
            .unwrap();
    let body = reply(
        "V41690973",
        &[("2025-09-01", "100.0"), ("2026-09-01", "105.0")],
    );
    let f = parse(series("V41690973"), &body);
    let (text, flags) = render(&[f], &evidence);
    assert_eq!(flags, 1, "{text}");
    assert!(text.contains("| review |"), "{text}");
    assert!(text.contains("2.10% (as of 2026-04-16)"), "{text}");
    assert!(text.contains("Refreshed: 1 of 1"));
}

#[test]
fn refresh_sends_only_get_requests_with_no_body() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = seen.clone();
    set_transport(Some(Box::new(move |method, u, _h, body| {
        assert_eq!(method, "GET");
        assert!(body.is_none());
        log.borrow_mut().push(u.to_string());
        let id = u
            .split("/observations/")
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        Ok(Response {
            status: 200,
            body: reply(id, &[("2025-11-01", "2.0"), ("2026-11-01", "2.0")]).into_bytes(),
        })
    })));
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
    let facts = live_facts::refresh(today).unwrap();
    assert_eq!(seen.borrow().len(), SERIES.len());
    assert!(facts.iter().filter(|f| f.value.is_some()).count() >= 3);
    // Same day again: served from the cache, no new request.
    live_facts::refresh(today).unwrap();
    assert_eq!(seen.borrow().len(), SERIES.len());
    set_transport(None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn offline_stops_after_one_request_and_changes_nothing() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-off-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    let calls: Rc<RefCell<usize>> = Rc::default();
    let n = calls.clone();
    set_transport(Some(Box::new(move |_m, _u, _h, _b| {
        *n.borrow_mut() += 1;
        Err(cashcast_engine::core::http::api_error("network down"))
    })));
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
    assert!(live_facts::refresh(today).is_err());
    assert_eq!(*calls.borrow(), 1);
    set_transport(None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edition_needle_is_next_year() {
    let e = &live_facts::EDITIONS[0];
    assert_eq!(
        live_facts::next_needle(e, "2026-04-16").as_deref(),
        Some("2027 Projection Assumption Guidelines")
    );
    assert_eq!(live_facts::next_needle(e, "bad"), None);
}

#[test]
fn live_facts_become_cited_plan_facts_and_stale_ones_drop() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-as-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    std::fs::create_dir_all(dir.join("live")).unwrap();
    std::fs::write(
        dir.join("live/latest.json"),
        "[{\"fact\": \"gic-1y\", \"series\": \"V80691339\", \"value\": 0.0245, \"date\": \"2026-11-04\"}, {\"fact\": \"cpi-yoy\", \"series\": \"V41690973\", \"value\": 0.03, \"date\": \"2026-01-01\"}]",
    )
    .unwrap();
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
    let facts = live_facts::as_facts(today);
    assert_eq!(facts.len(), 1, "the 10-month-old CPI is dropped");
    assert_eq!(facts[0].0, "live-gic-1y");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn page_text_is_normalized_before_matching() {
    let html = "<h2>2027&nbsp;Projection <em>Assumption</em>\n Guidelines</h2>";
    assert!(live_facts::plain(html).contains("2027 Projection Assumption Guidelines"));
}

#[test]
fn implausible_live_rates_and_stale_daily_series_never_become_facts() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-band-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    std::fs::create_dir_all(dir.join("live")).unwrap();
    // A 15% GIC is outside the plausible band; a 30-day-old policy rate is stale.
    std::fs::write(
        dir.join("live/latest.json"),
        "[{\"fact\": \"gic-1y\", \"value\": 0.15, \"date\": \"2026-11-04\"}, {\"fact\": \"boc-policy-rate\", \"value\": 0.025, \"date\": \"2026-10-09\"}, {\"fact\": \"cpi-yoy\", \"value\": 0.03, \"date\": \"2026-09-30\"}]",
    )
    .unwrap();
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
    let ids: Vec<String> = live_facts::as_facts(today)
        .into_iter()
        .map(|f| f.0)
        .collect();
    assert_eq!(
        ids,
        vec!["live-cpi-yoy"],
        "monthly CPI at 39 days is still fresh"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_series_keeps_its_last_good_value() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-keep-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    let fact = |v: Option<f64>, d: &str| live_facts::LiveFact {
        fact: "gic-1y".into(),
        label: "x".into(),
        series: "V80691339".into(),
        value: v,
        date: d.into(),
        note: "HTTP 503".into(),
    };
    live_facts::save(&[fact(Some(0.0245), "2026-11-04")]).unwrap();
    live_facts::save(&[fact(None, "")]).unwrap();
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
    let ids: Vec<String> = live_facts::as_facts(today)
        .into_iter()
        .map(|f| f.0)
        .collect();
    assert_eq!(
        ids,
        vec!["live-gic-1y"],
        "one failed reply must not erase the value"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn offline_auto_tries_once_a_day() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-once-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    unsafe { std::env::remove_var("CASHCAST_LIVE") };
    let calls: Rc<RefCell<usize>> = Rc::default();
    let c = calls.clone();
    set_transport(Some(Box::new(move |_m, _u, _h, _b| {
        *c.borrow_mut() += 1;
        Err(cashcast_engine::core::http::api_error("network down"))
    })));
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
    let first = live_facts::auto(today);
    let second = live_facts::auto(today);
    unsafe { std::env::set_var("CASHCAST_LIVE", "off") };
    set_transport(None);
    assert!(first.is_some(), "auto runs by default");
    {
        assert_eq!(*calls.borrow(), 1, "the second run that day sends nothing");
        assert!(second.unwrap().contains("offline earlier"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn usd(home: &std::path::Path, today: &str) -> (f64, String) {
    use cashcast_engine::strategy::situation::Situation;
    let day = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").unwrap();
    let rates = Situation::load(home, "t", day).unwrap().fx_rates().unwrap();
    let (_, r) = rates.iter().find(|(k, _)| k == "USD").unwrap();
    let v = match r.get("cad_per_unit") {
        Some(cashcast_engine::core::pyjson::Json::Float(v)) => *v,
        other => panic!("cad_per_unit: {other:?}"),
    };
    (v, r.get("date").map(|d| d.py_str()).unwrap_or_default())
}

#[test]
fn a_fresh_live_usd_rate_converts_foreign_cash_unless_the_user_pinned_one() {
    let dir = std::env::temp_dir().join(format!("cashcast-live-fx-{}", std::process::id()));
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("CASHCAST_PRIVATE", &dir) };
    unsafe { std::env::set_var("CASHCAST_ROOT", concat!(env!("CARGO_MANIFEST_DIR"), "/..")) };
    let home = dir.join("users/t");
    std::fs::create_dir_all(dir.join("live")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("profile.toml"), "[goals]\n").unwrap();
    let live = |value: &str, date: &str| {
        std::fs::write(
            dir.join("live/latest.json"),
            format!("[{{\"fact\": \"usd-cad\", \"series\": \"FXUSDCAD\", \"value\": {value}, \"date\": \"{date}\"}}]"),
        )
        .unwrap();
    };
    let (shared, shared_date) = {
        let _ = std::fs::remove_file(dir.join("live/latest.json"));
        usd(&home, "2026-11-14")
    };

    live("1.2345", "2099-01-02");
    assert_eq!(
        usd(&home, "2099-01-05"),
        (1.2345, "2099-01-02".into()),
        "fresh live wins"
    );
    assert_eq!(
        usd(&home, "2099-02-05"),
        (shared, shared_date.clone()),
        "stale live drops"
    );

    live("1.2345", "2000-01-02");
    assert_eq!(
        usd(&home, "2000-01-05").1,
        shared_date,
        "live older than the shared rate loses"
    );

    live("1.2345", "2099-01-02");
    std::fs::create_dir_all(home.join("knowledge")).unwrap();
    std::fs::write(
        home.join("knowledge/foreign_cash.toml"),
        "[[rates]]\nid = \"USD\"\ncad_per_unit = 1.1111\ndate = 2098-06-01\n",
    )
    .unwrap();
    assert_eq!(
        usd(&home, "2099-01-05"),
        (1.1111, "2098-06-01".into()),
        "a pinned rate wins"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pag_equity_matches_the_2026_guidelines_by_region() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../knowledge/data/invest_evidence.toml"
    );
    let t: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let f = t["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"].as_str() == Some("pag-equity"))
        .unwrap();
    let text = f["text"].as_str().unwrap();
    // FP Canada PAG 2026, "Return rates (nominal)".
    for part in [
        "Canadian stocks 6.3%",
        "US stocks 6.4%",
        "international developed stocks 6.6%",
        "emerging-market stocks 7.5%",
    ] {
        assert!(text.contains(part), "{part} missing from: {text}");
    }
    // The value is a global all-equity mix, so it sits inside the regional range.
    let v = f["value"].as_float().unwrap();
    assert!((0.063..=0.075).contains(&v), "{v}");
    assert_eq!(v, 0.065);
    // The net total's expected return reads the same guideline.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../knowledge/data/investing.toml"
    );
    let t: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    assert_eq!(t["expected_returns"]["equity"].as_float(), Some(v));
}
