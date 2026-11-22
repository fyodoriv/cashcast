//! In-place MoneyWiz edits: the plan the Core Data writer applies, and the
//! check that it did (decision 0007).
//!
//! The owner lists edits in `inbox/moneywiz-edits.toml` (agents never open
//! it); `moneywiz edit --fixes` adds the wrong amounts and opening balances
//! the sync found. Each row edit names the row as the owner sees it
//! (account, date, amount, part of the payee) and must match exactly one
//! MoneyWiz row, which the plan then names by its Core Data `GID`. The
//! writer checks the expected amount and date again before it changes
//! anything. Reports hold counts and edit numbers only.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::export::Txn;
use crate::moneywiz::{MwTxn, Plan, Store, corrected};

pub const CONTRACT: u32 = 1;
pub const EDITS_FILE: &str = "moneywiz-edits.toml";
pub const PLAN_FILE: &str = "moneywiz-plan.json";

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EditsFile {
    #[serde(default)]
    pub row: Vec<RowSpec>,
    #[serde(default)]
    pub category: Vec<CategorySpec>,
}

/// One row to change, as the owner sees it in MoneyWiz.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowSpec {
    pub account: String,
    pub date: String,
    pub amount: f64,
    /// Part of the payee or description, when the date and amount are not enough.
    pub text: Option<String>,
    pub set_amount: Option<f64>,
    /// A category path, such as "Food/Groceries".
    pub set_category: Option<String>,
    /// The category as its exact path parts, for a name that holds a slash.
    pub set_category_path: Option<Vec<String>>,
    /// "expense" or "income": which of two categories with the same path.
    pub set_category_kind: Option<String>,
    pub set_payee: Option<String>,
    pub set_memo: Option<String>,
    /// A new day, YYYY-MM-DD.
    pub set_date: Option<String>,
    /// Which of several identical rows (same day, amount and text), from 1
    /// in MoneyWiz's own order: one block per row gives each its edit.
    pub nth: Option<usize>,
    /// Remove the row (one the owner decided no bank row backs). Alone: no
    /// `set_*` with it, and never a transfer (its other side would stay).
    #[serde(default)]
    pub delete: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategorySpec {
    pub add: Option<String>,
    pub rename: Option<String>,
    pub to: Option<String>,
    pub delete: Option<String>,
    /// Where rows in a deleted category go.
    pub move_to: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct WriterPlan {
    pub contract: u32,
    pub rows: Vec<RowEdit>,
    pub accounts: Vec<AccountEdit>,
    pub categories: Vec<CategoryOp>,
    /// A category migration's tree after the plan (`moneywiz_categories::fingerprint`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_tree: Option<String>,
    /// Broker trades to add to MoneyWiz investment accounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trades: Vec<TradeAdd>,
    /// Holdings whose opening shares change (to 0 when the broker file
    /// holds every trade).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holdings: Vec<HoldingEdit>,
    /// Accounts to archive or show again (`edit --account NAME --archived
    /// yes|no`): MoneyWiz drops links sent to an archived account.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archive: Vec<ArchiveEdit>,
}

/// An account's `archived` flag, checked first.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ArchiveEdit {
    pub gid: String,
    pub expect: bool,
    pub archived: bool,
}

/// The change that archives (or shows again) the account `name`, or why
/// there is none: no such account, two of that name, or already so.
pub fn archive_edit(store: &Store, name: &str, archived: bool) -> Result<ArchiveEdit, String> {
    let found: Vec<_> = store
        .accounts
        .iter()
        .filter(|a| a.name == name.trim())
        .collect();
    match found[..] {
        [] => Err(format!("no MoneyWiz account named \"{name}\"")),
        [a] if a.gid.is_empty() => Err(format!("\"{name}\" has no Core Data id")),
        [a] if a.archived == archived => Err(format!(
            "\"{name}\" is already {}",
            if archived { "archived" } else { "shown" }
        )),
        [a] => Ok(ArchiveEdit {
            gid: a.gid.clone(),
            expect: a.archived,
            archived,
        }),
        _ => Err(format!(
            "{} MoneyWiz accounts are named \"{name}\"",
            found.len()
        )),
    }
}

/// A holding's opening shares (`openningNumberOfShares`) and price per
/// share, each checked first.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HoldingEdit {
    pub gid: String,
    pub expect_opening: f64,
    pub opening: f64,
    /// A market price to set, with the price MoneyWiz has now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(default)]
    pub expect_price: f64,
}

/// A buy or sell row to add, with its holding (made when missing).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TradeAdd {
    pub account_gid: String,
    pub sell: bool,
    pub date: String,
    pub symbol: String,
    pub shares: f64,
    pub price: f64,
    /// Cash moved: below zero for a buy.
    pub amount: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RowEdit {
    pub entity: String,
    pub gid: String,
    pub expect_amount: f64,
    pub expect_date: String,
    pub amount: Option<f64>,
    pub category: Option<Vec<String>>,
    /// The category's type (1 expense, 2 income) when its path is shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_kind: Option<i64>,
    pub payee: Option<String>,
    pub memo: Option<String>,
    /// A new day (YYYY-MM-DD), the time of day kept: the bank's day for a
    /// row the owner typed a few days off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Remove the row: a cashcast correction folded into the row it fixed,
    /// or a reconcile row and the offset that cancels it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delete: bool,
    /// Move the row to this account (its `GID`): `[moneywiz.merge]`. The
    /// other side of a transfer then points at the new account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// With `delete`, on a coin transfer: the other side's account keeps the
    /// coins moving as a plain buy or sell with no cash, and both sides go.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unlink: bool,
}

