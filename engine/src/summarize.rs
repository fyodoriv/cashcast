//! Monthly aggregates, pay detection, balances and data checks.
//!
//! Works on the merged inbox. Output holds names, counts, dates and sums
//! only; payees appear as numbered groups, never as text.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;

use crate::config::Config;
use crate::detect::{self, Class, PayCandidate, Schedule, Suggestion};
use crate::export::{AccountKind, Balance, CATEGORY_SEPARATORS, Txn};
use crate::inbox::Inbox;
use crate::money::fmt_cents;
use crate::text::Hint;

#[derive(Serialize)]
pub struct Summary {
    pub generated_on: String,
    pub base_currency: String,
    pub months: Vec<String>,
    pub files: Vec<FileSummary>,
    pub accounts: Vec<AccountSummary>,
    pub pay: PaySummary,
    /// Income by type, cents per month. Positive means money in.
    pub income_cents: BTreeMap<String, Vec<i64>>,
    /// Income by source, so recurring items can be picked.
    pub income_by_category_cents: BTreeMap<String, Vec<i64>>,
    /// Spending by category, cents per month. Positive means spent.
    pub spending_cents: BTreeMap<String, Vec<i64>>,
    /// salary, salary_off_cycle, income_tax (signed), invested,
    /// uncategorized_in, transfer_unknown (signed), card_spend, card_payments.
    pub flows_cents: BTreeMap<String, Vec<i64>>,
    pub suggested_extra_income: Vec<Suggestion>,
    /// Payees that charge on a schedule, as numbered groups.
    pub recurring_payments: Vec<RecurringPayment>,
    /// Categories classified automatically, and the role chosen.
    pub category_roles: BTreeMap<String, String>,
    pub checks: Vec<Check>,
    /// Spending rows in the window, for `cards` cap counting. Never written to JSON.
    #[serde(skip)]
    pub charges: Vec<SpendRow>,
}

/// One spending row in the base currency: positive is spent, negative a refund.
/// Holds the account, the full category path and the date, never payee text.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendRow {
    pub account: String,
    pub date: NaiveDate,
    pub category: String,
    pub cents: i64,
    /// True when the row was in another currency than the base.
    pub foreign: bool,
}

#[derive(Serialize)]
pub struct FileSummary {
    pub name: String,
    pub format: String,
    pub transactions: usize,
    pub errors: usize,
    pub date_min: Option<String>,
    pub date_max: Option<String>,
    pub notes: Vec<String>,
    pub accounts: Vec<String>,
    /// Set when the file could not be read at all.
    pub error: Option<String>,
    /// True for an investment or crypto ledger, kept out of the cash import.
    pub ledger: bool,
}

#[derive(Serialize)]
pub struct AccountSummary {
    pub name: String,
    pub class: Class,
    /// True when the class was guessed, not set in the config.
    pub guessed: bool,
    pub class_reason: String,
    /// The account's most common currency. Balances are in it.
    pub currency: String,
    pub rows: usize,
    pub last_date: Option<String>,
    /// Best known current balance. Not for excluded accounts.
    pub balance_cents: Option<i64>,
    pub balance_source: Option<String>,
    /// Sum of all rows up to today. Right only if the account started at zero.
    pub sum_balance_cents: Option<i64>,
}

#[derive(Serialize)]
pub struct PaySummary {
    pub schedule: Option<String>,
    /// Where pay was found: a config rule, a category, or a payee group.
    pub source: String,
    pub anchor: Option<String>,
    pub cycles: Vec<Cycle>,
    pub half_years: Vec<HalfYear>,
    pub gap_days: BTreeMap<i64, usize>,
    pub off_cycle_deposits: usize,
    pub deposit_accounts: BTreeMap<String, usize>,
    pub cycles_per_month: Vec<usize>,
}

/// One pay cycle. Dates only; amounts appear only as medians and monthly sums.
#[derive(Serialize)]
pub struct Cycle {
    pub nominal: String,
    pub paid_on: String,
    pub weekday: String,
    pub offset_days: i64,
    pub deposits: usize,
}

#[derive(Serialize)]
pub struct HalfYear {
    pub period: String,
    pub cheques: usize,
    pub median_cents: i64,
    /// Cheques more than 5% away from the median.
    pub off_median: usize,
}

/// One payee that charges on a schedule. Never the payee text.
#[derive(Serialize)]
pub struct RecurringPayment {
    /// "Payments #n", numbered by yearly cost, largest first.
    pub label: String,
    /// weekly, monthly, quarterly or yearly.
    pub cadence: String,
    /// The spending category of the latest charge.
    pub category: String,
    pub median_cents: i64,
    pub count: usize,
    pub last_seen: String,
    pub yearly_cents: i64,
    /// Links the label to the owner's private review file. Not written out.
    #[serde(skip)]
    pub payee_key: u64,
}

#[derive(Serialize)]
pub struct Check {
    pub name: String,
    pub count: usize,
    pub detail: String,
}

#[derive(Clone)]
enum Rule {
    Salary,
    Income(String),
    IncomeTax,
    Internal,
    Invested,
    Spending,
}

enum Kind {
    Salary,
    Income { kind: String, source: String },
    Spend(String),
    IncomeTax,
    Invested,
    Internal { counterpart: Option<String> },
    UnknownTransfer,
    UncategorizedIn,
}

const UNCATEGORIZED: &str = "(Uncategorized)";
const SALARY_OTHER: &str = "salary_other_accounts";
const INTERNAL_CATEGORY: &[&str] = &[
    "transfer",
    "transfers",
    "credit card payment",
    "card payment",
    "internal transfer",
];
const TAX_CATEGORY: &[&str] = &["tax", "taxes", "income tax", "income taxes"];
const INCOME_CATEGORY: &[&str] = &[
    "income",
    "salary",
    "paycheck",
    "paycheque",
    "payroll",
    "wages",
    "bonus",
    "dividend",
    "dividends",
    "refund",
    "refunds",
    "cashback",
    "cash back",
    "rewards",
    "reimbursement",
    "ready to assign",
    "to be budgeted",
];
const SALARY_WORDS: &[&str] = &["salary", "paycheck", "paycheque", "payroll", "wages"];

