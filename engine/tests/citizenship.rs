//! Citizenship application (ported from sidequest): the answers judge catches each seeded
//! defect in a synthetic person's answers, the collectors read synthetic sources, the
//! draft is sealed, and no canary (fake UCI, SIN, passport number, name) is ever printed.
use cashcast_engine::paperwork::citizenship::{answers as A, collect as C, draft as D, judge};
use chrono::NaiveDate;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Once;

const TODAY: &str = "2026-11-07";
static ENV: Once = Once::new();

/// One test home and test key for the whole binary (set once: no env races).
fn env() -> PathBuf {
    let home = std::env::temp_dir().join(format!("cashcast-cz-{}", std::process::id()));
    ENV.call_once(|| {
        std::fs::create_dir_all(&home).unwrap();
        // SAFETY: set once, before any test reads them.
        unsafe {
            std::env::set_var("CASHCAST_PRIVATE", &home);
            std::env::set_var("CASHCAST_TEST_VAULT_KEY", "ab".repeat(32));
        }
    });
    home
}
fn user(tag: &str) -> PathBuf {
    let d = env().join("users").join(tag);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}
fn run(u: &Path, args: &[&str]) -> Result<(String, i32), String> {
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    judge::run(&a, u, day(TODAY))
}

const UCI: &str = "0000000042";
const SIN: &str = "046454286";
const PASSNO: &str = "X0CANARY";

fn base() -> A::Answers {
    let v = json!({
        "method": "online", "service_language": "English", "accommodation": "none",
        "uci": UCI, "name": {"family": "Canaryson", "given": "Alex"}, "other_names": [],
        "name_changed_since_pr": false, "marital_status": "single",
        "birth_date": "1990-04-02", "pr_date": "2024-01-10", "sign_date": "2030-06-01",
        "contact": {"address": "1 Example Street, Apt 2", "postal_code": "A1A 1A1",
                    "phone": "+1 555 010 0000", "email": "alex@example.com"},
        "temp_periods": [],
        "trips": [
            {"left": "2026-07-01", "returned": "2026-07-20", "country": "Freedonia"},
            {"left": "2028-12-20", "returned": "2029-01-05", "country": "Sylvania"}],
        "addresses": [
            {"from": "2027-05-01", "to": "present", "address": "1 Example Street, Apt 2", "city": "Toronto", "country": "Canada", "postal_code": "A1A 1A1"},
            {"from": "2023-09-01", "to": "2027-04-30", "address": "9 Sample Avenue", "city": "Toronto", "country": "Canada", "postal_code": "A1A 1B1"}],
        "activities": [{"from": "2023-06-01", "to": "present", "what": "software developer", "organization": "Example Corp", "city": "Toronto", "country": "Canada"}],
        "taxes": {"sin": SIN, "years": ([2025, 2026, 2027, 2028, 2029].iter().map(|y| json!({"year": y, "required": true, "filed": true})).collect::<Vec<_>>()), "cra_consent": true},
        "other_status": [{"country": "Freedonia", "status": "citizen"}],
        "passports": [{"number": PASSNO, "country": "Freedonia", "issued": "2022-01-01", "expires": "2032-01-01"}],
        "ids": ["pr-card", "drivers-licence"], "language_proof": "celpip",
        "prohibitions_none": true, "representative": false, "police_certificates": [],
        "certificate": "electronic", "consent_mp": false, "consent_electors": true, "city": "Toronto"
    });
    v.as_object().unwrap().clone()
}
fn bad(a: &A::Answers) -> Vec<String> {
    A::judge_answers(a, TODAY)
        .into_iter()
        .filter(|f| f.level != "OK")
        .map(|f| format!("{} {}: {}", f.level, f.id, f.msg))
        .collect()
}
fn ids(a: &A::Answers) -> Vec<String> {
    bad(a)
        .iter()
        .map(|s| s.split(':').next().unwrap().to_string())
        .collect()
}
fn at<'a>(a: &'a mut A::Answers, k: &str) -> &'a mut Value {
    a.get_mut(k).unwrap()
}

#[test]
fn passes_the_synthetic_example() {
    assert_eq!(bad(&base()), Vec::<String>::new());
}

