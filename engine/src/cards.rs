//! Which card to use right now, from actual numbers: the cap each card used
//! so far in its current cap period, the cap left and its reset date, the
//! credit left, and the best card per spending category today.
//!
//! Reads the wallet in `profile.toml`, the card rules in
//! `knowledge/data/credit_cards.toml` (merged with the person's own copy)
//! and the spending rows `summarize` classified. Prints card names,
//! category names, dates, counts and sums only, never payees. Nothing here
//! moves money or applies for a card.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use chrono::{Datelike, Months, NaiveDate};

use crate::money::fmt_cents;
use crate::summarize::SpendRow;

/// Keyword → reward category, matched against the lower-case category path.
/// First match wins. Kept in step with `CATEGORY_RULES` in
/// `src/cashcast/situation.py` (`tests/unit/test_cards_now.py` checks).
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

/// A rent row counts as one rent charge when it is at least this share of
/// the monthly charge, so a car rental filed under "rent" is not a charge.
const RENT_CHARGE_SHARE: f64 = 0.4;

/// Rows older than this many days are a stale count.
const STALE_DAYS: u64 = 14;

/// Smallest cap left that applies, with its reset date (None: unknown). None: no cap.
pub type CapLeft = Option<(i64, Option<NaiveDate>)>;

/// When a cap period starts over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Period {
    /// Each year from the card's `cap_reset` day (its anniversary).
    CardYear,
    /// January 1 to December 31.
    CalendarYear,
    /// Each calendar month.
    Month,
    /// Each statement, from the day after the statement closes.
    StatementMonth,
}

