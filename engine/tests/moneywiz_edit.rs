//! In-place MoneyWiz edit plans on a synthetic MoneyWiz database. No real data.

use std::path::PathBuf;

use cashcast_engine::moneywiz;
use cashcast_engine::moneywiz_edit::{self, EditsFile};
use chrono::NaiveDate;
use rusqlite::Connection;

fn ts(date: &str) -> f64 {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    (d.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() - 978_307_200) as f64
}

fn store(name: &str) -> moneywiz::Store {
    let dir = std::env::temp_dir().join(format!("cashcast-mw-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("MoneyWiz.sqlite");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER PRIMARY KEY, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);
         CREATE TABLE ZSYNCOBJECT (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, ZGID VARCHAR, ZNAME VARCHAR,
           ZCURRENCYNAME VARCHAR, ZOPENINGBALANCE FLOAT, ZNAME2 VARCHAR, ZPARENTCATEGORY INTEGER, ZAMOUNT1 FLOAT,
           ZDATE1 TIMESTAMP, ZDESC2 VARCHAR, ZNOTES1 VARCHAR, ZACCOUNT2 INTEGER, ZRECIPIENTACCOUNT1 INTEGER,
           ZSTATUS1 INTEGER);
         CREATE TABLE ZCATEGORYASSIGMENT (Z_PK INTEGER PRIMARY KEY, ZTRANSACTION INTEGER, ZCATEGORY INTEGER, ZAMOUNT FLOAT);
         INSERT INTO Z_PRIMARYKEY VALUES (10,'Account',0,0),(11,'BankChequeAccount',10,0),(20,'Category',0,0),
           (37,'Transaction',0,0),(48,'WithdrawTransaction',37,0),(46,'TransferWithdrawTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (1,11,'ga','Chequing','CAD',0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY) VALUES
           (4,20,'gc4','Food',NULL),(5,20,'gc5','Groceries',4),(6,20,'gc6','Fun',NULL),(7,20,'gc7','Dining/Out',4),
           (8,20,'gc8','Pets/Cats',NULL),(9,20,'gc9','Pets',NULL),(10,20,'gc10','Cats',9);",
    )
    .unwrap();
    for (pk, ent, amount, day, desc) in [
        (100, 48, -12.0, "2026-09-01", "COFFEE CANARYMW1"),
        (101, 48, -12.0, "2026-09-01", "BAKERY CANARYMW2"),
        (102, 46, -50.0, "2026-09-02", "TRANSFER CANARYMW3"),
        (103, 48, -30.0, "2026-09-03", "CINEMA CANARYMW4"),
        (104, 48, -7.0, "2026-09-04", "ETRANSFER CANARYMW5"),
        (105, 48, -7.0, "2026-09-04", "ETRANSFER CANARYMW5"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZSTATUS1) VALUES (?1,?2,?3,1,?4,?5,?6,?7)",
            rusqlite::params![pk, ent, format!("g{pk}"), amount, ts(day), desc, if pk == 101 { 1 } else { 2 }],
        )
        .unwrap();
    }
    c.execute("INSERT INTO ZCATEGORYASSIGMENT VALUES (1,103,6,-30.0)", [])
        .unwrap();
    drop(c);
    moneywiz::open(&path).unwrap()
}

fn file(text: &str) -> EditsFile {
    toml::from_str(text).unwrap()
}

#[test]
fn rows_resolve_to_exactly_one_row_by_gid() {
    let s = store("rows");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-01"
            amount = -12.00
            set_memo = "two rows match"

            [[row]]
            account = "Chequing"
            date = "2026-09-01"
            amount = -12.00
            text = "bakery"
            set_amount = -12.5
            set_category = "Food/Groceries"

            [[row]]
            account = "Chequing"
            date = "2026-09-02"
            amount = -50.00
            set_amount = -55
            "#,
        ),
        None,
    );
    assert_eq!(r.plan.rows.len(), 1);
    let e = &r.plan.rows[0];
    assert_eq!(
        (e.gid.as_str(), e.expect_amount, e.amount),
        ("g101", -12.0, Some(-12.5))
    );
    assert_eq!(
        e.category.as_deref(),
        Some(&["Food".to_string(), "Groceries".to_string()][..])
    );
    assert!(r.problems[0].starts_with("row #1: 2 MoneyWiz rows match"));
    assert!(r.problems[1].contains("transfers have two sides"));
    assert!(
        !r.problems.join(" ").contains("CANARY"),
        "problems name edits by number only"
    );
}

