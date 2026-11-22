//! The MoneyWiz connection: reads the MoneyWiz database on this Mac, matches
//! it with the bank and card exports in the inbox, and adds what is missing
//! through MoneyWiz's own URL scheme.
//!
//! Privacy boundary: this module reads raw rows (from the MoneyWiz database
//! and from `Txn::text`) like `export`, `ofx` and `ledger`. Raw text goes to
//! exactly two places: the `moneywiz://` links handed to the MoneyWiz app on
//! this Mac, and the owner-only fix list in `out/`. Reports hold account
//! names, counts, dates and sums only.
//!
//! Safety: this module never writes the database. A snapshot copy is opened
//! read only. Raw SQL writes break MoneyWiz's Core Data and iCloud sync, so
//! new transactions go through `moneywiz://expense|income|transfer?...&save=true`
//! and MoneyWiz saves them itself. In-place edits go through the Core Data
//! writer (`moneywiz_edit.rs`, `engine/moneywiz-writer`, decision 0007).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OpenFlags};

use crate::config::{Config, MoneyWiz};
use crate::export::{AccountKind, Balance, Export, Txn};
use crate::inbox::Inbox;
use crate::money::fmt_cents;
use crate::text;

/// Seconds from 1970-01-01 to 2001-01-01 (the Core Data epoch), UTC.
const CORE_DATA_EPOCH: i64 = 978_307_200;
/// The memo every transaction added by cashcast carries.
pub const ADDED_MEMO: &str = "Added by cashcast";
/// Memo prefix of a correction; the MoneyWiz key of the fixed row follows.
pub const FIX_MEMO: &str = "cashcast fix #";
/// Memo of a correction that makes the balance match the bank.
pub const BALANCE_MEMO: &str = "cashcast balance fix";
/// Memo of the correction that sets a stock plan account's cash before its
/// first statement row to 0 (`AccountPlan::start_fix`).
pub const START_MEMO: &str = "cashcast start cash fix";
/// Memo of the correction that makes MoneyWiz move across a month the bank
/// never issued as the statements around it do (`AccountPlan::span_fix`).
pub const SPAN_MEMO: &str = "cashcast statement gap fix";
/// MoneyWiz rows that move shares; in a stock plan account their cash is
/// offset (`AccountPlan::share_moves`).
const SHARE_MOVES: &[&str] = &["InvestmentBuyTransaction", "InvestmentSellTransaction"];
/// Database files on a Mac, newest store layout first (relative to $HOME).
const CANDIDATES: &[&str] = &[
    "Library/Containers/com.moneywiz.personalfinance/Data/Library/Application Support/MoneyWiz_iCloud.sqlite",
    "Library/Containers/com.moneywiz.personalfinance-setapp/Data/Library/Application Support/MoneyWiz_iCloud.sqlite",
    "Library/Containers/com.moneywiz.personalfinance/Data/Documents/.AppData/ipadMoneyWiz.sqlite",
    "Library/Containers/com.moneywiz.personalfinance-setapp/Data/Documents/.AppData/ipadMoneyWiz.sqlite",
    "Library/Containers/com.moneywiz.mac/Data/Documents/.AppData/ipadMoneyWiz.sqlite",
];
/// Transaction entities that belong to one account and that a bank row can match.
const CASH_ENTITIES: &[&str] = &[
    "DepositTransaction",
    "WithdrawTransaction",
    "RefundTransaction",
    "TransferDepositTransaction",
    "TransferWithdrawTransaction",
];

#[derive(Clone, Debug)]
pub struct Account {
    pub pk: i64,
    pub name: String,
    /// Entity name, such as `CashAccount` or `CreditCardAccount`.
    pub kind: String,
    pub currency: String,
    pub opening_cents: i64,
    /// Core Data sync id (`GID`), for in-place edits. Empty when absent.
    pub gid: String,
    /// Archived in MoneyWiz (Preferences, Archived Accounts): MoneyWiz drops
    /// every link sent to it, so the sync cannot add rows there.
    pub archived: bool,
}

impl Account {
    pub fn is_card(&self) -> bool {
        self.kind.contains("CreditCard")
    }
}

#[derive(Clone, Debug)]
pub struct MwTxn {
    pub pk: i64,
    pub account: i64,
    pub entity: String,
    pub date: NaiveDate,
    pub cents: i64,
    /// Payee and description. Private.
    pub text: String,
    pub memo: String,
    pub category: Vec<String>,
    /// The type of the row's category (1 expense, 2 income), when it has one.
    pub category_kind: Option<i64>,
    /// Split over two or more categories, or in another currency than its
    /// account: the writer cannot change its amount in place.
    pub fixed_amount: bool,
    /// The other account of a transfer.
    pub other_account: Option<i64>,
    /// The symbol of the investment holding the row is on, upper case (empty
    /// when unknown). A forex account keeps even its own currency on a
    /// holding, so compare with the account's currency.
    pub holding: Option<String>,
    /// Core Data sync id (`GID`), for in-place edits. Empty when absent.
    pub gid: String,
}

impl MwTxn {
    pub fn is_cash(&self) -> bool {
        CASH_ENTITIES.contains(&self.entity.as_str())
    }
    /// The MoneyWiz row a correction belongs to.
    fn fixes(&self) -> Option<i64> {
        self.memo.strip_prefix(FIX_MEMO)?.trim().parse().ok()
    }
    /// A correction cashcast added: it never matches a bank row.
    pub fn is_correction(&self) -> bool {
        self.fixes().is_some()
            || self.memo.starts_with(BALANCE_MEMO)
            || self.memo.starts_with(START_MEMO)
            || self.memo.starts_with(SPAN_MEMO)
    }
}

#[derive(Clone)]
pub struct Store {
    pub file_name: String,
    /// Which columns were read, for `status` (column names, no values).
    pub layout: String,
    pub accounts: Vec<Account>,
    pub txns: Vec<MwTxn>,
    /// Category paths, such as `["Food", "Groceries"]`.
    pub categories: Vec<Vec<String>>,
    /// MoneyWiz's raw category type by path: 1 expense, 2 income.
    pub category_types: HashMap<Vec<String>, i64>,
    /// Every category's type by path, one entry per category: a path that
    /// two categories share (one income, one expense) lists both.
    pub path_kinds: HashMap<Vec<String>, Vec<i64>>,
    /// Every MoneyWiz link to a category by path, also from rows not in
    /// `txns` (such as scheduled transactions or extra splits).
    pub category_links: HashMap<Vec<String>, usize>,
    /// Investment buys and sells: symbol, shares and price by row pk.
    pub trades: HashMap<i64, Trade>,
    /// Investment holdings: MoneyWiz adds the opening shares to the buy and
    /// sell rows and values the holding at its price per share.
    pub holdings: Vec<Holding>,
}

/// A holding: opening shares (`openningNumberOfShares`) and price per share.
#[derive(Clone, Debug, Default)]
pub struct Holding {
    pub gid: String,
    pub account: i64,
    pub symbol: String,
    pub shares: f64,
    pub price: f64,
}

/// What an investment row holds beyond its amount.
#[derive(Clone, Debug, Default)]
pub struct Trade {
    pub symbol: String,
    pub shares: f64,
    pub price: f64,
}

impl Store {
    pub fn balance(&self, account: i64) -> i64 {
        let opening = self
            .accounts
            .iter()
            .find(|a| a.pk == account)
            .map_or(0, |a| a.opening_cents);
        opening
            + self
                .txns
                .iter()
                .filter(|t| t.account == account)
                .map(|t| t.cents)
                .sum::<i64>()
    }
    /// The balance at the end of `day`: MoneyWiz rows dated later (recorded
    /// before the bank posts them) are not in a bank balance of that day.
    pub fn balance_on(&self, account: i64, day: NaiveDate) -> i64 {
        self.balance(account)
            - self
                .txns
                .iter()
                .filter(|t| t.account == account && t.date > day)
                .map(|t| t.cents)
                .sum::<i64>()
    }
    pub fn account(&self, pk: i64) -> Option<&Account> {
        self.accounts.iter().find(|a| a.pk == pk)
    }
}

/// The engine's spending-category guess for a MoneyWiz row (`text::derive`),
/// so other modules never read the row text.
pub fn guessed_category(t: &MwTxn) -> Option<&'static str> {
    crate::text::derive(&t.text, t.cents).category
}

// --- Finding and reading the database ------------------------------------

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The MoneyWiz database: `[moneywiz] db`, else the newest one on this Mac.
/// Whether `db` is the MoneyWiz app's own database under `home`. Links go to
/// the app, which writes only that file: a sync that read any other file (a
/// test's scratch copy) must not hand its links to the real app.
pub fn app_owns(home: &Path, db: &Path) -> bool {
    CANDIDATES.iter().any(|c| home.join(c) == db)
}

pub fn find_db(cfg: Option<&MoneyWiz>) -> Result<PathBuf, String> {
    if let Some(db) = cfg.and_then(|c| c.db.as_deref()) {
        let path = expand_home(db);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!(
                "the MoneyWiz database in [moneywiz] db does not exist: {}",
                path.display()
            ))
        };
    }
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = CANDIDATES
        .iter()
        .map(|c| Path::new(&home).join(c))
        .filter(|p| p.is_file())
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    found.sort();
    found.pop().map(|f| f.1).ok_or_else(|| {
        "no MoneyWiz database found on this computer. Open MoneyWiz once on this Mac, or set [moneywiz] db = \"PATH\" in cashcast.toml".to_string()
    })
}

/// Copies the database and its WAL files into a private temporary folder,
/// so MoneyWiz can stay open and its files are never touched.
fn snapshot(db: &Path) -> Result<(PathBuf, PathBuf), String> {
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create a temporary folder: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let name = db.file_name().ok_or("the MoneyWiz path has no file name")?;
    let copy = dir.join(name);
    std::fs::copy(db, &copy).map_err(|e| format!("cannot read the MoneyWiz database: {e}"))?;
    for suffix in ["-wal", "-shm"] {
        let mut side = db.as_os_str().to_owned();
        side.push(suffix);
        let mut to = copy.as_os_str().to_owned();
        to.push(suffix);
        if Path::new(&side).is_file() {
            std::fs::copy(&side, &to)
                .map_err(|e| format!("cannot read the MoneyWiz database: {e}"))?;
        }
    }
    Ok((dir, copy))
}

/// A fresh temporary folder name: process id, clock, and a per-process counter
/// (process scope, never reset), because the clock alone repeats within one tick.
fn snapshot_dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "cashcast-moneywiz-{}-{}-{n}",
        std::process::id(),
        nanos()
    ))
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Reads accounts, cash transactions and categories from a MoneyWiz database.
pub fn open(db: &Path) -> Result<Store, String> {
    let (dir, copy) = snapshot(db)?;
    let result = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| format!("cannot open the MoneyWiz database: {e}"))
        .and_then(|conn| read(&conn));
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = result?;
    store.file_name = db
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(store)
}

fn sql_err(e: rusqlite::Error) -> String {
    format!(
        "the MoneyWiz database has an unexpected layout ({e}); this MoneyWiz version is not supported yet"
    )
}

struct Schema {
    columns: Vec<String>,
    /// Entity id to entity name.
    entities: HashMap<i64, String>,
}

impl Schema {
    fn ids(&self, pred: impl Fn(&str) -> bool) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .entities
            .iter()
            .filter(|(_, n)| pred(n))
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids
    }

    /// The first of `preferred` that holds values for these entities, else
    /// the `prefix`+digits column with the most values (column suffixes
    /// change between MoneyWiz versions).
    fn column(
        &self,
        conn: &Connection,
        ids: &[i64],
        preferred: &[&str],
        prefix: &str,
    ) -> Result<Option<String>, String> {
        if ids.is_empty() {
            return Ok(None);
        }
        let list = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let count = |col: &str| -> Result<i64, String> {
            conn.query_row(
                &format!("SELECT COUNT({col}) FROM ZSYNCOBJECT WHERE Z_ENT IN ({list})"),
                [],
                |r| r.get(0),
            )
            .map_err(sql_err)
        };
        for p in preferred {
            if self.columns.iter().any(|c| c == p) && count(p)? > 0 {
                return Ok(Some(p.to_string()));
            }
        }
        let mut best: Option<(i64, String)> = None;
        for c in &self.columns {
            let rest = c.strip_prefix(prefix).unwrap_or("x");
            if !rest.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let n = count(c)?;
            if n > 0 && best.as_ref().is_none_or(|b| n > b.0) {
                best = Some((n, c.clone()));
            }
        }
        Ok(best.map(|b| b.1))
    }
}

