//! Forecast vs actual, as `forecast/backtest.py`.
//!
//! - Snapshots: each `forecast` run saves its monthly lines to
//!   data/forecasts/<made>.json, so a later run can score them.
//! - Backtest: forecast from each past month (rolling origin) with only the
//!   history before it, then compare with what happened (Hyndman and
//!   Athanasopoulos, FPP3 5.10).

use super::model::{Failure, History, Inputs, Row, ZERO, run};
use crate::core::pyjson::{self, Json};
use chrono::NaiveDate;
use rust_decimal::{Decimal, RoundingStrategy};
use std::path::{Path, PathBuf};

pub const HORIZON: i64 = 6;
pub const MIN_HISTORY: usize = 12;
pub const LINES: [&str; 6] = [
    "net_salary",
    "extra_income",
    "rent",
    "spending_excl_rent",
    "taxes",
    "net_cash_flow",
];
pub const LABELS: [(&str, &str); 7] = [
    ("net_salary", "Net salary"),
    ("extra_income", "Extra income"),
    ("rent", "Rent"),
    ("spending_excl_rent", "Spending excl. rent"),
    ("taxes", "Taxes"),
    ("net_cash_flow", "Net cash flow"),
    ("month_end", "Month end (cumulative net)"),
];

pub fn label(line: &str) -> &'static str {
    LABELS
        .iter()
        .find(|(k, _)| *k == line)
        .map_or("", |(_, v)| v)
}

/// Lines for one month: the six `LINES` in order, and spending per category.
pub struct Lines {
    pub vals: [Decimal; 6],
    pub spending: Vec<(String, Decimal)>,
}

impl Lines {
    pub fn get(&self, line: &str) -> Decimal {
        LINES
            .iter()
            .position(|l| *l == line)
            .map_or(ZERO, |i| self.vals[i])
    }
}

fn find(v: &[(String, Decimal)], k: &str) -> Option<Decimal> {
    v.iter().rev().find(|(c, _)| c == k).map(|(_, x)| *x)
}

pub fn truncate(hist: &History, end: NaiveDate) -> History {
    let keep: Vec<usize> = (0..hist.months.len())
        .filter(|&i| hist.months[i] < end)
        .collect();
    let cut = |s: &Vec<Decimal>| keep.iter().map(|&i| s[i]).collect::<Vec<_>>();
    History {
        months: keep.iter().map(|&i| hist.months[i]).collect(),
        salary: cut(&hist.salary),
        salary_off_cycle: cut(&hist.salary_off_cycle),
        income: hist
            .income
            .iter()
            .map(|(k, v)| (k.clone(), cut(v)))
            .collect(),
        spending: hist
            .spending
            .iter()
            .map(|(k, v)| (k.clone(), cut(v)))
            .collect(),
        income_tax: cut(&hist.income_tax),
        invested: cut(&hist.invested),
    }
}

pub fn actual_lines(hist: &History, i: usize, rent_category: Option<&str>) -> Lines {
    let extra = hist
        .income
        .iter()
        .fold(hist.salary_off_cycle[i], |a, (_, v)| a + v[i]);
    let spending: Vec<(String, Decimal)> = hist
        .spending
        .iter()
        .map(|(c, v)| (c.clone(), v[i]))
        .collect();
    let rent = rent_category
        .and_then(|r| find(&spending, r))
        .unwrap_or(ZERO);
    let total = spending.iter().fold(ZERO, |a, (_, v)| a + v);
    Lines {
        vals: [
            hist.salary[i],
            extra,
            rent,
            total - rent,
            hist.income_tax[i],
            hist.salary[i] + extra - total + hist.income_tax[i],
        ],
        spending,
    }
}

/// Drops skipped categories (owner-confirmed one-offs) from actual spending,
/// so the backtest scores only what the forecast is meant to predict.
fn without_skipped(mut a: Lines, skip: &[String]) -> Lines {
    let gone = a
        .spending
        .iter()
        .filter(|(c, _)| skip.contains(c))
        .fold(ZERO, |s, (_, v)| s + v);
    a.spending.retain(|(c, _)| !skip.contains(c));
    a.vals[3] -= gone;
    a.vals[5] += gone;
    a
}