#[test]
fn category_changes_follow_renames_and_guard_deletes() {
    let s = store("cats");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[category]]
            rename = "Food"
            to = "Eating"

            [[category]]
            add = "Eating/Groceries"

            [[category]]
            add = "Eating/Coffee"

            [[category]]
            delete = "Fun"

            [[category]]
            delete = "Eating"

            [[row]]
            account = "Chequing"
            date = "2026-09-01"
            amount = -12.00
            text = "coffee"
            set_category = "Eating/Coffee"
            "#,
        ),
        None,
    );
    assert_eq!(r.already, 1, "the renamed subcategory already exists");
    let ops: Vec<&str> = r.plan.categories.iter().map(|c| c.op.as_str()).collect();
    assert_eq!(ops, ["rename", "add"]);
    assert_eq!(
        r.plan.rows.len(),
        1,
        "a row can use a category added in the same file"
    );
    assert!(
        r.problems
            .iter()
            .any(|p| p == "category #4: 1 rows use it; add move_to")
    );
    assert!(
        r.problems
            .iter()
            .any(|p| p == "category #5: delete or move its subcategories first")
    );
}

#[test]
fn verify_counts_what_the_store_shows() {
    let s = store("verify");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            "[[row]]\naccount = \"Chequing\"\ndate = \"2026-09-03\"\namount = -30\nset_amount = -31\n",
        ),
        None,
    );
    let (report, missing) = moneywiz_edit::verify(&s, &r.plan);
    assert_eq!(missing, 1);
    assert!(report.starts_with("Verified 0 of 1 changes"));
}

#[test]
fn a_category_name_with_a_slash_is_found_whole() {
    let s = store("slash");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_category = "Food/Dining/Out"
            "#,
        ),
        None,
    );
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(
        r.plan.rows[0].category.as_deref(),
        Some(&["Food".to_string(), "Dining/Out".to_string()][..])
    );
}

#[test]
fn a_name_that_fits_a_slash_name_and_a_subcategory_is_refused() {
    let s = store("slashclash");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_category = "Pets/Cats"
            "#,
        ),
        None,
    );
    assert!(r.plan.rows.is_empty());
    assert!(
        r.problems
            .iter()
            .any(|p| p.contains("slash name and a subcategory")),
        "{:?}",
        r.problems
    );
    // The exact path parts name one of the two.
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_category_path = ["Pets/Cats"]
            "#,
        ),
        None,
    );
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.plan.rows.len(), 1);
}

#[test]
fn set_date_moves_a_row_and_refuses_a_bad_day() {
    let s = store("set-date");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            "[[row]]\naccount = \"Chequing\"\ndate = \"2026-09-03\"\namount = -30\nset_date = \"2026-09-01\"\n",
        ),
        None,
    );
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.plan.rows[0].date.as_deref(), Some("2026-09-01"));
    let bad = moneywiz_edit::resolve(
        &s,
        &file(
            "[[row]]\naccount = \"Chequing\"\ndate = \"2026-09-03\"\namount = -30\nset_date = \"Sep 1\"\n",
        ),
        None,
    );
    assert!(
        bad.problems.iter().any(|p| p.contains("YYYY-MM-DD")),
        "{:?}",
        bad.problems
    );
}

