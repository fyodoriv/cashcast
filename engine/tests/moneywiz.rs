//! The MoneyWiz connection on a synthetic MoneyWiz database (Core Data
//! layout) and a synthetic bank export. No real data.

use std::path::{Path, PathBuf};

use cashcast_engine::{config, inbox, moneywiz};
use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

const EPOCH: i64 = 978_307_200;

fn ts(date: &str) -> f64 {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    (d.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() - EPOCH) as f64
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cashcast-mw-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inbox")).unwrap();
    dir
}

/// Entity ids differ from the common ones on purpose: the reader must map
/// them by name.
fn make_db(path: &Path) {
    let c = Connection::open(path).unwrap();
    c.execute_batch(
        "CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER PRIMARY KEY, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);
         CREATE TABLE ZSYNCOBJECT (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, Z_OPT INTEGER, ZGID VARCHAR,
           ZNAME VARCHAR, ZCURRENCYNAME VARCHAR, ZOPENINGBALANCE FLOAT, ZNAME2 VARCHAR, ZPARENTCATEGORY INTEGER,
           ZNAME5 VARCHAR, ZAMOUNT1 FLOAT, ZDATE1 TIMESTAMP, ZDESC2 VARCHAR, ZNOTES1 VARCHAR, ZACCOUNT2 INTEGER,
           ZPAYEE2 INTEGER, ZRECIPIENTACCOUNT1 INTEGER, ZSENDERACCOUNT INTEGER);
         CREATE TABLE ZCATEGORYASSIGMENT (Z_PK INTEGER PRIMARY KEY, ZTRANSACTION INTEGER, ZCATEGORY INTEGER, ZAMOUNT FLOAT);
         INSERT INTO Z_PRIMARYKEY VALUES (10,'Account',0,0),(11,'BankChequeAccount',10,0),(13,'CashAccount',10,0),
           (14,'CreditCardAccount',10,0),(20,'Category',0,0),(29,'Payee',0,0),(37,'Transaction',0,0),
           (38,'DepositTransaction',37,0),(48,'WithdrawTransaction',37,0),(46,'TransferWithdrawTransaction',37,0),
           (45,'TransferDepositTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (1,11,'Chequing','CAD',1000.0),(2,14,'Visa','CAD',0.0),(3,13,'Cash','CAD',100.0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY) VALUES (4,20,'Food',NULL),(5,20,'Groceries',4),(6,20,'Dining',4);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME5) VALUES (7,29,'GROCER CANARYMW1');",
    )
    .unwrap();
    let add = |pk: i64,
               ent: i64,
               account: i64,
               amount: f64,
               date: &str,
               desc: &str,
               payee: Option<i64>| {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZPAYEE2) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![pk, ent, account, amount, ts(date), desc, payee],
        )
        .unwrap();
    };
    add(100, 48, 1, -1800.0, "2026-09-01", "RENT CANARYMW0", None);
    add(101, 48, 1, -40.0, "2026-09-02", "", Some(7)); // the bank says -45.00
    add(102, 48, 1, -12.0, "2026-09-10", "DUPLICATE CANARYMW2", None);
    add(103, 48, 3, -20.0, "2026-09-03", "COFFEE CANARYMW3", None); // cash, by hand
    c.execute("INSERT INTO ZCATEGORYASSIGMENT VALUES (1,101,5,-40.0)", [])
        .unwrap();
}

const EXPORT: &str = "Date,Account,Description,Amount,Balance
2026-09-01,Chequing,RENT CANARY01,-1800.00,
2026-09-02,Chequing,GROCER CANARY02,-45.00,
2026-09-05,Chequing,PHONE CO CANARY03,-80.00,
2026-09-15,Chequing,VISA PAYMENT CANARY04,-500.00,
2026-09-30,Chequing,PAYROLL CANARY05,3000.00,2575.00
2026-09-08,Visa,RESTAURANT CANARY06,-30.00,
2026-09-15,Visa,PAYMENT THANK YOU CANARY07,500.00,470.00
";

const CONFIG: &str = r#"
[moneywiz]
db = "DB"
"#;

fn setup(name: &str) -> (PathBuf, PathBuf, config::Config) {
    let home = scratch(name);
    let db = home.join("ipadMoneyWiz.sqlite");
    make_db(&db);
    std::fs::write(home.join("inbox").join("bank.csv"), EXPORT).unwrap();
    let cfg = config::parse(&CONFIG.replace("DB", &db.display().to_string())).unwrap();
    (home, db, cfg)
}