#[test]
fn catches_each_seeded_defect() {
    type M = fn(&mut A::Answers);
    let cases: &[(M, &str)] = &[
        (
            |a| at(a, "addresses")[1]["to"] = json!("2027-03-31"),
            "FAIL addresses",
        ),
        (
            |a| at(a, "activities")[0]["from"] = json!("2026-01-01"),
            "FAIL activities",
        ),
        (
            |a| {
                at(a, "trips").as_array_mut().unwrap().push(
                    json!({"left": "2025-07-01", "returned": "2027-12-01", "country": "Sylvania"}),
                );
                a.insert(
                    "police_certificates".into(),
                    json!([{"country": "Sylvania", "issued": "2028-01-15"}]),
                );
            },
            "FAIL presence",
        ),
        (
            |a| {
                a.insert("sign_date".into(), json!("2026-12-01"));
            },
            "FAIL presence",
        ),
        (
            |a| {
                at(a, "trips").as_array_mut().unwrap().push(
                    json!({"left": "2027-01-01", "returned": "2027-08-01", "country": "Freedonia"}),
                )
            },
            "FAIL police",
        ),
        (
            |a| {
                a.insert("uci".into(), json!("12345"));
            },
            "FAIL uci",
        ),
        (
            |a| {
                a.insert("language_proof".into(), json!("none"));
            },
            "FAIL language_proof",
        ),
        (
            |a| {
                for y in at(a, "taxes")["years"].as_array_mut().unwrap() {
                    let yr = y["year"].as_i64().unwrap();
                    y["filed"] = json!(yr >= 2028);
                }
            },
            "FAIL taxes",
        ),
        (
            |a| at(a, "passports")[0]["issued"] = json!("2027-01-01"),
            "WARN passports",
        ),
        (
            |a| {
                a.insert("ids".into(), json!(["pr-card", "foreign-passport"]));
            },
            "FAIL ids",
        ),
    ];
    for (m, want) in cases {
        let mut a = base();
        m(&mut a);
        assert!(ids(&a).contains(&want.to_string()), "{want}: {:?}", bad(&a));
    }
}

#[test]
fn early_sign_day_names_the_earliest_and_past_fails() {
    let mut a = base();
    a.insert("sign_date".into(), json!("2026-12-01"));
    let m = bad(&a)
        .into_iter()
        .find(|f| f.starts_with("FAIL presence"))
        .unwrap();
    assert!(
        regex::Regex::new(r"earliest sign day \d{4}-\d{2}-\d{2}")
            .unwrap()
            .is_match(&m)
    );
    let f = A::judge_answers(&base(), "2031-01-01");
    assert!(f.iter().any(|x| x.id == "sign_date" && x.level == "FAIL"));
}

#[test]
fn missing_answers_become_questions() {
    let mut a = base();
    a.remove("uci");
    a.remove("trips");
    let asks: Vec<_> = A::judge_answers(&a, TODAY)
        .into_iter()
        .filter(|x| x.level == "ASK")
        .collect();
    assert_eq!(
        asks.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
        ["uci", "trips"]
    );
    assert!(asks[0].msg.contains("PR card"));
    let all = A::judge_answers(&Default::default(), TODAY);
    assert_eq!(
        all.iter().filter(|x| x.level == "ASK").count(),
        A::questions().len()
    );
}

#[test]
fn older_than_55_needs_no_language_and_yes_text_counts() {
    let mut a = base();
    a.insert("birth_date".into(), json!("1970-01-01"));
    a.insert("language_proof".into(), json!("none"));
    a.insert("consent_mp".into(), json!("no"));
    at(&mut a, "taxes")["cra_consent"] = json!("yes");
    assert_eq!(bad(&a), Vec::<String>::new());
    a.insert("consent_mp".into(), json!("maybe"));
    assert!(ids(&a).contains(&"FAIL consent_mp".to_string()));
    // An unquoted UCI or SIN (a number) fails with a fix.
    a.insert("uci".into(), json!(42));
    assert!(
        bad(&a)
            .iter()
            .any(|m| m.starts_with("FAIL uci") && m.contains("in quotes"))
    );
}

#[test]
fn police_skips_origin_before_pr_but_not_after() {
    let mut a = base();
    // Lived in Freedonia right before PR: no certificate for that time.
    at(&mut a, "addresses").as_array_mut().unwrap().push(
        json!({"from": "2020-01-01", "to": "2023-08-31", "address": "x", "city": "y", "country": "Freedonia"}),
    );
    a.insert("pr_date".into(), json!("2023-09-01"));
    assert!(A::police_countries(&a).is_empty());
    // Another country with 183+ days in a row is needed.
    at(&mut a, "trips")
        .as_array_mut()
        .unwrap()
        .push(json!({"left": "2027-01-01", "returned": "2027-07-02", "country": "Sylvania"}));
    assert_eq!(
        A::police_countries(&a),
        vec![("Sylvania".into(), "2027-07-02".into())]
    );
}