fn read(conn: &Connection) -> Result<Store, String> {
    let mut entities = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT Z_ENT, Z_NAME FROM Z_PRIMARYKEY")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map_err(sql_err)?;
    for row in rows {
        let (id, name) = row.map_err(sql_err)?;
        entities.insert(id, name);
    }
    let mut columns = Vec::new();
    let mut stmt = conn
        .prepare("PRAGMA table_info(ZSYNCOBJECT)")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(sql_err)?;
    for row in rows {
        columns.push(row.map_err(sql_err)?);
    }
    if columns.is_empty() {
        return Err("this file is not a MoneyWiz database (no ZSYNCOBJECT table)".into());
    }
    let schema = Schema { columns, entities };

    // Accounts.
    let account_ids = schema.ids(|n| n.ends_with("Account") && n != "Account");
    let name_col = schema
        .column(conn, &account_ids, &["ZNAME"], "ZNAME")?
        .ok_or("no account names found")?;
    let cur_col = schema.column(conn, &account_ids, &["ZCURRENCYNAME"], "ZCURRENCYNAME")?;
    let open_col = schema.column(conn, &account_ids, &["ZOPENINGBALANCE"], "ZOPENINGBALANCE")?;
    let gid_col = |ids: &[i64]| schema.column(conn, ids, &["ZGID"], "ZGID");
    let account_gid = gid_col(&account_ids)?;
    let archived_col = schema.column(conn, &account_ids, &["ZARCHIVED"], "ZARCHIVED")?;
    let mut accounts = Vec::new();
    {
        let sql = format!(
            "SELECT Z_PK, Z_ENT, {name_col}, {}, {}, {}, {} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
            cur_col.as_deref().unwrap_or("NULL"),
            open_col.as_deref().unwrap_or("NULL"),
            account_gid.as_deref().unwrap_or("NULL"),
            archived_col.as_deref().unwrap_or("NULL"),
            join_ids(&account_ids)
        );
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (pk, ent, name, currency, opening, gid, archived) = row.map_err(sql_err)?;
            accounts.push(Account {
                pk,
                name: name.unwrap_or_default().trim().to_string(),
                kind: schema.entities.get(&ent).cloned().unwrap_or_default(),
                currency: currency.unwrap_or_default(),
                opening_cents: to_cents(opening.unwrap_or(0.0)),
                gid: gid.unwrap_or_default(),
                archived: archived.unwrap_or(0) != 0,
            });
        }
    }
    accounts.sort_by(|a, b| a.name.cmp(&b.name));

    // Categories, as full paths.
    let category_ids = schema.ids(|n| n == "Category");
    let mut categories_by_pk: HashMap<i64, (String, Option<i64>)> = HashMap::new();
    let mut type_by_pk: HashMap<i64, i64> = HashMap::new();
    if let Some(cname) = schema.column(conn, &category_ids, &["ZNAME2", "ZNAME"], "ZNAME")? {
        let parent = schema.column(conn, &category_ids, &["ZPARENTCATEGORY"], "ZPARENTCATEGORY")?;
        let kind = schema.column(conn, &category_ids, &["ZTYPE2", "ZTYPE"], "ZTYPE")?;
        let sql = format!(
            "SELECT Z_PK, {cname}, {}, {} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
            parent.as_deref().unwrap_or("NULL"),
            kind.as_deref().unwrap_or("NULL"),
            join_ids(&category_ids)
        );
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (pk, name, parent, kind) = row.map_err(sql_err)?;
            categories_by_pk.insert(pk, (name.unwrap_or_default(), parent));
            if let Some(k) = kind {
                type_by_pk.insert(pk, k);
            }
        }
    }
    let path_of = |pk: i64| -> Vec<String> {
        let mut path = Vec::new();
        let mut cur = Some(pk);
        while let Some(p) = cur {
            let Some((name, parent)) = categories_by_pk.get(&p) else {
                break;
            };
            path.insert(0, name.clone());
            cur = *parent;
            if path.len() > 8 {
                break;
            }
        }
        path
    };

    // Payees.
    let payee_ids = schema.ids(|n| n == "Payee");
    let mut payees: HashMap<i64, String> = HashMap::new();
    if let Some(pname) = schema.column(conn, &payee_ids, &["ZNAME5", "ZNAME"], "ZNAME")? {
        let sql = format!(
            "SELECT Z_PK, {pname} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
            join_ids(&payee_ids)
        );
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (pk, name) = row.map_err(sql_err)?;
            payees.insert(pk, name.unwrap_or_default());
        }
    }

    // Category of each transaction (the largest split wins).
    let mut txn_category: HashMap<i64, (f64, i64)> = HashMap::new();
    let mut splits: HashMap<i64, usize> = HashMap::new();
    let mut links_by_pk: HashMap<i64, usize> = HashMap::new();
    let has_assignments: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='ZCATEGORYASSIGMENT'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map_err(sql_err)?
        > 0;
    if has_assignments {
        let mut stmt = conn
            .prepare("SELECT ZTRANSACTION, ZCATEGORY, ZAMOUNT FROM ZCATEGORYASSIGMENT")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                ))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (t, c, amount) = row.map_err(sql_err)?;
            if let Some(c) = c {
                *links_by_pk.entry(c).or_default() += 1;
            }
            if let (Some(t), Some(c), amount) = (t, c, amount) {
                *splits.entry(t).or_default() += 1;
                let amount = amount.unwrap_or(0.0).abs();
                let e = txn_category.entry(t).or_insert((amount, c));
                if amount > e.0 {
                    *e = (amount, c);
                }
            }
        }
    }

    // Transactions: every entity named *Transaction that has an account.
    let txn_ids = schema
        .ids(|n| n.ends_with("Transaction") && n != "Transaction" && !n.starts_with("Scheduled"));
    let col = |preferred: &[&str], prefix: &str| schema.column(conn, &txn_ids, preferred, prefix);
    let account_col = col(&["ZACCOUNT2", "ZACCOUNT1", "ZACCOUNT"], "ZACCOUNT")?;
    let amount_col = col(&["ZAMOUNT1", "ZAMOUNT"], "ZAMOUNT")?;
    let date_col = col(&["ZDATE1", "ZDATE"], "ZDATE")?;
    let txn_rows: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
                join_ids(&txn_ids)
            ),
            [],
            |r| r.get(0),
        )
        .map_err(sql_err)?;
    if txn_rows > 0 && (account_col.is_none() || amount_col.is_none() || date_col.is_none()) {
        return Err(format!(
            "the MoneyWiz database has {txn_rows} transactions but no account, amount or date column this version of cashcast knows; this MoneyWiz version is not supported yet"
        ));
    }
    let layout = format!(
        "{} account and {} transaction entities; columns: account {}, amount {}, date {}",
        account_ids.len(),
        txn_ids.len(),
        account_col.as_deref().unwrap_or("-"),
        amount_col.as_deref().unwrap_or("-"),
        date_col.as_deref().unwrap_or("-")
    );
    let mut txns = Vec::new();
    if let (Some(account_col), Some(amount_col), Some(date_col)) =
        (account_col, amount_col, date_col)
    {
        let desc = col(&["ZDESC2", "ZDESC1", "ZDESC"], "ZDESC")?;
        let payee = col(&["ZPAYEE2", "ZPAYEE1", "ZPAYEE"], "ZPAYEE")?;
        let notes = col(&["ZNOTES1", "ZNOTES"], "ZNOTES")?;
        let recipient = col(
            &["ZRECIPIENTACCOUNT1", "ZRECIPIENTACCOUNT"],
            "ZRECIPIENTACCOUNT",
        )?;
        let sender = col(&["ZSENDERACCOUNT", "ZSENDERACCOUNT1"], "ZSENDERACCOUNT")?;
        let gid = gid_col(&txn_ids)?;
        let original = col(&["ZORIGINALAMOUNT1", "ZORIGINALAMOUNT"], "ZORIGINALAMOUNT")?;
        let holding = col(
            &["ZINVESTMENTHOLDING", "ZINVESTMENTHOLDING1"],
            "ZINVESTMENTHOLDING",
        )?;
        let holding_symbols = holding_symbols(conn, &schema)?;
        let or_null = |c: &Option<String>| c.clone().unwrap_or_else(|| "NULL".into());
        let sql = format!(
            "SELECT Z_PK, Z_ENT, {account_col}, {amount_col}, {date_col}, {}, {}, {}, {}, {}, {}, {}, {} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
            or_null(&desc),
            or_null(&payee),
            or_null(&notes),
            or_null(&recipient),
            or_null(&sender),
            or_null(&gid),
            or_null(&original),
            or_null(&holding),
            join_ids(&txn_ids)
        );
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, Option<f64>>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                ))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (
                pk,
                ent,
                account,
                amount,
                date,
                desc,
                payee,
                notes,
                recipient,
                sender,
                gid,
                orig,
                held,
            ) = row.map_err(sql_err)?;
            let (Some(account), Some(amount), Some(date)) = (account, amount, date) else {
                continue;
            };
            let Some(date) = core_data_date(date) else {
                continue;
            };
            let payee = payee
                .and_then(|p| payees.get(&p))
                .cloned()
                .unwrap_or_default();
            let desc = desc.unwrap_or_default();
            let text = if desc.trim().is_empty() || desc.trim() == payee.trim() {
                payee
            } else if payee.trim().is_empty() {
                desc
            } else {
                format!("{payee} {desc}")
            };
            let entity = schema.entities.get(&ent).cloned().unwrap_or_default();
            let other = if entity.starts_with("TransferWithdraw") {
                recipient
            } else if entity.starts_with("TransferDeposit") {
                sender
            } else {
                None
            };
            txns.push(MwTxn {
                pk,
                account,
                entity,
                date,
                cents: to_cents(amount),
                text: text.trim().to_string(),
                memo: notes.unwrap_or_default(),
                category: txn_category
                    .get(&pk)
                    .map(|c| path_of(c.1))
                    .unwrap_or_default(),
                fixed_amount: splits.get(&pk).is_some_and(|&n| n > 1)
                    || orig.is_some_and(|o| to_cents(o) != to_cents(amount)),
                category_kind: txn_category
                    .get(&pk)
                    .and_then(|c| type_by_pk.get(&c.1).copied()),
                other_account: other,
                holding: held.map(|h| holding_symbols.get(&h).cloned().unwrap_or_default()),
                gid: gid.unwrap_or_default(),
            });
        }
    }
    txns.sort_by_key(|t| (t.date, t.pk));
    let mut categories: Vec<Vec<String>> = categories_by_pk.keys().map(|&pk| path_of(pk)).collect();
    categories.sort();
    // Two categories may share a path: the lowest key gives the type, so
    // every read agrees.
    let mut typed: Vec<(i64, i64)> = type_by_pk.iter().map(|(&pk, &k)| (pk, k)).collect();
    typed.sort();
    let mut category_types: HashMap<Vec<String>, i64> = HashMap::new();
    let mut path_kinds: HashMap<Vec<String>, Vec<i64>> = HashMap::new();
    for &(pk, k) in &typed {
        path_kinds.entry(path_of(pk)).or_default().push(k);
    }
    for (pk, k) in typed {
        category_types.entry(path_of(pk)).or_insert(k);
    }
    let mut category_links: HashMap<Vec<String>, usize> = HashMap::new();
    for (&pk, &n) in &links_by_pk {
        if categories_by_pk.contains_key(&pk) {
            *category_links.entry(path_of(pk)).or_default() += n;
        }
    }
    // Investment rows: symbol, shares and price.
    let inv_ids = schema.ids(|n| n.starts_with("Investment") && n.ends_with("Transaction"));
    let icol = |p: &[&str], prefix: &str| schema.column(conn, &inv_ids, p, prefix);
    let mut trades = HashMap::new();
    if let (Some(sym), Some(sh), Some(pr)) = (
        icol(&["ZSYMBOL", "ZSYMBOL1"], "ZSYMBOL")?,
        icol(&["ZNUMBEROFSHARES", "ZNUMBEROFSHARES1"], "ZNUMBEROFSHARES")?,
        icol(&["ZPRICEPERSHARE", "ZPRICEPERSHARE1"], "ZPRICEPERSHARE")?,
    ) {
        let sql = format!(
            "SELECT Z_PK, {sym}, {sh}, {pr} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
            join_ids(&inv_ids)
        );
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                ))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (pk, symbol, shares, price) = row.map_err(sql_err)?;
            trades.insert(
                pk,
                Trade {
                    symbol: symbol.unwrap_or_default(),
                    shares: shares.unwrap_or(0.0),
                    price: price.unwrap_or(0.0),
                },
            );
        }
    }
    Ok(Store {
        file_name: String::new(),
        layout,
        accounts,
        txns,
        categories,
        category_types,
        path_kinds,
        category_links,
        trades,
        holdings: holdings(conn, &schema)?,
    })
}

/// Every investment holding.
fn holdings(conn: &Connection, schema: &Schema) -> Result<Vec<Holding>, String> {
    let ids = schema.ids(|n| n == "InvestmentHolding");
    let col = |p: &[&str], prefix: &str| schema.column(conn, &ids, p, prefix);
    let (Some(sym), Some(open), Some(acct), Some(gid)) = (
        col(&["ZSYMBOL", "ZSYMBOL1"], "ZSYMBOL")?,
        col(&["ZOPENNINGNUMBEROFSHARES"], "ZOPENNINGNUMBEROFSHARES")?,
        col(&["ZINVESTMENTACCOUNT"], "ZINVESTMENTACCOUNT")?,
        col(&["ZGID"], "ZGID")?,
    ) else {
        return Ok(Vec::new());
    };
    let price = col(&["ZPRICEPERSHARE", "ZPRICEPERSHARE1"], "ZPRICEPERSHARE")?
        .unwrap_or_else(|| "0".into());
    let sql = format!(
        "SELECT {gid}, {acct}, {sym}, {open}, {price} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
        join_ids(&ids)
    );
    let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Holding {
                gid: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                account: r.get::<_, Option<i64>>(1)?.unwrap_or_default(),
                symbol: r
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_default()
                    .trim()
                    .to_uppercase(),
                shares: r.get::<_, Option<f64>>(3)?.unwrap_or_default(),
                price: r.get::<_, Option<f64>>(4)?.unwrap_or_default(),
            })
        })
        .map_err(sql_err)?;
    rows.collect::<Result<_, _>>().map_err(sql_err)
}

/// Each investment holding's symbol, upper case, by its Z_PK.
fn holding_symbols(conn: &Connection, schema: &Schema) -> Result<HashMap<i64, String>, String> {
    let ids = schema.ids(|n| n == "InvestmentHolding");
    let Some(sym) = schema.column(conn, &ids, &["ZSYMBOL", "ZSYMBOL1"], "ZSYMBOL")? else {
        return Ok(HashMap::new());
    };
    let sql = format!(
        "SELECT Z_PK, {sym} FROM ZSYNCOBJECT WHERE Z_ENT IN ({})",
        join_ids(&ids)
    );
    let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(sql_err)?;
    let mut out = HashMap::new();
    for row in rows {
        let (pk, s) = row.map_err(sql_err)?;
        out.insert(pk, s.unwrap_or_default().trim().to_uppercase());
    }
    Ok(out)
}

fn join_ids(ids: &[i64]) -> String {
    if ids.is_empty() {
        return "-1".into();
    }
    ids.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn to_cents(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// A Core Data timestamp as a date on this computer's clock.
pub fn core_data_date(seconds: f64) -> Option<NaiveDate> {
    let unix = seconds.floor() as i64 + CORE_DATA_EPOCH;
    Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|d| d.date_naive())
}

// --- Matching MoneyWiz accounts with the exports --------------------------

fn norm_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// MoneyWiz account pk to the export account it follows. A cash account
/// follows an export only when `[moneywiz.accounts]` names it: the owner
/// records cash by hand, so a name that happens to match never links it.
pub fn links(
    store: &Store,
    cfg: Option<&MoneyWiz>,
    export_accounts: &[String],
) -> BTreeMap<i64, String> {
    let mut out = BTreeMap::new();
    let mapping = cfg.map(|c| c.accounts.clone()).unwrap_or_default();
    for a in &store.accounts {
        if let Some(target) = mapping.get(&a.name) {
            if !target.is_empty() {
                out.insert(a.pk, target.clone());
            }
            continue;
        }
        if a.kind == "CashAccount" {
            continue;
        }
        if let Some(e) = export_accounts
            .iter()
            .find(|e| norm_name(e) == norm_name(&a.name))
        {
            out.insert(a.pk, e.clone());
        }
    }
    out
}

/// A ledger row that is the cash side of a buy or sell ("trade buy").
fn is_trade_leg(text: &str) -> bool {
    text == "trade" || text.starts_with("trade ")
}

/// Links an investment account that `[moneywiz.investments]` maps, and
/// `[moneywiz.accounts]` does not name, to the cash rows of its broker
/// account: the one export account whose broker ids (`ledger_ids`) the
/// mapping takes, all of them. Several such exports (a CAD and a USD side)
/// narrow to the account's currency, then to the one with trade legs; still
/// several, or none, links nothing.
fn investment_links(
    store: &Store,
    cfg: Option<&MoneyWiz>,
    inbox: &Inbox,
    export_accounts: &[String],
    links: &mut BTreeMap<i64, String>,
) {
    let Some(cfg) = cfg else { return };
    let mut ids: BTreeMap<&str, Vec<(&str, &str, &str)>> = BTreeMap::new();
    for f in &inbox.files {
        for (e, set) in &f.ledger_ids {
            let v = ids.entry(e.as_str()).or_default();
            v.extend(
                set.iter()
                    .map(|(b, i)| (f.format.as_str(), b.as_str(), i.as_str())),
            );
        }
    }
    for a in &store.accounts {
        if links.contains_key(&a.pk) || cfg.accounts.contains_key(&a.name) {
            continue;
        }
        let Some(mapping) = cfg.investments.get(&a.name) else {
            continue;
        };
        let taken: Vec<&String> = links.values().collect();
        let mut found: Vec<&String> = export_accounts
            .iter()
            .filter(|e| !taken.contains(e))
            .filter(|e| {
                ids.get(e.as_str()).is_some_and(|v| {
                    !v.is_empty()
                        && v.iter()
                            .all(|(f, b, i)| crate::moneywiz_invest::spec_takes(mapping, f, b, i))
                })
            })
            .collect();
        if found.len() > 1 && !a.currency.is_empty() {
            found.retain(|e| {
                inbox
                    .txns
                    .iter()
                    .find(|t| &t.account == *e)
                    .is_some_and(|t| t.currency.eq_ignore_ascii_case(&a.currency))
            });
        }
        // Still several (a second account of the type with cash only): the
        // one with trade legs, since the mapped buy and sell rows come from it.
        if found.len() > 1 {
            found.retain(|e| {
                inbox
                    .txns
                    .iter()
                    .any(|t| &t.account == *e && is_trade_leg(&t.text))
            });
        }
        if let [e] = found[..] {
            links.insert(a.pk, e.clone());
        }
    }
}

/// The sync report's "Broker trades" part: per investment account that
/// `[moneywiz.investments]` maps, the broker buys and sells MoneyWiz lacks
/// (counts and symbols only). Also the names of those accounts, which the
/// sync does not leave alone. Empty when nothing is mapped.
fn broker_rows(
    store: &Store,
    cfg: &crate::config::Config,
    home: &Path,
) -> Vec<crate::moneywiz_invest::Row> {
    use crate::moneywiz_invest as inv;
    let Some(m) = cfg.moneywiz.as_ref().filter(|m| !m.investments.is_empty()) else {
        return Vec::new();
    };
    let Ok(h) = crate::ledger::load_dir(&home.join("inbox"), cfg) else {
        return Vec::new();
    };
    let (adjust, _) = inv::holding_adjustments(&h.trades, &m.investments, &m.holdings);
    let mut trades = h.trades;
    trades.extend(adjust);
    let snaps = crate::pdf::sunlife::snapshots(&home.join("inbox"));
    let _ = inv::statement_lots(store, &mut trades, &snaps, &m.investments);
    inv::compare(store, &trades, &m.investments)
}

/// Sets each investment account's `trades`: the cash of the broker trades
/// MoneyWiz lacks, which `edit --fixes` adds after the sync. Only a trade
/// whose cash leg the export holds counts (a Wealthsimple "trade buy" row):
/// that leg is in the bank's balance, but the sync leaves it to the buy row.
/// A plan whose export has no legs (Sun Life) keeps its balance as it is.
pub fn add_pending_trades(
    plan: &mut Plan,
    store: &Store,
    inbox: &Inbox,
    cfg: &crate::config::Config,
    home: &Path,
) {
    let rows = broker_rows(store, cfg, home);
    for ap in &mut plan.accounts {
        let Some(account) = store.account(ap.mw) else {
            continue;
        };
        let mut legs: Vec<(NaiveDate, i64)> = inbox
            .txns
            .iter()
            .filter(|t| t.account == ap.export_account && is_trade_leg(&t.text))
            .map(|t| (t.date, t.cents))
            .collect();
        ap.trades = rows
            .iter()
            .filter(|r| r.account == account.name)
            .flat_map(|r| &r.missing_trades)
            .filter_map(|t| {
                let cash = crate::moneywiz_invest::trade_cash(t, &account.currency);
                let cents = (cash * 100.0).round() as i64;
                if cents == 0 {
                    return None;
                }
                // Settlement can date the leg a few days after the trade.
                let leg = legs
                    .iter()
                    .position(|&(d, c)| c == cents && (d - t.date).num_days().abs() <= 5)?;
                legs.swap_remove(leg);
                Some((t.date, cents))
            })
            .collect();
    }
}

impl AccountPlan {
    /// The cash of the pending broker trades up to a day (all with None).
    pub fn trades_upto(&self, day: Option<NaiveDate>) -> i64 {
        self.trades
            .iter()
            .filter(|(d, _)| day.is_none_or(|day| *d <= day))
            .map(|(_, c)| c)
            .sum()
    }
}

fn broker_trades(store: &Store, cfg: &crate::config::Config, home: &Path) -> (Vec<String>, String) {
    let rows = broker_rows(store, cfg, home);
    if rows.is_empty() {
        return (Vec::new(), String::new());
    }
    let mut names: Vec<String> = rows.iter().map(|r| r.account.clone()).collect();
    names.dedup();
    let mut missing: BTreeMap<&str, (usize, Vec<&str>)> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.missing > 0) {
        let e = missing.entry(&r.account).or_default();
        e.0 += r.missing;
        e.1.push(&r.symbol);
    }
    let mut out = String::from("\n## Broker trades\n\n");
    if missing.is_empty() {
        out.push_str(&format!(
            "The {} investment accounts in `[moneywiz.investments]` hold every buy and sell of their broker files.\n",
            names.len()
        ));
    } else {
        out.push_str("| MoneyWiz account | Trades to add | Symbols |\n|---|---|---|\n");
        for (account, (n, symbols)) in &missing {
            out.push_str(&format!("| {account} | {n} | {} |\n", symbols.join(", ")));
        }
        out.push_str(
            "\n`bin/cashcast moneywiz edit --fixes` adds them (`--dry-run` first). The sync above sends the cash rows only.\n",
        );
    }
    (names, out)
}