impl Period {
    fn parse(text: &str) -> Result<Period, String> {
        match text {
            "card_year" | "year" | "annual" => Ok(Period::CardYear),
            "calendar_year" => Ok(Period::CalendarYear),
            "month" | "monthly" => Ok(Period::Month),
            "statement" | "statement_month" => Ok(Period::StatementMonth),
            other => Err(format!("unknown cap period {other:?}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cap {
    pub categories: Vec<String>,
    pub limit: i64,
    pub period: Period,
    pub per_category: bool,
    pub after_rate: f64,
}

#[derive(Debug, Clone)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub earn: BTreeMap<String, f64>,
    pub caps: Vec<Cap>,
    pub fx_fee: f64,
    /// Export account names that hold this card's rows.
    pub accounts: Vec<String>,
    /// Month and day the card year starts (`cap_reset = "MM-DD"`).
    pub cap_reset: Option<(u32, u32)>,
    pub statement_close_day: Option<u32>,
    pub limit: Option<i64>,
    pub target: Option<i64>,
    /// At most this many rent charges per rent year (`rent_max`).
    pub rent_max: Option<u32>,
    /// Month and day the rent count starts over (`rent_year_start`); default the card year.
    pub rent_year_start: Option<(u32, u32)>,
    /// First day the card's own `rent` rate applies (`rent_rate_from`). Rent
    /// rows posted before it earned like `codes_as` and count toward its caps.
    pub rent_rate_from: Option<NaiveDate>,
}

#[derive(Debug, Clone, Default)]
pub struct Rent {
    /// What one monthly rent charge costs on a card, with the service fee.
    pub charge: i64,
    pub fee_rate: f64,
    /// The category the rent service's charge codes as on cards with no rent rate.
    pub codes_as: String,
}

#[derive(Debug, Clone, Default)]
pub struct Wallet {
    pub cards: Vec<Card>,
    pub rent: Option<Rent>,
    /// `[strategy.categories]`: category path prefix → reward category.
    pub overrides: Vec<(String, String)>,
}

/// How one cap stands today.
#[derive(Debug, Clone, PartialEq)]
pub struct CapState {
    /// The categories this line counts (all of the cap's, or one for per-category caps).
    pub categories: Vec<String>,
    pub limit: i64,
    pub used: i64,
    pub start: NaiveDate,
    pub resets: NaiveDate,
    /// True when the start is a guess: the card has no `cap_reset`.
    pub guessed: bool,
    pub after_rate: f64,
}

impl CapState {
    pub fn left(&self) -> i64 {
        (self.limit - self.used).max(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RentCount {
    pub used: u32,
    pub max: Option<u32>,
    pub since: NaiveDate,
    /// Charges left: the rule's count, else what the cap left holds.
    pub left: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardState {
    pub id: String,
    pub name: String,
    pub caps: Vec<CapState>,
    pub rent: Option<RentCount>,
    pub balance: Option<i64>,
    pub limit: Option<i64>,
    pub target: Option<i64>,
    pub next_close: Option<NaiveDate>,
    /// Latest row date on this card's accounts.
    pub data_until: Option<NaiveDate>,
    /// Open tasks that will change this card (a product change).
    pub pending: Vec<String>,
}

impl CardState {
    pub fn credit_left(&self) -> Option<i64> {
        Some(self.limit? - self.balance?)
    }
}

/// The best card for one spending category today, and the runner-up.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub category: String,
    pub card: String,
    pub rate: f64,
    pub cap_left: CapLeft,
    pub credit_left: Option<i64>,
    pub then: Option<(String, f64)>,
    /// Why better cards were passed over.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub today: NaiveDate,
    pub cards: Vec<CardState>,
    pub picks: Vec<Pick>,
    pub notes: Vec<String>,
}

fn cents(v: Option<&toml::Value>) -> Option<i64> {
    match v {
        Some(toml::Value::Integer(i)) => Some(i * 100),
        Some(toml::Value::Float(f)) => Some((f * 100.0).round() as i64),
        _ => None,
    }
}

fn float(v: Option<&toml::Value>) -> Option<f64> {
    match v {
        Some(toml::Value::Integer(i)) => Some(*i as f64),
        Some(toml::Value::Float(f)) => Some(*f),
        _ => None,
    }
}

fn month_day(v: Option<&toml::Value>, key: &str, id: &str) -> Result<Option<(u32, u32)>, String> {
    let Some(text) = v.and_then(|x| x.as_str()) else {
        return Ok(None);
    };
    let bad = || format!("card {id}: {key} = {text:?} must be \"MM-DD\", like \"10-26\"");
    let (m, d) = text.split_once('-').ok_or_else(bad)?;
    let (m, d): (u32, u32) = (m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    Ok(Some((m, d)))
}

fn parse_caps(v: &toml::Value, id: &str) -> Result<Vec<Cap>, String> {
    let mut caps = Vec::new();
    for c in v.as_array().into_iter().flatten() {
        let categories: Vec<String> = c
            .get("categories")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        let period = c
            .get("period")
            .and_then(|x| x.as_str())
            .unwrap_or("card_year");
        caps.push(Cap {
            categories,
            limit: cents(c.get("limit")).ok_or_else(|| format!("card {id}: a cap has no limit"))?,
            period: Period::parse(period).map_err(|e| format!("card {id}: {e}"))?,
            per_category: c
                .get("per_category")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            after_rate: float(c.get("after_rate")).unwrap_or(0.0),
        });
    }
    Ok(caps)
}

fn catalog_cards(text: &str, what: &str) -> Result<BTreeMap<String, toml::Table>, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("{what}: {}", e.message()))?;
    Ok(v.get("cards")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let t = c.as_table()?.clone();
            Some((t.get("id")?.as_str()?.to_string(), t))
        })
        .collect())
}

/// Reads the wallet. A card's rules come from the catalog, then the
/// person's own catalog, then the card's own entry in the profile.
pub fn parse(profile: &str, catalog: &str, own_catalog: Option<&str>) -> Result<Wallet, String> {
    let mut known = catalog_cards(catalog, "credit_cards.toml")?;
    if let Some(own) = own_catalog {
        for (id, t) in catalog_cards(own, "your knowledge/credit_cards.toml")? {
            known.entry(id).or_default().extend(t);
        }
    }
    let p: toml::Value =
        toml::from_str(profile).map_err(|e| format!("profile.toml: {}", e.message()))?;
    let mut cards = Vec::new();
    let entries = p
        .get("wallet")
        .and_then(|w| w.get("cards"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    for entry in &entries {
        let Some(id) = entry.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        let mut t = known.get(id).cloned().unwrap_or_default();
        if let Some(own) = entry.as_table() {
            t.extend(own.clone());
        }
        let earn: BTreeMap<String, f64> = t
            .get("earn")
            .and_then(|e| e.as_table())
            .ok_or_else(|| {
                format!(
                    "card {id} is not in the knowledge catalog: add its earn rates in profile.toml"
                )
            })?
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), float(Some(v))?)))
            .collect();
        let get = |k: &str| t.get(k);
        cards.push(Card {
            id: id.to_string(),
            name: get("name")
                .and_then(|x| x.as_str())
                .unwrap_or(id)
                .to_string(),
            earn,
            caps: match get("caps") {
                Some(c) => parse_caps(c, id)?,
                None => Vec::new(),
            },
            fx_fee: float(get("fx_fee")).unwrap_or(0.0),
            accounts: get("accounts")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            cap_reset: month_day(get("cap_reset"), "cap_reset", id)?,
            statement_close_day: get("statement_close_day")
                .and_then(|x| x.as_integer())
                .map(|d| d.clamp(1, 31) as u32),
            limit: cents(get("limit")),
            target: cents(get("target")),
            rent_max: get("rent_max")
                .and_then(|x| x.as_integer())
                .map(|n| n.max(0) as u32),
            rent_year_start: month_day(get("rent_year_start"), "rent_year_start", id)?,
            rent_rate_from: match get("rent_rate_from") {
                None => None,
                Some(v) => Some(
                    NaiveDate::parse_from_str(&v.to_string().replace('"', ""), "%Y-%m-%d")
                        .map_err(|_| {
                            format!("card {id}: rent_rate_from must be a date (YYYY-MM-DD)")
                        })?,
                ),
            },
        });
    }
    let rent = p.get("rent").and_then(|r| {
        let monthly = float(r.get("monthly"))?;
        let fee_rate = float(r.get("fee_rate")).unwrap_or(0.0);
        Some(Rent {
            charge: (monthly * (1.0 + fee_rate) * 100.0).round() as i64,
            fee_rate,
            codes_as: r
                .get("codes_as")
                .and_then(|x| x.as_str())
                .unwrap_or("other")
                .to_string(),
        })
    });
    let mut overrides: Vec<(String, String)> = p
        .get("strategy")
        .and_then(|s| s.get("categories"))
        .and_then(|c| c.as_table())
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect();
    overrides.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    Ok(Wallet {
        cards,
        rent,
        overrides,
    })
}

/// The reward category of a category path: the longest matching
/// `[strategy.categories]` prefix, else the first keyword rule, else "other".
pub fn reward_category<'a>(path: &str, overrides: &'a [(String, String)]) -> &'a str {
    for (prefix, cat) in overrides {
        if path == prefix || path.starts_with(&format!("{prefix} > ")) {
            return cat;
        }
    }
    let low = path.to_lowercase();
    for (cat, words) in CATEGORY_RULES {
        if words.iter().any(|w| low.contains(w)) {
            return cat;
        }
    }
    "other"
}

