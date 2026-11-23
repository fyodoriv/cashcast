//! Inputs, history and the forecast computed without a spreadsheet.
//!
//! `build` writes spreadsheet formulas. This module computes the same
//! forecast without them, so `verify` can compare the two to the cent.
//!
//! Inputs come from two files in the data folder:
//! - data/summary.json: aggregates written by the engine (no transactions).
//! - cashcast.toml: the owner's answers. Anything missing gets a default,
//!   and every default is listed in `Inputs::assumptions`.
//!
//! Numbers are decimals, as in the Python model (decision 0010 parity).

use crate::core::pyjson::{Json, float_repr};
use chrono::{Datelike, Duration, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};
use std::str::FromStr;

pub const METHODS: [&str; 4] = [
    "trailing-12 average",
    "trailing-12 median",
    "same month last year",
    "flat baseline",
];
pub const SCHEDULES: [&str; 5] = ["weekly", "biweekly", "semimonthly", "monthly", "none"];
pub const ZERO: Decimal = Decimal::ZERO;

/// The forecast needs answers it cannot default.
#[derive(Debug)]
pub struct MissingInputs(pub Vec<String>);

/// A file the forecast cannot read: the message is printed as Python would.
#[derive(Debug)]
pub struct Failure(pub String);

/// Spreadsheet ROUND(x, 2): half away from zero.
pub fn round2(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

pub fn first_of_month(y: i32, m: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid month")
}

pub fn parse_month(text: &str) -> Option<NaiveDate> {
    let mut parts = text.split('-');
    let y = parts.next()?.trim().parse().ok()?;
    let m = parts.next()?.trim().parse().ok()?;
    NaiveDate::from_ymd_opt(y, m, 1)
}

pub fn add_months(day: NaiveDate, n: i32) -> NaiveDate {
    let total = day.year() * 12 + day.month0() as i32 + n;
    first_of_month(total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}

/// `%b %Y`, e.g. "Oct 2026".
pub fn month_label(d: NaiveDate) -> String {
    d.format("%b %Y").to_string()
}

/// Python's `Decimal(str(value))` for a TOML value.
pub fn dec(v: &toml::Value) -> Option<Decimal> {
    match v {
        toml::Value::Integer(i) => Some(Decimal::from(*i)),
        toml::Value::Float(f) => dec_str(&float_repr(*f)),
        toml::Value::String(s) => dec_str(s),
        _ => None,
    }
}

pub fn dec_str(s: &str) -> Option<Decimal> {
    let t = s.trim();
    Decimal::from_str(t)
        .ok()
        .or_else(|| Decimal::from_scientific(t).ok())
}

pub fn cents_to_dollars(c: i64) -> Decimal {
    Decimal::from(c) / Decimal::from(100)
}

fn json_int(v: Option<&Json>) -> Option<i64> {
    match v? {
        Json::Int(i) => Some(*i),
        Json::Float(f) => Some(*f as i64),
        _ => None,
    }
}

/// One extra-income line. `from` and `until` (first days of months) bound the
/// months it pays; `known` is when it became known (a grant or lease date), so
/// a backtest from an earlier origin leaves it out.
#[derive(Clone, Debug, Default)]
pub struct ExtraItem {
    pub name: String,
    pub months: Vec<i64>,
    pub amount: Decimal,
    pub from: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub known: Option<NaiveDate>,
}

#[derive(Clone, Debug)]
pub struct History {
    pub months: Vec<NaiveDate>,
    pub salary: Vec<Decimal>,
    pub salary_off_cycle: Vec<Decimal>,
    /// Income by type, in summary order.
    pub income: Vec<(String, Vec<Decimal>)>,
    /// Spending by category, in summary order.
    pub spending: Vec<(String, Vec<Decimal>)>,
    pub income_tax: Vec<Decimal>,
    pub invested: Vec<Decimal>,
}

impl History {
    /// The 12 months before `start`, or before the month after the history ends when the
    /// history stops earlier (the import leaves out months the data does not cover), so
    /// every calendar month appears once.
    pub fn last12(&self, start: NaiveDate) -> Vec<usize> {
        let end = self
            .months
            .last()
            .map_or(start, |m| add_months(*m, 1).min(start));
        let window: Vec<NaiveDate> = (0..12).map(|i| add_months(end, -12 + i)).collect();
        (0..self.months.len())
            .filter(|i| window.contains(&self.months[*i]))
            .collect()
    }

    pub fn monthly_spending(&self, start: NaiveDate) -> Decimal {
        let idx = self.last12(start);
        let mut total = ZERO;
        for (_, s) in &self.spending {
            for &i in &idx {
                total += s[i];
            }
        }
        total / Decimal::from(12)
    }

    pub fn spend(&self, cat: &str) -> Option<&Vec<Decimal>> {
        self.spending.iter().find(|(c, _)| c == cat).map(|(_, v)| v)
    }
}

#[derive(Clone, Debug)]
pub struct Inputs {
    pub start: NaiveDate,
    pub horizon: i64,
    pub opening_accounts: Vec<(String, Decimal)>,
    pub card_balances: Vec<(String, Decimal)>,
    pub schedule: String,
    pub net_h1: Decimal,
    pub net_h2: Decimal,
    pub anchor: Option<NaiveDate>,
    pub holidays: Vec<(String, NaiveDate)>,
    pub rent_category: Option<String>,
    pub rent_charged: Option<Decimal>,
    pub floor_parts: Vec<(String, Decimal)>,
    pub method: String,
    pub baseline: Decimal,
    pub skip_categories: Vec<String>,
    pub extra_income: Vec<ExtraItem>,
    pub taxes: Vec<(String, Decimal)>,
    pub base_currency: String,
    pub assumptions: Vec<String>,
}

impl Inputs {
    pub fn opening_cash(&self) -> Decimal {
        self.opening_accounts.iter().map(|(_, v)| *v).sum()
    }
    pub fn floor(&self) -> Decimal {
        self.floor_parts.iter().map(|(_, v)| *v).sum()
    }
    pub fn period_days(&self) -> i64 {
        if self.schedule == "weekly" { 7 } else { 14 }
    }
    pub fn rent(&self) -> Decimal {
        self.rent_charged.unwrap_or(ZERO)
    }
    pub fn tax(&self, key: &str) -> Decimal {
        self.taxes
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map_or(ZERO, |(_, v)| *v)
    }
}

fn series(v: Option<&Json>) -> Result<Vec<Decimal>, Failure> {
    match v {
        Some(Json::List(items)) => items
            .iter()
            .map(|x| {
                json_int(Some(x))
                    .map(cents_to_dollars)
                    .ok_or_else(|| Failure("bad summary series".into()))
            })
            .collect(),
        _ => Err(Failure("KeyError in summary.json".into())),
    }
}

fn named_series(v: Option<&Json>) -> Result<Vec<(String, Vec<Decimal>)>, Failure> {
    match v {
        Some(Json::Obj(items)) => items
            .iter()
            .map(|(k, s)| Ok((k.clone(), series(Some(s))?)))
            .collect(),
        _ => Err(Failure("KeyError in summary.json".into())),
    }
}

pub fn load_history(summary: &Json) -> Result<History, Failure> {
    let flows = summary.get("flows_cents");
    let flow = |k: &str| series(flows.and_then(|f| f.get(k)));
    let months = match summary.get("months") {
        Some(Json::List(items)) => items
            .iter()
            .map(|m| match m {
                Json::Str(s) => parse_month(s).ok_or_else(|| Failure("bad month".into())),
                _ => Err(Failure("bad month".into())),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(Failure("KeyError in summary.json".into())),
    };
    Ok(History {
        months,
        salary: flow("salary")?,
        salary_off_cycle: flow("salary_off_cycle")?,
        income: named_series(summary.get("income_cents"))?,
        spending: named_series(summary.get("spending_cents"))?,
        income_tax: flow("income_tax")?,
        invested: flow("invested")?,
    })
}

fn str_of(v: Option<&Json>) -> Option<String> {
    match v? {
        Json::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn table<'a>(t: &'a toml::Table, key: &str) -> Option<&'a toml::Table> {
    t.get(key).and_then(toml::Value::as_table)
}

/// `f"{value:,.2f}"` and friends: Python's decimal format, half-even.
pub fn fmt(v: Decimal, dp: u32, plus: bool) -> String {
    let r = v.round_dp_with_strategy(dp, RoundingStrategy::MidpointNearestEven);
    let neg = v.is_sign_negative() && !v.is_zero();
    let s = format!("{:.*}", dp as usize, r.abs());
    let (int, frac) = s.split_once('.').map_or((s.as_str(), ""), |(a, b)| (a, b));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if neg {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    };
    if dp > 0 {
        format!("{sign}{grouped}.{frac}")
    } else {
        format!("{sign}{grouped}")
    }
}

pub fn money(v: Decimal) -> String {
    fmt(v, 2, false)
}

fn date_of(v: &toml::Value) -> Option<NaiveDate> {
    match v {
        toml::Value::String(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
        toml::Value::Datetime(dt) if dt.time.is_none() => {
            let d = dt.date?;
            NaiveDate::from_ymd_opt(d.year as i32, d.month as u32, d.day as u32)
        }
        _ => None,
    }
}

/// Merges the owner's answers with what the engine detected.
pub fn inputs_from(
    cfg: &toml::Table,
    summary: &Json,
    today: NaiveDate,
) -> Result<Result<Inputs, MissingInputs>, Failure> {
    let bad = |what: &str| Failure(format!("cashcast.toml: {what} has the wrong type"));
    let empty = toml::Table::new();
    let mut notes = Vec::new();
    let mut problems = Vec::new();
    let fc = table(cfg, "forecast").unwrap_or(&empty);
    let hist = load_history(summary)?;

    let start = match fc.get("start_month") {
        Some(toml::Value::String(s)) => parse_month(s).ok_or_else(|| bad("start_month"))?,
        Some(_) => return Err(bad("start_month")),
        None => first_of_month(today.year(), today.month()),
    };
    if !fc.contains_key("start_month") {
        notes.push(format!(
            "Forecast starts this month ({}).",
            month_label(start)
        ));
    }
    let horizon = match fc.get("horizon_months") {
        None => 15,
        Some(toml::Value::Integer(i)) => *i,
        Some(toml::Value::Float(f)) => f.trunc() as i64,
        Some(toml::Value::String(s)) => s.trim().parse().map_err(|_| bad("horizon_months"))?,
        Some(_) => return Err(bad("horizon_months")),
    };

    // Opening cash: config balances win, then balances found in the exports.
    let accounts: Vec<&Json> = match summary.get("accounts") {
        Some(Json::List(a)) => a.iter().collect(),
        _ => Vec::new(),
    };
    let mut given: Vec<(String, Decimal)> = Vec::new();
    if let Some(b) = table(cfg, "balances") {
        for (k, v) in b {
            given.push((k.clone(), dec(v).ok_or_else(|| bad("balances"))?));
        }
    }
    let lookup =
        |list: &[(String, Decimal)], k: &str| list.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
    let mut opening: Vec<(String, Decimal)> = Vec::new();
    let put = |list: &mut Vec<(String, Decimal)>, k: &str, v: Decimal| match list
        .iter_mut()
        .find(|(n, _)| n == k)
    {
        Some(slot) => slot.1 = v,
        None => list.push((k.to_string(), v)),
    };
    for a in &accounts {
        if str_of(a.get("class")).as_deref() != Some("cash") {
            continue;
        }
        let name = str_of(a.get("name")).unwrap_or_default();
        if let Some(v) = lookup(&given, &name) {
            put(&mut opening, &name, v);
        } else if let Some(c) = json_int(a.get("balance_cents")) {
            put(&mut opening, &name, cents_to_dollars(c));
            let source = a
                .get("balance_source")
                .map(Json::py_str)
                .unwrap_or_else(|| "None".into());
            notes.push(format!(
                "Opening balance of {name} comes from the export ({source})."
            ));
        } else {
            problems.push(format!("balance needed: {name}"));
        }
    }
    for (name, v) in &given {
        if lookup(&opening, name).is_none() {
            opening.push((name.clone(), *v));
        }
    }
    let mut cards: Vec<(String, Decimal)> = Vec::new();
    for a in &accounts {
        if str_of(a.get("class")).as_deref() != Some("card") {
            continue;
        }
        let c = json_int(a.get("balance_cents"))
            .or_else(|| json_int(a.get("sum_balance_cents")))
            .unwrap_or(0);
        put(
            &mut cards,
            &str_of(a.get("name")).unwrap_or_default(),
            cents_to_dollars(c),
        );
    }

    let pay_cfg = table(cfg, "pay").unwrap_or(&empty);
    let pay = summary.get("pay");
    let cfg_str = |k: &str| {
        pay_cfg
            .get(k)
            .and_then(toml::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let schedule = cfg_str("schedule")
        .or_else(|| str_of(pay.and_then(|p| p.get("schedule"))).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "none".into());
    if !SCHEDULES.contains(&schedule.as_str()) {
        problems.push(format!(
            "pay.schedule must be one of ('weekly', 'biweekly', 'semimonthly', 'monthly', 'none')"
        ));
    }
    let anchor = match pay_cfg.get("anchor") {
        Some(v) if !matches!(v, toml::Value::String(s) if s.is_empty()) => {
            Some(date_of(v).ok_or_else(|| bad("pay.anchor"))?)
        }
        _ => match str_of(pay.and_then(|p| p.get("anchor"))).filter(|s| !s.is_empty()) {
            Some(s) => {
                Some(NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(|_| bad("pay.anchor"))?)
            }
            None => None,
        },
    };
    if (schedule == "weekly" || schedule == "biweekly") && anchor.is_none() {
        problems.push("pay.anchor needed: a recent payday as YYYY-MM-DD".into());
    }
    let mut halves: Vec<(String, Decimal)> = Vec::new();
    if let Some(Json::List(hs)) = pay.and_then(|p| p.get("half_years")) {
        for h in hs {
            let period = h.get("period").map(Json::py_str).unwrap_or_default();
            let v = cents_to_dollars(json_int(h.get("median_cents")).unwrap_or(0));
            put(&mut halves, &period, v);
        }
    }
    halves.sort_by(|a, b| a.0.cmp(&b.0));
    let latest = |half: &str| {
        halves
            .iter()
            .filter(|(k, _)| k.ends_with(half))
            .map(|(_, v)| *v)
            .next_back()
    };
    let net_h1 = match pay_cfg.get("net_h1") {
        Some(v) => dec(v).ok_or_else(|| bad("pay.net_h1"))?,
        None => latest("H1").or_else(|| latest("H2")).unwrap_or(ZERO),
    };
    let net_h2 = match pay_cfg.get("net_h2") {
        Some(v) => dec(v).ok_or_else(|| bad("pay.net_h2"))?,
        None => latest("H2").or_else(|| latest("H1")).unwrap_or(ZERO),
    };
    if schedule == "none" {
        notes.push(
            "No regular pay found, so the forecast has no salary. Set [pay] to add it.".into(),
        );
    } else if !pay_cfg.contains_key("net_h1") || !pay_cfg.contains_key("net_h2") {
        notes.push(format!(
            "Net pay per cheque: {} (Jan-Jun) and {} (Jul-Dec), the latest medians found.",
            money(net_h1),
            money(net_h2)
        ));
    }

    let holidays = match pay_cfg.get("holidays") {
        None => {
            let (y0, y1) = (start.year(), add_months(start, horizon as i32).year() + 2);
            let mut h: Vec<(String, NaiveDate)> = (y0..y1)
                .map(|y| (format!("New Year {y}"), first_of_month(y, 1)))
                .collect();
            h.extend((y0..y1).map(|y| {
                (
                    format!("Christmas {y}"),
                    NaiveDate::from_ymd_opt(y, 12, 25).expect("date"),
                )
            }));
            h
        }
        Some(toml::Value::Table(t)) => t
            .iter()
            .map(|(k, v)| {
                date_of(v)
                    .map(|d| (k.clone(), d))
                    .ok_or_else(|| bad("pay.holidays"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(bad("pay.holidays")),
    };

    let rent_cfg = table(cfg, "rent").unwrap_or(&empty);
    let rent_charged = match rent_cfg.get("charged") {
        Some(v) => Some(dec(v).ok_or_else(|| bad("rent.charged"))?),
        None => None,
    };
    let rent_category = if rent_charged.is_some() {
        rent_cfg
            .get("category")
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    } else {
        None
    };

    let parts = match cfg.get("floor") {
        Some(toml::Value::Table(floor)) => {
            let mut parts = Vec::new();
            for (group, items) in floor {
                if let toml::Value::Table(items) = items {
                    for (k, v) in items {
                        put(
                            &mut parts,
                            &format!("{group}: {k}"),
                            dec(v).ok_or_else(|| bad("floor"))?,
                        );
                    }
                } else {
                    put(&mut parts, group, dec(items).ok_or_else(|| bad("floor"))?);
                }
            }
            parts
        }
        _ if fc.contains_key("floor") => vec![(
            "Cash floor".to_string(),
            dec(&fc["floor"]).ok_or_else(|| bad("forecast.floor"))?,
        )],
        _ => {
            let spend = hist.monthly_spending(start) * Decimal::from(3) / Decimal::from(100);
            let default = spend.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
                * Decimal::from(100);
            notes.push(format!(
                "Cash floor defaults to 3 months of spending: {}.",
                fmt(default, 0, false)
            ));
            vec![("Cash floor (3 months of spending)".to_string(), default)]
        }
    };

    let method = match fc.get("method") {
        None => "trailing-12 average".to_string(),
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string()),
    };
    if !METHODS.contains(&method.as_str()) {
        problems.push("forecast.method must be one of ('trailing-12 average', 'trailing-12 median', 'same month last year', 'flat baseline')".into());
    }
    let mut extra: Vec<ExtraItem> = Vec::new();
    if let Some(items) = table(cfg, "extra_income") {
        for (name, item) in items {
            let item = item.as_table().ok_or_else(|| bad("extra_income"))?;
            let months = match item.get("months") {
                Some(toml::Value::Array(ms)) => ms
                    .iter()
                    .map(|m| match m {
                        toml::Value::Integer(i) => Ok(*i),
                        toml::Value::Float(f) => Ok(f.trunc() as i64),
                        toml::Value::String(s) => {
                            s.trim().parse().map_err(|_| bad("extra_income months"))
                        }
                        _ => Err(bad("extra_income months")),
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err(bad("extra_income months")),
            };
            let amount = item
                .get("amount")
                .and_then(dec)
                .ok_or_else(|| bad("extra_income amount"))?;
            let ym = |k: &str| -> Result<Option<NaiveDate>, _> {
                match item.get(k) {
                    None => Ok(None),
                    Some(toml::Value::String(v)) => {
                        NaiveDate::parse_from_str(&format!("{}-01", v.trim()), "%Y-%m-%d")
                            .map(Some)
                            .map_err(|_| bad("extra_income dates (YYYY-MM)"))
                    }
                    _ => Err(bad("extra_income dates (YYYY-MM)")),
                }
            };
            extra.push(ExtraItem {
                name: name.clone(),
                months,
                amount,
                from: ym("from")?,
                until: ym("until")?,
                known: ym("known")?,
            });
        }
    }
    let truthy = |v: Option<&toml::Value>| match v {
        None => false,
        Some(toml::Value::Boolean(b)) => *b,
        Some(toml::Value::Integer(i)) => *i != 0,
        Some(toml::Value::Float(f)) => *f != 0.0,
        Some(toml::Value::String(s)) => !s.is_empty(),
        Some(toml::Value::Array(a)) => !a.is_empty(),
        Some(toml::Value::Table(t)) => !t.is_empty(),
        Some(toml::Value::Datetime(_)) => true,
    };
    if truthy(fc.get("use_suggestions")) {
        let names: Vec<String> = extra.iter().map(|x| x.name.clone()).collect();
        if let Some(Json::List(sugg)) = summary.get("suggested_extra_income") {
            for s in sugg {
                let name = str_of(s.get("name")).unwrap_or_default();
                if !names.contains(&name) {
                    let months = match s.get("months") {
                        Some(Json::List(ms)) => {
                            ms.iter().filter_map(|m| json_int(Some(m))).collect()
                        }
                        _ => Vec::new(),
                    };
                    extra.push(ExtraItem {
                        name,
                        months,
                        amount: cents_to_dollars(json_int(s.get("amount_cents")).unwrap_or(0)),
                        ..Default::default()
                    });
                }
            }
        }
        notes.push("Recurring extra income uses the engine's suggestions.".into());
    }

    if !problems.is_empty() {
        return Ok(Err(MissingInputs(problems)));
    }
    let mut taxes = Vec::new();
    if let Some(t) = table(cfg, "taxes") {
        for (k, v) in t {
            taxes.push((k.clone(), dec(v).ok_or_else(|| bad("taxes"))?));
        }
    }
    let skip = match fc.get("skip") {
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            })
            .collect(),
        _ => Vec::new(),
    };
    let baseline = match fc.get("baseline") {
        Some(v) => dec(v).ok_or_else(|| bad("forecast.baseline"))?,
        None => ZERO,
    };
    Ok(Ok(Inputs {
        start,
        horizon,
        opening_accounts: opening,
        card_balances: cards,
        schedule,
        net_h1,
        net_h2,
        anchor,
        holidays,
        rent_category,
        rent_charged,
        floor_parts: parts,
        method,
        baseline,
        skip_categories: skip,
        extra_income: extra,
        taxes,
        base_currency: str_of(summary.get("base_currency")).unwrap_or_default(),
        assumptions: notes,
    }))
}

/// Rent first (when set), then categories with spending in the last 12 months.
pub fn forecast_categories(inputs: &Inputs, hist: &History) -> Vec<String> {
    let idx = hist.last12(inputs.start);
    let mut totals: Vec<(String, Decimal)> = hist
        .spending
        .iter()
        .map(|(c, s)| (c.clone(), idx.iter().map(|&i| s[i]).sum()))
        .collect();
    totals.sort_by(|a, b| b.1.cmp(&a.1)); // stable, like Python's sorted
    let rest: Vec<String> = totals
        .into_iter()
        .filter(|(c, t)| {
            Some(c) != inputs.rent_category.as_ref()
                && !inputs.skip_categories.contains(c)
                && !t.is_zero()
        })
        .map(|(c, _)| c)
        .collect();
    match &inputs.rent_category {
        Some(r) => std::iter::once(r.clone()).chain(rest).collect(),
        None => rest,
    }
}

/// Weekly or biweekly paydays from the first one on or after the start,
/// moved to the day before when they fall on a holiday.
pub fn paydays(inputs: &Inputs, count: i64) -> Vec<NaiveDate> {
    let Some(anchor) = inputs.anchor else {
        return Vec::new();
    };
    let period = inputs.period_days();
    let gap = (inputs.start - anchor).num_days();
    let k0 = -((-gap).div_euclid(period)); // ceiling division, like ROUNDUP
    (0..count)
        .map(|k| {
            let nominal = anchor + Duration::days(period * (k0 + k));
            if inputs.holidays.iter().any(|(_, d)| *d == nominal) {
                nominal - Duration::days(1)
            } else {
                nominal
            }
        })
        .collect()
}

pub fn cheques_in(inputs: &Inputs, month: NaiveDate, pays: &[NaiveDate]) -> i64 {
    match inputs.schedule.as_str() {
        "monthly" => 1,
        "semimonthly" => 2,
        "none" => 0,
        _ => pays
            .iter()
            .filter(|p| p.year() == month.year() && p.month() == month.month())
            .count() as i64,
    }
}

pub fn spending_excl_rent(inputs: &Inputs, hist: &History, idx: &[usize]) -> Decimal {
    let rent = if inputs.rent_charged.is_some() {
        inputs.rent_category.as_deref()
    } else {
        None
    };
    let mut total = ZERO;
    for (c, s) in &hist.spending {
        if Some(c.as_str()) == rent {
            continue;
        }
        for &i in idx {
            total += s[i];
        }
    }
    total / Decimal::from(12)
}

/// Median of the 12 months before the start; a month with no history
/// counts as 0, as it does in the average. One-off spike months move the
/// average but not the median (the L1-optimal point forecast for WAPE).
pub fn median12(s: &[Decimal], idx: &[usize]) -> Decimal {
    let mut v: Vec<Decimal> = idx.iter().map(|&i| s[i]).collect();
    v.resize(12.max(v.len()), ZERO);
    v.sort();
    let mid = v.len() / 2;
    (v[mid - 1] + v[mid]) / Decimal::from(2)
}

#[derive(Clone, Debug)]
pub struct Row {
    pub month: NaiveDate,
    pub paycheques: i64,
    pub net_salary: Decimal,
    pub extra_income: Decimal,
    pub income: Decimal,
    pub spending: Vec<(String, Decimal)>,
    pub untracked: Decimal,
    pub expenses: Decimal,
    pub taxes: Decimal,
    pub net_cash_flow: Decimal,
    pub month_start: Decimal,
    pub before_investing: Decimal,
    pub floor: Decimal,
    pub to_invest: Decimal,
    pub month_end: Decimal,
}

pub fn run(inputs: &Inputs, hist: &History) -> Result<Vec<Row>, Failure> {
    let idx = hist.last12(inputs.start);
    let cats = forecast_categories(inputs, hist);
    let pays = paydays(inputs, 80);
    let t12 = spending_excl_rent(inputs, hist, &idx);
    let scale = if inputs.method == "flat baseline" {
        if t12.is_zero() {
            return Err(Failure(
                "decimal.DivisionByZero: no spending in the last 12 months to scale".into(),
            ));
        }
        (inputs.baseline - inputs.rent()) / t12
    } else {
        Decimal::ONE
    };
    let floor = inputs.floor();
    let mut rows = Vec::new();
    let mut cash = inputs.opening_cash();
    for n in 0..inputs.horizon {
        let month = add_months(inputs.start, n as i32);
        let cheques = cheques_in(inputs, month, &pays);
        let net_salary = Decimal::from(cheques)
            * if month.month() <= 6 {
                inputs.net_h1
            } else {
                inputs.net_h2
            };
        let extra: Decimal = inputs
            .extra_income
            .iter()
            .filter(|x| {
                x.months.contains(&(month.month() as i64))
                    && x.from.is_none_or(|f| month >= f)
                    && x.until.is_none_or(|u| month <= u)
                    && x.known.is_none_or(|k| inputs.start >= k)
            })
            .map(|x| x.amount)
            .sum();
        let mut spend: Vec<(String, Decimal)> = Vec::new();
        for c in &cats {
            if Some(c) == inputs.rent_category.as_ref() {
                spend.push((c.clone(), inputs.rent()));
                continue;
            }
            let s = hist.spend(c).expect("category from history");
            let base = if inputs.method == "same month last year" {
                idx.iter()
                    .filter(|&&i| hist.months[i].month() == month.month())
                    .map(|&i| s[i])
                    .sum()
            } else if inputs.method == "trailing-12 median" {
                median12(s, &idx)
            } else {
                idx.iter().map(|&i| s[i]).sum::<Decimal>() / Decimal::from(12)
            };
            spend.push((c.clone(), round2(base * scale)));
        }
        let categories_total: Decimal = spend.iter().map(|(_, v)| *v).sum();
        let untracked = (inputs.baseline - categories_total).max(ZERO);
        let expenses = categories_total + untracked;
        let taxes = inputs.tax(&month.format("%Y-%m").to_string());
        let income = net_salary + extra;
        let net = income - expenses + taxes;
        // Pass 1: the path with no investing; pass 2 below sets to_invest.
        let before = cash + net;
        let to_invest = ZERO;
        let end = before;
        rows.push(Row {
            month,
            paycheques: cheques,
            net_salary,
            extra_income: extra,
            income,
            spending: spend,
            untracked,
            expenses,
            taxes,
            net_cash_flow: net,
            month_start: cash,
            before_investing: before,
            floor,
            to_invest,
            month_end: end,
        });
        cash = end;
    }
    // Pass 2: invest only cash that no later month needs. Money invested in
    // month k leaves every later month-end too, so the total invested by k
    // is capped by the lowest headroom above the floor from k onward.
    let mut later_min = vec![ZERO; rows.len()];
    let mut low: Option<Decimal> = None;
    for (k, r) in rows.iter().enumerate().rev() {
        let m = low.map_or(r.before_investing, |l| l.min(r.before_investing));
        later_min[k] = m;
        low = Some(m);
    }
    let mut invested = ZERO;
    for (k, r) in rows.iter_mut().enumerate() {
        let total = invested.max(later_min[k] - floor);
        r.to_invest = total - invested;
        r.month_start -= invested;
        r.before_investing -= invested;
        r.month_end = r.before_investing - r.to_invest;
        invested = total;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_python_decimal() {
        let d = |s: &str| Decimal::from_str(s).unwrap();
        assert_eq!(fmt(d("1234567.125"), 2, false), "1,234,567.12");
        assert_eq!(fmt(d("-0.3"), 0, true), "-0");
        assert_eq!(fmt(d("2.5"), 0, true), "+2");
        assert_eq!(fmt(d("0"), 0, true), "+0");
        assert_eq!(round2(d("2.345")), d("2.35"));
        assert_eq!(round2(d("-2.345")), d("-2.35"));
        assert_eq!(
            add_months(first_of_month(2026, 1), -12),
            first_of_month(2025, 1)
        );
        assert_eq!(
            add_months(first_of_month(2026, 11), 3),
            first_of_month(2027, 2)
        );
    }
}