/// What one sync would do for one linked account.
#[derive(Default)]
pub struct AccountPlan {
    pub mw: i64,
    pub export_account: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub bank_rows: usize,
    pub matched: usize,
    /// Indexes into the inbox transactions.
    pub add: Vec<usize>,
    /// (MoneyWiz pk, inbox index): same transaction, different amount.
    pub fix: Vec<(i64, usize)>,
    /// MoneyWiz rows in the window with no bank row.
    pub only_in_moneywiz: Vec<i64>,
    /// MoneyWiz rows that a bank row matched, with the same or another amount.
    pub matched_mw: Vec<i64>,
    /// MoneyWiz rows standing in for a month the bank never issued (the
    /// statements around it agree with them): left alone, and covered.
    pub stood_in: Vec<i64>,
    /// In `[moneywiz.trust_bank]`: the bank wins even when the sync moves
    /// MoneyWiz away from the bank's balance.
    pub trust_bank: bool,
    /// (MoneyWiz pk, inbox index) for each one-to-one match: what the bank
    /// calls a row the owner entered (`moneywiz categorize` learns from it).
    pub pairs: Vec<(i64, usize)>,
    /// Bank rows that match more than one MoneyWiz row equally well (or the
    /// other way round). Nothing is added or fixed for them: the owner picks.
    pub conflicts: Vec<Conflict>,
    pub bank_balance: Option<Balance>,
    pub note: Option<String>,
    /// MoneyWiz reconcile rows in the period (not yet offset): the owner's
    /// stand-ins for rows the bank lists. The sync offsets them.
    pub reconcile: Vec<i64>,
    /// MoneyWiz rows in the period with no bank row, offset because the
    /// bank balance proves they stood in for the rows the sync adds.
    pub stand_in: Vec<i64>,
    /// The account's cash comes from stock plan statements, and
    /// `[moneywiz.investments]` maps it: shares arrive by vest or payroll
    /// purchase, and a sale's cash is a statement row. Its corrections stay
    /// rows (`edit --fixes` folds nothing and keeps the opening balance).
    pub stock_plan: bool,
    /// Stock plan: buy and sell rows whose cash is not offset yet. They move
    /// shares only, so each gets an offset (owner, 2026-11-16).
    pub share_moves: Vec<i64>,
    /// Stock plan: the cash before the first statement row, as the
    /// statements show it: the bank balance less the statement rows (0 with
    /// no balance to tell, or when it is below 0 by less than the gate's
    /// room: a balance read on the site may leave out the dividends).
    pub start_cash: i64,
    /// Stock plan: the correction that brings MoneyWiz's cash before the
    /// first statement row to `start_cash` (its opening balance and early
    /// rows stay).
    pub start_fix: i64,
    /// Around a month the bank never issued: (last missing day, amount) of
    /// the correction that makes MoneyWiz move across the span as the
    /// statements do. Its rows stand in and are left alone.
    pub span_fix: Vec<(NaiveDate, i64)>,
    /// Investment account: the cash of the broker trades MoneyWiz lacks
    /// (day, cents; below zero for a buy). `edit --fixes` adds them after
    /// the sync, so the gate and the balance correction count them.
    pub trades: Vec<(NaiveDate, i64)>,
    /// An account the bank gives only statements for: (day, balance)
    /// MoneyWiz must hold on each statement day (`span_fix` gets it there).
    pub statement_days: Vec<(NaiveDate, i64)>,
}

/// Bank rows and MoneyWiz rows that pair up equally well in more than one way.
#[derive(Clone, Debug)]
pub struct Conflict {
    /// Inbox indexes.
    pub rows: Vec<usize>,
    /// MoneyWiz pks.
    pub candidates: Vec<i64>,
}

/// A `[moneywiz.balances]` value, "AMOUNT@YYYY-MM-DD": (cents, day).
pub fn read_balance(value: &str) -> Option<(i64, chrono::NaiveDate)> {
    let (a, d) = value.split_once('@')?;
    let a: f64 = a.trim().replace(',', "").parse().ok()?;
    let d = chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok()?;
    Some(((a * 100.0).round() as i64, d))
}

/// Whether a row comes from a bank or card export, not from MoneyWiz itself.
pub(crate) fn from_bank(inbox: &Inbox, t: &Txn) -> bool {
    inbox
        .files
        .get(t.file)
        .is_none_or(|f| !f.format.starts_with("MoneyWiz"))
}

/// Each export account's newest download day: the latest saved day of a
/// bank file holding its rows (`inbox::saved_day`), duplicates included: a
/// new download that repeats an older file's rows is still a download.
fn export_saved(dir: &Path, inbox: &Inbox, today: NaiveDate) -> HashMap<String, NaiveDate> {
    let mut out: HashMap<String, NaiveDate> = HashMap::new();
    for (f, accounts) in inbox.files.iter().zip(&inbox.file_accounts) {
        if f.format.starts_with("MoneyWiz") {
            continue;
        }
        let Some(day) = crate::inbox::saved_day(&dir.join(&f.file_name), today) else {
            continue;
        };
        for a in accounts {
            let slot = out.entry(a.clone()).or_insert(day);
            *slot = (*slot).max(day);
        }
    }
    out
}

/// The most days an export may age: the `fetch.stale_days` setting, the
/// same limit `fetch plan` uses (7 when it cannot be read).
fn stale_limit() -> i64 {
    match crate::core::settings::get("fetch.stale_days") {
        Ok(crate::core::settings::Value::Int(n)) if n > 0 => n,
        _ => 7,
    }
}

/// The same for a crypto-only service: `fetch.crypto_stale_days` (31).
fn crypto_stale_limit() -> i64 {
    match crate::core::settings::get("fetch.crypto_stale_days") {
        Ok(crate::core::settings::Value::Int(n)) if n > 0 => n,
        _ => 31,
    }
}

/// Export accounts whose every file is a format only crypto-only services
/// read (`fetch::crypto_formats`): they get the crypto age limit.
fn crypto_accounts(inbox: &Inbox, formats: &[String]) -> std::collections::HashSet<String> {
    let mut crypto = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    for (f, accounts) in inbox.files.iter().zip(&inbox.file_accounts) {
        let set = if formats.contains(&f.format) {
            &mut crypto
        } else {
            &mut other
        };
        set.extend(accounts.iter().cloned());
    }
    crypto.retain(|a| !other.contains(a));
    crypto
}

/// An export needs a new download when its newest row and its newest
/// download are both more than `limit` days old. A recent download with old
/// rows is an account with no activity since: current.
/// `closed`: the export has no rows and both the bank and MoneyWiz hold
/// 0.00 (an emptied account read from statements): nothing to download.
fn export_old(
    to: Option<NaiveDate>,
    saved: Option<NaiveDate>,
    today: NaiveDate,
    closed: bool,
    limit: i64,
) -> bool {
    let old = |d: NaiveDate| (today - d).num_days() > limit;
    !closed && to.is_none_or(old) && saved.is_none_or(old)
}

pub struct Plan {
    pub accounts: Vec<AccountPlan>,
    /// MoneyWiz accounts with no export (cash, for example): left alone.
    pub untouched: Vec<i64>,
    /// Export accounts with no MoneyWiz account.
    pub unlinked_exports: Vec<String>,
    /// "Accounts" named after their file: a CSV with no account column (TD,
    /// CIBC). Not real accounts, so not unlinked ones either.
    pub file_named: Vec<String>,
}

fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !w.chars().any(|c| c.is_ascii_digit()))
        .map(|w| w.to_uppercase())
        .collect()
}

fn similar(a: &str, b: &str) -> bool {
    let (wa, wb) = (words(a), words(b));
    !wa.is_empty() && wa.intersection(&wb).next().is_some()
}

/// The best set of 2 to 6 of `cents` whose sum is within $1 and 0.5 % of `want`:
/// the smallest difference first, then the fewest rows.
fn group_sum(cents: &[i64], want: i64) -> Option<Vec<usize>> {
    fn walk(
        cents: &[i64],
        want: i64,
        start: usize,
        pick: &mut Vec<usize>,
        sum: i64,
        best: &mut Option<(i64, usize, Vec<usize>)>,
    ) {
        if pick.len() >= 2 {
            let diff = (sum - want).abs();
            if diff <= 100
                && diff * 200 <= want.abs()
                && best
                    .as_ref()
                    .is_none_or(|b| (diff, pick.len()) < (b.0, b.1))
            {
                *best = Some((diff, pick.len(), pick.clone()));
            }
        }
        if pick.len() == 6 || best.as_ref().is_some_and(|b| b.0 == 0) {
            return;
        }
        for x in start..cents.len() {
            // Same direction for all rows: past the target is never closer.
            if (sum + cents[x]).abs() > want.abs() + 100 {
                continue;
            }
            pick.push(x);
            walk(cents, want, x + 1, pick, sum + cents[x], best);
            pick.pop();
        }
    }
    let mut best = None;
    walk(cents, want, 0, &mut Vec::new(), 0, &mut best);
    best.map(|b| b.2)
}

/// `corrected`, for the engine binary.
pub fn corrected_amounts(store: &Store) -> HashMap<i64, i64> {
    corrected(store)
}

/// The amount of a MoneyWiz row with its cashcast corrections added.
pub(crate) fn corrected(store: &Store) -> HashMap<i64, i64> {
    let mut out: HashMap<i64, i64> = store.txns.iter().map(|t| (t.pk, t.cents)).collect();
    for t in &store.txns {
        if let Some(target) = t.fixes()
            && let Some(c) = out.get_mut(&target)
        {
            *c += t.cents;
        }
    }
    out
}

/// The running engine binary's size and modified time: a rebuilt engine
/// may plan differently, so it must not reuse an older run's report.
fn engine_build() -> String {
    std::env::current_exe().map_or(String::new(), |p| build_stamp(&p))
}

fn build_stamp(exe: &Path) -> String {
    std::fs::metadata(exe)
        .map(|m| format!("{}@{:?}", m.len(), m.modified().ok()))
        .unwrap_or_default()
}

/// Sets the stock plan fields of an account's plan (`AccountPlan::stock_plan`).
/// The start correction applies only when no earlier row of the account was
/// left out (`since`), so the statement rows are the whole history. Runs
/// after the matching: a MoneyWiz row a statement row pairs with (loose
/// matching reaches before the first one) is not start cash.
fn stock_plan(
    store: &Store,
    inbox: &Inbox,
    bank: &[usize],
    invested: bool,
    export_account: &str,
    amounts: &HashMap<i64, i64>,
    ap: &mut AccountPlan,
) {
    let statement = |i: usize| {
        inbox
            .files
            .get(inbox.txns[i].file)
            .is_some_and(|f| f.format == crate::pdf::ETRADE_STOCKPLAN)
    };
    let (Some(from), Some(to)) = (ap.from, ap.to) else {
        return;
    };
    if !invested || !bank.iter().all(|&i| statement(i)) {
        return;
    }
    ap.stock_plan = true;
    // Rows up to the day the balance is checked, so the check sees every offset.
    let last = ap
        .bank_balance
        .as_ref()
        .and_then(|b| b.as_of)
        .map_or(to, |d| d.max(to));
    let rows = || store.txns.iter().filter(|t| t.account == ap.mw);
    ap.share_moves = rows()
        .filter(|t| SHARE_MOVES.contains(&t.entity.as_str()) && t.date <= last)
        .filter(|t| amounts[&t.pk] != 0)
        .map(|t| t.pk)
        .collect();
    let earlier = inbox
        .txns
        .iter()
        .any(|t| t.account == export_account && from_bank(inbox, t) && t.date < from);
    if earlier {
        return;
    }
    let rows_sum: i64 = bank.iter().map(|&i| inbox.txns[i].cents).sum();
    ap.start_cash = match &ap.bank_balance {
        Some(b) if b.as_of.is_none_or(|d| d >= to) => {
            let implied = b.cents - rows_sum;
            let room = (b.cents.abs() / 20).max(10_000);
            if (-room..0).contains(&implied) {
                0
            } else {
                implied
            }
        }
        _ => 0,
    };
    let opening = store.account(ap.mw).map_or(0, |a| a.opening_cents);
    let cash_before: i64 = rows()
        .filter(|t| !SHARE_MOVES.contains(&t.entity.as_str()))
        .filter(|t| {
            if t.memo.starts_with(START_MEMO) {
                return true;
            }
            t.date < from
                && !ap.matched_mw.contains(&t.pk)
                && (t.memo.starts_with(BALANCE_MEMO) || !t.is_correction())
        })
        .map(|t| amounts[&t.pk])
        .sum();
    ap.start_fix = ap.start_cash - (opening + cash_before);
}