#[test]
fn merge_moves_rows_in_the_targets_currency_and_drops_hand_trades() {
    let _ = store("merge");
    let path = std::env::temp_dir()
        .join(format!("cashcast-mw-edit-merge-{}", std::process::id()))
        .join("MoneyWiz.sqlite");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(
        "INSERT INTO Z_PRIMARYKEY VALUES (12,'InvestmentAccount',10,0),(47,'InvestmentExchangeTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES
           (2,11,'gold','Old','CAD',0),(3,11,'goldeur','Old','EUR',0),(30,12,'gnew','New','CAD',0);
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZINVESTMENTHOLDING INTEGER;
         ALTER TABLE ZSYNCOBJECT ADD COLUMN ZSYMBOL VARCHAR;
         INSERT INTO Z_PRIMARYKEY VALUES (60,'InvestmentHolding',0,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZSYMBOL) VALUES (98,60,'cad'),(99,60,'BTC');",
    )
    .unwrap();
    for (pk, ent, account, desc) in [
        (200, 48, 2, "FEE CANARYMW5"),
        (201, 46, 2, "TO BANK CANARYMW6"),
        (202, 47, 2, "HAND TRADE CANARYMW7"),
        (203, 48, 3, "EURO ROW CANARYMW8"),
        (204, 48, 2, "COIN IN CANARYMW9"),
        (205, 48, 2, "CAD ON HOLDING CANARYMW10"),
        (206, 46, 2, "COIN TO WALLET CANARYMW11"),
    ] {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,?2,?3,?4,-5.0,?5,?6)",
            rusqlite::params![pk, ent, format!("g{pk}"), account, ts("2026-09-05"), desc],
        )
        .unwrap();
    }
    c.execute(
        "UPDATE ZSYNCOBJECT SET ZINVESTMENTHOLDING=99 WHERE Z_PK=204",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE ZSYNCOBJECT SET ZINVESTMENTHOLDING=99, ZRECIPIENTACCOUNT1=3 WHERE Z_PK=206",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE ZSYNCOBJECT SET ZINVESTMENTHOLDING=98 WHERE Z_PK=205",
        [],
    )
    .unwrap();
    drop(c);
    let s = moneywiz::open(&path).unwrap();
    let merge = [("Old".to_string(), "New".to_string())]
        .into_iter()
        .collect();
    let broker = [(
        "New".to_string(),
        "kraken-ledger-csv:non-registered".to_string(),
    )]
    .into_iter()
    .collect();
    let taken = std::collections::HashSet::new();

    let (edits, problems) = moneywiz_edit::merge_edits(&s, &merge, &broker, &taken);
    assert!(problems.is_empty(), "{problems:?}");
    let by = |g: &str| edits.iter().find(|e| e.gid == g);
    assert_eq!(by("g200").unwrap().account.as_deref(), Some("gnew"));
    assert_eq!(by("g201").unwrap().account.as_deref(), Some("gnew"));
    let trade = by("g202").expect("the hand trade is planned");
    assert!(trade.delete && trade.account.is_none());
    let coin = by("g204").expect("a row on a holding is planned");
    assert!(coin.delete, "a row on a coin holding is a hand trade too");
    assert!(!coin.unlink, "a plain coin row has no other side");
    let sent = by("g206").expect("a coin transfer to another account is planned");
    assert!(
        sent.delete && sent.unlink,
        "the other account keeps its coins as a plain row; the broker file has this side"
    );
    assert_eq!(
        by("g205").unwrap().account.as_deref(),
        Some("gnew"),
        "a forex account's own currency is cash"
    );
    assert!(
        by("g203").is_none(),
        "a EUR row never moves into a CAD account"
    );

    // Retiring an account deletes every row; a transfer to another account
    // leaves that account a plain row.
    let retire = [("Old".to_string(), "cad".to_string())]
        .into_iter()
        .collect();
    let gone = moneywiz_edit::retire_edits(&s, &retire, &std::collections::HashSet::new());
    let g = |id: &str| gone.iter().find(|e| e.gid == id).expect("every row goes");
    assert!(gone.iter().all(|e| e.delete));
    assert!(
        g("g206").unlink,
        "a transfer's other account keeps a plain row"
    );
    assert!(
        !g("g200").unlink && !g("g201").unlink,
        "no other account, nothing to keep"
    );
    assert!(
        gone.iter().all(|e| e.gid != "g203"),
        "only the named currency's account"
    );

    // A balance read on a site with no export: the opening balance moves so
    // MoneyWiz ends that day at it, counting rows the plan already deletes.
    let new = s.accounts.iter().find(|a| a.name == "New").unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
    let mut plan = moneywiz_edit::WriterPlan::default();
    let read = [("New".to_string(), "12.34@2030-01-01".to_string())]
        .into_iter()
        .collect();
    assert!(moneywiz_edit::balance_edits(&s, &read, &mut plan).is_empty());
    let gap = 1234 - s.balance_on(new.pk, day);
    assert_eq!(plan.accounts.len(), usize::from(gap != 0));
    if let Some(e) = plan.accounts.first() {
        assert_eq!(e.gid, new.gid);
        assert_eq!(((e.opening - e.expect_opening) * 100.0).round() as i64, gap);
    }
    let bad = [("New".to_string(), "soon".to_string())]
        .into_iter()
        .collect();
    let problems =
        moneywiz_edit::balance_edits(&s, &bad, &mut moneywiz_edit::WriterPlan::default());
    assert!(problems[0].contains("AMOUNT@YYYY-MM-DD"));

    // A row the plan moves in, with a new amount, counts in the gap.
    let moved = s.txns.iter().find(|t| t.gid == "g200").unwrap();
    let mut edit = by_gid(&s, "g200", &new.gid);
    edit.amount = Some(5.0);
    let mut plan = moneywiz_edit::WriterPlan {
        rows: vec![edit],
        ..Default::default()
    };
    assert!(moneywiz_edit::balance_edits(&s, &read, &mut plan).is_empty());
    let counted = if moved.date <= day { 500 } else { 0 };
    let gap = 1234 - (s.balance_on(new.pk, day) + counted);
    let got = plan.accounts.first().map_or(0, |e| {
        ((e.opening - e.expect_opening) * 100.0).round() as i64
    });
    assert_eq!(got, gap);
    // A second fix for the same account is a problem, not a silent skip.
    if !plan.accounts.is_empty() {
        let problems = moneywiz_edit::balance_edits(&s, &read, &mut plan);
        assert!(problems[0].contains("skipped"), "{problems:?}");
    }

    // Without a broker file the hand trade cannot move or go.
    let none = std::collections::BTreeMap::new();
    let (edits, problems) = moneywiz_edit::merge_edits(&s, &merge, &none, &taken);
    assert!(edits.iter().all(|e| e.gid != "g202" && e.gid != "g204"));
    assert_eq!(problems.len(), 1);

    // A row another edit already changes stays out.
    let taken = ["g200".to_string()].into_iter().collect();
    let (edits, _) = moneywiz_edit::merge_edits(&s, &merge, &broker, &taken);
    assert!(edits.iter().all(|e| e.gid != "g200"));

    // A missing target is a problem, not a guess.
    let wrong = [("Old".to_string(), "Nope".to_string())]
        .into_iter()
        .collect();
    let (edits, problems) = moneywiz_edit::merge_edits(&s, &wrong, &broker, &taken);
    assert!(edits.is_empty());
    assert!(problems[0].contains("0 MoneyWiz accounts"));

    // The plan shows the moves; the check needs the row in the new account.
    let plan = moneywiz_edit::WriterPlan {
        contract: moneywiz_edit::CONTRACT,
        rows: vec![by_gid(&s, "g200", "gnew")],
        ..Default::default()
    };
    let (_, missing) = moneywiz_edit::verify(&s, &plan);
    assert_eq!(missing, 1, "the row is still in the old account");
    let text = moneywiz_edit::render(
        &moneywiz_edit::Resolved {
            plan,
            problems: vec![],
            already: 0,
            rows_already: 0,
        },
        "data/p.json",
    );
    assert!(text.contains("1 moves to another account"), "{text}");
}

