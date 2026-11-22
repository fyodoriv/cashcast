//! Stock optimizer: where new money goes so a portfolio moves toward its
//! targets, as `src/cashcast/optimize.py`. Inputs are the owner's own
//! (`[optimize]` in profile.toml, or `--input FILE`); nothing here predicts
//! returns or picks stocks.

use super::DISCLAIMER;
use super::py::{self, getf, list, money, obj, pct};
use crate::core::pyjson::Json;

const NON_REG: &str = "non-registered";
const EPS: f64 = 0.005;

/// An input the owner must fix; the message goes to the person as is.
pub struct OptimizeError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, OptimizeError> {
    Err(OptimizeError(msg.into()))
}

fn preference_of(asset_class: &str, listing: &str) -> &'static [&'static str] {
    match (asset_class, listing) {
        ("bond", "CA") | ("bond", "US") => &["rrsp", "rrif", "tfsa", "fhsa", NON_REG],
        ("equity", "US") => &["rrsp", "rrif", NON_REG, "tfsa", "fhsa"],
        _ => &["tfsa", "fhsa", "rrsp", "rrif", NON_REG],
    }
}

pub struct Target {
    pub symbol: String,
    pub weight: f64,
    pub asset_class: String,
    pub listing: String,
    pub price: Option<f64>,
    pub accounts: Vec<String>,
}

pub struct Trade {
    pub account: String,
    pub symbol: String,
    pub amount: f64,
    pub units: Option<i64>,
}

pub struct Plan {
    pub trades: Vec<Trade>,
    pub before: Vec<(String, f64)>,
    pub after: Vec<(String, f64)>,
    pub targets: Vec<(String, f64)>,
    pub left: Vec<(String, f64)>,
    pub notes: Vec<String>,
}

fn get(m: &[(String, f64)], k: &str) -> Option<f64> {
    m.iter().find(|(s, _)| s == k).map(|(_, v)| *v)
}

/// `d[k] = d.get(k, 0) + v` on an ordered map.
fn add(m: &mut Vec<(String, f64)>, k: &str, v: f64) {
    match m.iter_mut().find(|(s, _)| s == k) {
        Some(slot) => slot.1 += v,
        None => m.push((k.to_string(), v)),
    }
}

impl Plan {
    /// Largest gap between a weight and its target, in weight points.
    pub fn drift(&self, weights: &[(String, f64)]) -> f64 {
        self.targets
            .iter()
            .map(|(s, _)| s)
            .chain(weights.iter().map(|(s, _)| s))
            .map(|s| (get(weights, s).unwrap_or(0.0) - get(&self.targets, s).unwrap_or(0.0)).abs())
            .fold(0.0, f64::max)
    }
}

pub fn account_key(name: &str) -> String {
    let low = name.trim().to_lowercase();
    if [
        "non-registered",
        "nonreg",
        "non-reg",
        "taxable",
        "margin",
        "cash account",
    ]
    .contains(&low.as_str())
    {
        NON_REG.into()
    } else {
        low
    }
}