pub fn forecast_lines(row: &Row, rent_category: Option<&str>) -> Lines {
    let mut spending = row.spending.clone();
    if !row.untracked.is_zero() {
        spending.push(("(untracked)".into(), row.untracked));
    }
    let rent = rent_category
        .and_then(|r| find(&spending, r))
        .unwrap_or(ZERO);
    Lines {
        vals: [
            row.net_salary,
            row.extra_income,
            rent,
            row.expenses - rent,
            row.taxes,
            row.net_cash_flow,
        ],
        spending,
    }
}

/// Forecast minus actual for one line, one origin, one month ahead.
pub struct Miss {
    pub origin: NaiveDate,
    pub ahead: i64,
    pub line: String,
    pub forecast: Decimal,
    pub actual: Decimal,
}

impl Miss {
    pub fn error(&self) -> Decimal {
        self.forecast - self.actual
    }
}

pub fn origins(hist: &History) -> Vec<NaiveDate> {
    hist.months
        .iter()
        .copied()
        .filter(|m| hist.months.iter().filter(|x| *x < m).count() >= MIN_HISTORY)
        .collect()
}

fn forecast_from(inputs: &Inputs, hist: &History, origin: NaiveDate) -> Result<Vec<Row>, Failure> {
    let after = hist.months.iter().filter(|m| **m >= origin).count() as i64;
    let mut inp = inputs.clone();
    inp.start = origin;
    inp.horizon = HORIZON.min(after);
    run(&inp, &truncate(hist, origin))
}

/// Rolling-origin misses per line, plus one per category ("cat:<name>").
pub fn backtest(inputs: &Inputs, hist: &History) -> Result<Vec<Miss>, Failure> {
    let at = |m: NaiveDate| hist.months.iter().position(|x| *x == m);
    let rent = inputs.rent_category.as_deref();
    let mut out = Vec::new();
    for origin in origins(hist) {
        let rows = forecast_from(inputs, hist, origin)?;
        let (mut cum_f, mut cum_a) = (ZERO, ZERO);
        for (k, row) in rows.iter().enumerate() {
            let Some(i) = at(row.month) else { continue };
            let ahead = k as i64 + 1;
            let f = forecast_lines(row, rent);
            let a = without_skipped(actual_lines(hist, i, rent), &inputs.skip_categories);
            for (j, line) in LINES.iter().enumerate() {
                out.push(Miss {
                    origin,
                    ahead,
                    line: line.to_string(),
                    forecast: f.vals[j],
                    actual: a.vals[j],
                });
            }
            cum_f += f.get("net_cash_flow");
            cum_a += a.get("net_cash_flow");
            out.push(Miss {
                origin,
                ahead,
                line: "month_end".into(),
                forecast: cum_f,
                actual: cum_a,
            });
            // Python walks a set here; this keeps one fixed order (forecast categories first).
            let mut cats: Vec<&String> = f.spending.iter().map(|(c, _)| c).collect();
            for (c, _) in &a.spending {
                if !cats.contains(&c) {
                    cats.push(c);
                }
            }
            for c in cats {
                if Some(c.as_str()) == rent {
                    continue;
                }
                out.push(Miss {
                    origin,
                    ahead,
                    line: format!("cat:{c}"),
                    forecast: find(&f.spending, c).unwrap_or(ZERO),
                    actual: find(&a.spending, c).unwrap_or(ZERO),
                });
            }
        }
    }
    Ok(out)
}

