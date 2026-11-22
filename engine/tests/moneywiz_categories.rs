//! MoneyWiz category migrations on a synthetic MoneyWiz database. No real data.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cashcast_engine::moneywiz::{self, Store};
use cashcast_engine::moneywiz_categories::{self as cats, Migration, Op};
use cashcast_engine::moneywiz_edit::{self, WriterPlan};
use chrono::NaiveDate;
use rusqlite::Connection;

fn ts(date: &str) -> f64 {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    (d.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() - 978_307_200) as f64
}

/// A home with a MoneyWiz database (types: 1 expense, 2 income) and a
/// profile whose budget names a category MoneyWiz lacks.
fn home(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("cashcast-mw-cats-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inbox")).unwrap();
    let db = dir.join("MoneyWiz.sqlite");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER PRIMARY KEY, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);
         CREATE TABLE ZSYNCOBJECT (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, ZGID VARCHAR, ZNAME VARCHAR,
           ZCURRENCYNAME VARCHAR, ZOPENINGBALANCE FLOAT, ZNAME2 VARCHAR, ZPARENTCATEGORY INTEGER, ZTYPE2 INTEGER,
           ZAMOUNT1 FLOAT, ZDATE1 TIMESTAMP, ZDESC2 VARCHAR, ZNOTES1 VARCHAR, ZACCOUNT2 INTEGER);
         CREATE TABLE ZCATEGORYASSIGMENT (Z_PK INTEGER PRIMARY KEY, ZTRANSACTION INTEGER, ZCATEGORY INTEGER, ZAMOUNT FLOAT);
         INSERT INTO Z_PRIMARYKEY VALUES (10,'Account',0,0),(11,'BankChequeAccount',10,0),(20,'Category',0,0),
           (37,'Transaction',0,0),(38,'DepositTransaction',37,0),(48,'WithdrawTransaction',37,0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE) VALUES (1,11,'ga','Chequing','CAD',0);
         INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY,ZTYPE2) VALUES
           (4,20,'gc4','Food',NULL,1),(5,20,'gc5','Groceries',4,1),(6,20,'gc6','groceries',NULL,1),
           (7,20,'gc7','Fun',NULL,1),(8,20,'gc8','Salary',NULL,2),(9,20,'gc9','Refunds',NULL,1),
           (10,20,'gc10','Pharmacy',NULL,1),(11,20,'gc11','Health',NULL,1);",
    )
    .unwrap();
    let rows: [(i64, i64, f64, &str, i64); 12] = [
        (100, 48, -10.0, "GROCER CANARYMC1", 5),
        (101, 48, -20.0, "GROCER CANARYMC2", 5),
        (102, 48, -30.0, "GROCER CANARYMC3", 5),
        (103, 48, -5.0, "GROCER CANARYMC4", 6),
        (104, 38, 3000.0, "PAYROLL CANARYMC5", 8),
        (105, 38, 10.0, "REBATE CANARYMC6", 9),
        (106, 38, 11.0, "REBATE CANARYMC7", 9),
        (107, 38, 12.0, "REBATE CANARYMC8", 9),
        (108, 48, -8.0, "SHOPPERS DRUG MART CANARYMC9", 10),
        (109, 48, -40.0, "DENTAL CANARYMCA", 11),
        (110, 48, -60.0, "CLINIC CANARYMCB", 11),
        (111, 48, -1.0, "NO CATEGORY CANARYMCC", 0),
    ];
    for (pk, ent, amount, desc, cat) in rows {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,?2,?3,1,?4,?5,?6)",
            rusqlite::params![pk, ent, format!("g{pk}"), amount, ts("2026-09-01"), desc],
        )
        .unwrap();
        if cat > 0 {
            c.execute(
                "INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (?1,?2,?3)",
                rusqlite::params![pk, cat, amount],
            )
            .unwrap();
        }
    }
    std::fs::write(
        dir.join("profile.toml"),
        "[budget]\n\"Food > Coffee\" = 50\n\"Food\" = 500\n",
    )
    .unwrap();
    (dir, db)
}