/// Non-negative buys adding up to `cash` that minimise sum((gap - buy)^2).
pub fn water_fill(gaps: &[f64], cash: f64) -> Vec<f64> {
    if cash <= 0.0 || gaps.is_empty() {
        return vec![0.0; gaps.len()];
    }
    let mut order: Vec<usize> = (0..gaps.len()).collect();
    order.sort_by(|a, b| {
        gaps[*b]
            .partial_cmp(&gaps[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let (mut total, mut level) = (0.0, 0.0);
    for (k, i) in order.iter().enumerate() {
        total += gaps[*i];
        level = (total - cash) / (k + 1) as f64;
        let next = order.get(k + 1).map_or(f64::NEG_INFINITY, |j| gaps[*j]);
        if next <= level {
            break;
        }
    }
    gaps.iter().map(|g| (g - level).max(0.0)).collect()
}

/// The `[optimize]` inputs: from `--input FILE` (its `[optimize]`, else the
/// whole file) or the profile.
pub fn load(profile: &Json, file: Option<&Json>) -> Result<Json, OptimizeError> {
    if let Some(data) = file {
        return Ok(data
            .get("optimize")
            .cloned()
            .unwrap_or_else(|| data.clone()));
    }
    match profile.get("optimize") {
        Some(o) => Ok(o.clone()),
        None => err(
            "No inputs yet. Add an [optimize] section to profile.toml (targets, cash per account, holdings), \
             or pass --input FILE. See docs/optimize.md.",
        ),
    }
}

fn targets_of(spec: &Json) -> Result<Vec<Target>, OptimizeError> {
    let mut out: Vec<Target> = Vec::new();
    for t in list(spec, "targets") {
        if !t.get("symbol").is_some_and(Json::truthy) || t.get("weight").is_none() {
            return err("each [[optimize.targets]] needs a symbol and a weight");
        }
        out.push(Target {
            symbol: t
                .get("symbol")
                .map_or(String::new(), Json::py_str)
                .to_uppercase(),
            weight: getf(t, "weight", 0.0),
            asset_class: t
                .get("asset_class")
                .map_or("equity".into(), Json::py_str)
                .to_lowercase(),
            listing: t
                .get("listing")
                .map_or("CA".into(), Json::py_str)
                .to_uppercase(),
            price: t
                .get("price")
                .filter(|p| p.truthy())
                .and_then(|p| py::num(Some(p))),
            accounts: list(t, "accounts")
                .iter()
                .map(|a| account_key(&a.py_str()))
                .collect(),
        });
    }
    if out.is_empty() {
        return err("add at least one [[optimize.targets]] with a symbol and a weight");
    }
    let mut dup: Vec<&str> = out
        .iter()
        .filter(|t| out.iter().filter(|o| o.symbol == t.symbol).count() > 1)
        .map(|t| t.symbol.as_str())
        .collect();
    dup.sort();
    dup.dedup();
    if !dup.is_empty() {
        return err(format!(
            "each symbol is listed once in the targets ({} is listed twice)",
            dup.join(", ")
        ));
    }
    let total = py::sum(out.iter().map(|t| t.weight));
    if (total - 1.0).abs() > 0.001 || out.iter().any(|t| t.weight < 0.0) {
        return err(format!(
            "target weights must be 0 or more and add up to 1 (they add up to {total:.4})"
        ));
    }
    Ok(out)
}

fn prices_of(spec: &Json, targets: &[Target]) -> Vec<(String, f64)> {
    let mut prices: Vec<(String, f64)> = Vec::new();
    let mut put = |k: String, v: f64| match prices.iter_mut().find(|(s, _)| *s == k) {
        Some(slot) => slot.1 = v,
        None => prices.push((k, v)),
    };
    for t in targets {
        if let Some(p) = t.price {
            put(t.symbol.clone(), p);
        }
    }
    if let Json::Obj(items) = obj(spec, "prices") {
        for (k, v) in items {
            put(k.to_uppercase(), py::num(Some(v)).unwrap_or(0.0));
        }
    }
    prices
}

type Held = Vec<((String, String), f64)>;

/// Market value per (account, symbol): from the inputs, else from the engine's holdings at the target prices.
fn holdings_of(spec: &Json, targets: &[Target], engine: Option<&Json>) -> (Held, Vec<String>) {
    let prices = prices_of(spec, targets);
    let mut out: Held = Vec::new();
    let mut notes = Vec::new();
    let mut rows: Vec<Json> = match spec.get("holdings") {
        Some(Json::List(items)) => items.clone(),
        Some(_) => Vec::new(),
        None => Vec::new(),
    };
    if spec.get("holdings").is_none()
        && let Some(h) = engine.filter(|h| h.truthy())
    {
        rows = list(h, "assets")
            .iter()
            .map(|a| {
                Json::Obj(vec![
                    (
                        "account".into(),
                        a.get("account_bucket")
                            .cloned()
                            .unwrap_or(Json::Str(NON_REG.into())),
                    ),
                    (
                        "symbol".into(),
                        a.get("symbol").cloned().unwrap_or(Json::Str(String::new())),
                    ),
                    (
                        "units".into(),
                        a.get("quantity").cloned().unwrap_or(Json::Int(0)),
                    ),
                ])
            })
            .collect();
        notes.push("Holdings come from your imported exports (bin/cashcast holdings), valued at the prices you gave.".into());
    }
    for h in &rows {
        let sym = h
            .get("symbol")
            .map_or(String::new(), Json::py_str)
            .to_uppercase();
        let acct = account_key(&h.get("account").map_or(NON_REG.to_string(), Json::py_str));
        let some = |k: &str| h.get(k).filter(|v| !matches!(v, Json::Null));
        let value = if let Some(v) = some("value") {
            py::num(Some(v)).unwrap_or(0.0)
        } else if let (Some(u), Some(p)) = (some("units"), get(&prices, &sym)) {
            py::num(Some(u)).unwrap_or(0.0) * p
        } else {
            notes.push(format!(
                "{sym} in {acct}: no value and no price, so it is left out."
            ));
            continue;
        };
        if value != 0.0 {
            let key = (acct, sym);
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 += value,
                None => out.push((key, value)),
            }
        }
    }
    (out, notes)
}

/// Where a target may go, best first. Accounts outside the known ones only take targets that name them.
fn preference(t: &Target, accounts: &[String]) -> Vec<String> {
    let base: Vec<String> = if t.accounts.is_empty() {
        preference_of(&t.asset_class, &t.listing)
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        t.accounts.clone()
    };
    base.into_iter().filter(|a| accounts.contains(a)).collect()
}

pub fn optimize(spec: &Json, engine: Option<&Json>) -> Result<Plan, OptimizeError> {
    let targets = targets_of(spec)?;
    let prices = prices_of(spec, &targets);
    let (held, mut notes) = holdings_of(spec, &targets, engine);
    let mut cash: Vec<(String, f64)> = Vec::new();
    if let Json::Obj(items) = obj(spec, "cash") {
        for (k, v) in items {
            let k = account_key(k);
            let v = py::num(Some(v)).unwrap_or(0.0);
            match cash.iter_mut().find(|(s, _)| *s == k) {
                Some(slot) => slot.1 = v,
                None => cash.push((k, v)),
            }
        }
    }
    if cash.iter().any(|(_, v)| *v < 0.0) {
        return err("cash per account must be 0 or more");
    }
    let min_trade = getf(spec, "min_trade", 0.0);
    let allow_sell = spec.get("allow_sell").is_some_and(Json::truthy);
    let mut by_sym: Vec<(String, f64)> = Vec::new();
    for ((_, s), v) in &held {
        add(&mut by_sym, s, *v);
    }
    let invested = py::sum(by_sym.iter().map(|(_, v)| *v));
    let total = invested + py::sum(cash.iter().map(|(_, v)| *v));
    if total <= 0.0 {
        return err("nothing to invest: add cash per account or holdings");
    }
    let tw: Vec<(String, f64)> = targets
        .iter()
        .map(|t| (t.symbol.clone(), t.weight))
        .collect();
    let before: Vec<(String, f64)> = if invested != 0.0 {
        by_sym
            .iter()
            .map(|(s, v)| (s.clone(), v / invested))
            .collect()
    } else {
        Vec::new()
    };
    for (s, _) in &by_sym {
        if get(&tw, s).is_none() {
            notes.push(format!(
                "{s} is held but has no target, so it counts as weight 0."
            ));
        }
    }
    let gaps: Vec<(String, f64)> = targets
        .iter()
        .map(|t| {
            (
                t.symbol.clone(),
                t.weight * total - get(&by_sym, &t.symbol).unwrap_or(0.0),
            )
        })
        .collect();
    let mut trades: Vec<Trade> = Vec::new();
    let mut left = cash.clone();
    let want: Vec<(String, f64)> = if allow_sell {
        let mut syms: Vec<String> = by_sym
            .iter()
            .map(|(s, _)| s.clone())
            .chain(tw.iter().map(|(s, _)| s.clone()))
            .collect();
        syms.sort();
        syms.dedup();
        for s in syms {
            let mut excess = get(&by_sym, &s).unwrap_or(0.0) - get(&tw, &s).unwrap_or(0.0) * total;
            if excess <= EPS {
                continue;
            }
            let mut accts: Vec<&String> = held
                .iter()
                .filter(|((_, x), _)| *x == s)
                .map(|((a, _), _)| a)
                .collect();
            accts.sort_by(|a, b| {
                (a.as_str() == NON_REG, a.as_str()).cmp(&(b.as_str() == NON_REG, b.as_str()))
            });
            for acct in accts {
                let have = held
                    .iter()
                    .find(|((a, x), _)| a == acct && *x == s)
                    .map_or(0.0, |(_, v)| *v);
                let mut take = excess.min(have);
                if let Some(p) = get(&prices, &s) {
                    take = (take / p + 1e-9).floor() * p;
                }
                if take > EPS {
                    trades.push(Trade {
                        account: acct.clone(),
                        symbol: s.clone(),
                        amount: -take,
                        units: None,
                    });
                    add(&mut left, acct, take);
                    excess -= take;
                    if acct == NON_REG {
                        notes.push(format!(
                            "Selling {s} in the non-registered account may realize a capital gain (check bin/cashcast holdings)."
                        ));
                    }
                }
            }
        }
        gaps.iter().map(|(s, g)| (s.clone(), g.max(0.0))).collect()
    } else {
        let buys = water_fill(
            &gaps.iter().map(|(_, g)| *g).collect::<Vec<_>>(),
            py::sum(cash.iter().map(|(_, v)| *v)),
        );
        gaps.iter()
            .zip(buys)
            .map(|((s, _), b)| (s.clone(), b))
            .collect()
    };
    let accounts: Vec<String> = left.iter().map(|(a, _)| a.clone()).collect();
    let mut order: Vec<&Target> = targets.iter().collect();
    let key = |t: &Target| {
        (
            t.accounts.is_empty(),
            t.asset_class != "bond",
            t.listing != "US",
            -get(&want, &t.symbol).unwrap_or(0.0),
        )
    };
    order.sort_by(|a, b| {
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for t in order {
        let mut need = get(&want, &t.symbol).unwrap_or(0.0);
        for acct in preference(t, &accounts) {
            if need <= EPS {
                break;
            }
            let mut amount = need.min(get(&left, &acct).unwrap_or(0.0));
            if amount <= EPS {
                continue;
            }
            let mut units = None;
            if let Some(p) = t.price {
                let u = (amount / p).floor() as i64;
                units = Some(u);
                amount = u as f64 * p;
            }
            if amount < min_trade.max(EPS) {
                continue;
            }
            trades.push(Trade {
                account: acct.clone(),
                symbol: t.symbol.clone(),
                amount,
                units,
            });
            add(&mut left, &acct, -amount);
            need -= amount;
        }
        if need > min_trade.max(1.0) && !t.accounts.is_empty() {
            notes.push(format!(
                "{}: {} more fits its target, but its accounts ({}) have no cash left.",
                t.symbol,
                money(need),
                t.accounts.join(", ")
            ));
        }
    }
    let mut after_val = by_sym.clone();
    for tr in &trades {
        add(&mut after_val, &tr.symbol, tr.amount);
    }
    spend_leftover(
        &targets,
        &accounts,
        &mut left,
        &mut after_val,
        total,
        &mut trades,
        min_trade,
    );
    for (acct, v) in &left {
        if *v > min_trade.max(1.0)
            && !targets
                .iter()
                .any(|t| preference(t, &accounts).contains(acct))
        {
            notes.push(format!(
                "{acct}: no target may go there (an account cashcast does not know); name it in a target's `accounts`."
            ));
        }
    }
    let invested_after = py::sum(after_val.iter().map(|(_, v)| *v));
    let after = if invested_after != 0.0 {
        after_val
            .iter()
            .filter(|(_, v)| *v > EPS)
            .map(|(s, v)| (s.clone(), v / invested_after))
            .collect()
    } else {
        Vec::new()
    };
    Ok(Plan {
        trades,
        before,
        after,
        targets: tw,
        left: left.into_iter().filter(|(_, v)| *v > EPS).collect(),
        notes,
    })
}

/// Buys whole units with the cash rounding left, while one more unit brings a symbol closer to its target.
fn spend_leftover(
    targets: &[Target],
    accounts: &[String],
    left: &mut Vec<(String, f64)>,
    value: &mut Vec<(String, f64)>,
    total: f64,
    trades: &mut Vec<Trade>,
    min_trade: f64,
) {
    loop {
        let mut best: Option<(f64, &Target, String, f64)> = None;
        for t in targets {
            let gap = t.weight * total - get(value, &t.symbol).unwrap_or(0.0);
            for acct in preference(t, accounts) {
                let cash = get(left, &acct).unwrap_or(0.0);
                let step = t.price.unwrap_or_else(|| gap.min(cash));
                // a unit helps only when it lands closer to the target: price < 2 x gap
                if step <= EPS || step > cash + 1e-9 || step >= 2.0 * gap || step < min_trade {
                    continue;
                }
                if best.as_ref().is_none_or(|b| gap > b.0) {
                    best = Some((gap, t, acct, step));
                }
            }
        }
        let Some((_, t, acct, step)) = best else {
            return;
        };
        match trades
            .iter_mut()
            .find(|tr| tr.account == acct && tr.symbol == t.symbol && tr.amount > 0.0)
        {
            Some(same) => {
                same.amount += step;
                same.units = same.units.map(|u| u + 1);
            }
            None => trades.push(Trade {
                account: acct.clone(),
                symbol: t.symbol.clone(),
                amount: step,
                units: t.price.map(|_| 1),
            }),
        }
        add(left, &acct, -step);
        add(value, &t.symbol, step);
    }
}

pub fn render(plan: &Plan) -> String {
    let mut lines: Vec<String> = vec![
        "# Stock optimizer: where the new money goes".into(),
        String::new(),
        format!("*{DISCLAIMER}*"),
        String::new(),
    ];
    let buys: Vec<&Trade> = plan.trades.iter().filter(|t| t.amount > 0.0).collect();
    let sells: Vec<&Trade> = plan.trades.iter().filter(|t| t.amount < 0.0).collect();
    if !sells.is_empty() {
        lines.extend(
            [
                "**Sell first:**",
                "",
                "| Account | Symbol | Amount |",
                "|---|---|---|",
            ]
            .map(String::from),
        );
        lines.extend(
            sells
                .iter()
                .map(|t| format!("| {} | {} | {} |", t.account, t.symbol, money(-t.amount))),
        );
        lines.push(String::new());
    }
    lines.extend(
        [
            "**Buy:**",
            "",
            "| Account | Symbol | Amount | Units |",
            "|---|---|---|---|",
        ]
        .map(String::from),
    );
    if buys.is_empty() {
        lines.push("| none | | | |".into());
    }
    lines.extend(buys.iter().map(|t| {
        format!(
            "| {} | {} | {} | {} |",
            t.account,
            t.symbol,
            money(t.amount),
            t.units.map_or(String::new(), |u| u.to_string())
        )
    }));
    lines.extend(
        [
            "",
            "| Symbol | Target | Before | After |",
            "|---|---|---|---|",
        ]
        .map(String::from),
    );
    let mut syms: Vec<&String> = plan
        .targets
        .iter()
        .chain(&plan.before)
        .chain(&plan.after)
        .map(|(s, _)| s)
        .collect();
    syms.sort();
    syms.dedup();
    for s in syms {
        let w = |m: &[(String, f64)]| pct(get(m, s).unwrap_or(0.0));
        lines.push(format!(
            "| {s} | {} | {} | {} |",
            w(&plan.targets),
            w(&plan.before),
            w(&plan.after)
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "- Largest gap to target: {} before, {} after.",
        pct(plan.drift(&plan.before)),
        pct(plan.drift(&plan.after))
    ));
    if !plan.left.is_empty() {
        let mut left = plan.left.clone();
        left.sort_by(|a, b| a.0.cmp(&b.0));
        lines.push(format!(
            "- Cash left: {} (whole units, or under the minimum trade).",
            left.iter()
                .map(|(a, v)| format!("{a} {}", money(*v)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.extend(plan.notes.iter().map(|n| format!("- {n}")));
    lines.push("- Check that each contribution fits your room first (bin/cashcast tax shows TFSA and RRSP room).".into());
    lines.push(
        "- You place the trades yourself: buying and selling moves money, so no agent does it."
            .into(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_fill_levels_the_largest_gaps_first() {
        assert_eq!(water_fill(&[300.0, 100.0], 200.0), vec![200.0, 0.0]);
        assert_eq!(water_fill(&[300.0, 100.0], 400.0), vec![300.0, 100.0]);
        assert_eq!(water_fill(&[1.0], 0.0), vec![0.0]);
    }

    #[test]
    fn account_names_fold_to_one_key() {
        assert_eq!(account_key(" Taxable "), NON_REG);
        assert_eq!(account_key("TFSA"), "tfsa");
    }

    #[test]
    fn weights_that_do_not_add_up_are_an_error() {
        let spec =
            crate::core::pyjson::loads(r#"{"targets": [{"symbol": "A", "weight": 0.5}]}"#).unwrap();
        let msg = optimize(&spec, None).err().map(|e| e.0).unwrap_or_default();
        assert!(msg.contains("add up to 1"), "{msg}");
    }
}