fn on(year: i32, month: u32, day: u32) -> NaiveDate {
    let last = NaiveDate::from_ymd_opt(year + i32::from(month == 12), month % 12 + 1, 1)
        .and_then(|d| d.pred_opt())
        .map_or(28, |d| d.day());
    NaiveDate::from_ymd_opt(year, month, day.min(last)).expect("a valid date")
}

/// The latest date on or before `today` with this month and day.
fn last_yearly(today: NaiveDate, (m, d): (u32, u32)) -> NaiveDate {
    let this = on(today.year(), m, d);
    if this <= today {
        this
    } else {
        on(today.year() - 1, m, d)
    }
}

/// The current period's (start, next reset, guessed).
fn period_bounds(card: &Card, period: Period, today: NaiveDate) -> (NaiveDate, NaiveDate, bool) {
    match period {
        Period::CardYear => match card.cap_reset {
            Some(md) => {
                let start = last_yearly(today, md);
                (start, on(start.year() + 1, md.0, md.1), false)
            }
            // Unknown anniversary: count the last 12 months, which can only overcount.
            None => (
                today - Months::new(12) + chrono::Days::new(1),
                today + chrono::Days::new(1),
                true,
            ),
        },
        Period::CalendarYear => (on(today.year(), 1, 1), on(today.year() + 1, 1, 1), false),
        Period::Month => {
            let start = on(today.year(), today.month(), 1);
            (start, start + Months::new(1), false)
        }
        Period::StatementMonth => match card.statement_close_day {
            Some(day) => {
                let close = on(today.year(), today.month(), day);
                let last_close = if close < today {
                    close
                } else {
                    let prev = today - Months::new(1);
                    on(prev.year(), prev.month(), day)
                };
                let next = last_close + Months::new(1);
                let next = on(next.year(), next.month(), day);
                (
                    last_close + chrono::Days::new(1),
                    next + chrono::Days::new(1),
                    false,
                )
            }
            None => {
                let start = on(today.year(), today.month(), 1);
                (start, start + Months::new(1), true)
            }
        },
    }
}

