//! Detection that needs no raw text: account classes, own-account
//! transfers, pay schedules, recurring income and recurring payments.

use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use crate::export::{AccountKind, Txn};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Cash,
    Card,
    Other,
    Excluded,
}

impl Class {
    pub fn parse(text: &str) -> Option<Class> {
        match text.trim().to_lowercase().as_str() {
            "cash" => Some(Class::Cash),
            "card" => Some(Class::Card),
            "other" => Some(Class::Other),
            "excluded" => Some(Class::Excluded),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::Cash => "cash",
            Class::Card => "card",
            Class::Other => "other",
            Class::Excluded => "excluded",
        }
    }
}

const EXCLUDED_WORDS: &[&str] = &[
    "tfsa",
    "rrsp",
    "fhsa",
    "resp",
    "rrif",
    "lira",
    "401k",
    "ira",
    "roth",
    "hsa",
    "brokerage",
    "invest",
    "investing",
    "investment",
    "investments",
    "crypto",
    "bitcoin",
    "kraken",
    "coinbase",
    "binance",
    "etrade",
    "trade",
    "robinhood",
    "vanguard",
    "fidelity",
    "schwab",
    "questrade",
    "pension",
    "retirement",
    "stock",
    "stocks",
    "shares",
    "rsu",
    "espp",
    "margin",
    "registered",
    "nonregistered",
];
const CARD_WORDS: &[&str] = &[
    "credit",
    "visa",
    "mastercard",
    "amex",
    "american express",
    "discover",
    "card",
];
const WALLET_WORDS: &[&str] = &["cash", "wallet"];
const BANK_WORDS: &[&str] = &[
    "debit", "chequing", "checking", "savings", "current", "everyday", "bank",
];

/// Guesses an account's class from its name, statement type and currency.
pub fn guess_class(
    name: &str,
    kind: Option<AccountKind>,
    currency: &str,
    base: &str,
) -> (Class, &'static str) {
    let n = format!(
        " {} ",
        name.to_lowercase()
            .replace(|c: char| !c.is_alphanumeric(), " ")
    );
    let n = n.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = format!(" {n} ");
    let any = |words: &[&str]| words.iter().any(|w| n.contains(&format!(" {w} ")));
    let foreign = !currency.is_empty() && !base.is_empty() && currency != base;
    if any(EXCLUDED_WORDS) {
        (Class::Excluded, "investment-like name")
    } else if any(&["debit"]) && !any(&["credit"]) {
        if foreign {
            (Class::Other, "foreign-currency bank account")
        } else {
            (Class::Cash, "debit account name")
        }
    } else if kind == Some(AccountKind::Card) {
        (Class::Card, "credit card statement")
    } else if any(CARD_WORDS) {
        (Class::Card, "card-like name")
    } else if foreign {
        (Class::Other, "foreign currency")
    } else if any(WALLET_WORDS) {
        (Class::Other, "cash wallet")
    } else if kind == Some(AccountKind::Bank) {
        (Class::Cash, "bank statement")
    } else if any(BANK_WORDS) {
        (Class::Cash, "bank-like name")
    } else {
        (Class::Other, "unknown: please confirm")
    }
}