pub fn plan(store: &Store, inbox: &Inbox, cfg: Option<&MoneyWiz>) -> Result<Plan, String> {
    let since = match cfg.and_then(|c| c.since.as_deref()) {
        Some(s) => Some(
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| "[moneywiz] since must be \"YYYY-MM-DD\"")?,
        ),
        None => None,
    };
    let days = cfg.and_then(|c| c.match_days).unwrap_or(4).clamp(0, 31);
    let loose = match cfg.and_then(|c| c.matching.as_deref()).unwrap_or("strict") {
        "strict" => false,
        "loose" => true,
        _ => return Err("[moneywiz] match must be \"strict\" or \"loose\"".into()),
    };
    let mut export_accounts: Vec<String> = inbox
        .txns
        .iter()
        .filter(|t| from_bank(inbox, t))
        .map(|t| t.account.clone())
        .collect();
    export_accounts.sort();
    export_accounts.dedup();
    let mut links = links(store, cfg, &export_accounts);
    investment_links(store, cfg, inbox, &export_accounts, &mut links);
    let amounts = corrected(store);
    // Several files may give one account's balance (a statement per
    // period): the newest wins, and a current one (no day) beats any day.
    let mut balances: HashMap<&String, &Balance> = HashMap::new();
    for (k, b) in inbox.files.iter().flat_map(|f| f.balances.iter()) {
        let newer = balances
            .get(k)
            .is_none_or(|old| match (old.as_of, b.as_of) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(o), Some(n)) => n >= o,
            });
        if newer {
            balances.insert(k, b);
        }
    }

    let mut plan = Plan {
        accounts: Vec::new(),
        untouched: store
            .accounts
            .iter()
            .map(|a| a.pk)
            .filter(|pk| !links.contains_key(pk))
            .collect(),
        unlinked_exports: Vec::new(),
        file_named: Vec::new(),
    };
    let stems: HashSet<&str> = inbox
        .files
        .iter()
        .map(|f| {
            f.file_name
                .rsplit_once('.')
                .map_or(f.file_name.as_str(), |(s, _)| s)
        })
        .collect();
    for e in export_accounts
        .iter()
        .filter(|e| !links.values().any(|v| v == *e))
        .filter(|e| !cfg.is_some_and(|c| c.not_tracked.contains_key(e.as_str())))
    {
        if stems.contains(e.as_str()) {
            plan.file_named.push(e.clone());
        } else {
            plan.unlinked_exports.push(e.clone());
        }
    }
    for (&mw, export_account) in &links {
        let account = store.account(mw).expect("linked account exists");
        let mut ap = AccountPlan {
            mw,
            export_account: export_account.clone(),
            trust_bank: cfg.is_some_and(|c| c.trust_bank.contains_key(&account.name)),
            ..Default::default()
        };
        ap.bank_balance = balances
            .get(export_account)
            .map(|b| (*b).clone())
            .or_else(|| {
                // A balance read on the site (`[moneywiz.balances]`) when the
                // export gives none: the gate checks the sync against it.
                let v = cfg?
                    .balances
                    .iter()
                    .find(|(k, _)| k.trim() == account.name)
                    .map(|(_, v)| v)?;
                let (cents, day) = read_balance(v)?;
                Some(Balance {
                    cents,
                    as_of: Some(day),
                    source: "read on the site",
                })
            });
        // A newer download with no balance (Wealthsimple's all-accounts
        // export) can hold rows after that day: carry the balance forward by
        // them, so the gate still checks the sync against the bank.
        if let Some(b) = ap.bank_balance.as_mut()
            && let Some(day) = b.as_of
        {
            let later: Vec<&crate::export::Txn> = inbox
                .txns
                .iter()
                .filter(|t| &t.account == export_account && t.date > day)
                .filter(|t| from_bank(inbox, t))
                .collect();
            if let Some(last) = later.iter().map(|t| t.date).max() {
                b.cents += later.iter().map(|t| t.cents).sum::<i64>();
                b.as_of = Some(last);
            }
        }
        let invested = cfg.is_some_and(|c| c.investments.contains_key(&account.name));
        // Around a month the bank never issued, MoneyWiz's rows stand in.
        let sts = crate::moneywiz_diagnose::statements(inbox, export_account);
        let spans = crate::moneywiz_diagnose::stand_in_spans(&sts, |d| store.balance_on(mw, d));
        let span_fix: Vec<(NaiveDate, i64)> =
            crate::moneywiz_diagnose::stand_ins(&sts, |d| store.balance_on(mw, d))
                .into_iter()
                .map(|s| s.fix)
                .filter(|&(_, c)| c != 0)
                .collect();
        let spared = |d: NaiveDate| spans.iter().any(|&(a, b)| d >= a && d <= b);
        ap.stood_in = store
            .txns
            .iter()
            .filter(|t| t.account == mw && t.is_cash() && !t.is_correction() && spared(t.date))
            .map(|t| t.pk)
            .collect();
        let bank: Vec<usize> = (0..inbox.txns.len())
            .filter(|&i| &inbox.txns[i].account == export_account)
            .filter(|&i| from_bank(inbox, &inbox.txns[i]))
            .filter(|&i| since.is_none_or(|s| inbox.txns[i].date >= s))
            .filter(|&i| !spared(inbox.txns[i].date))
            // An account `[moneywiz.investments]` covers gets its trades as
            // buy and sell rows (`edit --fixes`); a ledger's cash leg of a
            // trade would count them twice.
            .filter(|&i| !(invested && is_trade_leg(&inbox.txns[i].text)))
            // A 0.00 row moves no money, and MoneyWiz does not save one.
            .filter(|&i| inbox.txns[i].cents != 0)
            .collect();
        ap.bank_rows = bank.len();
        // Only a span inside the checked days (`since`) gets its correction.
        ap.span_fix = span_fix
            .into_iter()
            .filter(|&(d, _)| since.is_none_or(|s| d >= s))
            .collect();
        // Statements and never a row (a Smart Investor plan): MoneyWiz
        // holds each statement's balance on its days.
        let no_rows = !inbox
            .txns
            .iter()
            .any(|t| &t.account == export_account && from_bank(inbox, t));
        if no_rows && !sts.is_empty() {
            ap.statement_days = crate::moneywiz_diagnose::statement_targets(&sts, since);
            ap.span_fix = crate::moneywiz_diagnose::statement_fixes(&ap.statement_days, |d| {
                store.balance_on(mw, d)
            });
        }
        let Some(from) = bank.iter().map(|&i| inbox.txns[i].date).min() else {
            ap.note = Some("no bank rows in the period".into());
            plan.accounts.push(ap);
            continue;
        };
        let to = bank
            .iter()
            .map(|&i| inbox.txns[i].date)
            .max()
            .expect("non-empty");
        (ap.from, ap.to) = (Some(from), Some(to));
        let bank_currency = bank
            .iter()
            .map(|&i| inbox.txns[i].currency.as_str())
            .find(|c| !c.is_empty())
            .unwrap_or("");
        if !account.currency.is_empty()
            && !bank_currency.is_empty()
            && !account.currency.eq_ignore_ascii_case(bank_currency)
        {
            ap.note = Some(format!(
                "skipped: MoneyWiz uses {}, the export {bank_currency}",
                account.currency
            ));
            plan.accounts.push(ap);
            continue;
        }
        // Loose: a hand row with the bank's exact amount pairs up to three
        // times `match_days` away (at most 14): the owner types the date late.
        let far = if loose { (days * 3).min(14) } else { days };
        let lo = from - chrono::Duration::days(far);
        let hi = to + chrono::Duration::days(far);
        let mw_rows: Vec<&MwTxn> = store
            .txns
            .iter()
            .filter(|t| {
                t.account == mw
                    && t.is_cash()
                    && !t.is_correction()
                    && t.date >= lo
                    && t.date <= hi
                    && !spared(t.date)
                    // An offset row (corrected to 0) is settled: never paired again.
                    && amounts[&t.pk] != 0
            })
            .collect();

        let mut bank_used = vec![false; bank.len()];
        let mut mw_used = vec![false; mw_rows.len()];
        // Pass 1: same amount, closest date.
        let mut pairs = Vec::new();
        for (bi, &i) in bank.iter().enumerate() {
            let b = &inbox.txns[i];
            for (mi, m) in mw_rows.iter().enumerate() {
                let gap = (b.date - m.date).num_days().abs();
                if gap <= days && amounts[&m.pk] == b.cents {
                    let alike = !similar(&b.text, &m.text) as i64;
                    pairs.push((gap, alike, bi, mi));
                }
            }
        }
        pairs.sort();
        for (_, _, bi, mi) in pairs {
            if !bank_used[bi] && !mw_used[mi] {
                bank_used[bi] = true;
                mw_used[mi] = true;
                ap.matched += 1;
                ap.matched_mw.push(mw_rows[mi].pk);
                ap.pairs.push((mw_rows[mi].pk, bank[bi]));
            }
        }
        // Pass 1b (loose): the same amount further away, closest first.
        if far > days {
            let mut pairs = Vec::new();
            for (bi, &i) in bank.iter().enumerate() {
                if bank_used[bi] {
                    continue;
                }
                let b = &inbox.txns[i];
                for (mi, m) in mw_rows.iter().enumerate() {
                    let gap = (b.date - m.date).num_days().abs();
                    if !mw_used[mi] && gap <= far && amounts[&m.pk] == b.cents {
                        pairs.push((gap, bi, mi));
                    }
                }
            }
            pairs.sort();
            for (_, bi, mi) in pairs {
                if !bank_used[bi] && !mw_used[mi] {
                    bank_used[bi] = true;
                    mw_used[mi] = true;
                    ap.matched += 1;
                    ap.matched_mw.push(mw_rows[mi].pk);
                    ap.pairs.push((mw_rows[mi].pk, bank[bi]));
                }
            }
        }
        // Pass 2: same payee (or same day) and same direction, other amount.
        // Loose: any payee and day in the window, an amount within 25 %.
        let mut pairs = Vec::new();
        for (bi, &i) in bank.iter().enumerate() {
            if bank_used[bi] {
                continue;
            }
            let b = &inbox.txns[i];
            for (mi, m) in mw_rows.iter().enumerate() {
                if mw_used[mi] || (!loose && (m.date < from || m.date > to)) {
                    continue;
                }
                let amount = amounts[&m.pk];
                let gap = (b.date - m.date).num_days().abs();
                let same_sign = (amount < 0) == (b.cents < 0);
                let close = (amount - b.cents).abs() * 4 <= b.cents.abs();
                // Same payee, but more than 3x apart: another transaction.
                let within_3x =
                    amount.abs() * 3 >= b.cents.abs() && b.cents.abs() * 3 >= amount.abs();
                let alike = similar(&b.text, &m.text) && within_3x;
                if gap <= days && same_sign && (alike || (close && (gap == 0 || loose))) {
                    // A close amount a day off beats a far one on the same
                    // day: 0 within 1 %, 1 within 5 %, 2 beyond.
                    let diff = (amount - b.cents).abs();
                    let near = if diff * 100 <= b.cents.abs() {
                        0
                    } else if diff * 20 <= b.cents.abs() {
                        1
                    } else {
                        2
                    };
                    pairs.push((!alike as i64, near, gap, diff, bi, mi));
                }
            }
        }
        pairs.sort();
        // A pair is a guess when another free pair for the same bank row or
        // the same MoneyWiz row is just as good (same payee test, same day
        // gap): fixing the amount of the wrong row would be worse than
        // listing them. The owner picks.
        for x in 0..pairs.len() {
            let (alike, near, gap, diff, bi, mi) = pairs[x];
            if bank_used[bi] || mw_used[mi] {
                continue;
            }
            let ties: Vec<(usize, usize)> = pairs
                .iter()
                .filter(|p| (p.0, p.1, p.2) == (alike, near, gap) && (!loose || p.3 == diff))
                .filter(|p| (p.4 == bi) != (p.5 == mi))
                .filter(|p| !bank_used[p.4] && !mw_used[p.5])
                .map(|p| (p.4, p.5))
                .collect();
            if ties.is_empty() {
                bank_used[bi] = true;
                mw_used[mi] = true;
                ap.fix.push((mw_rows[mi].pk, bank[bi]));
                ap.matched_mw.push(mw_rows[mi].pk);
                ap.pairs.push((mw_rows[mi].pk, bank[bi]));
                continue;
            }
            let mut rows: Vec<usize> = vec![bi];
            let mut candidates: Vec<usize> = vec![mi];
            for (b2, m2) in ties {
                rows.push(b2);
                candidates.push(m2);
            }
            rows.sort();
            rows.dedup();
            candidates.sort();
            candidates.dedup();
            if loose {
                // Loose ties: pair them in date order, but only through pairs
                // that passed the checks above. Rows left over stay free.
                let mut tied: Vec<(usize, usize)> = pairs
                    .iter()
                    .filter(|p| (p.0, p.1, p.2, p.3) == (alike, near, gap, diff))
                    .filter(|p| rows.contains(&p.4) && candidates.contains(&p.5))
                    .map(|p| (p.4, p.5))
                    .collect();
                tied.sort_by_key(|&(b, m)| (inbox.txns[bank[b]].date, mw_rows[m].date));
                for (b, m) in tied {
                    if !bank_used[b] && !mw_used[m] {
                        bank_used[b] = true;
                        mw_used[m] = true;
                        ap.fix.push((mw_rows[m].pk, bank[b]));
                        ap.matched_mw.push(mw_rows[m].pk);
                        ap.pairs.push((mw_rows[m].pk, bank[b]));
                    }
                }
                continue;
            }
            for &b in &rows {
                bank_used[b] = true;
            }
            for &m in &candidates {
                mw_used[m] = true;
            }
            ap.conflicts.push(Conflict {
                rows: rows.iter().map(|&b| bank[b]).collect(),
                candidates: candidates.iter().map(|&m| mw_rows[m].pk).collect(),
            });
        }
        // Pass 3 (loose): one MoneyWiz row the owner typed for several
        // purchases. It matches 2 to 6 free bank rows of the same direction
        // within a week whose sum is within $1 and 0.5 % of it (only rows left after pass 2).
        // When the bank wins, the lump goes and its bank rows are added
        // instead, so each day's balance and every cent match the bank.
        let bank_wins = loose
            && mode(cfg).is_ok_and(|m| m != "list")
            && cfg.and_then(|c| c.bank_wins).unwrap_or(false);
        if loose && !bank_wins {
            let mut order: Vec<usize> = (0..mw_rows.len()).filter(|&mi| !mw_used[mi]).collect();
            order.sort_by_key(|&mi| std::cmp::Reverse(amounts[&mw_rows[mi].pk].abs()));
            for mi in order {
                let m = mw_rows[mi];
                let want = amounts[&m.pk];
                let mut near: Vec<usize> = (0..bank.len())
                    .filter(|&bi| {
                        let t = &inbox.txns[bank[bi]];
                        !bank_used[bi]
                            && (t.cents < 0) == (want < 0)
                            && t.cents.abs() < want.abs()
                            && (t.date - m.date).num_days().abs() <= 7
                    })
                    .collect();
                near.sort_by_key(|&bi| (inbox.txns[bank[bi]].date - m.date).num_days().abs());
                near.truncate(16);
                let cents: Vec<i64> = near.iter().map(|&bi| inbox.txns[bank[bi]].cents).collect();
                if let Some(set) = group_sum(&cents, want) {
                    for x in set {
                        bank_used[near[x]] = true;
                        ap.matched += 1;
                    }
                    mw_used[mi] = true;
                    ap.matched_mw.push(m.pk);
                }
            }
        }
        ap.add = bank
            .iter()
            .enumerate()
            .filter(|(bi, _)| !bank_used[*bi])
            .map(|(_, &i)| i)
            .collect();
        ap.reconcile = store
            .txns
            .iter()
            .filter(|t| {
                t.account == mw
                    && t.entity == "ReconcileTransaction"
                    && t.date >= from
                    && t.date <= to
                    && amounts[&t.pk] != 0
            })
            .map(|t| t.pk)
            .collect();
        ap.only_in_moneywiz = mw_rows
            .iter()
            .enumerate()
            .filter(|(mi, m)| !mw_used[*mi] && m.date >= from && m.date <= to)
            .map(|(_, m)| m.pk)
            .collect();
        stock_plan(
            store,
            inbox,
            &bank,
            invested,
            export_account,
            &amounts,
            &mut ap,
        );
        // The bank balance decides: when offsetting the MoneyWiz rows that no
        // bank row pairs with brings MoneyWiz closer to it, they stood in for
        // the rows the sync adds (the owner's rough or lumped entries).
        // List mode sends no offsets, so it only lists them.
        let offsets = loose && mode(cfg).is_ok_and(|m| m != "list");
        let extra: i64 = ap.only_in_moneywiz.iter().map(|pk| amounts[pk]).sum();
        let (diff, added) = sums(inbox, &ap, &amounts);
        let prove = match ap.bank_balance.as_ref() {
            _ if !offsets || extra == 0 => false,
            Some(b) if b.as_of.is_none_or(|d| d >= to) => {
                let before = store.balance_on(mw, b.as_of.unwrap_or(to));
                let without = (before + diff + added - b.cents).abs();
                let with = (before + diff + added - extra - b.cents).abs();
                // At least 1.00 closer, and within the gate's room.
                with + 100 <= without && with <= (b.cents.abs() / 20).max(10_000)
            }
            Some(_) => false,
            // No balance to prove it: only when the rows to add cover them
            // and the sync then moves MoneyWiz by half as much or less.
            None => {
                extra.abs() <= added.abs()
                    && (extra < 0) == (added < 0)
                    && (diff + added - extra).abs() * 2 <= (diff + added).abs()
            }
        };
        // `bank_wins`: the owner's entries are rough, so every MoneyWiz row
        // with no bank row in the synced period goes, proof or not.
        if prove || bank_wins {
            ap.stand_in = std::mem::take(&mut ap.only_in_moneywiz);
        }
        plan.accounts.push(ap);
    }
    Ok(plan)
}