/// Stands in for engine/moneywiz-writer on the synthetic store, in the
/// writer's order: adds, renames, rows, deletes.
fn write(db: &Path, plan: &WriterPlan) {
    let c = Connection::open(db).unwrap();
    let cat = |path: &[String]| -> i64 {
        let mut parent: Option<i64> = None;
        for name in path {
            parent = Some(
                c.query_row(
                    "SELECT Z_PK FROM ZSYNCOBJECT WHERE Z_ENT=20 AND ZNAME2=?1 AND ZPARENTCATEGORY IS ?2",
                    rusqlite::params![name, parent],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| panic!("no category {path:?}")),
            );
        }
        parent.unwrap()
    };
    for op in plan.categories.iter().filter(|o| o.op == "add") {
        let parent = (op.path.len() > 1).then(|| cat(&op.path[..op.path.len() - 1]));
        let template = parent
            .or_else(|| op.like.as_ref().map(|l| cat(l)))
            .unwrap_or(4);
        let kind: i64 = c
            .query_row(
                "SELECT ZTYPE2 FROM ZSYNCOBJECT WHERE Z_PK=?1",
                [template],
                |r| r.get(0),
            )
            .unwrap();
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY,ZTYPE2) VALUES (20,'new',?1,?2,?3)",
            rusqlite::params![op.path.last().unwrap(), parent, kind],
        )
        .unwrap();
    }
    for op in plan.categories.iter().filter(|o| o.op == "rename") {
        c.execute(
            "UPDATE ZSYNCOBJECT SET ZNAME2=?1 WHERE Z_PK=?2",
            rusqlite::params![op.to.as_ref().unwrap(), cat(&op.path)],
        )
        .unwrap();
    }
    for e in &plan.rows {
        let (pk, amount): (i64, f64) = c
            .query_row(
                "SELECT Z_PK, ZAMOUNT1 FROM ZSYNCOBJECT WHERE ZGID=?1",
                [&e.gid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((amount - e.expect_amount).abs() < 0.005);
        if let Some(path) = &e.category {
            c.execute("DELETE FROM ZCATEGORYASSIGMENT WHERE ZTRANSACTION=?1", [pk])
                .unwrap();
            c.execute(
                "INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (?1,?2,?3)",
                rusqlite::params![pk, cat(path), amount],
            )
            .unwrap();
        }
    }
    for op in plan.categories.iter().filter(|o| o.op == "delete") {
        let old = cat(&op.path);
        if let Some(m) = &op.move_to {
            c.execute(
                "UPDATE ZCATEGORYASSIGMENT SET ZCATEGORY=?1 WHERE ZCATEGORY=?2",
                rusqlite::params![cat(m), old],
            )
            .unwrap();
        }
        let left: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM ZCATEGORYASSIGMENT WHERE ZCATEGORY=?1",
                [old],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "rows still use a category to delete");
        c.execute("DELETE FROM ZSYNCOBJECT WHERE Z_PK=?1", [old])
            .unwrap();
    }
}

fn row_categories(s: &Store) -> Vec<(String, Vec<String>)> {
    let mut v: Vec<_> = s
        .txns
        .iter()
        .map(|t| (t.gid.clone(), t.category.clone()))
        .collect();
    v.sort();
    v
}

fn migration(store: &Store, ops: &str) -> Migration {
    toml::from_str(&format!(
        "expect_tree = \"{}\"\n{ops}",
        cats::fingerprint(store)
    ))
    .unwrap()
}

