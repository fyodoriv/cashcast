//! A person's situation, built from their profile and the engine's
//! aggregates, as `src/cashcast/situation.py`.
//!
//! Reads only `profile.toml`, `cashcast.toml`, `data/summary.json`,
//! `data/holdings.json` and `data/api-balances.json`. Never the raw exports.

use super::py::{self, getf, gets, list, obj};
use crate::core::pyjson::{self, Json};
use chrono::NaiveDate;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Keyword → reward category, matched against the lower-case category path.
/// First match wins, so the specific words come first.
pub const CATEGORY_RULES: &[(&str, &[&str])] = &[
    (
        "bank_only",
        &[
            "mortgage",
            "income tax",
            "taxes",
            "loan",
            "transfer",
            "invest",
            "savings",
            "rrsp",
            "tfsa",
            "atm",
            "cash withdrawal",
            "government",
            "law",
        ],
    ),
    ("rent", &["rent"]),
    (
        "delivery",
        &["delivery", "uber eats", "doordash", "skip the dishes"],
    ),
    ("groceries", &["grocer", "supermarket", "groceries"]),
    (
        "dining",
        &[
            "dining",
            "restaurant",
            "coffee",
            "cafe",
            "fast food",
            "bar",
            "takeout",
        ],
    ),
    ("ev_charging", &["ev charging", "charging"]),
    ("gas", &["gas", "fuel", "petrol"]),
    ("rideshare", &["rideshare", "taxi", "uber", "lyft"]),
    (
        "transit",
        &[
            "transit",
            "ttc",
            "presto",
            "public transport",
            "train",
            "bus",
            "parking",
        ],
    ),
    (
        "streaming",
        &["streaming", "netflix", "spotify", "music", "video"],
    ),
    (
        "recurring",
        &[
            "bills",
            "utilities",
            "phone",
            "internet",
            "mobile",
            "subscription",
            "hydro",
            "electricity",
        ],
    ),
    ("drugstore", &["pharmacy", "drugstore", "drug store"]),
    (
        "travel",
        &[
            "travel", "flight", "hotel", "airline", "vacation", "lodging",
        ],
    ),
    (
        "entertainment",
        &["entertainment", "movies", "events", "leisure", "concert"],
    ),
];

const BUCKETS: &[(&str, &str)] = &[
    ("tfsa", "TFSA"),
    ("fhsa", "FHSA"),
    ("rrsp", "RRSP"),
    ("rrif", "RRSP"),
    ("spousal", "RRSP"),
    ("resp", "RESP"),
    ("lira", "LIRA"),
    ("crypto", "Crypto"),
];

/// The tax bucket an account name points to (TFSA, RRSP, ...); anything else is non-registered.
pub fn bucket(account: &str) -> &'static str {
    let low = account.to_lowercase();
    BUCKETS
        .iter()
        .find(|(w, _)| low.contains(w))
        .map_or("Non-registered", |(_, b)| b)
}

pub fn reward_category(path: &str, overrides: &Json) -> String {
    if let Json::Obj(items) = overrides {
        let mut sorted: Vec<&(String, Json)> = items.iter().collect();
        sorted.sort_by_key(|(k, _)| std::cmp::Reverse(k.chars().count()));
        for (prefix, cat) in sorted {
            if path == prefix || path.starts_with(&format!("{prefix} > ")) {
                return cat.py_str();
            }
        }
    }
    let low = path.to_lowercase();
    for (cat, words) in CATEGORY_RULES {
        if words.iter().any(|w| low.contains(w)) {
            return cat.to_string();
        }
    }
    "other".into()
}

/// Python `_merge`: tables merge by key, lists of tables with ids merge by id.
fn merge(base: &Json, extra: &Json) -> Json {
    match (base, extra) {
        (Json::Obj(b), Json::Obj(e)) => {
            let mut out = b.clone();
            for (k, v) in e {
                let merged = match base.get(k) {
                    Some(old) => merge(old, v),
                    None => v.clone(),
                };
                match out.iter_mut().find(|(x, _)| x == k) {
                    Some(slot) => slot.1 = merged,
                    None => out.push((k.clone(), merged)),
                }
            }
            Json::Obj(out)
        }
        (Json::List(b), Json::List(e))
            if b.iter()
                .chain(e)
                .all(|x| matches!(x, Json::Obj(_)) && x.get("id").is_some()) =>
        {
            let mut by_id: Vec<(Json, Json)> = b
                .iter()
                .map(|x| (x.get("id").cloned().unwrap_or(Json::Null), x.clone()))
                .collect();
            for x in e {
                let id = x.get("id").cloned().unwrap_or(Json::Null);
                match by_id.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1 = merge(&slot.1, x),
                    None => by_id.push((id, merge(&Json::Obj(Vec::new()), x))),
                }
            }
            Json::List(by_id.into_iter().map(|(_, v)| v).collect())
        }
        _ => extra.clone(),
    }
}

