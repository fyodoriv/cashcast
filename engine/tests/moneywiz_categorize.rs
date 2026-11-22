//! `moneywiz categorize` and the category of rows the sync adds, on a
//! synthetic MoneyWiz database and bank export. No real data.

use std::path::{Path, PathBuf};

use cashcast_engine::{config, inbox, moneywiz, moneywiz_categorize};
use chrono::NaiveDate;
use rusqlite::Connection;

const EPOCH: i64 = 978_307_200;

fn ts(date: &str) -> f64 {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    (d.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() - EPOCH) as f64
}

fn make_db(path: &Path) {
    let c = Connection::open(path).unwrap();
    c.execute_batch(
        "CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER PRIMARY KEY, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);
         CREATE TABLE ZSYNCOBJECT (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, Z_OPT INTEGER, ZGID VARCHAR,
           ZNAME VARCHAR, ZCURRENCYNAME VARCHAR, ZOPENINGBALANCE FLOAT, ZNAME2 VARCHAR, ZPARENTCATEGORY INTEGER,
           ZNAME5 VARCHAR, ZAMOUNT1 FLOAT, ZDATE1 TIMESTAMP, ZDESC2 VARCHAR, ZNOTES1 VARCHAR, ZACCOUNT2 INTEGER,
           ZPAYEE2 INTEGER, ZRECIPIENTACCOUNT1 INTEGER, ZSENDERACCOUNT INTEGER);
         CREATE TABLE ZCATEGORYASSIGMENT (Z_PK INTEGER PRIMARY KEY, ZTRANSACTION INTEGER, ZCATEGORY INTEGER, ZAMOUNT FLOAT);
         INSERT INTO Z_PRIMARYKEY VALUES (10,'Account',0,0),(11,'BankChequeAccount',10,0),
           (20,'Category',0,0),(29,'Payee',0,0),(37,'Transaction',0,0),
           (38,'DepositTransaction',37,0),(48,'WithdrawTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (1,11,'Chequing','CAD',1000.0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY) VALUES (4,20,'Food',NULL),(5,20,'Groceries',4),(6,20,'Dining',4);",
    )
    .unwrap();
    let add = |pk: i64, amount: f64, date: &str, desc: &str| {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,1,?2,?3,?4)",
            rusqlite::params![pk, amount, ts(date), desc],
        )
        .unwrap();
    };
    // The owner's own categorized rows, before the bank period.
    add(100, -30.0, "2026-08-01", "Greenleaf Market CANARYMW1");
    add(101, -35.0, "2026-08-08", "Greenleaf Market CANARYMW1");
    add(102, -20.0, "2026-08-09", "Bistro Moon CANARYMW2");
    add(103, -25.0, "2026-08-10", "Bistro Moon CANARYMW2");
    for (pk, row, cat) in [(1, 100, 5), (2, 101, 5), (3, 102, 6), (4, 103, 6)] {
        c.execute(
            "INSERT INTO ZCATEGORYASSIGMENT VALUES (?1,?2,?3,0)",
            rusqlite::params![pk, row, cat],
        )
        .unwrap();
    }
    // Rows in the bank period with no category.
    add(200, -40.0, "2026-09-02", "Greenleaf Market CANARYMW3"); // own payee
    add(201, -22.0, "2026-09-03", "BM"); // hand text too short; the bank names it
    add(202, -9.0, "2026-09-04", "Mystery CANARYMW4"); // nothing learned
    add(203, -5.0, "2026-09-05", ""); // no text at all
}

const EXPORT: &str = "Date,Account,Description,Amount,Balance
2026-09-02,Chequing,GREENLEAF MARKET CANARY01,-40.00,
2026-09-03,Chequing,BISTRO MOON CANARY02,-22.00,
2026-09-04,Chequing,MYSTERY CANARY03,-9.00,
2026-09-05,Chequing,0001 4242,-5.00,
2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,
";