#[derive(Serialize, Deserialize)]
pub struct AccountEdit {
    pub gid: String,
    pub expect_opening: f64,
    pub opening: f64,
}

/// Applied in this order: add, rename, then (after the row edits) delete.
#[derive(Serialize, Deserialize)]
pub struct CategoryOp {
    pub op: String,
    pub path: Vec<String>,
    pub to: Option<String>,
    pub move_to: Option<Vec<String>>,
    /// add: a top-level category whose settings (income or expense) to copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub like: Option<Vec<String>>,
}

pub struct Resolved {
    pub plan: WriterPlan,
    /// Problems by edit number. No payees, no amounts.
    pub problems: Vec<String>,
    /// Category adds that already exist.
    pub already: usize,
}

pub fn load(path: &Path) -> Result<EditsFile, String> {
    if !path.exists() {
        return Ok(EditsFile::default());
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {EDITS_FILE}: {e}"))?;
    // The parse error can quote a line of the file, so it is not shown.
    toml::from_str(&text).map_err(|_| {
        format!("{EDITS_FILE} is not valid: each [[row]] needs account, date, amount and a set_ field; each [[category]] one of add, rename (with to) or delete")
    })
}

fn split_path(text: &str) -> Vec<String> {
    text.split('/')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn dollars(cents: i64) -> f64 {
    cents as f64 / 100.0
}

fn cents(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// The plan for the writer: the owner's edits, plus with `fixes` the wrong
/// amounts and opening balances one sync found.
pub fn resolve(store: &Store, file: &EditsFile, fixes: Option<(&[Txn], &Plan)>) -> Resolved {
    let mut problems = Vec::new();
    let mut plan = WriterPlan {
        contract: CONTRACT,
        ..Default::default()
    };
    let mut already = 0;

    // Categories first: rows may use a category added or renamed here.
    let mut paths: BTreeSet<Vec<String>> = store.categories.iter().cloned().collect();
    let mut deletes = Vec::new();
    for (n, spec) in file.category.iter().enumerate() {
        let n = n + 1;
        match (&spec.add, &spec.rename, &spec.delete) {
            (Some(add), None, None) => {
                let path = split_path(add);
                if path.is_empty() {
                    problems.push(format!("category #{n}: empty path"));
                } else if paths.contains(&path) {
                    already += 1;
                } else if path.len() > 1 && !paths.contains(&path[..path.len() - 1]) {
                    problems.push(format!("category #{n}: its parent category does not exist"));
                } else {
                    paths.insert(path.clone());
                    plan.categories.push(CategoryOp {
                        op: "add".into(),
                        path,
                        to: None,
                        move_to: None,
                        like: None,
                    });
                }
            }
            (None, Some(old), None) => {
                let path = split_path(old);
                let to = spec.to.as_deref().unwrap_or("").trim().to_string();
                let mut new = path.clone();
                new.pop();
                new.push(to.clone());
                if !paths.contains(&path) {
                    problems.push(format!("category #{n}: no such category"));
                } else if to.is_empty() || to.contains('/') {
                    problems.push(format!("category #{n}: `to` must be a new name without /"));
                } else if paths.contains(&new) {
                    problems.push(format!(
                        "category #{n}: a category with the new name exists"
                    ));
                } else {
                    let moved: Vec<Vec<String>> = paths
                        .iter()
                        .filter(|p| p.starts_with(&path))
                        .cloned()
                        .collect();
                    for p in moved {
                        paths.remove(&p);
                        let mut q = new.clone();
                        q.extend_from_slice(&p[path.len()..]);
                        paths.insert(q);
                    }
                    plan.categories.push(CategoryOp {
                        op: "rename".into(),
                        path,
                        to: Some(to),
                        move_to: None,
                        like: None,
                    });
                }
            }
            (None, None, Some(del)) => {
                deletes.push((n, split_path(del), spec.move_to.as_deref().map(split_path)))
            }
            _ => problems.push(format!(
                "category #{n}: use exactly one of add, rename or delete"
            )),
        }
    }

    // Rows.
    let mut by_gid: HashMap<String, usize> = HashMap::new();
    for (n, spec) in file.row.iter().enumerate() {
        let n = n + 1;
        match row_edit(store, spec, &paths) {
            Ok(edit) => {
                if by_gid.contains_key(&edit.gid) {
                    problems.push(format!("row #{n}: another edit already changes this row"));
                } else {
                    by_gid.insert(edit.gid.clone(), plan.rows.len());
                    plan.rows.push(edit);
                }
            }
            Err(e) => problems.push(format!("row #{n}: {e}")),
        }
    }

    if let Some((bank, sync)) = fixes {
        fix_edits(store, bank, sync, &mut plan, &mut by_gid);
        fold_edits(store, sync, &mut plan, &mut by_gid);
        date_kept_corrections(store, sync, &mut plan, &mut by_gid);
    }

    // Deletes come last, after rows may have moved out of them.
    for (n, path, move_to) in deletes {
        let in_use = store.txns.iter().filter(|t| t.category == path).count();
        if !paths.contains(&path) {
            problems.push(format!("category #{n}: no such category"));
        } else if paths
            .iter()
            .any(|p| p.len() > path.len() && p.starts_with(&path))
        {
            problems.push(format!(
                "category #{n}: delete or move its subcategories first"
            ));
        } else if move_to
            .as_ref()
            .is_some_and(|m| !paths.contains(m) || *m == path)
        {
            problems.push(format!(
                "category #{n}: move_to must be another existing category"
            ));
        } else if in_use > 0 && move_to.is_none() {
            problems.push(format!("category #{n}: {in_use} rows use it; add move_to"));
        } else {
            paths.remove(&path);
            plan.categories.push(CategoryOp {
                op: "delete".into(),
                path,
                to: None,
                move_to,
                like: None,
            });
        }
    }
    Resolved {
        plan,
        problems,
        already,
    }
}

fn row_edit(
    store: &Store,
    spec: &RowSpec,
    paths: &BTreeSet<Vec<String>>,
) -> Result<RowEdit, String> {
    let account = store
        .accounts
        .iter()
        .find(|a| a.name == spec.account.trim())
        .ok_or_else(|| "no MoneyWiz account with that name".to_string())?;
    let date = NaiveDate::parse_from_str(spec.date.trim(), "%Y-%m-%d")
        .map_err(|_| "date must be YYYY-MM-DD".to_string())?;
    let want = cents(spec.amount);
    let text = spec.text.as_deref().map(str::to_lowercase);
    let found: Vec<&MwTxn> = store
        .txns
        .iter()
        .filter(|t| t.account == account.pk && t.date == date && t.cents == want)
        .filter(|t| {
            text.as_ref()
                .is_none_or(|w| t.text.to_lowercase().contains(w.as_str()))
        })
        .collect();
    let same = found
        .windows(2)
        .all(|w| w[0].text == w[1].text && w[0].memo == w[1].memo);
    let t = match found.as_slice() {
        [t] => *t,
        many if same && spec.nth.is_some_and(|n| (1..=many.len()).contains(&n)) => {
            many[spec.nth.unwrap_or(1) - 1]
        }
        [] => {
            return Err(format!(
                "no MoneyWiz row in {} on that date with that amount",
                account.name
            ));
        }
        _ => {
            return Err(format!(
                "{} MoneyWiz rows match; add text = part of the payee (or nth = 1, 2, … when they are the same)",
                found.len()
            ));
        }
    };
    if t.gid.is_empty() {
        return Err("this MoneyWiz database has no GID for the row".into());
    }
    if spec.delete {
        let changes = spec.set_amount.is_some()
            || spec.set_category.is_some()
            || spec.set_category_path.is_some()
            || spec.set_category_kind.is_some()
            || spec.set_payee.is_some()
            || spec.set_memo.is_some()
            || spec.set_date.is_some();
        if changes {
            return Err("delete = true goes alone: remove the set_* lines".into());
        }
        if t.entity.starts_with("Transfer") {
            return Err("delete a transfer in MoneyWiz itself: its other side would stay".into());
        }
        return Ok(RowEdit {
            entity: t.entity.clone(),
            gid: t.gid.clone(),
            expect_amount: dollars(t.cents),
            expect_date: t.date.to_string(),
            amount: None,
            category: None,
            category_kind: None,
            payee: None,
            memo: None,
            date: None,
            delete: true,
            account: None,
            unlink: false,
        });
    }
    // A category whose own name holds a slash ("Dining/Eating Out") is
    // matched whole first; otherwise each slash starts a subcategory.
    // A name that fits both a slash name and a subcategory is refused.
    let category = match spec.set_category.as_deref() {
        _ if spec.set_category_path.is_some() => spec.set_category_path.clone(),
        None => None,
        Some(text) => {
            let mut fits = paths
                .iter()
                .filter(|p| p.join("/") == text.trim())
                .cloned()
                .collect::<Vec<_>>();
            let split = split_path(text);
            if paths.contains(&split) && !fits.contains(&split) {
                fits.push(split.clone());
            }
            match fits.len() {
                0 | 1 => Some(fits.pop().unwrap_or(split)),
                _ => {
                    return Err(
                        "set_category fits a slash name and a subcategory; rename one".into(),
                    );
                }
            }
        }
    };
    if category.as_ref().is_some_and(|c| !paths.contains(c)) {
        return Err("set_category names no category (add it in a [[category]] block)".into());
    }
    // Two categories may share a path (one income, one expense): the kind
    // names one. A shared path with no kind is refused, as the writer would.
    let want = match spec.set_category_kind.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("expense") => Some(crate::moneywiz_categories::EXPENSE),
        Some("income") => Some(crate::moneywiz_categories::INCOME),
        Some(_) => return Err("set_category_kind must be \"expense\" or \"income\"".into()),
    };
    let kinds = category
        .as_ref()
        .and_then(|c| store.path_kinds.get(c))
        .cloned()
        .unwrap_or_default();
    let category_kind = match (kinds.len(), want) {
        (0 | 1, _) => None,
        (_, None) => {
            return Err(
                "set_category names two categories; add set_category_kind = \"expense\" or \"income\""
                    .into(),
            );
        }
        (_, Some(k)) if kinds.iter().filter(|&&x| x == k).count() == 1 => Some(k),
        (_, Some(_)) => {
            return Err("set_category_kind does not name exactly one category".into());
        }
    };
    if spec.set_amount.is_some() && !is_plain(&t.entity) {
        return Err(
            "only expenses, income and refunds can change amount; transfers have two sides".into(),
        );
    }
    let edit = RowEdit {
        entity: t.entity.clone(),
        gid: t.gid.clone(),
        expect_amount: dollars(t.cents),
        expect_date: t.date.to_string(),
        amount: spec.set_amount.map(|a| dollars(cents(a))),
        category,
        category_kind,
        payee: spec
            .set_payee
            .as_deref()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty()),
        memo: spec.set_memo.clone(),
        date: match spec.set_date.as_deref() {
            Some(d) if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() => {
                return Err("set_date must be YYYY-MM-DD".into());
            }
            d => d.map(str::to_string),
        },
        delete: false,
        account: None,
        unlink: false,
    };
    if edit.amount.is_none()
        && edit.category.is_none()
        && edit.payee.is_none()
        && edit.memo.is_none()
        && edit.date.is_none()
    {
        return Err(
            "nothing to change: add set_amount, set_category, set_payee, set_memo, set_date or delete = true"
                .into(),
        );
    }
    Ok(edit)
}

/// A row whose amount a correction folds into: plain rows and transfers, split
/// or foreign too (owner rule 2026-11-08: MoneyWiz mirrors the bank).
fn foldable(entity: &str) -> bool {
    is_plain(entity) || entity.starts_with("Transfer")
}

fn is_plain(entity: &str) -> bool {
    matches!(
        entity,
        "WithdrawTransaction" | "DepositTransaction" | "RefundTransaction"
    )
}

/// Wrong amounts become amount edits; a balance that still differs from the
/// bank becomes an opening-balance edit (the same cases `adjust` corrects).
fn fix_edits(
    store: &Store,
    bank: &[Txn],
    sync: &Plan,
    plan: &mut WriterPlan,
    by_gid: &mut HashMap<String, usize>,
) {
    let amounts = corrected(store);
    for ap in &sync.accounts {
        let mut diff = 0;
        let mut complete = true;
        for &(pk, i) in &ap.fix {
            let Some(t) = store.txns.iter().find(|t| t.pk == pk) else {
                continue;
            };
            diff += bank[i].cents - amounts[&pk];
            if !crate::moneywiz::can_edit_in_place(t, amounts[&pk]) {
                // A transfer, or a row with an earlier correction: the sync adds a
                // correction row for it instead (`links_for` in "edit" mode).
                complete = false;
                continue;
            }
            let amount = Some(dollars(bank[i].cents));
            match by_gid.get(&t.gid) {
                Some(&j) if plan.rows[j].amount.is_none() => plan.rows[j].amount = amount,
                Some(_) => {}
                None => {
                    by_gid.insert(t.gid.clone(), plan.rows.len());
                    plan.rows.push(RowEdit {
                        entity: t.entity.clone(),
                        gid: t.gid.clone(),
                        expect_amount: dollars(t.cents),
                        expect_date: t.date.to_string(),
                        amount,
                        category: None,
                        category_kind: None,
                        payee: None,
                        memo: None,
                        date: None,
                        delete: false,
                        account: None,
                        unlink: false,
                    });
                }
            }
        }
        for &(pk, i) in &ap.pairs {
            let Some(t) = store.txns.iter().find(|t| t.pk == pk) else {
                continue;
            };
            let dated = is_plain(&t.entity) || t.entity.starts_with("Transfer");
            if bank[i].date == t.date || t.gid.is_empty() || !dated {
                continue;
            }
            let day = Some(bank[i].date.to_string());
            match by_gid.get(&t.gid) {
                Some(&j) => plan.rows[j].date = day,
                None => {
                    by_gid.insert(t.gid.clone(), plan.rows.len());
                    plan.rows.push(RowEdit {
                        entity: t.entity.clone(),
                        gid: t.gid.clone(),
                        expect_amount: dollars(t.cents),
                        expect_date: t.date.to_string(),
                        amount: None,
                        category: None,
                        category_kind: None,
                        payee: None,
                        memo: None,
                        date: day,
                        delete: false,
                        account: None,
                        unlink: false,
                    });
                }
            }
        }
        let Some(b) = ap.bank_balance.as_ref() else {
            continue;
        };
        let account = store.account(ap.mw).expect("linked account exists");
        let settled =
            complete && ap.note.is_none() && ap.add.is_empty() && ap.only_in_moneywiz.is_empty();
        // A stock plan account keeps its opening balance: its start
        // correction sets the cash (owner, 2026-11-16).
        if !settled
            || ap.stock_plan
            || ap.to.is_none()
            || (diff != 0 && b.as_of.is_some_and(|d| Some(d) < ap.to))
            || account.gid.is_empty()
        {
            continue;
        }
        let day = b.as_of.or(ap.to).expect("checked above");
        let gap = b.cents - (store.balance_on(ap.mw, day) + diff);
        if gap != 0 {
            plan.accounts.push(AccountEdit {
                gid: account.gid.clone(),
                expect_opening: dollars(account.opening_cents),
                opening: dollars(account.opening_cents + gap),
            });
        }
    }
}

/// Folds cashcast's own correction rows away in synced accounts: a
/// correction of a plain row becomes that row's amount, a reconcile or
/// stand-in row and the offset that cancels it both go, and a balance
/// correction becomes the account's opening balance. What stays: the
/// corrections of investment rows. Safe to run again.
pub const KEPT_NO_BANK: &str = "the account has no bank balance";
pub const KEPT_NO_GID: &str = "a row has no sync id";
pub const KEPT_GONE: &str = "the row it fixed is gone";
pub const KEPT_OTHER_ACCOUNT: &str = "the row it fixed is in another account";
pub const KEPT_NOT_PLAIN: &str = "the row it fixed is an investment row";
pub const KEPT_STOCK_PLAN: &str = "the account is a stock plan: its corrections stay rows";

/// `cashcast fix #` rows that `fold_edits` leaves in place, with the reason,
/// per MoneyWiz account. Mirrors the skips in `fold_edits`.
pub fn kept_corrections(store: &Store, sync: &Plan) -> Vec<(i64, &'static str)> {
    let synced: HashSet<i64> = sync
        .accounts
        .iter()
        .filter(|ap| ap.note.is_none() && ap.bank_balance.is_some())
        .map(|ap| ap.mw)
        .collect();
    let stock: HashSet<i64> = sync
        .accounts
        .iter()
        .filter(|ap| ap.stock_plan)
        .map(|ap| ap.mw)
        .collect();
    let fix_of = |c: &MwTxn| {
        c.memo
            .strip_prefix(crate::moneywiz::FIX_MEMO)
            .and_then(|r| r.trim().parse::<i64>().ok())
    };
    // The sum of every correction `fold_edits` folds into a row, per row pk.
    let mut fixed: HashMap<i64, i64> = HashMap::new();
    for c in &store.txns {
        if stock.contains(&c.account) || !synced.contains(&c.account) || c.gid.is_empty() {
            continue;
        }
        let Some(pk) = fix_of(c) else {
            continue;
        };
        if store
            .txns
            .iter()
            .any(|t| t.pk == pk && t.account == c.account)
        {
            *fixed.entry(pk).or_default() += c.cents;
        }
    }
    let mut out = Vec::new();
    for c in &store.txns {
        let Some(pk) = fix_of(c) else {
            continue;
        };
        let t = store.txns.iter().find(|t| t.pk == pk);
        let why = if stock.contains(&c.account) {
            KEPT_STOCK_PLAN
        } else if !synced.contains(&c.account) {
            KEPT_NO_BANK
        } else if c.gid.is_empty() {
            KEPT_NO_GID
        } else if let Some(t) = t {
            if t.account != c.account {
                KEPT_OTHER_ACCOUNT
            } else if t.gid.is_empty() {
                KEPT_NO_GID
            } else if t.cents + fixed.get(&pk).copied().unwrap_or(0) == 0 {
                continue;
            } else if !foldable(&t.entity) {
                KEPT_NOT_PLAIN
            } else {
                continue;
            }
        } else {
            KEPT_GONE
        };
        out.push((c.account, why));
    }
    out
}

/// A correction that stays a row (a stock plan's, or one of an investment
/// row) moves to the day of the row it fixes, so each day's balance is the
/// bank's. Rows other edits already change are left to them.
fn date_kept_corrections(
    store: &Store,
    sync: &Plan,
    plan: &mut WriterPlan,
    by_gid: &mut HashMap<String, usize>,
) {
    for ap in sync
        .accounts
        .iter()
        .filter(|ap| ap.note.is_none() && ap.bank_balance.is_some())
    {
        let Some(from) = ap.from else { continue };
        for c in store.txns.iter().filter(|c| c.account == ap.mw) {
            let Some(pk) = c
                .memo
                .strip_prefix(crate::moneywiz::FIX_MEMO)
                .and_then(|r| r.trim().parse::<i64>().ok())
            else {
                continue;
            };
            let Some(t) = store
                .txns
                .iter()
                .find(|t| t.pk == pk && t.account == c.account)
            else {
                continue;
            };
            if c.gid.is_empty()
                || by_gid.contains_key(&c.gid)
                || !is_plain(&c.entity)
                || c.date == t.date
                || c.date.min(t.date) < from
            {
                continue;
            }
            by_gid.insert(c.gid.clone(), plan.rows.len());
            plan.rows.push(RowEdit {
                entity: c.entity.clone(),
                gid: c.gid.clone(),
                expect_amount: dollars(c.cents),
                expect_date: c.date.to_string(),
                amount: None,
                category: None,
                category_kind: None,
                payee: None,
                memo: None,
                date: Some(t.date.to_string()),
                delete: false,
                account: None,
                unlink: false,
            });
        }
    }
}

fn fold_edits(
    store: &Store,
    sync: &Plan,
    plan: &mut WriterPlan,
    by_gid: &mut HashMap<String, usize>,
) {
    // A stock plan account's corrections stay: nothing of it is deleted.
    let synced: HashSet<i64> = sync
        .accounts
        .iter()
        .filter(|ap| ap.note.is_none() && ap.bank_balance.is_some() && !ap.stock_plan)
        .map(|ap| ap.mw)
        .collect();
    let row = |t: &MwTxn| RowEdit {
        entity: t.entity.clone(),
        gid: t.gid.clone(),
        expect_amount: dollars(t.cents),
        expect_date: t.date.to_string(),
        amount: None,
        category: None,
        category_kind: None,
        payee: None,
        memo: None,
        date: None,
        delete: true,
        account: None,
        unlink: false,
    };
    let mut opening: HashMap<i64, i64> = HashMap::new();
    // Every correction of a row, grouped by the row it fixes, so a row with
    // several corrections folds them all in one run.
    let mut fixes: Vec<(i64, Vec<&MwTxn>)> = Vec::new();
    for c in &store.txns {
        if !synced.contains(&c.account) || c.gid.is_empty() || by_gid.contains_key(&c.gid) {
            continue;
        }
        if c.memo.starts_with(crate::moneywiz::BALANCE_MEMO) {
            if plan
                .accounts
                .iter()
                .any(|a| Some(&a.gid) == store.account(c.account).map(|x| &x.gid))
            {
                continue;
            }
            *opening.entry(c.account).or_default() += c.cents;
            by_gid.insert(c.gid.clone(), plan.rows.len());
            plan.rows.push(row(c));
            continue;
        }
        let Some(pk) = c
            .memo
            .strip_prefix(crate::moneywiz::FIX_MEMO)
            .and_then(|r| r.trim().parse::<i64>().ok())
        else {
            continue;
        };
        match fixes.iter_mut().find(|(p, _)| *p == pk) {
            Some((_, cs)) => cs.push(c),
            None => fixes.push((pk, vec![c])),
        }
    }
    for (pk, cs) in fixes {
        let Some(t) = store.txns.iter().find(|t| t.pk == pk) else {
            continue;
        };
        let cs: Vec<&MwTxn> = cs.into_iter().filter(|c| t.account == c.account).collect();
        if cs.is_empty() || t.gid.is_empty() || by_gid.contains_key(&t.gid) {
            continue;
        }
        let cents = t.cents + cs.iter().map(|c| c.cents).sum::<i64>();
        if cents == 0 {
            // A stand-in and its offsets: all go, the balance stays.
            for x in std::iter::once(t).chain(cs) {
                by_gid.insert(x.gid.clone(), plan.rows.len());
                plan.rows.push(row(x));
            }
        } else if foldable(&t.entity) {
            let mut e = row(t);
            e.delete = false;
            e.amount = Some(dollars(cents));
            by_gid.insert(t.gid.clone(), plan.rows.len());
            plan.rows.push(e);
            for c in cs {
                by_gid.insert(c.gid.clone(), plan.rows.len());
                plan.rows.push(row(c));
            }
        }
    }
    for (account, cents) in opening {
        let a = store.account(account).expect("synced account exists");
        if a.gid.is_empty() {
            continue;
        }
        plan.accounts.push(AccountEdit {
            gid: a.gid.clone(),
            expect_opening: dollars(a.opening_cents),
            opening: dollars(a.opening_cents + cents),
        });
    }
}

/// `[moneywiz.merge]`: each source account's rows move into the target
/// account. Only sources in the target's currency count (two accounts can
/// share a name). When the target shows a broker file (`investments`), the
/// source's investment rows are deleted instead: the broker's trades replace
/// them. Rows another edit already changes, and rows with no `GID`, stay.
/// Returns the edits and the problems (account names only).
/// `[moneywiz.retire]`: every row of each named account (in that currency)
/// goes. A transfer goes with its other side, and that account keeps a plain
/// row of the same day and amount (`unlink`).
pub fn retire_edits(
    store: &Store,
    retire: &std::collections::BTreeMap<String, String>,
    taken: &std::collections::HashSet<String>,
) -> Vec<RowEdit> {
    let mut edits = Vec::new();
    for (name, currency) in retire {
        for src in store
            .accounts
            .iter()
            .filter(|a| a.name == name.trim() && a.currency.eq_ignore_ascii_case(currency.trim()))
        {
            for t in store.txns.iter().filter(|t| t.account == src.pk) {
                if t.gid.is_empty() || taken.contains(&t.gid) {
                    continue;
                }
                let unlink = t.entity.starts_with("Transfer")
                    && t.other_account.is_some_and(|o| o != src.pk);
                edits.push(RowEdit {
                    entity: t.entity.clone(),
                    gid: t.gid.clone(),
                    expect_amount: dollars(t.cents),
                    expect_date: t.date.to_string(),
                    amount: None,
                    category: None,
                    category_kind: None,
                    payee: None,
                    memo: None,
                    date: None,
                    delete: true,
                    account: None,
                    unlink,
                });
            }
        }
    }
    edits
}

/// `[moneywiz.balances]`: each named account ("AMOUNT@YYYY-MM-DD") gets an
/// opening balance that makes MoneyWiz show that amount at the end of that
/// day, after the row deletes and trade adds already in `plan`. Returns the
/// problems (account names only).
pub fn balance_edits(
    store: &Store,
    balances: &std::collections::BTreeMap<String, String>,
    plan: &mut WriterPlan,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, value) in balances {
        let Some((amount, day)) = crate::moneywiz::read_balance(value) else {
            problems.push(format!(
                "balances \"{name}\": write \"AMOUNT@YYYY-MM-DD\", such as \"0.00@2026-11-08\""
            ));
            continue;
        };
        let found: Vec<_> = store
            .accounts
            .iter()
            .filter(|a| a.name == name.trim())
            .collect();
        let [account] = found[..] else {
            problems.push(format!(
                "balances \"{name}\": {} MoneyWiz accounts have that name; need exactly one",
                found.len()
            ));
            continue;
        };
        if account.gid.is_empty() {
            continue;
        }
        if plan.accounts.iter().any(|e| e.gid == account.gid) {
            problems.push(format!(
                "balances \"{name}\": skipped, another fix already sets this account's opening balance"
            ));
            continue;
        }
        // What the planned row edits (deletes, new amounts, days or
        // accounts) change in this account's balance on `day`.
        let on_day = day.to_string();
        let changed: i64 = plan
            .rows
            .iter()
            .filter_map(|e| store.txns.iter().find(|t| t.gid == e.gid).map(|t| (e, t)))
            .map(|(e, t)| {
                let before = if t.account == account.pk && t.date <= day {
                    t.cents
                } else {
                    0
                };
                let here = match &e.account {
                    Some(gid) => *gid == account.gid,
                    None => t.account == account.pk,
                };
                let dated = e
                    .date
                    .as_deref()
                    .map_or(t.date <= day, |d| d <= on_day.as_str());
                let cents = e.amount.map_or(t.cents, |a| (a * 100.0).round() as i64);
                let after = if !e.delete && here && dated { cents } else { 0 };
                after - before
            })
            .sum();
        let added: i64 = plan
            .trades
            .iter()
            .filter(|t| t.account_gid == account.gid && t.date.as_str() <= day.to_string().as_str())
            .map(|t| (t.amount * 100.0).round() as i64)
            .sum();
        let gap = amount - (store.balance_on(account.pk, day) + changed + added);
        if gap != 0 {
            plan.accounts.push(AccountEdit {
                gid: account.gid.clone(),
                expect_opening: dollars(account.opening_cents),
                opening: dollars(account.opening_cents + gap),
            });
        }
    }
    problems
}