#[test]
fn list_shows_types_and_counts_only() {
    let (home, db) = home("list");
    let s = moneywiz::open(&db).unwrap();
    let out = cats::render_list(&s, &home).unwrap();
    assert!(
        out.contains("8 categories (7 expense, 1 income). 11 rows have a category, 1 have none.")
    );
    assert!(out.contains("| Food/Groceries | expense | 3 | 0 | 0 |"));
    assert!(out.contains("| Refunds | expense | 3 | 3 | 0 |"));
    assert!(out.contains("| Salary | income | 1 | 1 | 0 |"));
    assert!(out.contains("Migrations: 0 applied, 0 pending."));
    assert!(!out.contains("CANARY"), "no payees in the list");
}

#[test]
fn suggestions_cover_each_rule_and_form_one_valid_migration() {
    let (home, db) = home("suggest");
    let s = moneywiz::open(&db).unwrap();
    let out = cats::write_suggestions(&s, &home, "2026-11-01").unwrap();
    for want in [
        "uses \"Food/Coffee\", which MoneyWiz does not have",
        "\"Refunds\" is an expense category, but 3 of its rows are income",
        "\"groceries\" (1 row) looks like a duplicate of \"Food/Groceries\" (3 rows)",
        "\"Pharmacy\" has only 1 row, and they look like Health: move it under \"Health\"",
        "\"Fun\" has no rows.",
        "Wrote inbox/moneywiz-migrations/0001-suggested.toml (5 changes)",
    ] {
        assert!(out.contains(want), "missing {want:?} in\n{out}");
    }
    assert!(!out.contains("CANARY"));
    let dir = home.join("inbox").join(cats::DIR);
    let (files, _) = cats::files(&dir).unwrap();
    let (m, _) = cats::load_migration(&files[0]).unwrap();
    let c = cats::compile(&s, &m).unwrap();
    assert_eq!((c.adds, c.moves, c.deletes, c.rows_moved), (3, 1, 3, 5));

    // Suggesting again replaces the unapplied suggestion, keeping its number.
    cats::write_suggestions(&s, &home, "2026-11-02").unwrap();
    let (files, _) = cats::files(&dir).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file, "0001-suggested.toml");
}

#[test]
fn migrate_then_rollback_brings_back_the_tree_and_every_row() {
    let (home, db) = home("roundtrip");
    let s = moneywiz::open(&db).unwrap();
    let before_tree = cats::fingerprint(&s);
    let before_rows = row_categories(&s);
    cats::write_suggestions(&s, &home, "2026-11-01").unwrap();

    let out = cats::plan(&s, &home, false).unwrap();
    assert!(out.contains("Migration 0001-suggested.toml"), "{out}");
    assert!(out.contains("Rollback computed and checked"));
    let plan_text =
        std::fs::read_to_string(home.join("data").join(moneywiz_edit::PLAN_FILE)).unwrap();
    assert!(
        !plan_text.contains("CANARY"),
        "the plan names rows by GID only"
    );
    let plan: WriterPlan = serde_json::from_str(&plan_text).unwrap();
    write(&db, &plan);

    let after = moneywiz::open(&db).unwrap();
    let (report, missing) = moneywiz_edit::verify(&after, &plan);
    assert_eq!(missing, 0, "{report}");
    let paths: Vec<String> = after.categories.iter().map(|p| p.join("/")).collect();
    for want in ["Food/Coffee", "Refunds (income)", "Health/Pharmacy"] {
        assert!(paths.contains(&want.to_string()), "{paths:?}");
    }
    for gone in ["groceries", "Fun", "Pharmacy"] {
        assert!(!paths.contains(&gone.to_string()));
    }
    assert_eq!(
        after.category_types[&vec!["Refunds (income)".to_string()]],
        cats::INCOME
    );
    assert!(
        cats::commit(&after, &home, Some("data/moneywiz-backups/x".into()))
            .unwrap()
            .contains("Recorded 0001-suggested.toml as applied")
    );
    let record = std::fs::read_to_string(home.join("data").join(cats::APPLIED_FILE)).unwrap();
    assert!(
        !record.contains("CANARY") && !record.contains("3000"),
        "no row content in the record"
    );
    assert!(
        cats::plan(&after, &home, false)
            .unwrap()
            .contains("No pending migrations.")
    );

    // Rollback: the computed inverse, through the same writer.
    cats::plan(&after, &home, true).unwrap();
    let plan: WriterPlan = serde_json::from_str(
        &std::fs::read_to_string(home.join("data").join(moneywiz_edit::PLAN_FILE)).unwrap(),
    )
    .unwrap();
    write(&db, &plan);
    let back = moneywiz::open(&db).unwrap();
    assert_eq!(moneywiz_edit::verify(&back, &plan).1, 0);
    assert_eq!(cats::fingerprint(&back), before_tree);
    assert_eq!(
        row_categories(&back),
        before_rows,
        "every row is back in its category"
    );
    assert!(
        cats::commit(&back, &home, None)
            .unwrap()
            .contains("Rolled back 0001-suggested.toml")
    );
    assert!(
        cats::render_list(&back, &home)
            .unwrap()
            .contains("Migrations: 0 applied, 1 pending")
    );
}

