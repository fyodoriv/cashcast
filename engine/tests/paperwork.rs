//! Paperwork (ported from sidequest US-26): the presence math follows the
//! official rule (CIT 0407 examples), private values stay sealed on disk and
//! never reach an agent or a pipe. Synthetic people only: every private value
//! is a SYNTH canary.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

use cashcast_engine::paperwork::facts::{self, CITIZENSHIP_ID as P};
use cashcast_engine::paperwork::store::{self, Applies, ProcessState, Saved};
use cashcast_engine::paperwork::{cli, presence, vault};
use chrono::NaiveDate;

const CANARY: &str = "SYNTH-CANARY-Q7";

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2030, 3, 3).unwrap()
}

static INIT: Once = Once::new();
static N: AtomicUsize = AtomicUsize::new(0);

/// A fresh user folder under one temp private root (set once: tests run in threads).
fn home() -> PathBuf {
    INIT.call_once(|| {
        let root = std::env::temp_dir().join(format!("cashcast-paper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut k = [0u8; 32];
        getrandom::getrandom(&mut k).unwrap();
        let hex: String = k.iter().map(|b| format!("{b:02x}")).collect();
        // SAFETY: set once, before any test reads them.
        unsafe {
            std::env::set_var("CASHCAST_PRIVATE", &root);
            std::env::set_var(vault::TEST_KEY_ENV, hex);
        }
    });
    let root = PathBuf::from(std::env::var_os("CASHCAST_PRIVATE").unwrap());
    let d = root
        .join("users")
        .join(format!("t{}", N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn proc_() -> facts::Process {
    store::find(P).unwrap()
}

fn fresh() -> ProcessState {
    ProcessState::default()
}

fn items(v: &store::View) -> Vec<&store::ItemView> {
    v.steps.iter().flat_map(|s| s.items.iter()).collect()
}

fn item(ps: &ProcessState, id: &str) -> store::ItemView {
    let v = store::view(&proc_(), ps, today()).unwrap();
    items(&v)
        .into_iter()
        .find(|x| x.item.id == id)
        .unwrap()
        .clone()
}

struct Ran {
    code: i32,
    said: String,
    asked: Vec<String>,
}

fn run(user: &Path, argv: &[&str], terminal: bool, answers: &[&str]) -> Ran {
    let mut queue: VecDeque<String> = answers.iter().map(|s| s.to_string()).collect();
    let mut asked = vec![];
    let mut ask = |q: &str| {
        asked.push(q.to_string());
        queue.pop_front()
    };
    let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let r = cli::run_as(
        &args,
        user,
        today(),
        &mut cli::Owner {
            terminal,
            ask: &mut ask,
        },
    );
    let (said, code) = match r {
        Ok((t, c)) => (t, c),
        Err(e) => (e, 2),
    };
    Ran { code, said, asked }
}

/// The owner at a terminal, who types `answers` at the prompts.
fn owner(user: &Path, argv: &[&str], answers: &[&str]) -> Ran {
    run(user, argv, true, answers)
}

/// An agent or a pipe: no terminal.
fn agent(user: &Path, argv: &[&str]) -> Ran {
    run(user, argv, false, &[])
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p))
        } else {
            out.push(p)
        }
    }
    out
}

fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Fill a process with private values the owner's way, and a sealed document.
fn fill(user: &Path) -> store::FileRef {
    let doc = user.join("scan.txt");
    std::fs::write(&doc, format!("passport page {CANARY}-DOC")).unwrap();
    for (argv, answers) in [
        (vec!["set", P, "pr_date"], vec!["2026-01-10"]),
        (
            vec!["trip", P],
            vec!["2027-07-01", "2027-07-10", "SYNTH-CANARY-Q7-LAND"],
        ),
        (
            vec!["trip", P],
            vec!["2028-02-01", "2028-02-05", "SYNTH-CANARY-Q7-WEB"],
        ),
        (
            vec!["mark", P, "pr-status", "done", "--add-note"],
            vec!["SYNTH-CANARY-Q7-NOTE"],
        ),
    ] {
        let r = owner(user, &argv, &answers);
        assert_eq!(r.code, 0, "{}", r.said);
    }
    let r = store::update(user, P, |ps, p| {
        store::attach(user, ps, p, "passport-pages", &doc, today())
    })
    .unwrap();
    std::fs::remove_file(&doc).unwrap();
    r
}

fn ps_of(user: &Path) -> Option<ProcessState> {
    store::load(user).unwrap().processes.get(P).cloned()
}

// ── presence calculator (official rule) ──────────────────────────────────────

#[test]
fn travel_days_count_as_in_canada() {
    let t = |l: &str, r: &str| {
        store::absent_days(&store::Trip {
            left: l.into(),
            returned: r.into(),
            where_: None,
        })
    };
    assert_eq!(t("2030-07-01", "2030-07-01"), 0);
    assert_eq!(t("2030-07-01", "2030-07-02"), 0);
    assert_eq!(t("2030-07-01", "2030-07-06"), 4);
    assert_eq!(t("2030-07-01", "2030-07-10"), 8);
    assert_eq!(t("2018-12-01", "2018-12-31"), 29);
}

#[test]
fn a_date_field_refuses_a_year_no_one_could_mean() {
    let mut ps = fresh();
    for v in ["0020-02-20", "1899-12-31", "2101-01-01"] {
        assert_eq!(
            store::set_field(&mut ps, "pr_date", v).unwrap_err(),
            "pr_date must be a year from 1900 to 2100"
        );
    }
    assert!(!ps.fields.contains_key("pr_date"));
    store::set_field(&mut ps, "pr_date", "1900-01-01").unwrap();
    store::set_field(&mut ps, "pr_date", "2100-12-31").unwrap();
    assert_eq!(ps.fields["pr_date"], "2100-12-31");
}

#[test]
fn the_window_is_the_5_years_before_the_sign_day() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2010-01-01").unwrap();
    let r = store::presence(&proc_(), &ps, Some("2018-04-01"), today())
        .unwrap()
        .unwrap();
    assert_eq!(
        (r.window.from.as_str(), r.window.to.as_str()),
        ("2013-04-01", "2018-03-31")
    );
    assert_eq!(r.total, 1826); // CIT 0407 Box C example
}

