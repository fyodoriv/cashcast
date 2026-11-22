//! Groceries: every price seen at every store, so a run reuses fresh prices,
//! picks the best-value pack (lowest price per 100 g/ml, bulk allowed) plus a
//! substitute, and totals each store's cart with its fees and promos. Orders
//! placed are logged, so grocery spending reads per month. Moved from
//! sidequest (`sidequest grocery`) on 2026-11-22.
//!
//! State scope: one SQLite file per user at `<home>/data/groceries.sqlite`,
//! opened per call and closed when the call returns. Nothing here leaves the
//! computer; store pages are read by the agent in the owner's browser
//! (`skills/grocery-cart`).

use chrono::{Duration, NaiveDate};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The default organic premium: organic wins when its price per 100 is at most
/// this share above the cheapest (0.75 = 75 %).
pub const ORGANIC_PREMIUM: f64 = 0.75;
/// The default age, in days, after which a price counts as stale.
pub const FRESH_DAYS: i64 = 3;

/// One price read on a store page.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Row {
    pub item: String,
    pub store: String,
    pub name: String,
    pub price: f64,
    #[serde(default)]
    pub was: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub organic: bool,
    #[serde(default)]
    pub checked: Option<String>,
}

/// A saved price with its price per 100 g/ml (or `None` for counts).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Priced {
    pub item: String,
    pub store: String,
    pub name: String,
    pub price: f64,
    pub was: Option<f64>,
    pub unit: Option<String>,
    pub link: Option<String>,
    pub organic: bool,
    pub per100: Option<f64>,
    pub checked: String,
}

/// A store's fee rule after the owner's memberships, and its promo.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Fee {
    pub store: String,
    pub delivery: f64,
    pub service: f64,
    pub free_over: Option<f64>,
    /// Store promo, for example 30 % off up to $19 on $20 or more.
    pub pct_off: Option<f64>,
    pub max_off: Option<f64>,
    pub min_order: Option<f64>,
    pub note: String,
    pub checked: String,
}

/// One grocery order the owner placed.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Order {
    pub day: String,
    pub store: String,
    pub total: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Line {
    pub item: String,
    pub best: Option<Priced>,
    pub sub: Option<Priced>,
}

/// One store's cart: the best pack of each item, fees, promo and total.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Basket {
    pub store: String,
    pub lines: Vec<Line>,
    pub missing: Vec<String>,
    pub subtotal: f64,
    pub discount: f64,
    pub delivery: Option<f64>,
    pub service: Option<f64>,
    pub total: Option<f64>,
    pub fee_note: String,
}

/// An item with saved prices: how many stores and the newest check.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Known {
    pub item: String,
    pub stores: i64,
    pub checked: String,
}

/// Grocery spending in one month, from the orders log.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Month {
    pub month: String,
    pub orders: i64,
    pub total: f64,
}

pub type Result<T> = std::result::Result<T, String>;

/// The database file of a user's home.
pub fn file(home: &Path) -> PathBuf {
    home.join("data").join("groceries.sqlite")
}

fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let c = Connection::open(path).map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| e.to_string())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS prices (item TEXT, store TEXT, name TEXT, price REAL,
           was REAL, unit TEXT, per100 REAL, organic INT, link TEXT, checked TEXT,
           PRIMARY KEY (item, store, name, checked));
         CREATE TABLE IF NOT EXISTS fees (store TEXT PRIMARY KEY, delivery REAL, service REAL,
           free_over REAL, note TEXT, checked TEXT, pct_off REAL, max_off REAL, min_order REAL);
         CREATE TABLE IF NOT EXISTS orders (day TEXT, store TEXT, total REAL, note TEXT);",
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(c)
}

fn round(n: f64) -> f64 {
    (n * 1000.0).round() / 1000.0
}

fn cents(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

static PER100: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\$?\s*([\d.]+)\s*(¢?)\s*/\s*100\s*(?:g|ml|gram)?\b").unwrap()
});
static PER_KG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\$?\s*([\d.]+)\s*(¢?)\s*/\s*(?:1\s*)?(?:kg|l|litre)\b").unwrap()
});
static SIZE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:(\d+)\s*[x×]\s*)?([\d.]+)\s*(ml|l|litre|g|kg)\b").unwrap()
});