/// The category a purchase counts as on this card: its own category when
/// the card names it, the rent service's category for rent, else "other".
fn counted<'a>(card: &Card, cat: &'a str, rent: Option<&'a Rent>) -> &'a str {
    if cat == "rent" && !card.earn.contains_key("rent") {
        return rent.map_or("other", |r| r.codes_as.as_str());
    }
    let named = card.earn.contains_key(cat)
        || card
            .caps
            .iter()
            .any(|c| c.categories.iter().any(|x| x == cat));
    if named { cat } else { "other" }
}

/// `counted` for a row posted on `date`: before the card's `rent_rate_from`,
/// rent counts as the rent service's category, as it earned then.
fn counted_on<'a>(card: &Card, cat: &'a str, rent: Option<&'a Rent>, date: NaiveDate) -> &'a str {
    if cat == "rent" && card.rent_rate_from.is_some_and(|from| date < from) {
        return rent.map_or("other", |r| r.codes_as.as_str());
    }
    counted(card, cat, rent)
}

fn next_close(today: NaiveDate, day: u32) -> NaiveDate {
    let d = on(today.year(), today.month(), day);
    if d >= today {
        d
    } else {
        let n = today + Months::new(1);
        on(n.year(), n.month(), day)
    }
}

/// How each card's caps, rent count and credit stand on `today`.
pub fn states(
    wallet: &Wallet,
    rows: &[SpendRow],
    balances: &BTreeMap<String, i64>,
    pending: &BTreeMap<String, Vec<String>>,
    today: NaiveDate,
) -> Vec<CardState> {
    let rent = wallet.rent.as_ref();
    wallet
        .cards
        .iter()
        .map(|card| {
            let mine: Vec<&SpendRow> = rows
                .iter()
                .filter(|r| r.date <= today && card.accounts.contains(&r.account))
                .collect();
            let cat_of = |r: &SpendRow| {
                counted_on(
                    card,
                    reward_category(&r.category, &wallet.overrides),
                    rent,
                    r.date,
                )
                .to_string()
            };
            let mut caps = Vec::new();
            for cap in &card.caps {
                let (start, resets, guessed) = period_bounds(card, cap.period, today);
                let groups: Vec<Vec<String>> = if cap.per_category {
                    cap.categories.iter().map(|c| vec![c.clone()]).collect()
                } else {
                    vec![cap.categories.clone()]
                };
                for group in groups {
                    let used: i64 = mine
                        .iter()
                        .filter(|r| r.date >= start && group.contains(&cat_of(r)))
                        .map(|r| r.cents)
                        .sum();
                    caps.push(CapState {
                        categories: group,
                        limit: cap.limit,
                        used: used.max(0),
                        start,
                        resets,
                        guessed,
                        after_rate: cap.after_rate,
                    });
                }
            }
            let rent_count = rent
                .filter(|_| {
                    card.rent_max.is_some()
                        || card.caps.iter().any(|c| {
                            c.categories
                                .iter()
                                .any(|x| x == counted_on(card, "rent", rent, today))
                        })
                })
                .map(|r| {
                    let since = match (card.rent_year_start, card.cap_reset) {
                        (Some(md), _) | (None, Some(md)) => last_yearly(today, md),
                        (None, None) => caps
                            .iter()
                            .find(|c| {
                                c.categories
                                    .iter()
                                    .any(|x| x == counted_on(card, "rent", rent, today))
                            })
                            .map_or(today - Months::new(12) + chrono::Days::new(1), |c| c.start),
                    };
                    let used = mine
                        .iter()
                        .filter(|x| {
                            x.date >= since
                                && reward_category(&x.category, &wallet.overrides) == "rent"
                                && x.cents as f64 >= r.charge as f64 * RENT_CHARGE_SHARE
                        })
                        .count() as u32;
                    let by_cap = caps
                        .iter()
                        .filter(|c| {
                            c.categories
                                .iter()
                                .any(|x| x == counted_on(card, "rent", rent, today))
                        })
                        .map(|c| {
                            if r.charge > 0 {
                                (c.left() / r.charge) as u32
                            } else {
                                0
                            }
                        })
                        .min();
                    let by_rule = card.rent_max.map(|m| m.saturating_sub(used));
                    let left = match (by_rule, by_cap) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    RentCount {
                        used,
                        max: card.rent_max,
                        since,
                        left,
                    }
                });
            CardState {
                id: card.id.clone(),
                name: card.name.clone(),
                caps,
                rent: rent_count,
                balance: balances.get(&card.id).copied(),
                limit: card.limit,
                target: card.target,
                next_close: card.statement_close_day.map(|d| next_close(today, d)),
                data_until: mine.iter().map(|r| r.date).max(),
                pending: pending.get(&card.id).cloned().unwrap_or_default(),
            }
        })
        .collect()
}