// --- Links for the MoneyWiz app -------------------------------------------

/// Percent-encodes a URL query value.
pub fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn amount_text(cents: i64) -> String {
    format!(
        "{}.{:02}",
        cents.unsigned_abs() / 100,
        cents.unsigned_abs() % 100
    )
}

/// MoneyWiz names accounts in links without spaces.
fn link_account(name: &str) -> String {
    name.split_whitespace().collect()
}

fn clean_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// A MoneyWiz category for an inbox row: its own category or the engine's
/// guess, when MoneyWiz has a category of that name.
fn category_for(store: &Store, t: &Txn) -> Option<String> {
    let wanted: Vec<String> = t.category.iter().map(|c| c.to_lowercase()).collect();
    if wanted.is_empty() {
        return None;
    }
    let full = store
        .categories
        .iter()
        .find(|c| c.iter().map(|x| x.to_lowercase()).collect::<Vec<_>>() == wanted);
    let leaf = || {
        store
            .categories
            .iter()
            .find(|c| c.last().map(|x| x.to_lowercase()) == wanted.last().cloned())
    };
    full.or_else(leaf).map(|c| c.join("/"))
}

#[derive(Clone)]
pub struct Link {
    pub url: String,
    /// Inbox rows this link adds (two for a transfer).
    pub rows: Vec<usize>,
    pub kind: &'static str,
    /// What the link stands for, stable between runs: the bank row(s) it
    /// adds, or the MoneyWiz row and amount it corrects. Private: only its
    /// keyed hash is stored (`SentList`).
    pub id: String,
}

/// The identity of one bank row: its export account, date, amount and text.
fn row_id(t: &Txn) -> String {
    format!(
        "{}|{}|{}|{}",
        t.account,
        t.date,
        t.cents,
        clean_text(&t.text)
    )
}

fn transaction_link(
    account: &Account,
    t: &Txn,
    cents: i64,
    memo: &str,
    category: Option<String>,
) -> String {
    dated_link(account, t.date, &t.text, cents, memo, category)
}

/// A MoneyWiz link that adds one row of `cents` on `day`.
fn dated_link(
    account: &Account,
    day: NaiveDate,
    text: &str,
    cents: i64,
    memo: &str,
    category: Option<String>,
) -> String {
    let kind = if cents < 0 { "expense" } else { "income" };
    let text = clean_text(text);
    let mut q = vec![
        ("account", link_account(&account.name)),
        ("amount", amount_text(cents)),
        ("date", format!("{} 12:00:00", day.format("%Y-%m-%d"))),
        ("payee", text.clone()),
        ("description", text),
        ("memo", memo.to_string()),
    ];
    if !account.currency.is_empty() {
        q.push(("currency", account.currency.clone()));
    }
    if let Some(c) = category {
        q.push(("category", c));
    }
    q.push(("save", "true".into()));
    format!("moneywiz://{kind}?{}", query(&q))
}