#[test]
fn days_before_pr_count_as_half_days_up_to_365() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2030-01-01").unwrap();
    store::add_temp_period(&mut ps, "2029-09-01", "2029-12-31").unwrap(); // 122 days
    let r = store::presence(&proc_(), &ps, Some("2030-03-01"), today())
        .unwrap()
        .unwrap();
    assert_eq!((r.temp_days, r.temp_credit, r.pr_days), (122, 61, 59));
    store::add_temp_period(&mut ps, "2025-03-01", "2029-08-31").unwrap();
    let r = store::presence(&proc_(), &ps, Some("2030-03-01"), today())
        .unwrap()
        .unwrap();
    assert_eq!(r.temp_credit, 365);
}

#[test]
fn earliest_sign_day_moves_by_a_trips_full_days_away() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2030-01-01").unwrap();
    let e = |ps: &ProcessState| {
        store::presence(&proc_(), ps, None, today())
            .unwrap()
            .unwrap()
    };
    assert_eq!(e(&ps).earliest.as_deref(), Some("2032-12-31"));
    store::add_trip(&mut ps, "2031-06-01", "2031-06-12", "").unwrap(); // 10 full days away
    let r = e(&ps);
    assert_eq!(r.earliest.as_deref(), Some("2033-01-10"));
    assert!(!r.met);
    let v = store::view(&proc_(), &ps, today()).unwrap();
    assert_eq!(
        (v.deadlines[0].what, v.deadlines[0].date.as_str()),
        ("Earliest day to sign", "2033-01-10")
    );
    assert_eq!(item(&ps, "presence").status, "todo");
}

#[test]
fn a_stay_of_183_days_abroad_needs_a_police_certificate() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2024-01-01").unwrap();
    store::add_trip(&mut ps, "2030-03-01", "2030-03-05", "").unwrap();
    let it = item(&ps, "police-certificate");
    assert_eq!((it.applies, it.status), (store::MAYBE, "todo"));
    store::add_trip(&mut ps, "2027-01-01", "2027-08-01", "").unwrap();
    let it = item(&ps, "police-certificate");
    assert_eq!((it.applies, it.status), (Applies::Yes(true), "todo"));
}

#[test]
fn the_police_rule_counts_the_day_you_left_and_came_back() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2024-01-01").unwrap();
    let sign = NaiveDate::from_ymd_opt(2030, 3, 1).unwrap();
    store::add_trip(&mut ps, "2027-01-01", "2027-07-01", "").unwrap(); // 182 days
    assert_eq!(store::police_trips(&proc_(), &ps, sign).len(), 0);
    store::add_trip(&mut ps, "2026-04-01", "2026-09-30", "").unwrap(); // 183 days
    assert_eq!(store::police_trips(&proc_(), &ps, sign).len(), 1);
}

#[test]
fn the_police_rule_counts_only_the_4_years_since_age_18() {
    let police = |l: &str, r: &str, birth: Option<&str>| {
        let mut ps = fresh();
        store::set_field(&mut ps, "pr_date", "2024-01-01").unwrap();
        if let Some(b) = birth {
            store::set_field(&mut ps, "birth_date", b).unwrap();
        }
        store::add_trip(&mut ps, l, r, "").unwrap();
        store::police_trips(&proc_(), &ps, today()).len()
    };
    assert_eq!(police("2025-09-01", "2026-04-01", None), 0);
    assert_eq!(police("2025-08-01", "2026-09-01", None), 1);
    assert_eq!(police("2025-08-01", "2026-08-31", None), 0);
    assert_eq!(police("2026-01-02", "2026-09-01", Some("2008-06-01")), 0);
    assert_eq!(police("2026-01-02", "2026-12-01", Some("2008-06-01")), 1);
}

#[test]
fn not_needed_comes_back_when_the_rule_starts_to_apply() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2024-01-01").unwrap();
    store::mark(&mut ps, &proc_(), "police-certificate", "na", None, today()).unwrap();
    assert_eq!(item(&ps, "police-certificate").status, "na");
    store::add_trip(&mut ps, "2027-01-01", "2027-08-01", "").unwrap();
    let it = item(&ps, "police-certificate");
    assert_eq!((it.applies, it.status), (Applies::Yes(true), "todo"));
    assert_eq!(
        it.why.as_deref(),
        Some("1 trip of 183+ days in the last 4 years")
    );
}