/// (net rate today, smallest applicable cap left with its reset).
fn rate_now(
    card: &Card,
    state: &CardState,
    cat: &str,
    wallet: &Wallet,
    today: NaiveDate,
) -> (f64, CapLeft) {
    let rent = wallet.rent.as_ref();
    let base = if cat == "foreign" { "foreign" } else { cat };
    let c = counted_on(card, base, rent, today);
    let earn = |k: &str| {
        card.earn
            .get(k)
            .or(card.earn.get("other"))
            .copied()
            .unwrap_or(0.0)
    };
    let applies: Vec<&CapState> = state
        .caps
        .iter()
        .filter(|s| s.categories.iter().any(|x| x == c))
        .collect();
    let mut rate = earn(c);
    let mut cap_left = None;
    if let Some(done) = applies
        .iter()
        .filter(|s| s.left() == 0)
        .map(|s| s.after_rate)
        .reduce(f64::min)
    {
        rate = done;
    } else if let Some(s) = applies.iter().min_by_key(|s| s.left()) {
        cap_left = Some((s.left(), (!s.guessed).then_some(s.resets)));
    }
    if cat == "foreign" {
        rate -= card.fx_fee;
    }
    if cat == "rent" {
        rate -= rent.map_or(0.0, |r| r.fee_rate);
        if state.rent.as_ref().and_then(|r| r.left) == Some(0) && card.rent_max.is_some() {
            // The rent rule's count is used up: rent goes to the next card.
            rate = f64::MIN;
        }
    }
    (rate, cap_left)
}