fn query(q: &[(&str, String)]) -> String {
    q.iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The links that add the missing rows. A pair of rows that leave one
/// linked account and reach another one becomes one transfer.
pub fn add_links(store: &Store, inbox: &Inbox, plan: &Plan) -> Vec<Link> {
    let learned = crate::moneywiz_categorize::learn(store, inbox, plan);
    let category =
        |t: &Txn| category_for(store, t).or_else(|| learned.for_bank(t).map(|p| p.join("/")));
    let mut rows: Vec<(i64, usize)> = plan
        .accounts
        .iter()
        .flat_map(|a| a.add.iter().map(move |&i| (a.mw, i)))
        .collect();
    rows.sort_by_key(|(_, i)| (inbox.txns[*i].date, *i));
    let mut used = vec![false; rows.len()];
    let mut links = Vec::new();
    for x in 0..rows.len() {
        if used[x] {
            continue;
        }
        let (mw, i) = rows[x];
        let t = &inbox.txns[i];
        let account = store.account(mw).expect("linked account exists");
        let partner = (0..rows.len()).find(|&y| {
            let (mw2, j) = rows[y];
            let u = &inbox.txns[j];
            !used[y]
                && y != x
                && mw2 != mw
                && t.cents != 0
                && u.cents == -t.cents
                && (u.date - t.date).num_days().abs() <= 3
        });
        if let Some(y) = partner {
            used[x] = true;
            used[y] = true;
            let (out_row, in_row) = if t.cents < 0 {
                (rows[x], rows[y])
            } else {
                (rows[y], rows[x])
            };
            let from = store.account(out_row.0).expect("linked account exists");
            let to = store.account(in_row.0).expect("linked account exists");
            let o = &inbox.txns[out_row.1];
            let q = vec![
                ("account", link_account(&from.name)),
                ("toAccount", link_account(&to.name)),
                ("amount", amount_text(o.cents)),
                ("date", format!("{} 12:00:00", o.date.format("%Y-%m-%d"))),
                ("description", clean_text(&o.text)),
                ("memo", ADDED_MEMO.to_string()),
                ("save", "true".into()),
            ];
            links.push(Link {
                url: format!("moneywiz://transfer?{}", query(&q)),
                rows: vec![out_row.1, in_row.1],
                kind: "transfer",
                id: format!("add|{}+{}", row_id(o), row_id(&inbox.txns[in_row.1])),
            });
            continue;
        }
        used[x] = true;
        links.push(Link {
            url: transaction_link(account, t, t.cents, ADDED_MEMO, category(t)),
            rows: vec![i],
            kind: if t.cents < 0 { "expense" } else { "income" },
            id: format!("add|{}", row_id(t)),
        });
    }
    links
}

/// Links that add a correction for each fix: the difference, dated like
/// the MoneyWiz row, in its category.
pub fn fix_links(store: &Store, inbox: &Inbox, plan: &Plan) -> Vec<Link> {
    fix_links_where(store, inbox, plan, |_, _| true)
}

/// Whether the Core Data writer can change this row's amount in place: a
/// plain income, expense or refund with a sync id and no earlier correction.
/// Transfers and corrected rows get a correction row instead, in every mode.
pub fn can_edit_in_place(t: &MwTxn, corrected_cents: i64) -> bool {
    !t.gid.is_empty()
        && !t.fixed_amount
        && matches!(
            t.entity.as_str(),
            "WithdrawTransaction" | "DepositTransaction" | "RefundTransaction"
        )
        && corrected_cents == t.cents
}

fn fix_links_where(
    store: &Store,
    inbox: &Inbox,
    plan: &Plan,
    keep: impl Fn(&MwTxn, i64) -> bool,
) -> Vec<Link> {
    let amounts = corrected(store);
    let mut links = Vec::new();
    for ap in &plan.accounts {
        let account = store.account(ap.mw).expect("linked account exists");
        for &(pk, i) in &ap.fix {
            let m = store
                .txns
                .iter()
                .find(|t| t.pk == pk)
                .expect("fix row exists");
            if !keep(m, amounts[&pk]) {
                continue;
            }
            let diff = inbox.txns[i].cents - amounts[&pk];
            let mut row = inbox.txns[i].clone();
            row.date = m.date;
            row.text = format!(
                "Correction: {}",
                if m.text.is_empty() {
                    &row.text
                } else {
                    &m.text
                }
            );
            let category = (!m.category.is_empty()).then(|| m.category.join("/"));
            links.push(Link {
                url: transaction_link(account, &row, diff, &format!("{FIX_MEMO}{pk}"), category),
                rows: vec![i],
                kind: "correction",
                id: format!("fix|{pk}|{diff}"),
            });
        }
    }
    links
}

/// In "adjust" mode: when the balance still differs from the bank after the
/// adds and corrections, and no MoneyWiz row lacks a bank row (which would
/// explain it), one correction the day before the period makes it match.
/// This is how a wrong opening balance gets fixed.
pub fn balance_links(store: &Store, inbox: &Inbox, plan: &Plan) -> Vec<Link> {
    let amounts = corrected(store);
    plan.accounts
        .iter()
        // A stock plan account's cash is set by its start correction.
        .filter(|ap| ap.note.is_none() && ap.only_in_moneywiz.is_empty() && !ap.stock_plan)
        .filter_map(|ap| balance_link(store, inbox, ap, &amounts))
        .collect()
}

fn balance_link(
    store: &Store,
    inbox: &Inbox,
    ap: &AccountPlan,
    amounts: &HashMap<i64, i64>,
) -> Option<Link> {
    let (from, to, bank) = (ap.from?, ap.to?, ap.bank_balance.as_ref()?);
    let shape = inbox
        .txns
        .iter()
        .position(|t| t.account == ap.export_account)?;
    let day = bank.as_of.unwrap_or(to);
    // A balance older than the last row compares on its own day, with the
    // changes dated up to it.
    let (diff, added) = if day < to {
        sums_upto(store, inbox, ap, amounts, day)
    } else {
        sums(inbox, ap, amounts)
    };
    if day < from {
        return None;
    }
    let gap =
        bank.cents - (store.balance_on(ap.mw, day) + added + diff + ap.trades_upto(Some(day)));
    if gap == 0 {
        return None;
    }
    let mut row = inbox.txns[shape].clone();
    row.date = from - chrono::Duration::days(1);
    row.text = "Balance correction".into();
    let account = store.account(ap.mw)?;
    Some(Link {
        url: transaction_link(account, &row, gap, BALANCE_MEMO, None),
        rows: Vec::new(),
        kind: "balance",
        id: format!("balance|{}|{gap}", ap.export_account),
    })
}

/// Every link one sync sends, in order.
/// A correction that brings each reconcile row of a synced period to zero.
pub fn reconcile_links(store: &Store, inbox: &Inbox, plan: &Plan) -> Vec<Link> {
    let amounts = corrected(store);
    let mut links = Vec::new();
    for ap in &plan.accounts {
        // A statement-only account has no row to copy: its links need none.
        if let Some(account) = store.account(ap.mw) {
            for &(day, cents) in &ap.span_fix {
                let text = if ap.statement_days.is_empty() {
                    "Statement gap correction"
                } else {
                    "Statement balance correction"
                };
                links.push(Link {
                    url: dated_link(account, day, text, cents, SPAN_MEMO, None),
                    rows: Vec::new(),
                    kind: "span",
                    id: format!("span|{}|{day}|{cents}", ap.export_account),
                });
            }
        }
        let Some(shape) = inbox
            .txns
            .iter()
            .position(|t| t.account == ap.export_account)
        else {
            continue;
        };
        let account = store.account(ap.mw).expect("linked account exists");
        let stand = ap.stand_in.iter().map(|&pk| (pk, "No bank row: offset"));
        for (pk, text) in ap
            .reconcile
            .iter()
            .map(|&pk| (pk, "Reconcile offset"))
            .chain(stand)
        {
            let m = store.txns.iter().find(|t| t.pk == pk).expect("row exists");
            let mut row = inbox.txns[shape].clone();
            row.date = m.date;
            row.text = text.into();
            let diff = -amounts[&pk];
            links.push(Link {
                url: transaction_link(account, &row, diff, &format!("{FIX_MEMO}{pk}"), None),
                rows: Vec::new(),
                kind: "reconcile",
                id: format!("reconcile|{}|{pk}|{diff}", ap.export_account),
            });
        }
        links.extend(stock_plan_links(
            store,
            &inbox.txns[shape],
            ap,
            account,
            &amounts,
        ));
    }
    links
}

/// A stock plan account's offsets: one beside each buy and sell row, and
/// the start cash correction on the first statement day.
fn stock_plan_links(
    store: &Store,
    shape: &Txn,
    ap: &AccountPlan,
    account: &Account,
    amounts: &HashMap<i64, i64>,
) -> Vec<Link> {
    let mut links = Vec::new();
    for &pk in &ap.share_moves {
        let m = store.txns.iter().find(|t| t.pk == pk).expect("row exists");
        let mut row = shape.clone();
        row.date = m.date;
        row.text = "Share move: offset".into();
        let diff = -amounts[&pk];
        links.push(Link {
            url: transaction_link(account, &row, diff, &format!("{FIX_MEMO}{pk}"), None),
            rows: Vec::new(),
            kind: "share",
            id: format!("share|{}|{pk}|{diff}", ap.export_account),
        });
    }
    if let (Some(from), true) = (ap.from, ap.start_fix != 0) {
        let mut row = shape.clone();
        row.date = from;
        row.text = "Start cash correction".into();
        links.push(Link {
            url: transaction_link(account, &row, ap.start_fix, START_MEMO, None),
            rows: Vec::new(),
            kind: "start",
            id: format!("start|{}|{}", ap.export_account, ap.start_fix),
        });
    }
    links
}

pub fn links_for(store: &Store, inbox: &Inbox, plan: &Plan, mode: &str) -> Vec<Link> {
    let mut links = add_links(store, inbox, plan);
    if mode != "list" {
        links.extend(reconcile_links(store, inbox, plan));
    }
    if mode == "adjust" {
        links.extend(fix_links(store, inbox, plan));
        links.extend(balance_links(store, inbox, plan));
    } else if mode == "edit" {
        // The writer edits the rest in place; these it cannot touch.
        links.extend(fix_links_where(store, inbox, plan, |t, c| {
            !can_edit_in_place(t, c)
        }));
    }
    links
}

// --- Sending links ---------------------------------------------------------

/// Hands one link to MoneyWiz. `CASHCAST_MONEYWIZ_OPEN` names another
/// opener (tests use a fake app).
pub fn send(url: &str) -> Result<(), String> {
    let status = match std::env::var("CASHCAST_MONEYWIZ_OPEN") {
        Ok(program) if !program.is_empty() => Command::new(program).arg(url).status(),
        _ => Command::new("open").args(["-g", url]).status(),
    }
    .map_err(|e| format!("cannot hand the transaction to MoneyWiz: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("MoneyWiz did not accept a transaction link. Is MoneyWiz installed on this Mac?".into())
    }
}

/// The key of a link in a sent list written before version 2 (an unkeyed
/// `DefaultHasher` hash of the URL, kept for one day). Read only, so a sync
/// on the day of the upgrade does not send those links again.
fn legacy_key(url: &str) -> String {
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The private list of every link sent to MoneyWiz, in `data/`: a keyed
/// hash of each link's identity (never the row itself), the day it was sent,
/// and whether MoneyWiz was seen holding it. Entries never expire.
///
/// - Sent today: waiting for MoneyWiz to save it; not sent again.
/// - Sent earlier, never seen in MoneyWiz: MoneyWiz did not take it; sent again.
/// - Sent earlier, seen in MoneyWiz, missing now: the owner deleted or
///   changed it there. Listed for the owner, not sent again unless asked.
pub struct SentList {
    key: Option<Vec<u8>>,
    pub sent: BTreeMap<String, Sent>,
    legacy_today: HashSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sent {
    pub on: NaiveDate,
    /// MoneyWiz was seen holding it after it was sent.
    pub saved: bool,
}

/// What to do with one link, from the sent list.
#[derive(Debug, PartialEq, Eq)]
pub enum SentState {
    New,
    Waiting,
    NotSaved,
    Gone,
}

pub const SENT_FILE: &str = "moneywiz-sent.json";
pub const KEY_FILE: &str = "moneywiz-key";

fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..32)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

impl SentList {
    /// Reads the list. With `create`, makes the random key on first use.
    pub fn load(data: &Path, today: NaiveDate, create: bool) -> Result<SentList, String> {
        let key_path = data.join(KEY_FILE);
        let key = match std::fs::read_to_string(&key_path) {
            Ok(text) => Some(parse_hex(&text).ok_or_else(|| {
                format!(
                    "{} is damaged. Move it away to start a new sent list (rows sent before are then not known).",
                    key_path.display()
                )
            })?),
            Err(_) if create => {
                let mut bytes = [0u8; 32];
                use std::io::Read;
                std::fs::File::open("/dev/urandom")
                    .and_then(|mut f| f.read_exact(&mut bytes))
                    .map_err(|e| format!("cannot make a random key: {e}"))?;
                std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
                crate::write_private(&key_path, &crate::digest::hex(&bytes))?;
                Some(bytes.to_vec())
            }
            Err(_) => None,
        };
        let text = std::fs::read_to_string(data.join(SENT_FILE)).unwrap_or_default();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let date = |v: &serde_json::Value| NaiveDate::parse_from_str(v.as_str()?, "%Y-%m-%d").ok();
        let object = |v: Option<&serde_json::Value>| {
            v.and_then(|v| v.as_object()).cloned().unwrap_or_default()
        };
        let (sent, legacy_today) = if value.get("version").and_then(|v| v.as_i64()) == Some(2) {
            let sent = object(value.get("sent"))
                .iter()
                .filter_map(|(k, e)| {
                    let on = date(e.get("on")?)?;
                    let saved = e.get("saved").and_then(|v| v.as_bool()).unwrap_or(false);
                    Some((k.clone(), Sent { on, saved }))
                })
                .collect();
            (sent, HashSet::new())
        } else {
            let legacy = object(Some(&value))
                .iter()
                .filter(|(_, d)| date(d).is_some_and(|d| (today - d).num_days() < 1))
                .map(|(k, _)| k.clone())
                .collect();
            (BTreeMap::new(), legacy)
        };
        Ok(SentList {
            key,
            sent: if key_path.exists() {
                sent
            } else {
                BTreeMap::new()
            },
            legacy_today,
        })
    }

    /// The keyed hash of any text, under this user's random key.
    pub fn keyed(&self, text: &str) -> Option<String> {
        let key = self.key.as_ref()?;
        Some(crate::digest::hex(&crate::digest::hmac_sha256(
            key,
            text.as_bytes(),
        )))
    }

    pub fn key_of(&self, link: &Link) -> Option<String> {
        let key = self.key.as_ref()?;
        Some(crate::digest::hex(&crate::digest::hmac_sha256(
            key,
            link.id.as_bytes(),
        )))
    }

    /// What to do with a link MoneyWiz does not hold now.
    pub fn state(&self, link: &Link, today: NaiveDate) -> SentState {
        if self.legacy_today.contains(&legacy_key(&link.url)) {
            return SentState::Waiting;
        }
        match self.key_of(link).and_then(|k| self.sent.get(&k).copied()) {
            None => SentState::New,
            Some(s) if s.on >= today => SentState::Waiting,
            Some(s) if s.saved => SentState::Gone,
            Some(_) => SentState::NotSaved,
        }
    }

    pub fn record(&mut self, link: &Link, today: NaiveDate) {
        if let Some(k) = self.key_of(link) {
            self.sent.insert(
                k,
                Sent {
                    on: today,
                    saved: false,
                },
            );
        }
    }

    /// Marks as saved every sent link that MoneyWiz holds now: the ones that
    /// are not among the links still missing.
    pub fn mark_saved(&mut self, missing: &[Link]) {
        let missing: HashSet<String> = missing.iter().filter_map(|l| self.key_of(l)).collect();
        for (k, s) in self.sent.iter_mut() {
            if !missing.contains(k) {
                s.saved = true;
            }
        }
    }

    /// Written to a temporary file first, so a crash never leaves half a list.
    pub fn write(&self, data: &Path) -> Result<(), String> {
        let sent: BTreeMap<&String, serde_json::Value> = self
            .sent
            .iter()
            .map(|(k, s)| {
                (
                    k,
                    serde_json::json!({"on": s.on.to_string(), "saved": s.saved}),
                )
            })
            .collect();
        let text = serde_json::to_string_pretty(&serde_json::json!({"version": 2, "sent": sent}))
            .map_err(|e| e.to_string())?;
        let path = data.join(SENT_FILE);
        let tmp = data.join(format!("{SENT_FILE}.tmp"));
        crate::write_private(&tmp, &text)?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

/// One sync at a time per user: a second one would send the same links.
struct SyncLock(PathBuf);

impl SyncLock {
    fn take(data: &Path) -> Result<SyncLock, String> {
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        let path = data.join("moneywiz-sync.lock");
        // Stale when the process that took it is gone (a killed run), when
        // it is older than 3 hours (its process id may now be another
        // program's), or, with no process id, after 15 minutes.
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        let older = |secs| age.is_some_and(|a| a > Duration::from_secs(secs));
        let stale = match std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
        {
            Some(pid) => process_gone(pid) || older(3 * 3600),
            None => older(15 * 60),
        };
        if stale {
            let _ = std::fs::remove_file(&path);
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| {
                "another MoneyWiz sync is running. Wait for it to finish, then run the command again.".to_string()
            })?;
        use std::io::Write as _;
        let _ = write!(file, "{}", std::process::id());
        Ok(SyncLock(path))
    }
}

/// True only when `kill -0` says there is no such process. Any other
/// failure (no permission, a sandbox) counts as running: a live lock must
/// never be taken over.
fn process_gone(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|o| {
            !o.status.success()
                && String::from_utf8_lossy(&o.stderr)
                    .to_lowercase()
                    .contains("no such process")
        })
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        // Only our own lock: another run may have taken over a stale one.
        let ours = std::fs::read_to_string(&self.0)
            .is_ok_and(|t| t.trim() == std::process::id().to_string());
        if ours {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

// --- Reports ---------------------------------------------------------------

pub fn mode(cfg: Option<&MoneyWiz>) -> Result<&str, String> {
    match cfg.and_then(|c| c.fixes.as_deref()).unwrap_or("adjust") {
        m @ ("list" | "adjust" | "edit") => Ok(m),
        _ => Err("[moneywiz] fixes must be \"list\", \"adjust\" or \"edit\"".into()),
    }
}

/// `moneywiz status`: the MoneyWiz accounts and the exports they follow.
pub fn render_status(store: &Store, inbox: Option<&Inbox>, cfg: Option<&MoneyWiz>) -> String {
    let mut accounts: Vec<String> = inbox
        .map(|i| i.txns.iter().map(|t| t.account.clone()).collect())
        .unwrap_or_default();
    accounts.sort();
    accounts.dedup();
    let links = links(store, cfg, &accounts);
    let mut out = format!(
        "# MoneyWiz\n\nDatabase: {} (read from a copy; never written)\nLayout: {}\n\n",
        store.file_name, store.layout
    );
    out.push_str("| Account | Type | Currency | Transactions | Last | Balance | Follows export |\n|---|---|---|---|---|---|---|\n");
    for a in &store.accounts {
        let rows: Vec<&MwTxn> = store.txns.iter().filter(|t| t.account == a.pk).collect();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            a.name,
            a.kind.trim_end_matches("Account"),
            a.currency,
            rows.len(),
            rows.last().map_or("-".into(), |t| t.date.to_string()),
            fmt_cents(store.balance(a.pk)),
            links
                .get(&a.pk)
                .map_or("none: recorded by hand".to_string(), |e| e.clone()),
        ));
    }
    let unlinked: Vec<&String> = accounts
        .iter()
        .filter(|e| !links.values().any(|v| v == *e))
        .collect();
    if !unlinked.is_empty() {
        out.push_str("\nExport accounts with no MoneyWiz account (name one in [moneywiz.accounts] in cashcast.toml):\n");
        for e in unlinked {
            out.push_str(&format!("- {e}\n"));
        }
    }
    out
}

/// `sums` up to a day: the corrections and adds dated on or before it (a
/// bank balance older than the last row compares on its own day).
pub(crate) fn sums_upto(
    store: &Store,
    inbox: &Inbox,
    ap: &AccountPlan,
    amounts: &HashMap<i64, i64>,
    day: NaiveDate,
) -> (i64, i64) {
    let mw_by = |pks: &[i64]| -> i64 {
        store
            .txns
            .iter()
            .filter(|t| pks.contains(&t.pk) && t.date <= day)
            .map(|t| amounts[&t.pk])
            .sum()
    };
    let diff = ap
        .fix
        .iter()
        .filter(|&&(_, i)| inbox.txns[i].date <= day)
        .map(|&(pk, i)| inbox.txns[i].cents - amounts[&pk])
        .sum::<i64>()
        - mw_by(&ap.reconcile)
        - mw_by(&ap.stand_in)
        - mw_by(&ap.share_moves)
        + if ap.from.is_some_and(|f| f <= day) {
            ap.start_fix
        } else {
            0
        }
        + ap.span_fix
            .iter()
            .filter(|&&(d, _)| d <= day)
            .map(|&(_, c)| c)
            .sum::<i64>();
    let added = ap
        .add
        .iter()
        .filter(|&&i| inbox.txns[i].date <= day)
        .map(|&i| inbox.txns[i].cents)
        .sum();
    (diff, added)
}

/// (amount difference to fix, amount to add) of one account's plan.
/// (corrections, adds) of an account's plan. Corrections include the offset
/// of its reconcile rows.
pub(crate) fn sums(inbox: &Inbox, ap: &AccountPlan, amounts: &HashMap<i64, i64>) -> (i64, i64) {
    let diff = ap
        .fix
        .iter()
        .map(|&(pk, i)| inbox.txns[i].cents - amounts[&pk])
        .sum::<i64>()
        - ap.reconcile.iter().map(|pk| amounts[pk]).sum::<i64>()
        - ap.stand_in.iter().map(|pk| amounts[pk]).sum::<i64>()
        - ap.share_moves.iter().map(|pk| amounts[pk]).sum::<i64>()
        + ap.start_fix
        + ap.span_fix.iter().map(|&(_, c)| c).sum::<i64>();
    (diff, ap.add.iter().map(|&i| inbox.txns[i].cents).sum())
}

/// The "Balance check" cell: MoneyWiz's balance after the sync beside the bank's.
pub(crate) fn balance_check(
    store: &Store,
    ap: &AccountPlan,
    mode: &str,
    diff: i64,
    added: i64,
) -> String {
    // Broker trades `edit --fixes` adds after the sync move the cash too.
    let change = added + if mode == "list" { 0 } else { diff } + ap.trades_upto(None);
    let after = store.balance(ap.mw) + change;
    match (&ap.note, &ap.bank_balance) {
        (Some(n), _) => n.clone(),
        // A balance older than the last row still compares on its own day
        // once the sync has nothing left to change.
        (None, Some(b)) if change == 0 || b.as_of.is_none_or(|d| Some(d) >= ap.to) => {
            // Compared on the bank's day, like the balance correction.
            let after = match b.as_of.or(ap.to) {
                Some(day) => {
                    store.balance_on(ap.mw, day) + change - ap.trades_upto(None)
                        + ap.trades_upto(Some(day))
                }
                None => after,
            };
            if after == b.cents {
                format!("matches the bank ({})", fmt_cents(b.cents))
            } else {
                format!(
                    "MoneyWiz {} vs bank {} after sync",
                    fmt_cents(after),
                    fmt_cents(b.cents)
                )
            }
        }
        _ => format!("MoneyWiz {} after sync", fmt_cents(after)),
    }
}

pub struct SyncResult {
    pub report: String,
    /// Owner-only list of fixes and rows to check, with payees.
    pub fixes_md: String,
    pub sent: usize,
}

/// `moneywiz sync`: the plan, and with `apply` the links sent to MoneyWiz.
pub fn sync(
    db: &Path,
    inbox: &Inbox,
    cfg: &Config,
    home: &Path,
    today: NaiveDate,
    apply: bool,
    resend: bool,
) -> Result<SyncResult, String> {
    let mw = cfg.moneywiz.as_ref();
    let mode = mode(mw)?;
    let data = home.join("data");
    let mut ledger = crate::moneywiz_ledger::Ledger::load(&data);
    let settings = mw
        .map(|m| {
            format!(
                "{:?}|{:?}|{:?}|{:?}|{:?}|{mode}",
                m.accounts, m.since, m.match_days, m.matching, m.db
            )
        })
        .unwrap_or_default();
    // Any config change (export options too), a new day (stale-export
    // warnings, link states) and a new engine build must miss the cache.
    let settings = format!(
        "{settings}|{today}|{}|{}|{}|{}",
        std::fs::read_to_string(home.join("cashcast.toml")).unwrap_or_default(),
        engine_build(),
        stale_limit(),
        crypto_stale_limit()
    );
    let inputs = crate::moneywiz_ledger::inputs_hash(&home.join("inbox"), db, &settings);
    let cached = std::fs::read_to_string(data.join(crate::moneywiz_ledger::REPORT_FILE)).ok();
    if !apply && !resend && ledger.inputs == inputs && ledger.recheck.is_empty() {
        if let Some(report) = cached {
            let at = ledger
                .runs
                .last()
                .and_then(|r| r["at"].as_str())
                .unwrap_or("the last run")
                .to_string();
            return Ok(SyncResult {
                report: format!(
                    "{report}\nCached: the exports, MoneyWiz and the settings are unchanged since {at}. `bin/cashcast moneywiz resync` checks rows again.\n"
                ),
                fixes_md: String::new(),
                sent: 0,
            });
        }
    }
    let store = open(db)?;
    let mut plan = plan(&store, inbox, mw)?;
    add_pending_trades(&mut plan, &store, inbox, cfg, home);
    let links = links_for(&store, inbox, &plan, mode);
    let amounts = corrected(&store);
    let held: HashMap<i64, String> = plan
        .accounts
        .iter()
        .filter_map(|ap| {
            crate::moneywiz_ledger::hold_reason(&store, inbox, ap, &amounts, mode)
                .map(|r| (ap.mw, r))
        })
        .collect();
    let held_exports: HashSet<&str> = plan
        .accounts
        .iter()
        .filter(|ap| held.contains_key(&ap.mw))
        .map(|ap| ap.export_account.as_str())
        .collect();
    let is_held = |l: &Link| {
        l.rows
            .iter()
            .any(|&i| held_exports.contains(inbox.txns[i].account.as_str()))
            || (matches!(l.kind, "balance" | "reconcile" | "share" | "start")
                && held_exports
                    .iter()
                    .any(|e| l.id.starts_with(&format!("{}|{e}|", l.kind))))
    };
    // An account with a row in a held transfer: its balance correction
    // counts that row, so it waits with it.
    let waits_along: HashSet<&str> = links
        .iter()
        .filter(|l| is_held(l))
        .flat_map(|l| l.rows.iter().map(|&i| inbox.txns[i].account.as_str()))
        .filter(|a| !held_exports.contains(a))
        .collect();
    let is_held = |l: &Link| {
        is_held(l)
            || (l.kind == "balance"
                && waits_along
                    .iter()
                    .any(|e| l.id.starts_with(&format!("balance|{e}|"))))
    };

    // The real app writes only its own database: links from a sync that read
    // another file would land in the owner's MoneyWiz. A test sets
    // CASHCAST_MONEYWIZ_OPEN to a fake app.
    let real_app = !std::env::var("CASHCAST_MONEYWIZ_OPEN").is_ok_and(|p| !p.is_empty());
    if apply && real_app {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        if !app_owns(&home, db) {
            return Err(format!(
                "refusing to send: the MoneyWiz app writes its own database, not {} (the file this sync read)",
                db.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            ));
        }
    }
    let _lock = if apply {
        Some(SyncLock::take(&data)?)
    } else {
        None
    };
    if apply {
        // A failed apply must not leave a cache key for the old MoneyWiz.
        ledger.inputs.clear();
        ledger.write(&data)?;
    }
    // The key is made on a dry run too: the ledger needs it to name rows.
    let mut sent = SentList::load(&data, today, true)?;
    sent.mark_saved(&links);
    let mut waiting = 0;
    let mut held_links = 0;
    let mut gone: Vec<&Link> = Vec::new();
    let mut to_send: Vec<&Link> = Vec::new();
    for l in &links {
        if is_held(l) {
            held_links += 1;
            continue;
        }
        // A resync window sends again what the owner deleted in MoneyWiz.
        let rechecked = !l.rows.is_empty()
            && l.rows.iter().all(|&i| {
                let t = &inbox.txns[i];
                ledger.rechecks(&t.account, t.date)
            });
        match sent.state(l, today) {
            // `--resend` also retries today's links MoneyWiz lacks now, even
            // one marked seen: a confirm that ran while MoneyWiz was still
            // writing can see a link under another amount. The owner runs it
            // only after a quit and a dry run show the row missing.
            SentState::Waiting if resend => to_send.push(l),
            SentState::Waiting => waiting += 1,
            SentState::Gone if !resend && !rechecked => gone.push(l),
            _ => to_send.push(l),
        }
    }
    let mut sent_now = 0;
    let mut confirmed = None;
    let mut sent_ids = HashSet::new();
    let mut sent_rows: HashSet<usize> = HashSet::new();
    if apply {
        let delay = std::env::var("CASHCAST_MONEYWIZ_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400);
        // Batches, each confirmed in the database: a MoneyWiz dialog (a
        // negative-balance alert, say) makes it drop every link after it.
        let batch = std::env::var("CASHCAST_MONEYWIZ_BATCH")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&n: &usize| n > 0)
            .unwrap_or(50);
        let mut blocked = None;
        let mut saved_total = 0;
        let chunks: Vec<&[&Link]> = to_send.chunks(batch).collect();
        for (n, chunk) in chunks.iter().enumerate() {
            let more = n + 1 < chunks.len();
            let mut ids = HashSet::new();
            for l in chunk.iter() {
                send(&l.url)?;
                sent.record(l, today);
                ids.insert(l.id.clone());
                sent_rows.extend(l.rows.iter().copied());
                sent_now += 1;
                // Written after every link, so a crash or a kill mid-run never
                // sends the links MoneyWiz already took a second time.
                sent.write(&data)?;
                std::thread::sleep(Duration::from_millis(delay));
            }
            let (saved, _, missing) = confirm(db, inbox, cfg, home, &ids, mode)?;
            sent.mark_saved(&missing);
            saved_total += saved;
            sent_ids.extend(ids);
            if saved == 0 && more {
                // None seen yet while more wait: stop. They stay recorded as
                // sent: MoneyWiz may only be slow to write them (in the
                // background it can take minutes), and sending them again
                // would add every row twice. `--resend` retries them.
                blocked = Some(chunk.len());
            }
            sent.write(&data)?;
            if blocked.is_some() {
                break;
            }
        }
        if sent_now > 0 || blocked.is_some() {
            confirmed = Some((saved_total, sent_now, blocked));
        }
    }

    let mut out = format!(
        "# MoneyWiz sync\n\nDatabase: {} (read from a copy; never written)\n\n",
        store.file_name
    );
    out.push_str("| MoneyWiz account | Export account | Period | Bank rows | Match | To add | Amounts differ | Only in MoneyWiz | Balance check |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|\n");
    let (mut total_add, mut total_fix, mut total_only) = (0, 0, 0);
    for ap in &plan.accounts {
        let a = store.account(ap.mw).expect("linked account exists");
        let period = match (ap.from, ap.to) {
            (Some(f), Some(t)) => format!("{f} to {t}"),
            _ => "-".into(),
        };
        let (diff, added) = sums(inbox, ap, &amounts);
        let check = balance_check(&store, ap, mode, diff, added);
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} ({}) | {} ({}) | {} | {} |\n",
            a.name,
            ap.export_account,
            period,
            ap.bank_rows,
            ap.matched,
            ap.add.len(),
            fmt_cents(added),
            ap.fix.len(),
            fmt_cents(diff),
            ap.only_in_moneywiz.len(),
            check
        ));
        total_add += ap.add.len();
        total_fix += ap.fix.len();
        total_only += ap.only_in_moneywiz.len();
    }
    if plan.accounts.is_empty() {
        out.push_str("| (none) | | | | | | | | |\n");
    }
    let invested = broker_trades(&store, cfg, home);
    out.push_str(&invested.1);
    let untouched: Vec<String> = plan
        .untouched
        .iter()
        .filter_map(|pk| store.account(*pk))
        .filter(|a| !invested.0.contains(&a.name))
        .map(|a| a.name.clone())
        .collect();
    if !untouched.is_empty() {
        out.push_str(&format!(
            "\nLeft alone (you record these by hand): {}\n",
            untouched.join(", ")
        ));
    }
    let saved = export_saved(&home.join("inbox"), inbox, today);
    let limit = stale_limit();
    let crypto = crate::tasks::connectors::load_all()
        .map(|specs| crypto_accounts(inbox, &crate::tasks::fetch::crypto_formats(&specs)))
        .unwrap_or_default();
    let stale: Vec<String> = plan
        .accounts
        .iter()
        .filter(|ap| {
            let closed = ap.to.is_none()
                && ap.bank_balance.as_ref().is_some_and(|b| b.cents == 0)
                && store.balance_on(ap.mw, today) == 0;
            let got = saved.get(&ap.export_account).copied();
            let limit = if crypto.contains(&ap.export_account) {
                crypto_stale_limit()
            } else {
                limit
            };
            export_old(ap.to, got, today, closed, limit)
        })
        .filter_map(|ap| {
            let name = &store.account(ap.mw)?.name;
            let rows = ap.to.map_or("no rows".to_string(), |t| format!("ends {t}"));
            Some(match saved.get(&ap.export_account) {
                Some(d) => format!("{name} ({rows}, downloaded {d})"),
                None => format!("{name} ({rows})"),
            })
        })
        .collect();
    if !stale.is_empty() {
        out.push_str(&format!(
            "\nExports older than {limit} days: {}. Download newer rows (`bin/cashcast connectors show <bank>`), then sync again.\n",
            stale.join(", ")
        ));
    }
    // MoneyWiz drops a link sent to an archived account, with no alert.
    let archived: Vec<&str> = store
        .accounts
        .iter()
        .filter(|a| a.archived)
        .filter(|a| {
            let key = format!("ccount={}", encode(&link_account(&a.name)));
            links
                .iter()
                .any(|l| l.url.contains(&format!("{key}&")) || l.url.ends_with(&key))
        })
        .map(|a| a.name.as_str())
        .collect();
    if !archived.is_empty() {
        out.push_str(&format!(
            "\nArchived in MoneyWiz, so MoneyWiz drops their links: {}. Show each first (`bin/cashcast moneywiz edit --account NAME --archived no`), sync, then archive it again (`--archived yes`).\n",
            archived.join(", ")
        ));
    }
    if !plan.unlinked_exports.is_empty() {
        out.push_str(&format!(
            "\nExport accounts with no MoneyWiz account: {}. Link one in cashcast.toml: [moneywiz.accounts] \"MoneyWiz name\" = \"export name\".\n",
            plan.unlinked_exports.join(", ")
        ));
    }
    if !plan.file_named.is_empty() {
        out.push_str(&format!(
            "\nDownloads that name no account (a TD or CIBC CSV; their rows stay out of the sync): {}. Download that bank as QFX instead, then move these files out of the inbox.\n",
            plan.file_named.join(", ")
        ));
    }
    let transfers = links.iter().filter(|l| l.kind == "transfer").count();
    out.push_str(&format!(
        "\n{total_add} rows to add ({} links, {transfers} of them transfers), {total_fix} amounts differ, {total_only} MoneyWiz rows have no bank row.\n",
        links
            .iter()
            .filter(|l| !matches!(l.kind, "correction" | "balance" | "share" | "start"))
            .count()
    ));
    if total_fix > 0 {
        out.push_str(if mode == "adjust" {
            "Amounts that differ get a correction transaction (memo \"cashcast fix\").\n"
        } else if mode == "edit" {
            "Amounts that differ are edited in place after the sync ([moneywiz] fixes = \"edit\", `moneywiz edit --fixes`).\n"
        } else {
            "Amounts that differ are listed in out/moneywiz-fixes.md for the owner ([moneywiz] fixes = \"list\"). Remove that line to have them corrected automatically.\n"
        });
    }
    let offsets = links.iter().filter(|l| l.kind == "reconcile").count();
    let stand_ins: usize = plan.accounts.iter().map(|ap| ap.stand_in.len()).sum();
    if offsets > 0 {
        out.push_str(&format!(
            "{} reconcile rows and {stand_ins} rows with no bank row in the synced periods get an offset (memo \"{FIX_MEMO}<row>\"): the rows they stood in for are now added. Rows with no bank row are offset only where the bank balance shows they stood in.\n",
            offsets - stand_ins.min(offsets)
        ));
    }
    let shares = links.iter().filter(|l| l.kind == "share").count();
    let starts = links.iter().filter(|l| l.kind == "start").count();
    if shares + starts > 0 {
        out.push_str(&format!(
            "Stock plan accounts: {shares} buy and sell rows get an offset (memo \"{FIX_MEMO}<row>\"): they move shares, and the statements hold the cash. {starts} start corrections (memo \"{START_MEMO}\") set the cash before the first statement row to what the statements show.\n"
        ));
    }
    let balance: Vec<String> = links
        .iter()
        .filter(|l| l.kind == "balance")
        .filter_map(|l| {
            let mut parts = l.id.splitn(3, '|').skip(1);
            let (account, cents) = (parts.next()?, parts.next()?.parse::<i64>().ok()?);
            Some(format!("{account} {}", fmt_cents(cents)))
        })
        .collect();
    let spans: Vec<String> = links
        .iter()
        .filter(|l| l.kind == "span")
        .filter_map(|l| {
            let mut parts = l.id.splitn(4, '|').skip(1);
            let (account, day) = (parts.next()?, parts.next()?);
            let cents = parts.next()?.parse::<i64>().ok()?;
            Some(format!("{account} {} on {day}", fmt_cents(cents)))
        })
        .collect();
    if !spans.is_empty() {
        out.push_str(&format!(
            "{} statement corrections (memo \"{SPAN_MEMO}\"), on the last day of a month the bank never issued or on a statement day of an account with statements and no rows, make MoneyWiz match the statements: {}.\n",
            spans.len(),
            spans.join(", ")
        ));
    }
    if !balance.is_empty() {
        out.push_str(&format!(
            "{} balance corrections (memo \"{BALANCE_MEMO}\"), the day before each period, make MoneyWiz match the bank's balance: {}.\n",
            balance.len(),
            balance.join(", ")
        ));
    }
    if total_fix + total_only > 0 {
        out.push_str(
            "The owner's list of rows to check: out/moneywiz-fixes.md (agents must not open it).\n",
        );
    }
    let conflicts: usize = plan.accounts.iter().map(|a| a.conflicts.len()).sum();
    if conflicts > 0 {
        out.push_str(&format!(
            "{conflicts} conflicts: bank rows that match more than one MoneyWiz row equally well. Nothing is added or fixed for them; pick the right row in out/moneywiz-fixes.md.\n"
        ));
    }
    if !inbox.moneywiz_files.is_empty() {
        out.push_str(&format!(
            "Not compared: {} MoneyWiz export(s) in the inbox. MoneyWiz is what the sync fills, not a bank.\n",
            inbox.moneywiz_files.len()
        ));
    }
    if waiting > 0 {
        out.push_str(&format!(
            "{waiting} links were sent today already and are not sent again until tomorrow.\n"
        ));
    }
    if !gone.is_empty() {
        out.push_str(&format!(
            "{} links were sent on an earlier day, but MoneyWiz does not hold them now (deleted or changed there?). They are not sent again; see out/moneywiz-fixes.md. To send them again, run `bin/cashcast moneywiz sync --resend`.\n",
            gone.len()
        ));
    }
    let to_categorize = uncategorized(&store, &plan);
    if to_categorize.is_empty() {
        out.push_str("Categories: every synced row has one.\n");
    } else {
        out.push_str(&format!(
            "Categories to fix: {} synced rows have no category ({}). Set them in MoneyWiz; the list is in out/moneywiz-fixes.md.\n",
            to_categorize.len(),
            guess_counts(&to_categorize)
        ));
    }
    if apply {
        out.push_str(&format!("\nSent {sent_now} links to MoneyWiz.\n"));
        if let Some((saved, total, blocked)) = confirmed {
            out.push_str(&format!("MoneyWiz saved {saved} of {total}.\n"));
            if let Some(n) = blocked {
                out.push_str(&format!(
                    "Stopped: MoneyWiz has saved none of the last {n} links yet. It may show a dialog (a negative-balance alert, for example), or be slow in the background. Close any dialog, keep MoneyWiz in front a minute, then check with `bin/cashcast moneywiz sync --dry-run`. Run with `--resend` only if those rows are still missing, and not right away (MoneyWiz may still save them). Note: `--resend` also sends again rows you deleted in MoneyWiz.\n"
                ));
            } else if saved < total {
                out.push_str("Some are not in the database yet. Keep MoneyWiz open a minute and run `bin/cashcast moneywiz sync --dry-run` to check.\n");
            }
        }
    } else {
        out.push_str("\nDry run: nothing was sent. Run `bin/cashcast moneywiz sync` to apply.\n");
    }

    // The ledger: every bank row's status and dates, and this run.
    let statuses = crate::moneywiz_ledger::statuses(&sent, inbox, &plan, &held, &sent_rows);
    ledger.update(&statuses, today);
    let mut accounts = serde_json::Map::new();
    for ap in &plan.accounts {
        let name = store
            .account(ap.mw)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        accounts.insert(
            name,
            serde_json::json!({
                "bank_rows": ap.bank_rows,
                "matched": ap.matched,
                "fix": ap.fix.len(),
                "add": ap.add.len(),
                "conflicts": ap.conflicts.len(),
                "only_in_moneywiz": ap.only_in_moneywiz.len(),
                "from": ap.from.map(|d| d.to_string()),
                "to": ap.to.map(|d| d.to_string()),
                "held": held.get(&ap.mw),
            }),
        );
    }
    ledger.add_run(serde_json::json!({
        "at": chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        "day": today.to_string(),
        "apply": apply,
        "sent": sent_now,
        "accounts": accounts,
    }));
    if apply {
        // Keep the windows of held accounts: nothing was sent there yet.
        let mut kept = Vec::new();
        for w in ledger.recheck.drain(..) {
            match &w.account {
                Some(a) if held_exports.contains(a.as_str()) => kept.push(w),
                Some(_) => {}
                None => kept.extend(held_exports.iter().map(|e| crate::moneywiz_ledger::Window {
                    account: Some(e.to_string()),
                    ..w.clone()
                })),
            }
        }
        ledger.recheck = kept;
    }
    // A dry run after an apply must look again: MoneyWiz changed.
    ledger.inputs = if apply { String::new() } else { inputs };
    if !held.is_empty() {
        out.push_str(&format!(
            "\nHeld ({held_links} links not sent): the plan for these accounts looks wrong, so nothing changes there until it is fixed.\n"
        ));
        for ap in &plan.accounts {
            if let Some(r) = held.get(&ap.mw) {
                let name = store.account(ap.mw).map(|a| a.name.as_str()).unwrap_or("");
                out.push_str(&format!("- {name}: {r}\n"));
            }
        }
        // A transfer with one side in a held account waits with it.
        let mut along: BTreeMap<&str, usize> = BTreeMap::new();
        for l in links.iter().filter(|l| is_held(l)) {
            for &i in &l.rows {
                let a = inbox.txns[i].account.as_str();
                if !held_exports.contains(a) {
                    *along.entry(a).or_default() += 1;
                }
            }
        }
        if !along.is_empty() {
            let list: Vec<String> = along.iter().map(|(a, n)| format!("{a} ({n})")).collect();
            out.push_str(&format!(
                "- Rows of other accounts in a transfer with a held account wait too: {}.\n",
                list.join(", ")
            ));
        }
    }
    let rows = ledger.rows.len();
    out.push_str(&format!(
        "Ledger: {rows} bank rows tracked in data/{} (status and the days each was first seen, checked and changed).\n",
        crate::moneywiz_ledger::LEDGER_FILE
    ));
    ledger.write(&data)?;
    crate::write_private(&data.join(crate::moneywiz_ledger::REPORT_FILE), &out)?;
    Ok(SyncResult {
        report: out,
        fixes_md: render_fixes(&store, inbox, &plan, &gone),
        sent: sent_now,
    })
}

/// MoneyWiz rows in the synced accounts' windows with no category (transfers
/// excluded): the owner's category fixes.
/// (rows with a category, rows that need one) in the compared windows.
pub fn category_coverage(store: &Store, plan: &Plan) -> (usize, usize) {
    let mut out = (0, 0);
    for ap in &plan.accounts {
        let (Some(from), Some(to), None) = (ap.from, ap.to, &ap.note) else {
            continue;
        };
        for t in store.txns.iter().filter(|t| t.account == ap.mw) {
            if t.date < from
                || t.date > to
                || t.cents == 0
                || !t.is_cash()
                || t.is_correction()
                || t.other_account.is_some()
            {
                continue;
            }
            out.1 += 1;
            out.0 += usize::from(!t.category.is_empty());
        }
    }
    out
}

pub fn uncategorized<'a>(store: &'a Store, plan: &Plan) -> Vec<(&'a MwTxn, i64)> {
    let mut rows = Vec::new();
    for ap in &plan.accounts {
        // Only accounts the sync compared: a note means it skipped this one.
        let (Some(from), Some(to), None) = (ap.from, ap.to, &ap.note) else {
            continue;
        };
        for t in store.txns.iter().filter(|t| t.account == ap.mw) {
            let inside = t.date >= from && t.date <= to;
            // Zero rows, cashcast's own corrections and non-cash rows need
            // no category.
            let needs = t.cents != 0 && t.is_cash() && !t.is_correction();
            if inside && needs && t.category.is_empty() && t.other_account.is_none() {
                rows.push((t, ap.mw));
            }
        }
    }
    rows.sort_by_key(|(t, _)| (t.date, t.pk));
    rows
}