fn has_words(text: &str, words: &[&str]) -> bool {
    let norm: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let norm = format!(
        " {} ",
        norm.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    words.iter().any(|w| norm.contains(&format!(" {w} ")))
}

/// True when the whole name (not just a word in it) is one of `names`.
fn is_named(text: &str, names: &[&str]) -> bool {
    let norm: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let norm = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    names.contains(&norm.as_str())
}

fn income_type_for(path: &str) -> String {
    if has_words(path, &["tax", "taxes", "gst", "hst"]) {
        "tax_refund"
    } else if has_words(
        path,
        &[
            "cashback",
            "cash back",
            "rewards",
            "interest",
            "dividend",
            "dividends",
            "bank",
        ],
    ) {
        "cashback"
    } else if has_words(path, &["gift", "gifts"]) {
        "gift"
    } else {
        "other"
    }
    .to_string()
}

struct Ctx<'a> {
    cfg: &'a Config,
    classes: &'a BTreeMap<String, Class>,
    rules: &'a [(Vec<String>, Rule)],
    roles: &'a HashMap<Vec<String>, (Vec<String>, Rule)>,
    separate: &'a [Vec<String>],
}

pub fn summarize(inbox: &Inbox, cfg: &Config, today: NaiveDate) -> Result<Summary, String> {
    let txns = &inbox.txns;
    let separator = inbox
        .files
        .iter()
        .map(|f| f.category_separator.as_str())
        .find(|s| !s.is_empty())
        .unwrap_or("");
    let base = base_currency(cfg, txns);
    let account_currency = account_currencies(txns);
    let mut kinds: BTreeMap<String, AccountKind> = BTreeMap::new();
    for f in &inbox.files {
        kinds.extend(f.account_kinds.iter().map(|(k, v)| (k.clone(), *v)));
    }
    let balances = merge_balances(inbox);

    // Classes: config first, then guesses.
    let mut names: BTreeSet<&str> = txns.iter().map(|t| t.account.as_str()).collect();
    names.extend(
        txns.iter()
            .filter(|t| !t.transfer.is_empty())
            .map(|t| t.transfer.as_str()),
    );
    names.extend(balances.keys().map(String::as_str));
    let mut classes: BTreeMap<String, Class> = BTreeMap::new();
    let mut guessed: BTreeMap<String, &'static str> = BTreeMap::new();
    for name in names {
        // An exact key wins; else the longest key ending in `*` that prefixes the name.
        let configured = cfg.accounts.get(name).or_else(|| {
            cfg.accounts
                .iter()
                .filter_map(|(k, v)| Some((k.strip_suffix('*')?, v)))
                .filter(|(prefix, _)| name.starts_with(prefix))
                .max_by_key(|(prefix, _)| prefix.len())
                .map(|(_, v)| v)
        });
        match configured {
            Some(text) => {
                let class = Class::parse(text).ok_or_else(|| {
                    format!("accounts.{name:?} = {text:?}: use cash, card, other or excluded")
                })?;
                classes.insert(name.to_string(), class);
            }
            None => {
                let currency = account_currency.get(name).map_or("", String::as_str);
                let (class, reason) =
                    detect::guess_class(name, kinds.get(name).copied(), currency, &base);
                classes.insert(name.to_string(), class);
                guessed.insert(name.to_string(), reason);
            }
        }
    }

    let missing: BTreeSet<&str> = txns
        .iter()
        .filter(|t| classes[t.account.as_str()] != Class::Excluded)
        .map(|t| t.currency.as_str())
        .filter(|c| !c.is_empty() && *c != base && !cfg.currencies.rates.contains_key(*c))
        .collect();
    if !missing.is_empty() {
        let list: Vec<&str> = missing.into_iter().collect();
        return Err(format!(
            "no rate for {} in [currencies] rates (base currency {base:?}); add rates such as rates = {{ {} = 1.0 }}",
            list.join(", "),
            list[0]
        ));
    }
    let to_base = |t: &Txn| match cfg.currencies.rates.get(&t.currency) {
        Some(rate) if t.currency != base && !t.currency.is_empty() => {
            (t.cents as f64 * rate).round() as i64
        }
        _ => t.cents,
    };

    let rules = build_rules(cfg, separator);
    let roles = auto_roles(txns, &classes, &rules);
    let separate: Vec<Vec<String>> = cfg
        .categories
        .separate
        .iter()
        .map(|p| split_rule_path(p, separator))
        .collect();
    cfg.rent.check()?;
    let (first, last) = window(cfg, today, covered_until(txns, &classes, today))?;
    let months = month_list(first, last);
    let n = months.len();
    let month_index = |d: NaiveDate| {
        let i = (d.year() - first.year()) * 12 + d.month() as i32 - first.month() as i32;
        (0..n as i32).contains(&i).then_some(i as usize)
    };

    let deposit_labels = deposit_labels(txns);
    let ctx = Ctx {
        cfg,
        classes: &classes,
        rules: &rules,
        roles: &roles,
        separate: &separate,
    };

    // Own-account transfers in files that do not record them.
    let eligible: Vec<bool> = txns
        .iter()
        .map(|t| {
            t.transfer.is_empty()
                && (!t.category_from_file || matches!(lookup_role(&ctx, t), Some(Rule::Internal)))
        })
        .collect();
    let partners = detect::match_transfers(txns, &eligible);
    let mut kinds_of: Vec<Kind> = txns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            classify(
                t,
                classes[t.account.as_str()],
                partners[i].map(|p| &txns[p]),
                &ctx,
            )
        })
        .collect();

    // Pay: a config rule, else the most regular large deposit group.
    let explicit_salary = !cfg.categories.salary.is_empty();
    let mut pay_source = if explicit_salary {
        "config rule".to_string()
    } else {
        "not found".to_string()
    };
    if !explicit_salary {
        let latest = txns.iter().map(|t| t.date).filter(|d| *d <= today).max();
        let mut groups: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();
        for (i, t) in txns.iter().enumerate() {
            let candidate = matches!(kinds_of[i], Kind::Income { .. } | Kind::UncategorizedIn);
            if !candidate || t.cents <= 0 || classes[t.account.as_str()] != Class::Cash {
                continue;
            }
            let (key, label) = deposit_group(t, &deposit_labels);
            groups
                .entry(key)
                .or_insert_with(|| (label, Vec::new()))
                .1
                .push(i);
        }
        let keys: Vec<&String> = groups.keys().collect();
        let candidates: Vec<PayCandidate> = keys
            .iter()
            .map(|k| {
                let (label, rows) = &groups[*k];
                let recent: Vec<usize> = rows
                    .iter()
                    .copied()
                    .filter(|&i| {
                        latest.is_some_and(|l| {
                            (l - txns[i].date).num_days() <= 365 && txns[i].date <= l
                        })
                    })
                    .collect();
                PayCandidate {
                    label: label.clone(),
                    dates: recent.iter().map(|&i| txns[i].date).collect(),
                    cents: recent.iter().map(|&i| to_base(&txns[i])).collect(),
                    payroll_hint: rows.iter().any(|&i| txns[i].hint == Hint::Payroll)
                        || has_words(label, SALARY_WORDS),
                }
            })
            .collect();
        if let Some((best, _)) = detect::pick_pay(&candidates) {
            pay_source = candidates[best].label.clone();
            for &i in &groups[keys[best]].1 {
                kinds_of[i] = Kind::Salary;
            }
        }
    }

    let mut income: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut income_by_category: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut spending: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut flows: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for key in [
        "salary",
        "salary_off_cycle",
        "income_tax",
        "invested",
        "uncategorized_in",
        "transfer_unknown",
        "card_spend",
        "card_payments",
    ] {
        flows.insert(key.to_string(), vec![0; n]);
    }
    let add = |map: &mut BTreeMap<String, Vec<i64>>, key: &str, i: usize, cents: i64| {
        map.entry(key.to_string()).or_insert_with(|| vec![0; n])[i] += cents;
    };

    let mut sums: HashMap<&str, i64> = HashMap::new();
    let mut rows: HashMap<&str, usize> = HashMap::new();
    let mut last_dates: HashMap<&str, NaiveDate> = HashMap::new();
    let mut currencies: BTreeMap<&str, usize> = BTreeMap::new();
    let mut pair_net: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut fingerprints: HashMap<u64, (usize, NaiveDate)> = HashMap::new();
    let mut deposits: Vec<(&Txn, i64)> = Vec::new();
    let mut charges: Vec<Charge> = Vec::new();
    let mut spend_rows: Vec<SpendRow> = Vec::new();
    let mut data_until: HashMap<&str, NaiveDate> = HashMap::new();
    let (
        mut future,
        mut before,
        mut uncategorized,
        mut unknown_transfers,
        mut salary_outside_cash,
        mut matched,
    ) = (0, 0, 0, 0, 0, 0);

    for (idx, t) in txns.iter().enumerate() {
        let class = classes[t.account.as_str()];
        *rows.entry(&t.account).or_insert(0) += 1;
        let seen = last_dates.entry(&t.account).or_insert(t.date);
        *seen = (*seen).max(t.date);
        if class == Class::Excluded {
            continue;
        }
        if t.date > today {
            future += 1;
            continue;
        }
        let until = data_until.entry(&t.account).or_insert(t.date);
        *until = (*until).max(t.date);
        *sums.entry(&t.account).or_insert(0) += t.cents;
        let currency = if t.currency.is_empty() {
            "(none)"
        } else {
            t.currency.as_str()
        };
        *currencies.entry(currency).or_insert(0) += 1;
        let cents = to_base(t);
        if partners[idx].is_some() {
            matched += 1;
        }

        let kind = &kinds_of[idx];
        let in_base =
            |name: &str| account_currency.get(name).map(String::as_str) == Some(base.as_str());
        if let Kind::Internal {
            counterpart: Some(other),
        } = kind
            && !t.transfer.is_empty()
            && in_base(&t.account)
            && in_base(other)
        {
            let key = if t.account < *other {
                (t.account.clone(), other.clone())
            } else {
                (other.clone(), t.account.clone())
            };
            *pair_net.entry(key).or_insert(0) += t.cents;
        }
        if let Kind::Spend(category) = kind
            && cents < 0
            && t.payee_key != 0
            && t.date >= first
        {
            charges.push(Charge {
                payee_key: t.payee_key,
                account: &t.account,
                date: t.date,
                cents: -cents,
                category: category.clone(),
            });
        }
        // Cap counting needs this month too, which the monthly window leaves out.
        if matches!(kind, Kind::Spend(_)) && t.date >= first {
            spend_rows.push(SpendRow {
                account: t.account.clone(),
                date: t.date,
                category: t.category.join(" > "),
                cents: -cents,
                foreign: !t.currency.is_empty() && t.currency != base,
            });
        }
        let path = kind_category(kind).map(|_| t.category.join(" > "));
        let Some(i) = month_index(cfg.rent.due_date(path.as_deref(), t.date)) else {
            if t.date < first {
                before += 1;
            }
            continue;
        };
        fingerprints.entry(t.fingerprint).or_insert((0, t.date)).0 += 1;
        if matches!(kind, Kind::UncategorizedIn)
            || matches!(kind, Kind::Spend(k) if k == UNCATEGORIZED)
        {
            uncategorized += 1;
        }

        // `[pay] min_salary`: a smaller deposit is side income, not pay.
        let side;
        let kind = if matches!(kind, Kind::Salary)
            && cfg
                .pay
                .min_salary
                .is_some_and(|m| cents < (m * 100.0).round() as i64)
        {
            side = Kind::Income {
                kind: "other".into(),
                source: t.category.join(" > "),
            };
            &side
        } else {
            kind
        };
        match kind {
            Kind::Salary => deposits.push((t, cents)),
            Kind::Income { kind, source } => {
                if kind == SALARY_OTHER {
                    salary_outside_cash += 1;
                }
                add(&mut income, kind, i, cents);
                add(&mut income_by_category, source, i, cents);
            }
            Kind::Spend(key) => {
                add(&mut spending, key, i, -cents);
                if class == Class::Card {
                    add(&mut flows, "card_spend", i, -cents);
                }
            }
            Kind::IncomeTax => add(&mut flows, "income_tax", i, cents),
            Kind::Invested => add(&mut flows, "invested", i, -cents),
            Kind::Internal { counterpart } => {
                let from_cash = counterpart
                    .as_deref()
                    .is_none_or(|c| matches!(classes.get(c), Some(Class::Cash | Class::Other)));
                if class == Class::Card && cents > 0 && from_cash {
                    add(&mut flows, "card_payments", i, cents);
                }
            }
            Kind::UnknownTransfer => {
                unknown_transfers += 1;
                add(&mut flows, "transfer_unknown", i, cents);
            }
            Kind::UncategorizedIn => {
                add(&mut flows, "uncategorized_in", i, cents);
                let (_, label) = deposit_group(t, &deposit_labels);
                add(&mut income_by_category, &label, i, cents);
            }
        }
    }

    // Pay schedule and cycles.
    let schedule = match &cfg.pay.schedule {
        Some(s) => Some(Schedule::parse(s).ok_or_else(|| {
            format!("pay.schedule = {s:?}: use weekly, biweekly, semimonthly or monthly")
        })?),
        None => detect::detect_schedule(&deposits.iter().map(|(t, _)| t.date).collect::<Vec<_>>()),
    };
    let anchor = match &cfg.pay.anchor {
        Some(a) => Some(
            NaiveDate::parse_from_str(a, "%Y-%m-%d")
                .map_err(|_| "pay.anchor must be a quoted YYYY-MM-DD date".to_string())?,
        ),
        None => deposits.iter().map(|(t, _)| t.date).max(),
    };
    let pay = pay_summary(
        &deposits,
        schedule,
        anchor,
        cfg,
        &months,
        &month_index,
        &mut flows,
        pay_source,
    );

    // Accounts.
    let mut account_rows: Vec<&String> = classes.keys().collect();
    account_rows.sort_by_key(|name| (classes[*name] as u8, name.to_string()));
    let accounts = account_rows
        .into_iter()
        .map(|name| {
            let class = classes[name];
            let money = class != Class::Excluded;
            let balance = balances.get(name.as_str()).filter(|_| money).map(|b| {
                let after: i64 = txns
                    .iter()
                    .filter(|t| {
                        &t.account == name && b.as_of.is_some_and(|d| t.date > d) && t.date <= today
                    })
                    .map(|t| t.cents)
                    .sum();
                (b.cents + after, b.source)
            });
            AccountSummary {
                name: name.clone(),
                class,
                guessed: guessed.contains_key(name),
                class_reason: guessed.get(name).map_or("set in config", |r| r).to_string(),
                currency: account_currency
                    .get(name.as_str())
                    .cloned()
                    .unwrap_or_default(),
                rows: rows.get(name.as_str()).copied().unwrap_or(0),
                last_date: last_dates.get(name.as_str()).map(|d| d.to_string()),
                balance_cents: balance.map(|b| b.0),
                balance_source: balance.map(|b| b.1.to_string()),
                sum_balance_cents: if money {
                    Some(sums.get(name.as_str()).copied().unwrap_or(0))
                } else {
                    None
                },
            }
        })
        .collect();

    // Files.
    let per_file = inbox.rows_per_file();
    let mut files: Vec<FileSummary> = inbox
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let dates: Vec<NaiveDate> = txns
                .iter()
                .filter(|t| t.file == i)
                .map(|t| t.date)
                .collect();
            let accounts: BTreeSet<&str> = txns
                .iter()
                .filter(|t| t.file == i)
                .map(|t| t.account.as_str())
                .collect();
            FileSummary {
                name: f.file_name.clone(),
                format: f.format.clone(),
                transactions: per_file[i],
                errors: f.bad_lines.len(),
                date_min: dates.iter().min().map(|d| d.to_string()),
                date_max: dates.iter().max().map(|d| d.to_string()),
                notes: f.notes.clone(),
                accounts: accounts.into_iter().map(String::from).collect(),
                error: None,
                ledger: false,
            }
        })
        .collect();
    for (name, error) in &inbox.failed {
        files.push(FileSummary {
            name: name.clone(),
            format: "unreadable".to_string(),
            transactions: 0,
            errors: 0,
            date_min: None,
            date_max: None,
            notes: Vec::new(),
            accounts: Vec::new(),
            error: Some(error.clone()),
            ledger: false,
        });
    }
    for (name, format) in &inbox.ledger_files {
        files.push(FileSummary {
            name: name.clone(),
            format: format.clone(),
            transactions: 0,
            errors: 0,
            date_min: None,
            date_max: None,
            notes: Vec::new(),
            accounts: Vec::new(),
            error: None,
            ledger: true,
        });
    }

    let mut duplicate_months = BTreeSet::new();
    let mut duplicate_groups = 0;
    for (count, date) in fingerprints.values() {
        if *count > 1 {
            duplicate_groups += 1;
            duplicate_months.insert(date.format("%Y-%m").to_string());
        }
    }
    let unbalanced: Vec<i64> = pair_net.values().copied().filter(|v| *v != 0).collect();
    let unused: Vec<&str> = cfg
        .accounts
        .keys()
        .map(String::as_str)
        .filter(|a| !classes.contains_key(*a))
        .collect();
    let other_currency: usize = currencies
        .iter()
        .filter(|(c, _)| **c != base && **c != "(none)")
        .map(|(_, n)| n)
        .sum();
    let bad_lines: usize = inbox.files.iter().map(|f| f.bad_lines.len()).sum();
    let split_candidates: usize = inbox.files.iter().map(|f| f.split_candidates).sum();

    let check = |name: &str, count: usize, detail: String| Check {
        name: name.to_string(),
        count,
        detail,
    };
    let checks = vec![
        check(
            "unreadable rows (run first-error to see the first one)",
            bad_lines,
            String::new(),
        ),
        check(
            "files that could not be read",
            inbox.failed.len(),
            inbox
                .failed
                .iter()
                .map(|f| f.0.clone())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        check(
            "rows dropped as duplicates of another file",
            inbox.duplicates_dropped,
            String::new(),
        ),
        check("future-dated rows skipped", future, String::new()),
        check("rows before the window", before, String::new()),
        check(
            "possible duplicate groups inside one file",
            duplicate_groups,
            format!("months: {}", join(&duplicate_months)),
        ),
        check(
            "rows left uncategorized (no category, no rule, no keyword)",
            uncategorized,
            String::new(),
        ),
        check(
            "rows matched as own-account transfers",
            matched,
            String::new(),
        ),
        check(
            "transfers to accounts missing from the export",
            unknown_transfers,
            String::new(),
        ),
        check(
            "own-account transfer pairs that do not net to zero",
            unbalanced.len(),
            format!(
                "total gap {}",
                fmt_cents(unbalanced.iter().map(|v| v.abs()).sum())
            ),
        ),
        check(
            &format!("salary rows outside cash accounts (income type {SALARY_OTHER})"),
            salary_outside_cash,
            String::new(),
        ),
        check(
            &format!(
                "rows converted to {}",
                if base.is_empty() {
                    "the base currency"
                } else {
                    base.as_str()
                }
            ),
            other_currency,
            format!("{currencies:?}"),
        ),
        check(
            "split-transaction candidates",
            split_candidates,
            String::new(),
        ),
        check(
            "config account names not found in the data",
            unused.len(),
            unused.join(", "),
        ),
        check(
            "accounts with a guessed class",
            guessed.len(),
            String::new(),
        ),
    ];

    let mut category_roles = BTreeMap::new();
    for (path, rule) in roles.values() {
        let role = match rule {
            Rule::Income(kind) => format!("income ({kind})"),
            Rule::IncomeTax => "income tax".to_string(),
            Rule::Internal => "own-account transfer".to_string(),
            _ => continue,
        };
        category_roles.insert(path.join(" > "), role);
    }
    let salary_label = pay.source.clone();
    let sources: BTreeMap<String, Vec<i64>> = income_by_category
        .iter()
        .filter(|(k, _)| **k != salary_label)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    Ok(Summary {
        generated_on: today.to_string(),
        base_currency: base,
        suggested_extra_income: detect::suggest_income(&months, &sources),
        recurring_payments: recurring_payments(&charges, &data_until),
        months,
        files,
        accounts,
        pay,
        income_cents: income,
        income_by_category_cents: income_by_category,
        spending_cents: spending,
        flows_cents: flows,
        category_roles,
        checks,
        charges: spend_rows,
    })
}