#[test]
fn reads_accounts_and_rows_by_entity_name() {
    let (home, db, _) = setup("read");
    let store = moneywiz::open(&db).unwrap();
    let names: Vec<&str> = store.accounts.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Cash", "Chequing", "Visa"]);
    assert_eq!(store.txns.len(), 4);
    let grocer = store.txns.iter().find(|t| t.pk == 101).unwrap();
    assert_eq!(grocer.cents, -4000);
    assert_eq!(grocer.category, ["Food", "Groceries"]);
    assert_eq!(grocer.date, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap());
    assert_eq!(store.balance(1), 100_000 - 185_200);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn plans_adds_fixes_and_leaves_cash_alone() {
    let (home, db, cfg) = setup("plan");
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let cheq = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Chequing")
        .unwrap();
    assert_eq!(
        (
            cheq.matched,
            cheq.add.len(),
            cheq.fix.len(),
            cheq.only_in_moneywiz.len()
        ),
        (1, 3, 1, 1)
    );
    let visa = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Visa")
        .unwrap();
    assert_eq!((visa.matched, visa.add.len()), (0, 2));
    assert_eq!(plan.untouched, vec![3]);

    let links = moneywiz::add_links(&store, &exports, &plan);
    let kinds: Vec<&str> = links.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, ["expense", "expense", "transfer", "income"]);
    let transfer = links.iter().find(|l| l.kind == "transfer").unwrap();
    assert!(
        transfer
            .url
            .starts_with("moneywiz://transfer?account=Chequing&toAccount=Visa&amount=500.00")
    );
    let phone = &links[0].url;
    assert!(
        phone.contains("amount=80.00")
            && phone.contains("date=2026-09-05%2012%3A00%3A00")
            && phone.ends_with("save=true")
    );
    let dining = &links[1].url;
    assert!(
        dining.contains("category=Food%2FDining"),
        "the guessed category maps to MoneyWiz's own"
    );

    let fixes = moneywiz::fix_links(&store, &exports, &plan);
    assert_eq!(fixes.len(), 1);
    assert!(
        fixes[0].url.contains("amount=5.00")
            && fixes[0].url.contains("memo=cashcast%20fix%20%23101")
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn sync_report_holds_no_private_text_and_dry_run_sends_nothing() {
    let (home, db, cfg) = setup("report");
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(!r.report.contains("CANARY"), "private text in the report");
    assert!(
        r.report.contains("| Chequing | Chequing |")
            && r.report
                .contains("Left alone (you record these by hand): Cash")
    );
    assert!(r.report.contains("Dry run"));
    assert_eq!(r.sent, 0);
    assert!(
        r.fixes_md.contains("CANARYMW1") && r.fixes_md.contains("CANARYMW2"),
        "the owner's list names the rows"
    );
    let status = moneywiz::render_status(
        &moneywiz::open(&db).unwrap(),
        Some(&exports),
        cfg.moneywiz.as_ref(),
    );
    assert!(!status.contains("CANARY") && status.contains("none: recorded by hand"));
    assert!(status.contains("columns: account ZACCOUNT2, amount ZAMOUNT1, date ZDATE1"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn import_brings_in_only_the_accounts_recorded_by_hand() {
    let (home, _, cfg) = setup("import");
    let all = inbox::load_dir(&home.join("inbox"), &cfg).unwrap();
    let mw = all
        .files
        .iter()
        .find(|f| f.format == "MoneyWiz database")
        .expect("MoneyWiz is a source");
    assert!(mw.balances.contains_key("Cash") && !mw.balances.contains_key("Chequing"));
    let cash: Vec<_> = all.txns.iter().filter(|t| t.account == "Cash").collect();
    assert_eq!(cash.len(), 1);
    assert_eq!(cash[0].cents, -2000);
    assert_eq!(
        all.txns.iter().filter(|t| t.account == "Chequing").count(),
        5,
        "bank rows come from the export only"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn rejects_a_file_that_is_not_moneywiz() {
    let home = scratch("bad");
    let db = home.join("other.sqlite");
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE T (A INTEGER);")
        .unwrap();
    assert!(moneywiz::open(&db).is_err());
    let _ = std::fs::remove_dir_all(home);
}

/// The fake app's variables are process-wide: tests that send links take turns.
static FAKE_APP: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A fake MoneyWiz that logs each link it gets (and saves nothing).
fn fake_app(home: &Path) -> (std::sync::MutexGuard<'static, ()>, PathBuf) {
    let guard = FAKE_APP.lock().unwrap_or_else(|e| e.into_inner());
    let log = home.join("sent.txt");
    let opener = home.join("fake-open.sh");
    std::fs::write(
        &opener,
        format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n", log.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Read only by `moneywiz::sync`, under the lock above.
    unsafe {
        std::env::set_var("CASHCAST_MONEYWIZ_OPEN", &opener);
        std::env::set_var("CASHCAST_MONEYWIZ_DELAY_MS", "0");
        std::env::set_var("CASHCAST_MONEYWIZ_WAIT_S", "0");
    }
    (guard, log)
}

#[test]
fn apply_sends_each_link_once_a_day() {
    let (home, db, cfg) = setup("apply");
    let (_app, log) = fake_app(&home);
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    assert_eq!(
        r.sent, 5,
        "4 adds and 1 correction; no balance fix while a row is only in MoneyWiz"
    );
    assert!(
        r.report.contains("Sent 5 links to MoneyWiz.")
            && r.report.contains("MoneyWiz saved 0 of 5.")
    );
    assert!(!r.report.contains("CANARY"));
    let sent = std::fs::read_to_string(&log).unwrap();
    assert_eq!(sent.lines().count(), 5);
    assert!(sent.lines().all(|l| l.starts_with("moneywiz://")
        && (l.contains("memo=Added%20by%20cashcast")
            || l.contains("memo=cashcast%20fix%20%23101"))));
    let ledger = std::fs::read_to_string(home.join("data/moneywiz-sent.json")).unwrap();
    assert!(
        !ledger.contains("CANARY") && !ledger.contains("-45") && !ledger.contains("80.00"),
        "the sent list holds keyed hashes only"
    );
    // The same day again: MoneyWiz may still be saving, so nothing is resent.
    let again = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    assert_eq!(again.sent, 0);
    assert!(again.report.contains("5 links were sent today already"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn refuses_transactions_it_cannot_read() {
    let home = scratch("unknown-layout");
    let db = home.join("new.sqlite");
    Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);
             CREATE TABLE ZSYNCOBJECT (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, ZNAME VARCHAR, ZVALUE FLOAT);
             INSERT INTO Z_PRIMARYKEY VALUES (1,'CashAccount',0,0),(2,'WithdrawTransaction',0,0);
             INSERT INTO ZSYNCOBJECT VALUES (1,1,'Cash',NULL),(2,2,NULL,-5.0);",
        )
        .unwrap();
    let err = moneywiz::open(&db)
        .err()
        .expect("an unknown layout is refused, not read as empty");
    assert!(err.contains("not supported yet"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_moneywiz_row_after_the_export_makes_no_balance_correction() {
    let (home, db, cfg) = setup("after-export");
    // Recorded by hand after the export's last day: the bank's balance cannot hold it yet.
    Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (200,48,2,-25.0,?1,'LATER CANARYMW9')",
            [ts("2026-09-20")],
        )
        .unwrap();
    let store = moneywiz::open(&db).unwrap();
    assert_eq!(store.balance(2), -2500);
    assert_eq!(
        store.balance_on(2, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap()),
        0
    );
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    assert!(
        links.iter().all(|l| l.kind != "balance"),
        "the Visa balance matches the bank on the bank's day"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn import_leaves_out_unlinked_bank_accounts() {
    // A MoneyWiz bank account whose export goes by another name would be counted twice.
    let (home, db, cfg) = setup("import-bank");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (8,11,'Old Savings','CAD',0.0);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (200,38,8,50.0,?1,'INTEREST')",
        rusqlite::params![ts("2026-09-04")],
    )
    .unwrap();
    drop(c);
    let all = inbox::load_dir(&home.join("inbox"), &cfg).unwrap();
    assert!(all.txns.iter().all(|t| t.account != "Old Savings"));
    assert_eq!(all.txns.iter().filter(|t| t.account == "Cash").count(), 1);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn edit_mode_corrects_what_it_cannot_edit_in_place() {
    let (home, db, cfg) = setup("edit-transfer");
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    // The wrong-amount row (101) has no sync id here, so the writer cannot touch it.
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let corrections = |mode| {
        moneywiz::links_for(&store, &exports, &plan, mode)
            .into_iter()
            .filter(|l| l.kind == "correction")
            .count()
    };
    assert_eq!(corrections("edit"), 1);
    assert_eq!(corrections("list"), 0);
    // With a sync id on a plain expense, edit mode leaves it to the writer.
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE ZSYNCOBJECT SET ZGID = 'gid-101' WHERE Z_PK = 101",
            [],
        )
        .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let n = moneywiz::links_for(&store, &exports, &plan, "edit")
        .into_iter()
        .filter(|l| l.kind == "correction")
        .count();
    assert_eq!(n, 0);
    // A transfer always gets a correction row.
    Connection::open(&db)
        .unwrap()
        .execute("UPDATE ZSYNCOBJECT SET Z_ENT = 46 WHERE Z_PK = 101", [])
        .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let n = moneywiz::links_for(&store, &exports, &plan, "edit")
        .into_iter()
        .filter(|l| l.kind == "correction")
        .count();
    assert_eq!(n, 1);
    let _ = std::fs::remove_dir_all(home);
}

fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn moneywiz_exports_in_the_inbox_are_not_bank_rows() {
    // The owner's inbox can hold a MoneyWiz CSV export. Its rows are MoneyWiz's
    // own (maybe old) rows: sending them back would bring deleted rows back.
    let (home, db, cfg) = setup("own-export");
    std::fs::write(
        home.join("inbox").join("moneywiz-export.csv"),
        "sep=,\n\"Name\",\"Current balance\",\"Account\",\"Transfers\",\"Description\",\"Payee\",\"Category\",\"Date\",\"Time\",\"Memo\",\"Amount\",\"Currency\",\"Check #\",\"Tags\"\n\
\"\",\"\",\"Chequing\",\"\",\"OLD ROW CANARYCSV1\",\"\",\"\",\"2026-09-20\",\"09:00\",\"\",\"-99.00\",\"CAD\",\"\",\"\"\n\
\"\",\"\",\"Cash\",\"\",\"OLD CASH CANARYCSV2\",\"\",\"\",\"2026-09-21\",\"09:00\",\"\",\"-7.00\",\"CAD\",\"\",\"\"\n",
    )
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    assert_eq!(exports.moneywiz_files, ["moneywiz-export.csv"]);
    assert!(exports.txns.iter().all(|t| !t.text.contains("CANARYCSV")));
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    assert!(links.iter().all(|l| !l.url.contains("CANARYCSV")));
    assert!(
        plan.untouched.contains(&3),
        "the cash account stays by hand"
    );
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(r.report.contains("Not compared: 1 MoneyWiz export(s)"));
    // The import still reads it: there it is the cash source.
    let all = inbox::load_dir(&home.join("inbox"), &cfg).unwrap();
    assert!(all.files.iter().any(|f| f.format == "MoneyWiz CSV"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_cash_account_follows_an_export_only_when_named_in_the_config() {
    let (home, db, cfg) = setup("cash-name");
    // A bank export with an account that happens to be called "Cash".
    std::fs::write(
        home.join("inbox").join("other.csv"),
        "Date,Account,Description,Amount\n2026-09-04,Cash,ATM CANARY09,-60.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    assert!(plan.untouched.contains(&3));
    assert!(plan.unlinked_exports.contains(&"Cash".to_string()));
    let mapped = config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\n[moneywiz.accounts]\n\"Cash\" = \"Cash\"\n",
        db.display()
    ))
    .unwrap();
    let plan = moneywiz::plan(&store, &exports, mapped.moneywiz.as_ref()).unwrap();
    assert!(!plan.untouched.contains(&3), "named in the config: linked");
    // An export the owner keeps out of MoneyWiz is not "unlinked".
    let skipped = config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\n[moneywiz.not_tracked]\n\"Cash\" = \"unused savings\"\n",
        db.display()
    ))
    .unwrap();
    let plan = moneywiz::plan(&store, &exports, skipped.moneywiz.as_ref()).unwrap();
    assert!(!plan.unlinked_exports.contains(&"Cash".to_string()));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_equally_good_second_match_is_a_conflict_not_a_guess() {
    let (home, db, cfg) = setup("conflict");
    let c = Connection::open(&db).unwrap();
    for (pk, amount) in [(300, -10.0), (301, -11.0)] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,2,?2,?3,'CAFE CANARYMW5')",
            rusqlite::params![pk, amount, ts("2026-09-08")],
        )
        .unwrap();
    }
    drop(c);
    std::fs::write(
        home.join("inbox").join("visa.csv"),
        "Date,Account,Description,Amount\n2026-09-08,Visa,CAFE CANARY10,-12.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let visa = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Visa")
        .unwrap();
    assert!(visa.fix.is_empty(), "no amount is changed on a guess");
    assert_eq!(visa.conflicts.len(), 1);
    assert_eq!(visa.conflicts[0].candidates, [300, 301]);
    let conflicted = visa.conflicts[0].rows[0];
    assert!(
        !visa.add.contains(&conflicted),
        "and the row is not added twice"
    );
    assert!(
        visa.only_in_moneywiz
            .iter()
            .all(|pk| *pk != 300 && *pk != 301)
    );
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(r.report.contains("1 conflicts"));
    assert!(!r.report.contains("CANARY"));
    assert!(r.fixes_md.contains("CAFE CANARY10") && r.fixes_md.contains("CANARYMW5"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_sent_list_is_durable_keyed_and_private() {
    let (home, db, cfg) = setup("durable");
    let (_app, log) = fake_app(&home);
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
    let data = home.join("data");

    let r = moneywiz::sync(&db, &exports, &cfg, &home, day(1), true, false).unwrap();
    assert_eq!(r.sent, 5);
    assert_eq!(mode_of(&data.join(moneywiz::KEY_FILE)), 0o600);
    assert_eq!(mode_of(&data.join(moneywiz::SENT_FILE)), 0o600);
    let list: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data.join(moneywiz::SENT_FILE)).unwrap())
            .unwrap();
    let keys: Vec<&String> = list["sent"].as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 5);
    assert!(
        keys.iter()
            .all(|k| k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit()))
    );
    assert!(
        !data.join("moneywiz-sync.lock").exists(),
        "the lock is released"
    );

    // The next day MoneyWiz still lacks them and never held them: sent again.
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day(2), true, false).unwrap();
    assert_eq!(r.sent, 5, "MoneyWiz never took them");

    // Seen in MoneyWiz, then deleted there by the owner: not sent again.
    let mut sent = moneywiz::SentList::load(&data, day(2), false).unwrap();
    sent.mark_saved(&[]);
    sent.write(&data).unwrap();
    // The web view counts them the way the sync does: gone, not to send.
    let v = cashcast_engine::moneywiz_view::view(&db, &exports, &cfg, &home, &[], day(3)).unwrap();
    assert_eq!(
        (v["links"]["toSend"].as_u64(), v["links"]["gone"].as_u64()),
        (Some(0), Some(5))
    );
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day(3), true, false).unwrap();
    assert_eq!(r.sent, 0);
    assert!(r.report.contains("5 links were sent on an earlier day"));
    assert!(r.report.contains("moneywiz sync --resend"));
    assert!(
        r.fixes_md.contains("PHONE CO CANARY03"),
        "the owner sees which rows"
    );
    assert!(!r.report.contains("CANARY"));

    // The owner asks for them again.
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day(3), true, true).unwrap();
    assert_eq!(r.sent, 5);
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 15);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn another_sync_running_blocks_a_second_one() {
    let (home, db, cfg) = setup("lock");
    let (_app, _) = fake_app(&home);
    std::fs::create_dir_all(home.join("data")).unwrap();
    std::fs::write(home.join("data/moneywiz-sync.lock"), "").unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let err = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false)
        .err()
        .expect("a second sync is refused");
    assert!(err.contains("another MoneyWiz sync is running"));
    assert!(
        moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).is_ok(),
        "a dry run still works"
    );
    // A lock held by a live process blocks; one left by a killed run does not.
    let lock = home.join("data/moneywiz-sync.lock");
    std::fs::write(&lock, std::process::id().to_string()).unwrap();
    assert!(moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).is_err());
    let mut dead = std::process::Command::new("true").spawn().unwrap();
    let pid = dead.id();
    dead.wait().unwrap();
    std::fs::write(&lock, pid.to_string()).unwrap();
    assert!(
        moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).is_ok(),
        "a killed run's lock is taken over"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_sent_list_from_before_the_upgrade_still_holds_back_todays_links() {
    let (home, db, cfg) = setup("legacy");
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    use std::hash::{DefaultHasher, Hash, Hasher};
    let legacy: serde_json::Map<String, serde_json::Value> = links
        .iter()
        .map(|l| {
            let mut h = DefaultHasher::new();
            l.url.hash(&mut h);
            (format!("{:016x}", h.finish()), "2026-10-01".into())
        })
        .collect();
    std::fs::create_dir_all(home.join("data")).unwrap();
    std::fs::write(
        home.join("data").join(moneywiz::SENT_FILE),
        serde_json::to_string(&legacy).unwrap(),
    )
    .unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(r.report.contains("5 links were sent today already"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn coverage_counts_what_an_import_would_produce_and_names_each_gap() {
    use cashcast_engine::moneywiz_coverage;
    let (home, db, cfg) = setup("coverage");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO Z_PRIMARYKEY VALUES (15,'ForexAccount',10,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (8,14,'Northbank Card','CAD',0.0),(9,15,'Coinplace','CAD',0.0),(10,11,'Oldbank','CAD',0.0);",
    )
    .unwrap();
    for (pk, account, amount, date) in [
        (400, 8, -14.0, "2026-09-11"),
        (401, 8, -15.0, "2026-09-12"),
        (402, 9, 50.0, "2026-09-12"),
        (403, 10, -5.0, "2020-01-02"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,?2,?3,?4,'ROW CANARYMW7')",
            rusqlite::params![pk, account, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    // A card export of Northbank under the bank's own file name.
    std::fs::write(
        home.join("inbox").join("northbank.csv"),
        "Date,Account,Description,Amount\n2026-09-11,Northbank Visa 99,SHOP CANARY11,-14.00\n",
    )
    .unwrap();
    let connectors = home.join("connectors");
    std::fs::create_dir_all(&connectors).unwrap();
    std::fs::write(
        connectors.join("northbank.toml"),
        "id = \"northbank\"\nname = \"Northbank\"\nkinds = [\"bank\", \"card\"]\n[import]\nformats = [\"ofx\"]\n",
    )
    .unwrap();
    std::fs::write(
        connectors.join("coinplace.toml"),
        "id = \"coinplace\"\nname = \"Coinplace\"\nkinds = [\"crypto\"]\n[import]\nformats = [\"coinplace-ledger-csv\"]\n[api]\navailable = true\nread = [\"ledgers\"]\n",
    )
    .unwrap();
    let connectors = moneywiz_coverage::load_connectors(&connectors);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let cov =
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &connectors, today)
            .unwrap();
    let by = |name: &str| cov.accounts.iter().find(|a| a.name == name).unwrap();
    // Chequing: the rent matches, the grocer matches with another amount, the duplicate does not.
    assert_eq!((by("Chequing").rows, by("Chequing").covered), (3, 2));
    assert!(
        by("Chequing")
            .gap
            .as_deref()
            .unwrap()
            .contains("sync --dry-run")
    );
    assert!(
        cov.accounts.iter().all(|a| a.name != "Cash"),
        "cash is left out"
    );
    assert_eq!(by("Northbank Card").covered, 0);
    assert!(
        by("Northbank Card")
            .gap
            .as_deref()
            .unwrap()
            .contains("suggested links")
    );
    assert_eq!(
        cov.suggestions,
        [(
            "Northbank Card".to_string(),
            "Northbank Visa 99".to_string()
        )]
    );
    assert!(
        by("Coinplace")
            .gap
            .as_deref()
            .unwrap()
            .starts_with("reader missing: coinplace")
    );
    assert!(
        by("Oldbank")
            .gap
            .as_deref()
            .unwrap()
            .contains("no connector")
    );
    assert_eq!(by("Oldbank").rows_recent, 0);
    assert_eq!(
        cov.with_links,
        Some((3, 3)),
        "the suggested link adds one covered row"
    );
    let report = moneywiz_coverage::render(&cov, today);
    assert!(report.contains("Coverage: 2 of 7 rows (28.6 %), all time."));
    assert!(report.contains("\"Northbank Card\" = \"Northbank Visa 99\""));
    assert!(report.contains(
        "- coinplace: only ledger readers (coinplace-ledger-csv); its API has transactions"
    ));
    assert!(!report.contains("CANARY") && !report.contains("14.00") && !report.contains("1,800"));
    let _ = std::fs::remove_dir_all(home);
}

/// With `[moneywiz] since`, coverage counts every row from that day on: the
/// goal is every MoneyWiz row matched and no bank row missing. Each account
/// with a gap names the days to download; accounts in
/// `[moneywiz.out_of_scope]` are left out of the goal but still listed.
#[test]
fn coverage_since_names_every_gap_from_the_start_day() {
    use cashcast_engine::moneywiz_coverage;
    let (home, db, _) = setup("coverage-since");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (10,11,'Oldbank','CAD',0.0),(11,11,'Handbank','CAD',0.0);",
    )
    .unwrap();
    for (pk, account, amount, date) in [
        (104, 1, -9.0, "2026-08-20"),  // before the export starts
        (403, 10, -5.0, "2020-01-02"), // before `since`: not counted
        (404, 11, -7.0, "2026-09-03"), // out of scope
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,?2,?3,?4,'ROW CANARYMW8')",
            rusqlite::params![pk, account, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let cfg = config::parse(&format!(
        "{}since = \"2026-08-01\"\n[moneywiz.out_of_scope]\n\"Handbank\" = \"hand-entered\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let cov =
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &[], today).unwrap();
    let by = |name: &str| cov.accounts.iter().find(|a| a.name == name).unwrap();
    let cheq = by("Chequing");
    assert_eq!(
        (cheq.rows_since, cheq.covered_since, cheq.to_add_since),
        (4, 2, 3)
    );
    let gap = cheq.since_gap.as_deref().unwrap();
    assert!(gap.contains("download 2026-08-01 to 2026-08-31"), "{gap}");
    assert!(gap.contains("3 bank rows to add"), "{gap}");
    assert!(gap.contains("1 MoneyWiz row with no bank row"), "{gap}");
    assert_eq!((by("Visa").rows_since, by("Visa").to_add_since), (0, 2));
    assert_eq!(by("Oldbank").rows_since, 0);
    assert!(by("Oldbank").since_gap.is_none());
    let report = moneywiz_coverage::render(&cov, today);
    assert!(
        report.contains(
            "Coverage since 2026-08-01: 2 of 4 MoneyWiz rows (50.0 %), 5 bank rows not in MoneyWiz. Goal: 100 % and 0."
        ),
        "{report}"
    );
    assert!(
        report.contains("History gaps since 2026-08-01: Chequing ("),
        "{report}"
    );
    assert!(report.contains("Visa (2 bank rows to add"), "{report}");
    assert!(
        report.contains("Left out on purpose: Handbank (hand-entered)."),
        "{report}"
    );
    // Each open MoneyWiz row is listed (account, day, amount, kind), so the
    // owner can decide it; never its payee or text.
    assert!(
        report.contains("Open MoneyWiz rows since 2026-08-01 (2):"),
        "{report}"
    );
    assert!(
        report.contains("| Chequing | 2026-08-20 | -9.00 | Withdraw | - |"),
        "{report}"
    );
    assert!(
        !report.contains("| Handbank | 2026-09-03 |"),
        "out of scope rows are not open: {report}"
    );
    assert!(!report.contains("CANARY") && !report.contains("1,800"));
    let _ = std::fs::remove_dir_all(home);
}

/// A bank with no file for its oldest months: the owner accepts MoneyWiz as
/// the record up to a day (`[moneywiz.accepted]`), and coverage counts those
/// rows as decided instead of naming days to download.
#[test]
fn accepted_rows_count_as_covered_up_to_their_day() {
    use cashcast_engine::moneywiz_coverage;
    let (home, db, _) = setup("coverage-accepted");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (104,48,1,-9.0,?1,'ROW CANARYMW8')",
        rusqlite::params![ts("2026-08-20")],
    )
    .unwrap();
    drop(c);
    let base = format!(
        "{}since = \"2026-08-01\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    );
    let store = moneywiz::open(&db).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let run = |extra: &str| {
        let cfg = config::parse(&format!("{base}{extra}")).unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &[], today)
    };
    let before = run("").unwrap();
    let cheq = before
        .accounts
        .iter()
        .find(|a| a.name == "Chequing")
        .unwrap();
    assert!(
        cheq.since_gap
            .as_deref()
            .unwrap()
            .contains("download 2026-08-01")
    );
    let was = cheq.covered_since;
    let after =
        run("[moneywiz.accepted]\n\"Chequing\" = \"2026-08-20: no statement that old\"\n").unwrap();
    let cheq = after
        .accounts
        .iter()
        .find(|a| a.name == "Chequing")
        .unwrap();
    assert_eq!(
        cheq.covered_since,
        was + 1,
        "only the accepted row joins the covered ones"
    );
    let gap = cheq.since_gap.as_deref().unwrap_or("");
    assert!(!gap.contains("download 2026-08-01"), "{gap}");
    assert!(
        cheq.source
            .contains("owner accepted MoneyWiz to 2026-08-20"),
        "{}",
        cheq.source
    );
    // A 0.00 cash row changes no balance and no bank file lists it: it
    // needs no bank row (owner, 2026-11-16).
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (105,48,1,0.0,?1,'ZERO CANARYMW9')",
        rusqlite::params![ts("2026-08-25")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let cfg = config::parse(&base).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let zero =
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &[], today).unwrap();
    let cheq = zero.accounts.iter().find(|a| a.name == "Chequing").unwrap();
    assert_eq!(
        (cheq.rows_since, cheq.covered_since),
        (
            before
                .accounts
                .iter()
                .find(|a| a.name == "Chequing")
                .unwrap()
                .rows_since
                + 1,
            was + 1
        ),
        "the 0.00 row is counted and covered"
    );
    // A value without its day is refused, not ignored.
    let err = run("[moneywiz.accepted]\n\"Chequing\" = \"no day\"\n")
        .err()
        .unwrap();
    assert!(err.contains("[moneywiz.accepted]"), "{err}");
    let _ = std::fs::remove_dir_all(home);
}

/// Rows typed by hand are rough: another payee text, an amount a bit off,
/// a date a few days away. With `match = "loose"` such a row pairs with its
/// bank row (the bank's amount wins), instead of the bank row being added
/// a second time. Without it, nothing changes.
#[test]
fn loose_matching_pairs_rough_hand_rows_with_the_bank() {
    let (home, db, _) = setup("loose");
    let c = Connection::open(&db).unwrap();
    for (pk, amount, date) in [
        (300, -27.0, "2026-09-11"), // the bank's -30.00 RESTAURANT on 09-08
        (301, 30.0, "2026-09-08"),  // money in: never pairs with money out
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,2,?2,?3,'DINNER CANARYMW6')",
            rusqlite::params![pk, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let visa = |extra: &str| {
        let cfg = config::parse(&format!(
            "{}{extra}",
            CONFIG.replace("DB", &db.display().to_string())
        ))
        .unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        let a = plan
            .accounts
            .into_iter()
            .find(|a| a.export_account == "Visa")
            .unwrap();
        (
            a.matched,
            a.add.len(),
            a.fix.len(),
            // Loose mode may offset them (stand-ins); either way they are seen.
            a.only_in_moneywiz.len() + a.stand_in.len(),
            a.fix,
        )
    };
    let strict = visa("");
    assert_eq!((strict.0, strict.1, strict.2, strict.3), (0, 2, 0, 2));
    let loose = visa("match = \"loose\"\n");
    assert_eq!((loose.0, loose.1, loose.2, loose.3), (0, 1, 1, 1));
    assert_eq!(loose.4[0].0, 300, "the rough row gets the bank's amount");
    let _ = std::fs::remove_dir_all(home);
}

/// Investment accounts take the cash rows of their broker's ledger (Sun Life
/// contributions, fees and withdrawals; E*TRADE buys, sells and dividends).
/// Fund switches move no money and make no rows. An account named for no
/// bank gets its connector from `[moneywiz.connectors]`, and an account no
/// export will cover gets its reason from `[moneywiz.out_of_scope]`.
#[test]
fn coverage_reads_investment_ledgers_as_cash_rows() {
    use cashcast_engine::moneywiz_coverage;
    let (home, db, _) = setup("coverage-ledgers");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (20,16,'Sunlife Retirement','CAD',0.0),(21,16,'Work Stock','USD',0.0),
           (22,11,'Closedbank','CAD',0.0);",
    )
    .unwrap();
    for (pk, account, amount, date) in [
        (500, 20, 250.0, "2025-01-15"),
        (501, 20, 125.0, "2025-01-15"),
        (502, 20, -5.0, "2025-03-31"),
        (503, 20, -50.0, "2025-07-15"),
        (504, 21, 12.5, "2025-02-14"),
        (505, 21, -2005.0, "2025-01-10"),
        (506, 22, -9.0, "2025-05-01"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,?2,?3,?4,'ROW CANARYMW8')",
            rusqlite::params![pk, account, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (from, to) in [
        (
            "ledger-sunlife-transactions.csv",
            "sunlife-transactions.csv",
        ),
        ("ledger-etrade.csv", "etrade.csv"),
    ] {
        std::fs::copy(fixtures.join(from), home.join("inbox").join(to)).unwrap();
    }
    let connectors = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../connectors");
    let connectors = moneywiz_coverage::load_connectors(&connectors);
    let store = moneywiz::open(&db).unwrap();
    let cfg = config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\n[moneywiz.connectors]\n\"Work Stock\" = \"etrade\"\n[moneywiz.out_of_scope]\nClosedbank = \"closed, no exports\"\n",
        db.display()
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    // Switches (the fund transfer pair) and the share-only transfer make no rows.
    let rows = |a: &str| exports.txns.iter().filter(|t| t.account == a).count();
    assert_eq!(rows("Sun Life RRSP"), 8);
    assert_eq!(rows("Sun Life TFSA"), 1);
    assert_eq!(rows("E*TRADE"), 3);
    // The cash import never sees ledger rows.
    let import = inbox::load_dir(&home.join("inbox"), &cfg).unwrap();
    assert!(
        import
            .txns
            .iter()
            .all(|t| t.account != "E*TRADE" && !t.account.starts_with("Sun Life"))
    );

    let today = NaiveDate::from_ymd_opt(2025, 10, 1).unwrap();
    let cov =
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &connectors, today)
            .unwrap();
    let by = |name: &str| cov.accounts.iter().find(|a| a.name == name).unwrap();
    assert!(cov.accounts.iter().all(|a| {
        !a.gap.as_deref().unwrap_or("").starts_with("reader missing")
            && !a.gap.as_deref().unwrap_or("").contains("no connector")
    }));
    assert_eq!(by("Work Stock").connector.as_deref(), Some("etrade"));
    assert!(
        by("Work Stock")
            .gap
            .as_deref()
            .unwrap()
            .contains("suggested links")
    );
    assert_eq!(
        by("Closedbank").gap.as_deref(),
        Some("out of scope: closed, no exports")
    );
    // The Sun Life file holds two plans: the owner links one.
    assert!(
        by("Sunlife Retirement")
            .gap
            .as_deref()
            .unwrap()
            .starts_with("2 exports")
    );
    assert_eq!(
        cov.suggestions,
        [("Work Stock".to_string(), "E*TRADE".to_string())]
    );
    let (with_links, _) = cov.with_links.unwrap();
    assert_eq!(with_links, 2 + 2, "Chequing's 2 plus both E*TRADE rows");

    // Linked, the rows match as an import would.
    let mut linked = cfg.moneywiz.clone().unwrap();
    linked
        .accounts
        .insert("Sunlife Retirement".into(), "Sun Life RRSP".into());
    let cov =
        moneywiz_coverage::coverage(&store, &exports, Some(&linked), &connectors, today).unwrap();
    let s = cov
        .accounts
        .iter()
        .find(|a| a.name == "Sunlife Retirement")
        .unwrap();
    assert_eq!((s.rows, s.covered), (4, 4));
    let report = moneywiz_coverage::render(&cov, today);
    assert!(!report.contains("CANARY") && !report.contains("2,005") && !report.contains("250.00"));
    let _ = std::fs::remove_dir_all(home);
}

/// Two overlapping downloads of one ledger (the second starts a row later,
/// so every shared row sits on another line) hold each row once.
#[test]
fn overlapping_ledger_downloads_fold_into_one_copy_of_each_row() {
    let (home, _, cfg) = setup("ledger-overlap");
    let fixture = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ledger-sunlife-transactions.csv"),
    )
    .unwrap();
    let inbox_dir = home.join("inbox");
    std::fs::remove_file(inbox_dir.join("bank.csv")).unwrap();
    std::fs::write(inbox_dir.join("sunlife-transactions.csv"), &fixture).unwrap();
    let one = inbox::load_exports(&inbox_dir, &cfg).unwrap().txns.len();
    let mut lines: Vec<&str> = fixture.lines().collect();
    let header = lines.iter().position(|l| l.contains("Date")).unwrap_or(0);
    lines.remove(header + 1);
    std::fs::write(
        inbox_dir.join("sunlife-transactions (1).csv"),
        lines.join("\n") + "\n",
    )
    .unwrap();
    let both = inbox::load_exports(&inbox_dir, &cfg).unwrap();
    assert_eq!(both.txns.len(), one, "the overlap is held once");
    let _ = std::fs::remove_dir_all(home);
}

/// Hand rows with a rough date or amount (loose mode): the exact amount a
/// week off still pairs, and a close amount wins over a far one on the same
/// day, so the right rows pair and fewer need a fix.
#[test]
fn loose_matching_prefers_the_closest_amount_and_reaches_rough_dates() {
    let (home, db, _) = setup("loose-rough");
    let c = Connection::open(&db).unwrap();
    for (pk, amount, date) in [
        (310, -80.0, "2026-09-12"), // the bank's PHONE CO -80.00 on 09-05, a week off
        (311, -31.0, "2026-09-09"), // -30.00 RESTAURANT on 09-08: close amount, a day off
        (312, -24.0, "2026-09-08"), // same day as RESTAURANT, but 20 % off
    ] {
        let account = if pk == 310 { 1 } else { 2 };
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,?2,?3,?4,'ROUGH CANARYMW9')",
            rusqlite::params![pk, account, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let cfg = config::parse(&format!(
        "{}match = \"loose\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let get = |name: &str| {
        plan.accounts
            .iter()
            .find(|a| a.export_account == name)
            .unwrap()
    };
    let cheq = get("Chequing");
    assert!(
        cheq.matched_mw.contains(&310),
        "the exact amount a week off pairs"
    );
    let visa = get("Visa");
    assert_eq!(
        visa.fix.iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![311],
        "the close amount pairs, not the far one on the same day"
    );
    assert!(visa.only_in_moneywiz.contains(&312) || visa.stand_in.contains(&312));
    let _ = std::fs::remove_dir_all(home);
}

fn ledger_json(home: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(home.join("data/moneywiz-ledger.json")).unwrap();
    assert!(
        !text.contains("CANARY"),
        "the ledger holds keyed hashes, not row text"
    );
    serde_json::from_str(&text).unwrap()
}

/// Every bank row gets a saved status with the days it was first seen, last
/// checked and last changed; each run is kept; unchanged inputs reuse the
/// last report.
#[test]
fn the_ledger_keeps_each_rows_status_dates_and_the_runs() {
    let (home, db, cfg) = setup("ledger");
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day1 = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day1, false, false).unwrap();
    assert!(r.report.contains("Ledger: 7 bank rows tracked"));
    assert_eq!(mode_of(&home.join("data/moneywiz-ledger.json")), 0o600);
    let l = ledger_json(&home);
    let rows = l["rows"].as_object().unwrap();
    assert_eq!(rows.len(), 7);
    let count = |s: &str| rows.values().filter(|r| r["status"] == s).count();
    assert_eq!((count("matched"), count("fix"), count("add")), (1, 1, 5));
    assert!(
        rows.values()
            .all(|r| r["first_seen"] == "2026-10-01" && r["last_checked"] == "2026-10-01")
    );
    assert_eq!(l["runs"].as_array().unwrap().len(), 1);

    // Same inputs the same day: the cached report, nothing recomputed.
    let day2 = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
    let again = moneywiz::sync(&db, &exports, &cfg, &home, day1, false, false).unwrap();
    assert!(
        again
            .report
            .contains("Cached: the exports, MoneyWiz and the settings are unchanged")
    );
    assert_eq!(ledger_json(&home)["runs"].as_array().unwrap().len(), 1);
    // A new day misses the cache: export ages and link states change.
    let next = moneywiz::sync(&db, &exports, &cfg, &home, day2, false, false).unwrap();
    assert!(!next.report.contains("Cached"));
    // So does a config change (an export option, say).
    let toml = home.join("cashcast.toml");
    let before = std::fs::read_to_string(&toml).unwrap_or_default();
    std::fs::write(&toml, format!("{before}\n# changed\n")).unwrap();
    let changed = moneywiz::sync(&db, &exports, &cfg, &home, day2, false, false).unwrap();
    assert!(!changed.report.contains("Cached"));

    // A new export file: checked again, first-seen days kept.
    std::fs::write(
        home.join("inbox/more.csv"),
        "Date,Account,Description,Amount,Balance\n2026-09-20,Visa,BOOKS CANARY08,-12.00,\n",
    )
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day2, false, false).unwrap();
    assert!(!r.report.contains("Cached"));
    let l = ledger_json(&home);
    let rows = l["rows"].as_object().unwrap();
    assert_eq!(rows.len(), 8);
    assert_eq!(
        rows.values()
            .filter(|r| r["first_seen"] == "2026-10-01")
            .count(),
        7
    );
    assert!(rows.values().all(|r| r["last_checked"] == "2026-10-02"));
    assert_eq!(l["runs"].as_array().unwrap().len(), 4);
    let _ = std::fs::remove_dir_all(home);
}

/// `resync` asks the next sync to send again the rows the owner deleted in
/// MoneyWiz, in one account and date range only.
#[test]
fn resync_sends_again_only_the_asked_range() {
    let (home, db, cfg) = setup("resync");
    let (_app, log) = fake_app(&home);
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
    moneywiz::sync(&db, &exports, &cfg, &home, day(1), true, false).unwrap();
    // MoneyWiz saved them all once, and the owner deleted them since.
    let path = home.join("data").join(moneywiz::SENT_FILE);
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"saved\": false", "\"saved\": true");
    std::fs::write(&path, text).unwrap();
    std::fs::write(&log, "").unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day(2), true, false).unwrap();
    assert_eq!(r.sent, 0, "deleted in MoneyWiz: not sent again unasked");

    let said = cashcast_engine::moneywiz_ledger::ask_resync(
        &home.join("data"),
        Some("Chequing".into()),
        Some(day(1).with_month(9).unwrap()),
        Some(NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()),
        day(3),
    )
    .unwrap();
    assert!(said.contains("Chequing from 2026-09-01 to 2026-09-10"));
    let r = moneywiz::sync(&db, &exports, &cfg, &home, day(3), true, false).unwrap();
    assert_eq!(
        r.sent, 2,
        "PHONE CO (09-05) and the GROCER fix (09-02) only"
    );
    let sent = std::fs::read_to_string(&log).unwrap();
    assert!(sent.contains("amount=80.00") && sent.contains("memo=cashcast%20fix%20%23101"));
    assert!(
        !sent.contains("1800") && !sent.contains("toAccount"),
        "nothing after 09-10 or in Visa"
    );
    assert!(ledger_json(&home)["recheck"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(home);
}

/// An account whose plan looks wrong is held: none of its links are sent,
/// and the report says why. Other accounts still sync.
#[test]
fn the_gate_holds_an_account_whose_plan_looks_wrong() {
    let (home, db, cfg) = setup("gate");
    let (_app, log) = fake_app(&home);
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (30,14,'Amex','CAD',0.0)",
        [],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/amex.csv"),
        "Date,Account,Description,Amount\n2026-09-04,Amex,BIG CANARY09,-6000.00\n",
    )
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    // A resync asked for every account: the held account keeps its window.
    cashcast_engine::moneywiz_ledger::ask_resync(&home.join("data"), None, None, None, today)
        .unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    let windows = ledger_json(&home)["recheck"].as_array().unwrap().clone();
    assert_eq!(windows.len(), 1, "{windows:?}");
    assert_eq!(windows[0]["account"], "Amex");
    assert!(r.report.contains(
        "- Amex: no bank balance to check, and the sync would move MoneyWiz by -6,000.00"
    ));
    assert_eq!(r.sent, 5, "Chequing and Visa still sync");
    assert!(!std::fs::read_to_string(&log).unwrap().contains("6000"));
    let l = ledger_json(&home);
    assert_eq!(
        l["rows"]
            .as_object()
            .unwrap()
            .values()
            .filter(|r| r["status"] == "held")
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(home);
}

/// An opening balance can stand in for rows no export held yet (the
/// opening-balance fix set it to the balance read on the site). When the
/// real rows arrive, the sync moves MoneyWiz away from that balance, but the
/// opening-balance fix brings it back to about 0: not a wrong plan, so the
/// account is not held. An opening balance that does not explain the
/// difference still holds it.
#[test]
fn rows_that_replace_a_stand_in_opening_balance_are_not_held() {
    let (home, db, _) = setup("stand-in");
    let (_app, _log) = fake_app(&home);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (30,11,'Savings','CAD',6000.0),(31,11,'Spare','CAD',1500.0);",
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount\n2026-09-04,Savings,DEPOSIT CANARY21,6000.00\n2026-09-04,Spare,DEPOSIT CANARY22,6000.00\n",
    )
    .unwrap();
    let cfg = config::parse(&format!(
        "{}[moneywiz.balances]\n\"Savings\" = \"6000.00@2026-09-30\"\n\"Spare\" = \"1500.00@2026-09-30\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    assert!(
        !r.report.contains("- Savings:"),
        "the rows replace the stand-in opening balance: {}",
        r.report
    );
    assert!(
        r.report.contains("- Spare: the sync moves MoneyWiz away"),
        "an opening balance that does not explain it still holds: {}",
        r.report
    );
    assert!(!r.report.contains("CANARY"));
    let _ = std::fs::remove_dir_all(home);
}

/// The owner squares a balance with a MoneyWiz reconcile row instead of the
/// missing rows. When the sync adds those rows, it offsets the reconcile
/// rows of the synced period (a correction; nothing is deleted), so the
/// money is not counted twice.
#[test]
fn reconcile_rows_in_the_synced_period_are_offset() {
    let (home, db, cfg) = setup("reconcile");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO Z_PRIMARYKEY VALUES (49,'ReconcileTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (320,49,1,-80.0,0,'');
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (321,49,1,-7.0,0,'');",
    )
    .unwrap();
    c.execute(
        "UPDATE ZSYNCOBJECT SET ZDATE1=?1 WHERE Z_PK=320",
        [ts("2026-09-20")],
    )
    .unwrap();
    c.execute(
        "UPDATE ZSYNCOBJECT SET ZDATE1=?1 WHERE Z_PK=321",
        [ts("2026-08-01")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let cheq = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Chequing")
        .unwrap();
    assert_eq!(
        cheq.reconcile,
        vec![320],
        "only the one inside the synced period"
    );
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    let offset: Vec<&moneywiz::Link> = links.iter().filter(|l| l.kind == "reconcile").collect();
    assert_eq!(offset.len(), 1);
    assert!(
        offset[0].url.contains("amount=80.00")
            && offset[0].url.contains("memo=cashcast%20fix%20%23320")
    );
    let report = moneywiz::sync(
        &db,
        &exports,
        &cfg,
        &home,
        NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
        false,
        false,
    )
    .unwrap()
    .report;
    assert!(report.contains("1 reconcile rows"), "{report}");
    // In "list" mode nothing is corrected, the offset included.
    assert!(
        moneywiz::links_for(&store, &exports, &plan, "list")
            .iter()
            .all(|l| l.kind != "reconcile")
    );
    let _ = std::fs::remove_dir_all(home);
}

/// The owner typed one MoneyWiz row for several purchases. Loose matching
/// pairs it with the bank rows whose sum it is, so none of them is added
/// again and no fix is made.
#[test]
fn loose_matching_pairs_one_moneywiz_row_with_the_bank_rows_it_sums() {
    let (home, db, _) = setup("loose-group");
    std::fs::write(
        home.join("inbox/visa-more.csv"),
        "Date,Account,Description,Amount,Balance\n\
         2026-09-15,Visa,SHOP A CANARY21,-40.00,\n\
         2026-09-16,Visa,SHOP B CANARY22,-35.00,\n\
         2026-09-17,Visa,SHOP C CANARY23,-25.50,\n",
    )
    .unwrap();
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (320,48,2,-100.00,?1,'WEEK CANARYMW8')",
        rusqlite::params![ts("2026-09-16")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let cfg = config::parse(&format!(
        "{}match = \"loose\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let visa = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Visa")
        .unwrap();
    assert!(visa.matched_mw.contains(&320), "the sum row pairs");
    let text = |i: &usize| exports.txns[*i].text.clone();
    assert!(
        !visa.add.iter().map(text).any(|t| t.starts_with("SHOP")),
        "none of its rows is added again"
    );
    assert!(!visa.fix.iter().any(|f| f.0 == 320), "no fix: within 1 %");
    let _ = std::fs::remove_dir_all(home);
}

/// Loose mode: the owner typed one rough row where the bank lists two that
/// match nothing. The bank balance shows MoneyWiz already holds that money,
/// so the rough row is offset as the two bank rows are added: the balance
/// stays right. Strict mode only lists it.
#[test]
fn loose_offsets_a_stand_in_row_when_the_bank_balance_proves_it() {
    let (home, db, _) = setup("stand-in-loose");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (40,14,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (340,48,40,-105.00,?1,'LUMP CANARYMW7')",
        rusqlite::params![ts("2026-09-12")],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount,Balance\n\
         2026-09-01,Savings,FEE CANARY31,-60.00,\n\
         2026-09-20,Savings,FEE CANARY32,-45.00,895.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let savings = |extra: &str| {
        let cfg = config::parse(&format!(
            "{}{extra}",
            CONFIG.replace("DB", &db.display().to_string())
        ))
        .unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        plan.accounts
            .into_iter()
            .find(|a| a.export_account == "Savings")
            .unwrap()
    };
    let strict = savings("");
    assert_eq!(
        (strict.only_in_moneywiz.clone(), strict.stand_in.len()),
        (vec![340], 0)
    );
    let loose = savings("match = \"loose\"\n");
    assert_eq!(loose.add.len(), 2);
    assert_eq!(loose.stand_in, vec![340], "offset: the balance proves it");
    assert!(loose.only_in_moneywiz.is_empty());
    // List mode sends no offsets: the row stays listed.
    let listed = savings("match = \"loose\"\nfixes = \"list\"\n");
    assert_eq!(
        (listed.only_in_moneywiz, listed.stand_in.len()),
        (vec![340], 0)
    );
    // The report's "Only in MoneyWiz" cell shows the offset row, not 0.
    let cfg = config::parse(&format!(
        "{}match = \"loose\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    let row = r
        .report
        .lines()
        .find(|l| l.starts_with("| Savings |"))
        .unwrap();
    assert!(row.contains("| 1 offset |"), "{row}");
    let _ = std::fs::remove_dir_all(home);
}

/// Loose ties (same amounts, same gaps) are paired in date order instead of
/// left for the owner: either pairing moves the balance the same.
#[test]
fn loose_ties_pair_in_date_order() {
    let (home, db, _) = setup("loose-ties");
    std::fs::write(
        home.join("inbox/visa-ties.csv"),
        "Date,Account,Description,Amount,Balance\n\
         2026-09-20,Visa,COFFEE CANARY41,-4.00,\n\
         2026-09-22,Visa,COFFEE CANARY42,-4.00,\n",
    )
    .unwrap();
    let c = Connection::open(&db).unwrap();
    for (pk, date) in [(350, "2026-09-21"), (351, "2026-09-23")] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,2,-4.20,?2,'COFFEE CANARYMW6')",
            rusqlite::params![pk, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let plan_for = |extra: &str| {
        let cfg = config::parse(&format!(
            "{}{extra}",
            CONFIG.replace("DB", &db.display().to_string())
        ))
        .unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        let visa = plan
            .accounts
            .into_iter()
            .find(|a| a.export_account == "Visa")
            .unwrap();
        let fixes: Vec<(i64, String)> = visa
            .fix
            .iter()
            .map(|&(pk, i)| (pk, exports.txns[i].text.clone()))
            .collect();
        (visa.conflicts.len(), fixes)
    };
    let (conflicts, fixes) = plan_for("match = \"loose\"\n");
    assert_eq!(conflicts, 0);
    assert!(
        fixes.contains(&(350, "COFFEE CANARY41".into())),
        "{fixes:?}"
    );
    assert!(
        fixes.contains(&(351, "COFFEE CANARY42".into())),
        "{fixes:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// A MoneyWiz dialog (or a slow MoneyWiz in the background) leaves a batch
/// unsaved: the sync stops there. The batch stays recorded as sent, so a
/// rerun the same day does not send it twice; `--resend` retries it.
#[test]
fn a_batch_moneywiz_saved_none_of_stops_the_run() {
    let (home, db, cfg) = setup("blocked");
    let (_app, log) = fake_app(&home);
    unsafe { std::env::set_var("CASHCAST_MONEYWIZ_BATCH", "2") };
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    let again = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    let lines = std::fs::read_to_string(&log).unwrap().lines().count();
    let retry = moneywiz::sync(&db, &exports, &cfg, &home, today, true, true).unwrap();
    unsafe { std::env::remove_var("CASHCAST_MONEYWIZ_BATCH") };
    assert!(
        r.report
            .contains("Stopped: MoneyWiz has saved none of the last 2 links yet"),
        "{}",
        r.report
    );
    assert_eq!(r.sent, 2);
    assert_eq!(
        lines, 4,
        "the rerun sends only the next batch, not the first again"
    );
    assert!(again.report.contains("Stopped"));
    assert!(retry.sent >= 2, "--resend retries today's unsaved links");
    let _ = std::fs::remove_dir_all(home);
}

/// A link sent today can be marked "seen in MoneyWiz" while MoneyWiz does
/// not hold it: a confirm that ran while MoneyWiz was still writing saw the
/// link with another amount. `--resend`, which the owner runs only after a
/// quit and a dry run show the row missing, sends it again the same day.
#[test]
fn resend_retries_a_link_sent_today_that_was_marked_seen() {
    let (home, db, cfg) = setup("resend-marked");
    let (_app, log) = fake_app(&home);
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let first = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    assert!(first.sent > 0, "{}", first.report);
    // Mark every sent link seen, as a confirm with other ids would.
    let data = home.join("data");
    let mut sent = moneywiz::SentList::load(&data, today, false).unwrap();
    sent.mark_saved(&[]);
    sent.write(&data).unwrap();
    let before = std::fs::read_to_string(&log).unwrap().lines().count();
    let retry = moneywiz::sync(&db, &exports, &cfg, &home, today, true, true).unwrap();
    assert_eq!(
        retry.sent, first.sent,
        "--resend sends every missing link again: {}",
        retry.report
    );
    let after = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(after - before, first.sent);
    let _ = std::fs::remove_dir_all(home);
}

/// A same-payee row far off in amount is another transaction, not a typo:
/// a 100 transfer must not pair with the bank's 3,000 deposit (the sync
/// would then "fix" it by thousands and break the balance history).
#[test]
fn same_payee_far_amount_does_not_pair() {
    let (home, db, cfg) = setup("far-amount");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (310,38,1,100.0,?1,'PAYROLL CANARYMW9')",
        rusqlite::params![ts("2026-09-30")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let cheq = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Chequing")
        .unwrap();
    assert!(
        !cheq.fix.iter().any(|f| f.0 == 310),
        "a 100 row must not take the bank's 3,000"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// A row the owner typed a few days off from the bank moves to the bank's
/// day (`edit --fixes`), so the balance history matches day by day without
/// a correction row.
#[test]
fn fixes_move_a_paired_row_to_the_banks_day() {
    use cashcast_engine::moneywiz_edit;
    let (home, db, cfg) = setup("redate");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (320,48,'g320',1,-80.0,?1,'PHONE CO CANARYMW8')",
        rusqlite::params![ts("2026-09-03")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let r = moneywiz_edit::resolve(
        &store,
        &moneywiz_edit::EditsFile::default(),
        Some((&exports.txns, &plan)),
    );
    let e = r
        .plan
        .rows
        .iter()
        .find(|e| e.gid == "g320")
        .expect("a date edit");
    assert_eq!(e.date.as_deref(), Some("2026-09-05"));
    assert_eq!(e.amount, None);
    let _ = std::fs::remove_dir_all(home);
}

/// Each side of a transfer moves to its own bank's day too.
#[test]
fn fixes_move_a_transfer_side_to_the_banks_day() {
    use cashcast_engine::moneywiz_edit;
    let (home, db, cfg) = setup("redate-transfer");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (321,46,'g321',1,-500.0,?1,'VISA PAYMENT CANARYMW7')",
        rusqlite::params![ts("2026-09-13")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let r = moneywiz_edit::resolve(
        &store,
        &moneywiz_edit::EditsFile::default(),
        Some((&exports.txns, &plan)),
    );
    let e = r
        .plan
        .rows
        .iter()
        .find(|e| e.gid == "g321")
        .expect("a date edit");
    assert_eq!(e.date.as_deref(), Some("2026-09-15"));
    let _ = std::fs::remove_dir_all(home);
}

/// `edit --fixes` folds cashcast's corrections away: the correction of a
/// plain row becomes its amount and goes; a stand-in and its offset both go.
#[test]
fn fixes_fold_corrections_into_their_rows() {
    use cashcast_engine::moneywiz_edit;
    let (home, db, cfg) = setup("fold");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZNOTES1) VALUES
           (330,48,1,-5.0,0,'',  'cashcast fix #101'),
           (331,48,1,12.0,0,'',  'cashcast fix #102');
         UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;",
    )
    .unwrap();
    for pk in [330, 331] {
        c.execute(
            "UPDATE ZSYNCOBJECT SET ZDATE1 = ?1 WHERE Z_PK = ?2",
            rusqlite::params![ts("2026-09-02"), pk],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let r = moneywiz_edit::resolve(
        &store,
        &moneywiz_edit::EditsFile::default(),
        Some((&exports.txns, &plan)),
    );
    let get = |g: &str| r.plan.rows.iter().find(|e| e.gid == g);
    assert_eq!(get("g101").and_then(|e| e.amount), Some(-45.0));
    assert!(get("g330").is_some_and(|e| e.delete));
    assert!(get("g102").is_some_and(|e| e.delete), "the stand-in goes");
    assert!(get("g331").is_some_and(|e| e.delete), "and its offset");
    let _ = std::fs::remove_dir_all(home);
}

/// A row with two or more corrections folds them all in one run: the row
/// takes the sum, every correction goes, and a stand-in whose corrections
/// sum to its negation goes whole.
#[test]
fn fixes_fold_every_correction_of_a_row_in_one_run() {
    use cashcast_engine::moneywiz_edit;
    let (home, db, cfg) = setup("fold-many");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZNOTES1) VALUES
           (330,48,1,-5.0,0,'',  'cashcast fix #101'),
           (332,48,1,-3.0,0,'',  'cashcast fix #101'),
           (331,48,1,7.0,0,'',   'cashcast fix #102'),
           (333,48,1,5.0,0,'',   'cashcast fix #102');
         UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;",
    )
    .unwrap();
    for pk in [330, 331, 332, 333] {
        c.execute(
            "UPDATE ZSYNCOBJECT SET ZDATE1 = ?1 WHERE Z_PK = ?2",
            rusqlite::params![ts("2026-09-02"), pk],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let r = moneywiz_edit::resolve(
        &store,
        &moneywiz_edit::EditsFile::default(),
        Some((&exports.txns, &plan)),
    );
    let get = |g: &str| r.plan.rows.iter().find(|e| e.gid == g);
    assert_eq!(get("g101").and_then(|e| e.amount), Some(-48.0));
    for g in ["g330", "g332"] {
        assert!(get(g).is_some_and(|e| e.delete), "{g} folds into its row");
    }
    for g in ["g102", "g331", "g333"] {
        assert!(
            get(g).is_some_and(|e| e.delete),
            "{g}: the stand-in goes whole"
        );
    }
    assert_eq!(
        moneywiz_edit::kept_corrections(&store, &plan),
        vec![],
        "no correction is kept"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// The weekly sync is repeat-safe: once a fix plan is applied, the next
/// plan has no amount, date, delete or opening change left.
#[test]
fn a_second_fix_run_changes_nothing() {
    use cashcast_engine::moneywiz_edit;
    let (home, db, cfg) = setup("again");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZNOTES1) VALUES
           (330,48,1,-8.0,0,'',  'cashcast fix #101'),
           (332,48,1,3.0,0,'',   'cashcast fix #101'),
           (331,48,1,12.0,0,'',  'cashcast fix #102');
         UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;",
    )
    .unwrap();
    for pk in [330, 331, 332] {
        c.execute(
            "UPDATE ZSYNCOBJECT SET ZDATE1 = ?1 WHERE Z_PK = ?2",
            rusqlite::params![ts("2026-09-02"), pk],
        )
        .unwrap();
    }
    drop(c);
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let fix_plan = |db: &std::path::Path| {
        let store = moneywiz::open(db).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        moneywiz_edit::resolve(
            &store,
            &moneywiz_edit::EditsFile::default(),
            Some((&exports.txns, &plan)),
        )
        .plan
    };
    let first = fix_plan(&db);
    let changes = |p: &moneywiz_edit::WriterPlan| {
        p.rows
            .iter()
            .filter(|e| e.delete || e.amount.is_some() || e.date.is_some())
            .count()
            + p.accounts.len()
            + p.trades.len()
    };
    assert!(changes(&first) > 0, "the fixture needs fixes");
    // Apply the plan the way the writer does.
    let c = Connection::open(&db).unwrap();
    for e in &first.rows {
        if e.delete {
            c.execute("DELETE FROM ZSYNCOBJECT WHERE ZGID = ?1", [&e.gid])
                .unwrap();
            continue;
        }
        if let Some(a) = e.amount {
            c.execute(
                "UPDATE ZSYNCOBJECT SET ZAMOUNT1 = ?1 WHERE ZGID = ?2",
                rusqlite::params![a, e.gid],
            )
            .unwrap();
        }
        if let Some(d) = &e.date {
            c.execute(
                "UPDATE ZSYNCOBJECT SET ZDATE1 = ?1 WHERE ZGID = ?2",
                rusqlite::params![ts(d), e.gid],
            )
            .unwrap();
        }
    }
    for a in &first.accounts {
        c.execute(
            "UPDATE ZSYNCOBJECT SET ZOPENINGBALANCE = ?1 WHERE ZGID = ?2",
            rusqlite::params![a.opening, a.gid],
        )
        .unwrap();
    }
    drop(c);
    let (text, bad) = moneywiz_edit::verify(&moneywiz::open(&db).unwrap(), &first);
    assert_eq!(bad, 0, "{text}");
    let second = fix_plan(&db);
    assert_eq!(changes(&second), 0, "a second run must change nothing");
    let _ = std::fs::remove_dir_all(home);
}

/// `diagnose` compares a mapped investment account's shares with the
/// broker's trades, symbol names compared without case or extra spaces.
#[test]
fn investments_compare_shares_with_the_broker() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let (home, db, _) = setup("invest");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (9,16,'Broker TFSA','CAD',0.0);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE) VALUES (340,60,9,-100.0,?1,'canary  fund',2.0,50.0)",
        rusqlite::params![ts("2026-09-01")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let day = |d: &str| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap();
    let trade = |d: &str, shares: f64| Trade {
        date: day(d),
        symbol: "CANARY FUND".into(),
        bucket: "TFSA",
        format: "broker-csv".into(),
        account: String::new(),
        shares,
        cad: Some(shares * 60.0),
        cash: Some(String::new()),
    };
    let trades = [trade("2026-09-01", 2.0), trade("2026-10-01", 1.0)];
    let map = [("Broker TFSA".to_string(), "broker-csv:TFSA".to_string())].into();
    let rows = moneywiz_invest::compare(&store, &trades, &map);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(
        (r.moneywiz_shares, r.broker_shares, r.missing),
        (2.0, 3.0, 1)
    );
    assert_eq!(r.last_price.map(|p| p.1), Some(60.0));
    assert!(moneywiz_invest::unmapped(&trades, &map).is_empty());
    assert!(moneywiz_invest::ambiguous(&trades, &map).is_empty());
    // Two broker accounts sharing the last four characters: the mapping
    // takes both, and the shortest ends that tell them apart are shown.
    let mut a = trade("2026-09-01", 2.0);
    a.account = "CANARYX16CAD".into();
    let mut b = trade("2026-09-02", 1.0);
    b.account = "CANARYY26CAD".into();
    let same = [(
        "Broker TFSA".to_string(),
        "broker-csv:TFSA:6CAD".to_string(),
    )]
    .into();
    let amb = moneywiz_invest::ambiguous(&[a, b], &same);
    assert_eq!(
        amb,
        [(
            "Broker TFSA".to_string(),
            vec![("16CAD".to_string(), 1), ("26CAD".to_string(), 1)]
        )]
    );
    // The account has no sync id here, so nothing is written; with one,
    // the missing trade is written because it makes the totals equal.
    assert!(moneywiz_invest::trade_adds(&store, &rows).is_empty());
    // A broker total below zero (a partial history) writes nothing.
    let partial = [trade("2026-09-01", 2.0), trade("2026-10-01", -5.0)];
    let rows = moneywiz_invest::compare(&store, &partial, &map);
    assert!(rows[0].broker_shares < 0.0);
    assert!(moneywiz_invest::trade_adds(&store, &rows).is_empty());
    // A missing trade with no cash amount is named in the report.
    let mut no_cash = trade("2026-10-01", 1.0);
    no_cash.cad = None;
    let rows = moneywiz_invest::compare(&store, &[trade("2026-09-01", 2.0), no_cash], &map);
    assert!(moneywiz_invest::render(&rows).contains("1 missing broker trades have no CAD amount"));
    // Verify counts trades: a trade the store lacks is one failed change.
    let plan = cashcast_engine::moneywiz_edit::WriterPlan {
        trades: vec![cashcast_engine::moneywiz_edit::TradeAdd {
            account_gid: "no-such-account".into(),
            sell: false,
            date: "2026-10-01".into(),
            symbol: "CANARY FUND".into(),
            shares: 1.0,
            price: 60.0,
            amount: -60.0,
        }],
        ..Default::default()
    };
    let (text, bad) = cashcast_engine::moneywiz_edit::verify(&store, &plan);
    assert_eq!(bad, 1);
    assert!(text.starts_with("Verified 0 of 1 changes"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn export_aliases_join_files_and_count_overlap_once() {
    use cashcast_engine::export::{Balance, Export, Txn};
    fn row(account: &str, day: &str, cents: i64) -> Txn {
        Txn {
            file: 0,
            account: account.into(),
            transfer: String::new(),
            category: Vec::new(),
            category_from_file: false,
            hint: cashcast_engine::text::Hint::None,
            payee_key: 0,
            date: NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap(),
            cents,
            currency: "CAD".into(),
            fingerprint: cents as u64,
            text: "CANARYALIAS".into(),
        }
    }
    let mut own = Export::empty("card.qfx", "ofx");
    own.txns = vec![
        row("Card 1", "2026-09-18", -100),
        row("Card 1", "2026-11-02", -200),
    ];
    own.balances.insert(
        "Card 1".into(),
        Balance {
            cents: -300,
            as_of: None,
            source: "ofx",
        },
    );
    let mut csv = Export::empty("activity1.csv", "td-csv");
    csv.txns = vec![
        row("activity1", "2026-05-01", -50),  // before: kept
        row("activity1", "2026-10-01", -100), // the qfx covers it: dropped
        row("activity1", "2026-11-05", -70),  // after: kept
    ];
    csv.balances.insert(
        "activity1".into(),
        Balance {
            cents: 1,
            as_of: None,
            source: "csv",
        },
    );
    let mut other = Export::empty("other.csv", "csv");
    other.txns = vec![row("Chequing", "2026-10-01", 5)];
    let mut files = vec![own, csv, other];
    let aliases = [("activity*".to_string(), "Card 1".to_string())]
        .into_iter()
        .collect();
    inbox::apply_aliases(&mut files, &aliases);
    let days: Vec<String> = files[1]
        .txns
        .iter()
        .map(|t| format!("{} {}", t.account, t.date))
        .collect();
    assert_eq!(days, ["Card 1 2026-05-01", "Card 1 2026-11-05"]);
    assert_eq!(files[0].balances["Card 1"].cents, -300, "own balance stays");
    assert!(!files[1].balances.contains_key("activity1"));
    assert!(
        !files[1].balances.contains_key("Card 1"),
        "an alias balance never stands in for the account's own"
    );
    assert_eq!(files[2].txns[0].account, "Chequing");
}

/// Two downloads of one account, each named by its own day, both renamed to
/// the same account: a row both hold is one row, not two. A charge the bank
/// lists twice in one file stays twice.
#[test]
fn overlapping_alias_downloads_keep_one_copy_of_each_row() {
    use cashcast_engine::export::{Export, Txn};
    fn row(account: &str, day: &str, cents: i64, fingerprint: u64) -> Txn {
        Txn {
            file: 0,
            account: account.into(),
            transfer: String::new(),
            category: Vec::new(),
            category_from_file: false,
            hint: cashcast_engine::text::Hint::None,
            payee_key: 0,
            date: NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap(),
            cents,
            currency: "CAD".into(),
            fingerprint,
            text: "CANARYALIASDUP".into(),
        }
    }
    // The file name is part of each row's key, so the copies differ.
    let mut older = Export::empty("acts-2026-11-08.csv", "csv");
    older.txns = vec![
        row("acts-2026-11-08", "2026-11-01", 731, 1),
        row("acts-2026-11-08", "2026-11-05", 9_900, 2),
    ];
    let mut newer = Export::empty("acts-2026-11-16.csv", "csv");
    newer.txns = vec![
        row("acts-2026-11-16", "2026-11-01", 731, 11),
        row("acts-2026-11-16", "2026-11-05", 9_900, 12),
        row("acts-2026-11-16", "2026-11-08", -300, 13),
        row("acts-2026-11-16", "2026-11-08", -300, 14),
    ];
    let mut files = vec![older, newer];
    let aliases = [("acts-*".to_string(), "Canary Chequing".to_string())]
        .into_iter()
        .collect();
    inbox::apply_aliases(&mut files, &aliases);
    let merged = inbox::Inbox::merge(files, Vec::new());
    let mut rows: Vec<String> = merged
        .txns
        .iter()
        .map(|t| format!("{} {} {}", t.account, t.date, t.cents))
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            "Canary Chequing 2026-11-01 731",
            "Canary Chequing 2026-11-05 9900",
            "Canary Chequing 2026-11-08 -300",
            "Canary Chequing 2026-11-08 -300",
        ]
    );
    assert_eq!(merged.txns.iter().map(|t| t.cents).sum::<i64>(), 10_031);
}

#[test]
fn bank_wins_offsets_every_unpaired_row_without_proof() {
    let (home, db, _) = setup("bank-wins");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (40,14,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    // A rough entry far bigger than the rows the bank adds: no proof.
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (341,48,40,-300.00,?1,'ROUGH CANARYMW8')",
        rusqlite::params![ts("2026-09-12")],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount,Balance\n\
         2026-09-01,Savings,FEE CANARY33,-60.00,\n\
         2026-09-20,Savings,FEE CANARY34,-45.00,\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let savings = |extra: &str| {
        let cfg = config::parse(&format!(
            "{}match = \"loose\"\n{extra}",
            CONFIG.replace("DB", &db.display().to_string())
        ))
        .unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        plan.accounts
            .into_iter()
            .find(|a| a.export_account == "Savings")
            .unwrap()
    };
    let proof = savings("");
    assert_eq!(
        (proof.only_in_moneywiz.clone(), proof.stand_in.len()),
        (vec![341], 0)
    );
    let wins = savings("bank_wins = true\n");
    assert_eq!(wins.stand_in, vec![341], "the bank wins");
    assert!(wins.only_in_moneywiz.is_empty());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_balance_older_than_the_last_row_checks_once_nothing_changes() {
    let (home, db, _) = setup("older-balance");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (40,14,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    for (pk, amount, day) in [(342, -60.0, "2026-09-01"), (343, -45.0, "2026-09-20")] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,40,?2,?3,'FEE CANARYMW9')",
            rusqlite::params![pk, amount, ts(day)],
        )
        .unwrap();
    }
    drop(c);
    // The balance sits on the first row only, before the last row's day.
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount,Balance\n\
         2026-09-01,Savings,FEE CANARY35,-60.00,940.00\n\
         2026-09-20,Savings,FEE CANARY36,-45.00,\n",
    )
    .unwrap();
    let cfg = config::parse(&CONFIG.replace("DB", &db.display().to_string())).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    // The 940.00 carries forward by the later fee: 895.00 on the last day.
    assert!(
        r.report.contains("matches the bank (895.00)"),
        "{}",
        r.report
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_invested_account_leaves_trade_legs_to_the_investments_fix() {
    let (home, db, _) = setup("invested");
    Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (41,14,'Kraken','CAD',0.0)",
            [],
        )
        .unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ledger-kraken.csv"),
        home.join("inbox/ledger-kraken.csv"),
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let rows = |extra: &str| {
        let cfg = config::parse(&format!(
            "{}[moneywiz.accounts]\n\"Kraken\" = \"Kraken CAD\"\n{extra}",
            CONFIG.replace("DB", &db.display().to_string())
        ))
        .unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        plan.accounts
            .into_iter()
            .find(|a| a.export_account == "Kraken CAD")
            .unwrap()
            .bank_rows
    };
    assert_eq!(rows(""), 4, "a deposit and three trade legs");
    assert_eq!(
        rows("[moneywiz.investments]\n\"Kraken\" = \"kraken-ledger-csv:non-registered\"\n"),
        1,
        "only the deposit: trades come from the investments fix"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// The bank wins for investments too: a hand-typed MoneyWiz trade no broker
/// trade pairs with is deleted and the broker's trade written instead, but
/// only when that makes MoneyWiz's shares the broker's.
#[test]
fn bank_wins_replaces_an_unpaired_moneywiz_trade_with_the_brokers() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let (home, db, _) = setup("invest-replace");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Broker RSU','CAD',0.0,'acct-canary');",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE,ZGID) VALUES (340,60,9,-105.0,?1,'CANARY',2.1,50.0,'row-canary')",
        rusqlite::params![ts("2026-09-01")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let trade = Trade {
        date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        symbol: "CANARY".into(),
        bucket: "non-registered",
        format: "broker-csv".into(),
        account: String::new(),
        shares: 2.0,
        cad: Some(120.0),
        cash: Some(String::new()),
    };
    let map = [(
        "Broker RSU".to_string(),
        "broker-csv:non-registered".to_string(),
    )]
    .into();
    let rows = moneywiz_invest::compare(&store, &[trade], &map);
    assert_eq!(rows[0].missing, 1);
    // Without bank_wins nothing changes: adding would give 4.1 shares.
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, false);
    assert!(adds.is_empty() && deletes.is_empty());
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert_eq!(adds.len(), 1);
    assert_eq!((adds[0].shares, adds[0].price), (2.0, 60.0));
    assert_eq!(deletes.len(), 1);
    assert!(deletes[0].delete);
    assert_eq!(deletes[0].gid, "row-canary");
    assert_eq!(deletes[0].expect_date, "2026-09-01");
    let _ = std::fs::remove_dir_all(home);
}

/// When a paired MoneyWiz trade is off by a little, patching cannot make the
/// shares equal; with bank_wins every row of the symbol is replaced.
#[test]
fn bank_wins_replaces_every_row_when_a_paired_trade_is_off() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let (home, db, _) = setup("invest-replace-all");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Broker RSU','CAD',0.0,'acct-canary');",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE,ZGID) VALUES (340,60,9,-100.0,?1,'CANARY',2.0,50.0,'row-canary')",
        rusqlite::params![ts("2026-09-01")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let trade = |m: u32, shares: f64| Trade {
        date: NaiveDate::from_ymd_opt(2026, m, 1).unwrap(),
        symbol: "CANARY".into(),
        bucket: "non-registered",
        format: "broker-csv".into(),
        account: String::new(),
        shares,
        cad: Some(shares * 50.0),
        cash: Some(String::new()),
    };
    let map = [(
        "Broker RSU".to_string(),
        "broker-csv:non-registered".to_string(),
    )]
    .into();
    let rows = moneywiz_invest::compare(&store, &[trade(9, 2.03), trade(10, 1.0)], &map);
    assert_eq!(rows[0].missing, 1);
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, false);
    assert!(adds.is_empty() && deletes.is_empty());
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    let shares: Vec<f64> = adds.iter().map(|a| a.shares).collect();
    assert_eq!(shares, [2.03, 1.0]);
    assert_eq!(
        deletes.iter().map(|d| d.gid.as_str()).collect::<Vec<_>>(),
        ["row-canary"]
    );
    // No broker trade missing: the only paired trade is off by a little
    // (within the 2 % pairing room). The shares still differ, so the bank wins.
    let rows = moneywiz_invest::compare(&store, &[trade(9, 2.03)], &map);
    assert_eq!(rows[0].missing, 0);
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert_eq!(adds.iter().map(|a| a.shares).collect::<Vec<_>>(), [2.03]);
    assert_eq!(deletes.len(), 1);
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, false);
    assert!(adds.is_empty() && deletes.is_empty());
    // Same shares and day, other cash: the cost base (and the gain) is off,
    // so the row is replaced by the broker's trade.
    let mut dear = trade(9, 2.0);
    dear.cad = Some(120.0);
    let rows = moneywiz_invest::compare(&store, &[dear], &map);
    assert_eq!((rows[0].missing, rows[0].off_rows), (0, 1));
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert_eq!(adds.iter().map(|a| a.price).collect::<Vec<_>>(), [60.0]);
    assert_eq!(deletes.len(), 1);
    // The exact broker trade is not off.
    let rows = moneywiz_invest::compare(&store, &[trade(9, 2.0)], &map);
    assert_eq!(rows[0].off_rows, 0);
    let _ = std::fs::remove_dir_all(home);
}

/// Rows pair within 0.0001 shares each, but a gap of 0.00006 still shows at
/// four decimals (2.0001 against 2.0000): with bank_wins the symbol's rows are
/// replaced. A float-noise gap is left alone, so a second run changes nothing.
#[test]
fn bank_wins_replaces_rows_when_shares_differ_at_four_decimals() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let (home, db, _) = setup("invest-tiny-gap");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Broker RSU','CAD',0.0,'acct-canary');",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE,ZGID) VALUES (340,60,9,-100.0,?1,'CANARY',2.00006,50.0,'row-canary')",
        rusqlite::params![ts("2026-09-01")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let trade = |shares: f64| Trade {
        date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        symbol: "CANARY".into(),
        bucket: "non-registered",
        format: "broker-csv".into(),
        account: String::new(),
        shares,
        cad: Some(100.0),
        cash: Some(String::new()),
    };
    let map = [(
        "Broker RSU".to_string(),
        "broker-csv:non-registered".to_string(),
    )]
    .into();
    let rows = moneywiz_invest::compare(&store, &[trade(2.0)], &map);
    assert_eq!((rows[0].missing, rows[0].off_rows), (0, 0), "the row pairs");
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert_eq!(adds.iter().map(|a| a.shares).collect::<Vec<_>>(), [2.0]);
    assert_eq!(
        deletes.iter().map(|d| d.gid.as_str()).collect::<Vec<_>>(),
        ["row-canary"]
    );
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, false);
    assert!(adds.is_empty() && deletes.is_empty());
    // Float noise is not a gap.
    let rows = moneywiz_invest::compare(&store, &[trade(2.0000599)], &map);
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert!(adds.is_empty() && deletes.is_empty());
    let _ = std::fs::remove_dir_all(home);
}

/// A symbol only MoneyWiz holds, in an account the broker file covers, is a
/// stale hand entry: with bank_wins its rows are deleted. Without bank_wins,
/// or when the broker has no trades for the account, nothing changes.
#[test]
fn bank_wins_deletes_a_symbol_only_moneywiz_holds() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let (home, db, _) = setup("invest-stale-symbol");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Broker RSU','CAD',0.0,'acct-canary');",
    )
    .unwrap();
    for (pk, sym, gid) in [
        (340, "CANARYKEEP", "row-keep"),
        (341, "CANARYSTALE", "row-stale"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE,ZGID) VALUES (?1,60,9,-100.0,?2,?3,2.0,50.0,?4)",
            rusqlite::params![pk, ts("2026-09-01"), sym, gid],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let trade = Trade {
        date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        symbol: "CANARYKEEP".into(),
        bucket: "non-registered",
        format: "broker-csv".into(),
        account: String::new(),
        shares: 2.0,
        cad: Some(100.0),
        cash: Some(String::new()),
    };
    let map = [(
        "Broker RSU".to_string(),
        "broker-csv:non-registered".to_string(),
    )]
    .into();
    let rows = moneywiz_invest::compare(&store, &[trade], &map);
    let (_, deletes) = moneywiz_invest::trade_fixes(&store, &rows, false);
    assert!(deletes.is_empty());
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert!(adds.is_empty());
    let gids: Vec<&str> = deletes.iter().map(|d| d.gid.as_str()).collect();
    assert_eq!(gids, ["row-stale"]);
    // No broker trades for the account: nothing is deleted.
    let rows = moneywiz_invest::compare(&store, &[], &map);
    let (_, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert!(deletes.is_empty());
    let _ = std::fs::remove_dir_all(home);
}

/// MoneyWiz takes trades only in an investment account: a broker mapped to
/// a cash account gets no trades, so the writer never refuses the batch.
#[test]
fn trades_are_planned_only_for_investment_accounts() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let (home, db, _) = setup("invest-cash-account");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'CashAccount',10,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Coin Wallet','CAD',0.0,'acct-canary');",
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let trade = Trade {
        date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        symbol: "CANARY".into(),
        bucket: "non-registered",
        format: "broker-csv".into(),
        account: String::new(),
        shares: 2.0,
        cad: Some(120.0),
        cash: Some(String::new()),
    };
    let map = [(
        "Coin Wallet".to_string(),
        "broker-csv:non-registered".to_string(),
    )]
    .into();
    let rows = moneywiz_invest::compare(&store, &[trade], &map);
    let (adds, deletes) = moneywiz_invest::trade_fixes(&store, &rows, true);
    assert!(adds.is_empty() && deletes.is_empty());
    let _ = std::fs::remove_dir_all(home);
}

/// `--fixes` leaves some `cashcast fix #` rows in place; `diagnose` says why
/// for each, in counts only.
#[test]
fn diagnose_says_why_fixes_keeps_a_correction() {
    use cashcast_engine::{moneywiz_diagnose, moneywiz_edit};
    let (home, db, cfg) = setup("kept");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZNOTES1) VALUES
           (330,48,1,-5.0,0,'','cashcast fix #101'),
           (331,48,1,-1.0,0,'','cashcast fix #999'),
           (333,46,1,-7.0,0,'',NULL),
           (332,48,1,-2.0,0,'','cashcast fix #333'),
           (334,48,1,-3.0,0,'','cashcast fix #103'),
           (335,48,3,-4.0,0,'','cashcast fix #103');
         UPDATE ZSYNCOBJECT SET ZGID = 'g' || Z_PK;",
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let mut kept: Vec<(i64, &str)> = moneywiz_edit::kept_corrections(&store, &plan);
    kept.sort();
    assert_eq!(
        kept,
        [
            (1, moneywiz_edit::KEPT_GONE),
            (1, moneywiz_edit::KEPT_OTHER_ACCOUNT),
            (3, moneywiz_edit::KEPT_NO_BANK),
        ]
    );
    let out = moneywiz_diagnose::render(&store, &exports, &plan);
    assert!(out.contains("## Corrections `--fixes` keeps, and why"));
    assert!(out.contains(&format!("| Chequing | {} | 1 |", moneywiz_edit::KEPT_GONE)));
    assert!(out.contains(&format!("| Cash | {} | 1 |", moneywiz_edit::KEPT_NO_BANK)));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn trade_cash_moves_only_the_accounts_own_currency() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest::trade_cash};
    let t = |shares: f64, cash: Option<&str>| Trade {
        date: chrono::NaiveDate::from_ymd_opt(2024, 12, 14).unwrap(),
        symbol: "CANARYCOIN".into(),
        bucket: "non-registered",
        format: "kraken-ledger-csv".into(),
        account: String::new(),
        shares,
        cad: Some(500.0),
        cash: cash.map(str::to_string),
    };
    assert_eq!(
        trade_cash(&t(2.0, Some("CAD")), "CAD"),
        -500.0,
        "a CAD buy pays CAD"
    );
    assert_eq!(trade_cash(&t(-2.0, Some("")), "CAD"), 500.0, "own currency");
    assert_eq!(
        trade_cash(&t(2.0, Some("USD")), "CAD"),
        0.0,
        "paid from the USD balance"
    );
    assert_eq!(
        trade_cash(&t(2.0, None), "CAD"),
        0.0,
        "coins moved in: no cash"
    );
    assert_eq!(
        trade_cash(&t(-2.0, None), "CAD"),
        0.0,
        "coins sent out: no cash"
    );
}

#[test]
fn a_holding_read_on_the_site_adds_one_fee_trade() {
    use cashcast_engine::{ledger::Trade, moneywiz_invest};
    let trade = |shares: f64| Trade {
        date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        symbol: "CANARYCOIN".into(),
        bucket: "non-registered",
        format: "vault-csv".into(),
        account: String::new(),
        shares,
        cad: Some(shares * 100.0),
        cash: None,
    };
    let map = [("Vault".to_string(), "vault-csv:non-registered".to_string())].into();
    let held = [(
        "Vault".to_string(),
        "canarycoin:0.97@2026-11-08".to_string(),
    )]
    .into();
    let (adds, problems) = moneywiz_invest::holding_adjustments(&[trade(1.0)], &map, &held);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(adds.len(), 1);
    assert!((adds[0].shares + 0.03).abs() < 1e-9);
    // Priced at the last trade (100 each) so MoneyWiz keeps a real price,
    // but no cash moves.
    assert!(
        (adds[0].cad.unwrap() - 3.0).abs() < 1e-9,
        "{:?}",
        adds[0].cad
    );
    assert_eq!(
        moneywiz_invest::trade_cash(&adds[0], "CAD"),
        0.0,
        "a fee moves no cash"
    );
    // Already there: nothing to add.
    let (adds, _) = moneywiz_invest::holding_adjustments(&[trade(0.97)], &map, &held);
    assert!(adds.is_empty());
    // Bad text and an unmapped account are problems, not guesses.
    let bad = [
        ("Vault".to_string(), "soon".to_string()),
        ("Nope".to_string(), "X:1@2026-11-08".to_string()),
    ]
    .into();
    let (adds, problems) = moneywiz_invest::holding_adjustments(&[trade(1.0)], &map, &bad);
    assert!(adds.is_empty());
    assert_eq!(problems.len(), 2);
    assert!(
        !problems.join(" ").contains("0.97"),
        "problems hold no amounts"
    );
}

/// A holding's opening shares count on the MoneyWiz side, and a plan that
/// sets them to 0 is checked against the database.
#[test]
fn fixes_zero_a_holdings_opening_shares() {
    use cashcast_engine::{ledger::Trade, moneywiz_edit, moneywiz_invest};
    let (home, db, _) = setup("invest-opening");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZOPENNINGNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZINVESTMENTACCOUNT INTEGER;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0),(70,'InvestmentHolding',0,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Broker RSU','CAD',0.0,'acct-canary');
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZSYMBOL,ZOPENNINGNUMBEROFSHARES,ZINVESTMENTACCOUNT,ZGID) VALUES (80,70,'CANARY',3.0,9,'holding-canary');",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE,ZGID) VALUES (340,60,9,-100.0,?1,'CANARY',2.0,50.0,'row-canary')",
        rusqlite::params![ts("2026-09-01")],
    )
    .unwrap();
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    assert_eq!(store.holdings.len(), 1);
    assert_eq!(store.holdings[0].gid, "holding-canary");
    let trade = Trade {
        date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        symbol: "CANARY".into(),
        bucket: "non-registered",
        format: "broker-csv".into(),
        account: String::new(),
        shares: 2.0,
        cad: Some(100.0),
        cash: Some(String::new()),
    };
    let map = [(
        "Broker RSU".to_string(),
        "broker-csv:non-registered".to_string(),
    )]
    .into();
    let rows = moneywiz_invest::compare(&store, &[trade], &map);
    assert!(
        (rows[0].moneywiz_shares - 5.0).abs() < 1e-9,
        "opening shares count"
    );
    // A plan to zero it is not done until the database shows 0.
    let plan = moneywiz_edit::WriterPlan {
        contract: 1,
        holdings: vec![moneywiz_edit::HoldingEdit {
            gid: "holding-canary".into(),
            expect_opening: 3.0,
            opening: 0.0,
            price: None,
            expect_price: 0.0,
        }],
        ..Default::default()
    };
    assert_eq!(moneywiz_edit::verify(&store, &plan).1, 1);
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE ZSYNCOBJECT SET ZOPENNINGNUMBEROFSHARES = 0 WHERE Z_PK = 80",
            [],
        )
        .unwrap();
    let store = moneywiz::open(&db).unwrap();
    assert_eq!(moneywiz_edit::verify(&store, &plan).1, 0);
    // A market price is done only when the holding shows it.
    let priced = moneywiz_edit::WriterPlan {
        contract: 1,
        holdings: vec![moneywiz_edit::HoldingEdit {
            gid: "holding-canary".into(),
            expect_opening: 0.0,
            opening: 0.0,
            price: Some(61.5),
            expect_price: 0.0,
        }],
        ..Default::default()
    };
    assert_eq!(moneywiz_edit::verify(&store, &priced).1, 1);
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE ZSYNCOBJECT SET ZPRICEPERSHARE = 61.5 WHERE Z_PK = 80",
            [],
        )
        .unwrap();
    let store = moneywiz::open(&db).unwrap();
    assert_eq!(moneywiz_edit::verify(&store, &priced).1, 0);
    let _ = std::fs::remove_dir_all(home);
}

/// A Wealthsimple activity file feeds the investment accounts that
/// `[moneywiz.investments]` maps: their cash rows (deposits, dividends) link
/// like a bank's, the trade legs stay with the buy and sell rows, coverage
/// counts the paired trades, and the sync names the trades MoneyWiz lacks.
/// Another person's account in the same file never reaches a mapped one.
#[test]
fn wealthsimple_investment_accounts_sync_like_a_bank() {
    use cashcast_engine::moneywiz_coverage;
    let home = scratch("ws-invest");
    let db = home.join("ipadMoneyWiz.sqlite");
    make_db(&db);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (9,16,'Tax-free CANARYMW8','CAD',0.0),(11,16,'Taxable CANARYMW9','CAD',0.0);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (500,38,9,5000.0,?1,'IN CANARYMW10')",
        rusqlite::params![ts("2026-09-01")],
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE) VALUES (501,60,9,-2000.0,?1,'VFV',20.0,100.0)",
        rusqlite::params![ts("2026-09-02")],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox").join("activity.csv"),
        "transaction_date,settlement_date,account_id,account_type,activity_type,activity_sub_type,direction,symbol,name,currency,quantity,unit_price,commission,net_cash_amount
2026-09-01,2026-09-01,CANARYT1CAD,TFSA,Deposit,,,,,CAD,,,,5000.00
2026-09-02,2026-09-03,CANARYT1CAD,TFSA,Trade,BUY,LONG,VFV,CANARY-WSN1,CAD,20,100.00,0,-2000.00
2026-09-30,2026-09-30,CANARYT1CAD,TFSA,Dividend,,,VFV,CANARY-WSN1,CAD,,,,12.00
2026-09-05,2026-09-05,CANARYX16CAD,Non-registered,Deposit,,,,,CAD,,,,300.00
2026-09-06,2026-09-06,CANARYY26CAD,Non-registered,Deposit,,,,,CAD,,,,400.00
2026-09-07,2026-09-08,CANARYY26CAD,Non-registered,Trade,BUY,LONG,XEQT,CANARY-WSN2,CAD,10,30.00,0,-300.00
",
    )
    .unwrap();
    let cfg = config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\n[moneywiz.investments]\n\"Tax-free CANARYMW8\" = \"wealthsimple-activity-csv:TFSA\"\n\"Taxable CANARYMW9\" = \"wealthsimple-activity-csv:non-registered:Y26CAD\"\n",
        db.display()
    ))
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    for t in &exports.txns {
        assert!(!t.account.contains("CANARYT1CAD"), "{}", t.account);
        assert!(!t.text.contains("CANARY-WSN"), "{}", t.text);
    }
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = |pk: i64| {
        plan.accounts
            .iter()
            .find(|a| a.mw == pk)
            .unwrap_or_else(|| panic!("account {pk} is not linked"))
    };
    // TFSA: the deposit matches, the dividend is added, the trade leg is left
    // to the buy row.
    assert_eq!((ap(9).bank_rows, ap(9).matched, ap(9).add.len()), (2, 1, 1));
    // Taxable: only its own account id; the other person's deposit stays out.
    assert_eq!((ap(11).bank_rows, ap(11).add.len()), (1, 1));
    assert_eq!(exports.txns[ap(11).add[0]].cents, 40_000);

    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let mut cov =
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &[], today).unwrap();
    moneywiz_coverage::add_trades(&mut cov, &store, &home.join("inbox"), &cfg, today);
    let tfsa = cov
        .accounts
        .iter()
        .find(|a| a.name == "Tax-free CANARYMW8")
        .unwrap();
    // The deposit by its cash row, the buy by its broker trade.
    assert_eq!((tfsa.rows, tfsa.covered, tfsa.covered_recent), (2, 2, 2));
    assert!(tfsa.gap.is_none(), "{:?}", tfsa.gap);

    let report = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false)
        .unwrap()
        .report;
    assert!(
        report.contains("| Taxable CANARYMW9 | 1 | XEQT |"),
        "{report}"
    );
    assert!(!report.contains("CANARYT1CAD"), "{report}");
    let _ = std::fs::remove_dir_all(home);
}

/// A mapping without an id end can take two broker accounts of one type
/// (a second TFSA with cash only). The link goes to the one that holds the
/// trades, since the mapped buy and sell rows come from it.
#[test]
fn an_investment_link_picks_the_account_with_the_trades() {
    let home = scratch("ws-two-tfsa");
    let db = home.join("ipadMoneyWiz.sqlite");
    make_db(&db);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (9,16,'Tax-free CANARYMW11','CAD',0.0);",
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox").join("activity.csv"),
        "transaction_date,settlement_date,account_id,account_type,activity_type,activity_sub_type,direction,symbol,name,currency,quantity,unit_price,commission,net_cash_amount
2026-09-01,2026-09-01,CANARYA16CAD,TFSA,Deposit,,,,,CAD,,,,1000.00
2026-09-02,2026-09-03,CANARYA16CAD,TFSA,Trade,BUY,LONG,VFV,CANARY-WSN3,CAD,5,100.00,0,-500.00
2026-09-04,2026-09-04,CANARYB26CAD,TFSA,Deposit,,,,,CAD,,,,70.00
",
    )
    .unwrap();
    let cfg = config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\n[moneywiz.investments]\n\"Tax-free CANARYMW11\" = \"wealthsimple-activity-csv:TFSA\"\n",
        db.display()
    ))
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan
        .accounts
        .iter()
        .find(|a| a.mw == 9)
        .expect("the account with the trades is linked");
    assert!(
        ap.export_account.ends_with("A16CAD"),
        "{}",
        ap.export_account
    );
    assert_eq!((ap.bank_rows, ap.add.len()), (1, 1));
    let _ = std::fs::remove_dir_all(home);
}

/// An account with no export (a broker that only wires money out) is
/// covered where its rows are transfers whose other side a bank row matches:
/// the bank row proves the amount and the day. A transfer whose other side
/// has no bank row stays a gap.
#[test]
fn transfers_into_a_matched_account_cover_an_account_with_no_export() {
    use cashcast_engine::moneywiz_coverage;
    let (home, db, _) = setup("coverage-transfer-pairs");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (12,11,'Broker','CAD',0.0);",
    )
    .unwrap();
    for (pk, ent, account, amount, date, to, from) in [
        // Wired to Chequing: the bank's PAYROLL 3000.00 row on 09-30. The
        // broker side is 1 % more: the wire fee.
        (500, 46, 12, -3030.0, "2026-09-30", Some(1), None),
        (501, 45, 1, 3000.0, "2026-09-30", None, Some(12)),
        // Wired to Chequing, but the bank has no such row.
        (502, 46, 12, -77.0, "2026-09-20", Some(1), None),
        (503, 45, 1, 77.0, "2026-09-20", None, Some(12)),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZRECIPIENTACCOUNT1,ZSENDERACCOUNT) VALUES (?1,?2,?3,?4,?5,'WIRE CANARYMW9',?6,?7)",
            rusqlite::params![pk, ent, account, amount, ts(date), to, from],
        )
        .unwrap();
    }
    drop(c);
    let cfg = config::parse(&format!(
        "{}since = \"2026-08-01\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let cov =
        moneywiz_coverage::coverage(&store, &exports, cfg.moneywiz.as_ref(), &[], today).unwrap();
    let broker = cov.accounts.iter().find(|a| a.name == "Broker").unwrap();
    assert_eq!(
        (
            broker.rows,
            broker.covered,
            broker.rows_since,
            broker.covered_since
        ),
        (2, 1, 2, 1)
    );
    assert!(broker.source.contains("transfers"), "{}", broker.source);
    let gap = broker.since_gap.as_deref().unwrap();
    assert!(gap.contains("rows 2026-09-20 to 2026-09-20"), "{gap}");
    let report = moneywiz_coverage::render(&cov, today);
    assert!(!report.contains("CANARY"), "{report}");
    let _ = std::fs::remove_dir_all(home);
}

/// A synthetic stock plan statement: a sale, a wire out and a dividend.
const STOCK_PLAN: &str = "\
CANARY HOLDER NAME
CLIENT STATEMENT     For the Period September 1- September 30, 2026          Page 1 of 1
Account Detail     123-456789-321
CASH FLOW ACTIVITY BY DATE
Activity Settlement
Date     Date        Activity Type         Description            Comments      Quantity      Price     Credits/(Debits)
 9/6     9/8         Sold                  CANARY CORP            CANARY LOT    (4.000)       $50.25         201.00
 9/10                Funds Transferred     WIRE OUT                                                        (195.00)
 9/20                Qualified Dividend    CANARY CORP                                                        2.00
NET CREDITS/(DEBITS)                                                                                         $8.00
";

/// A MoneyWiz stock plan account typed by hand: an opening balance and an
/// early deposit, a vest as a buy, a sale as a sell row plus a deposit of
/// its cash, and a rough wire.
fn stock_plan_db(name: &str) -> (PathBuf, PathBuf) {
    let (home, db, _) = setup(name);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),
           (60,'InvestmentBuyTransaction',37,0),(61,'InvestmentSellTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES (9,16,'Stock Plan','USD',100.0,'acct-canary');",
    )
    .unwrap();
    for (pk, ent, amount, day, desc) in [
        (350, 38, 50.0, "2026-08-30", "EARLY CANARYMW20"),
        (351, 60, -300.0, "2026-09-01", ""),
        (352, 61, 200.0, "2026-09-05", ""),
        (353, 38, 200.0, "2026-09-06", "SALE CANARYMW21"),
        (354, 48, -190.0, "2026-09-10", "WIRE CANARYMW22"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZGID) VALUES (?1,?2,9,?3,?4,?5,'g' || ?1)",
            rusqlite::params![pk, ent, amount, ts(day), desc],
        )
        .unwrap();
    }
    c.execute_batch(
        "UPDATE ZSYNCOBJECT SET ZSYMBOL = 'CANARY', ZPRICEPERSHARE = 50.0, ZNUMBEROFSHARES = 6.0 WHERE Z_PK = 351;
         UPDATE ZSYNCOBJECT SET ZSYMBOL = 'CANARY', ZPRICEPERSHARE = 50.0, ZNUMBEROFSHARES = 4.0 WHERE Z_PK = 352;",
    )
    .unwrap();
    (home, db)
}

fn stock_plan_cfg(db: &Path, invested: bool, balance: &str) -> config::Config {
    let investments = if invested {
        "[moneywiz.investments]\n\"Stock Plan\" = \"etrade-benefit-espp:non-registered\"\n"
    } else {
        ""
    };
    config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\nmatch = \"loose\"\nbank_wins = true\n\
         [moneywiz.accounts]\n\"Stock Plan\" = \"E*TRADE Stock Plan …9321\"\n\
         [moneywiz.balances]\n\"Stock Plan\" = \"{balance}@2026-09-30\"\n{investments}",
        db.display()
    ))
    .unwrap()
}