pub fn merge_edits(
    store: &Store,
    merge: &std::collections::BTreeMap<String, String>,
    investments: &std::collections::BTreeMap<String, String>,
    taken: &HashSet<String>,
) -> (Vec<RowEdit>, Vec<String>) {
    let mut edits = Vec::new();
    let mut problems = Vec::new();
    for (from, to) in merge {
        let targets: Vec<_> = store
            .accounts
            .iter()
            .filter(|a| a.name == to.trim())
            .collect();
        let [target] = targets[..] else {
            problems.push(format!(
                "merge \"{from}\": {} MoneyWiz accounts are named \"{to}\"; need exactly one",
                targets.len()
            ));
            continue;
        };
        if target.gid.is_empty() {
            problems.push(format!("merge \"{from}\": \"{to}\" has no sync id"));
            continue;
        }
        let sources: Vec<_> = store
            .accounts
            .iter()
            .filter(|a| a.name == from.trim() && a.currency == target.currency && a.pk != target.pk)
            .collect();
        if sources.is_empty() {
            problems.push(format!(
                "merge \"{from}\": no account by that name in {}",
                target.currency
            ));
            continue;
        }
        let broker = investments.contains_key(to.trim());
        for src in sources {
            for t in store.txns.iter().filter(|t| t.account == src.pk) {
                if t.gid.is_empty() || taken.contains(&t.gid) {
                    continue;
                }
                // A forex account keeps its own currency on a holding too: only
                // a holding in another symbol (a coin, a share) is a trade.
                let on_asset = match t.holding.as_deref() {
                    None => false,
                    Some("") => {
                        problems.push(format!(
                            "merge \"{from}\": a row is on a holding with no symbol"
                        ));
                        continue;
                    }
                    Some(sym) => !sym.eq_ignore_ascii_case(&target.currency),
                };
                let investment = t.entity.starts_with("Investment") || on_asset;
                if investment && !broker {
                    problems.push(format!(
                        "merge \"{from}\": an investment row cannot move; map \"{to}\" in [moneywiz.investments]"
                    ));
                    continue;
                }
                if investment
                    && t.entity.starts_with("Transfer")
                    && t.other_account.is_none_or(|o| o == src.pk)
                {
                    problems.push(format!(
                        "merge \"{from}\": a coin transfer with no other account stays; fix it in MoneyWiz"
                    ));
                    continue;
                }
                // A coin transfer goes with its other side; that account keeps
                // its coins moving as a plain row (owner, 2026-11-08: the
                // broker file already has this side's deposit or withdrawal).
                edits.push(RowEdit {
                    entity: t.entity.clone(),
                    gid: t.gid.clone(),
                    expect_amount: dollars(t.cents),
                    expect_date: t.date.to_string(),
                    amount: None,
                    category: None,
                    category_kind: None,
                    payee: None,
                    memo: None,
                    date: None,
                    delete: investment,
                    account: (!investment).then(|| target.gid.clone()),
                    unlink: investment && t.entity.starts_with("Transfer"),
                });
            }
        }
    }
    problems.dedup();
    (edits, problems)
}

