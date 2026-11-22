//! Shared helpers for the app-behavior tests (ported from tests/conftest.py,
//! tests/helpers.py and tests/unit/full_profile.py). Synthetic data only:
//! every free-text value carries a canary.
//!
//! The app reads its folders from environment variables, which are global to
//! the test process. `Env::new` takes a lock, so tests in one file run one at
//! a time, each with its own fresh private root.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use cashcast_engine::core::paths::UserHome;
use cashcast_engine::core::users;
use chrono::NaiveDate;
use serde_json::{Value, json};

pub const CANARY: &str = "CANARY";

static LOCK: Mutex<()> = Mutex::new(());
static COUNT: AtomicUsize = AtomicUsize::new(0);

/// The repo root (one folder above `engine/`).
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// conftest's `today` fixture.
pub fn today() -> NaiveDate {
    d("2026-10-18")
}

/// One test's private root. Holds the env lock until dropped.
pub struct Env {
    pub root: PathBuf,
    pub tmp: PathBuf,
    _guard: MutexGuard<'static, ()>,
}

pub fn set(key: &str, value: &str) {
    // SAFETY: every test that touches the environment holds `LOCK`.
    unsafe { std::env::set_var(key, value) }
}

pub fn unset(key: &str) {
    // SAFETY: every test that touches the environment holds `LOCK`.
    unsafe { std::env::remove_var(key) }
}