/// Numbers payee groups of uncategorized deposits by total size, so reports
/// can say "Deposits #1" without showing the payee.
pub fn deposit_labels(txns: &[Txn]) -> HashMap<u64, usize> {
    let mut totals: HashMap<u64, i64> = HashMap::new();
    for t in txns
        .iter()
        .filter(|t| t.cents > 0 && t.payee_key != 0 && !t.category_from_file)
    {
        *totals.entry(t.payee_key).or_insert(0) += t.cents;
    }
    let mut ranked: Vec<(u64, i64)> = totals.into_iter().collect();
    ranked.sort_by_key(|(k, total)| (-total, *k));
    ranked
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (*k, i + 1))
        .collect()
}

/// One outflow that may belong to a recurring payment.
struct Charge<'a> {
    payee_key: u64,
    account: &'a str,
    date: NaiveDate,
    /// Spent, in base-currency cents (positive).
    cents: i64,
    category: String,
}

/// Groups outflows by hashed payee and keeps the regular ones, numbered
/// "Payments #n" by yearly cost. A series is judged against the last date
/// its own accounts have data for, so an older export does not hide it.
fn recurring_payments(
    charges: &[Charge],
    data_until: &HashMap<&str, NaiveDate>,
) -> Vec<RecurringPayment> {
    let mut groups: BTreeMap<u64, Vec<&Charge>> = BTreeMap::new();
    for c in charges {
        groups.entry(c.payee_key).or_default().push(c);
    }
    let mut found: Vec<(u64, detect::Recurring, String)> = Vec::new();
    for (key, rows) in groups {
        let Some(until) = rows.iter().filter_map(|c| data_until.get(c.account)).max() else {
            continue;
        };
        let dated: Vec<(NaiveDate, i64)> = rows.iter().map(|c| (c.date, c.cents)).collect();
        if let Some(r) = detect::detect_recurring(&dated, *until) {
            let latest = rows
                .iter()
                .max_by_key(|c| c.date)
                .expect("a group has rows");
            found.push((key, r, latest.category.clone()));
        }
    }
    found.sort_by_key(|(key, r, _)| (-r.yearly_cents(), *key));
    found
        .into_iter()
        .enumerate()
        .map(|(i, (key, r, category))| RecurringPayment {
            label: format!("Payments #{}", i + 1),
            cadence: r.cadence.name().to_string(),
            category,
            median_cents: r.median_cents,
            count: r.count,
            last_seen: r.last_seen.to_string(),
            yearly_cents: r.yearly_cents(),
            payee_key: key,
        })
        .collect()
}