#[test]
fn gaps_find_uncovered_days_and_present_runs_to_sign() {
    let s = |f: &str, t: &str| (f.to_string(), t.to_string());
    let g = A::gaps(
        &[s("2020-01-01", "2020-01-10"), s("2020-01-15", "present")],
        "2020-01-01",
        "2020-02-01",
        "2020-02-01",
    );
    assert_eq!(g, vec![s("2020-01-11", "2020-01-14")]);
    assert_eq!(
        A::gaps(&[], "2020-01-01", "2020-01-02", "x"),
        vec![s("2020-01-01", "2020-01-02")]
    );
}

#[test]
fn screen_matches_other_formats_and_never_echoes() {
    let f = |l: &str, v: &str| (l.to_string(), v.to_string());
    let r = A::judge_screen(
        &[
            f("UCI", "0000-0000-42"),
            f("Date of birth", "April 2, 1990"),
            f("Telephone", "555 010 0000"),
            f("Telephone type", "mobile"),
            f("Postal code", "a1a1a1"),
            f("Family name", "Wrongname"),
            f("Given name", ""),
            f("Other family name", "x"),
            f("Member of Parliament consent", "No"),
        ],
        &base(),
    );
    let lv: Vec<&str> = r.iter().map(|x| x.level).collect();
    assert_eq!(
        lv,
        [
            "MATCH", "MATCH", "MATCH", "UNKNOWN", "MATCH", "MISMATCH", "EMPTY", "UNKNOWN", "MATCH"
        ]
    );
    assert!(
        r.iter()
            .all(|x| !x.msg.contains("Canaryson") && !x.msg.contains(UCI))
    );
}

#[test]
fn parsers_read_csv_months_mrz_and_pr_card() {
    let rows = C::csv("Notes:\n\"a\",b\n\"x, y\",\"he said \"\"hi\"\"\"\n");
    assert_eq!(C::cell(&rows[0], "a"), Some("x, y"));
    assert_eq!(C::cell(&rows[0], "b"), Some("he said \"hi\""));
    assert_eq!(C::day("Mar 2022", false).as_deref(), Some("2022-03-01"));
    assert_eq!(C::day("Feb 2024", true).as_deref(), Some("2024-02-29"));
    assert_eq!(C::day("2021", true).as_deref(), Some("2021-12-31"));
    let m = C::parse_mrz(PASSPORT, TODAY).unwrap();
    assert!(m.valid && m.passport);
    assert_eq!(
        (m.number.as_str(), m.birth.as_deref()),
        ("L898902C3", Some("1974-08-12"))
    );
    assert!(
        !C::parse_mrz(&PASSPORT.replace("L898902C36", "L898902C46"), TODAY)
            .unwrap()
            .valid
    );
    let (p, v) = C::parse_pr_card(PR_CARD, TODAY);
    assert_eq!(v, Some(true));
    assert_eq!(p["uci"], json!("0000000000"));
    assert_eq!(p["pr_date"], json!("2021-06-01"));
    assert_eq!(p["birth_date"], json!("1974-08-12"));
}

#[test]
fn stays_join_into_trips_and_cra_reads_years() {
    let stays = C::parse_nomads(NOMADS).unwrap();
    let trips = C::trips_from_stays(&stays);
    assert_eq!(trips.len(), 2);
    assert_eq!(trips[1]["country"], json!("Mexico, United States"));
    assert_eq!(trips[1]["stays"].as_array().unwrap().len(), 2);
    let t = C::parse_cra("Tax year 2023 Assessed on 2024-04-15\n2022 Not filed\n");
    let ys = t["taxes"]["years"].as_array().unwrap();
    assert_eq!(
        ys[0],
        json!({"year": 2022, "required": false, "filed": false})
    );
    assert_eq!(ys[1]["filed"], json!(true));
}

#[test]
fn merge_keeps_the_owners_answers_and_refreshes_its_own() {
    let mut a = A::Answers::new();
    a.insert("city".into(), json!("Mine"));
    let part = json!({"city": "Toronto", "uci": "1"})
        .as_object()
        .unwrap()
        .clone();
    let (n, added, kept) = D::merge(&a, &part, "leases", Some(TODAY));
    assert_eq!(
        (n["city"].clone(), added, kept),
        (
            json!("Mine"),
            vec!["uci".to_string()],
            vec!["city".to_string()]
        )
    );
    let part2 = json!({"uci": "2"}).as_object().unwrap().clone();
    let (n2, ..) = D::merge(&n, &part2, "pr-card", None);
    assert_eq!(n2["uci"], json!("2"));
    // An edit of a collected value survives the next collect.
    let mut e = n2.clone();
    e.insert("uci".into(), json!("3"));
    let (n3, _, kept) = D::merge(&e, &part2, "pr-card", None);
    assert_eq!(
        (n3["uci"].clone(), kept),
        (json!("3"), vec!["uci".to_string()])
    );
}