/// A file Python would fail on with a traceback: the command stops with one
/// error line and exit 1 (AppError kind `crash`).
#[derive(Debug)]
pub struct Unported(pub String);

fn read_toml(path: &Path) -> Result<Option<Json>, Unported> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| Unported(e.to_string()))?;
    let t: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| Unported(e.to_string()))?;
    Ok(Some(py::table(&t)))
}

fn read_json(path: &Path) -> Result<Option<Json>, Unported> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| Unported(e.to_string()))?;
    pyjson::loads(&text)
        .map(Some)
        .ok_or_else(|| Unported(format!("bad JSON in {}", path.display())))
}

/// `knowledge.load(name, user)`: the shared file merged with the user's own.
pub fn knowledge(name: &str, home: Option<&Path>) -> Result<Json, Unported> {
    let base = read_toml(&crate::knowledge::data_dir().join(format!("{name}.toml")))?
        .unwrap_or(Json::Obj(Vec::new()));
    match home {
        Some(h) => match read_toml(&h.join("knowledge").join(format!("{name}.toml")))? {
            Some(own) => Ok(merge(&base, &own)),
            None => Ok(base),
        },
        None => Ok(base),
    }
}

pub struct Situation {
    pub home: PathBuf,
    pub name: String,
    pub profile: Json,
    pub config: Json,
    pub summary: Option<Json>,
    pub holdings: Option<Json>,
    pub today: NaiveDate,
    pub notes: RefCell<Vec<String>>,
    pub api_balances: Option<Json>,
}

impl Situation {
    pub fn load(home: &Path, name: &str, today: NaiveDate) -> Result<Self, Unported> {
        let profile =
            read_toml(&home.join("profile.toml"))?.ok_or_else(|| Unported("no profile".into()))?;
        Ok(Situation {
            home: home.to_path_buf(),
            name: name.to_string(),
            config: read_toml(&home.join("cashcast.toml"))?.unwrap_or(Json::Obj(Vec::new())),
            summary: read_json(&home.join("data").join("summary.json"))?,
            holdings: read_json(&home.join("data").join("holdings.json"))?,
            api_balances: read_json(&home.join("data").join("api-balances.json"))?,
            profile,
            today,
            notes: RefCell::new(Vec::new()),
        })
    }

    pub fn knowledge(&self, name: &str) -> Result<Json, Unported> {
        knowledge(name, Some(&self.home))
    }

    pub fn note(&self, n: String) {
        self.notes.borrow_mut().push(n);
    }

    pub fn province(&self) -> String {
        gets(&self.profile, "province", "").to_uppercase()
    }

    pub fn goal(&self, key: &str) -> Option<&Json> {
        obj(&self.profile, "goals").get(key)
    }

    fn summary_months(&self) -> Vec<String> {
        self.summary
            .as_ref()
            .map(|s| list(s, "months").iter().map(Json::py_str).collect())
            .unwrap_or_default()
    }

    /// Indexes of the last 12 complete months in the summary.
    pub fn months_window(&self) -> Vec<usize> {
        let this_month = self.today.format("%Y-%m").to_string();
        let idx: Vec<usize> = self
            .summary_months()
            .iter()
            .enumerate()
            .filter(|(_, m)| **m < this_month)
            .map(|(i, _)| i)
            .collect();
        idx[idx.len().saturating_sub(12)..].to_vec()
    }

    /// Average monthly spending per category path over the last 12 complete months.
    pub fn category_spending(&self) -> Vec<(String, f64)> {
        let Some(s) = &self.summary else {
            return Vec::new();
        };
        let idx = self.months_window();
        let n = idx.len().max(1) as f64;
        let Json::Obj(items) = obj(s, "spending_cents") else {
            return Vec::new();
        };
        items
            .iter()
            .map(|(cat, series)| {
                let cents: i64 = idx
                    .iter()
                    .map(|&i| match series {
                        Json::List(l) => match l.get(i) {
                            Some(Json::Int(c)) => *c,
                            _ => 0,
                        },
                        _ => 0,
                    })
                    .sum();
                (cat.clone(), cents as f64 / 100.0 / n)
            })
            .collect()
    }