#[test]
fn drift_and_unsafe_orders_are_refused() {
    let (home, db) = home("refuse");
    let s = moneywiz::open(&db).unwrap();
    let mut m = migration(&s, "[[op]]\ndelete = \"Fun\"\n");
    m.expect_tree = "0000000000000000".into();
    let e = cats::compile(&s, &m).err().unwrap();
    assert!(
        e.contains("changed since this migration was written"),
        "{e}"
    );

    let reuse = migration(
        &s,
        "[[op]]\nrename = \"Fun\"\nto = \"Play\"\n[[op]]\nadd = \"Fun\"\nkind = \"expense\"\n",
    );
    assert!(
        cats::compile(&s, &reuse)
            .err()
            .unwrap()
            .contains("split it into two migrations")
    );

    let in_use = migration(&s, "[[op]]\ndelete = \"Health\"\n");
    assert_eq!(
        cats::compile(&s, &in_use).err().unwrap(),
        "op #1: 2 rows use it; add move_to"
    );

    let top = migration(&s, "[[op]]\nadd = \"Gifts\"\n");
    assert!(
        cats::compile(&s, &top)
            .err()
            .unwrap()
            .contains("needs kind")
    );

    // A move of only income rows, then a rename of its target, in one plan.
    let ok = migration(
        &s,
        "[[op]]\nadd = \"Rebates\"\nkind = \"income\"\n[[op]]\nmove = \"Refunds\"\nto = \"Rebates\"\nonly = \"income\"\n[[op]]\nrename = \"Rebates\"\nto = \"Cash back\"\n",
    );
    let c = cats::compile(&s, &ok).unwrap();
    assert_eq!(
        c.plan.categories[0].path,
        vec!["Rebates".to_string()],
        "adds use names before renames"
    );
    assert_eq!(
        c.plan.categories[0].like,
        Some(vec!["Salary".to_string()]),
        "copies an income category"
    );
    assert!(
        c.plan
            .rows
            .iter()
            .all(|r| r.category == Some(vec!["Cash back".to_string()]))
    );
    assert_eq!(c.plan.rows.len(), 3);
    write(&db, &c.plan);
    let after = moneywiz::open(&db).unwrap();
    assert_eq!(moneywiz_edit::verify(&after, &c.plan).1, 0);

    // A rollback whose rows moved again since is refused.
    let mut back = migration(&after, "");
    back.op = c.inverse.clone();
    assert!(cats::compile(&after, &back).is_ok());
    back.op.push(Op {
        delete: Some("Nope".into()),
        ..Default::default()
    });
    assert!(
        cats::compile(&after, &back)
            .err()
            .unwrap()
            .contains("no such category")
    );
    let _ = home;
}