/// Sums each origin's months 1-3 and 4-6 ahead into 3-month totals (full
/// quarters only), so money that lands a few days early or late still counts.
pub fn quarters(misses: &[&Miss]) -> Vec<Miss> {
    let mut out: Vec<Miss> = Vec::new();
    for m in misses {
        let q = (m.ahead - 1) / 3 + 1;
        match out
            .iter_mut()
            .find(|x| x.origin == m.origin && x.ahead == q)
        {
            Some(x) => {
                x.forecast += m.forecast;
                x.actual += m.actual;
            }
            None => out.push(Miss {
                origin: m.origin,
                ahead: q,
                line: m.line.clone(),
                forecast: m.forecast,
                actual: m.actual,
            }),
        }
    }
    let full = |o: NaiveDate, q: i64| {
        misses
            .iter()
            .filter(|m| m.origin == o && (m.ahead - 1) / 3 + 1 == q)
            .count()
            == 3
    };
    out.retain(|x| full(x.origin, x.ahead));
    out
}

pub struct Score {
    pub mean_abs: Decimal,
    pub bias: Decimal,
    pub wape: Option<Decimal>,
}

pub fn score(misses: &[&Miss]) -> Score {
    if misses.is_empty() {
        return Score {
            mean_abs: ZERO,
            bias: ZERO,
            wape: None,
        };
    }
    let abs_sum = misses.iter().fold(ZERO, |a, m| a + m.error().abs());
    let actual = misses.iter().fold(ZERO, |a, m| a + m.actual.abs());
    let n = Decimal::from(misses.len());
    Score {
        mean_abs: abs_sum / n,
        bias: misses.iter().fold(ZERO, |a, m| a + m.error()) / n,
        wape: if actual.is_zero() {
            None
        } else {
            Some(abs_sum / actual)
        },
    }
}

/// Net cash flow WAPE for each forecast method on the same history, so the
/// owner can pick the method that misses least (`forecast.method`).
pub fn by_method(inputs: &Inputs, hist: &History) -> Vec<(&'static str, Option<Decimal>)> {
    crate::forecast::model::METHODS
        .iter()
        .map(|&m| {
            let mut i = inputs.clone();
            i.method = m.to_string();
            let misses = backtest(&i, hist).unwrap_or_default();
            let net: Vec<&Miss> = misses.iter().filter(|x| x.line == "net_cash_flow").collect();
            (m, score(&net).wape)
        })
        .collect()
}

