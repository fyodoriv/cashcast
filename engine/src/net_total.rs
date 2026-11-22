//! Net total: the main number. Everything the owner has (cash, investments,
//! crypto, other assets) minus everything they owe (card balances, loans).
//! Higher is better.
//!
//! In: aggregate items the strategy gathered (one signed CAD value per
//! account or source; debts are negative), the options the strategy sections
//! offer (each with its effect on the net total a year), and the owner's
//! boundaries from `[boundaries]` in profile.toml. Out: the total, its trend
//! (one point a day in `data/net-total-history.json`), and the options ranked
//! by effect, with every option that breaks a boundary refused and the
//! reason named. Nothing here moves money.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::money::fmt_cents;

/// Risk levels, lowest first. An option above `max_risk` is refused.
pub const RISKS: [&str; 6] = [
    "none",
    "conservative",
    "balanced",
    "growth",
    "all-equity",
    "speculative",
];
pub const DEFAULT_MAX_WORK_HOURS: f64 = 45.0;
pub const DEFAULT_MARGIN: u32 = 5;
pub const MAX_MARGIN: u32 = 20;
/// A value read longer ago than this is flagged as stale.
pub const STALE_DAYS: i64 = 31;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Item {
    pub label: String,
    /// cash, investment, crypto, other or debt.
    pub kind: String,
    /// Signed CAD: what it adds to the net total (a debt is negative).
    pub cad: f64,
    #[serde(default)]
    pub as_of: Option<String>,
    /// Where the value came from: summary, api, browser, cost, profile.
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Opt {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub section: String,
    /// Effect on the net total a year, in CAD. Ignored when `per_dollar` is set.
    #[serde(default)]
    pub effect_year: f64,
    /// Effect a year per dollar of cash moved: the engine sizes `cash_out`
    /// to what the cash floor allows, then effect = per_dollar × amount.
    #[serde(default)]
    pub per_dollar: Option<f64>,
    /// A known one-time cost (a fee, tax on a sale), taken off the first year.
    #[serde(default)]
    pub cost_once: f64,
    /// Cash this option takes out of the cash accounts.
    #[serde(default)]
    pub cash_out: f64,
    /// New debt it takes on (a loan, a card balance carried).
    #[serde(default)]
    pub new_debt: f64,
    /// Extra work hours a week it needs.
    #[serde(default)]
    pub work_hours_week: f64,
    /// The risk level of what it buys (see RISKS).
    #[serde(default)]
    pub risk: Option<String>,
    #[serde(default)]
    pub sells_home: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Input {
    pub today: String,
    #[serde(default)]
    pub items: Vec<Item>,
    /// Sources with no value yet, by name only.
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub options: Vec<Opt>,
    /// The strategy's reserve (emergency fund + float): the cash floor when
    /// the owner sets no `min_cash`.
    #[serde(default)]
    pub reserve: Option<f64>,
    /// The owner's risk level (`goals.risk`): the risk limit when the owner
    /// sets no `max_risk`.
    #[serde(default)]
    pub risk: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Boundaries {
    pub max_work_hours_week: f64,
    /// Hours a week worked now; unknown means extra-work options are refused.
    pub work_hours_week: Option<f64>,
    /// Cash never goes below this (before the margin).
    pub min_cash: Option<f64>,
    pub min_cash_source: String,
    /// Percent kept above `min_cash` as a safety margin (0 to 20).
    pub margin: u32,
    pub max_risk: String,
    pub max_new_debt: f64,
    pub keep_home: bool,
    /// Tags the owner never wants (for example "leverage", "crypto").
    pub refuse: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Deserialize)]
pub struct Point {
    pub date: String,
    pub total: f64,
    #[serde(default)]
    pub by_kind: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ranked {
    #[serde(flatten)]
    pub option: Opt,
    /// First-year effect on the net total: effect a year minus the one-time cost.
    pub net_effect: f64,
    pub refused: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    pub days: i64,
    pub since: String,
    pub change: f64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub today: String,
    pub total: f64,
    pub by_kind: BTreeMap<String, f64>,
    pub items: Vec<Item>,
    pub stale: Vec<String>,
    pub missing: Vec<String>,
    pub history: Vec<Point>,
    pub changes: Vec<Change>,
    pub boundaries: Boundaries,
    /// Options inside the boundaries, best first by first-year effect.
    pub ranked: Vec<Ranked>,
    /// Options that break a boundary, each with its reasons.
    pub refused: Vec<Ranked>,
}

fn num(v: Option<&toml::Value>) -> Option<f64> {
    match v {
        Some(toml::Value::Integer(i)) => Some(*i as f64),
        Some(toml::Value::Float(f)) => Some(*f),
        _ => None,
    }
}

/// Reads `[boundaries]` from profile.toml, with safe defaults: at most 45
/// work hours a week, cash never below the reserve plus a 5 % margin, risk at
/// most the owner's own risk level, no new debt, and the home kept.
pub fn boundaries(profile: &str, input: &Input) -> Result<Boundaries, String> {
    let v: toml::Value =
        toml::from_str(profile).map_err(|e| format!("profile.toml: {}", e.message()))?;
    let b = v.get("boundaries");
    let get = |k: &str| b.and_then(|b| b.get(k));
    let positive = |k: &str| -> Result<Option<f64>, String> {
        match get(k) {
            None => Ok(None),
            Some(x) => match num(Some(x)) {
                Some(n) if n >= 0.0 => Ok(Some(n)),
                _ => Err(format!("[boundaries] {k}: use a number 0 or more")),
            },
        }
    };
    let max_hours = positive("max_work_hours_week")?.unwrap_or(DEFAULT_MAX_WORK_HOURS);
    let hours = positive("work_hours_week")?;
    let (min_cash, min_cash_source) = match positive("min_cash")? {
        Some(n) => (Some(n), "your [boundaries] min_cash".to_string()),
        None => match input.reserve {
            Some(r) => (Some(r), "your reserve (emergency fund + float)".to_string()),
            None => (None, "unknown: set [boundaries] min_cash".to_string()),
        },
    };
    let margin = match get("margin") {
        None => DEFAULT_MARGIN,
        Some(x) => match x.as_integer() {
            Some(n) if (0..=i64::from(MAX_MARGIN)).contains(&n) => n as u32,
            _ => {
                return Err(format!(
                    "[boundaries] margin: use a whole percent from 0 to {MAX_MARGIN}"
                ));
            }
        },
    };
    let max_risk = match get("max_risk") {
        None => input
            .risk
            .clone()
            .filter(|r| RISKS.contains(&r.as_str()))
            .unwrap_or_else(|| "growth".into()),
        Some(x) => {
            let r = x.as_str().unwrap_or_default().to_lowercase();
            if !RISKS.contains(&r.as_str()) {
                return Err(format!(
                    "[boundaries] max_risk: use one of {}",
                    RISKS.join(", ")
                ));
            }
            r
        }
    };
    let keep_home = match get("keep_home") {
        None => true,
        Some(x) => x
            .as_bool()
            .ok_or("[boundaries] keep_home: use true or false")?,
    };
    let refuse = get("refuse")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    Ok(Boundaries {
        max_work_hours_week: max_hours,
        work_hours_week: hours,
        min_cash,
        min_cash_source,
        margin,
        max_risk,
        max_new_debt: positive("max_new_debt")?.unwrap_or(0.0),
        keep_home,
        refuse,
    })
}

/// The owner's own entries: `[[net_total.items]]` (a home, a car, a loan:
/// things no export holds) and `[[net_total.ideas]]` (moves the owner is
/// weighing, checked against the boundaries like cashcast's own).
pub fn own_entries(profile: &str) -> Result<(Vec<Item>, Vec<Opt>), String> {
    #[derive(Deserialize, Default)]
    struct Own {
        #[serde(default)]
        items: Vec<Item>,
        #[serde(default)]
        ideas: Vec<Opt>,
    }
    #[derive(Deserialize, Default)]
    struct Profile {
        #[serde(default)]
        net_total: Own,
    }
    let p: Profile =
        toml::from_str(profile).map_err(|e| format!("profile.toml: {}", e.message()))?;
    let items = p
        .net_total
        .items
        .into_iter()
        .map(|mut i| {
            if i.kind == "debt" && i.cad > 0.0 {
                i.cad = -i.cad; // the owner writes what they owe as a plain amount
            }
            if i.source.is_empty() {
                i.source = "profile".into();
            }
            i
        })
        .collect();
    let ideas = p
        .net_total
        .ideas
        .into_iter()
        .map(|mut o| {
            if o.section.is_empty() {
                o.section = "your idea".into();
            }
            o
        })
        .collect();
    Ok((items, ideas))
}

fn risk_rank(r: &str) -> usize {
    RISKS.iter().position(|x| *x == r).unwrap_or(RISKS.len())
}

/// Checks one option against the boundaries. Sizes a `per_dollar` option to
/// the cash the floor allows. Returns the option as ranked, with its reasons
/// when refused.
pub fn check(o: &Opt, b: &Boundaries, cash: f64) -> Ranked {
    let mut o = o.clone();
    let mut why = Vec::new();
    let floor = b.min_cash.map(|m| m * (1.0 + f64::from(b.margin) / 100.0));
    if o.cash_out > 0.0 {
        match floor {
            None => why.push(
                "the cash floor is unknown: set [boundaries] min_cash so cashcast can check it"
                    .to_string(),
            ),
            Some(f) => {
                let room = (cash - f).max(0.0);
                if o.per_dollar.is_some() {
                    o.cash_out = o.cash_out.min(room).floor();
                    if o.cash_out <= 0.0 {
                        why.push(format!(
                            "cash would drop below your floor of {} ({} + {} % margin)",
                            dollars(f),
                            dollars(b.min_cash.unwrap_or_default()),
                            b.margin
                        ));
                    }
                } else if o.cash_out > room {
                    why.push(format!(
                        "it takes {} of cash, but only {} is above your floor of {} ({} % margin)",
                        dollars(o.cash_out),
                        dollars(room),
                        dollars(f),
                        b.margin
                    ));
                }
            }
        }
    }
    if let Some(rate) = o.per_dollar {
        o.effect_year = rate * o.cash_out;
    }
    if o.work_hours_week > 0.0 {
        match b.work_hours_week {
            None => why.push(
                "it adds work hours, and your hours now are unknown: set [boundaries] work_hours_week"
                    .to_string(),
            ),
            Some(now) if now + o.work_hours_week > b.max_work_hours_week => why.push(format!(
                "no burnout: {:.0} + {:.0} work hours a week is above your limit of {:.0}",
                now, o.work_hours_week, b.max_work_hours_week
            )),
            _ => {}
        }
    }
    if let Some(r) = &o.risk
        && risk_rank(r) > risk_rank(&b.max_risk)
    {
        why.push(format!(
            "its risk ({r}) is above your limit ({})",
            b.max_risk
        ));
    }
    if o.new_debt > b.max_new_debt {
        why.push(format!(
            "it adds {} of debt; your limit is {}",
            dollars(o.new_debt),
            dollars(b.max_new_debt)
        ));
    }
    if o.sells_home && b.keep_home {
        why.push("it sells your home, and you keep your home (keep_home = true)".into());
    }
    for t in &o.tags {
        if b.refuse.contains(&t.to_lowercase()) {
            why.push(format!("you refuse \"{t}\" moves"));
        }
    }
    let net_effect = o.effect_year - o.cost_once;
    Ranked {
        option: o,
        net_effect,
        refused: why,
    }
}

fn cents(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// Adds today's point to the history (a second run the same day replaces
/// it) and keeps it in date order.
pub fn upsert(history: &mut Vec<Point>, point: Point) {
    history.retain(|p| p.date != point.date);
    history.push(point);
    history.sort_by(|a, b| a.date.cmp(&b.date));
}

/// The change against the newest point at least `days` before today.
pub fn changes(history: &[Point], today: NaiveDate) -> Vec<Change> {
    let Some(now) = history.last() else {
        return Vec::new();
    };
    [30, 365]
        .into_iter()
        .filter_map(|days| {
            let cutoff = today - chrono::Duration::days(days);
            let then = history.iter().rev().find(|p| {
                NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").is_ok_and(|d| d <= cutoff)
            })?;
            Some(Change {
                days,
                since: then.date.clone(),
                change: now.total - then.total,
            })
        })
        .collect()
}

pub fn compute(input: &Input, profile: &str, mut history: Vec<Point>) -> Result<Report, String> {
    let today = NaiveDate::parse_from_str(&input.today, "%Y-%m-%d")
        .map_err(|_| "net-total input: today must be YYYY-MM-DD")?;
    let b = boundaries(profile, input)?;
    let (own_items, ideas) = own_entries(profile)?;
    let items: Vec<Item> = input.items.iter().cloned().chain(own_items).collect();
    let mut by_kind: BTreeMap<String, i64> = BTreeMap::new();
    for i in &items {
        *by_kind.entry(i.kind.clone()).or_default() += cents(i.cad);
    }
    let total_cents: i64 = by_kind.values().sum();
    let total = total_cents as f64 / 100.0;
    let by_kind: BTreeMap<String, f64> = by_kind
        .into_iter()
        .map(|(k, v)| (k, v as f64 / 100.0))
        .collect();
    let stale = items
        .iter()
        .filter(|i| {
            i.as_of
                .as_deref()
                .and_then(|d| NaiveDate::parse_from_str(d.get(..10).unwrap_or(d), "%Y-%m-%d").ok())
                .is_some_and(|d| (today - d).num_days() > STALE_DAYS)
        })
        .map(|i| i.label.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    upsert(
        &mut history,
        Point {
            date: input.today.clone(),
            total,
            by_kind: by_kind.clone(),
        },
    );
    let cash = by_kind.get("cash").copied().unwrap_or(0.0);
    let (mut ranked, mut refused): (Vec<Ranked>, Vec<Ranked>) = input
        .options
        .iter()
        .chain(&ideas)
        .map(|o| check(o, &b, cash))
        .partition(|r| r.refused.is_empty());
    let order = |a: &Ranked, b: &Ranked| {
        b.net_effect
            .total_cmp(&a.net_effect)
            .then_with(|| a.option.id.cmp(&b.option.id))
    };
    ranked.sort_by(order);
    refused.sort_by(order);
    Ok(Report {
        today: input.today.clone(),
        total,
        changes: changes(&history, today),
        by_kind,
        items,
        stale,
        missing: input.missing.clone(),
        history,
        boundaries: b,
        ranked,
        refused,
    })
}

/// Whole dollars with thousands separators: "$1,234", "-$56".
pub fn dollars(x: f64) -> String {
    let s = fmt_cents(cents(x.abs().round()));
    let s = s.strip_suffix(".00").unwrap_or(&s);
    if x.round() < 0.0 {
        format!("-${s}")
    } else {
        format!("${s}")
    }
}

fn signed(x: f64) -> String {
    if x >= 0.0 {
        format!("+{}", dollars(x))
    } else {
        dollars(x)
    }
}

const KINDS: [(&str, &str); 5] = [
    ("cash", "Cash"),
    ("investment", "Investments"),
    ("crypto", "Crypto"),
    ("other", "Other assets"),
    ("debt", "Debts"),
];

/// One line for the top of `bin/cashcast`: the total and its trend.
pub fn headline(r: &Report) -> String {
    let mut s = format!("Net total: {}", dollars(r.total));
    let trend: Vec<String> = r
        .changes
        .iter()
        .map(|c| format!("{} in {} days", signed(c.change), c.days))
        .collect();
    if !trend.is_empty() {
        s += &format!(" ({})", trend.join(", "));
    }
    if !r.stale.is_empty() || !r.missing.is_empty() {
        s += &format!(
            ". {} source(s) stale or missing: bin/cashcast net-total",
            r.stale.len() + r.missing.len()
        );
    }
    s
}

/// Items summed per (label, kind), with the oldest reading of each group.
fn groups(items: &[Item]) -> BTreeMap<(String, String), (f64, Option<String>)> {
    let mut out: BTreeMap<(String, String), (f64, Option<String>)> = BTreeMap::new();
    for i in items {
        let e = out.entry((i.label.clone(), i.kind.clone())).or_default();
        e.0 += i.cad;
        if let Some(d) = &i.as_of {
            let d = d.get(..10).unwrap_or(d).to_string();
            if e.1.as_ref().is_none_or(|old| d < *old) {
                e.1 = Some(d);
            }
        }
    }
    out
}

pub fn render(r: &Report) -> String {
    let mut o = String::new();
    o += &format!("## Net total: {}\n\n", dollars(r.total));
    o += "Everything you have minus everything you owe. Higher is better. ";
    o += "Every option below is ranked by what it adds to this number, inside your boundaries.\n\n";
    if r.changes.is_empty() {
        o += "- Trend: first point saved today. The change shows after 30 days.\n";
    }
    for c in &r.changes {
        o += &format!(
            "- Last {} days: **{}** (since {}).\n",
            c.days,
            signed(c.change),
            c.since
        );
    }
    o += "\n| Part | CAD |\n|---|---|\n";
    for (k, name) in KINDS {
        if let Some(v) = r.by_kind.get(k) {
            o += &format!("| {name} | {} |\n", dollars(*v));
        }
    }
    for (k, v) in &r.by_kind {
        if !KINDS.iter().any(|(id, _)| id == k) {
            o += &format!("| {k} | {} |\n", dollars(*v));
        }
    }
    o += &format!("| **Net total** | **{}** |\n", dollars(r.total));
    o += "\n<details><summary>Each source</summary>\n\n| Source | Part | CAD | Oldest reading |\n|---|---|---|---|\n";
    for ((label, kind), (cad, as_of)) in groups(&r.items) {
        o += &format!(
            "| {label} | {kind} | {} | {} |\n",
            dollars(cad),
            as_of.unwrap_or_else(|| "-".into())
        );
    }
    o += "\n</details>\n";
    if !r.stale.is_empty() {
        o += &format!(
            "\n- Read more than {STALE_DAYS} days ago, so the total may be off: {}. Refresh them.\n",
            r.stale.join(", ")
        );
    }
    for m in &r.missing {
        o += &format!("- Not counted yet: {m}.\n");
    }
    let b = &r.boundaries;
    o += "\n### Your boundaries\n\n";
    o += &format!(
        "- Work: at most {:.0} hours a week (now: {}).\n",
        b.max_work_hours_week,
        b.work_hours_week
            .map_or("unknown".to_string(), |h| format!("{h:.0}"))
    );
    o += &format!(
        "- Cash floor: {} + {} % margin, from {}.\n",
        b.min_cash.map_or("unknown".to_string(), dollars),
        b.margin,
        b.min_cash_source
    );
    o += &format!("- Risk: at most {}.\n", b.max_risk);
    o += &format!("- New debt: at most {}.\n", dollars(b.max_new_debt));
    o += &format!(
        "- Home: {}.\n",
        if b.keep_home {
            "kept, never sold"
        } else {
            "may be sold"
        }
    );
    if !b.refuse.is_empty() {
        o += &format!("- Never: {}.\n", b.refuse.join(", "));
    }
    o += "- Change them in `[boundaries]` of your profile.toml.\n";
    o += "\n### Options, best first\n\n";
    o += "Each option's net total is a long-run planning estimate, not a forecast; investing can lose money. Education and arithmetic on your own data, not personal financial advice.\n\n";
    if r.ranked.is_empty() {
        o += "- No option inside your boundaries adds to the net total right now.\n";
    } else {
        o += "| # | Option | Net total a year | From |\n|---|---|---|---|\n";
        for (n, x) in r.ranked.iter().enumerate() {
            o += &format!(
                "| {} | {} | {} | {} |\n",
                n + 1,
                x.option.title,
                signed(x.net_effect),
                x.option.section
            );
        }
    }
    if !r.refused.is_empty() {
        o += "\n### Refused: outside your boundaries\n\n";
        for x in &r.refused {
            o += &format!(
                "- ~~{}~~ ({} a year): {}.\n",
                x.option.title,
                signed(x.net_effect),
                x.refused.join("; ")
            );
        }
    }
    o
}

#[derive(Serialize)]
pub struct Output<'a> {
    pub headline: String,
    pub markdown: String,
    #[serde(flatten)]
    pub report: &'a Report,
}

/// The engine's `net-total` command: input JSON, profile text and the saved
/// history in; the output JSON and the new history out.
pub fn run(input: &str, profile: &str, history: Option<&str>) -> Result<(String, String), String> {
    let input: Input = serde_json::from_str(input).map_err(|e| format!("net-total input: {e}"))?;
    let history: Vec<Point> = match history {
        Some(h) => serde_json::from_str(h).map_err(|e| format!("net-total history: {e}"))?,
        None => Vec::new(),
    };
    let report = compute(&input, profile, history)?;
    let out = Output {
        headline: headline(&report),
        markdown: render(&report),
        report: &report,
    };
    Ok((
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?,
        serde_json::to_string_pretty(&report.history).map_err(|e| e.to_string())?,
    ))
}
