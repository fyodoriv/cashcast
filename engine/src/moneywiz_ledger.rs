//! The MoneyWiz sync ledger: what each sync found for every bank row, kept
//! between runs in `data/moneywiz-ledger.json` (private, mode 600).
//!
//! - Rows: one entry per bank row, under a keyed hash of the row (never its
//!   text): its account, date, status, and the days it was first seen, last
//!   checked and last changed. A row can be checked again by date range
//!   (`moneywiz resync`).
//! - Runs: one entry per sync (counts per account, held accounts), the last
//!   100 kept. The web app shows them.
//! - Cache: the hash of every input (exports, MoneyWiz file, settings). A dry
//!   run with the same inputs and no resync pending reuses the last report.
//! - Gate: an account whose plan looks wrong is held: nothing is sent for it.

use crate::inbox::Inbox;
use crate::money::fmt_cents;
use crate::moneywiz::{AccountPlan, Plan, SentList, Store};
use chrono::NaiveDate;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const LEDGER_FILE: &str = "moneywiz-ledger.json";
pub const REPORT_FILE: &str = "moneywiz-last-report.md";
const KEEP_RUNS: usize = 100;

/// Where a bank row stands after a sync.
pub const STATUSES: [&str; 6] = ["matched", "fix", "add", "sent", "conflict", "held"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub account: String,
    pub date: NaiveDate,
    pub status: String,
    pub first_seen: NaiveDate,
    pub last_checked: NaiveDate,
    pub last_changed: NaiveDate,
}

/// A date range of one account (or all) to check again on the next sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub account: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub asked: NaiveDate,
}

impl Window {
    pub fn holds(&self, account: &str, date: NaiveDate) -> bool {
        self.account.as_deref().is_none_or(|a| a == account)
            && self.from.is_none_or(|f| date >= f)
            && self.to.is_none_or(|t| date <= t)
    }
}

#[derive(Default)]
pub struct Ledger {
    pub rows: BTreeMap<String, Row>,
    pub runs: Vec<Value>,
    pub recheck: Vec<Window>,
    pub inputs: String,
}

fn day(v: Option<&Value>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v?.as_str()?, "%Y-%m-%d").ok()
}