fn stock_plan_inbox() -> inbox::Inbox {
    use cashcast_engine::{export::LoadOptions, pdf};
    let e = pdf::parse(
        STOCK_PLAN,
        "statement.pdf",
        "statement",
        &LoadOptions::default(),
    )
    .unwrap();
    assert_eq!(e.format, pdf::ETRADE_STOCKPLAN);
    inbox::Inbox::merge(vec![e], Vec::new())
}

/// In a stock plan account the statements hold the cash: MoneyWiz's buy and
/// sell rows only move shares, so each gets an offset, and one correction
/// brings the cash before the first statement row to 0. The opening balance
/// and the early rows stay; MoneyWiz then matches the statement.
#[test]
fn a_stock_plan_account_offsets_share_moves_and_its_start_cash() {
    let (home, db) = stock_plan_db("stock-plan");
    let store = moneywiz::open(&db).unwrap();
    let exports = stock_plan_inbox();
    let account = |cfg: &config::Config| {
        moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref())
            .unwrap()
            .accounts
            .into_iter()
            .find(|a| a.mw == 9)
            .unwrap()
    };
    let cfg = stock_plan_cfg(&db, true, "8.00");
    let ap = account(&cfg);
    assert!(ap.stock_plan);
    assert_eq!(ap.share_moves, vec![351, 352]);
    assert_eq!(
        (ap.start_cash, ap.start_fix),
        (0, -15_000),
        "opening 100.00 and the early 50.00"
    );
    assert_eq!((ap.fix.len(), ap.add.len()), (2, 1));
    let amounts = moneywiz::corrected_amounts(&store);
    assert_eq!(
        cashcast_engine::moneywiz_ledger::hold_reason(&store, &exports, &ap, &amounts, "adjust"),
        None
    );
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    let kinds = |k: &str| links.iter().filter(|l| l.kind == k).count();
    assert_eq!(
        (kinds("share"), kinds("start"), kinds("balance")),
        (2, 1, 0)
    );
    assert!(
        links
            .iter()
            .any(|l| l.kind == "start" && l.url.contains("2026-09-06"))
    );
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(r.report.contains("matches the bank (8.00)"), "{}", r.report);
    assert!(r.report.contains("2 buy and sell rows get an offset"));

    // The statements show 50.00 of cash before their first row: the start
    // correction leaves that much.
    let more = account(&stock_plan_cfg(&db, true, "58.00"));
    assert_eq!((more.start_cash, more.start_fix), (5_000, -10_000));
    let check = |ap: &moneywiz::AccountPlan| {
        cashcast_engine::moneywiz_ledger::hold_reason(&store, &exports, ap, &amounts, "adjust")
    };
    assert_eq!(check(&more), None);
    // A little below 0 is a site reading without the dividends: 0.
    let near = account(&stock_plan_cfg(&db, true, "5.00"));
    assert_eq!((near.start_cash, near.start_fix), (0, -15_000));
    assert_eq!(check(&near), None);
    // Far below 0 before the first row: a statement is missing, so it holds.
    let missing = account(&stock_plan_cfg(&db, true, "-200.00"));
    assert!(check(&missing).is_some_and(|r| r.contains("a statement is missing")));

    // Not mapped as an investment account: no stock plan offsets.
    let plain = account(&stock_plan_cfg(&db, false, "8.00"));
    assert!(!plain.stock_plan);
    assert!(plain.share_moves.is_empty() && plain.start_fix == 0);

    // The sale typed 3 days before the first statement row still pairs with
    // it (loose matching), so it is not start cash too.
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE ZSYNCOBJECT SET ZDATE1 = ?1 WHERE Z_PK = 353",
            [ts("2026-09-03")],
        )
        .unwrap();
    let early = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&early, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan.accounts.iter().find(|a| a.mw == 9).unwrap();
    assert!(ap.pairs.iter().any(|&(pk, _)| pk == 353));
    assert_eq!(ap.start_fix, -15_000);
    let _ = std::fs::remove_dir_all(home);
}