    /// Annual spend per reward category, from history or the profile's own estimates.
    pub fn annual_by_reward_category(&self) -> Vec<(String, f64)> {
        let mut out: Vec<(String, f64)> = Vec::new();
        let put = |out: &mut Vec<(String, f64)>, k: &str, f: &dyn Fn(f64) -> f64| match out
            .iter_mut()
            .find(|(c, _)| c == k)
        {
            Some(slot) => slot.1 = f(slot.1),
            None => out.push((k.to_string(), f(0.0))),
        };
        let overrides = obj(obj(&self.profile, "strategy"), "categories");
        for (path, monthly) in self.category_spending() {
            let cat = reward_category(&path, overrides);
            put(&mut out, &cat, &|v| v + monthly * 12.0);
        }
        if let Json::Obj(items) = obj(&self.profile, "spending") {
            for (cat, monthly) in items {
                if crate::knowledge::CATEGORIES.contains(&cat.as_str()) || cat == "bank_only" {
                    let m = py::num(Some(monthly)).unwrap_or(0.0);
                    put(&mut out, cat, &|_| m * 12.0);
                }
            }
        }
        if let Some(rent) = obj(&self.profile, "rent")
            .get("monthly")
            .filter(|v| v.truthy())
        {
            let m = py::num(Some(rent)).unwrap_or(0.0);
            put(&mut out, "rent", &|_| m * 12.0);
        }
        if out.is_empty() {
            self.note("No spending history or [spending] estimates yet: import exports or fill [spending] in profile.toml.".into());
        }
        out.into_iter()
            .filter(|(_, v)| *v > 0.0)
            .map(|(k, v)| (k, py::round(v, 2)))
            .collect()
    }

    pub fn monthly_spending(&self) -> f64 {
        if let Some(own) = self.goal("monthly_spending").filter(|v| v.truthy()) {
            return py::num(Some(own)).unwrap_or(0.0);
        }
        py::round(
            py::sum(self.category_spending().into_iter().map(|(_, v)| v)),
            2,
        )
    }

    /// CAD per unit by currency code, from foreign_cash.toml (plus the user's override).
    pub fn fx_rates(&self) -> Result<Vec<(String, Json)>, Unported> {
        let fc = self.knowledge("foreign_cash")?;
        let mut rates: Vec<(String, Json)> = Vec::new();
        for r in list(&fc, "rates") {
            let id = gets(r, "id", "").to_uppercase();
            match rates.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = r.clone(),
                None => rates.push((id, r.clone())),
            }
        }
        let cad = Json::Obj(vec![
            ("id".into(), Json::Str("CAD".into())),
            ("cad_per_unit".into(), Json::Float(1.0)),
        ]);
        match rates.iter_mut().find(|(k, _)| k == "CAD") {
            Some(slot) => slot.1 = cad,
            None => rates.push(("CAD".into(), cad)),
        }
        self.live_rates(&mut rates)?;
        Ok(rates)
    }

    /// A saved live Bank of Canada rate (`knowledge refresh`, decision 0015)
    /// replaces the shared dated rate when it is newer. `as_facts` already
    /// drops a live rate older than its series' `max_age_days`. A rate the
    /// user pinned in their own `knowledge/foreign_cash.toml` always wins.
    fn live_rates(&self, rates: &mut Vec<(String, Json)>) -> Result<(), Unported> {
        const LIVE_FX: &[(&str, &str)] = &[("live-usd-cad", "USD")];
        let own = read_toml(&self.home.join("knowledge").join("foreign_cash.toml"))?;
        let pinned = |code: &str| {
            own.as_ref().is_some_and(|o| {
                list(o, "rates")
                    .iter()
                    .any(|r| gets(r, "id", "").eq_ignore_ascii_case(code))
            })
        };
        for (id, fact) in crate::live_facts::as_facts(self.today) {
            let Some((_, code)) = LIVE_FX.iter().find(|(f, _)| *f == id) else {
                continue;
            };
            if pinned(code) {
                continue;
            }
            let date = gets(&fact, "as_of", "");
            let entry = Json::Obj(vec![
                ("id".into(), Json::Str((*code).into())),
                (
                    "cad_per_unit".into(),
                    Json::Float(getf(&fact, "value", 0.0)),
                ),
                ("date".into(), Json::Str(date.clone())),
                ("source".into(), Json::Str(gets(&fact, "source", ""))),
                ("confidence".into(), Json::Str("verified".into())),
            ]);
            match rates.iter_mut().find(|(k, _)| k == code) {
                Some(slot) if slot.1.get("date").map(|d| d.py_str()).unwrap_or_default() < date => {
                    slot.1 = entry;
                }
                Some(_) => {}
                None => rates.push(((*code).into(), entry)),
            }
        }
        Ok(())
    }

    pub fn to_cad(&self, amount: f64, currency: &str) -> Result<Option<f64>, Unported> {
        let code = if currency.is_empty() {
            "CAD".to_string()
        } else {
            currency.to_uppercase()
        };
        Ok(self
            .fx_rates()?
            .iter()
            .find(|(k, _)| *k == code)
            .map(|(_, r)| amount * getf(r, "cad_per_unit", 0.0)))
    }

    /// Annual interest on a currency held at a service, else 0.
    pub fn interest_rate(&self, service: &str, currency: &str) -> Result<f64, Unported> {
        let fc = self.knowledge("foreign_cash")?;
        let rate = obj(obj(&fc, "interest"), service)
            .get(&currency.to_uppercase())
            .cloned();
        Ok(match rate {
            Some(Json::Int(i)) => i as f64,
            Some(Json::Float(f)) => f,
            _ => 0.0,
        })
    }
}