impl Ledger {
    pub fn load(data: &Path) -> Ledger {
        let text = std::fs::read_to_string(data.join(LEDGER_FILE)).unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or_default();
        if v.get("version").and_then(Value::as_i64) != Some(1) {
            return Ledger::default();
        }
        let rows = v["rows"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, r)| {
                        Some((
                            k.clone(),
                            Row {
                                account: r["account"].as_str()?.to_string(),
                                date: day(r.get("date"))?,
                                status: r["status"].as_str()?.to_string(),
                                first_seen: day(r.get("first_seen"))?,
                                last_checked: day(r.get("last_checked"))?,
                                last_changed: day(r.get("last_changed"))?,
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let recheck = v["recheck"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|w| {
                        Some(Window {
                            account: w["account"].as_str().map(str::to_string),
                            from: day(w.get("from")),
                            to: day(w.get("to")),
                            asked: day(w.get("asked"))?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ledger {
            rows,
            runs: v["runs"].as_array().cloned().unwrap_or_default(),
            recheck,
            inputs: v["inputs"].as_str().unwrap_or_default().to_string(),
        }
    }

    /// Written to a temporary file first, so a crash never leaves half a file.
    pub fn write(&self, data: &Path) -> Result<(), String> {
        let rows: BTreeMap<&String, Value> = self
            .rows
            .iter()
            .map(|(k, r)| {
                (
                    k,
                    json!({
                        "account": r.account,
                        "date": r.date.to_string(),
                        "status": r.status,
                        "first_seen": r.first_seen.to_string(),
                        "last_checked": r.last_checked.to_string(),
                        "last_changed": r.last_changed.to_string(),
                    }),
                )
            })
            .collect();
        let recheck: Vec<Value> = self
            .recheck
            .iter()
            .map(|w| {
                json!({
                    "account": w.account,
                    "from": w.from.map(|d| d.to_string()),
                    "to": w.to.map(|d| d.to_string()),
                    "asked": w.asked.to_string(),
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&json!({
            "version": 1,
            "inputs": self.inputs,
            "rows": rows,
            "runs": self.runs,
            "recheck": recheck,
        }))
        .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        let path = data.join(LEDGER_FILE);
        let tmp = data.join(format!("{LEDGER_FILE}.tmp"));
        crate::write_private(&tmp, &text)?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// Whether a row of `account` on `date` is asked to be checked again.
    pub fn rechecks(&self, account: &str, date: NaiveDate) -> bool {
        self.recheck.iter().any(|w| w.holds(account, date))
    }

    /// Updates every bank row of the plan. `status_of` gives a row's status.
    pub fn update(&mut self, keys: &[(String, String, NaiveDate, &'static str)], today: NaiveDate) {
        for (key, account, date, status) in keys {
            match self.rows.get_mut(key) {
                Some(r) => {
                    if r.status != *status {
                        r.status = status.to_string();
                        r.last_changed = today;
                    }
                    r.last_checked = today;
                }
                None => {
                    self.rows.insert(
                        key.clone(),
                        Row {
                            account: account.clone(),
                            date: *date,
                            status: status.to_string(),
                            first_seen: today,
                            last_checked: today,
                            last_changed: today,
                        },
                    );
                }
            }
        }
    }

    pub fn add_run(&mut self, run: Value) {
        self.runs.push(run);
        let extra = self.runs.len().saturating_sub(KEEP_RUNS);
        self.runs.drain(..extra);
    }

    /// Per account: rows by status, and the last day any row was checked.
    pub fn summary(&self) -> BTreeMap<String, (BTreeMap<String, usize>, Option<NaiveDate>)> {
        let mut out: BTreeMap<String, (BTreeMap<String, usize>, Option<NaiveDate>)> =
            BTreeMap::new();
        for r in self.rows.values() {
            let e = out.entry(r.account.clone()).or_default();
            *e.0.entry(r.status.clone()).or_default() += 1;
            e.1 = e.1.max(Some(r.last_checked));
        }
        out
    }
}

/// The ledger keys of an account's bank rows: a keyed hash of account, date,
/// amount, text and the copy number (two equal rows on one day stay two).
pub fn row_keys(sent: &SentList, inbox: &Inbox, rows: &[usize]) -> Vec<Option<String>> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    rows.iter()
        .map(|&i| {
            let t = &inbox.txns[i];
            let id = format!("{}|{}|{}|{}", t.account, t.date, t.cents, t.text);
            let n = seen.entry(id.clone()).or_default();
            *n += 1;
            sent.keyed(&format!("ledger|{id}|{n}"))
        })
        .collect()
}

/// Why an account's plan looks wrong, or None when it may be applied.
pub fn hold_reason(
    store: &Store,
    inbox: &Inbox,
    ap: &AccountPlan,
    amounts: &HashMap<i64, i64>,
    mode: &str,
) -> Option<String> {
    if ap.note.is_some() || ap.bank_rows == 0 {
        return None;
    }
    // Brokerage cash is never below 0: a statement is missing.
    if ap.stock_plan && ap.start_cash < 0 {
        return Some(format!(
            "the statements imply {} of cash before the first row: a statement is missing",
            fmt_cents(ap.start_cash)
        ));
    }
    // One statement's closing is not the next one's opening: a statement
    // is missing, and its month's MoneyWiz rows would look unmatched.
    // Unless MoneyWiz's own rows account for the month (the bank never
    // issued it): then they stand in for it.
    let sts = crate::moneywiz_diagnose::statements(inbox, &ap.export_account);
    let left = crate::moneywiz_diagnose::unexplained_breaks(&sts, |d| store.balance_on(ap.mw, d));
    if let Some(&b) = left.first() {
        return Some(format!(
            "a statement is missing ({}): download it, then sync",
            crate::moneywiz_diagnose::missing_days(b)
        ));
    }
    let (diff, added) = crate::moneywiz::sums(inbox, ap, amounts);
    let change = added + if mode == "list" { 0 } else { diff };
    let rough = ap
        .fix
        .iter()
        .filter(|&&(pk, i)| {
            let b = inbox.txns[i].cents;
            (amounts[&pk] - b).abs() * 20 > b.abs()
        })
        .count();
    if rough * 4 > ap.bank_rows {
        return Some(format!(
            "{rough} of {} bank rows would get a fix over 5 %: the pairs look like guesses",
            ap.bank_rows
        ));
    }
    match &ap.bank_balance {
        Some(b) if b.as_of.is_none_or(|d| Some(d) >= ap.to) => {
            let day = b.as_of.or(ap.to);
            let before = match day {
                Some(d) => store.balance_on(ap.mw, d),
                None => store.balance(ap.mw),
            };
            // Broker trades `edit --fixes` adds after the sync move the cash too.
            let change = change + ap.trades_upto(day);
            let (gap_before, gap_after) =
                ((before - b.cents).abs(), (before + change - b.cents).abs());
            // A right fix can move away a little (a wrong opening balance is fixed
            // apart): hold past max(100.00, 5 % of the bank balance).
            let room = (b.cents.abs() / 20).max(10_000);
            // Real rows that replace an opening balance standing in for them:
            // the opening-balance fix (adjust, or `edit --fixes`) then brings
            // the opening balance to about 0, and MoneyWiz back to the bank.
            let opening = store.account(ap.mw).map_or(0, |a| a.opening_cents);
            let excess = before + change - b.cents;
            let stand_in = mode != "list" && opening != 0 && (opening - excess).abs() <= room;
            // A row before the period stands in the same way (a transfer in
            // typed early, dividends never typed): MoneyWiz leads the bank's
            // own start balance (its balance less every row of the period)
            // by what the sync adds. The balance correction the day before
            // the period squares it.
            // Only when the bank's start is plausible (not below 0 outside a
            // card: below 0 means a statement or the balance is wrong) and
            // no MoneyWiz row of the period is left without a bank row (a
            // missing month would leave some).
            let card = store.account(ap.mw).is_some_and(|a| a.is_card());
            let whole = ap.only_in_moneywiz.is_empty() && ap.stand_in.is_empty();
            let early = whole
                && !card
                && ap.from.is_some_and(|from| {
                    let in_period: i64 = inbox
                        .txns
                        .iter()
                        .filter(|t| t.account == ap.export_account && t.date >= from)
                        .filter(|t| day.is_none_or(|d| t.date <= d))
                        .filter(|t| crate::moneywiz::from_bank(inbox, t))
                        .map(|t| t.cents)
                        .sum();
                    let start = b.cents - in_period;
                    let lead = store.balance_on(ap.mw, from - chrono::Days::new(1)) - start;
                    mode != "list" && start >= -room && lead != 0 && (lead - excess).abs() <= room
                });
            // The owner chose the bank for this account (`trust_bank`).
            (gap_after > gap_before + room && !stand_in && !early && !ap.trust_bank).then(|| {
                format!(
                    "the sync moves MoneyWiz away from the bank's balance (gap {} now, {} after)",
                    fmt_cents(gap_before),
                    fmt_cents(gap_after)
                )
            })
        }
        _ => (change.abs() > 500_000).then(|| {
            format!(
                "no bank balance to check, and the sync would move MoneyWiz by {}",
                fmt_cents(change)
            )
        }),
    }
}

/// A hash of every input of a sync: export files (name, size, time), the
/// MoneyWiz file (size, time), the settings and this program's version.
pub fn inputs_hash(inbox_dir: &Path, db: &Path, settings: &str) -> String {
    let mut text = format!("{}\n{settings}\n", env!("CARGO_PKG_VERSION"));
    let stamp = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| {
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                format!("{} {t}", m.len())
            })
            .unwrap_or_default()
    };
    let mut files: Vec<_> = std::fs::read_dir(inbox_dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for f in files {
        text.push_str(&format!("{} {}\n", f.display(), stamp(&f)));
    }
    // MoneyWiz writes through its WAL file first.
    for suffix in ["", "-wal"] {
        let p = std::path::PathBuf::from(format!("{}{suffix}", db.display()));
        text.push_str(&format!("{} {}\n", p.display(), stamp(&p)));
    }
    crate::digest::hex(&crate::digest::sha256(text.as_bytes()))
}

/// The ledger keys and statuses of every bank row in the plan.
pub fn statuses(
    sent: &SentList,
    inbox: &Inbox,
    plan: &Plan,
    held: &HashMap<i64, String>,
    sent_ids: &std::collections::HashSet<usize>,
) -> Vec<(String, String, NaiveDate, &'static str)> {
    let mut out = Vec::new();
    for ap in &plan.accounts {
        let mut status: HashMap<usize, &'static str> = HashMap::new();
        for &(_, i) in &ap.fix {
            status.insert(i, "fix");
        }
        for &i in &ap.add {
            status.insert(i, if sent_ids.contains(&i) { "sent" } else { "add" });
        }
        for c in &ap.conflicts {
            for &i in &c.rows {
                status.insert(i, "conflict");
            }
        }
        let rows: Vec<usize> = (0..inbox.txns.len())
            .filter(|&i| inbox.txns[i].account == ap.export_account)
            .filter(|&i| crate::moneywiz::from_bank(inbox, &inbox.txns[i]))
            .filter(|&i| ap.from.is_some_and(|f| inbox.txns[i].date >= f))
            .collect();
        let keys = row_keys(sent, inbox, &rows);
        for (&i, key) in rows.iter().zip(keys) {
            let Some(key) = key else { continue };
            let s = if held.contains_key(&ap.mw) && status.contains_key(&i) {
                "held"
            } else {
                status.get(&i).copied().unwrap_or("matched")
            };
            out.push((key, ap.export_account.clone(), inbox.txns[i].date, s));
        }
    }
    out
}

/// `moneywiz resync`: asks the next sync to check a range of rows again and
/// to send again what the owner deleted in MoneyWiz there.
pub fn ask_resync(
    data: &Path,
    account: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    today: NaiveDate,
) -> Result<String, String> {
    if let (Some(f), Some(t)) = (from, to)
        && f > t
    {
        return Err("--from must be on or before --to".into());
    }
    let mut ledger = Ledger::load(data);
    let w = Window {
        account,
        from,
        to,
        asked: today,
    };
    let n = ledger
        .rows
        .values()
        .filter(|r| w.holds(&r.account, r.date))
        .count();
    let what = format!(
        "{} from {} to {}",
        w.account.as_deref().unwrap_or("every account"),
        w.from.map(|d| d.to_string()).unwrap_or("the start".into()),
        w.to.map(|d| d.to_string()).unwrap_or("today".into())
    );
    ledger.recheck.push(w);
    ledger.inputs.clear();
    ledger.write(data)?;
    Ok(format!(
        "The next sync checks {what} again ({n} rows known so far) and sends again rows that MoneyWiz no longer holds there.\nRun `bin/cashcast moneywiz sync --dry-run` to see it, then `bin/cashcast moneywiz sync`.\n"
    ))
}