/// Price per 100 g or ml, from a unit label ("$1.20/100g", "$4/kg") or the
/// size in the unit or name ("4x355 ml", "1 l", "1.4 kg"). `None` for counts.
pub fn per100(price: f64, unit: Option<&str>, name: &str) -> Option<f64> {
    let u = unit.unwrap_or("");
    let num = |s: &str| s.parse::<f64>().ok();
    if let Some(m) = PER100.captures(u) {
        let n = num(&m[1])?;
        return Some(round(if m[2].is_empty() { n } else { n / 100.0 }));
    }
    if let Some(m) = PER_KG.captures(u) {
        let n = num(&m[1])?;
        return Some(round(if m[2].is_empty() { n } else { n / 100.0 } / 10.0));
    }
    let text = format!("{u} {name}");
    let m = SIZE.captures(&text)?;
    let count = m.get(1).map_or(Some(1.0), |c| num(c.as_str()))?;
    let n = count * num(&m[2])?;
    let unit = m[3].to_lowercase();
    let base = if matches!(unit.as_str(), "l" | "litre" | "kg") {
        n * 1000.0
    } else {
        n
    };
    (base > 0.0).then(|| round(price / base * 100.0))
}

fn organic_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("organic") || n.contains("biologique")
}

/// Saves price rows; rows with no item, store, name or price are skipped.
pub fn save(path: &Path, rows: &[Row], today: NaiveDate) -> Result<usize> {
    let mut c = open(path)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut n = 0;
    {
        let mut st = tx
            .prepare("INSERT OR REPLACE INTO prices VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .map_err(|e| e.to_string())?;
        for r in rows {
            if r.item.trim().is_empty() || r.store.is_empty() || r.name.is_empty() || r.price <= 0.0
            {
                continue;
            }
            st.execute(params![
                r.item.trim().to_lowercase(),
                r.store,
                r.name,
                r.price,
                r.was,
                r.unit,
                per100(r.price, r.unit.as_deref(), &r.name),
                (r.organic || organic_name(&r.name)) as i64,
                r.link,
                r.checked.clone().unwrap_or_else(|| today.to_string()),
            ])
            .map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(n)
}

/// The latest price of each product for an item, no older than `days`.
pub fn latest(path: &Path, item: &str, days: i64, today: NaiveDate) -> Result<Vec<Priced>> {
    if !path.exists() {
        // Reading never creates the database.
        return Ok(Vec::new());
    }
    let c = open(path)?;
    let item = item.trim().to_lowercase();
    let since = (today - Duration::days(days)).to_string();
    let mut st = c
        .prepare(
            "SELECT p.item, p.store, p.name, p.price, p.was, p.unit, p.link, p.organic, p.per100, p.checked
             FROM prices p JOIN (SELECT store, name, MAX(checked) c FROM prices
               WHERE item = ?1 AND checked >= ?2 GROUP BY store, name) l
             ON p.store = l.store AND p.name = l.name AND p.checked = l.c
             WHERE p.item = ?1 ORDER BY p.store, p.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map(params![item, since], |r| {
            Ok(Priced {
                item: r.get(0)?,
                store: r.get(1)?,
                name: r.get(2)?,
                price: r.get(3)?,
                was: r.get(4)?,
                unit: r.get(5)?,
                link: r.get(6)?,
                organic: r.get::<_, i64>(7)? == 1,
                per100: r.get(8)?,
                checked: r.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Items with no price newer than `days`: only these need a live search.
pub fn stale(path: &Path, items: &[String], days: i64, today: NaiveDate) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for i in items {
        if latest(path, i, days, today)?.is_empty() {
            out.push(i.clone());
        }
    }
    Ok(out)
}

/// Every item with saved prices.
pub fn known(path: &Path) -> Result<Vec<Known>> {
    if !path.exists() {
        // Reading never creates the database.
        return Ok(Vec::new());
    }
    let c = open(path)?;
    let mut st = c
        .prepare(
            "SELECT item, COUNT(DISTINCT store), MAX(checked) FROM prices GROUP BY item ORDER BY item",
        )
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| {
            Ok(Known {
                item: r.get(0)?,
                stores: r.get(1)?,
                checked: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// The best pack and a substitute at one store. Organic wins when its price
/// per 100 is at most `premium` above the cheapest; the substitute is the next
/// best product with another name.
pub fn pick(rows: &[Priced], premium: f64) -> Option<(Priced, Option<Priced>)> {
    let mut sized: Vec<&Priced> = rows.iter().filter(|r| r.per100.is_some()).collect();
    sized.sort_by(|a, b| a.per100.partial_cmp(&b.per100).unwrap());
    // Products with no size (counts) stay as later choices, cheapest first.
    let mut counts: Vec<&Priced> = rows.iter().filter(|r| r.per100.is_none()).collect();
    counts.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    let all: Vec<&Priced> = sized.into_iter().chain(counts).collect();
    let first = *all.first()?;
    let floor = first.per100.unwrap_or(first.price);
    let bi = all
        .iter()
        .position(|r| r.organic && r.per100.unwrap_or(r.price) <= floor * (1.0 + premium))
        .unwrap_or(0);
    let best = all[bi];
    let sub = all
        .iter()
        .enumerate()
        .find(|(i, r)| *i != bi && r.name != best.name)
        .map(|(_, r)| (*r).clone());
    Some((best.clone(), sub))
}

/// Saves a store's fee rule with today's date.
pub fn set_fee(path: &Path, f: &Fee, today: NaiveDate) -> Result<()> {
    let c = open(path)?;
    let checked = if f.checked.is_empty() {
        today.to_string()
    } else {
        f.checked.clone()
    };
    c.execute(
        "INSERT OR REPLACE INTO fees (store, delivery, service, free_over, note, checked, pct_off, max_off, min_order)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            f.store, f.delivery, f.service, f.free_over, f.note, checked, f.pct_off, f.max_off,
            f.min_order
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn fees(path: &Path) -> Result<Vec<Fee>> {
    if !path.exists() {
        // Reading never creates the database.
        return Ok(Vec::new());
    }
    let c = open(path)?;
    let mut st = c
        .prepare(
            "SELECT store, delivery, service, free_over, pct_off, max_off, min_order, note, checked
             FROM fees ORDER BY store",
        )
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| {
            Ok(Fee {
                store: r.get(0)?,
                delivery: r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                service: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                free_over: r.get(3)?,
                pct_off: r.get(4)?,
                max_off: r.get(5)?,
                min_order: r.get(6)?,
                note: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                checked: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// One store's cart for these items.
pub fn basket(
    path: &Path,
    store: &str,
    items: &[String],
    premium: f64,
    days: i64,
    today: NaiveDate,
) -> Result<Basket> {
    let mut lines = Vec::new();
    for item in items {
        let here: Vec<Priced> = latest(path, item, days, today)?
            .into_iter()
            .filter(|r| r.store == store)
            .collect();
        let (best, sub) = match pick(&here, premium) {
            Some((b, s)) => (Some(b), s),
            None => (None, None),
        };
        lines.push(Line {
            item: item.clone(),
            best,
            sub,
        });
    }
    let subtotal = round(lines.iter().filter_map(|l| l.best.as_ref()).map(|b| b.price).sum());
    let f = fees(path)?.into_iter().find(|x| x.store == store);
    // A subtotal with missing items is too low to trust: free delivery and a
    // promo minimum count only for a full basket.
    let full = lines.iter().all(|l| l.best.is_some());
    let delivery = f.as_ref().map(|f| match f.free_over {
        Some(over) if full && subtotal >= over => 0.0,
        _ => f.delivery,
    });
    let discount = match &f {
        Some(f) if f.pct_off.is_some_and(|p| p > 0.0) && full && subtotal >= f.min_order.unwrap_or(0.0) => {
            round((subtotal * f.pct_off.unwrap() / 100.0).min(f.max_off.unwrap_or(f64::INFINITY)))
        }
        _ => 0.0,
    };
    let service = f.as_ref().map(|f| f.service);
    let total = delivery.map(|d| round(subtotal - discount + d + service.unwrap_or(0.0)));
    Ok(Basket {
        store: store.to_string(),
        missing: lines
            .iter()
            .filter(|l| l.best.is_none())
            .map(|l| l.item.clone())
            .collect(),
        lines,
        subtotal,
        discount,
        delivery,
        service,
        total,
        fee_note: f.map_or_else(
            || "no fee rule saved: run `bin/cashcast grocery fee STORE --delivery N`".to_string(),
            |f| f.note,
        ),
    })
}

/// Every store with fresh prices for these items, full baskets first, then
/// the cheapest total.
pub fn compare(
    path: &Path,
    items: &[String],
    premium: f64,
    days: i64,
    today: NaiveDate,
) -> Result<Vec<Basket>> {
    let mut stores = BTreeSet::new();
    for i in items {
        for r in latest(path, i, days, today)? {
            stores.insert(r.store);
        }
    }
    let mut out = Vec::new();
    for s in stores {
        out.push(basket(path, &s, items, premium, days, today)?);
    }
    out.sort_by(|a, b| {
        a.missing.len().cmp(&b.missing.len()).then(
            a.total
                .unwrap_or(1e9)
                .partial_cmp(&b.total.unwrap_or(1e9))
                .unwrap(),
        )
    });
    Ok(out)
}

/// Logs an order the owner placed.
pub fn record_order(path: &Path, o: &Order) -> Result<()> {
    let c = open(path)?;
    c.execute(
        "INSERT INTO orders VALUES (?, ?, ?, ?)",
        params![o.day, o.store, cents(o.total), o.note],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Every logged order, newest first.
pub fn orders(path: &Path) -> Result<Vec<Order>> {
    if !path.exists() {
        // Reading never creates the database.
        return Ok(Vec::new());
    }
    let c = open(path)?;
    let mut st = c
        .prepare("SELECT day, store, total, note FROM orders ORDER BY day DESC, rowid DESC")
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| {
            Ok(Order {
                day: r.get(0)?,
                store: r.get(1)?,
                total: r.get(2)?,
                note: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Grocery spending per month, newest first.
pub fn months(orders: &[Order]) -> Vec<Month> {
    let mut out: Vec<Month> = Vec::new();
    for o in orders {
        let m = o.day.get(..7).unwrap_or(&o.day).to_string();
        match out.iter_mut().find(|x| x.month == m) {
            Some(x) => {
                x.orders += 1;
                x.total = cents(x.total + o.total);
            }
            None => out.push(Month {
                month: m,
                orders: 1,
                total: cents(o.total),
            }),
        }
    }
    out.sort_by(|a, b| b.month.cmp(&a.month));
    out
}

/// Copies prices and fee rules from an older groceries database (sidequest's
/// `groceries.sqlite`, same tables). The source is opened read-only and left
/// as it is. Returns (prices, fee rules) copied.
pub fn import(path: &Path, from: &Path) -> Result<(usize, usize)> {
    if !from.is_file() {
        return Err(format!("no database at {}", from.display()));
    }
    let src = Connection::open_with_flags(from, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let has = |t: &str| -> bool {
        src.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
            [t],
            |_| Ok(()),
        )
        .is_ok()
    };
    if !has("prices") {
        return Err(format!("{} has no prices table", from.display()));
    }
    let mut dst = open(path)?;
    let tx = dst.transaction().map_err(|e| e.to_string())?;
    let mut prices = 0;
    {
        let mut read = src
            .prepare("SELECT item, store, name, price, was, unit, per100, organic, link, checked FROM prices")
            .map_err(|e| e.to_string())?;
        let mut write = tx
            .prepare("INSERT OR REPLACE INTO prices VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .map_err(|e| e.to_string())?;
        let mut rows = read.query([]).map_err(|e| e.to_string())?;
        while let Some(r) = rows.next().map_err(|e| e.to_string())? {
            let v: Vec<rusqlite::types::Value> = (0..10)
                .map(|i| r.get(i))
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| e.to_string())?;
            write
                .execute(rusqlite::params_from_iter(v))
                .map_err(|e| e.to_string())?;
            prices += 1;
        }
    }
    let mut fee_rules = 0;
    if has("fees") {
        let cols: Vec<String> = src
            .prepare("SELECT name FROM pragma_table_info('fees')")
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .map_err(|e| e.to_string())?;
        let col = |c: &str| {
            if cols.iter().any(|x| x == c) {
                c.to_string()
            } else {
                "NULL".to_string()
            }
        };
        let sql = format!(
            "SELECT store, delivery, service, free_over, note, checked, {}, {}, {} FROM fees",
            col("pct_off"),
            col("max_off"),
            col("min_order")
        );
        let mut read = src.prepare(&sql).map_err(|e| e.to_string())?;
        let mut write = tx
            .prepare(
                "INSERT OR REPLACE INTO fees (store, delivery, service, free_over, note, checked, pct_off, max_off, min_order)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = read.query([]).map_err(|e| e.to_string())?;
        while let Some(r) = rows.next().map_err(|e| e.to_string())? {
            let v: Vec<rusqlite::types::Value> = (0..9)
                .map(|i| r.get(i))
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| e.to_string())?;
            write
                .execute(rusqlite::params_from_iter(v))
                .map_err(|e| e.to_string())?;
            fee_rules += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok((prices, fee_rules))
}

/// The owner's grocery preferences: `[groceries]` in profile.toml.
#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    pub organic_premium: f64,
    pub fresh_days: i64,
    /// Every other key, as `key = value` lines (rules, quantities, memberships…).
    pub lines: Vec<String>,
}

pub fn prefs(profile: &toml::Table) -> Prefs {
    let g = profile.get("groceries").and_then(|v| v.as_table());
    let num = |k: &str| {
        g.and_then(|t| t.get(k)).and_then(|v| {
            v.as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
        })
    };
    let mut lines = Vec::new();
    if let Some(t) = g {
        for (k, v) in t {
            if k == "organic_premium" || k == "fresh_days" {
                continue;
            }
            match v {
                toml::Value::Array(a) => {
                    lines.push(format!("{k}:"));
                    for x in a {
                        lines.push(format!(
                            "  - {}",
                            x.as_str().map_or_else(|| x.to_string(), str::to_string)
                        ));
                    }
                }
                toml::Value::Table(t) => {
                    lines.push(format!("{k}:"));
                    for (kk, vv) in t {
                        lines.push(format!(
                            "  - {kk}: {}",
                            vv.as_str().map_or_else(|| vv.to_string(), str::to_string)
                        ));
                    }
                }
                other => lines.push(format!(
                    "{k}: {}",
                    other.as_str().map_or_else(|| other.to_string(), str::to_string)
                )),
            }
        }
    }
    Prefs {
        organic_premium: num("organic_premium").unwrap_or(ORGANIC_PREMIUM),
        fresh_days: num("fresh_days").map_or(FRESH_DAYS, |d| d as i64),
        lines,
    }
}

fn money(n: Option<f64>) -> String {
    n.map_or_else(|| "?".to_string(), |n| format!("{n:.2}"))
}

/// One price as a line: price, sale, organic, name, price per 100.
pub fn line(r: &Priced) -> String {
    let mut s = money(Some(r.price));
    if let Some(w) = r.was {
        s += &format!(" (was {})", money(Some(w)));
    }
    s.push(' ');
    if r.organic {
        s += "organic ";
    }
    s += &r.name;
    if let Some(p) = r.per100 {
        s += &format!(" [{p}/100]");
    }
    s
}

pub fn render_compare(baskets: &[Basket]) -> String {
    baskets
        .iter()
        .map(|b| {
            let mut head = format!(
                "{}: {} (items {}",
                b.store,
                money(b.total),
                money(Some(b.subtotal))
            );
            if b.discount > 0.0 {
                head += &format!(", promo -{}", money(Some(b.discount)));
            }
            head += &format!(
                ", delivery {}, service {})",
                money(b.delivery),
                money(b.service)
            );
            if !b.missing.is_empty() {
                head += &format!(" missing: {}", b.missing.join(", "));
            }
            let mut out = vec![head];
            for l in &b.lines {
                out.push(format!(
                    "  {}: {}",
                    l.item,
                    l.best.as_ref().map_or_else(|| "-".to_string(), line)
                ));
                if let Some(s) = &l.sub {
                    out.push(format!("    sub: {}", line(s)));
                }
            }
            if b.total.is_none() {
                out.push(format!("  {}", b.fee_note));
            }
            out.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_fee(f: &Fee) -> String {
    let mut s = format!("{:<26} delivery {}", f.store, money(Some(f.delivery)));
    if let Some(o) = f.free_over {
        s += &format!(" (free over {})", money(Some(o)));
    }
    s += &format!(" service {}", money(Some(f.service)));
    if let Some(p) = f.pct_off.filter(|p| *p > 0.0) {
        s += &format!(
            " promo {p}% off up to {} on {}+",
            money(Some(f.max_off.unwrap_or(0.0))),
            money(Some(f.min_order.unwrap_or(0.0)))
        );
    }
    if !f.note.is_empty() {
        s += &format!(" {}", f.note);
    }
    if !f.checked.is_empty() {
        s += &format!(" (checked {})", f.checked);
    }
    s
}