/// The cap `cat` shares with rent on this card while the card still takes rent
/// charges: (cap left, cents the charges need, charges left, rate after the cap).
fn rent_reserve(
    card: &Card,
    state: &CardState,
    cat: &str,
    wallet: &Wallet,
    today: NaiveDate,
) -> Option<(i64, i64, u32, f64)> {
    let rent = wallet.rent.as_ref()?;
    let charges = state.rent.as_ref()?.left.filter(|&n| n > 0)?;
    if cat == "rent" {
        return None;
    }
    let (rc, cc) = (
        counted_on(card, "rent", Some(rent), today),
        counted_on(card, cat, Some(rent), today),
    );
    let shared: Vec<&CapState> = state
        .caps
        .iter()
        .filter(|s| s.categories.iter().any(|x| x == rc) && s.categories.iter().any(|x| x == cc))
        .collect();
    let left = shared.iter().map(|s| s.left()).min()?;
    let after = shared.iter().map(|s| s.after_rate).fold(f64::MAX, f64::min);
    Some((left, i64::from(charges) * rent.charge, charges, after))
}

/// The categories worth a line: those with spending in the last year, every
/// category a wallet card names, rent when the profile has it, and "other".
fn categories(wallet: &Wallet, rows: &[SpendRow], today: NaiveDate) -> Vec<String> {
    let year_ago = today - Months::new(12);
    let mut seen: BTreeSet<String> = rows
        .iter()
        .filter(|r| r.date > year_ago && r.date <= today && r.cents > 0)
        .map(|r| {
            if r.foreign {
                "foreign".to_string()
            } else {
                reward_category(&r.category, &wallet.overrides).to_string()
            }
        })
        .filter(|c| c != "bank_only")
        .collect();
    seen.extend(wallet.cards.iter().flat_map(|c| c.earn.keys().cloned()));
    if wallet.rent.is_some() {
        seen.insert("rent".into());
    }
    seen.insert("other".into());
    seen.into_iter().collect()
}

