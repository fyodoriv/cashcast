//! Grocery prices, best packs, store totals, orders and the import from an
//! older database. Synthetic stores and products only; names carry canaries.

use cashcast_engine::groceries::{self, Fee, Order, Row};
use chrono::NaiveDate;
use std::path::PathBuf;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

const TODAY: &str = "2026-11-20";

fn db(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cashcast-groceries-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    groceries::file(&dir)
}

fn row(item: &str, store: &str, name: &str, price: f64, unit: Option<&str>) -> Row {
    Row {
        item: item.into(),
        store: store.into(),
        name: name.into(),
        price,
        unit: unit.map(str::to_string),
        ..Row::default()
    }
}

fn items(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn per100_reads_unit_labels_and_pack_sizes() {
    assert_eq!(groceries::per100(3.0, Some("$1.20/100g"), "x"), Some(1.2));
    assert_eq!(groceries::per100(3.0, Some("85¢/100 ml"), "x"), Some(0.85));
    assert_eq!(groceries::per100(3.0, Some("$4.00/1kg"), "x"), Some(0.4));
    // A bulk pack counts every can: 12 x 330 ml for 9.90 is 0.25 per 100 ml.
    assert_eq!(groceries::per100(9.9, None, "Fizz CANARY41 12 x 330 mL"), Some(0.25));
    assert_eq!(groceries::per100(5.0, None, "Juice 1 L"), Some(0.5));
    // Counts have no size: they sort after sized packs.
    assert_eq!(groceries::per100(2.0, None, "Avocado CANARY42"), None);
}

#[test]
fn best_pack_prefers_organic_within_the_premium_and_names_a_substitute() {
    let path = db("pick");
    let rows = vec![
        row("oats", "store-a", "Oats CANARY43 1 kg", 4.0, None),
        row("oats", "store-a", "Organic Oats CANARY44 1 kg", 6.0, None),
        row("oats", "store-a", "Oats bulk CANARY45 2 kg", 7.0, None),
    ];
    assert_eq!(groceries::save(&path, &rows, d(TODAY)).unwrap(), 3);
    let fresh = groceries::latest(&path, "Oats", 3, d(TODAY)).unwrap();
    assert_eq!(fresh.len(), 3);
    // Organic 0.6/100 g is within 75 % of the cheapest 0.35/100 g.
    let (best, sub) = groceries::pick(&fresh, 0.75).unwrap();
    assert!(best.organic);
    assert_eq!(best.name, "Organic Oats CANARY44 1 kg");
    assert_eq!(sub.unwrap().name, "Oats bulk CANARY45 2 kg");
    // With no premium the bulk pack wins on price per 100 g.
    let (best, _) = groceries::pick(&fresh, 0.0).unwrap();
    assert_eq!(best.name, "Oats bulk CANARY45 2 kg");
}

#[test]
fn old_prices_are_stale_and_only_they_need_a_search() {
    let path = db("stale");
    let mut old = row("rice", "store-a", "Rice CANARY46 2 kg", 5.0, None);
    old.checked = Some("2026-11-10".into());
    groceries::save(&path, &[old, row("tea", "store-a", "Tea CANARY47 100 g", 3.0, None)], d(TODAY))
        .unwrap();
    let st = groceries::stale(&path, &items(&["rice", "tea", "milk"]), 3, d(TODAY)).unwrap();
    assert_eq!(st, items(&["rice", "milk"]));
    assert_eq!(groceries::latest(&path, "rice", 14, d(TODAY)).unwrap().len(), 1);
}

#[test]
fn compare_ranks_full_baskets_first_with_fees_free_delivery_and_promos() {
    let path = db("compare");
    groceries::save(
        &path,
        &[
            row("oats", "store-a", "Oats CANARY48 1 kg", 20.0, None),
            row("tea", "store-a", "Tea CANARY49 100 g", 15.0, None),
            row("oats", "store-b", "Oats CANARY50 1 kg", 18.0, None),
            row("tea", "store-b", "Tea CANARY51 100 g", 14.0, None),
            row("oats", "store-c", "Oats CANARY52 1 kg", 1.0, None),
        ],
        d(TODAY),
    )
    .unwrap();
    let fee = |store: &str, delivery: f64| Fee {
        store: store.into(),
        delivery,
        ..Fee::default()
    };
    // store-a: free delivery over 30, so 35 items + 0 delivery.
    groceries::set_fee(&path, &Fee { free_over: Some(30.0), service: 1.0, ..fee("store-a", 5.0) }, d(TODAY)).unwrap();
    // store-b: 30 % off up to 5 on 20 or more: 32 - 5 + 4.
    groceries::set_fee(
        &path,
        &Fee { pct_off: Some(30.0), max_off: Some(5.0), min_order: Some(20.0), ..fee("store-b", 4.0) },
        d(TODAY),
    )
    .unwrap();
    let res = groceries::compare(&path, &items(&["oats", "tea"]), 0.75, 3, d(TODAY)).unwrap();
    let names: Vec<&str> = res.iter().map(|b| b.store.as_str()).collect();
    // store-c is cheapest but misses tea, and has no fee rule: it ranks last.
    assert_eq!(names, ["store-b", "store-a", "store-c"]);
    assert_eq!(res[0].total, Some(31.0));
    assert_eq!(res[0].discount, 5.0);
    assert_eq!(res[1].total, Some(36.0));
    assert_eq!(res[1].delivery, Some(0.0));
    assert_eq!(res[2].missing, items(&["tea"]));
    assert_eq!(res[2].total, None);
    let text = groceries::render_compare(&res);
    assert!(text.contains("store-b: 31.00 (items 32.00, promo -5.00, delivery 4.00, service 0.00)"));
    assert!(text.contains("store-c: ? (items 1.00, delivery ?, service ?) missing: tea"));
    assert!(text.contains("no fee rule saved"));
}

#[test]
fn a_promo_and_free_delivery_need_a_full_basket() {
    let path = db("partial");
    groceries::save(&path, &[row("oats", "store-a", "Oats CANARY53 1 kg", 50.0, None)], d(TODAY)).unwrap();
    groceries::set_fee(
        &path,
        &Fee {
            store: "store-a".into(),
            delivery: 5.0,
            free_over: Some(10.0),
            pct_off: Some(10.0),
            ..Fee::default()
        },
        d(TODAY),
    )
    .unwrap();
    let b = groceries::basket(&path, "store-a", &items(&["oats", "tea"]), 0.75, 3, d(TODAY)).unwrap();
    assert_eq!(b.delivery, Some(5.0));
    assert_eq!(b.discount, 0.0);
    assert_eq!(b.total, Some(55.0));
}

#[test]
fn orders_sum_per_month_newest_first() {
    let path = db("orders");
    for (day, total) in [("2026-10-03", 80.5), ("2026-11-01", 100.25), ("2026-11-15", 60.0)] {
        groceries::record_order(
            &path,
            &Order { day: day.into(), store: "store-a".into(), total, note: "CANARY54".into() },
        )
        .unwrap();
    }
    let all = groceries::orders(&path).unwrap();
    assert_eq!(all[0].day, "2026-11-15");
    let m = groceries::months(&all);
    assert_eq!((m[0].month.as_str(), m[0].orders, m[0].total), ("2026-11", 2, 160.25));
    assert_eq!((m[1].month.as_str(), m[1].orders, m[1].total), ("2026-10", 1, 80.5));
}

#[test]
fn import_copies_prices_and_fees_from_an_older_database_and_leaves_it_as_is() {
    // The older layout: fees without the promo columns.
    let old = db("import-src");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(&old).unwrap();
    c.execute_batch(
        "CREATE TABLE prices (item TEXT, store TEXT, name TEXT, price REAL, was REAL, unit TEXT,
           per100 REAL, organic INT, link TEXT, checked TEXT, PRIMARY KEY (item, store, name, checked));
         CREATE TABLE fees (store TEXT PRIMARY KEY, delivery REAL, service REAL, free_over REAL, note TEXT, checked TEXT);
         INSERT INTO prices VALUES ('tea','store-a','Tea CANARY55 100 g',3.0,NULL,NULL,3.0,0,NULL,'2026-11-19');
         INSERT INTO fees VALUES ('store-a',4.99,1.0,35.0,'CANARY56','2026-11-19');",
    )
    .unwrap();
    drop(c);
    let before = std::fs::read(&old).unwrap();
    let path = db("import-dst");
    assert_eq!(groceries::import(&path, &old).unwrap(), (1, 1));
    assert_eq!(std::fs::read(&old).unwrap(), before);
    assert_eq!(groceries::latest(&path, "tea", 3, d(TODAY)).unwrap()[0].price, 3.0);
    let f = &groceries::fees(&path).unwrap()[0];
    assert_eq!((f.delivery, f.free_over, f.pct_off), (4.99, Some(35.0), None));
    // Running it again replaces the same rows: no doubles.
    assert_eq!(groceries::import(&path, &old).unwrap(), (1, 1));
    assert_eq!(groceries::known(&path).unwrap()[0].stores, 1);
    assert!(groceries::import(&path, &old.with_file_name("missing.sqlite")).is_err());
}

#[test]
fn prefs_read_the_groceries_table_with_defaults() {
    let t: toml::Table = toml::from_str(
        "[groceries]\norganic_premium = 0.5\nrules = [\"never CANARY57\"]\n[groceries.quantities]\noats = \"2 bags\"\n",
    )
    .unwrap();
    let p = groceries::prefs(&t);
    assert_eq!((p.organic_premium, p.fresh_days), (0.5, 3));
    assert_eq!(p.lines, ["rules:", "  - never CANARY57", "quantities:", "  - oats: 2 bags"]);
    let none = groceries::prefs(&toml::Table::new());
    assert_eq!((none.organic_premium, none.fresh_days), (0.75, 3));
}

#[test]
fn reading_an_empty_home_creates_no_database() {
    let path = db("read-only");
    assert!(groceries::known(&path).unwrap().is_empty());
    assert!(groceries::compare(&path, &items(&["oats"]), 0.75, 3, d(TODAY)).unwrap().is_empty());
    assert!(groceries::orders(&path).unwrap().is_empty());
    assert!(!path.exists());
}
