//! Bank accounts: is holding the fee-waiving minimum cheaper than paying the
//! fee? As `strategy/banking.py`.
//!
//! Break-even return = (12 × monthly fee + rebate lost) / minimum balance.

use super::cashback::update;
use super::py::{getf, gets, list, money, obj, pct};
use super::situation::{Situation, Unported};
use super::{Ctx, Data, Section, Task};
use crate::core::pyjson::Json;

pub struct Verdict {
    pub yearly_fee: f64,
    pub hold_cost: f64,
    pub breakeven: f64,
    pub choice: &'static str,
}

/// Holding the minimum costs only the interest it would earn elsewhere above what the account itself pays.
pub fn verdict(
    monthly_fee: f64,
    minimum: f64,
    rebate: f64,
    cash_rate: f64,
    own_rate: f64,
) -> Verdict {
    let yearly_fee = 12.0 * monthly_fee + rebate;
    let hold_cost = minimum * (cash_rate - own_rate).max(0.0);
    Verdict {
        yearly_fee,
        hold_cost,
        breakeven: if minimum != 0.0 {
            yearly_fee / minimum
        } else {
            0.0
        },
        choice: if yearly_fee == 0.0 && minimum == 0.0 {
            "no fee"
        } else if minimum != 0.0 && hold_cost < yearly_fee {
            "hold the minimum"
        } else {
            "pay the fee or switch"
        },
    }
}

/// Python `str.title()`.
pub fn title(s: &str) -> String {
    let mut out = String::new();
    let mut prev_letter = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev_letter {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            prev_letter = true;
        } else {
            out.push(c);
            prev_letter = false;
        }
    }
    out
}

fn account_name(a: &Json) -> String {
    let name = a
        .get("name")
        .or_else(|| a.get("id"))
        .map_or("None".to_string(), Json::py_str);
    format!("{} {name}", gets(a, "bank", "")).trim().to_string()
}

/// The highest CAD rate among the person's own accounts and API cash services: (name, rate).
pub fn best_rate(sit: &Situation, held: &[Json]) -> Result<(String, f64), Unported> {
    let mut places: Vec<(String, f64)> = held
        .iter()
        .map(|a| (account_name(a), getf(a, "interest_rate", 0.0)))
        .collect();
    let fc = sit.knowledge("foreign_cash")?;
    if let Json::Obj(services) = obj(&fc, "interest") {
        for (s, _) in services {
            if sit
                .api_balances
                .as_ref()
                .and_then(|b| b.get(s))
                .is_some_and(Json::truthy)
            {
                places.push((format!("{} CAD", title(s)), sit.interest_rate(s, "CAD")?));
            }
        }
    }
    let mut best: Option<(String, f64)> = None;
    for p in places {
        if best.as_ref().is_none_or(|b| p.1 > b.1) {
            best = Some(p);
        }
    }
    Ok(best.unwrap_or((String::new(), 0.0)))
}

pub fn accounts(sit: &Situation) -> Result<Vec<Json>, Unported> {
    let catalog = sit.knowledge("banks")?;
    let mut out = Vec::new();
    for entry in list(obj(&sit.profile, "banking"), "accounts") {
        let mut base: Vec<(String, Json)> = match list(&catalog, "accounts")
            .iter()
            .rev()
            .find(|a| a.get("id") == entry.get("id"))
        {
            Some(Json::Obj(items)) => items.clone(),
            _ => Vec::new(),
        };
        update(&mut base, entry);
        out.push(Json::Obj(base));
    }
    Ok(out)
}

/// `connectors.find_by_name`: the connector for a bank name, matched on its id or name.
pub fn find_connector(name: &str) -> Result<String, Unported> {
    let want = name.trim().to_lowercase();
    if want.is_empty() {
        return Ok(String::new());
    }
    let dir = crate::core::paths::root().join("connectors");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| Unported(e.to_string()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    for p in files {
        let sid = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&p).map_err(|e| Unported(e.to_string()))?;
        let t: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| Unported(e.to_string()))?;
        let cname = t
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Unported("connector has no name".into()))?;
        if want == sid || want == cname.to_lowercase() {
            return Ok(sid);
        }
    }
    Ok(String::new())
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("banking", "Bank accounts: hold the minimum or pay the fee");
    let held = accounts(sit)?;
    if held.is_empty() {
        sec.line("- Add your chequing packages to `[[banking.accounts]]` in profile.toml (ids from knowledge/data/banks.toml).");
        return Ok(sec);
    }
    let best = best_rate(sit, &held)?;
    let (cash_rate, why) = match sit.goal("cash_rate").filter(|v| !matches!(v, Json::Null)) {
        Some(v) => (
            super::py::num(Some(v)).unwrap_or(0.0),
            "set by `goals.cash_rate`".to_string(),
        ),
        None if best.1 > 0.0 => (
            best.1,
            format!(
                "the best rate among your accounts, {}; set `goals.cash_rate` to change it",
                best.0
            ),
        ),
        None => (0.02, "a default; set `goals.cash_rate`".into()),
    };
    sec.line(format!(
        "Cash is valued at {} a year ({why}). Holding a minimum costs that rate minus what the account pays.",
        pct(cash_rate)
    ));
    sec.line("");
    sec.line("| Account | Fee/month | Minimum | Keep | Interest | Holding costs | Fee + rebate lost | Break-even | Verdict |");
    sec.line("|---|---|---|---|---|---|---|---|---|");
    let mut total_kept = 0.0;
    for a in &held {
        let (fee, minimum) = (
            getf(a, "monthly_fee", 0.0),
            getf(a, "waive_min_balance", 0.0),
        );
        let keep = getf(
            a,
            "keep",
            if minimum != 0.0 { minimum + 500.0 } else { 0.0 },
        );
        total_kept += keep;
        let own = getf(a, "interest_rate", 0.0);
        let v = verdict(
            fee,
            minimum,
            getf(a, "card_fee_rebate", 0.0),
            cash_rate,
            own,
        );
        sec.line(format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            format!(
                "{} {}",
                gets(a, "bank", ""),
                a.get("name")
                    .or_else(|| a.get("id"))
                    .map_or("None".into(), Json::py_str)
            ),
            money(fee),
            money(minimum),
            money(keep),
            pct(own),
            money(v.hold_cost),
            money(v.yearly_fee),
            pct(v.breakeven),
            v.choice
        ));
        // The minimum to keep stays in this table, never a task: a daily duty cannot be ticked done (owner rule).
        if v.choice != "hold the minimum" && fee != 0.0 {
            let mut t = Task::new(
                format!(
                    "bank-package-{}",
                    a.get("id").map_or("None".into(), Json::py_str)
                ),
                format!(
                    "Switch {} {} to a cheaper package or a no-fee account",
                    gets(a, "bank", ""),
                    gets(a, "name", "")
                ),
            )
            .notes(
                "The fee costs more than holding the minimum. Compare packages before you switch.",
            )
            .risk("settings");
            t.service = find_connector(&gets(a, "bank", ""))?;
            t.action = "change-chequing-package".into();
            sec.tasks.push(t);
        }
    }
    sec.line("");
    sec.line(format!(
        "- Cash held at bank minimums: {} (counts toward the emergency fund).",
        money(total_kept)
    ));
    sec.line("- Minimums are usually the balance at the end of **every** day; one day below means the full monthly fee.");
    sec.line(
        "- Money moves in before it moves out: transfer first, then pay the card the same day.",
    );
    sec.data = Data::Kept(total_kept);
    Ok(sec)
}