pub fn render(r: &Resolved, plan_path: &str) -> String {
    let p = &r.plan;
    let count = |f: &dyn Fn(&RowEdit) -> bool| p.rows.iter().filter(|e| f(e)).count();
    let ops = |op: &str| p.categories.iter().filter(|c| c.op == op).count();
    let mut out = String::from("# MoneyWiz edits\n\n");
    out.push_str(&format!(
        "{} row changes: {} amounts, {} categories, {} payees, {} memos, {} dates, {} deletes{}\n",
        p.rows.len(),
        count(&|e| e.amount.is_some()),
        count(&|e| e.category.is_some()),
        count(&|e| e.payee.is_some()),
        count(&|e| e.memo.is_some()),
        count(&|e| e.date.is_some()),
        count(&|e| e.delete),
        match count(&|e| e.account.is_some()) {
            0 => String::new(),
            n => format!(", {n} moves to another account"),
        },
    ));
    out.push_str(&format!("{} opening balances\n", p.accounts.len()));
    for a in &p.archive {
        out.push_str(if a.archived {
            "1 account to archive\n"
        } else {
            "1 archived account to show again\n"
        });
    }
    if !p.trades.is_empty() {
        out.push_str(&format!("{} investment trades to add\n", p.trades.len()));
    }
    if !p.holdings.is_empty() {
        let zero = p
            .holdings
            .iter()
            .filter(|h| h.opening != h.expect_opening)
            .count();
        let priced = p.holdings.iter().filter(|h| h.price.is_some()).count();
        out.push_str(&format!(
            "{} holdings: {zero} opening shares set to 0, {priced} priced at the market\n",
            p.holdings.len()
        ));
    }
    out.push_str(&format!(
        "{} category changes: {} add, {} rename, {} delete{}\n",
        p.categories.len(),
        ops("add"),
        ops("rename"),
        ops("delete"),
        if r.already > 0 {
            format!(" ({} already there)", r.already)
        } else {
            String::new()
        }
    ));
    if !r.problems.is_empty() {
        out.push_str(&format!(
            "\n{} problems; those edits are left out:\n",
            r.problems.len()
        ));
        for pr in &r.problems {
            out.push_str(&format!("- {pr}\n"));
        }
    }
    if is_empty(p) {
        out.push_str("\nNothing to change.\n");
    } else {
        out.push_str(&format!("\nPlan saved for the writer: {plan_path}\n"));
    }
    out
}