fn setup(name: &str) -> (PathBuf, moneywiz::Store, inbox::Inbox, moneywiz::Plan) {
    let home = std::env::temp_dir().join(format!("cashcast-mwcat-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("inbox")).unwrap();
    let db = home.join("ipadMoneyWiz.sqlite");
    make_db(&db);
    std::fs::write(home.join("inbox").join("bank.csv"), EXPORT).unwrap();
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    (home, store, exports, plan)
}

#[test]
fn categorize_learns_from_own_rows_and_the_banks_words() {
    let (home, store, exports, plan) = setup("learn");
    let (picks, left) = moneywiz_categorize::picks(&store, &exports, &plan);
    let got: Vec<(i64, String, &str)> = picks
        .iter()
        .map(|p| (p.row.pk, p.path.join("/"), p.source))
        .collect();
    assert_eq!(
        got,
        [
            (200, "Food/Groceries".to_string(), "payee"),
            // Its own text is too short; its bank row's words match the
            // owner's "Bistro Moon" rows.
            (201, "Food/Dining".to_string(), "payee"),
            // Unknown payee and no text: the account's usual category.
            (202, "Food/Groceries".to_string(), "best guess"),
            (203, "Food/Groceries".to_string(), "best guess"),
        ]
    );
    assert_eq!((left.no_text, left.unknown), (0, 0));

    let report = moneywiz_categorize::render(&picks, &left, None);
    assert!(!report.contains("CANARY"), "{report}");
    assert!(
        report.contains("| Food > Groceries | 3 | 1 | 2 |"),
        "{report}"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn categorize_writes_an_edits_file_and_never_overwrites_one() {
    let (home, store, exports, plan) = setup("write");
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let path = home.join("inbox").join("moneywiz-edits.toml");
    let (written, skipped) = moneywiz_categorize::write_edits(&store, &picks, &path).unwrap();
    assert_eq!((written, skipped), (4, 0));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("account = \"Chequing\"\ndate = \"2026-09-02\"\namount = -40.00\nset_category = \"Food/Groceries\""), "{text}");
    assert!(text.contains("set_category = \"Food/Dining\""), "{text}");
    let err = moneywiz_categorize::write_edits(&store, &picks, &path).unwrap_err();
    assert!(err.contains("exists"), "{err}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn categorize_list_shows_each_guessed_row_for_the_owner_to_answer() {
    let (home, store, exports, plan) = setup("list");
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let out = moneywiz_categorize::render_picks(&store, &picks);
    assert!(
        out.contains(
            "| Day | Account | Amount | Payee and description | Suggested category | From |"
        ),
        "{out}"
    );
    assert!(out.contains("| 2026-09-02 | Chequing | -40.00 |"), "{out}");
    assert!(out.contains("Food > Groceries"), "{out}");
    assert_eq!(
        out.lines().filter(|l| l.starts_with("| 20")).count(),
        picks.len(),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn categorize_parks_an_edits_file_moneywiz_already_shows() {
    let (home, store, exports, plan) = setup("applied");
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let inbox = home.join("inbox");
    let path = inbox.join("moneywiz-edits.toml");
    // Only a category add MoneyWiz already has: nothing left to apply.
    let done = store.categories[0].join("/");
    std::fs::write(&path, format!("[[category]]\nadd = \"{done}\"\n")).unwrap();
    let (written, _) = moneywiz_categorize::write_edits(&store, &picks, &path).unwrap();
    assert_eq!(written, 4);
    let parked: Vec<_> = std::fs::read_dir(&inbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("moneywiz-edits-applied-"))
        .collect();
    assert_eq!(parked.len(), 1, "{parked:?}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn rows_the_sync_adds_get_the_learned_category() {
    let (home, store, exports, plan) = setup("add");
    let links = moneywiz::add_links(&store, &exports, &plan);
    let added: Vec<&String> = links.iter().map(|l| &l.url).collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert!(
        added[0].contains("category=Food%2FGroceries"),
        "{}",
        added[0]
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn one_row_is_enough_and_the_most_used_category_wins_a_conflict() {
    let (home, _, _, _) = setup("vote");
    let db = home.join("ipadMoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    // One old "Mystery" row in Dining, and a "Greenleaf" row in Dining that
    // conflicts with its two Groceries rows.
    c.execute_batch(&format!(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES
           (104,48,1,-8.0,{},'Mystery CANARYMW5'),(105,48,1,-31.0,{},'Greenleaf Market CANARYMW6');
         INSERT INTO ZCATEGORYASSIGMENT VALUES (5,104,6,0),(6,105,6,0);",
        ts("2026-08-11"),
        ts("2026-08-12")
    ))
    .unwrap();
    drop(c);
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let got: Vec<(i64, String)> = picks.iter().map(|p| (p.row.pk, p.path.join("/"))).collect();
    assert!(got.contains(&(200, "Food/Groceries".into())), "{got:?}");
    assert!(got.contains(&(202, "Food/Dining".into())), "{got:?}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_check_predicts_hidden_rows_from_the_rest() {
    let (home, store, exports, plan) = setup("check");
    // Row 100 is hidden (pk % 5 == 0); row 101 teaches the same payee.
    let [(n, right), _] = moneywiz_categorize::check(&store, &exports, &plan);
    assert_eq!((n, right), (1, 1));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_category_path_that_names_two_categories_is_skipped() {
    let (home, _, _, _) = setup("dup");
    let db = home.join("ipadMoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    // A second "Food/Groceries" (MoneyWiz allows one per type).
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY) VALUES (8,20,'Groceries',4)",
        [],
    )
    .unwrap();
    drop(c);
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let path = home.join("inbox").join("moneywiz-edits.toml");
    let (written, skipped) = moneywiz_categorize::write_edits(&store, &picks, &path).unwrap();
    assert_eq!((written, skipped), (1, 3));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("Food/Groceries"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_shared_path_is_written_with_the_type_that_fits_the_row() {
    let (home, _, _, _) = setup("kind");
    let db = home.join("ipadMoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZTYPE INTEGER;
         UPDATE ZSYNCOBJECT SET ZTYPE = 1 WHERE Z_ENT = 20;
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY,ZTYPE) VALUES (8,20,'Groceries',4,2);
         UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;",
    )
    .unwrap();
    drop(c);
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let path = home.join("inbox").join("moneywiz-edits.toml");
    let (written, skipped) = moneywiz_categorize::write_edits(&store, &picks, &path).unwrap();
    assert_eq!((written, skipped), (4, 0));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("set_category = \"Food/Groceries\"\nset_category_kind = \"expense\""),
        "{text}"
    );
    // The edit plan carries the type to the writer.
    let edits = cashcast_engine::moneywiz_edit::load(&path).unwrap();
    let r = cashcast_engine::moneywiz_edit::resolve(&store, &edits, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let grocer = r
        .plan
        .rows
        .iter()
        .find(|e| e.category.as_deref() == Some(&["Food".to_string(), "Groceries".to_string()][..]))
        .unwrap();
    assert_eq!(grocer.category_kind, Some(1));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_shared_path_takes_the_type_the_owner_used_for_rows_of_that_sign() {
    let (home, _, _, _) = setup("kindsign");
    let db = home.join("ipadMoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZTYPE INTEGER;
         UPDATE ZSYNCOBJECT SET ZTYPE = 1 WHERE Z_ENT = 20;
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY,ZTYPE) VALUES (8,20,'Groceries',4,2);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES
           (104,38,1,50.0,0,'Greenleaf Market CANARYMW1'),(105,38,1,10.0,0,'Greenleaf Market CANARYMW1'),
           (204,38,1,15.0,0,'Greenleaf Market CANARYMW6');
         INSERT INTO ZCATEGORYASSIGMENT VALUES (5,104,8,0),(6,105,8,0);
         UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;",
    )
    .unwrap();
    for (pk, day) in [
        (104, "2026-08-15"),
        (105, "2026-08-16"),
        (204, "2026-09-06"),
    ] {
        c.execute(
            "UPDATE ZSYNCOBJECT SET ZDATE1 = ?1 WHERE Z_PK = ?2",
            rusqlite::params![ts(day), pk],
        )
        .unwrap();
    }
    drop(c);
    std::fs::write(
        home.join("inbox").join("bank.csv"),
        format!("{EXPORT}2026-09-06,Chequing,GREENLEAF MARKET CANARY06,15.00,\n"),
    )
    .unwrap();
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let path = home.join("inbox").join("moneywiz-edits.toml");
    moneywiz_categorize::write_edits(&store, &picks, &path).unwrap();
    let edits = cashcast_engine::moneywiz_edit::load(&path).unwrap();
    let r = cashcast_engine::moneywiz_edit::resolve(&store, &edits, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let kind = |cents: i64| {
        r.plan
            .rows
            .iter()
            .find(|e| {
                store
                    .txns
                    .iter()
                    .any(|t| t.gid == e.gid && t.cents == cents)
            })
            .and_then(|e| e.category_kind)
    };
    assert_eq!(
        kind(1500),
        Some(2),
        "a deposit takes the owner's income type"
    );
    assert_eq!(
        kind(-4000),
        Some(1),
        "a spend takes the owner's expense type"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn rows_cashcast_categorized_are_recorded_and_not_learned_from() {
    let (home, _, _, _) = setup("record");
    let db = home.join("ipadMoneyWiz.sqlite");
    Connection::open(&db)
        .unwrap()
        .execute_batch("UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;")
        .unwrap();
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let path = home.join("inbox").join("moneywiz-edits.toml");
    let mut record = moneywiz_categorize::Record::new();
    let (written, _) =
        moneywiz_categorize::write_edits_into(&store, &picks, &path, Some(&mut record)).unwrap();
    assert!(written > 0);
    assert_eq!(record.len(), written);
    let data = home.join("data");
    moneywiz_categorize::save_record(&data, &record).unwrap();
    assert_eq!(moneywiz_categorize::load_record(&data), record);
    // A backup with no categories against itself records nothing.
    assert!(moneywiz_categorize::record_since(&store, &store).is_empty());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn coverage_counts_synced_cash_rows() {
    let (home, store, _, plan) = setup("coverage");
    let (with, need) = moneywiz::category_coverage(&store, &plan);
    // The four September rows are in the bank period and have no category.
    assert_eq!((with, need), (0, 4));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn balance_history_finds_the_day_moneywiz_leaves_the_bank() {
    let (home, _, _, _) = setup("history");
    let csv = EXPORT.replace(
        "2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,",
        "2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,753.00",
    );
    std::fs::write(home.join("inbox").join("bank.csv"), csv).unwrap();
    let db = home.join("ipadMoneyWiz.sqlite");
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let h = cashcast_engine::moneywiz_diagnose::history(&store, &exports, &plan);
    assert_eq!(h.len(), 1);
    let d = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
    assert_eq!(
        h[0].worst,
        Some((d, 6100)),
        "MoneyWiz lacks the last bank row"
    );
    assert_eq!(h[0].close_days, (4, 5));
    assert!(h[0].last_off.is_some());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn categorize_writes_exact_parts_when_any_name_holds_a_slash() {
    let (home, mut store, exports, plan) = setup("slashnames");
    // "Food/Groceries" could then also name a top-level "Food/Groceries".
    store.categories.push(vec!["Food/Groceries".to_string()]);
    let (picks, _) = moneywiz_categorize::picks(&store, &exports, &plan);
    let path = home.join("inbox").join("moneywiz-edits.toml");
    moneywiz_categorize::write_edits(&store, &picks, &path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("set_category_path = [\"Food\", \"Groceries\"]"),
        "{text}"
    );
    assert!(!text.contains("set_category = "), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn no_edit_to_write_leaves_no_file_to_block_the_next_run() {
    let (home, store, _, _) = setup("none");
    let path = home.join("inbox").join("moneywiz-edits.toml");
    let (written, _) = moneywiz_categorize::write_edits(&store, &[], &path).unwrap();
    assert_eq!(written, 0);
    assert!(
        !path.exists(),
        "an empty edits file blocks the next categorize"
    );
    // So a second run works too.
    moneywiz_categorize::write_edits(&store, &[], &path).unwrap();
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn balance_history_counts_a_fund_buy_the_export_leaves_out() {
    // An RRSP export lists the contribution but not the fund buy it pays
    // for; the balance it reports already holds the buy.
    let (home, _, _, _) = setup("fundbuy");
    let csv = format!(
        "{}2026-09-10,Chequing,CONTRIB CANARY06,50.00,\n",
        EXPORT.replace(
            "2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,",
            "2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,753.00",
        )
    );
    std::fs::write(home.join("inbox").join("bank.csv"), csv).unwrap();
    let db = home.join("ipadMoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    c.execute_batch("INSERT INTO Z_PRIMARYKEY VALUES (60,'InvestmentBuyTransaction',37,0);")
        .unwrap();
    for (pk, ent, amount, date) in [
        (204, 48, -61.0, "2026-09-20"),
        (205, 38, 50.0, "2026-09-10"),
        (206, 60, -50.0, "2026-09-10"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,?2,1,?3,?4,'')",
            rusqlite::params![pk, ent, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let h = cashcast_engine::moneywiz_diagnose::history(&store, &exports, &plan);
    assert_eq!(h.len(), 1);
    let (close, all) = h[0].close_days;
    assert_eq!(close, all, "every bank day matches: {:?}", h[0].worst);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn balance_history_counts_a_fund_buy_the_export_also_lists_once() {
    // A Wealthsimple export lists the contribution and the buy it pays for,
    // so the MoneyWiz buy must not count a second time.
    let (home, _, _, _) = setup("fundbuylisted");
    let csv = format!(
        "{}2026-09-10,Chequing,CONTRIB CANARY07,50.00,\n2026-09-10,Chequing,BUY CANARY08,-50.00,\n",
        EXPORT.replace(
            "2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,",
            "2026-09-20,Chequing,GREENLEAF MARKET CANARY05,-61.00,753.00",
        )
    );
    std::fs::write(home.join("inbox").join("bank.csv"), csv).unwrap();
    let db = home.join("ipadMoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    c.execute_batch("INSERT INTO Z_PRIMARYKEY VALUES (60,'InvestmentBuyTransaction',37,0);")
        .unwrap();
    for (pk, ent, amount, date) in [
        (204, 48, -61.0, "2026-09-20"),
        (205, 38, 50.0, "2026-09-10"),
        (206, 60, -50.0, "2026-09-10"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,?2,1,?3,?4,'')",
            rusqlite::params![pk, ent, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let cfg = config::parse(&format!("[moneywiz]\ndb = \"{}\"\n", db.display())).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let h = cashcast_engine::moneywiz_diagnose::history(&store, &exports, &plan);
    assert_eq!(h.len(), 1);
    let (close, all) = h[0].close_days;
    assert_eq!(close, all, "every bank day matches: {:?}", h[0].worst);
    let _ = std::fs::remove_dir_all(home);
}