/// Pairs an outflow in one account with an equal inflow in another account
/// within four days. Returns each row's partner index, if any.
pub fn match_transfers(txns: &[Txn], eligible: &[bool]) -> Vec<Option<usize>> {
    let mut partner = vec![None; txns.len()];
    let mut inflows: HashMap<(i64, &str), Vec<usize>> = HashMap::new();
    for (i, t) in txns.iter().enumerate() {
        if eligible[i] && t.cents > 0 {
            inflows
                .entry((t.cents, t.currency.as_str()))
                .or_default()
                .push(i);
        }
    }
    let mut outflows: Vec<usize> = (0..txns.len())
        .filter(|&i| eligible[i] && txns[i].cents < 0)
        .collect();
    outflows.sort_by_key(|&i| txns[i].date);
    for o in outflows {
        let out = &txns[o];
        let Some(candidates) = inflows.get(&(-out.cents, out.currency.as_str())) else {
            continue;
        };
        let best = candidates
            .iter()
            .copied()
            .filter(|&i| partner[i].is_none() && txns[i].account != out.account)
            .map(|i| (i, (txns[i].date - out.date).num_days().abs()))
            .filter(|(_, gap)| *gap <= 4)
            .min_by_key(|(i, gap)| (*gap, *i));
        if let Some((i, _)) = best {
            partner[o] = Some(i);
            partner[i] = Some(o);
        }
    }
    partner
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Schedule {
    Weekly,
    Biweekly,
    Semimonthly,
    Monthly,
}

impl Schedule {
    pub fn parse(text: &str) -> Option<Schedule> {
        match text.trim().to_lowercase().as_str() {
            "weekly" => Some(Schedule::Weekly),
            "biweekly" => Some(Schedule::Biweekly),
            "semimonthly" => Some(Schedule::Semimonthly),
            "monthly" => Some(Schedule::Monthly),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Schedule::Weekly => "weekly",
            Schedule::Biweekly => "biweekly",
            Schedule::Semimonthly => "semimonthly",
            Schedule::Monthly => "monthly",
        }
    }

    pub fn period_days(self) -> Option<i64> {
        match self {
            Schedule::Weekly => Some(7),
            Schedule::Biweekly => Some(14),
            _ => None,
        }
    }
}

/// Finds a regular pay schedule in a set of deposit dates.
pub fn detect_schedule(dates: &[NaiveDate]) -> Option<Schedule> {
    let mut days: Vec<NaiveDate> = dates.to_vec();
    days.sort();
    days.dedup();
    if days.len() < 4 {
        return None;
    }
    let gaps: Vec<i64> = days.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
    let share = |lo: i64, hi: i64| {
        gaps.iter().filter(|g| (lo..=hi).contains(*g)).count() as f64 / gaps.len() as f64
    };
    if share(14, 14) >= 0.6 && share(13, 15) >= 0.7 {
        Some(Schedule::Biweekly)
    } else if share(6, 8) >= 0.7 {
        Some(Schedule::Weekly)
    } else if share(27, 33) >= 0.7 {
        Some(Schedule::Monthly)
    } else if share(12, 19) >= 0.7 {
        Some(Schedule::Semimonthly)
    } else {
        None
    }
}

/// A group of deposits that might be pay.
pub struct PayCandidate {
    pub label: String,
    pub dates: Vec<NaiveDate>,
    pub cents: Vec<i64>,
    /// The group was marked as payroll by its description or category name.
    pub payroll_hint: bool,
}

/// Picks the most likely pay group: regular, similar amounts, and the
/// largest yearly total. Returns its index and schedule.
pub fn pick_pay(candidates: &[PayCandidate]) -> Option<(usize, Schedule)> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let schedule = detect_schedule(&c.dates)?;
            let mut amounts = c.cents.clone();
            amounts.sort_unstable();
            let median = amounts[amounts.len() / 2].max(1);
            let close = amounts
                .iter()
                .filter(|a| (**a - median).abs() * 2 <= median)
                .count();
            if close * 10 < amounts.len() * 6 {
                return None;
            }
            let total: i64 = c.cents.iter().sum();
            let score = if c.payroll_hint {
                total.saturating_mul(2)
            } else {
                total
            };
            Some((i, schedule, score))
        })
        .max_by_key(|(_, _, score)| *score)
        .map(|(i, schedule, _)| (i, schedule))
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub name: String,
    pub pattern: &'static str,
    pub months: Vec<u32>,
    pub amount_cents: i64,
}