/// The owner's private review file: which payee each deposit group and
/// each recurring payment is. Written to private/ only; never printed.
pub fn render_review(inbox: &Inbox, summary: &Summary) -> String {
    let mut labels: HashMap<u64, &str> = HashMap::new();
    for f in &inbox.files {
        for (k, v) in &f.payee_labels {
            labels.entry(*k).or_insert(v.as_str());
        }
    }
    let mut groups: Vec<(usize, u64)> = deposit_labels(&inbox.txns)
        .into_iter()
        .map(|(k, n)| (n, k))
        .collect();
    groups.sort();
    let mut o = String::from(
        "# Cashcast review (private)\n\nFor your eyes only. It shows short payee samples so you can tell the\ndeposit groups and recurring payments apart. It stays in private/ and the app never sends it anywhere.\n\n## Deposit groups\n\n",
    );
    for (n, k) in groups {
        let _ = writeln!(
            o,
            "- Deposits #{n}: {}",
            labels.get(&k).copied().unwrap_or("(no text)")
        );
    }
    if !summary.recurring_payments.is_empty() {
        let _ = writeln!(o, "\n## Recurring payments\n");
        for p in &summary.recurring_payments {
            let _ = writeln!(
                o,
                "- {}: {}",
                p.label,
                labels.get(&p.payee_key).copied().unwrap_or("(no text)")
            );
        }
    }
    o
}