/// Net cash flow WAPE if one line (or one spending category, "cat:<name>")
/// had been forecast exactly: the line's miss is taken out of each net miss.
/// The lines whose number falls most are where accuracy is lost.
pub fn if_exact(misses: &[Miss]) -> Vec<(String, Option<Decimal>)> {
    let sign = |line: &str| match line {
        "net_salary" | "extra_income" | "taxes" => Decimal::ONE,
        _ => -Decimal::ONE,
    };
    let net: Vec<&Miss> = misses.iter().filter(|m| m.line == "net_cash_flow").collect();
    let mut keys: Vec<&str> = Vec::new();
    for m in misses {
        let k = m.line.as_str();
        if k != "net_cash_flow" && k != "month_end" && !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys.into_iter()
        .map(|k| {
            let fixed: Vec<Miss> = net
                .iter()
                .map(|n| {
                    let part = misses
                        .iter()
                        .find(|m| m.line == k && m.origin == n.origin && m.ahead == n.ahead)
                        .map_or(ZERO, |m| m.error());
                    Miss {
                        origin: n.origin,
                        ahead: n.ahead,
                        line: n.line.clone(),
                        forecast: n.forecast - sign(k) * part,
                        actual: n.actual,
                    }
                })
                .collect();
            (k.to_string(), score(&fixed.iter().collect::<Vec<_>>()).wape)
        })
        .collect()
}

pub fn by_line(misses: &[Miss]) -> Vec<(String, Vec<&Miss>)> {
    let mut out: Vec<(String, Vec<&Miss>)> = Vec::new();
    for m in misses {
        match out.iter_mut().find(|(k, _)| *k == m.line) {
            Some(slot) => slot.1.push(m),
            None => out.push((m.line.clone(), vec![m])),
        }
    }
    out
}

/// Python `int((value * 100).to_integral_value())`: half to even.
pub fn cents(v: Decimal) -> i64 {
    (v * Decimal::from(100))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
        .try_into()
        .unwrap_or(0)
}

fn snapshot_dir(home: &Path) -> PathBuf {
    home.join("data").join("forecasts")
}

fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

/// Saves this run's monthly lines (cents), one file per day; a rerun the same day replaces it.
pub fn save_snapshot(
    home: &Path,
    inputs: &Inputs,
    rows: &[Row],
    made: NaiveDate,
) -> Result<PathBuf, Failure> {
    let folder = snapshot_dir(home);
    std::fs::create_dir_all(&folder).map_err(|e| Failure(e.to_string()))?;
    chmod(&folder, 0o700);
    let rent = inputs.rent_category.as_deref();
    let mut months = Vec::new();
    for r in rows {
        let f = forecast_lines(r, rent);
        let mut m = vec![(
            "month".to_string(),
            Json::Str(r.month.format("%Y-%m").to_string()),
        )];
        for (j, line) in LINES.iter().enumerate() {
            m.push((line.to_string(), Json::Int(cents(f.vals[j]))));
        }
        m.push(("month_end".into(), Json::Int(cents(r.month_end))));
        let mut sp: Vec<(String, Json)> = Vec::new();
        for (c, v) in &f.spending {
            match sp.iter_mut().find(|(k, _)| k == c) {
                Some(slot) => slot.1 = Json::Int(cents(*v)),
                None => sp.push((c.clone(), Json::Int(cents(*v)))),
            }
        }
        m.push(("spending".into(), Json::Obj(sp)));
        months.push(Json::Obj(m));
    }
    let data = Json::Obj(vec![
        (
            "made".into(),
            Json::Str(made.format("%Y-%m-%d").to_string()),
        ),
        (
            "start".into(),
            Json::Str(inputs.start.format("%Y-%m").to_string()),
        ),
        ("method".into(), Json::Str(inputs.method.clone())),
        (
            "rent_category".into(),
            rent.map_or(Json::Null, |r| Json::Str(r.into())),
        ),
        ("months".into(), Json::List(months)),
    ]);
    let path = folder.join(format!("{}.json", made.format("%Y-%m-%d")));
    std::fs::write(&path, pyjson::dumps(&data, Some(1), true) + "\n")
        .map_err(|e| Failure(e.to_string()))?;
    chmod(&path, 0o600);
    Ok(path)
}

/// The newest snapshot made before `before` that covers an imported month.
pub fn latest_snapshot(
    home: &Path,
    hist: &History,
    before: NaiveDate,
) -> Result<Option<Json>, Failure> {
    let folder = snapshot_dir(home);
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Ok(None);
    };
    let known: Vec<String> = hist
        .months
        .iter()
        .map(|m| m.format("%Y-%m").to_string())
        .collect();
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .collect();
    paths.sort();
    paths.reverse();
    let cutoff = before.format("%Y-%m-%d").to_string();
    for path in paths {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if stem >= cutoff {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| Failure(e.to_string()))?;
        let data = pyjson::loads(&text).ok_or_else(|| Failure("bad snapshot".into()))?;
        let Some(Json::List(months)) = data.get("months") else {
            return Err(Failure("KeyError: months".into()));
        };
        if months
            .iter()
            .any(|m| matches!(m.get("month"), Some(Json::Str(s)) if known.contains(s)))
        {
            return Ok(Some(data));
        }
    }
    Ok(None)
}

pub struct SnapRow {
    pub month: String,
    pub f: Vec<(String, Decimal)>,
    pub a: Vec<(String, Decimal)>,
    pub categories: Vec<(String, (Decimal, Decimal))>,
}

impl SnapRow {
    pub fn forecast(&self, line: &str) -> Decimal {
        find(&self.f, line).unwrap_or(ZERO)
    }
    pub fn actual(&self, line: &str) -> Decimal {
        find(&self.a, line).unwrap_or(ZERO)
    }
}

fn int_of(v: Option<&Json>) -> Result<Decimal, Failure> {
    match v {
        Some(Json::Int(n)) => Ok(Decimal::from(*n)),
        _ => Err(Failure("bad snapshot value".into())),
    }
}