/// Once the offsets are in MoneyWiz, nothing is left to send, and
/// `edit --fixes` deletes nothing in a stock plan account and keeps its
/// opening balance (owner, 2026-11-16).
#[test]
fn edit_fixes_keeps_a_stock_plan_accounts_corrections() {
    use cashcast_engine::moneywiz_edit;
    let (home, db) = stock_plan_db("stock-plan-kept");
    let c = Connection::open(&db).unwrap();
    for (pk, ent, amount, day, memo) in [
        (355, 38, 300.0, "2026-09-01", "cashcast fix #351"),
        (356, 48, -200.0, "2026-09-05", "cashcast fix #352"),
        (357, 48, -150.0, "2026-09-06", "cashcast start cash fix"),
        (358, 38, 1.0, "2026-09-06", "cashcast fix #353"),
        (359, 48, -5.0, "2026-09-10", "cashcast fix #354"),
        (360, 38, 2.0, "2026-09-20", ""),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZNOTES1,ZGID) VALUES (?1,?2,9,?3,?4,?5,'g' || ?1)",
            rusqlite::params![pk, ent, amount, ts(day), memo],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = stock_plan_inbox();
    let cfg = stock_plan_cfg(&db, true, "8.00");
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan.accounts.iter().find(|a| a.mw == 9).unwrap();
    assert!(ap.stock_plan && ap.share_moves.is_empty());
    assert_eq!(
        (ap.start_fix, ap.fix.len(), ap.add.len()),
        (0, 0, 0),
        "settled"
    );
    assert!(moneywiz::links_for(&store, &exports, &plan, "adjust").is_empty());
    let r = moneywiz_edit::resolve(
        &store,
        &moneywiz_edit::EditsFile::default(),
        Some((&exports.txns, &plan)),
    );
    assert!(r.plan.rows.iter().all(|e| !e.delete), "nothing is deleted");
    assert!(r.plan.accounts.is_empty(), "the opening balance stays");
    let kept = moneywiz_edit::kept_corrections(&store, &plan);
    assert_eq!(kept.len(), 4);
    assert!(
        kept.iter()
            .all(|&(a, why)| a == 9 && why == moneywiz_edit::KEPT_STOCK_PLAN)
    );
    let _ = std::fs::remove_dir_all(home);
}