fn by_gid(s: &moneywiz::Store, gid: &str, to: &str) -> moneywiz_edit::RowEdit {
    let t = s.txns.iter().find(|t| t.gid == gid).unwrap();
    moneywiz_edit::RowEdit {
        entity: t.entity.clone(),
        gid: t.gid.clone(),
        expect_amount: t.cents as f64 / 100.0,
        expect_date: t.date.to_string(),
        amount: None,
        category: None,
        category_kind: None,
        payee: None,
        memo: None,
        date: None,
        delete: false,
        account: Some(to.into()),
        unlink: false,
    }
}

#[test]
fn a_category_edit_picks_the_one_matching_row_with_no_category() {
    let _ = store("uncat");
    let path = std::env::temp_dir()
        .join(format!("cashcast-mw-edit-uncat-{}", std::process::id()))
        .join("MoneyWiz.sqlite");
    let c = Connection::open(&path).unwrap();
    // A second -30.00 row on the day of the categorized row 103.
    c.execute(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (106,48,'g106',1,-30.0,?1,'CINEMA CANARYMW6')",
        rusqlite::params![ts("2026-09-03")],
    )
    .unwrap();
    drop(c);
    let s = moneywiz::open(&path).unwrap();
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_category = "Food/Groceries"

            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_memo = "a memo edit still needs text"
            "#,
        ),
        None,
    );
    let gids: Vec<_> = r.plan.rows.iter().map(|e| e.gid.as_str()).collect();
    assert_eq!(gids, ["g106"], "{:?}", r.problems);
    assert!(
        r.problems[0].starts_with("row #2: 2 MoneyWiz rows match"),
        "{:?}",
        r.problems
    );
}