#[test]
fn a_name_may_hold_a_slash() {
    let (_home, db) = home("slash");
    Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY,ZTYPE2) VALUES (12,20,'gc12','Dining/Eating Out',4,1)",
            [],
        )
        .unwrap();
    let s = moneywiz::open(&db).unwrap();
    let m = migration(
        &s,
        "[[op]]\nmove = \"Food/Groceries\"\nto = \"Food/Dining/Eating Out\"\n[[op]]\nadd = \"Food/Dining/Eating Out/Lunch\"\n[[op]]\ndelete = \"groceries\"\nmove_to = \"Food/Dining/Eating Out\"\n",
    );
    let c = cats::compile(&s, &m).unwrap();
    let slash = vec!["Food".to_string(), "Dining/Eating Out".to_string()];
    assert_eq!(
        c.plan.categories[0].path,
        [slash.clone(), vec!["Lunch".to_string()]].concat()
    );
    assert!(
        c.plan
            .rows
            .iter()
            .all(|r| r.category.as_ref() == Some(&slash))
    );
    assert_eq!(c.plan.categories[1].move_to.as_ref(), Some(&slash));
    write(&db, &c.plan);
    assert_eq!(
        moneywiz_edit::verify(&moneywiz::open(&db).unwrap(), &c.plan).1,
        0
    );
}

/// A top-level "Other" must not merge into "Bank/Other" when another
/// subcategory is also named "Other", or when it holds rows of the other type.
#[test]
fn a_shared_leaf_name_or_mixed_rows_is_not_a_duplicate() {
    let (home, db) = home("dup-other");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY,ZTYPE2) VALUES
           (12,20,'gc12','Bank',NULL,2),(13,20,'gc13','Other',12,2),
           (14,20,'gc14','Other',11,1),(15,20,'gc15','Other',NULL,2);",
    )
    .unwrap();
    let rows: [(i64, i64, f64, &str, i64); 8] = [
        (200, 38, 1.0, "INTEREST CANARYMD1", 13),
        (201, 38, 2.0, "INTEREST CANARYMD2", 13),
        (202, 38, 3.0, "INTEREST CANARYMD3", 13),
        (203, 38, 4.0, "INTEREST CANARYMD4", 13),
        (204, 38, 5.0, "INTEREST CANARYMD5", 13),
        (205, 38, 6.0, "GIFT CANARYMD6", 15),
        (206, 38, 7.0, "GIFT CANARYMD7", 15),
        (207, 48, -8.0, "SHOP CANARYMD8", 15),
    ];
    for (pk, ent, amount, desc, cat) in rows {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,?2,?3,1,?4,?5,?6)",
            rusqlite::params![pk, ent, format!("g{pk}"), amount, ts("2026-09-01"), desc],
        )
        .unwrap();
        c.execute(
            "INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (?1,?2,?3)",
            rusqlite::params![pk, cat, amount],
        )
        .unwrap();
    }
    let merge = "\"Other\" (3 rows) looks like a duplicate of \"Bank/Other\"";
    let suggest =
        || cats::write_suggestions(&moneywiz::open(&db).unwrap(), &home, "2026-11-01").unwrap();

    // Two subcategories end in "Other": the leaf name alone is not enough.
    let out = suggest();
    assert!(!out.contains(merge), "unexpected merge in\n{out}");

    // One "Other" subcategory left, but the top-level one holds an expense row.
    c.execute(
        "UPDATE ZSYNCOBJECT SET ZPARENTCATEGORY=NULL, ZNAME2='Misc' WHERE Z_PK=14",
        [],
    )
    .unwrap();
    let out = suggest();
    assert!(!out.contains(merge), "unexpected merge in\n{out}");

    // Only income rows left: now it is a duplicate.
    c.execute(
        "UPDATE ZCATEGORYASSIGMENT SET ZCATEGORY=13 WHERE ZTRANSACTION=207",
        [],
    )
    .unwrap();
    let out = suggest();
    assert!(
        out.contains("\"Other\" (2 rows) looks like a duplicate of \"Bank/Other\" (6 rows)"),
        "missing merge in\n{out}"
    );
}