/// A MoneyWiz row before the bank period can stand in for rows the export
/// holds (a transfer in typed early, dividends never typed). The bank's own
/// start balance (its balance less every row of the period) proves it: when
/// MoneyWiz is ahead before the period by what the sync adds, the plan is
/// right, and the balance correction the day before the period squares it.
/// A bank start below 0 (the balance or a statement is wrong) still holds.
#[test]
fn an_early_row_standing_in_for_bank_rows_is_not_held() {
    let (home, db, _) = setup("early-stand-in");
    let (_app, _log) = fake_app(&home);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (50,11,'Invest','CAD',0.0),(51,11,'Other','CAD',0.0);",
    )
    .unwrap();
    for (pk, account, amount) in [(500, 50, 6000.0), (501, 51, 1500.0)] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,38,?2,?3,?4,'EARLY CANARYMW8')",
            rusqlite::params![pk, account, amount, ts("2026-06-01")],
        )
        .unwrap();
    }
    drop(c);
    std::fs::write(
        home.join("inbox/invest.csv"),
        "Date,Account,Description,Amount\n\
         2026-09-04,Invest,TRANSFER IN CANARY41,5900.00\n\
         2026-09-20,Invest,DIVIDEND CANARY42,100.00\n\
         2026-09-04,Other,TRANSFER IN CANARY43,5900.00\n\
         2026-09-20,Other,DIVIDEND CANARY44,100.00\n",
    )
    .unwrap();
    let cfg = config::parse(&format!(
        "{}match = \"loose\"\n[moneywiz.balances]\n\"Invest\" = \"6000.00@2026-09-30\"\n\"Other\" = \"1500.00@2026-09-30\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    assert!(
        !r.report.contains("- Invest:"),
        "the early row stands in for the bank rows: {}",
        r.report
    );
    assert!(
        r.report.contains("- Other: the sync moves MoneyWiz away"),
        "a lead that does not explain it still holds: {}",
        r.report
    );
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    assert!(
        links.iter().any(|l| l.kind == "balance"
            && l.id.contains("Invest")
            && l.url.contains("2026-09-03")),
        "one balance correction the day before the period"
    );
    assert!(!r.report.contains("CANARY"));
    let _ = std::fs::remove_dir_all(home);
}