/// Per imported month: forecast and actual per line, in dollars.
pub fn compare_snapshot(snap: &Json, hist: &History) -> Result<Vec<SnapRow>, Failure> {
    let rent = match snap.get("rent_category") {
        Some(Json::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let Some(Json::List(months)) = snap.get("months") else {
        return Err(Failure("KeyError: months".into()));
    };
    let hundred = Decimal::from(100);
    let (mut out, mut cum_f, mut cum_a) = (Vec::new(), ZERO, ZERO);
    for m in months {
        let Some(Json::Str(key)) = m.get("month") else {
            return Err(Failure("KeyError: month".into()));
        };
        let Some(i) = hist
            .months
            .iter()
            .position(|x| x.format("%Y-%m").to_string() == *key)
        else {
            break;
        };
        let a = actual_lines(hist, i, rent.as_deref());
        let mut f: Vec<(String, Decimal)> = Vec::new();
        for line in LINES {
            f.push((line.to_string(), int_of(m.get(line))? / hundred));
        }
        cum_f += find(&f, "net_cash_flow").unwrap_or(ZERO);
        cum_a += a.get("net_cash_flow");
        f.push(("month_end".into(), cum_f));
        let mut av: Vec<(String, Decimal)> =
            LINES.iter().map(|l| (l.to_string(), a.get(l))).collect();
        av.push(("month_end".into(), cum_a));
        let mut cats: Vec<(String, (Decimal, Decimal))> = Vec::new();
        if let Some(Json::Obj(sp)) = m.get("spending") {
            for (c, v) in sp {
                let fv = int_of(Some(v))? / hundred;
                let av = find(&a.spending, c).unwrap_or(ZERO);
                match cats.iter_mut().find(|(k, _)| k == c) {
                    Some(slot) => slot.1 = (fv, av),
                    None => cats.push((c.clone(), (fv, av))),
                }
            }
        } else {
            return Err(Failure("KeyError: spending".into()));
        }
        for (c, v) in &a.spending {
            if Some(c.as_str()) != rent.as_deref() && !cats.iter().any(|(k, _)| k == c) {
                cats.push((c.clone(), (ZERO, *v)));
            }
        }
        out.push(SnapRow {
            month: key.clone(),
            f,
            a: av,
            categories: cats,
        });
    }
    Ok(out)
}

/// 80 % range for month-end cash, per month ahead (1..=horizon): the 10th and
/// 90th percentile of (actual - forecast) month-end misses at that distance in
/// the rolling-origin backtest (empirical prediction intervals, FPP3 5.5).
/// Past the longest backtested distance the last range widens by
/// sqrt(h / last), as a random walk would. Empty when there are no misses.
pub fn month_end_ranges(misses: &[Miss], horizon: usize) -> Vec<(f64, f64)> {
    use rust_decimal::prelude::ToPrimitive;
    let mut per: Vec<Vec<f64>> = Vec::new();
    for m in misses.iter().filter(|m| m.line == "month_end") {
        let k = m.ahead as usize;
        if per.len() < k {
            per.resize(k, Vec::new());
        }
        per[k - 1].push((m.actual - m.forecast).to_f64().unwrap_or(0.0));
    }
    let q = |v: &[f64], p: f64| {
        let i = p * (v.len() - 1) as f64;
        let (lo, hi) = (i.floor() as usize, i.ceil() as usize);
        v[lo] + (v[hi] - v[lo]) * (i - lo as f64)
    };
    let mut base: Vec<(f64, f64)> = Vec::new();
    for v in per.iter_mut() {
        if v.is_empty() {
            break;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        base.push((q(v, 0.1).min(0.0), q(v, 0.9).max(0.0)));
    }
    let Some(&(lo, hi)) = base.last() else {
        return Vec::new();
    };
    let last = base.len() as f64;
    (1..=horizon)
        .map(|h| match base.get(h - 1) {
            Some(r) => *r,
            None => {
                let s = (h as f64 / last).sqrt();
                (lo * s, hi * s)
            }
        })
        .collect()
}