/// The best card per category today, from the card states.
pub fn now(
    wallet: &Wallet,
    rows: &[SpendRow],
    balances: &BTreeMap<String, i64>,
    pending: &BTreeMap<String, Vec<String>>,
    today: NaiveDate,
) -> Report {
    let cards = states(wallet, rows, balances, pending, today);
    let mut notes = Vec::new();
    for (card, state) in wallet.cards.iter().zip(&cards) {
        if card.accounts.is_empty() && (!card.caps.is_empty() || card.rent_max.is_some()) {
            notes.push(format!(
                "{}: no `accounts` in profile.toml, so no rows are counted; caps show as unused. Add the export account names that hold this card.",
                card.name
            ));
        }
        if state.caps.iter().any(|c| c.guessed) {
            notes.push(format!(
                "{}: cap reset date unknown, so the last 12 months are counted (this can only overcount). Set `cap_reset = \"MM-DD\"` on the card in profile.toml once the issuer confirms it.",
                card.name
            ));
        }
        if let Some(until) = state.data_until
            && until + chrono::Days::new(STALE_DAYS) < today
        {
            notes.push(format!(
                "{}: its rows end {until}, so newer spending is not counted. Import fresh exports (`bin/cashcast fetch plan`) for exact caps.",
                card.name
            ));
        }
        for task in &state.pending {
            notes.push(format!(
                "{}: a product change is pending (task `{task}`). It is counted as the current card until you mark the task done.",
                card.name
            ));
        }
        if let (Some(b), Some(t), Some(close)) = (state.balance, state.target, state.next_close)
            && b > t
        {
            notes.push(format!(
                "{}: balance is above its target of {}; pay it down before the statement closes on {}.",
                card.name,
                fmt_cents(t),
                close.format("%a %b %d")
            ));
        }
    }
    let rent_charge = wallet.rent.as_ref().map_or(0, |r| r.charge);
    // Where rent pushed off a capped card lands: the best card with no rent
    // limit, else a bank transfer at 0 %.
    let rent_fallback = wallet
        .cards
        .iter()
        .zip(&cards)
        .filter(|(_, s)| s.rent.is_none())
        .map(|(c, s)| rate_now(c, s, "rent", wallet, today).0)
        .fold(0.0, f64::max);
    let mut picks = Vec::new();
    for cat in categories(wallet, rows, today) {
        let mut options: Vec<(f64, CapLeft, usize)> = Vec::new();
        let mut skipped = Vec::new();
        for (i, (card, state)) in wallet.cards.iter().zip(&cards).enumerate() {
            let (rate, cap_left) = rate_now(card, state, &cat, wallet, today);
            if rate == f64::MIN {
                skipped.push(format!("{}: rent count used up", card.name));
                continue;
            }
            let need = if cat == "rent" { rent_charge.max(1) } else { 1 };
            if let Some(left) = state.credit_left()
                && left < need
            {
                skipped.push(format!(
                    "{}: credit left {} is too little",
                    card.name,
                    fmt_cents(left)
                ));
                continue;
            }
            options.push((rate, cap_left, i));
        }
        // A cap shared with rent goes to rent first when rent gains more on it
        // than this category gains over its next best card. Best rate first, so
        // the next best already counts any cap saved on a better card.
        options.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        for k in 0..options.len() {
            let i = options[k].2;
            let (card, state) = (&wallet.cards[i], &cards[i]);
            let Some((left, saved, charges, after)) =
                rent_reserve(card, state, &cat, wallet, today)
            else {
                continue;
            };
            let next = options
                .iter()
                .filter(|o| o.2 != i)
                .map(|o| o.0)
                .fold(0.0, f64::max);
            let opt = &mut options[k];
            let rent_gain = rate_now(card, state, "rent", wallet, today).0 - rent_fallback;
            if rent_gain <= opt.0 - next {
                continue;
            }
            let room = left - saved;
            if room >= rent_charge.max(1) {
                opt.1 = opt.1.map(|(l, resets)| (l.min(room), resets));
            } else {
                opt.0 = after - if cat == "foreign" { card.fx_fee } else { 0.0 };
                opt.1 = None;
                skipped.push(format!(
                    "{}: cap saved for rent ({charges} charges left)",
                    card.name
                ));
            }
        }
        // Best rate first; on a tie, the card with no cap or the most cap left, then wallet order.
        options.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let room = |o: &CapLeft| o.map_or(i64::MAX, |c| c.0);
                    room(&b.1).cmp(&room(&a.1))
                })
                .then(a.2.cmp(&b.2))
        });
        let Some(&(rate, cap_left, i)) = options.first() else {
            picks.push(Pick {
                category: cat,
                card: String::new(),
                rate: 0.0,
                cap_left: None,
                credit_left: None,
                then: None,
                skipped,
            });
            continue;
        };
        let best_rate = rate;
        picks.push(Pick {
            category: cat,
            card: wallet.cards[i].id.clone(),
            rate: best_rate,
            cap_left,
            credit_left: cards[i].credit_left(),
            then: options
                .get(1)
                .map(|&(r, _, j)| (wallet.cards[j].id.clone(), r)),
            skipped,
        });
    }
    Report {
        today,
        cards,
        picks,
        notes,
    }
}

fn pct(rate: f64) -> String {
    let p = rate * 100.0;
    if (p - p.round()).abs() < 0.005 {
        format!("{p:.0}%")
    } else {
        format!("{p:.2}%")
    }
}

fn day(d: NaiveDate) -> String {
    d.format("%a %b %d %Y").to_string()
}