/// Two categories with one path but different types must give the same
/// fingerprint on every read: the first one (lowest key) gives the type.
#[test]
fn the_fingerprint_is_stable_when_two_categories_share_a_path() {
    let (_home, db) = home("same-path");
    Connection::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY,ZTYPE2) VALUES
               (12,20,'gc12','Other',NULL,2),(13,20,'gc13','Other',NULL,1);",
        )
        .unwrap();
    let prints: HashSet<String> = (0..32)
        .map(|_| cats::fingerprint(&moneywiz::open(&db).unwrap()))
        .collect();
    assert_eq!(prints.len(), 1, "fingerprints: {prints:?}");
    let s = moneywiz::open(&db).unwrap();
    assert_eq!(s.category_types.get(&vec!["Other".to_string()]), Some(&2));
}

/// Two categories named "Other" at one level are one path to cashcast: it
/// must not suggest or apply changes there, and it says to rename one. A
/// wrong-type move looks for its target among siblings only.
#[test]
fn a_shared_path_is_left_alone_and_wrong_type_moves_stay_among_siblings() {
    let (home, db) = home("shared-path");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZNAME2,ZPARENTCATEGORY,ZTYPE2) VALUES
           (12,20,'gc12','Other',NULL,1),(13,20,'gc13','Other',NULL,2),
           (14,20,'gc14','Bank',NULL,2),(15,20,'gc15','Other',14,2),
           (16,20,'gc16','Gifts',NULL,1),(17,20,'gc17','Family',NULL,2),(18,20,'gc18','Gifts',17,2);",
    )
    .unwrap();
    let rows: [(i64, f64, &str, i64); 9] = [
        (200, 1.0, "GIFT CANARYME1", 13),
        (201, 2.0, "GIFT CANARYME2", 13),
        (202, 3.0, "GIFT CANARYME3", 13),
        (203, 4.0, "GIFT CANARYME4", 13),
        (204, 5.0, "INTEREST CANARYME5", 15),
        (205, 6.0, "INTEREST CANARYME6", 15),
        (206, 7.0, "PRESENT CANARYME7", 16),
        (207, 8.0, "PRESENT CANARYME8", 16),
        (208, 9.0, "PRESENT CANARYME9", 16),
    ];
    for (pk, amount, desc, cat) in rows {
        c.execute(
            "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZGID,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2) VALUES (?1,38,?2,1,?3,?4,?5)",
            rusqlite::params![pk, format!("g{pk}"), amount, ts("2026-09-01"), desc],
        )
        .unwrap();
        c.execute(
            "INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (?1,?2,?3)",
            rusqlite::params![pk, cat, amount],
        )
        .unwrap();
    }
    let s = moneywiz::open(&db).unwrap();
    let out = cats::write_suggestions(&s, &home, "2026-11-01").unwrap();
    assert!(
        out.contains("Two MoneyWiz categories are named \"Other\": rename one in MoneyWiz"),
        "missing note in\n{out}"
    );
    assert!(
        !out.contains("\"Other\" is an"),
        "unexpected move in\n{out}"
    );
    assert!(
        !out.contains("\"Bank/Other\""),
        "unexpected target in\n{out}"
    );
    // "Family/Gifts" is not a sibling of "Gifts": a new category is added.
    assert!(
        out.contains("\"Gifts\" is an expense category, but 3 of its rows are income"),
        "missing move in\n{out}"
    );
    let dir = home.join("inbox").join(cats::DIR);
    let (files, _) = cats::files(&dir).unwrap();
    let (m, _) = cats::load_migration(&files[0]).unwrap();
    let c = cats::compile(&s, &m).unwrap();
    // 3 adds and 1 move come from the base store.
    assert_eq!((c.adds, c.moves), (4, 2));
    assert!(
        c.plan
            .categories
            .iter()
            .any(|op| op.path == ["Gifts (income)".to_string()])
    );

    for ops in [
        "[[op]]\ndelete = \"Other\"\n",
        "[[op]]\nmove = \"Bank/Other\"\nto = \"Other\"\n",
        "[[op]]\nadd = \"Other/Cash\"\n",
    ] {
        let e = cats::compile(&s, &migration(&s, ops))
            .err()
            .unwrap_or_default();
        assert!(e.contains("rename one in MoneyWiz"), "{ops:?} gave {e:?}");
    }
}