#[test]
fn the_test_deadline_shows_only_while_the_test_is_to_do() {
    let mut ps = fresh();
    store::set_field(&mut ps, "test_invited", "2030-03-01").unwrap();
    let deadline = |ps: &ProcessState| {
        store::view(&proc_(), ps, today())
            .unwrap()
            .deadlines
            .iter()
            .any(|x| x.what == "Take the citizenship test by")
    };
    assert!(deadline(&ps));
    store::mark(&mut ps, &proc_(), "test", "done", None, today()).unwrap();
    assert!(!deadline(&ps));
    store::mark(&mut ps, &proc_(), "test", "na", None, today()).unwrap();
    assert!(!deadline(&ps));
    store::mark(&mut ps, &proc_(), "test", "todo", None, today()).unwrap();
    assert!(deadline(&ps));
    store::set_field(&mut ps, "birth_date", "1960-01-01").unwrap();
    assert!(!deadline(&ps));
}

#[test]
fn a_long_multi_country_trip_can_be_not_needed_until_a_new_long_trip() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2024-01-01").unwrap();
    store::add_trip(
        &mut ps,
        "2027-01-01",
        "2027-07-05",
        "SYNTH-LAND-A, SYNTH-LAND-B",
    )
    .unwrap();
    let it = item(&ps, "police-certificate");
    assert!(it.applies == Applies::Yes(true) && it.can_skip);
    store::mark(&mut ps, &proc_(), "police-certificate", "na", None, today()).unwrap();
    assert_eq!(item(&ps, "police-certificate").status, "na");
    store::add_trip(&mut ps, "2028-01-01", "2028-08-01", "").unwrap();
    let it = item(&ps, "police-certificate");
    assert_eq!(it.status, "todo");
    assert!(it.why.unwrap().contains("183+ days"));
    store::mark(&mut ps, &proc_(), "police-certificate", "na", None, today()).unwrap();
    assert_eq!(item(&ps, "police-certificate").status, "na");
}

#[test]
fn not_needed_sticks_only_where_it_can() {
    let mut ps = fresh();
    let p = proc_();
    let v = store::view(&p, &ps, today()).unwrap();
    let online = items(&v)
        .into_iter()
        .find(|x| x.item.when == Some("online"))
        .unwrap()
        .clone();
    assert!(!online.can_skip);
    assert!(
        store::mark(&mut ps, &p, online.item.id, "na", None, today())
            .unwrap_err()
            .contains("applies to you")
    );
    let age = items(&v)
        .into_iter()
        .find(|x| x.item.when == Some("age-18-54"))
        .unwrap()
        .item
        .id;
    let it = item(&ps, age);
    assert!(it.applies == store::MAYBE && it.can_skip);
    store::set_field(&mut ps, "birth_date", "1990-04-02").unwrap();
    let it = item(&ps, age);
    assert!(it.applies == Applies::Yes(true) && !it.can_skip);
    assert!(store::mark(&mut ps, &p, age, "na", None, today()).is_err());
    assert!(!ps.items.contains_key(age));
    assert!(item(&ps, "translations").can_skip);
    assert!(!item(&ps, "taxes").can_skip);
    assert!(store::mark(&mut ps, &p, "taxes", "na", None, today()).is_err());
    store::mark(&mut ps, &p, "translations", "na", None, today()).unwrap();
    assert_eq!(item(&ps, "translations").status, "na");
    assert!(!item(&ps, "presence").can_skip);
    assert!(item(&ps, "police-certificate").can_skip);
    store::add_trip(&mut ps, "2027-01-01", "2027-08-01", "").unwrap();
    let it = item(&ps, "police-certificate");
    assert!(it.applies == Applies::Yes(true) && it.can_skip);
}

#[test]
fn an_old_not_needed_on_a_step_everyone_needs_comes_back() {
    let mut ps = fresh();
    ps.items.insert(
        "taxes".into(),
        Saved {
            mark: "na".into(),
            note: None,
            at: "2030-01-01".into(),
            covers: vec![],
        },
    );
    let it = item(&ps, "taxes");
    assert_eq!((it.status, it.can_skip), ("todo", false));
    assert!(it.why.unwrap().contains("everyone needs"));
}

#[test]
fn bad_input_names_the_rule_not_the_value() {
    let mut ps = fresh();
    assert!(
        store::add_trip(&mut ps, "2030-07-10", "2030-07-01", "")
            .unwrap_err()
            .contains("end on or after")
    );
    assert!(
        store::set_field(&mut ps, "pr_date", "soon")
            .unwrap_err()
            .contains("YYYY-MM-DD")
    );
    assert!(
        store::set_field(&mut ps, "passport_number", "x")
            .unwrap_err()
            .contains("unknown field")
    );
    for key in ["constructor", "__proto__", "toString"] {
        assert!(
            store::set_field(&mut ps, key, "2030-01-01")
                .unwrap_err()
                .contains("unknown field")
        );
    }
    store::set_field(&mut ps, "pr_date", "2026-01-10").unwrap();
    assert!(
        store::presence(&proc_(), &ps, Some("soon"), today())
            .unwrap_err()
            .contains("YYYY-MM-DD")
    );
}