#[test]
fn nth_picks_one_of_identical_rows_only() {
    let s = store("nth");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-04"
            amount = -7.00
            nth = 1
            set_category = "Fun"

            [[row]]
            account = "Chequing"
            date = "2026-09-04"
            amount = -7.00
            nth = 2
            set_category = "Fun"

            [[row]]
            account = "Chequing"
            date = "2026-09-01"
            amount = -12.00
            nth = 1
            set_category = "Fun"
            "#,
        ),
        None,
    );
    let gids: Vec<_> = r.plan.rows.iter().map(|e| e.gid.as_str()).collect();
    assert_eq!(gids, ["g104", "g105"]);
    // Rows that differ need text, not a position.
    assert!(
        r.problems[0].starts_with("row #3: 2 MoneyWiz rows match"),
        "{:?}",
        r.problems
    );
}

/// `delete = true` removes one row the owner names (a placeholder no bank
/// row backs), alone: it never mixes with a change, and a transfer, whose
/// other side would stay behind, is refused.
#[test]
fn delete_removes_one_named_row_alone() {
    let s = store("delete-row");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            "[[row]]\naccount = \"Chequing\"\ndate = \"2026-09-03\"\namount = -30\ndelete = true\n",
        ),
        None,
    );
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.plan.rows.len(), 1);
    assert!(r.plan.rows[0].delete);
    assert_eq!(r.plan.rows[0].gid, "g103");
    let mixed = moneywiz_edit::resolve(
        &s,
        &file(
            "[[row]]\naccount = \"Chequing\"\ndate = \"2026-09-03\"\namount = -30\ndelete = true\nset_memo = \"x\"\n",
        ),
        None,
    );
    assert!(
        mixed.problems.iter().any(|p| p.contains("delete")),
        "{:?}",
        mixed.problems
    );
    let transfer = moneywiz_edit::resolve(
        &s,
        &file(
            "[[row]]\naccount = \"Chequing\"\ndate = \"2026-09-02\"\namount = -50\ndelete = true\n",
        ),
        None,
    );
    assert!(
        transfer.problems.iter().any(|p| p.contains("transfer")),
        "{:?}",
        transfer.problems
    );
}