/// A bank balance dated before the export's last row (a statement balance,
/// then newer rows from another file) still gets the balance correction:
/// it compares on its own day, with the changes dated up to that day.
#[test]
fn an_older_bank_balance_still_corrects_a_wrong_start() {
    let (home, db, _) = setup("older-balance-start");
    let c = Connection::open(&db).unwrap();
    // MoneyWiz starts 40.00 too high; every row after matches the bank.
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (60,11,'Savings','CAD',140.0)",
        [],
    )
    .unwrap();
    for (pk, amount, date) in [(600, -10.0, "2026-09-02"), (601, -20.0, "2026-09-20")] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,60,?2,?3,'FEE CANARYMW10')",
            rusqlite::params![pk, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount\n2026-09-02,Savings,FEE CANARY51,-10.00\n2026-09-20,Savings,FEE CANARY52,-20.00\n",
    )
    .unwrap();
    let cfg = config::parse(&format!(
        "{}[moneywiz.balances]\n\"Savings\" = \"90.00@2026-09-10\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    ))
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    let balance: Vec<_> = links.iter().filter(|l| l.kind == "balance").collect();
    assert_eq!(balance.len(), 1, "one correction for the wrong start");
    assert!(balance[0].id.ends_with("|-4000"), "{}", balance[0].id);
    assert!(
        balance[0].url.contains("2026-09-01"),
        "the day before the period"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// A 0.00 bank row moves no money, and MoneyWiz does not save one: the
/// plan never adds it, so it is not counted as missing either.
#[test]
fn a_zero_bank_row_is_never_added() {
    let (home, db, cfg) = setup("zero-row");
    std::fs::write(
        home.join("inbox/zero.csv"),
        "Date,Account,Description,Amount\n2026-09-12,Chequing,INFO CANARY61,0.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Chequing")
        .unwrap();
    assert!(
        ap.add.iter().all(|&i| exports.txns[i].cents != 0),
        "no 0.00 row to add"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// The bank never issued a month, and the owner's MoneyWiz rows stand in for
/// it: across the statement before the gap and the one after, MoneyWiz moves
/// as the statements do. The sync leaves every row of those days alone (a
/// bank row near the gap would pair with a row typed for the missing month),
/// and holds nothing.
#[test]
fn rows_around_a_month_moneywiz_stands_in_for_are_left_alone() {
    let (home, db, cfg) = setup("stand-in-month");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (70,11,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    // September's rent, typed early (August 30); November's fee.
    for (pk, amount, date) in [(700, -500.0, "2026-08-30"), (701, -5.0, "2026-11-05")] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,70,?2,?3,'CANARYMW70')",
            rusqlite::params![pk, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount\n2026-08-29,Savings,SHOP CANARY71,-480.00\n2026-11-05,Savings,FEE CANARY72,-5.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let mut exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let st =
        |start: &str, end: &str, opening: i64, closing: i64| cashcast_engine::export::Statement {
            account: "Savings".into(),
            start: day(start),
            end: day(end),
            opening,
            closing,
        };
    // September is missing: October opens 500.00 lower than August closed.
    exports.files[0].statements.extend([
        st("2026-08-01", "2026-08-31", 100_000, 100_000),
        st("2026-10-01", "2026-10-31", 50_000, 50_000),
    ]);
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Savings")
        .unwrap();
    assert!(ap.add.is_empty(), "the August bank row is left alone");
    assert!(ap.fix.is_empty());
    assert!(
        ap.only_in_moneywiz.is_empty(),
        "the typed rent is left alone"
    );
    assert_eq!(ap.bank_rows, 1, "November still syncs");
    assert_eq!(
        ap.stood_in,
        vec![700],
        "the typed rent stands in, and counts as covered"
    );
    let amounts = moneywiz::corrected_amounts(&store);
    assert_eq!(
        cashcast_engine::moneywiz_ledger::hold_reason(&store, &exports, ap, &amounts, "adjust"),
        None
    );
    let _ = std::fs::remove_dir_all(home);
}

/// A newer download with no balance column (Wealthsimple's all-accounts
/// export) adds rows after the day an older file's balance was read. The
/// balance carries forward by those rows, so the gate still checks the sync
/// against the bank instead of holding the account for "no bank balance".
#[test]
fn a_balance_carries_forward_over_later_rows_of_a_file_without_one() {
    let (home, db, cfg) = setup("carry-balance");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (70,11,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (700,48,70,-100.0,?1,'CANARYMW70')",
        rusqlite::params![ts("2026-10-10")],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount\n2026-10-10,Savings,SHOP CANARY71,-100.00\n2026-11-10,Savings,DEPOSIT CANARY72,6000.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let mut exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    // The older file's balance: read on October 31, before the deposit.
    exports.files[0].balances.insert(
        "Savings".into(),
        cashcast_engine::export::Balance {
            cents: 90_000,
            as_of: Some(day("2026-10-31")),
            source: "csv",
        },
    );
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Savings")
        .unwrap();
    let b = ap.bank_balance.as_ref().expect("a bank balance");
    assert_eq!(
        (b.cents, b.as_of),
        (690_000, Some(day("2026-11-10"))),
        "the October balance plus the November deposit"
    );
    let amounts = moneywiz::corrected_amounts(&store);
    assert_eq!(
        cashcast_engine::moneywiz_ledger::hold_reason(&store, &exports, ap, &amounts, "adjust"),
        None,
        "the deposit brings MoneyWiz to the bank"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// The owner may let the bank win for one account even where the sync moves
/// MoneyWiz away from its balance (rows typed in another form stand in for
/// bank rows): `[moneywiz.trust_bank]` sends the bank's rows, and one balance
/// correction the day before the period squares the end.
#[test]
fn an_account_the_owner_trusts_the_bank_for_is_not_held() {
    let (home, db, _) = setup("trust-bank");
    let (_app, _log) = fake_app(&home);
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (52,11,'Other','CAD',0.0)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (520,38,52,1500.0,?1,'EARLY CANARYMW9')",
        rusqlite::params![ts("2026-06-01")],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/other.csv"),
        "Date,Account,Description,Amount\n\
         2026-09-04,Other,TRANSFER IN CANARY45,5900.00\n\
         2026-09-20,Other,DIVIDEND CANARY46,100.00\n",
    )
    .unwrap();
    let base = format!(
        "{}match = \"loose\"\n[moneywiz.balances]\n\"Other\" = \"1500.00@2026-09-30\"\n",
        CONFIG.replace("DB", &db.display().to_string())
    );
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let held = config::parse(&base).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &held).unwrap();
    let r = moneywiz::sync(&db, &exports, &held, &home, today, true, false).unwrap();
    assert!(r.report.contains("- Other: the sync moves MoneyWiz away"));
    let cfg = config::parse(&format!(
        "{base}[moneywiz.trust_bank]\n\"Other\" = \"the owner chose the bank\"\n"
    ))
    .unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, true, false).unwrap();
    assert!(!r.report.contains("- Other:"), "{}", r.report);
    let store = moneywiz::open(&db).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    assert!(
        links
            .iter()
            .any(|l| l.kind == "balance" && l.id.ends_with("|-600000")),
        "one correction squares the end: {:?}",
        links.iter().map(|l| &l.id).collect::<Vec<_>>()
    );
    assert!(!r.report.contains("CANARY"));
    let _ = std::fs::remove_dir_all(home);
}

/// Links go to the MoneyWiz app, which writes its own database. A sync that
/// read another file (a test's scratch copy) must never hand its links to the
/// real app: only a file of the app's own container counts (2026-11-16, a
/// test sent 5 synthetic links to the owner's MoneyWiz).
#[test]
fn only_the_apps_own_database_may_send_to_the_real_app() {
    let home = Path::new("/Users/someone");
    assert!(moneywiz::app_owns(
        home,
        &home.join("Library/Containers/com.moneywiz.personalfinance/Data/Library/Application Support/MoneyWiz_iCloud.sqlite")
    ));
    assert!(!moneywiz::app_owns(
        home,
        Path::new("/tmp/cashcast-mw-test-x/ipadMoneyWiz.sqlite")
    ));
    assert!(!moneywiz::app_owns(
        Path::new("/Users/other"),
        &home.join("Library/Containers/com.moneywiz.personalfinance/Data/Library/Application Support/MoneyWiz_iCloud.sqlite")
    ));
}

/// A stock plan statement's closing cash proves MoneyWiz's rows to its day
/// in sum, when the broker files also pair every share move: coverage counts
/// them, rows after the day stay open, and a cash gap proves nothing.
#[test]
fn stock_plan_statement_cash_with_paired_shares_covers_the_rows_before_it() {
    use cashcast_engine::moneywiz_coverage::{self, AccountCoverage, Coverage};
    use cashcast_engine::moneywiz_invest::Row;
    let (home, db, _cfg) = setup("stock-plan-cash");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (9,16,'CANARY Plan','USD',0.0);",
    )
    .unwrap();
    // A hand-typed reconcile row and a dividend before the statement, one after.
    for (pk, amount, day) in [
        (360, 40.0, "2025-12-15"),
        (361, 12.0, "2025-12-16"),
        (362, 5.0, "2026-02-01"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1) VALUES (?1,38,9,?2,?3)",
            rusqlite::params![pk, amount, ts(day)],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let account = |statement_cash: Option<NaiveDate>| AccountCoverage {
        name: "CANARY Plan".into(),
        kind: "Investment".into(),
        rows: 3,
        covered: 0,
        rows_recent: 3,
        covered_recent: 0,
        first: Some(day("2025-12-15")),
        last: Some(day("2026-02-01")),
        connector: None,
        source: "-".into(),
        gap: Some("CANARY gap".into()),
        rows_since: 0,
        covered_since: 0,
        to_add_since: 0,
        since_gap: None,
        left_out: None,
        export_days: None,
        dates_since: Vec::new(),
        missing_statements: Vec::new(),
        statement_cash,
    };
    let cov = |statement_cash| Coverage {
        accounts: vec![account(statement_cash)],
        suggestions: Vec::new(),
        reader_gaps: Vec::new(),
        with_links: None,
        matches: Vec::new(),
        since: None,
        open_rows: Default::default(),
    };
    let shares = |moneywiz: f64, missing: usize| Row {
        account: "CANARY Plan".into(),
        symbol: "CANARY".into(),
        moneywiz_shares: moneywiz,
        broker_shares: 3.0,
        missing,
        missing_trades: Vec::new(),
        last_price: None,
        extra: Vec::new(),
        all_rows: Vec::new(),
        all_trades: Vec::new(),
        off_rows: 0,
    };
    // MoneyWiz's cash equals the closing cash on 2025-12-31, shares agree.
    let checks = moneywiz_coverage::stock_plan_checks(
        &store,
        &cov(Some(day("2025-12-31"))),
        &[shares(3.0, 0)],
    );
    let mut c = cov(Some(day("2025-12-31")));
    moneywiz_coverage::count_statements(&mut c, &store, &[], &checks, today);
    assert_eq!(
        (c.accounts[0].covered, c.accounts[0].source.as_str()),
        (2, "statement cash to 2025-12-31")
    );
    // No proven cash day, or shares the broker files do not pair: nothing.
    for (cash, row) in [
        (None, shares(3.0, 0)),
        (Some(day("2025-12-31")), shares(2.0, 1)),
    ] {
        let checks = moneywiz_coverage::stock_plan_checks(&store, &cov(cash), &[row]);
        let mut c = cov(cash);
        moneywiz_coverage::count_statements(&mut c, &store, &[], &checks, today);
        assert_eq!(c.accounts[0].covered, 0);
    }
    let _ = std::fs::remove_dir_all(home);
}

/// A statement before the broker's history: its units move out of the first
/// day's lump into buys on the statement's day, paid by the contributions up
/// to then. The account's cash stays the same, and buys already in MoneyWiz
/// do not change the result.
#[test]
fn a_statement_before_the_history_moves_its_units_out_of_the_lump() {
    use cashcast_engine::pdf::sunlife::{Fund, Snapshot};
    use cashcast_engine::{ledger::Trade, moneywiz_invest as inv};
    let (home, db, _) = setup("statement-lots");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (9,16,'Plan RRSP','CAD',0.0);",
    )
    .unwrap();
    // Two contributions (300) before the statement, and one buy a past
    // run already wrote on the statement's day.
    for (pk, ent, amount, day, symbol, shares) in [
        (350, 38, 200.0, "2026-03-01", None, None),
        (351, 38, 100.0, "2026-04-01", None, None),
        (
            352,
            60,
            -225.0,
            "2026-06-30",
            Some("CANARY EQUITY INDEX FUND"),
            Some(10.0),
        ),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE) VALUES (?1,?2,9,?3,?4,?5,?6,?7)",
            rusqlite::params![pk, ent, amount, ts(day), symbol, shares, shares.map(|n| -amount / n)],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let trade = |day: &str, symbol: &str, shares: f64, cad: f64| Trade {
        date: NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap(),
        symbol: symbol.into(),
        bucket: "RRSP",
        format: "sunlife-transactions-csv".into(),
        account: String::new(),
        shares,
        cad: Some(cad),
        cash: Some(String::new()),
    };
    // The history starts 2026-08-01 with the lump: 12 units at 30, 5 at 20.
    let lump = vec![
        trade("2026-08-01", "CANARY EQUITY INDEX FUND", 12.0, 360.0),
        trade("2026-08-01", "CANARY BOND", 5.0, 100.0),
        trade("2026-09-01", "CANARY BOND", 1.0, 21.0),
    ];
    let fund = |name: &str, units: f64| Fund {
        name: name.into(),
        units,
        value_cents: 0,
    };
    let snaps = [Snapshot {
        file: "statement-2026-06-30.pdf".into(),
        plan: "RRSP".into(),
        as_of: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        funds: vec![
            fund("CANARY Equity Index Fund", 10.0),
            fund("CANARY Bond Fund", 5.0),
        ],
        total_cents: 0,
    }];
    let map = [(
        "Plan RRSP".to_string(),
        "sunlife-transactions-csv:RRSP".to_string(),
    )]
    .into();
    let mut trades = lump.clone();
    let problems = inv::statement_lots(&store, &mut trades, &snaps, &map);
    assert!(problems.is_empty(), "{problems:?}");
    // In date order: a symbol is priced at its last trade.
    assert!(trades.windows(2).all(|w| w[0].date <= w[1].date));
    let mut got: Vec<(String, String, f64, f64)> = trades
        .iter()
        .map(|t| {
            (
                t.date.to_string(),
                t.symbol.clone(),
                t.shares,
                t.cad.unwrap(),
            )
        })
        .collect();
    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // 300 of contributions split 300:100 by the lump's prices (30 and 20);
    // the lump's other 160 pays for the 2 units left in it.
    let want = [
        ("2026-06-30", "CANARY BOND", 5.0, 75.0),
        ("2026-06-30", "CANARY EQUITY INDEX FUND", 10.0, 225.0),
        ("2026-08-01", "CANARY EQUITY INDEX FUND", 2.0, 160.0),
        ("2026-09-01", "CANARY BOND", 1.0, 21.0),
    ];
    assert_eq!(
        got,
        want.map(|(d, s, u, c)| (d.to_string(), s.to_string(), u, c))
    );
    let cash = |ts: &[Trade]| ts.iter().map(|t| t.cad.unwrap()).sum::<f64>();
    assert!((cash(&trades) - cash(&lump)).abs() < 1e-9);
    // A statement on or after the history's first day changes nothing.
    let mut later = lump.clone();
    let mut late = snaps.clone();
    late[0].as_of = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
    assert!(inv::statement_lots(&store, &mut later, &late, &map).is_empty());
    assert_eq!(later.len(), lump.len());
    // A fund with more units than the lump holds is a problem; nothing moves.
    let mut more = lump.clone();
    let mut big = snaps.clone();
    big[0].funds[1].units = 6.0;
    let problems = inv::statement_lots(&store, &mut more, &big, &map);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(more.len(), lump.len());
    let _ = std::fs::remove_dir_all(home);
}

/// A Sun Life statement's units check MoneyWiz's units per fund on its day;
/// a match proves the account's rows up to then, and coverage counts them.
#[test]
fn statement_units_check_moneywiz_and_cover_the_rows_before_them() {
    use cashcast_engine::moneywiz_coverage;
    use cashcast_engine::moneywiz_invest as inv;
    use cashcast_engine::pdf::sunlife::{Fund, Snapshot};
    let (home, db, cfg) = setup("statement-units");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (9,16,'Plan RRSP','CAD',0.0);",
    )
    .unwrap();
    // Contributions arrive as cash and buy units the same day.
    for (pk, ent, amount, day, symbol, shares) in [
        (350, 38, 200.0, "2026-03-01", None, None),
        (
            351,
            60,
            -200.0,
            "2026-03-01",
            Some("canary equity index fund"),
            Some(10.0),
        ),
        (352, 38, 100.0, "2026-04-01", None, None),
        (
            353,
            60,
            -100.0,
            "2026-04-01",
            Some("CANARY BOND"),
            Some(5.0),
        ),
        (354, 38, 50.0, "2026-08-01", None, None), // after the statement
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZSYMBOL,ZNUMBEROFSHARES,ZPRICEPERSHARE) VALUES (?1,?2,9,?3,?4,?5,?6,?7)",
            rusqlite::params![pk, ent, amount, ts(day), symbol, shares, shares.map(|n| -amount / n)],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let fund = |name: &str, units: f64| Fund {
        name: name.into(),
        units,
        value_cents: 0,
    };
    let snap = |day: &str, funds: Vec<Fund>| Snapshot {
        file: format!("statement-{day}.pdf"),
        plan: "RRSP".into(),
        as_of: NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap(),
        funds,
        total_cents: 0,
    };
    let snaps = [
        // MoneyWiz bought nothing yet: 0 units, the statement says 1.
        snap("2026-02-28", vec![fund("CANARY Equity Index Fund", 1.0)]),
        snap(
            "2026-06-30",
            vec![
                fund("CANARY Equity Index Fund", 10.0),
                fund("CANARY Bond Fund", 5.0),
            ],
        ),
    ];
    let map = [(
        "Plan RRSP".to_string(),
        "sunlife-transactions-csv:RRSP".to_string(),
    )]
    .into();
    let checks = inv::statement_checks(&store, &snaps, &map);
    let verdicts: Vec<(String, bool)> = checks
        .iter()
        .map(|k| (k.as_of.to_string(), k.matches))
        .collect();
    assert_eq!(
        verdicts,
        [
            ("2026-02-28".to_string(), false),
            ("2026-06-30".to_string(), true)
        ]
    );
    // "CANARY Bond Fund" pairs "CANARY BOND" by its words.
    assert_eq!(
        checks[1].funds[1]
            .moneywiz
            .as_ref()
            .map(|(s, u)| (s.as_str(), *u)),
        Some(("CANARY BOND", 5.0))
    );
    let report = inv::render_statement_checks(&checks);
    assert!(
        report.contains("## Statements against MoneyWiz units"),
        "{report}"
    );
    assert!(
        report.contains(
            "| Plan RRSP | 2026-02-28 | CANARY Equity Index Fund | 1.0000 | - | - | no symbol |"
        ),
        "{report}"
    );
    // A wrong unit count or cash left in the plan fails the check.
    let off = [snap(
        "2026-06-30",
        vec![
            fund("CANARY Equity Index Fund", 10.5),
            fund("CANARY Bond Fund", 5.0),
        ],
    )];
    assert!(!inv::statement_checks(&store, &off, &map)[0].matches);

    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let mut cov = moneywiz_coverage::coverage(&store, &exports, None, &[], today).unwrap();
    let plan = |c: &moneywiz_coverage::Coverage| {
        let a = c.accounts.iter().find(|a| a.name == "Plan RRSP").unwrap();
        (a.rows, a.covered, a.source.clone())
    };
    assert_eq!(plan(&cov).1, 0);
    moneywiz_coverage::count_statements(&mut cov, &store, &[], &checks, today);
    // The four rows to 2026-06-30 are proven; the August row is not.
    assert_eq!(
        plan(&cov),
        (5, 4, "statement units to 2026-06-30".to_string())
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn bank_wins_replaces_a_lump_with_the_bank_rows_it_sums() {
    let (home, db, _) = setup("bank-wins-lump");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (40,14,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    // One hand-typed row for two purchases on other days, off by cents.
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (341,48,40,-105.00,?1,'LUMP CANARYMW10')",
        rusqlite::params![ts("2026-09-12")],
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount,Balance\n\
         2026-09-10,Savings,SHOP CANARY35,-60.03,\n\
         2026-09-14,Savings,SHOP CANARY36,-45.00,\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let savings = |extra: &str| {
        let cfg = config::parse(&format!(
            "{}match = \"loose\"\n{extra}",
            CONFIG.replace("DB", &db.display().to_string())
        ))
        .unwrap();
        let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
        let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
        plan.accounts
            .into_iter()
            .find(|a| a.export_account == "Savings")
            .unwrap()
    };
    let loose = savings("");
    assert_eq!(
        (
            loose.add.len(),
            loose.stand_in.len(),
            loose.matched_mw.clone()
        ),
        (0, 0, vec![341]),
        "without bank_wins the lump covers both rows"
    );
    let wins = savings("bank_wins = true\n");
    assert_eq!(wins.add.len(), 2, "the bank's own rows are added");
    assert_eq!(wins.stand_in, vec![341], "the lump goes");
    assert!(wins.matched_mw.is_empty());
    let _ = std::fs::remove_dir_all(home);
}

/// A stock plan correction stays a row, so it must sit on the day of the
/// row it fixes, or MoneyWiz differs from the statement between the two days.
#[test]
fn edit_fixes_dates_a_kept_correction_on_the_day_of_its_row() {
    use cashcast_engine::moneywiz_edit;
    let (home, db) = stock_plan_db("stock-plan-dated");
    let c = Connection::open(&db).unwrap();
    for (pk, ent, amount, day, memo) in [
        (355, 38, 300.0, "2026-09-01", "cashcast fix #351"),
        (356, 48, -200.0, "2026-09-05", "cashcast fix #352"),
        (357, 48, -150.0, "2026-09-06", "cashcast start cash fix"),
        // Row 353 is on 2026-09-06; its correction landed two days later.
        (358, 38, 1.0, "2026-09-08", "cashcast fix #353"),
        (359, 48, -5.0, "2026-09-10", "cashcast fix #354"),
        (360, 38, 2.0, "2026-09-20", ""),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZNOTES1,ZGID) VALUES (?1,?2,9,?3,?4,?5,'g' || ?1)",
            rusqlite::params![pk, ent, amount, ts(day), memo],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = stock_plan_inbox();
    let cfg = stock_plan_cfg(&db, true, "8.00");
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let r = moneywiz_edit::resolve(
        &store,
        &moneywiz_edit::EditsFile::default(),
        Some((&exports.txns, &plan)),
    );
    let dated: Vec<(&str, Option<&str>, bool)> = r
        .plan
        .rows
        .iter()
        .map(|e| (e.gid.as_str(), e.date.as_deref(), e.delete))
        .collect();
    assert_eq!(dated, vec![("g358", Some("2026-09-06"), false)]);
    let _ = std::fs::remove_dir_all(home);
}

/// Around a month the bank never issued, MoneyWiz's rows stand in, but the
/// statements still say how much the balance moved across the gap. One
/// correction on the last missing day makes MoneyWiz move by that much, and
/// "Balance over time" compares the days outside the gap.
#[test]
fn a_stood_in_month_gets_one_correction_to_the_statements() {
    let (home, db, cfg) = setup("stand-in-fix");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (70,11,'Savings','CAD',1000.0)",
        [],
    )
    .unwrap();
    // September is typed 10.00 more than the statements show.
    for (pk, amount, date) in [
        (700, -20.0, "2026-07-15"),
        (701, -480.0, "2026-08-30"),
        (702, -60.0, "2026-09-10"),
        (703, -5.0, "2026-11-05"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,48,70,?2,?3,'CANARYMW73')",
            rusqlite::params![pk, amount, ts(date)],
        )
        .unwrap();
    }
    drop(c);
    std::fs::write(
        home.join("inbox/savings.csv"),
        "Date,Account,Description,Amount\n\
         2026-07-15,Savings,SHOP CANARY74,-20.00\n\
         2026-08-29,Savings,SHOP CANARY75,-480.00\n\
         2026-11-05,Savings,FEE CANARY76,-5.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let mut exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let st =
        |start: &str, end: &str, opening: i64, closing: i64| cashcast_engine::export::Statement {
            account: "Savings".into(),
            start: day(start),
            end: day(end),
            opening,
            closing,
        };
    // September is missing: it moved the balance by -50.00.
    exports.files[0].statements.extend([
        st("2026-08-01", "2026-08-31", 98_000, 50_000),
        st("2026-10-01", "2026-10-31", 45_000, 45_000),
    ]);
    exports.files[0].balances.insert(
        "Savings".into(),
        cashcast_engine::export::Balance {
            cents: 44_500,
            as_of: Some(day("2026-11-05")),
            source: "csv",
        },
    );
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "Savings")
        .unwrap();
    assert_eq!(ap.span_fix, vec![(day("2026-09-30"), 1000)]);
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    let ids: Vec<&str> = links
        .iter()
        .filter(|l| l.id.contains("Savings") || l.kind == "balance")
        .map(|l| l.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["span|Savings|2026-09-30|1000"],
        "the span fix squares the end too"
    );
    let h = cashcast_engine::moneywiz_diagnose::history(&store, &exports, &plan);
    let s = h.iter().find(|h| h.account == "Savings").unwrap();
    assert_eq!(s.close_days, (1, 2), "the days inside the gap are left out");
    assert_eq!(s.worst, Some((day("2026-11-05"), -1000)));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_statement_only_plan_gets_corrections_to_each_statement_day() {
    let (home, db, mut cfg) = setup("statement-only");
    let c = Connection::open(&db).unwrap();
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (80,11,'Fund plan','CAD',0.0)",
        [],
    )
    .unwrap();
    // One hand-typed deposit; the plan's trades and interest are not kept.
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (800,48,80,100.0,?1,'CANARYMW81')",
        rusqlite::params![ts("2024-02-01")],
    )
    .unwrap();
    drop(c);
    let m = cfg.moneywiz.as_mut().unwrap();
    m.since = Some("2023-09-30".into());
    m.accounts.insert("Fund plan".into(), "PLAN 1234".into());
    let store = moneywiz::open(&db).unwrap();
    let mut exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let st =
        |start: &str, end: &str, opening: i64, closing: i64| cashcast_engine::export::Statement {
            account: "PLAN 1234".into(),
            start: day(start),
            end: day(end),
            opening,
            closing,
        };
    // Year-end statements only, and no row: emptied during 2025.
    exports.files[0].statements.extend([
        st("2023-10-01", "2023-12-31", 400_000, 422_618),
        st("2024-10-01", "2024-12-31", 430_000, 431_000),
        st("2025-10-01", "2025-12-31", 0, 0),
    ]);
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let ap = plan
        .accounts
        .iter()
        .find(|a| a.export_account == "PLAN 1234")
        .unwrap();
    assert_eq!(
        ap.span_fix,
        vec![
            (day("2023-09-30"), 400_000),
            (day("2023-12-31"), 22_618),
            (day("2024-09-30"), -2_618),
            (day("2024-12-31"), 1_000),
            (day("2025-09-30"), -431_000),
        ],
        "the hand deposit stays; each statement day gets what is left"
    );
    let links = moneywiz::links_for(&store, &exports, &plan, "adjust");
    let ids: Vec<&str> = links
        .iter()
        .filter(|l| l.id.contains("PLAN 1234"))
        .map(|l| l.id.as_str())
        .collect();
    assert_eq!(ids.len(), 5, "{ids:?}");
    assert_eq!(ids[0], "span|PLAN 1234|2023-09-30|400000");
    let h = cashcast_engine::moneywiz_diagnose::history(&store, &exports, &plan);
    let s = h.iter().find(|h| h.account == "Fund plan").unwrap();
    assert_eq!(
        s.close_days,
        (0, 6),
        "MoneyWiz is off on every statement day"
    );
    assert_eq!(s.worst, Some((day("2023-12-31"), -422_618)));
    let chain = cashcast_engine::moneywiz_diagnose::chain(&exports, &plan);
    let c = chain.iter().find(|c| c.account == "PLAN 1234").unwrap();
    assert_eq!(c.checked, 0, "no row leads back to a closing");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn sync_warns_when_links_go_to_an_archived_account() {
    let (home, db, cfg) = setup("archived");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZARCHIVED INTEGER;
         UPDATE ZSYNCOBJECT SET ZARCHIVED = 0 WHERE Z_PK IN (2, 3);
         UPDATE ZSYNCOBJECT SET ZARCHIVED = 1 WHERE Z_PK = 1;",
    )
    .unwrap();
    drop(c);
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(
        r.report
            .contains("Archived in MoneyWiz, so MoneyWiz drops their links: Chequing."),
        "{}",
        r.report
    );
    // Shown again: no warning.
    let c = Connection::open(&db).unwrap();
    c.execute("UPDATE ZSYNCOBJECT SET ZARCHIVED = 0 WHERE Z_PK = 1", [])
        .unwrap();
    drop(c);
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(!r.report.contains("Archived in MoneyWiz"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_download_that_names_no_account_is_not_an_unlinked_account() {
    // 2026-11-22: CSVs with no account column (TD, CIBC) took the file name
    // as the account, so "Download (1)" and "accountactivity" showed up in
    // `weekly` "Not up to date" as exports with no MoneyWiz account.
    let (home, db, cfg) = setup("file-named");
    std::fs::write(
        home.join("inbox").join("Download (1).csv"),
        "Date,Description,Amount\n2026-09-04,SHOP CANARYFN1,-60.00\n",
    )
    .unwrap();
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    assert!(
        !plan.unlinked_exports.contains(&"Download (1)".to_string()),
        "{:?}",
        plan.unlinked_exports
    );
    assert_eq!(plan.file_named, ["Download (1)"]);
    let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let r = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false).unwrap();
    assert!(
        r.report.contains("Downloads that name no account"),
        "{}",
        r.report
    );
    assert!(!r.report.contains("CANARYFN1"));
    let _ = std::fs::remove_dir_all(home);
}

/// Money moved into an investment account and spent on a buy the same week:
/// the sync adds the deposit, and `edit --fixes` adds the buy. Counted
/// together they reach the bank's cash, so the sync is not held and no
/// balance correction undoes the buy (found 2026-11-22: the sync held the
/// deposit because the buy was missing, and `edit --fixes` held the buy
/// because the sync was held).
#[test]
fn a_deposit_spent_on_a_buy_the_edit_adds_is_not_held() {
    let home = scratch("ws-deposit-buy");
    let db = home.join("ipadMoneyWiz.sqlite");
    make_db(&db);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZNUMBEROFSHARES FLOAT;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZPRICEPERSHARE FLOAT;
         INSERT INTO Z_PRIMARYKEY VALUES (16,'InvestmentAccount',10,0),(60,'InvestmentBuyTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (12,16,'Taxable CANARYMW12','CAD',0.0);",
    )
    .unwrap();
    drop(c);
    std::fs::write(
        home.join("inbox").join("activity.csv"),
        "transaction_date,settlement_date,account_id,account_type,activity_type,activity_sub_type,direction,symbol,name,currency,quantity,unit_price,commission,net_cash_amount
2026-11-19,2026-11-19,CANARYZ36CAD,Non-registered,MoneyMovement,TRANSFER,,,,CAD,,,,1000.00
2026-11-19,2026-11-20,CANARYZ36CAD,Non-registered,Trade,BUY,LONG,XEQT,CANARY-WSN4,CAD,20,30.00,0,-600.00
",
    )
    .unwrap();
    let cfg = config::parse(&format!(
        "[moneywiz]\ndb = \"{}\"\nmatch = \"loose\"\nbank_wins = true\n[moneywiz.balances]\n\"Taxable CANARYMW12\" = \"400.00@2026-11-20\"\n[moneywiz.investments]\n\"Taxable CANARYMW12\" = \"wealthsimple-activity-csv:non-registered:Z36CAD\"\n",
        db.display()
    ))
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
    let report = moneywiz::sync(&db, &exports, &cfg, &home, today, false, false)
        .unwrap()
        .report;
    assert!(
        report.contains("| Taxable CANARYMW12 | 1 | XEQT |"),
        "{report}"
    );
    assert!(!report.contains("moves MoneyWiz away"), "{report}");
    assert!(!report.contains("balance corrections"), "{report}");
    assert!(!report.contains("CANARYZ36CAD"), "{report}");
    // The balance column counts the buy too, so it shows no false mismatch.
    assert!(report.contains("matches the bank (400.00)"), "{report}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_same_day_charge_and_its_reversal_are_not_bank_rows() {
    let (home, _, cfg) = setup("reversal");
    std::fs::write(
        home.join("inbox").join("bank.csv"),
        "Date,Account,Description,Amount,Balance
2026-09-01,Chequing,CARD CHECK CANARY11,-1.00,
2026-09-01,Chequing,CARD CHECK CANARY11,1.00,
2026-09-02,Chequing,COFFEE CANARY12,-4.00,
2026-09-03,Chequing,REFUND CANARY13,4.00,
2026-09-04,Chequing,HOTEL HOLD CANARY14,-80.00,
2026-09-04,Chequing,HOTEL HOLD CANARY14,80.00,
",
    )
    .unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let cents: Vec<i64> = exports.txns.iter().map(|t| t.cents).collect();
    assert_eq!(cents, [-400, 400, -8000, 8000], "only the card check pair is dropped");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_card_check_pair_moneywiz_holds_needs_no_bank_row() {
    let (home, db, cfg) = setup("cardcheck");
    let c = Connection::open(&db).unwrap();
    for (pk, ent, amount) in [(110, 48, -1.0), (111, 38, 1.0)] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,?2,1,?3,?4,'CARD CHECK CANARYMW9')",
            rusqlite::params![pk, ent, amount, ts("2026-09-04")],
        )
        .unwrap();
    }
    drop(c);
    let store = moneywiz::open(&db).unwrap();
    let exports = inbox::load_exports(&home.join("inbox"), &cfg).unwrap();
    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref()).unwrap();
    let flagged: Vec<i64> = plan
        .accounts
        .iter()
        .flat_map(|a| a.only_in_moneywiz.iter().chain(&a.stand_in))
        .copied()
        .collect();
    assert!(!flagged.contains(&110) && !flagged.contains(&111), "{flagged:?}");
    let _ = std::fs::remove_dir_all(home);
}