/// "Groceries ×2, no guess ×1": counts by the engine's guess, most first.
fn guess_counts(rows: &[(&MwTxn, i64)]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for (t, _) in rows {
        let g = guessed_category(t).unwrap_or("no guess").to_string();
        match counts.iter_mut().find(|(k, _)| *k == g) {
            Some(c) => c.1 += 1,
            None => counts.push((g, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    counts
        .iter()
        .map(|(k, n)| format!("{k} ×{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Re-reads the database until MoneyWiz has saved the links, or time runs out.
/// Returns (saved, sent).
fn confirm(
    db: &Path,
    inbox: &Inbox,
    cfg: &crate::config::Config,
    home: &Path,
    sent: &HashSet<String>,
    mode: &str,
) -> Result<(usize, usize, Vec<Link>), String> {
    let total = sent.len();
    let wait = std::env::var("CASHCAST_MONEYWIZ_WAIT_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30u64);
    let start = std::time::Instant::now();
    loop {
        let store = open(db)?;
        let mut plan = plan(&store, inbox, cfg.moneywiz.as_ref())?;
        // The same trades as the sent plan, so balance link ids match.
        add_pending_trades(&mut plan, &store, inbox, cfg, home);
        let missing = links_for(&store, inbox, &plan, mode);
        let left = missing.iter().filter(|l| sent.contains(&l.id)).count();
        if left == 0 || start.elapsed().as_secs() >= wait {
            return Ok((total.saturating_sub(left), total, missing));
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}

fn render_fixes(store: &Store, inbox: &Inbox, plan: &Plan, gone: &[&Link]) -> String {
    let amounts = corrected(store);
    let mut out = String::from(
        "# MoneyWiz: rows to check\n\nPrivate: for the owner only. Agents must not open this file.\n\n## Amounts that differ from the bank\n\n| Date | Account | MoneyWiz | MoneyWiz amount | Bank | Bank amount |\n|---|---|---|---|---|---|\n",
    );
    for ap in &plan.accounts {
        let a = store.account(ap.mw).expect("linked account exists");
        for &(pk, i) in &ap.fix {
            let m = store.txns.iter().find(|t| t.pk == pk).expect("row exists");
            let b = &inbox.txns[i];
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                m.date,
                a.name,
                cell(&m.text),
                fmt_cents(amounts[&pk]),
                cell(&b.text),
                fmt_cents(b.cents)
            ));
        }
    }
    out.push_str("\n## In MoneyWiz, not at the bank (a duplicate, or not posted yet)\n\n| Date | Account | MoneyWiz | Amount |\n|---|---|---|---|\n");
    for ap in &plan.accounts {
        let a = store.account(ap.mw).expect("linked account exists");
        for pk in &ap.only_in_moneywiz {
            let m = store.txns.iter().find(|t| t.pk == *pk).expect("row exists");
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                m.date,
                a.name,
                cell(&m.text),
                fmt_cents(amounts[pk])
            ));
        }
    }
    out.push_str("\n## Conflicts: a bank row matches more than one MoneyWiz row\n\nNothing was added or fixed for these. Fix the right MoneyWiz row by hand (or delete a duplicate), then sync again.\n\n| Account | Bank rows | MoneyWiz rows |\n|---|---|---|\n");
    for ap in &plan.accounts {
        let a = store.account(ap.mw).expect("linked account exists");
        for c in &ap.conflicts {
            let bank: Vec<String> = c
                .rows
                .iter()
                .map(|&i| {
                    let b = &inbox.txns[i];
                    format!("{} {} {}", b.date, cell(&b.text), fmt_cents(b.cents))
                })
                .collect();
            let mw: Vec<String> = c
                .candidates
                .iter()
                .filter_map(|pk| store.txns.iter().find(|t| t.pk == *pk))
                .map(|m| format!("{} {} {}", m.date, cell(&m.text), fmt_cents(amounts[&m.pk])))
                .collect();
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                a.name,
                bank.join("; "),
                mw.join("; ")
            ));
        }
    }
    out.push_str("\n## No category yet\n\nSet a category in MoneyWiz. The guess is cashcast's; it changes nothing.\n\n| Date | Account | MoneyWiz | Amount | Guess |\n|---|---|---|---|---|\n");
    for (t, acct) in uncategorized(store, plan) {
        let a = store.account(acct).expect("linked account exists");
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            t.date,
            a.name,
            cell(&t.text),
            fmt_cents(amounts.get(&t.pk).copied().unwrap_or(t.cents)),
            guessed_category(t).unwrap_or("")
        ));
    }
    out.push_str("\n## Sent before, not in MoneyWiz now (deleted or changed there?)\n\nNot sent again. If you deleted them on purpose, nothing to do. To send them again: `bin/cashcast moneywiz sync --resend`.\n\n| Kind | Date | Bank row | Amount |\n|---|---|---|---|\n");
    for l in gone {
        for &i in &l.rows {
            let b = &inbox.txns[i];
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                l.kind,
                b.date,
                cell(&b.text),
                fmt_cents(b.cents)
            ));
        }
        if l.rows.is_empty() {
            out.push_str(&format!("| {} | | | |\n", l.kind));
        }
    }
    out
}