#[test]
fn archive_edit_shows_an_archived_account_and_verify_checks_it() {
    let name = "archive";
    let _ = store(name);
    let path = std::env::temp_dir()
        .join(format!("cashcast-mw-edit-{name}-{}", std::process::id()))
        .join("MoneyWiz.sqlite");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(
        "ALTER TABLE ZSYNCOBJECT ADD COLUMN ZARCHIVED INTEGER;
         UPDATE ZSYNCOBJECT SET ZARCHIVED = 1 WHERE Z_PK = 1;",
    )
    .unwrap();
    let s = moneywiz::open(&path).unwrap();
    assert!(s.accounts[0].archived);

    // Already archived: nothing to do. Unknown name: refused.
    assert!(
        moneywiz_edit::archive_edit(&s, "Chequing", true)
            .unwrap_err()
            .contains("already archived")
    );
    assert!(moneywiz_edit::archive_edit(&s, "Savings", false).is_err());

    let e = moneywiz_edit::archive_edit(&s, "Chequing", false).unwrap();
    assert_eq!((e.gid.as_str(), e.expect, e.archived), ("ga", true, false));
    let r = moneywiz_edit::Resolved {
        plan: moneywiz_edit::WriterPlan {
            archive: vec![e],
            ..Default::default()
        },
        problems: vec![],
        already: 0,
        rows_already: 0,
    };
    assert!(!moneywiz_edit::is_empty(&r.plan));
    assert!(moneywiz_edit::render(&r, "plan.json").contains("1 archived account to show again"));
    let plan = r.plan;
    let (_, missing) = moneywiz_edit::verify(&s, &plan);
    assert_eq!(missing, 1, "still archived: not verified");

    c.execute("UPDATE ZSYNCOBJECT SET ZARCHIVED = 0 WHERE Z_PK = 1", [])
        .unwrap();
    let shown = moneywiz::open(&path).unwrap();
    let (report, missing) = moneywiz_edit::verify(&shown, &plan);
    assert_eq!(missing, 0, "{report}");
    assert!(report.starts_with("Verified 1 of 1 changes"), "{report}");
}

#[test]
fn a_row_edit_already_in_moneywiz_is_not_planned_again() {
    // CINEMA (103) already sits in "Fun": the edit is done, so a second
    // run plans nothing and says so (2026-11-22: 382 no-op category edits
    // showed up as changes on every `edit --fixes`).
    let s = store("already");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_category = "Fun"

            [[row]]
            account = "Chequing"
            date = "2026-09-03"
            amount = -30.00
            set_category = "Food/Groceries"
            "#,
        ),
        None,
    );
    assert_eq!(r.rows_already, 1, "the Fun edit is already there");
    assert_eq!(r.plan.rows.len(), 1, "only the real change stays");
    assert_eq!(
        r.plan.rows[0].category.as_deref(),
        Some(&["Food".to_string(), "Groceries".to_string()][..])
    );
    let out = moneywiz_edit::render(&r, "plan.json");
    assert!(out.contains("(1 already done)"), "{out}");
}

#[test]
fn a_category_add_names_its_kind_for_a_parent_of_two_kinds() {
    let s = store("kind");
    let r = moneywiz_edit::resolve(
        &s,
        &file(
            r#"
            [[category]]
            add = "Food/Shared"
            kind = "expense"

            [[category]]
            add = "Fun/Other"
            "#,
        ),
        None,
    );
    let kinds: Vec<_> = r.plan.categories.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [Some(1), None], "{:?}", r.problems);
    let json = serde_json::to_string(&r.plan).unwrap();
    assert!(json.contains("\"kind\":1"), "{json}");
}

#[test]
fn fixes_mark_pending_rows_cleared() {
    let s = store("pending");
    assert!(s.txns.iter().any(|t| t.pk == 101 && t.pending));
    let empty = moneywiz::Plan {
        accounts: Vec::new(),
        untouched: Vec::new(),
        unlinked_exports: Vec::new(),
        file_named: Vec::new(),
    };
    let r = moneywiz_edit::resolve(&s, &file(""), Some((&[], &empty)));
    let gids: Vec<&str> = r.plan.clear.iter().map(|c| c.gid.as_str()).collect();
    assert_eq!(gids, ["g101"], "only the pending row is cleared");
    assert!(!moneywiz_edit::is_empty(&r.plan));
    let none = moneywiz_edit::resolve(&s, &file(""), None);
    assert!(none.plan.clear.is_empty(), "only --fixes clears");
}