fn sources(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for (n, t) in [
        (
            "Email Addresses.csv",
            "Email Address,Confirmed,Primary,Updated On\nanna@example.com,Yes,Yes,1/1/20\n",
        ),
        (
            "PhoneNumbers.csv",
            "Extension,Number,Type\n,+1 555 010 0000,Mobile\n",
        ),
        (
            "Positions.csv",
            "Company Name,Title,Description,Location,Started On,Finished On\nExample Corp,Software Developer,\"Builds things, ships them\",\"Toronto, Ontario, Canada\",Mar 2022,\nSample Inc,Developer,,\"Toronto, Ontario, Canada\",Jan 2019,Feb 2022\n",
        ),
        (
            "cra-tax-returns.txt",
            &format!(
                "CRA My Account - Tax returns (synthetic)\nSocial insurance number: {SIN}\n2021 Assessed\n2022 Assessed\n2023 Reassessed\n2024 Assessed\n2025 Assessed\n"
            ),
        ),
        (
            "drivers-licence.txt",
            "ONTARIO DRIVER'S LICENCE  Class G  (synthetic)\n",
        ),
        (
            "language-celpip.txt",
            "CELPIP-General Official Score Report (synthetic). Listening 9 Speaking 8\n",
        ),
        (
            "leases.csv",
            "from,to,address,city,country,postal_code\n2023-05-01,present,1 Example Street Apt 2,Toronto,Canada,A1A 1A1\n2019-01-01,2023-04-30,9 Sample Avenue,Toronto,Canada,A1A 1B1\n",
        ),
        ("nomads.json", NOMADS),
        ("passport.txt", PASSPORT),
        ("pr-card.txt", PR_CARD),
        (
            "separation-agreement.txt",
            "SEPARATION AGREEMENT (synthetic). Date of separation: 2025-02-01.\n",
        ),
    ] {
        std::fs::write(dir.join(n), t).unwrap();
    }
}

#[test]
fn collect_all_fills_95_percent_sealed_and_prints_no_canary() {
    let u = user("collect");
    let src = u.join("scratch-sources");
    sources(&src);
    let (o1, _) = run(&u, &["collect-all", src.to_str().unwrap()]).unwrap();
    let (o2, _) = run(&u, &["status"]).unwrap();
    let (js, _) = run(&u, &["status", "--json"]).unwrap();
    let pct = serde_json::from_str::<Value>(&js).unwrap()["coverage"]["pct"]
        .as_f64()
        .unwrap();
    assert!(pct >= 95.0, "{pct}: {o2}");
    let file = D::answers_file(&u);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Sealed: the SIN is not on disk in plain text.
    let raw = std::fs::read(&file).unwrap();
    assert!(!raw.windows(SIN.len()).any(|w| w == SIN.as_bytes()));
    let a = D::read_answers(&file).unwrap();
    assert_eq!(a["taxes"]["sin"], json!(SIN));
    let (police, _) = run(&u, &["police"]).unwrap();
    let (versions, _) = run(&u, &["versions"]).unwrap();
    let (shown, _) = run(&u, &["show"]).unwrap();
    let (report, _) = run(&u, &["report"]).unwrap();
    assert!(police.contains("no police certificate needed"), "{police}");
    assert!(versions.contains(".sqv"));
    assert!(shown.contains("hidden: an ID number"));
    for secret in [
        SIN,
        "0000000000",
        "L898902C3",
        "ERIKSSON",
        "anna@example.com",
    ] {
        for o in [&o1, &o2, &js, &police, &versions, &report] {
            assert!(!o.contains(secret), "{secret} leaked in: {o}");
        }
    }
    // An agent sees the answers it needs (names too), but no ID number.
    for id in [SIN, "0000000000", "L898902C3"] {
        assert!(!shown.contains(id), "{id} leaked in show");
    }
    // The evidence page: written under the user's folder, mode 600, path printed.
    let page = D::citizenship_dir(&u).join("evidence.html");
    assert!(report.contains(&page.display().to_string()));
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.contains("default-src 'none'") && html.contains("id=\"q-trips\""));
}