fn cell(text: &str) -> String {
    clean_text(text).replace('|', "/")
}

// --- MoneyWiz as an import source -----------------------------------------

/// The MoneyWiz accounts that no export follows (cash, for example), as one
/// export for `import`. None when there is no `[moneywiz]` section, it says
/// `import = false`, or the inbox already holds a MoneyWiz CSV.
pub fn import_source(cfg: &Config, files: &[Export]) -> Option<Result<Export, String>> {
    let mw = cfg.moneywiz.as_ref()?;
    if mw.import == Some(false) || files.iter().any(|f| f.format == "MoneyWiz CSV") {
        return None;
    }
    Some(find_db(Some(mw)).and_then(|db| open(&db)).map(|store| {
        let mut accounts: Vec<String> = files
            .iter()
            .flat_map(|f| f.txns.iter().map(|t| t.account.clone()))
            .collect();
        accounts.sort();
        accounts.dedup();
        let links = links(&store, Some(mw), &accounts);
        to_export(&store, &links)
    }))
}

fn to_export(store: &Store, links: &BTreeMap<i64, String>) -> Export {
    let mut export = Export::empty("MoneyWiz database", "MoneyWiz database");
    let keep: HashMap<i64, &Account> = store
        .accounts
        .iter()
        .filter(|a| !links.contains_key(&a.pk))
        // Only cash: a bank or card account here usually has its own export
        // under another name, and importing both would count it twice.
        .filter(|a| a.kind == "CashAccount")
        .map(|a| (a.pk, a))
        .collect();
    for t in &store.txns {
        let Some(a) = keep.get(&t.account) else {
            continue;
        };
        let derived = text::derive(&t.text, t.cents);
        let mut h = DefaultHasher::new();
        ("moneywiz", t.pk).hash(&mut h);
        let transfer = t
            .other_account
            .and_then(|o| store.account(o))
            .map(|o| o.name.clone())
            .unwrap_or_default();
        let mut category = t.category.clone();
        if category.is_empty()
            && transfer.is_empty()
            && let Some(guess) = derived.category
        {
            category = vec![guess.to_string()];
        }
        export.txns.push(Txn {
            file: 0,
            account: a.name.clone(),
            transfer,
            category_from_file: !t.category.is_empty(),
            category,
            hint: derived.hint,
            payee_key: derived.payee_key,
            date: t.date,
            cents: t.cents,
            currency: a.currency.clone(),
            fingerprint: h.finish(),
            text: t.text.clone(),
        });
    }
    for a in keep.values() {
        export.balances.insert(
            a.name.clone(),
            Balance {
                cents: store.balance(a.pk),
                as_of: None,
                source: "MoneyWiz balance",
            },
        );
        export.account_kinds.insert(
            a.name.clone(),
            if a.is_card() {
                AccountKind::Card
            } else {
                AccountKind::Bank
            },
        );
    }
    export.notes.push(format!(
        "{} MoneyWiz accounts recorded by hand, read from the MoneyWiz database",
        keep.len()
    ));
    export
}

#[cfg(test)]
mod tests {
    /// A rebuilt engine (same size, new time) misses the sync report cache.
    #[test]
    fn a_rebuilt_engine_changes_the_build_stamp() {
        let f = std::env::temp_dir().join(format!("cashcast-stamp-{}", std::process::id()));
        std::fs::write(&f, "engine").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let file = std::fs::File::options().write(true).open(&f).unwrap();
        file.set_modified(old).unwrap();
        let before = build_stamp(&f);
        file.set_modified(old + Duration::from_secs(60)).unwrap();
        assert_ne!(build_stamp(&f), before);
        assert!(build_stamp(&f.with_extension("gone")).is_empty());
        let _ = std::fs::remove_file(&f);
    }

    /// A new download that repeats rows an older file already holds still
    /// counts as the account's newest download: the duplicate check keeps the
    /// older copy of each row, so the download day comes from every file
    /// that held the account's rows.
    #[test]
    fn a_repeat_download_refreshes_the_download_day() {
        let dir = std::env::temp_dir().join(format!("cashcast-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        let file = |name: &str, saved: NaiveDate| {
            let mut e = Export::empty(name, "canary-format");
            e.txns.push(Txn {
                file: 0,
                account: "canary-account".into(),
                transfer: String::new(),
                category: Vec::new(),
                category_from_file: false,
                hint: text::Hint::None,
                payee_key: 0,
                date: day(9, 25),
                cents: -100,
                currency: "CAD".into(),
                fingerprint: 42,
                text: "canary-row".into(),
            });
            let path = dir.join(name);
            std::fs::write(&path, "x").unwrap();
            let at = Local
                .from_local_datetime(&saved.and_hms_opt(12, 0, 0).unwrap())
                .unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(at.into()).unwrap();
            e
        };
        let old = file("a-old.csv", day(10, 19));
        let new = file("b-new.csv", day(11, 16));
        let inbox = Inbox::merge(vec![old, new], Vec::new());
        assert_eq!(inbox.txns.len(), 1, "the repeated row is kept once");
        let saved = export_saved(&dir, &inbox, day(11, 16));
        assert_eq!(saved.get("canary-account"), Some(&day(11, 16)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An account only crypto-only formats fill gets the crypto limit; one
    /// a bank file also fills keeps the bank limit.
    #[test]
    fn only_accounts_of_crypto_only_files_are_crypto() {
        let row = |account: &str, fingerprint| Txn {
            file: 0,
            account: account.into(),
            transfer: String::new(),
            category: Vec::new(),
            category_from_file: false,
            hint: text::Hint::None,
            payee_key: 0,
            date: NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
            cents: -100,
            currency: "CAD".into(),
            fingerprint,
            text: "canary-crypto".into(),
        };
        let mut coins = Export::empty("coins.csv", "canary-coin-csv");
        coins.txns = vec![row("Canary Coins", 1), row("Canary Mixed", 2)];
        let mut bank = Export::empty("bank.csv", "canary-bank-csv");
        bank.txns = vec![row("Canary Mixed", 3), row("Canary Bank", 4)];
        let inbox = Inbox::merge(vec![coins, bank], Vec::new());
        let crypto = crypto_accounts(&inbox, &["canary-coin-csv".to_string()]);
        assert_eq!(
            crypto.into_iter().collect::<Vec<_>>(),
            vec!["Canary Coins".to_string()]
        );
    }

    #[test]
    fn an_export_is_old_only_when_its_rows_and_its_download_are() {
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        let today = d(11, 16);
        // No activity since Nov 3, downloaded today: current.
        assert!(!export_old(
            Some(d(11, 3)),
            Some(d(11, 16)),
            today,
            false,
            7
        ));
        // Old rows and an old download: refresh it.
        assert!(export_old(Some(d(11, 3)), Some(d(11, 8)), today, false, 7));
        // No download date known: the rows decide, as before.
        assert!(export_old(Some(d(11, 3)), None, today, false, 7));
        assert!(!export_old(Some(d(11, 12)), None, today, false, 7));
        assert!(export_old(None, None, today, false, 7));
        // An emptied account read from statements only: never old.
        assert!(!export_old(None, Some(d(11, 1)), today, true, 7));
        // The owner's limit decides: 7 days old passes 7 and fails 6.
        assert!(!export_old(Some(d(11, 9)), Some(d(11, 9)), today, false, 7));
        assert!(export_old(Some(d(11, 9)), Some(d(11, 9)), today, false, 6));
        assert!(!export_old(
            Some(d(11, 10)),
            Some(d(11, 10)),
            today,
            false,
            6
        ));
    }

    #[test]
    fn uncategorized_counts_only_compared_windows() {
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        let row = |pk, account, date, category: &[&str], other| MwTxn {
            pk,
            account,
            entity: "WithdrawTransaction".into(),
            date,
            cents: -100,
            text: "CANARY".into(),
            memo: String::new(),
            category: category.iter().map(|c| c.to_string()).collect(),
            category_kind: None,
            fixed_amount: false,
            other_account: other,
            holding: None,
            gid: String::new(),
        };
        let store = Store {
            file_name: String::new(),
            layout: String::new(),
            accounts: Vec::new(),
            txns: vec![
                row(1, 1, d(9, 10), &[], None),       // counted
                row(2, 1, d(9, 11), &["Food"], None), // has a category
                row(3, 1, d(9, 12), &[], Some(2)),    // a transfer
                row(4, 1, d(1, 5), &[], None),        // before the window
                row(5, 2, d(9, 10), &[], None),       // account with no bank rows
                row(6, 3, d(9, 10), &[], None),       // skipped (currency)
            ],
            trades: HashMap::new(),
            categories: Vec::new(),
            category_types: HashMap::new(),
            path_kinds: HashMap::new(),
            category_links: HashMap::new(),
            holdings: Vec::new(),
        };
        let window = |mw, note: Option<&str>, w: bool| AccountPlan {
            mw,
            from: w.then(|| d(9, 1)),
            to: w.then(|| d(9, 30)),
            note: note.map(String::from),
            ..Default::default()
        };
        let plan = Plan {
            accounts: vec![
                window(1, None, true),
                window(2, Some("no bank rows in the period"), false),
                window(3, Some("skipped: MoneyWiz uses USD, the export CAD"), true),
            ],
            untouched: Vec::new(),
            unlinked_exports: Vec::new(),
            file_named: Vec::new(),
        };
        let pks: Vec<i64> = uncategorized(&store, &plan)
            .iter()
            .map(|(t, _)| t.pk)
            .collect();
        assert_eq!(pks, vec![1]);
    }

    #[test]
    fn snapshot_folders_are_unique_even_within_one_clock_tick() {
        // macOS clocks move in microseconds: two snapshots in one tick once shared a folder,
        // and one test's cleanup deleted the other's copy.
        let names: std::collections::HashSet<_> =
            (0..1000).map(|_| super::snapshot_dir()).collect();
        assert_eq!(names.len(), 1000);
    }

    use super::*;

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("A b/c&d=é"), "A%20b%2Fc%26d%3D%C3%A9");
    }

    #[test]
    fn core_data_dates_count_from_2001() {
        // 2026-01-15 12:00 UTC is a 2026-01-15 date in every time zone from -11 to +11.
        let secs = (NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
            - CORE_DATA_EPOCH) as f64;
        assert_eq!(core_data_date(secs), NaiveDate::from_ymd_opt(2026, 1, 15));
    }

    #[test]
    fn similar_needs_a_shared_word() {
        assert!(similar("GROCER CANARY 123", "Grocer"));
        assert!(!similar("GROCER", "PHONE CO"));
        assert!(!similar("", ""));
    }
}