pub fn render(wallet: &Wallet, report: &Report) -> String {
    let name = |id: &str| {
        wallet
            .cards
            .iter()
            .find(|c| c.id == id)
            .map_or(id.to_string(), |c| c.name.clone())
    };
    let mut o = String::new();
    let _ = writeln!(o, "# Which card to use now ({})\n", day(report.today));
    let _ = writeln!(
        o,
        "Counted from imported card rows (see \"Rows until\"); spending after a card's last row is not counted yet. Rates are net of FX and rent service fees.\n"
    );
    let _ = writeln!(o, "| Spending | Use now | Rate | Why | Next best |");
    let _ = writeln!(o, "|---|---|---|---|---|");
    for p in &report.picks {
        if p.card.is_empty() {
            let _ = writeln!(
                o,
                "| {} | bank transfer | 0% | no card can take it: {} | — |",
                p.category,
                p.skipped.join("; ")
            );
            continue;
        }
        let mut why = Vec::new();
        match p.cap_left {
            Some((left, Some(resets))) => why.push(format!(
                "cap left {} until {}",
                fmt_cents(left),
                day(resets)
            )),
            Some((left, None)) => {
                why.push(format!("cap left {} (reset date unknown)", fmt_cents(left)))
            }
            None => why.push("no cap left to watch".into()),
        }
        match p.credit_left {
            Some(c) => why.push(format!("credit left {}", fmt_cents(c))),
            None => why.push("balance not read".into()),
        }
        why.extend(p.skipped.iter().map(|s| format!("skipped {s}")));
        let then = p.then.as_ref().map_or("—".to_string(), |(id, r)| {
            format!("{} ({})", name(id), pct(*r))
        });
        let _ = writeln!(
            o,
            "| {} | **{}** | {} | {} | {} |",
            p.category,
            name(&p.card),
            pct(p.rate),
            why.join(" · "),
            then
        );
    }
    let _ = writeln!(o, "\n## Caps per card\n");
    let _ = writeln!(
        o,
        "| Card | Cap on | Used | Left | Counted since | Resets | Rows until |"
    );
    let _ = writeln!(o, "|---|---|---|---|---|---|---|");
    for s in &report.cards {
        let until = s
            .data_until
            .map_or("no rows".to_string(), |d| d.to_string());
        if s.caps.is_empty() {
            let _ = writeln!(o, "| {} | no caps | — | — | — | — | {until} |", s.name);
        }
        for c in &s.caps {
            let since = if c.guessed {
                format!("{} (guess)", c.start)
            } else {
                c.start.to_string()
            };
            let _ = writeln!(
                o,
                "| {} | {} | {} of {} | {} | {} | {} | {until} |",
                s.name,
                c.categories.join(", "),
                fmt_cents(c.used),
                fmt_cents(c.limit),
                fmt_cents(c.left()),
                since,
                if c.guessed {
                    "unknown".to_string()
                } else {
                    day(c.resets)
                }
            );
        }
    }
    let rents: Vec<&CardState> = report.cards.iter().filter(|s| s.rent.is_some()).collect();
    if !rents.is_empty() {
        let _ = writeln!(o, "\n## Rent charges\n");
        for s in rents {
            let r = s.rent.as_ref().expect("filtered");
            let max = r.max.map_or(String::new(), |m| format!(" of {m}"));
            let left = r.left.map_or("unknown".to_string(), |l| l.to_string());
            let _ = writeln!(
                o,
                "- {}: {}{max} charges since {} · {left} left",
                s.name, r.used, r.since
            );
        }
    }
    let _ = writeln!(o, "\n## Credit\n");
    let _ = writeln!(
        o,
        "| Card | Balance | Limit | Credit left | Target at close | Statement closes |"
    );
    let _ = writeln!(o, "|---|---|---|---|---|---|");
    let opt = |v: Option<i64>| v.map_or("—".to_string(), fmt_cents);
    for s in &report.cards {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} | {} |",
            s.name,
            s.balance.map_or("not read".to_string(), fmt_cents),
            opt(s.limit),
            opt(s.credit_left()),
            opt(s.target),
            s.next_close
                .map_or("—".to_string(), |d| d.format("%a %b %d").to_string())
        );
    }
    if !report.notes.is_empty() {
        let _ = writeln!(o, "\n## Check\n");
        for n in &report.notes {
            let _ = writeln!(o, "- {n}");
        }
    }
    o
}