#[test]
fn the_day_count_comes_from_the_dates_only() {
    let mut ps = fresh();
    assert!(
        store::mark(&mut ps, &proc_(), "presence", "done", None, today())
            .unwrap_err()
            .contains("day count")
    );
    ps.items.insert(
        "presence".into(),
        Saved {
            mark: "done".into(),
            note: None,
            at: "2030-01-01".into(),
            covers: vec![],
        },
    );
    assert_eq!(item(&ps, "presence").status, "todo");
}

#[test]
fn one_day_short_reads_1_day() {
    let mut ps = fresh();
    store::set_field(&mut ps, "pr_date", "2027-03-05").unwrap();
    assert_eq!(item(&ps, "presence").why.as_deref(), Some("1 day short"));
}

// ── checklist ────────────────────────────────────────────────────────────────

const PAPER: [&str; 5] = [
    "fill-paper-form",
    "presence-printout",
    "mail",
    "photos",
    "checklist-cit0007",
];
const ONLINE: [&str; 4] = [
    "online-account",
    "account-calculator",
    "fill-form",
    "submit",
];

#[test]
fn only_the_steps_of_the_chosen_way_to_apply_show() {
    let mut ps = fresh();
    let ids = |ps: &ProcessState| -> Vec<String> {
        let v = store::view(&proc_(), ps, today()).unwrap();
        items(&v).iter().map(|x| x.item.id.to_string()).collect()
    };
    let online = ids(&ps);
    assert!(ONLINE.iter().all(|x| online.iter().any(|y| y == x)));
    assert!(PAPER.iter().all(|x| !online.iter().any(|y| y == x)));
    store::set_field(&mut ps, "method", "paper").unwrap();
    let paper = ids(&ps);
    assert!(PAPER.iter().all(|x| paper.iter().any(|y| y == x)));
    assert!(ONLINE.iter().all(|x| !paper.iter().any(|y| y == x)));
}

#[test]
fn progress_counts_done_and_not_needed_counts_in_neither() {
    let mut ps = fresh();
    let start = store::view(&proc_(), &ps, today()).unwrap();
    assert_eq!(
        (start.done, start.not_needed, start.open),
        (0, 0, start.total)
    );
    store::mark(&mut ps, &proc_(), "translations", "na", None, today()).unwrap();
    store::mark(&mut ps, &proc_(), "pr-status", "done", None, today()).unwrap();
    let v = store::view(&proc_(), &ps, today()).unwrap();
    assert_eq!(
        (v.done, v.not_needed, v.total, v.open),
        (1, 1, start.total - 1, start.total - 2)
    );
}

#[test]
fn trips_and_periods_are_removed_by_dates_once() {
    let u = home();
    store::update(&u, P, |ps, _| {
        store::add_trip(ps, "2027-01-01", "2027-01-05", "")?;
        store::add_trip(ps, "2027-03-01", "2027-03-05", "")?;
        Ok(())
    })
    .unwrap();
    let untrip = || {
        store::update(&u, P, |ps, _| {
            store::remove_trip_on(ps, "2027-01-01", "2027-01-05", Some(""))
        })
        .unwrap()
    };
    assert!(untrip());
    assert!(!untrip());
    assert_eq!(
        ps_of(&u)
            .unwrap()
            .trips
            .iter()
            .map(|t| t.left.as_str())
            .collect::<Vec<_>>(),
        ["2027-03-01"]
    );
    store::update(&u, P, |ps, _| {
        store::add_temp_period(ps, "2024-01-01", "2024-06-30")
    })
    .unwrap();
    assert!(
        store::update(&u, P, |ps, _| Ok(store::remove_temp_on(
            ps,
            "2024-01-01",
            "2024-06-30"
        )))
        .unwrap()
    );
    assert!(ps_of(&u).unwrap().temp_periods.is_empty());
}

#[test]
fn remove_takes_the_trip_with_the_same_place() {
    let (l, r) = ("2027-01-01", "2027-01-05");
    let mut ps = fresh();
    for w in ["SYNTH-LAND-A", "SYNTH-LAND-B"] {
        store::add_trip(&mut ps, l, r, w).unwrap();
    }
    assert!(
        store::remove_trip_on(&mut ps, l, r, None)
            .unwrap_err()
            .contains("type the country")
    );
    assert!(store::remove_trip_on(&mut ps, l, r, Some("SYNTH-LAND-B")).unwrap());
    assert_eq!(ps.trips[0].where_.as_deref(), Some("SYNTH-LAND-A"));
    assert!(store::remove_trip_on(&mut ps, l, r, None).unwrap()); // only one left
    assert!(!store::remove_trip_on(&mut ps, l, r, None).unwrap());
}