/// The grouping key and report label of a deposit.
fn deposit_group(t: &Txn, labels: &HashMap<u64, usize>) -> (String, String) {
    if t.category_from_file && !t.category.is_empty() {
        let path = t.category.join(" > ");
        (format!("category:{path}"), path)
    } else if let Some(n) = labels.get(&t.payee_key) {
        (format!("payee:{}", t.payee_key), format!("Deposits #{n}"))
    } else {
        ("none".to_string(), "Uncategorized deposits".to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn pay_summary(
    deposits: &[(&Txn, i64)],
    schedule: Option<Schedule>,
    anchor: Option<NaiveDate>,
    cfg: &Config,
    months: &[String],
    month_index: &dyn Fn(NaiveDate) -> Option<usize>,
    flows: &mut BTreeMap<String, Vec<i64>>,
    source: String,
) -> PaySummary {
    let n = months.len();
    let mut cycle_map: BTreeMap<i64, (NaiveDate, Vec<(NaiveDate, i64)>)> = BTreeMap::new();
    let mut deposit_accounts: BTreeMap<String, usize> = BTreeMap::new();
    let mut off_cycle = 0;
    for &(t, cents) in deposits {
        *deposit_accounts.entry(t.account.clone()).or_insert(0) += 1;
        let Some(i) = month_index(t.date) else {
            continue;
        };
        match schedule
            .zip(anchor)
            .and_then(|(s, a)| cycle_of(t.date, s, a, cfg))
        {
            Some((key, nominal)) => {
                cycle_map
                    .entry(key)
                    .or_insert((nominal, Vec::new()))
                    .1
                    .push((t.date, cents));
                flows.get_mut("salary").expect("preset key")[i] += cents;
            }
            None if schedule.is_none() => {
                flows.get_mut("salary").expect("preset key")[i] += cents;
            }
            None => {
                off_cycle += 1;
                flows.get_mut("salary_off_cycle").expect("preset key")[i] += cents;
            }
        }
    }
    let mut cycles = Vec::new();
    let mut by_half: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut cycles_per_month = vec![0; n];
    for (nominal, parts) in cycle_map.values() {
        let paid_on = parts
            .iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .expect("a cycle has deposits")
            .0;
        let total: i64 = parts.iter().map(|p| p.1).sum();
        let half = if paid_on.month() <= 6 { 1 } else { 2 };
        by_half
            .entry(format!("{}-H{half}", paid_on.year()))
            .or_default()
            .push(total);
        if let Some(i) = month_index(paid_on) {
            cycles_per_month[i] += 1;
        }
        cycles.push(Cycle {
            nominal: nominal.to_string(),
            paid_on: paid_on.to_string(),
            weekday: paid_on.format("%a").to_string(),
            offset_days: (paid_on - *nominal).num_days(),
            deposits: parts.len(),
        });
    }
    let half_years = by_half
        .into_iter()
        .map(|(period, mut totals)| {
            totals.sort_unstable();
            let median = median(&totals);
            let off_median = totals
                .iter()
                .filter(|&&x| (x - median).abs() * 20 > median.abs())
                .count();
            HalfYear {
                period,
                cheques: totals.len(),
                median_cents: median,
                off_median,
            }
        })
        .collect();
    let mut gap_days = BTreeMap::new();
    for pair in cycles.windows(2) {
        let a = NaiveDate::parse_from_str(&pair[0].paid_on, "%Y-%m-%d").expect("own format");
        let b = NaiveDate::parse_from_str(&pair[1].paid_on, "%Y-%m-%d").expect("own format");
        *gap_days.entry((b - a).num_days()).or_insert(0) += 1;
    }
    PaySummary {
        schedule: schedule.map(|s| s.name().to_string()),
        source,
        anchor: anchor.map(|a| a.to_string()),
        cycles,
        half_years,
        gap_days,
        off_cycle_deposits: off_cycle,
        deposit_accounts,
        cycles_per_month,
    }
}

/// Cycle key and nominal payday for a deposit, or None when off-cycle.
fn cycle_of(
    date: NaiveDate,
    schedule: Schedule,
    anchor: NaiveDate,
    cfg: &Config,
) -> Option<(i64, NaiveDate)> {
    match schedule {
        Schedule::Weekly | Schedule::Biweekly => {
            let period = schedule.period_days().expect("weekly kinds have a period");
            let late = cfg.pay.late_days.unwrap_or(if period == 7 { 1 } else { 2 });
            let early = cfg
                .pay
                .early_days
                .unwrap_or(if period == 7 { 2 } else { 4 });
            let diff = (date - anchor).num_days();
            let r = diff.rem_euclid(period);
            let k = if r <= late {
                (diff - r) / period
            } else if r >= period - early {
                (diff + period - r) / period
            } else {
                return None;
            };
            Some((k, anchor + chrono::Duration::days(k * period)))
        }
        Schedule::Semimonthly => {
            let half = detect::half_of(date);
            let nominal = if half == 1 {
                detect::clamp_day(date.year(), date.month(), 15)
            } else {
                detect::last_day(date.year(), date.month())
            };
            Some((
                date.year() as i64 * 24 + (date.month() as i64 - 1) * 2 + half as i64,
                nominal,
            ))
        }
        Schedule::Monthly => Some((
            date.year() as i64 * 12 + date.month() as i64 - 1,
            detect::clamp_day(date.year(), date.month(), anchor.day()),
        )),
    }
}

fn classify(t: &Txn, class: Class, partner: Option<&Txn>, ctx: &Ctx) -> Kind {
    let counterpart = if !t.transfer.is_empty() {
        Some(t.transfer.as_str())
    } else {
        partner.map(|p| p.account.as_str())
    };
    if let Some(other) = counterpart {
        return match ctx.classes.get(other) {
            Some(Class::Cash | Class::Card | Class::Other) => Kind::Internal {
                counterpart: Some(other.to_string()),
            },
            Some(Class::Excluded) if t.cents < 0 => Kind::Invested,
            Some(Class::Excluded) => Kind::Income {
                kind: ctx
                    .cfg
                    .transfers
                    .from_excluded
                    .get(other)
                    .cloned()
                    .unwrap_or_else(|| ctx.cfg.transfers.default_income.clone()),
                source: format!("Transfer from {other}"),
            },
            None => Kind::UnknownTransfer,
        };
    }
    let source = t.category.join(" > ");
    let rule = match_rule(ctx.rules, &t.category).or_else(|| {
        if t.category_from_file {
            lookup_role(ctx, t)
        } else {
            None
        }
    });
    match rule {
        Some(Rule::Salary) if class == Class::Cash => return Kind::Salary,
        Some(Rule::Salary) => {
            return Kind::Income {
                kind: SALARY_OTHER.into(),
                source,
            };
        }
        Some(Rule::Income(kind)) => {
            return Kind::Income {
                kind: kind.clone(),
                source,
            };
        }
        Some(Rule::IncomeTax) => return Kind::IncomeTax,
        Some(Rule::Internal) => return Kind::Internal { counterpart: None },
        Some(Rule::Invested) => return Kind::Invested,
        Some(Rule::Spending) => return Kind::Spend(spend_key(t, ctx.separate)),
        None => {}
    }
    if !t.category_from_file {
        match t.hint {
            Hint::CardPayment | Hint::OwnTransfer => return Kind::Internal { counterpart: None },
            Hint::Invest if t.cents < 0 => return Kind::Invested,
            Hint::Invest => {
                return Kind::Income {
                    kind: ctx.cfg.transfers.default_income.clone(),
                    source: "Investment withdrawals".into(),
                };
            }
            Hint::IncomeTax => return Kind::IncomeTax,
            Hint::TaxRefund => {
                return Kind::Income {
                    kind: "tax_refund".into(),
                    source: "Tax refunds and credits".into(),
                };
            }
            Hint::Cashback => {
                return Kind::Income {
                    kind: "cashback".into(),
                    source: "Cashback and interest".into(),
                };
            }
            Hint::Payroll | Hint::None => {}
        }
        if t.cents > 0 && t.category.is_empty() {
            return Kind::UncategorizedIn;
        }
    } else if t.category.is_empty() && t.cents > 0 {
        return Kind::UncategorizedIn;
    }
    Kind::Spend(spend_key(t, ctx.separate))
}

fn lookup_role(ctx: &Ctx, t: &Txn) -> Option<Rule> {
    let lower: Vec<String> = t.category.iter().map(|p| p.to_lowercase()).collect();
    ctx.roles.get(&lower).map(|(_, rule)| rule.clone())
}

fn spend_key(t: &Txn, separate: &[Vec<String>]) -> String {
    let lower: Vec<String> = t.category.iter().map(|p| p.to_lowercase()).collect();
    let split = separate
        .iter()
        .filter(|p| !p.is_empty() && lower.starts_with(p))
        .max_by_key(|p| p.len());
    match split {
        Some(p) => t.category[..p.len()].join(" > "),
        None => t
            .category
            .first()
            .cloned()
            .unwrap_or_else(|| UNCATEGORIZED.to_string()),
    }
}

/// Roles for file categories that no config rule covers: transfers, income
/// tax and income are recognized by name and by sign; the rest is spending.
fn auto_roles(
    txns: &[Txn],
    classes: &BTreeMap<String, Class>,
    rules: &[(Vec<String>, Rule)],
) -> HashMap<Vec<String>, (Vec<String>, Rule)> {
    #[derive(Default)]
    struct Stats {
        rows: usize,
        positive: usize,
        net: i64,
    }
    let mut paths: HashMap<Vec<String>, (Vec<String>, Stats)> = HashMap::new();
    let mut tops: HashMap<String, i64> = HashMap::new();
    for t in txns {
        if !t.category_from_file
            || !t.transfer.is_empty()
            || t.category.is_empty()
            || classes[t.account.as_str()] == Class::Excluded
        {
            continue;
        }
        let lower: Vec<String> = t.category.iter().map(|p| p.to_lowercase()).collect();
        *tops.entry(lower[0].clone()).or_insert(0) += t.cents;
        let entry = paths
            .entry(lower)
            .or_insert_with(|| (t.category.clone(), Stats::default()));
        entry.1.rows += 1;
        entry.1.positive += usize::from(t.cents > 0);
        entry.1.net += t.cents;
    }
    let mut roles = HashMap::new();
    for (lower, (original, s)) in paths {
        if match_rule(rules, &original).is_some() {
            continue;
        }
        let all = original.join(" ");
        let last = original.last().cloned().unwrap_or_default();
        let top_net = tops.get(&lower[0]).copied().unwrap_or(0);
        let role = if is_named(&last, INTERNAL_CATEGORY) {
            Rule::Internal
        } else if is_named(&last, TAX_CATEGORY) && s.net < 0 {
            Rule::IncomeTax
        } else if (has_words(&all, INCOME_CATEGORY) && s.net > 0)
            || (s.positive * 10 >= s.rows * 8 && s.net > 0 && top_net > 0)
        {
            Rule::Income(income_type_for(&all))
        } else {
            continue;
        };
        roles.insert(lower, (original, role));
    }
    roles
}

fn merge_balances(inbox: &Inbox) -> BTreeMap<String, Balance> {
    let mut out: BTreeMap<String, Balance> = BTreeMap::new();
    for f in &inbox.files {
        for (name, b) in &f.balances {
            let replace = match out.get(name) {
                None => true,
                Some(old) => match (old.as_of, b.as_of) {
                    (None, _) => false,
                    (Some(_), None) => true,
                    (Some(a), Some(c)) => c > a,
                },
            };
            if replace {
                out.insert(name.clone(), b.clone());
            }
        }
    }
    out
}

fn base_currency(cfg: &Config, txns: &[Txn]) -> String {
    if let Some(base) = &cfg.currencies.base {
        return base.clone();
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for t in txns.iter().filter(|t| !t.currency.is_empty()) {
        *counts.entry(t.currency.as_str()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or_else(String::new, |(c, _)| c.to_string())
}

/// Each account's most common currency.
fn account_currencies(txns: &[Txn]) -> BTreeMap<&str, String> {
    let mut counts: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for t in txns {
        *counts
            .entry(t.account.as_str())
            .or_default()
            .entry(t.currency.as_str())
            .or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(account, c)| {
            (
                account,
                c.into_iter()
                    .max_by_key(|(_, n)| *n)
                    .map_or("", |(cur, _)| cur)
                    .to_string(),
            )
        })
        .collect()
}

fn build_rules(cfg: &Config, separator: &str) -> Vec<(Vec<String>, Rule)> {
    let c = &cfg.categories;
    let mut rules = Vec::new();
    let mut push = |paths: &[String], rule: Rule| {
        for p in paths {
            rules.push((split_rule_path(p, separator), rule.clone()));
        }
    };
    push(&c.salary, Rule::Salary);
    push(&c.rsu_espp, Rule::Income("rsu_espp".into()));
    push(&c.tax_refund, Rule::Income("tax_refund".into()));
    push(&c.cashback, Rule::Income("cashback".into()));
    push(&c.gift, Rule::Income("gift".into()));
    push(&c.other_income, Rule::Income("other".into()));
    push(&c.income_tax, Rule::IncomeTax);
    push(&c.internal, Rule::Internal);
    push(&c.invested, Rule::Invested);
    push(&c.spending, Rule::Spending);
    rules
}

/// Splits a config path on `>` or any export separator, lowercased.
fn split_rule_path(path: &str, separator: &str) -> Vec<String> {
    let mut parts = vec![path.to_string()];
    for sep in CATEGORY_SEPARATORS
        .iter()
        .filter(|s| **s != ":" || separator == ":")
    {
        parts = parts
            .iter()
            .flat_map(|p| p.split(sep).map(String::from).collect::<Vec<_>>())
            .collect();
    }
    parts
        .into_iter()
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect()
}

fn match_rule(rules: &[(Vec<String>, Rule)], category: &[String]) -> Option<Rule> {
    let lower: Vec<String> = category.iter().map(|p| p.to_lowercase()).collect();
    rules
        .iter()
        .filter(|(path, _)| !path.is_empty() && lower.starts_with(path))
        .max_by_key(|(path, _)| path.len())
        .map(|(_, rule)| rule.clone())
}

fn median(sorted: &[i64]) -> i64 {
    let n = sorted.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        let sum = sorted[n / 2 - 1] + sorted[n / 2];
        (sum + sum.signum()) / 2
    }
}

/// How far most accounts' data reaches: the upper median newest row among the included
/// accounts with at least 3 rows in the last 62 days of data. One fresh download beside a
/// stale export does not stretch it, and a quiet account (one interest row) does not
/// shorten it.
fn covered_until(
    txns: &[Txn],
    classes: &BTreeMap<String, Class>,
    today: NaiveDate,
) -> Option<NaiveDate> {
    let rows: Vec<&Txn> = txns
        .iter()
        .filter(|t| t.date <= today && classes.get(t.account.as_str()) != Some(&Class::Excluded))
        .collect();
    let top = rows.iter().map(|t| t.date).max()?;
    let mut recent: HashMap<&str, (usize, NaiveDate)> = HashMap::new();
    for t in rows.iter().filter(|t| t.date + chrono::Days::new(62) >= top) {
        let e = recent.entry(t.account.as_str()).or_insert((0, t.date));
        e.0 += 1;
        e.1 = e.1.max(t.date);
    }
    let mut active: Vec<NaiveDate> = recent
        .into_values()
        .filter(|(n, _)| *n >= 3)
        .map(|(_, d)| d)
        .collect();
    if active.is_empty() {
        return Some(top);
    }
    active.sort_unstable();
    Some(active[active.len() / 2])
}

/// Months to import. With no `last_month`, the window ends at last calendar month, or
/// earlier when the data (`newest`, from `covered_until`) leaves a month mostly empty: a
/// month counts only when the data reaches within 7 days of its end. A pinned
/// `first_month` after that month keeps the calendar end.
fn window(
    cfg: &Config,
    today: NaiveDate,
    newest: Option<NaiveDate>,
) -> Result<(NaiveDate, NaiveDate), String> {
    let parse = |s: &str| {
        NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
            .map_err(|_| format!("month {s:?} must be YYYY-MM"))
    };
    let this_month = today.with_day(1).expect("day 1 exists");
    let last = match &cfg.window.last_month {
        Some(m) => parse(m)?,
        None => {
            let calendar = this_month - Months::new(1);
            let covered = newest.map(|n| {
                let month = n.with_day(1).expect("day 1 exists");
                let month_end = month + Months::new(1) - chrono::Days::new(1);
                if n + chrono::Days::new(7) >= month_end {
                    month
                } else {
                    month - Months::new(1)
                }
            });
            let pinned_first = match &cfg.window.first_month {
                Some(m) => Some(parse(m)?),
                None => None,
            };
            match covered {
                Some(c) if pinned_first.is_none_or(|f| f <= c) => c.min(calendar),
                _ => calendar,
            }
        }
    };
    let first = match &cfg.window.first_month {
        Some(m) => parse(m)?,
        None => last - Months::new(35),
    };
    if first > last {
        return Err("window.first_month is after window.last_month".into());
    }
    Ok((first, last))
}

/// The spending series a row adds to, if it is spending.
fn kind_category(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Spend(c) => Some(c),
        _ => None,
    }
}

fn month_list(first: NaiveDate, last: NaiveDate) -> Vec<String> {
    let mut out = Vec::new();
    let mut m = first;
    while m <= last {
        out.push(m.format("%Y-%m").to_string());
        m = m + Months::new(1);
    }
    out
}

fn join(set: &BTreeSet<String>) -> String {
    set.iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The import report printed to the terminal. Names, counts and sums only.
pub fn render(s: &Summary) -> String {
    let mut o = String::new();
    let money = |c: Option<i64>| c.map_or("-".to_string(), fmt_cents);
    let _ = writeln!(
        o,
        "# Import report ({} to {})\n",
        s.months.first().map_or("", String::as_str),
        s.months.last().map_or("", String::as_str)
    );
    let _ = writeln!(o, "## Files\n");
    for f in &s.files {
        match &f.error {
            Some(e) => {
                let _ = writeln!(o, "- {}: NOT READ: {e}", f.name);
            }
            None if f.ledger => {
                let _ = writeln!(
                    o,
                    "- {} ({}): ledger file, kept out of the cash import. Run the holdings command for it.",
                    f.name, f.format
                );
            }
            None => {
                let notes = if f.notes.is_empty() {
                    String::new()
                } else {
                    format!(" Notes: {}.", f.notes.join("; "))
                };
                let _ = writeln!(
                    o,
                    "- {} ({}): {} transactions, {} unreadable rows, {} to {}.{notes}",
                    f.name,
                    f.format,
                    f.transactions,
                    f.errors,
                    f.date_min.as_deref().unwrap_or("-"),
                    f.date_max.as_deref().unwrap_or("-"),
                );
            }
        }
    }
    let base = if s.base_currency.is_empty() {
        "-"
    } else {
        &s.base_currency
    };
    let _ = writeln!(o, "\n## Accounts (base currency {base})\n");
    let _ = writeln!(
        o,
        "| Account | Class | Why | Cur | Rows | Last row | Balance | Balance source |"
    );
    let _ = writeln!(o, "|---|---|---|---|---:|---|---:|---|");
    for a in &s.accounts {
        let class = if a.guessed {
            format!("{} (guess)", a.class.name())
        } else {
            a.class.name().to_string()
        };
        let _ = writeln!(
            o,
            "| {} | {class} | {} | {} | {} | {} | {} | {} |",
            a.name,
            a.class_reason,
            a.currency,
            a.rows,
            a.last_date.as_deref().unwrap_or("-"),
            money(a.balance_cents),
            a.balance_source.as_deref().unwrap_or("unknown"),
        );
    }
    let _ = writeln!(o, "\n## Pay\n");
    match &s.pay.schedule {
        Some(schedule) => {
            let _ = writeln!(
                o,
                "- Schedule: {schedule}, from {}. Last payday: {}.",
                s.pay.source,
                s.pay.anchor.as_deref().unwrap_or("-")
            );
        }
        None => {
            let _ = writeln!(o, "- No regular pay found ({}).", s.pay.source);
        }
    }
    for h in &s.pay.half_years {
        let _ = writeln!(
            o,
            "- {}: {} cheques, median {}, {} more than 5% off the median",
            h.period,
            h.cheques,
            fmt_cents(h.median_cents),
            h.off_median
        );
    }
    if !s.category_roles.is_empty() {
        let _ = writeln!(o, "\n## Categories classified automatically\n");
        for (path, role) in &s.category_roles {
            let _ = writeln!(o, "- {path}: {role}");
        }
    }
    if !s.suggested_extra_income.is_empty() {
        let _ = writeln!(o, "\n## Recurring extra income found\n");
        for x in &s.suggested_extra_income {
            let _ = writeln!(
                o,
                "- {} ({}, months {:?}): {}",
                x.name,
                x.pattern,
                x.months,
                fmt_cents(x.amount_cents)
            );
        }
    }
    if !s.recurring_payments.is_empty() {
        let _ = writeln!(o, "\n## Recurring payments found\n");
        for p in &s.recurring_payments {
            let _ = writeln!(
                o,
                "- {}: {}, {}, {} charges, usually {}, last {}; about {} a year",
                p.label,
                p.cadence,
                p.category,
                p.count,
                fmt_cents(p.median_cents),
                p.last_seen,
                fmt_cents(p.yearly_cents)
            );
        }
    }
    let _ = writeln!(o, "\n## Spending categories\n");
    let _ = writeln!(
        o,
        "{}",
        s.spending_cents
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(o, "\n## Data checks\n");
    for c in s.checks.iter().filter(|c| c.count > 0) {
        let detail = if c.detail.is_empty() {
            String::new()
        } else {
            format!(" ({})", c.detail)
        };
        let _ = writeln!(o, "- {}: {}{detail}", c.name, c.count);
    }
    o
}