#[test]
fn put_seals_owner_answers_and_removes_its_scratch_file() {
    let u = user("put");
    let scratch = env().join("put-scratch.toml");
    std::fs::write(&scratch, format!("uci = \"{UCI}\"\nsign_date = 2030-06-01\nname = {{ family = \"Canaryson\", given = \"Alex\" }}\n")).unwrap();
    let (o, code) = run(&u, &["put", scratch.to_str().unwrap()]).unwrap();
    assert_eq!(code, 0);
    assert_eq!(o, "put: name, sign_date, uci");
    assert!(!scratch.exists());
    let a = D::read_answers(&D::answers_file(&u)).unwrap();
    assert_eq!(
        (a["sign_date"].clone(), a["_sources"]["uci"].clone()),
        (json!("2030-06-01"), json!("owner"))
    );
    // A bad file is removed too; the draft itself is refused and kept.
    std::fs::write(&scratch, "not = = toml").unwrap();
    assert!(run(&u, &["put", scratch.to_str().unwrap()]).is_err());
    assert!(!scratch.exists());
    let f = D::answers_file(&u);
    assert!(run(&u, &["put", f.to_str().unwrap()]).is_err());
    assert!(f.exists());
    let (q, _) = run(&u, &["questions"]).unwrap();
    assert!(q.starts_with("25 questions for you:"), "{q}");
    // A round trip of the full example through TOML keeps every answer.
    let b = base();
    assert_eq!(D::parse_toml(&D::to_toml(&b)).unwrap(), b);
}

#[test]
fn proofs_are_sealed_and_inlined_in_the_report() {
    let u = user("proof");
    let scratch = env().join("proof-scratch.toml");
    std::fs::write(&scratch, "city = \"Toronto\"\n").unwrap();
    run(&u, &["put", scratch.to_str().unwrap()]).unwrap();
    let img = env().join("shot.png");
    std::fs::write(&img, b"\x89PNG-not-really-CANARYPIXELS").unwrap();
    let (o, _) = run(
        &u,
        &[
            "proof",
            "city",
            img.to_str().unwrap(),
            "--source",
            "lease page",
            "--url",
            "javascript:alert(1)",
        ],
    )
    .unwrap_or_else(|e| (e, 2));
    assert!(o.contains("web address"), "{o}");
    let (o, code) = run(
        &u,
        &[
            "proof",
            "city",
            img.to_str().unwrap(),
            "--source",
            "lease page",
        ],
    )
    .unwrap();
    assert_eq!(code, 0, "{o}");
    let proofs = D::proofs_dir(&D::answers_file(&u));
    let names: Vec<String> = std::fs::read_dir(&proofs)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(names.iter().all(|n| n.ends_with(".sqv")), "{names:?}");
    assert!(
        D::read_proof(&D::answers_file(&u), "../x.png")
            .unwrap()
            .is_none()
    );
    run(&u, &["report"]).unwrap();
    let html = std::fs::read_to_string(D::citizenship_dir(&u).join("evidence.html")).unwrap();
    assert_eq!(html.matches("data:image/png;base64,").count(), 1);
    assert!(html.contains("lease page"));
    assert_eq!(judge::base64(b"hello"), "aGVsbG8=");
}

const PASSPORT: &str = "Specimen passport (ICAO 9303 sample, synthetic)\nDate of issue 2018-04-16\nP<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<\nL898902C36UTO7408122F1204159ZE184226B<<<<<10\n";
const PR_CARD: &str = "PERMANENT RESIDENT CARD / CARTE DE RESIDENT PERMANENT\nUCI/IUC 0000-0000-00\nPR since / RP depuis 2021 06 01\nI<UTOD231458907<<<<<<<<<<<<<<<\n7408122F1204159UTO<<<<<<<<<<<6\nERIKSSON<<ANNA<MARIA<<<<<<<<<<\n";
const NOMADS: &str = r#"{"trips": [
 {"date_start": "2020-01-01", "date_end": "2021-05-20", "place": "Toronto", "country": "Canada"},
 {"date_start": "2021-05-21", "date_end": "2021-05-31", "place": "Mexico City", "country": "Mexico"},
 {"date_start": "2021-06-01", "date_end": "2024-02-09", "place": "Toronto", "country": "Canada"},
 {"date_start": "2024-02-10", "date_end": "2024-02-20", "place": "Cancun", "country": "Mexico"},
 {"date_start": "2024-02-20", "date_end": "2024-03-01", "place": "Austin", "country": "United States"},
 {"date_start": "2024-03-02", "date_end": "2026-11-01", "place": "Toronto", "country": "Canada"}]}"#;