#[test]
fn the_cli_removes_the_trip_with_no_country_when_the_owner_types_dash() {
    let u = home();
    store::update(&u, P, |ps, _| {
        store::add_trip(ps, "2027-02-01", "2027-02-05", "")?;
        store::add_trip(ps, "2027-02-01", "2027-02-05", "SYNTH-LAND-A")?;
        Ok(())
    })
    .unwrap();
    let empty = owner(&u, &["untrip", P], &["2027-02-01", "2027-02-05", ""]);
    assert!(
        empty.said.contains("- for the one with none"),
        "{}",
        empty.said
    );
    let r = owner(&u, &["untrip", P], &["2027-02-01", "2027-02-05", "-"]);
    assert_eq!(r.code, 0, "{}", r.said);
    let left: Vec<_> = ps_of(&u)
        .unwrap()
        .trips
        .into_iter()
        .map(|t| t.where_)
        .collect();
    assert_eq!(left, [Some("SYNTH-LAND-A".to_string())]);
}

#[test]
fn a_mark_without_a_note_keeps_the_items_note() {
    let u = home();
    let r = owner(
        &u,
        &["mark", P, "taxes", "done", "--add-note"],
        &["SYNTH-NOTE"],
    );
    assert_eq!(r.code, 0, "{}", r.said);
    assert_eq!(owner(&u, &["mark", P, "taxes", "todo"], &[]).code, 0);
    let s = &ps_of(&u).unwrap().items["taxes"];
    assert_eq!(
        (s.mark.as_str(), s.note.as_deref()),
        ("todo", Some("SYNTH-NOTE"))
    );
}

// ── official facts ───────────────────────────────────────────────────────────

#[test]
fn every_requirement_cites_an_official_page_a_quote_and_a_day() {
    for p in facts::processes() {
        let all = p
            .steps
            .iter()
            .flat_map(|s| s.items.iter().flat_map(|i| i.sources.iter()))
            .chain(p.rules.sources.iter());
        for s in all {
            assert!(facts::official(s.url), "{}", s.url);
            assert!(s.quote.len() > 1);
            assert!(store::parse_day(s.checked).is_some());
        }
    }
    assert!(!facts::official("https://example.com/citizenship"));
    assert!(!facts::official("https://canada.ca.example.com/x"));
}

#[test]
fn the_online_steps_name_both_reasons_to_apply_on_paper() {
    let v = store::view(&proc_(), &fresh(), today()).unwrap();
    let all = items(&v);
    let quotes = |id: &str| -> Vec<&str> {
        all.iter()
            .find(|x| x.item.id == id)
            .unwrap()
            .item
            .sources
            .iter()
            .map(|s| s.quote)
            .collect()
    };
    let how = all
        .iter()
        .find(|x| x.item.id == "online-account")
        .unwrap()
        .item
        .how;
    assert!(how.find("Crown servant").unwrap() < how.find("representative").unwrap());
    assert!(quotes("online-account").contains(&facts::PAPER_REP.quote));
    assert!(quotes("representative").contains(&facts::PAPER_REP.quote));
    assert!(quotes("police-certificate").contains(&facts::POLICE_COUNTRIES.quote));
}

#[test]
fn sources_live_flags_a_page_that_no_longer_says_what_we_cite() {
    let p = proc_();
    let fetch = |url: &str| {
        Ok((
            200,
            if url.contains("fees.asp") {
                "<p>Adult 700.00</p>".to_string()
            } else {
                "every quote is here".to_string()
            },
        ))
    };
    let checks = store::check_sources(&p, true, today(), &fetch);
    let fee = checks.iter().find(|c| c.url.contains("fees.asp")).unwrap();
    assert_eq!(fee.status, "changed");
    assert!(fee.missing.iter().any(|m| m == "653.00"));
    let never = |_: &str| -> Result<(u16, String), String> { panic!("offline must not fetch") };
    let offline = store::check_sources(&p, false, today(), &never);
    assert!(offline.iter().all(|c| c.status == "due")); // today is 2030 in tests
}

#[test]
fn sources_live_lists_a_pdf_to_check_by_hand() {
    let fetch = |url: &str| {
        Ok((
            200,
            if url.ends_with(".pdf") {
                String::new()
            } else {
                "a page".to_string()
            },
        ))
    };
    let checks = store::check_sources(&proc_(), true, today(), &fetch);
    let pdf: Vec<_> = checks.iter().filter(|c| c.url.ends_with(".pdf")).collect();
    assert!(!pdf.is_empty());
    for c in pdf {
        assert_eq!(c.status, "manual");
        assert!(!c.missing.is_empty());
        assert!(c.detail.contains(&c.missing[0]));
    }
    let down = |_: &str| Err("no route".to_string());
    assert!(
        store::check_sources(&proc_(), true, today(), &down)
            .iter()
            .all(|c| c.status == "unreachable")
    );
}

#[test]
fn page_text_drops_scripts_tags_and_entities() {
    let t = store::page_text(
        "<script>x 653.00</script><p>at&nbsp;least <b>1,095</b> days &amp; more</p>",
    );
    assert_eq!(t.trim(), "at least 1,095 days & more");
}

// ── privacy ──────────────────────────────────────────────────────────────────