/// Suggests recurring extra income from monthly series: monthly items,
/// yearly items (same calendar month two years running) and quarterly items.
pub fn suggest_income(months: &[String], sources: &BTreeMap<String, Vec<i64>>) -> Vec<Suggestion> {
    let n = months.len();
    if n < 12 {
        return Vec::new();
    }
    let month_of = |i: usize| months[i][5..7].parse::<u32>().unwrap_or(1);
    let mut out = Vec::new();
    for (name, series) in sources {
        let last12: Vec<usize> = (n - 12..n).collect();
        let hits: Vec<usize> = last12.iter().copied().filter(|&i| series[i] > 0).collect();
        if hits.is_empty() {
            continue;
        }
        if hits.len() >= 10 {
            let mut values: Vec<i64> = hits.iter().map(|&i| series[i]).collect();
            values.sort_unstable();
            out.push(Suggestion {
                name: name.clone(),
                pattern: "monthly",
                months: (1..=12).collect(),
                amount_cents: values[values.len() / 2],
            });
            continue;
        }
        let yearly: Vec<usize> = hits
            .iter()
            .copied()
            .filter(|&i| i >= 12 && series[i - 12] > 0)
            .collect();
        if !yearly.is_empty() && hits.len() <= 3 {
            for i in yearly {
                out.push(Suggestion {
                    name: name.clone(),
                    pattern: "yearly",
                    months: vec![month_of(i)],
                    amount_cents: series[i],
                });
            }
            continue;
        }
        for residue in 0..3 {
            let quarter_months: Vec<usize> = last12
                .iter()
                .copied()
                .filter(|&i| month_of(i) % 3 == residue)
                .collect();
            let quarter_hits = quarter_months.iter().filter(|&&i| series[i] > 0).count();
            if quarter_hits >= 3 {
                let total: i64 = quarter_months.iter().map(|&i| series[i]).sum();
                let mut qm: Vec<u32> = quarter_months.iter().map(|&i| month_of(i)).collect();
                qm.sort_unstable();
                out.push(Suggestion {
                    name: name.clone(),
                    pattern: "quarterly",
                    months: qm,
                    amount_cents: total / 4,
                });
                break;
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cadence {
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

const CADENCES: [Cadence; 4] = [
    Cadence::Weekly,
    Cadence::Monthly,
    Cadence::Quarterly,
    Cadence::Yearly,
];

impl Cadence {
    pub fn name(self) -> &'static str {
        match self {
            Cadence::Weekly => "weekly",
            Cadence::Monthly => "monthly",
            Cadence::Quarterly => "quarterly",
            Cadence::Yearly => "yearly",
        }
    }

    pub fn per_year(self) -> i64 {
        match self {
            Cadence::Weekly => 52,
            Cadence::Monthly => 12,
            Cadence::Quarterly => 4,
            Cadence::Yearly => 1,
        }
    }

    /// Gaps in days that count as this cadence. Months are 28 to 31 days,
    /// and a charge can post a few days late (weekends, holidays), so
    /// longer periods get a wider band.
    pub fn gap_days(self) -> (i64, i64) {
        match self {
            Cadence::Weekly => (6, 8),
            Cadence::Monthly => (25, 35),
            Cadence::Quarterly => (84, 98),
            Cadence::Yearly => (350, 380),
        }
    }
}

/// Share of gaps (and of amounts) that must be regular. One missed or
/// doubled charge in a long series still counts; with 3 charges, all must fit.
const REGULAR_SHARE: f64 = 0.75;
/// A charge is "the usual amount" within 25% of the median, so a price
/// change does not break the series.
const AMOUNT_TOLERANCE_PCT: i64 = 25;

/// A payee's regular outflows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Recurring {
    pub cadence: Cadence,
    /// Median of the last year of charges (at least the last 3), in cents.
    pub median_cents: i64,
    /// Days with a charge.
    pub count: usize,
    pub last_seen: NaiveDate,
}

impl Recurring {
    pub fn yearly_cents(&self) -> i64 {
        self.median_cents * self.cadence.per_year()
    }
}

/// Finds a regular schedule in one payee's charges (date, cents spent as a
/// positive number). Charges on the same day add up to one charge.
///
/// A payee counts when it has at least 3 charges, at least 75% of the gaps
/// fall in one cadence's band, and at least 75% of the charges are within
/// 25% of the median amount. A series is dropped as stopped when, by
/// `data_until` (the last date its accounts have data for), the next charge
/// is more than half a period late.
pub fn detect_recurring(charges: &[(NaiveDate, i64)], data_until: NaiveDate) -> Option<Recurring> {
    let mut by_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for (date, cents) in charges {
        *by_day.entry(*date).or_insert(0) += cents;
    }
    if by_day.len() < 3 {
        return None;
    }
    let days: Vec<NaiveDate> = by_day.keys().copied().collect();
    let amounts: Vec<i64> = by_day.values().copied().collect();
    let mut gaps: Vec<i64> = days.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
    gaps.sort_unstable();
    let middle = gaps[gaps.len() / 2];
    let cadence = CADENCES.into_iter().find(|c| {
        let (lo, hi) = c.gap_days();
        (lo..=hi).contains(&middle)
    })?;
    let (lo, hi) = cadence.gap_days();
    let regular = gaps.iter().filter(|g| (lo..=hi).contains(*g)).count();
    if (regular as f64) < REGULAR_SHARE * gaps.len() as f64 {
        return None;
    }
    let mut sorted = amounts.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let usual = amounts
        .iter()
        .filter(|a| (**a - median).abs() * 100 <= median.abs() * AMOUNT_TOLERANCE_PCT)
        .count();
    if (usual as f64) < REGULAR_SHARE * amounts.len() as f64 {
        return None;
    }
    let last_seen = *days.last().expect("at least 3 days");
    let period = (lo + hi) / 2;
    if (data_until - last_seen).num_days() > hi + period / 2 {
        return None;
    }
    let recent = cadence.per_year().max(3) as usize;
    let mut last: Vec<i64> = amounts[amounts.len().saturating_sub(recent)..].to_vec();
    last.sort_unstable();
    Some(Recurring {
        cadence,
        median_cents: last[last.len() / 2],
        count: days.len(),
        last_seen,
    })
}

/// Day of a month, clamped to the month's length.
pub fn clamp_day(year: i32, month: u32, day: u32) -> NaiveDate {
    (1..=day)
        .rev()
        .find_map(|d| NaiveDate::from_ymd_opt(year, month, d))
        .expect("day 1 exists")
}

pub fn last_day(year: i32, month: u32) -> NaiveDate {
    clamp_day(year, month, 31)
}

pub fn half_of(date: NaiveDate) -> u32 {
    if date.day() <= 15 { 1 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn series(start: &str, step_days: i64, n: usize) -> Vec<NaiveDate> {
        (0..n)
            .map(|k| d(start) + chrono::Duration::days(step_days * k as i64))
            .collect()
    }

    #[test]
    fn schedules_are_detected() {
        assert_eq!(
            detect_schedule(&series("2026-01-02", 14, 10)),
            Some(Schedule::Biweekly)
        );
        assert_eq!(
            detect_schedule(&series("2026-01-02", 7, 10)),
            Some(Schedule::Weekly)
        );
        let monthly: Vec<NaiveDate> = (1..=8).map(|m| clamp_day(2026, m, 28)).collect();
        assert_eq!(detect_schedule(&monthly), Some(Schedule::Monthly));
        let semi: Vec<NaiveDate> = (1..=6)
            .flat_map(|m| [d(&format!("2026-{m:02}-15")), last_day(2026, m)])
            .collect();
        assert_eq!(detect_schedule(&semi), Some(Schedule::Semimonthly));
        assert_eq!(
            detect_schedule(&[
                d("2026-01-01"),
                d("2026-03-09"),
                d("2026-03-10"),
                d("2026-07-01")
            ]),
            None
        );
    }

    #[test]
    fn classes_are_guessed() {
        assert_eq!(
            guess_class("Everyday Chequing", None, "CAD", "CAD").0,
            Class::Cash
        );
        assert_eq!(
            guess_class("Visa Infinite", None, "CAD", "CAD").0,
            Class::Card
        );
        assert_eq!(
            guess_class("Main TFSA", None, "CAD", "CAD").0,
            Class::Excluded
        );
        assert_eq!(
            guess_class("Checking …1234", Some(AccountKind::Bank), "CAD", "CAD").0,
            Class::Cash
        );
        assert_eq!(
            guess_class("Statement …9", Some(AccountKind::Card), "", "CAD").0,
            Class::Card
        );
        assert_eq!(
            guess_class("Travel wallet", None, "USD", "CAD").0,
            Class::Other
        );
        assert_eq!(guess_class("Mystery", None, "CAD", "CAD").0, Class::Other);
    }

    fn charges(dates: &[NaiveDate], cents: i64) -> Vec<(NaiveDate, i64)> {
        dates.iter().map(|d| (*d, cents)).collect()
    }

    #[test]
    fn recurring_payments_need_three_regular_charges() {
        let monthly: Vec<NaiveDate> = (1..=9).map(|m| clamp_day(2026, m, 31)).collect();
        let r = detect_recurring(&charges(&monthly, 1_699), d("2026-09-30")).unwrap();
        assert_eq!(
            (r.cadence, r.median_cents, r.count, r.yearly_cents()),
            (Cadence::Monthly, 1_699, 9, 1_699 * 12)
        );
        let weekly = detect_recurring(
            &charges(&series("2026-06-01", 7, 10), 2_500),
            d("2026-08-10"),
        )
        .unwrap();
        assert_eq!(weekly.cadence, Cadence::Weekly);
        let quarterly = detect_recurring(
            &charges(&series("2025-10-15", 91, 4), 3_000),
            d("2026-08-01"),
        )
        .unwrap();
        assert_eq!(quarterly.cadence, Cadence::Quarterly);
        let yearly = [d("2024-03-02"), d("2025-03-04"), d("2026-03-03")];
        let r = detect_recurring(&charges(&yearly, 12_000), d("2026-09-30")).unwrap();
        assert_eq!((r.cadence, r.yearly_cents()), (Cadence::Yearly, 12_000));

        // Two charges are not a pattern; irregular gaps are not either.
        assert_eq!(
            detect_recurring(&charges(&monthly[..2], 1_699), d("2026-02-28")),
            None
        );
        let irregular = [
            d("2026-01-03"),
            d("2026-01-20"),
            d("2026-03-09"),
            d("2026-03-11"),
            d("2026-07-01"),
        ];
        assert_eq!(
            detect_recurring(&charges(&irregular, 500), d("2026-07-01")),
            None
        );
    }

    #[test]
    fn recurring_payments_allow_a_price_change_but_not_varying_amounts() {
        let monthly: Vec<NaiveDate> = (1..=9).map(|m| d(&format!("2026-{m:02}-12"))).collect();
        let mut raised = charges(&monthly, 1_699);
        for c in raised.iter_mut().skip(6) {
            c.1 = 1_899;
        }
        assert!(detect_recurring(&raised, d("2026-09-30")).is_some());
        let groceries: Vec<(NaiveDate, i64)> = series("2026-01-03", 7, 30)
            .into_iter()
            .zip([4_000, 12_000, 7_500, 21_000, 3_000].into_iter().cycle())
            .collect();
        assert_eq!(detect_recurring(&groceries, d("2026-07-31")), None);
    }

    #[test]
    fn recurring_payments_that_stopped_are_dropped() {
        let monthly: Vec<NaiveDate> = (1..=4).map(|m| d(&format!("2026-{m:02}-05"))).collect();
        assert!(detect_recurring(&charges(&monthly, 999), d("2026-05-20")).is_some());
        assert_eq!(
            detect_recurring(&charges(&monthly, 999), d("2026-06-30")),
            None
        );
    }

    #[test]
    fn income_suggestions_follow_patterns() {
        let months: Vec<String> = (0..24)
            .map(|i| format!("{}-{:02}", 2025 + i / 12, i % 12 + 1))
            .collect();
        let mut sources = BTreeMap::new();
        sources.insert("Stipend".to_string(), vec![500; 24]);
        let mut bonus = vec![0; 24];
        bonus[7] = 9_000;
        bonus[19] = 9_500;
        sources.insert("Bonus".to_string(), bonus);
        let mut rsu = vec![0; 24];
        for i in [14, 17, 20, 23] {
            rsu[i] = 1_000;
        }
        sources.insert("Stock".to_string(), rsu);
        let s = suggest_income(&months, &sources);
        assert!(s.contains(&Suggestion {
            name: "Stipend".into(),
            pattern: "monthly",
            months: (1..=12).collect(),
            amount_cents: 500
        }));
        assert!(s.contains(&Suggestion {
            name: "Bonus".into(),
            pattern: "yearly",
            months: vec![8],
            amount_cents: 9_500
        }));
        assert!(s.contains(&Suggestion {
            name: "Stock".into(),
            pattern: "quarterly",
            months: vec![3, 6, 9, 12],
            amount_cents: 1_000
        }));
    }
}
