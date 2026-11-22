//! `api record`: balances an agent or the owner read on a service's own
//! pages, saved to `data/api-balances.json` (port of `services.record_balances`).

use chrono::Local;

use crate::core::events;
use crate::core::paths::{AppError, UserHome};
use crate::core::pyjson::{self, Json};

/// Python's `repr(str)` for plain text: single quotes unless the text holds
/// one and no double quote.
fn py_repr(text: &str) -> String {
    let q = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(q);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

fn api_error(msg: impl Into<String>) -> AppError {
    AppError::new("apierror", msg)
}

/// Python's `float(text)` for the amounts people type: None when it is not one.
fn py_float(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() || t.contains('_') {
        return None;
    }
    t.parse::<f64>().ok()
}

/// Four or more digits in a row: possibly an account number.
fn has_account_number(name: &str) -> bool {
    let mut run = 0;
    for c in name.chars() {
        run = if c.is_numeric() { run + 1 } else { 0 };
        if run >= 4 {
            return true;
        }
    }
    false
}

/// `_save_balances`: replaces this service's entry, keeping the others in place.
pub fn save(user: &UserHome, service: &str, entry: Json) -> Result<(), AppError> {
    let path = user.data().join("api-balances.json");
    let mut data = match std::fs::read_to_string(&path) {
        Ok(text) => match pyjson::loads(&text) {
            Some(Json::Obj(items)) => items,
            _ => return Err(AppError::new("crash", "api-balances.json")),
        },
        Err(_) => Vec::new(),
    };
    match data.iter_mut().find(|(k, _)| k == service) {
        Some(slot) => slot.1 = entry,
        None => data.push((service.to_string(), entry)),
    }
    std::fs::create_dir_all(user.data()).map_err(|e| AppError::new("oserror", e.to_string()))?;
    crate::write_private(&path, &pyjson::dumps(&Json::Obj(data), Some(1), false))
        .map_err(|e| AppError::new("oserror", e))
}

/// Merges `values` into `old` (a JSON object), later keys replacing earlier ones in place.
fn merged(old: Option<&Json>, values: &[(String, Json)]) -> Vec<(String, Json)> {
    let mut out = match old {
        Some(Json::Obj(items)) => items.clone(),
        _ => Vec::new(),
    };
    for (k, v) in values {
        match out.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.clone(),
            None => out.push((k.clone(), v.clone())),
        }
    }
    out
}

/// Records "NAME=AMOUNT,NAME=AMOUNT" for `service`; returns how many it read.
pub fn record(
    user: &UserHome,
    service: &str,
    spec: &str,
    currency: &str,
) -> Result<usize, AppError> {
    let specs = crate::tasks::connectors::load_all().map_err(|e| AppError::new("crash", e.0))?;
    if !specs.iter().any(|(id, _)| id == service) {
        return Err(api_error(format!(
            "unknown service {}: see bin/cashcast connectors list",
            py_repr(service)
        )));
    }
    let mut out: Vec<(String, Json)> = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, amount) = match part.split_once('=') {
            Some((n, a)) if !n.trim().is_empty() => (n.trim(), a),
            _ => {
                return Err(api_error(format!(
                    "expected NAME=AMOUNT, got {}",
                    py_repr(part)
                )));
            }
        };
        if has_account_number(name) {
            return Err(api_error(
                "an account name has 4 or more digits in a row: use a plain name such as TFSA, never a number",
            ));
        }
        let clean: String = amount
            .chars()
            .filter(|c| !matches!(c, '$' | ' ' | '_'))
            .collect();
        let value =
            py_float(&clean).ok_or_else(|| api_error(format!("not an amount for {name}")))?;
        match out.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = Json::Float(value),
            None => out.push((name.to_string(), Json::Float(value))),
        }
    }
    if out.is_empty() {
        return Err(api_error(
            "nothing to record: pass --set NAME=AMOUNT,NAME=AMOUNT",
        ));
    }
    let path = user.data().join("api-balances.json");
    let old = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| pyjson::loads(&t))
        .and_then(|d| d.get(service).cloned());
    // Only an earlier browser record merges; an API entry is replaced.
    let keep = old.filter(|o| o.get("source").map(Json::py_str).as_deref() == Some("browser"));
    let balances = merged(keep.as_ref().and_then(|k| k.get("balances")), &out);
    let upper = currency.to_uppercase();
    let curr: Vec<(String, Json)> = out
        .iter()
        .map(|(n, _)| (n.clone(), Json::Str(upper.clone())))
        .collect();
    let currencies = merged(keep.as_ref().and_then(|k| k.get("currencies")), &curr);
    let entry = Json::Obj(vec![
        ("balances".into(), Json::Obj(balances)),
        (
            "fetched_on".into(),
            Json::Str(Local::now().date_naive().to_string()),
        ),
        ("currencies".into(), Json::Obj(currencies)),
        ("source".into(), Json::Str("browser".into())),
    ]);
    save(user, service, entry)?;
    events::log(
        user,
        "balances_recorded",
        &[
            ("service", Json::Str(service.into())),
            ("count", Json::Int(out.len() as i64)),
        ],
    );
    Ok(out.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_and_names_follow_python() {
        assert_eq!(py_float("1000"), Some(1000.0));
        assert_eq!(py_float(" 12.5 "), Some(12.5));
        assert_eq!(py_float("abc"), None);
        assert!(has_account_number("acct 12345"));
        assert!(!has_account_number("TFSA 2"));
        assert_eq!(py_repr("ab"), "'ab'");
        assert_eq!(py_repr("o'x"), "\"o'x\"");
        assert_eq!(py_repr("a'b\""), "'a\\'b\"'");
    }
}
