//! Budget vs actual, per month and per year, plus budget proposals, as
//! `strategy/budget.py`.

use super::py::{self, money, pct};
use super::situation::Unported;
use super::{Ctx, Data, Section};
use crate::core::pyjson::Json;

fn actuals(summary: &Json, index: usize) -> Vec<(String, f64)> {
    match summary.get("spending_cents") {
        Some(Json::Obj(items)) => items
            .iter()
            .map(|(cat, series)| {
                let v = match series {
                    Json::List(v) => py::num(v.get(index)).unwrap_or(0.0),
                    _ => 0.0,
                };
                (cat.clone(), v / 100.0)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn roll_up(values: &[(String, f64)], keys: &[String]) -> Vec<f64> {
    let mut out = vec![0.0; keys.len()];
    for (cat, v) in values {
        if let Some(i) = keys
            .iter()
            .position(|k| cat == k || cat.starts_with(&format!("{k} > ")))
        {
            out[i] += v;
        }
    }
    out
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("budget", "Budget vs actual");
    let Some(summary) = sit.summary.as_ref().filter(|s| s.truthy()) else {
        sec.line("- No imported data yet.");
        return Ok(sec);
    };
    let window = sit.months_window();
    let Some(&last) = window.last() else {
        sec.line("- No complete month yet.");
        return Ok(sec);
    };
    let budget: Vec<(String, f64)> = match super::py::obj(&sit.profile, "budget") {
        Json::Obj(items) => items
            .iter()
            .map(|(k, v)| (k.clone(), py::num(Some(v)).unwrap_or(0.0)))
            .collect(),
        _ => Vec::new(),
    };
    let keys: Vec<String> = budget.iter().map(|(k, _)| k.clone()).collect();
    let months: Vec<String> = super::py::list(summary, "months")
        .iter()
        .map(Json::py_str)
        .collect();
    let month = months.get(last).cloned().unwrap_or_default();
    if !budget.is_empty() {
        let actual = roll_up(&actuals(summary, last), &keys);
        let year: String = month.chars().take(4).collect();
        let year_idx: Vec<usize> = window
            .iter()
            .copied()
            .filter(|&i| months.get(i).is_some_and(|m| m.starts_with(&year)))
            .collect();
        let mut ytd = vec![0.0; keys.len()];
        for &i in &year_idx {
            for (j, v) in roll_up(&actuals(summary, i), &keys).into_iter().enumerate() {
                ytd[j] += v;
            }
        }
        let label = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
            .map_or(month.to_string(), |d| d.format("%b %Y").to_string());
        sec.line(format!(
            "Month {label} and the year so far ({} months).",
            year_idx.len()
        ));
        sec.line("");
        sec.line(
            "| Category | Budget/month | Actual | Over/under | Year so far | Year budget | Used |",
        );
        sec.line("|---|---|---|---|---|---|---|");
        for (i, (k, b)) in budget.iter().enumerate() {
            let diff = actual[i] - b;
            let year_budget = b * 12.0;
            sec.line(format!(
                "| {k} | {} | {} | {}{} | {} | {} | {} |",
                money(*b),
                money(actual[i]),
                if diff > 0.0 { "+" } else { "" },
                money(diff),
                money(ytd[i]),
                money(year_budget),
                if year_budget != 0.0 {
                    pct(ytd[i] / year_budget)
                } else {
                    "—".into()
                }
            ));
        }
        let over: Vec<&str> = budget
            .iter()
            .enumerate()
            .filter(|(i, (_, b))| actual[*i] > b * 1.1)
            .map(|(_, (k, _))| k.as_str())
            .collect();
        if !over.is_empty() {
            sec.line(format!(
                "\n- Over budget by more than 10% last month: {}.",
                over.join(", ")
            ));
        }
    }
    let mut tops: Vec<(String, f64)> = Vec::new();
    for (cat, v) in sit.category_spending() {
        let top = cat.split(" > ").next().unwrap_or("").to_string();
        match tops.iter_mut().find(|(t, _)| *t == top) {
            Some(slot) => slot.1 += v,
            None => tops.push((top, v)),
        }
    }
    let mut proposals: Vec<(String, i64)> = tops
        .iter()
        .filter(|(k, v)| !keys.contains(k) && *v > 0.0)
        .map(|(k, v)| (k.clone(), (v / 10.0).ceil() as i64 * 10))
        .collect();
    if !proposals.is_empty() {
        proposals.sort_by(|a, b| b.1.cmp(&a.1));
        sec.line("");
        sec.line(
            "**Budget proposals** (12-month average, rounded up; copy into `[budget]` to adopt):",
        );
        sec.line("");
        for (k, v) in &proposals {
            sec.line(format!("- \"{k}\" = {v}"));
        }
    }
    sec.data = Data::Budget;
    Ok(sec)
}