#[test]
fn a_terminal_with_no_owner_present_gets_exit_2_and_no_private_value() {
    let u = home();
    let r = fill(&u);
    presence::replace(Some(Box::new(|_| false)));
    let back = u.join("back.txt");
    let back_s = back.to_string_lossy().to_string();
    let scan = u.join("scan.txt").to_string_lossy().to_string();
    let cases: Vec<(Vec<&str>, Vec<&str>)> = vec![
        (vec!["show", P, "--reveal"], vec![]),
        (vec!["files", P, "--reveal"], vec![]),
        (vec!["presence", P, "--reveal"], vec![]),
        (vec!["export-file", P, &r.id, "--out", &back_s], vec![]),
        (vec!["attach", P, "passport-pages", &scan], vec![]),
        (vec!["set", P, "pr_date"], vec!["2031-01-01"]),
        (vec!["trip", P], vec!["2031-02-01", "2031-02-02", "X"]),
        (
            vec!["mark", P, "taxes", "done", "--add-note"],
            vec!["SYNTH-NEW-NOTE"],
        ),
    ];
    for (argv, answers) in &cases {
        let ran = owner(&u, argv, answers);
        assert_eq!(ran.code, 2, "{argv:?}");
        assert!(
            !ran.said.contains("SYNTH")
                && !ran.said.contains("2026-01-10")
                && !ran.said.contains("scan.txt")
        );
        assert!(ran.said.contains("Touch ID"), "{}", ran.said);
        assert!(ran.asked.is_empty(), "asked before the owner was confirmed");
    }
    assert!(!back.exists());
    let ps = ps_of(&u).unwrap();
    assert_eq!(ps.fields["pr_date"], "2026-01-10");
    assert_eq!((ps.trips.len(), ps.files.len()), (2, 1));
    assert_eq!(owner(&u, &["show", P], &[]).code, 0); // the redacted view asks nothing
    presence::replace(Some(Box::new(|_| true)));
    let yes = owner(&u, &["show", P, "--reveal"], &[]);
    presence::replace(None);
    assert_eq!(yes.code, 0);
    assert!(yes.said.contains("SYNTH-CANARY-Q7-NOTE"));
}

#[test]
fn nothing_plaintext_on_disk_and_modes_are_tight() {
    let u = home();
    let r = fill(&u);
    let root = u.join("paperwork");
    let files = walk(&root);
    assert!(files.len() >= 2);
    for f in &files {
        let b = std::fs::read(f).unwrap();
        assert!(!b.windows(12).any(|w| w == b"SYNTH-CANARY"), "{f:?}");
        assert!(!b.windows(10).any(|w| w == b"2027-07-01"), "{f:?}");
        assert_eq!(mode(f), 0o600, "{f:?}");
    }
    assert_eq!(mode(&root), 0o700);
    assert_eq!(mode(&root.join("files")), 0o700);
    let ps = ps_of(&u).unwrap();
    assert_eq!(
        ps.items["pr-status"].note.as_deref(),
        Some("SYNTH-CANARY-Q7-NOTE")
    );
    let out = u.join("back.txt");
    store::export_file(&u, &ps, &r.id, &out).unwrap();
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("SYNTH-CANARY-Q7-DOC")
    );
    assert_eq!(mode(&out), 0o600);
    assert!(
        store::export_file(&u, &ps, &r.id, &out)
            .unwrap_err()
            .contains("exists")
    );
    let fails: Vec<_> = store::doctor(&u)
        .into_iter()
        .filter(|(l, _)| *l == "FAIL")
        .collect();
    assert!(fails.is_empty(), "{fails:?}");
}

#[test]
fn a_tampered_or_swapped_file_is_refused() {
    let u = home();
    let r = fill(&u);
    let state = u.join("paperwork/state.sqv");
    let file = u.join(format!("paperwork/files/{}.sqv", r.id));
    let good = std::fs::read(&state).unwrap();
    let mut bad = good.clone();
    *bad.last_mut().unwrap() ^= 1;
    std::fs::write(&state, &bad).unwrap();
    assert!(store::load(&u).unwrap_err().contains("changed"));
    std::fs::write(&state, std::fs::read(&file).unwrap()).unwrap(); // a document as the state
    assert!(store::load(&u).unwrap_err().contains("changed"));
    std::fs::write(&state, &good).unwrap();
    assert_eq!(ps_of(&u).unwrap().trips.len(), 2);
}

#[test]
fn the_agent_view_is_redacted() {
    let u = home();
    fill(&u);
    let ps = ps_of(&u).unwrap();
    let v = store::view(&proc_(), &ps, today()).unwrap();
    let safe = serde_json::to_string(&store::agent_view(&v)).unwrap();
    assert!(!safe.contains("SYNTH") && !safe.contains("scan.txt"));
    let date = regex::Regex::new(r"\d{4}-\d\d-\d\d").unwrap();
    assert!(!date.is_match(&safe.replace(&v.facts_checked, "")));
    let mut said = String::new();
    for argv in [
        vec!["show", P, "--json"],
        vec!["show", P],
        vec!["presence", P],
        vec!["files", P],
        vec!["list"],
    ] {
        let r = agent(&u, &argv);
        assert_eq!(r.code, 0, "{argv:?}: {}", r.said);
        said.push_str(&r.said);
    }
    assert!(!said.contains("SYNTH") && !said.contains("scan.txt"));
    assert_eq!(agent(&u, &["show", P, "--reveal"]).code, 2);
}