/// One balance from the read-only APIs (`data/api-balances.json`), valued in CAD.
#[derive(Clone, Debug)]
pub struct SourceRow {
    pub service: String,
    pub currency: String,
    pub kind: String,
    pub account: String,
    pub bucket: Option<String>,
    pub source: Json,
    pub amount: f64,
    pub rate: Option<f64>,
    pub rate_date: Json,
    pub cad: Option<f64>,
    pub fetched_on: Json,
    pub counted: bool,
    pub covered_by_summary: bool,
}

fn currency_suffix(asset: &str) -> Option<String> {
    // r"\(([A-Z]{3})\)\s*$"
    let t = asset.trim_end();
    let b = t.as_bytes();
    if b.len() >= 5
        && b[b.len() - 1] == b')'
        && b[b.len() - 5] == b'('
        && b[b.len() - 4..b.len() - 1]
            .iter()
            .all(u8::is_ascii_uppercase)
    {
        return Some(t[t.len() - 4..t.len() - 1].to_string());
    }
    None
}

impl Situation {
    /// Balances from the read-only APIs, one row per service and asset, valued in CAD.
    pub fn source_balances(&self) -> Result<Vec<SourceRow>, Unported> {
        let Some(api) = &self.api_balances else {
            return Ok(Vec::new());
        };
        if !api.truthy() {
            return Ok(Vec::new());
        }
        let fc = self.knowledge("foreign_cash")?;
        let kinds = match fc.get("api_services") {
            Some(k) => k.clone(),
            None => Json::Obj(vec![("wise".into(), Json::Str("cash".into()))]),
        };
        let names: Vec<String> = self
            .summary
            .as_ref()
            .map(|s| {
                list(s, "accounts")
                    .iter()
                    .filter(|a| gets(a, "class", "") == "cash")
                    .map(|a| gets(a, "name", "").to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let rates = self.fx_rates()?;
        let rate_of = |code: &str| rates.iter().find(|(k, _)| k == code).map(|(_, r)| r);
        let mut any_price: Vec<(String, Json)> = Vec::new();
        if let Json::Obj(entries) = api {
            for (_, e) in entries {
                if let Json::Obj(_) = e {
                    super::cashback::update(&mut any_price, obj(e, "prices"));
                }
            }
        }
        let mut rows = Vec::new();
        let Json::Obj(kind_items) = &kinds else {
            return Ok(rows);
        };
        for (service, service_kind) in kind_items {
            let Some(entry) = api.get(service).filter(|e| e.truthy()) else {
                continue;
            };
            let service_kind = service_kind.py_str();
            let covered = names.iter().any(|n| n.contains(service.as_str()));
            let mut prices = any_price.clone();
            super::cashback::update(&mut prices, obj(entry, "prices"));
            let mut balances: Vec<(String, Json)> = match obj(entry, "balances") {
                Json::Obj(items) => items.clone(),
                _ => Vec::new(),
            };
            balances.sort_by(|a, b| a.0.cmp(&b.0));
            for (asset, amount) in balances {
                if !amount.truthy() {
                    continue;
                }
                let mut code = asset.to_uppercase();
                let kind;
                if service_kind == "investments" {
                    kind = "investment".to_string();
                    code = match obj(entry, "currencies").get(&asset) {
                        Some(c) => c.py_str(),
                        None => currency_suffix(&asset).unwrap_or_else(|| "CAD".into()),
                    }
                    .to_uppercase();
                } else {
                    kind = if service_kind == "cash" || rate_of(&code).is_some() {
                        "cash"
                    } else {
                        "crypto"
                    }
                    .to_string();
                }
                let (mut rate, mut rate_date) = (None, Json::Null);
                if (kind == "cash" || kind == "investment") && rate_of(&code).is_some() {
                    let r = rate_of(&code).expect("checked");
                    rate = Some(getf(r, "cad_per_unit", 0.0));
                    rate_date = r.get("date").cloned().unwrap_or(Json::Null);
                } else if kind == "crypto"
                    && let Some((_, p)) = prices.iter().find(|(k, _)| *k == code)
                {
                    let quote = rate_of(&gets(p, "currency", "CAD").to_uppercase());
                    if let Some(q) = quote {
                        rate = Some(getf(p, "price", 0.0) * getf(q, "cad_per_unit", 0.0));
                        rate_date = entry.get("fetched_on").cloned().unwrap_or(Json::Null);
                    }
                }
                let amount = py::num(Some(&amount)).unwrap_or(0.0);
                rows.push(SourceRow {
                    service: service.clone(),
                    currency: code,
                    bucket: if kind == "investment" {
                        Some(bucket(&asset).to_string())
                    } else {
                        None
                    },
                    account: asset,
                    source: entry
                        .get("source")
                        .cloned()
                        .unwrap_or(Json::Str("api".into())),
                    amount,
                    rate,
                    rate_date,
                    cad: rate.map(|r| py::round(amount * r, 2)),
                    fetched_on: entry.get("fetched_on").cloned().unwrap_or(Json::Null),
                    counted: kind == "cash" && !covered && rate.is_some(),
                    covered_by_summary: kind == "cash" && covered,
                    kind,
                });
            }
        }
        Ok(rows)
    }

    pub fn rows_of(&self, kind: &str) -> Result<Vec<SourceRow>, Unported> {
        Ok(self
            .source_balances()?
            .into_iter()
            .filter(|r| r.kind == kind)
            .collect())
    }

    /// Cash in CAD: the summary's cash accounts plus API cash not already in them.
    pub fn cash_balance(&self) -> Result<Option<f64>, Unported> {
        let given = obj(&self.config, "balances");
        let (mut total, mut known) = (0.0f64, false);
        let accounts: Vec<Json> = self
            .summary
            .as_ref()
            .map(|s| list(s, "accounts").to_vec())
            .unwrap_or_default();
        for a in &accounts {
            if gets(a, "class", "") != "cash" {
                continue;
            }
            let name = gets(a, "name", "");
            if let Some(v) = given.get(&name) {
                total += py::num(Some(v)).unwrap_or(0.0);
                known = true;
            } else if let Some(c) = a.get("balance_cents").filter(|v| !matches!(v, Json::Null)) {
                let cur = a
                    .get("currency")
                    .filter(|v| v.truthy())
                    .map_or("CAD".to_string(), Json::py_str);
                match self.to_cad(py::num(Some(c)).unwrap_or(0.0) / 100.0, &cur)? {
                    Some(cad) => {
                        total += cad;
                        known = true;
                    }
                    None => {
                        let shown = a.get("currency").map_or("None".into(), Json::py_str);
                        self.note(format!(
                            "No CAD rate for {shown}: add it to knowledge/foreign_cash.toml."
                        ));
                    }
                }
            }
        }
        for row in self.rows_of("cash")? {
            if row.counted {
                total += row.cad.unwrap_or(0.0);
                known = true;
            }
        }
        Ok(if known {
            Some(py::round(total, 2))
        } else {
            None
        })
    }
}