pub fn is_empty(p: &WriterPlan) -> bool {
    p.rows.is_empty()
        && p.accounts.is_empty()
        && p.categories.is_empty()
        && p.trades.is_empty()
        && p.holdings.is_empty()
        && p.archive.is_empty()
}

/// Checks that a store holds every change in the plan. Returns the report
/// and how many changes are missing.
pub fn verify(store: &Store, plan: &WriterPlan) -> (String, usize) {
    let mut missing = Vec::new();
    for (n, e) in plan.rows.iter().enumerate() {
        let n = n + 1;
        let found = store.txns.iter().find(|t| t.gid == e.gid);
        if e.delete {
            if found.is_some() {
                missing.push(format!("change #{n}: the row is still there"));
            }
            continue;
        }
        let Some(t) = found else {
            missing.push(format!("change #{n}: the row is gone"));
            continue;
        };
        let moved = e.account.as_ref().is_none_or(|g| {
            store
                .accounts
                .iter()
                .any(|a| a.gid == *g && a.pk == t.account)
        });
        let ok = moved
            && e.amount.is_none_or(|a| t.cents == cents(a))
            && e.category.as_ref().is_none_or(|c| t.category == *c)
            && e.memo.as_ref().is_none_or(|m| t.memo == *m)
            && e.date.as_ref().is_none_or(|d| t.date.to_string() == *d)
            && e.payee
                .as_ref()
                .is_none_or(|p| t.text.to_lowercase().contains(&p.to_lowercase()));
        if !ok {
            missing.push(format!("change #{n}: the row does not show the change"));
        }
    }
    for (n, tr) in plan.trades.iter().enumerate() {
        let Some(acc) = store.accounts.iter().find(|a| a.gid == tr.account_gid) else {
            missing.push(format!("trade #{}: the account is gone", n + 1));
            continue;
        };
        let here = store.txns.iter().any(|t| {
            t.account == acc.pk
                && t.date.to_string() == tr.date
                && store.trades.get(&t.pk).is_some_and(|x| {
                    crate::moneywiz_invest::norm(&x.symbol)
                        == crate::moneywiz_invest::norm(&tr.symbol)
                        && (x.shares.abs() - tr.shares.abs()).abs() < 1e-4
                })
        });
        if !here {
            missing.push(format!("trade #{}: not in the account", n + 1));
        }
    }
    for (n, a) in plan.accounts.iter().enumerate() {
        let ok = store
            .accounts
            .iter()
            .any(|x| x.gid == a.gid && x.opening_cents == cents(a.opening));
        if !ok {
            missing.push(format!("opening balance #{}: not changed", n + 1));
        }
    }
    for (n, a) in plan.archive.iter().enumerate() {
        let ok = store
            .accounts
            .iter()
            .any(|x| x.gid == a.gid && x.archived == a.archived);
        if !ok {
            missing.push(format!("archive #{}: not changed", n + 1));
        }
    }
    for (n, h) in plan.holdings.iter().enumerate() {
        let now = store.holdings.iter().find(|o| o.gid == h.gid);
        let left = now.map_or(0.0, |o| o.shares);
        let priced = h
            .price
            .is_none_or(|p| now.is_some_and(|o| (o.price - p).abs() < 1e-6));
        if (left - h.opening).abs() > 1e-9 || !priced {
            missing.push(format!("holding #{}: not changed", n + 1));
        }
    }
    // A migration's category steps are checked as one tree (`expect_tree`):
    // a later step can rename what an earlier one added.
    let steps = if plan.expect_tree.is_some() {
        &[][..]
    } else {
        &plan.categories[..]
    };
    for (n, c) in steps.iter().enumerate() {
        let has = |p: &Vec<String>| store.categories.contains(p);
        let ok = match c.op.as_str() {
            "add" => has(&c.path),
            "rename" => {
                let mut new = c.path.clone();
                new.pop();
                new.push(c.to.clone().unwrap_or_default());
                has(&new) && !has(&c.path)
            }
            _ => !has(&c.path),
        };
        if !ok {
            missing.push(format!("category change #{}: not done", n + 1));
        }
    }
    if let Some(want) = &plan.expect_tree
        && crate::moneywiz_categories::fingerprint(store) != *want
    {
        missing.push("the category tree is not the one the migration expects".into());
    }
    let total = plan.rows.len()
        + plan.trades.len()
        + plan.holdings.len()
        + plan.accounts.len()
        + plan.archive.len()
        + plan.categories.len()
        + usize::from(plan.expect_tree.is_some());
    let mut out = format!(
        "Verified {} of {total} changes in {}.\n",
        total.saturating_sub(missing.len()),
        store.file_name
    );
    for m in &missing {
        out.push_str(&format!("- {m}\n"));
    }
    (out, missing.len())
}