#[test]
fn the_agent_view_gives_away_no_date() {
    let mut seen = std::collections::BTreeSet::new();
    for pr in ["2028-06-15", "2029-01-20"] {
        let u = home();
        store::update(&u, P, |ps, _| store::set_field(ps, "pr_date", pr)).unwrap();
        let mut all = String::new();
        for (argv, code) in [
            (vec!["presence", P], 1),
            (vec!["presence", P, "--json"], 1),
            (vec!["show", P, "--json"], 0),
        ] {
            let r = agent(&u, &argv);
            assert_eq!(r.code, code, "{argv:?}");
            all.push_str(&r.said);
        }
        seen.insert(all);
    }
    assert_eq!(seen.len(), 1);
}

#[test]
fn agents_and_pipes_cannot_change_dates_try_sign_days_or_move_documents() {
    let u = home();
    let doc = u.join("scan.txt");
    std::fs::write(&doc, format!("passport page {CANARY}-DOC")).unwrap();
    let out = u.join("out.txt");
    let (doc_s, out_s) = (
        doc.to_string_lossy().to_string(),
        out.to_string_lossy().to_string(),
    );
    let mut said = String::new();
    for argv in [
        vec!["set", P, "sign_date"],
        vec!["set", P, "sign_date", "2031-01-01"],
        vec!["trip", P],
        vec!["untrip", P],
        vec!["temp", P],
        vec!["untemp", P],
        vec!["presence", P, "--sign"],
        vec!["mark", P, "taxes", "done", "--add-note"],
        vec!["attach", P, "passport-pages", &doc_s],
        vec!["export-file", P, "0123456789abcdef", "--out", &out_s],
    ] {
        let r = agent(&u, &argv);
        assert_eq!(r.code, 2, "{argv:?}");
        said.push_str(&r.said);
    }
    assert!(said.contains("only you"));
    assert!(!out.exists());
    assert!(ps_of(&u).is_none()); // nothing was saved
}

#[test]
fn private_values_are_typed_at_a_prompt_never_after_the_command() {
    let u = home();
    for argv in [
        vec!["set", P, "pr_date", "2028-01-10"],
        vec!["trip", P, "2029-01-01", "2029-01-05"],
        vec!["untrip", P, "2029-01-01", "2029-01-05"],
        vec!["temp", P, "2025-01-01", "2025-02-01"],
        vec!["untemp", P, "2025-01-01", "2025-02-01"],
        vec!["presence", P, "--sign", "2031-01-01"],
    ] {
        let r = owner(&u, &argv, &[]);
        assert_eq!(r.code, 2, "{argv:?}");
        assert!(r.said.contains("shell history"), "{}", r.said);
    }
    assert!(ps_of(&u).is_none());
    assert_eq!(owner(&u, &["set", P, "pr_date"], &["2028-01-10"]).code, 0);
    let trip = owner(&u, &["trip", P], &["2029-07-01", "2029-07-10", CANARY]);
    assert_eq!(trip.code, 0);
    assert!(trip.asked[0].contains("Day you left"));
    assert!(!trip.said.contains(CANARY)); // never echoed back
    assert_eq!(
        owner(
            &u,
            &["mark", P, "taxes", "done", "--add-note"],
            &["SYNTH-CANARY-Q7-NOTE"]
        )
        .code,
        0
    );
    let ps = ps_of(&u).unwrap();
    assert_eq!(ps.fields["pr_date"], "2028-01-10");
    assert_eq!(ps.trips[0].where_.as_deref(), Some(CANARY));
    assert_eq!(
        ps.items["taxes"].note.as_deref(),
        Some("SYNTH-CANARY-Q7-NOTE")
    );
    let sign = owner(&u, &["presence", P, "--sign", "--reveal"], &["2033-02-01"]);
    assert!(sign.said.contains("sign day 2033-02-01"), "{}", sign.said);
    let bad = owner(&u, &["set", P, "birth_date"], &["soon"]);
    assert_eq!(bad.code, 2);
    assert!(bad.said.contains("YYYY-MM-DD") && !bad.said.contains("soon"));
}

#[test]
fn presence_exit_code_says_met_or_not() {
    let u = home();
    assert_eq!(agent(&u, &["presence", P]).code, 1); // no PR date yet
    store::update(&u, P, |ps, _| store::set_field(ps, "pr_date", "2020-01-01")).unwrap();
    let r = agent(&u, &["presence", P]);
    assert_eq!(r.code, 0);
    assert!(r.said.contains("enough days"));
}

#[test]
fn steps_and_sources_need_no_state() {
    let u = home();
    let s = agent(&u, &["steps"]);
    assert_eq!(s.code, 0);
    assert!(s.said.contains("police-certificate"));
    let o = agent(&u, &["sources"]);
    assert_eq!(o.code, 0);
    assert!(o.said.contains("DUE"));
    assert!(!u.join("paperwork").exists());
    assert_eq!(agent(&u, &["nope"]).code, 2);
}

// ── the save lock ────────────────────────────────────────────────────────────