impl Env {
    pub fn new() -> Env {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let n = COUNT.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!(
            "cashcast-app-{}-{}-{n}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
                .chars()
                .take(60)
                .collect::<String>()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let root = tmp.join("private");
        set("CASHCAST_PRIVATE", root.to_str().unwrap());
        set("CASHCAST_ROOT", repo().to_str().unwrap());
        set("CASHCAST_LOG", "off");
        set("CASHCAST_LIVE", "off");
        set("CASHCAST_POSTHOG_KEY", "");
        set("HOME", tmp.to_str().unwrap());
        for k in ["CASHCAST_HOME", "CASHCAST_USER"]
            .into_iter()
            .chain(cashcast_engine::core::consent::AGENT_ENV)
            .chain(["DO_NOT_TRACK", "CASHCAST_OWNER_UI"])
        {
            unset(k);
        }
        Env {
            root,
            tmp,
            _guard: guard,
        }
    }

    /// conftest's `make_user`, without extras.
    pub fn user(&self, name: &str) -> UserHome {
        users::add(name, "ON", Some(&title(name))).unwrap()
    }

    /// `make_user(name, province, profile_extra, spending, balances)`.
    pub fn make_user(
        &self,
        name: &str,
        province: &str,
        extra: &str,
        spending: Option<&[(&str, Vec<f64>)]>,
        balances: &[(&str, f64)],
    ) -> UserHome {
        let u = users::add(name, province, Some(&title(name))).unwrap();
        if !extra.is_empty() {
            append(&u.profile(), &format!("\n{extra}"));
        }
        if let Some(sp) = spending {
            write_json(&summary_path(&u), &summary(sp, "2025-10", balances));
        }
        u
    }

    /// A person with every profile section filled (full_profile.py), the
    /// SPENDING history and the given cash balances.
    pub fn person(&self, balances: &[(&str, f64)]) -> UserHome {
        let u = self.make_user("alex", "ON", "", Some(&spending()), balances);
        let text = std::fs::read_to_string(u.profile()).unwrap();
        std::fs::write(u.profile(), apply(&text)).unwrap();
        u
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

fn title(name: &str) -> String {
    let mut c = name.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn append(path: &Path, text: &str) {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    std::fs::write(path, old + text).unwrap();
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

pub fn write_json(path: &Path, v: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string(v).unwrap()).unwrap();
}

pub fn summary_path(u: &UserHome) -> PathBuf {
    u.data().join("summary.json")
}

pub fn holdings_path(u: &UserHome) -> PathBuf {
    u.data().join("holdings.json")
}

pub fn balances_path(u: &UserHome) -> PathBuf {
    u.data().join("api-balances.json")
}

/// helpers.grant_by_hand: what the owner does when editing consent.toml by hand.
pub fn grant_by_hand(u: &UserHome, scopes: &[&str]) {
    let text: String = scopes
        .iter()
        .map(|s| {
            format!(
                "[[grants]]\nscope = \"{s}\"\ngranted_on = 2026-10-18\nexpires_on = 2099-01-01\nvia = \"hand\"\n\n"
            )
        })
        .collect();
    cashcast_engine::write_private(&u.consent(), &text).unwrap();
}

pub fn months(start: &str, n: usize) -> Vec<String> {
    let (mut y, mut m): (i32, u32) = (start[..4].parse().unwrap(), start[5..].parse().unwrap());
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(format!("{y:04}-{m:02}"));
        if m == 12 {
            y += 1;
            m = 1;
        } else {
            m += 1;
        }
    }
    out
}

/// helpers.summary: a summary.json with spending per category and cash balances.
pub fn summary(spending: &[(&str, Vec<f64>)], start: &str, balances: &[(&str, f64)]) -> Value {
    let n = spending[0].1.len();
    let zero = vec![0; n];
    let accounts: Vec<Value> = balances
        .iter()
        .map(|(k, v)| {
            json!({"name": k, "class": "cash", "guessed": false, "class_reason": "", "currency": "CAD",
                   "rows": 1, "last_date": null, "balance_cents": (v * 100.0) as i64,
                   "balance_source": "test", "sum_balance_cents": null})
        })
        .collect();
    let mut sp = serde_json::Map::new();
    for (k, v) in spending {
        sp.insert(
            k.to_string(),
            json!(
                v.iter()
                    .map(|x| (x * 100.0).round() as i64)
                    .collect::<Vec<_>>()
            ),
        );
    }
    json!({
        "generated_on": "2026-10-18",
        "base_currency": "CAD",
        "months": months(start, n),
        "files": [],
        "accounts": accounts,
        "pay": {"schedule": null, "source": "", "anchor": null, "cycles": [], "half_years": [],
                "gap_days": {}, "off_cycle_deposits": 0, "deposit_accounts": {}, "cycles_per_month": []},
        "income_cents": {},
        "income_by_category_cents": {},
        "spending_cents": sp,
        "flows_cents": {"salary": zero, "salary_off_cycle": zero, "income_tax": zero, "invested": zero},
        "suggested_extra_income": [],
        "category_roles": {},
        "checks": [],
    })
}

/// full_profile.SPENDING.
pub fn spending() -> Vec<(&'static str, Vec<f64>)> {
    let mut dining = vec![300.0; 9];
    dining.extend([450.0; 3]);
    let mut travel = vec![0.0; 6];
    travel.extend([1200.0; 6]);
    vec![
        ("Food > Groceries", vec![500.0; 12]),
        ("Food > Restaurants", dining),
        ("Bills > Phone", vec![80.0; 12]),
        ("Travel", travel),
        ("Fees > Overdraft", vec![5.0; 12]),
        ("Income tax", vec![400.0; 12]),
    ]
}

/// full_profile.CARDS.
pub const CARDS: &str = r#"
[[wallet.cards]]
id = "test-flat"
name = "Test Flat 2%"
earn = { other = 0.02 }
fx_fee = 0.0
limit = 25000
statement_close_day = 24
paydown_days_before = 1

[[wallet.cards]]
id = "test-grocery"
name = "Test Grocery 4%"
earn = { groceries = 0.04, recurring = 0.04, gas = 0.02, other = 0.01 }
caps = [{ categories = ["groceries", "recurring"], limit = 25000, period = "card_year", after_rate = 0.01 }]
annual_fee = 120
fee_rebated = true
fx_fee = 0.025
limit = 15000
statement_close_day = 25

[[wallet.cards]]
id = "test-costly"
name = "Test Costly"
earn = { dining = 0.021, other = 0.005 }
annual_fee = 150
fx_fee = 0.025
limit = 5000
statement_close_day = 18

[[banking.accounts]]
id = "test-package"
bank = "Test Bank"
name = "Package"
monthly_fee = 30.95
waive_min_balance = 6000
card_fee_rebate = 120
keep = 6500
source = "https://example.com"
confidence = "estimate"
"#;

fn replace_once(text: &str, from: &str, to: &str) -> String {
    text.replacen(from, to, 1)
}

/// full_profile.apply: fills the example profile's empty sections.
pub fn apply(profile: &str) -> String {
    let mut t = replace_once(profile, "employment = 0\n", "employment = 120000\n");
    t = replace_once(&t, "rrsp = 0\n", "rrsp = 20000\n");
    t = replace_once(&t, "rrsp_limit_year = 0\n", "rrsp_limit_year = 2026\n");
    t = replace_once(&t, "tfsa = 0\n", "tfsa = 7000\n");
    t = replace_once(&t, "fhsa = 0\n", "fhsa = 8000\n");
    t = t.replace(
        "first_time_home_buyer = false",
        "first_time_home_buyer = true\nmortgage_in_months = 10",
    );
    t = t.replace("# monthly = 2000", "monthly = 3000").replace(
        "# service = \"chexy\"",
        "service = \"test-rent\"\nfee_rate = 0.015\nmembership_fee = 150\nmembership_rate = 0.015\ncodes_as = \"recurring\"",
    );
    t = t.replace("# \"Food & Dining\" = 800", "\"Food\" = 700");
    t = t.replace("# symbol = \"ACME\"", "symbol = \"EMPL\"");
    t = t.replace(
        "symbols = []",
        "symbols = [\"EMPL\", \"XEQT\"]\nmetrics = { EMPL = { pe = 22.0, profit_margin = 0.2, roe = 0.3, revenue_growth = 0.05, beta = 1.1 } }",
    );
    t + CARDS
}

/// The engine binary's `cli` subcommand, run with this process's environment.
pub fn cli(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cashcast-engine"))
        .arg("cli")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The USD rate in knowledge/data/foreign_cash.toml.
pub fn usd() -> f64 {
    let text = read(&repo().join("knowledge/data/foreign_cash.toml"));
    let t: toml::Table = toml::from_str(&text).unwrap();
    t["rates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str() == Some("USD"))
        .unwrap()["cad_per_unit"]
        .as_float()
        .unwrap()
}

pub fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// Python's `f"{x:,.2f}"`.
pub fn money(x: f64) -> String {
    cashcast_engine::strategy::py::commas(x, 2)
}