/// MoneyWiz also links categories to rows cashcast does not read (such as
/// scheduled transactions). A rollback could not bring those links back, so
/// such a category is never deleted.
#[test]
fn a_category_with_other_links_is_not_deleted() {
    let (home, db) = home("other-links");
    Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (999,7,-1)",
            [],
        )
        .unwrap();
    let s = moneywiz::open(&db).unwrap();
    let out = cats::write_suggestions(&s, &home, "2026-11-01").unwrap();
    assert!(
        out.contains("\"Fun\" has no rows here, but 1 other MoneyWiz links use it"),
        "missing note in\n{out}"
    );
    assert!(
        !out.contains("\"Fun\" has no rows."),
        "unexpected delete in\n{out}"
    );
    let dir = home.join("inbox").join(cats::DIR);
    let (files, _) = cats::files(&dir).unwrap();
    let (m, _) = cats::load_migration(&files[0]).unwrap();
    assert_eq!(cats::compile(&s, &m).unwrap().deletes, 2);
    for ops in [
        "[[op]]\ndelete = \"Fun\"\n",
        "[[op]]\ndelete = \"Fun\"\nmove_to = \"Food\"\n",
    ] {
        let e = cats::compile(&s, &migration(&s, ops))
            .err()
            .unwrap_or_default();
        assert!(
            e.contains("1 other MoneyWiz links use it"),
            "{ops:?} gave {e:?}"
        );
    }
}

#[test]
fn the_match_table_pairs_each_category_and_web_picks_become_a_migration() {
    let (home, db) = home("view");
    let s = moneywiz::open(&db).unwrap();
    let v: serde_json::Value = serde_json::from_str(&cats::view_json(&s, &home).unwrap()).unwrap();
    let cats_json = v["categories"].as_array().unwrap();
    let food = cats_json.iter().find(|c| c["path"] == "Food").unwrap();
    assert_eq!(food["cashcast"], "Food");
    let fun = cats_json.iter().find(|c| c["path"] == "Fun").unwrap();
    assert!(fun["cashcast"].is_null());
    assert!(
        !fun["suggested"].as_array().unwrap().is_empty(),
        "an empty category is suggested"
    );
    assert_eq!(v["missing"], serde_json::json!(["Food/Coffee"]));
    assert_eq!(v["applied"], 0);
    assert!(!v.to_string().contains("CANARY"), "names and counts only");

    let ops: Vec<Op> = serde_json::from_str(
        r#"[{"rename":"groceries","to":"Groceries 2"},{"delete":"Fun"},{"add":"Food/Coffee"}]"#,
    )
    .unwrap();
    let out = cats::write_ops(&s, &home, Some("tidy"), &ops).unwrap();
    assert!(out.contains("0001-web.toml (3 changes)"), "{out}");
    let (files, _) = cats::files(&home.join("inbox").join(cats::DIR)).unwrap();
    let (m, _) = cats::load_migration(&files[0]).unwrap();
    let c = cats::compile(&s, &m).unwrap();
    assert_eq!((c.adds, c.deletes), (1, 1));
    // Picking again replaces the unapplied web file; a bad pick writes nothing.
    cats::write_ops(&s, &home, None, &ops[..1]).unwrap();
    assert_eq!(
        cats::files(&home.join("inbox").join(cats::DIR))
            .unwrap()
            .0
            .len(),
        1
    );
    let bad: Vec<Op> = serde_json::from_str(r#"[{"delete":"Nope"}]"#).unwrap();
    assert!(cats::write_ops(&s, &home, None, &bad).is_err());
    assert!(cats::write_ops(&s, &home, None, &[]).is_err());
}