#[test]
fn saves_at_the_same_time_keep_every_change() {
    let u = home();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let u = u.clone();
            std::thread::spawn(move || {
                for _ in 0..25 {
                    store::update(&u, P, |ps, _| {
                        store::add_trip(ps, "2027-01-01", "2027-01-02", "")
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(ps_of(&u).unwrap().trips.len(), 100);
    assert!(!u.join("paperwork").join(store::LOCK).exists());
}

#[test]
fn a_save_removes_only_its_own_lock() {
    let u = home();
    let lock = u.join("paperwork").join(store::LOCK);
    store::update(&u, P, |_, _| {
        std::fs::write(&lock, "999999").map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), "999999");
}

#[test]
fn a_lock_left_by_a_gone_writer_is_broken() {
    let u = home();
    let root = store::dir(&u).unwrap();
    // A pid that no process holds (above the macOS and Linux pid limits).
    std::fs::write(root.join(store::LOCK), "99999999").unwrap();
    let t = std::time::Instant::now();
    store::update(&u, P, |ps, _| {
        store::add_trip(ps, "2027-01-01", "2027-01-02", "")
    })
    .unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    assert!(!root.join(store::LOCK).exists());
}

#[test]
fn a_live_writers_lock_is_waited_for() {
    let u = home();
    let root = store::dir(&u).unwrap();
    // This process is alive: the lock holds, and the save gives up after 10 s.
    std::fs::write(root.join(store::LOCK), format!("{} x", std::process::id())).unwrap();
    let e = store::update(&u, P, |_, _| Ok(())).unwrap_err();
    assert!(e.contains("still running"));
    assert!(root.join(store::LOCK).exists());
}

// ── doctor ───────────────────────────────────────────────────────────────────

fn fails(u: &Path) -> String {
    store::doctor(u)
        .into_iter()
        .filter(|(l, _)| *l == "FAIL")
        .map(|(_, m)| m)
        .collect::<Vec<_>>()
        .join("\n")
}

fn chmod(p: &Path, m: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

#[test]
fn doctor_finds_folders_that_lost_their_mode() {
    let u = home();
    fill(&u);
    let root = u.join("paperwork");
    chmod(&root, 0o755);
    chmod(&root.join("files"), 0o755);
    let f = fails(&u);
    assert!(f.contains(&format!("{} is mode 755, want 700", root.display())));
    assert!(f.contains(&format!(
        "{} is mode 755, want 700",
        root.join("files").display()
    )));
}

#[test]
fn doctor_warns_on_a_sealed_leftover_and_fails_on_a_plain_file() {
    let u = home();
    fill(&u);
    let root = u.join("paperwork");
    let leftover = root.join("state.sqv.ab12cd34.tmp");
    std::fs::write(&leftover, [b"SQV1".as_slice(), &[0u8; 40]].concat()).unwrap();
    chmod(&leftover, 0o600);
    let found = store::doctor(&u);
    assert!(found.iter().any(|(l, m)| *l == "WARN"
        && m.contains("state.sqv.ab12cd34.tmp")
        && m.contains("delete")));
    assert!(fails(&u).is_empty());
    let plain = root.join("notes.txt");
    std::fs::write(&plain, "SYNTH plain").unwrap();
    chmod(&plain, 0o600);
    assert!(fails(&u).contains("plain file"));
}

#[test]
fn doctor_finds_a_plain_file_or_an_open_folder_at_any_depth() {
    let u = home();
    let root = store::dir(&u).unwrap();
    let old = root.join("files").join("old");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("scan.txt"), "SYNTH").unwrap();
    let f = fails(&u);
    assert!(f.contains("unexpected folder"));
    assert!(f.contains("plain file"));
    assert!(f.contains("want 600"));
}

#[test]
fn doctor_fails_on_links_and_hard_links() {
    let u = home();
    fill(&u);
    let root = u.join("paperwork");
    let outside = u.join("outside.sqv");
    std::fs::write(&outside, "x").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.sqv")).unwrap();
    assert!(fails(&u).contains("a link in the paperwork folder"));
    std::fs::remove_file(root.join("link.sqv")).unwrap();
    std::fs::hard_link(root.join("state.sqv"), u.join("copy.sqv")).unwrap();
    assert!(fails(&u).contains("has 2 names"));
}

#[test]
fn doctor_fails_when_the_paperwork_folder_is_a_link() {
    let u = home();
    let elsewhere = u.join("synced");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, u.join("paperwork")).unwrap();
    assert!(fails(&u).contains("the paperwork folder is a link"));
}

#[test]
fn doctor_warns_about_an_old_save_lock() {
    let u = home();
    let root = store::dir(&u).unwrap();
    let lock = root.join(store::LOCK);
    std::fs::write(&lock, "1").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let found = store::doctor(&u);
    assert!(
        found
            .iter()
            .any(|(l, m)| *l == "WARN" && m.contains("10 minutes old")),
        "{found:?}"
    );
}

#[test]
fn doctor_on_a_new_user_makes_nothing() {
    let u = home();
    let found = store::doctor(&u);
    assert!(found.iter().all(|(l, _)| *l == "ok"), "{found:?}");
    assert!(found.iter().any(|(_, m)| m.contains("test key")));
    assert!(!u.join("paperwork").exists());
}

/// canada.ca resets any request with a custom User-Agent (2026-11-22); curl's
/// own default gets the page. The live check must send none of its own.
#[test]
fn live_check_sends_curls_own_user_agent() {
    let h = store::page_headers();
    assert!(
        !h.iter().any(|(k, _)| k.eq_ignore_ascii_case("user-agent")),
        "custom User-Agent sent: {h:?}"
    );
}
