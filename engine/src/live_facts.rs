//! Live facts from the Bank of Canada Valet API (`knowledge refresh`).
//!
//! Hand-entered facts in `knowledge/data/` age between reviews. This module
//! reads a fixed list of official Bank of Canada series and compares them with
//! the planning facts the investment recommender uses, so a changed world shows
//! up without waiting for the next manual review.
//!
//! Safety rules:
//! - The request URLs are constants: series IDs only. No user data, amount,
//!   account or ticker ever goes into a request. No key, no consent needed.
//! - Read only: one GET per series. Replies are cached per day in
//!   `<private root>/live/` (mode 600), so a rerun on the same day is offline.
//! - A reply that does not parse, or a value out of its sane range, is shown
//!   as "unavailable" and never replaces a fact.

use chrono::{Datelike, NaiveDate};

use crate::core::http::{api_error, send};
use crate::core::paths::{self, AppError};
use crate::core::pyjson::{self, Json};

const VALET: &str = "https://www.bankofcanada.ca/valet/observations";

/// How a series turns into a fact value (a fraction, 0.025 = 2.5%).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The latest value is a percent.
    Percent,
    /// The latest value is a plain number (an exchange rate).
    Level,
    /// The value is an index; the fact is its change over 12 months.
    YearOverYear,
}

pub struct Series {
    pub id: &'static str,
    pub fact: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// Sane range for the fact value; outside it the reply is not trusted.
    pub range: (f64, f64),
    /// The hand-entered fact in `invest_evidence.toml` this one checks, if any.
    pub checks: Option<&'static str>,
    /// Oldest observation still trusted, in days: daily and weekly series go
    /// stale fast, monthly CPI lags about six weeks.
    pub max_age_days: i64,
}

/// The official series cashcast follows. Add one here with a test.
pub const SERIES: &[Series] = &[
    Series {
        id: "V39079",
        fact: "boc-policy-rate",
        label: "Bank of Canada policy rate",
        kind: Kind::Percent,
        range: (0.0, 0.15),
        checks: None,
        max_age_days: 10,
    },
    Series {
        id: "V41690973",
        fact: "cpi-yoy",
        label: "Inflation, total CPI over 12 months",
        kind: Kind::YearOverYear,
        range: (-0.05, 0.15),
        checks: Some("pag-inflation"),
        max_age_days: 75,
    },
    Series {
        id: "V80691311",
        fact: "prime-rate",
        label: "Prime rate of chartered banks",
        kind: Kind::Percent,
        range: (0.005, 0.2),
        checks: None,
        max_age_days: 10,
    },
    Series {
        id: "V80691339",
        fact: "gic-1y",
        label: "1-year GIC rate at chartered banks",
        kind: Kind::Percent,
        range: (0.002, 0.1),
        checks: Some("pag-short-term"),
        max_age_days: 14,
    },
    Series {
        id: "V80691341",
        fact: "gic-5y",
        label: "5-year GIC rate at chartered banks",
        kind: Kind::Percent,
        range: (0.002, 0.1),
        checks: None,
        max_age_days: 14,
    },
    Series {
        id: "BD.CDN.10YR.DQ.YLD",
        fact: "goc-10y",
        label: "Government of Canada 10-year bond yield",
        kind: Kind::Percent,
        range: (0.002, 0.12),
        checks: Some("pag-fixed-income"),
        max_age_days: 10,
    },
    Series {
        id: "FXUSDCAD",
        fact: "usd-cad",
        label: "US dollar in Canadian dollars",
        kind: Kind::Level,
        range: (0.9, 2.0),
        checks: None,
        max_age_days: 10,
    },
];

#[derive(Debug, Clone, PartialEq)]
pub struct LiveFact {
    pub fact: String,
    pub label: String,
    pub series: String,
    pub value: Option<f64>,
    pub date: String,
    pub note: String,
}

/// The URL for one series. 13 observations cover a 12-month change.
pub fn url(series: &Series) -> String {
    let recent = if series.kind == Kind::YearOverYear {
        13
    } else {
        1
    };
    format!("{VALET}/{}/json?recent={recent}", series.id)
}

/// (date, value) pairs, oldest first, from one Valet reply.
fn observations(body: &str, id: &str) -> Vec<(String, f64)> {
    let Some(j) = pyjson::loads(body) else {
        return Vec::new();
    };
    let mut out: Vec<(String, f64)> = Vec::new();
    if let Some(Json::List(rows)) = j.get("observations") {
        for row in rows {
            let date = match row.get("d") {
                Some(Json::Str(d)) => d.clone(),
                _ => continue,
            };
            let value = match row.get(id).and_then(|v| v.get("v")) {
                Some(Json::Str(v)) => v.trim().parse::<f64>().ok(),
                _ => None,
            };
            if let Some(v) = value.filter(|v| v.is_finite()) {
                out.push((date, v));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// One fact from one reply. Never panics; a bad reply gives `value: None`.
pub fn parse(series: &Series, body: &str) -> LiveFact {
    let obs = observations(body, series.id);
    let mut fact = LiveFact {
        fact: series.fact.to_string(),
        label: series.label.to_string(),
        series: series.id.to_string(),
        value: None,
        date: String::new(),
        note: String::new(),
    };
    let Some((date, last)) = obs.last().cloned() else {
        fact.note = "unavailable: no observations".into();
        return fact;
    };
    fact.date = date.clone();
    let value = match series.kind {
        Kind::Percent => Some(last / 100.0),
        Kind::Level => Some(last),
        Kind::YearOverYear => year_ago(&obs, &date).map(|old| last / old - 1.0),
    };
    match value {
        None => fact.note = "unavailable: no value 12 months back".into(),
        Some(v) if v < series.range.0 || v > series.range.1 => {
            fact.note = format!("unavailable: {v:.4} is outside the sane range");
        }
        Some(v) => fact.value = Some(v),
    }
    fact
}

fn year_ago(obs: &[(String, f64)], date: &str) -> Option<f64> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let target = NaiveDate::from_ymd_opt(d.year() - 1, d.month(), d.day())?.to_string();
    obs.iter()
        .find(|(day, v)| *day == target && *v > 0.0)
        .map(|(_, v)| *v)
}

fn cache_dir() -> std::path::PathBuf {
    paths::private_root().join("live")
}

/// Seconds to wait for one reply: the start screen must stay fast offline.
const TIMEOUT: u32 = 5;

/// Fetch every series (or read today's cache) and parse it. After the first
/// failed request the rest are skipped (one short wait when offline), and
/// `auto` does not retry that day.
pub fn refresh(today: NaiveDate) -> Result<Vec<LiveFact>, AppError> {
    let dir = cache_dir();
    let mut out = Vec::new();
    let mut down: Option<String> = None;
    for series in SERIES {
        let cache = dir.join(format!("boc-{}-{today}.json", series.id));
        let fetched = match (std::fs::read_to_string(&cache), &down) {
            (Ok(text), _) => Ok(text),
            (Err(_), Some(why)) => Err(why.clone()),
            (Err(_), None) => {
                let headers = vec![("User-Agent".to_string(), "cashcast".to_string())];
                match send("GET", &url(series), &headers, None, TIMEOUT) {
                    Ok(r) if r.status < 400 => {
                        let text = r.text();
                        // Cache only a reply that gives a value: a proxy page is retried.
                        if parse(series, &text).value.is_some() {
                            std::fs::create_dir_all(&dir).map_err(AppError::io)?;
                            crate::write_private(&cache, &text).map_err(AppError::io_text)?;
                        }
                        Ok(text)
                    }
                    Ok(r) => Err(format!("HTTP {}", r.status)),
                    Err(e) => {
                        down = Some(e.message.clone());
                        Err(e.message)
                    }
                }
            }
        };
        match fetched {
            Ok(body) => out.push(parse(series, &body)),
            Err(why) => out.push(LiveFact {
                fact: series.fact.into(),
                label: series.label.into(),
                series: series.id.into(),
                value: None,
                date: String::new(),
                note: format!("unavailable: {why}"),
            }),
        }
    }
    if out.iter().all(|f| f.value.is_none()) {
        return Err(api_error(
            "Bank of Canada Valet API gave no usable value; facts unchanged",
        ));
    }
    Ok(out)
}

/// A hand fact's value from `invest_evidence.toml`, by id.
fn hand_value(evidence: &toml::Table, id: &str) -> Option<(f64, String)> {
    let facts = evidence.get("facts")?.as_array()?;
    facts.iter().find_map(|f| {
        let t = f.as_table()?;
        (t.get("id")?.as_str()? == id).then(|| {
            let v = t
                .get("value")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                .unwrap_or(f64::NAN);
            let as_of = t
                .get("as_of")
                .map(|d| d.as_str().map_or_else(|| d.to_string(), str::to_string))
                .unwrap_or_default();
            (v, as_of)
        })
    })
}

fn pct(v: f64) -> String {
    format!("{:.2}%", v * 100.0)
}

/// The report: live values, and the hand fact each one checks.
/// A live value more than 1.5 points from a long-run assumption is flagged:
/// not wrong by itself (assumptions are long-run), but worth a review.
pub fn render(facts: &[LiveFact], evidence: &toml::Table) -> (String, usize) {
    let mut s = String::from("# Live facts (Bank of Canada Valet API)\n\n");
    s.push_str("Only fixed series IDs left this computer (with your IP address, as for any web page); no personal data.\n\n");
    s.push_str("| Fact | Live value | Date | Checks | Hand value | Flag |\n");
    s.push_str("|---|---|---|---|---|---|\n");
    let mut flags = 0;
    for f in facts {
        let series = SERIES.iter().find(|x| x.fact == f.fact);
        let live = match (f.value, series.map(|x| x.kind)) {
            (Some(v), Some(Kind::Level)) => format!("{v:.4}"),
            (Some(v), _) => pct(v),
            (None, _) => f.note.clone(),
        };
        let checks = series.and_then(|x| x.checks);
        let (hand, flag) = match (checks.and_then(|c| hand_value(evidence, c)), f.value) {
            (Some((h, as_of)), Some(v)) if h.is_finite() => {
                let far = (v - h).abs() > 0.015;
                if far {
                    flags += 1;
                }
                (
                    format!("{} (as of {as_of})", pct(h)),
                    if far { "review" } else { "ok" }.to_string(),
                )
            }
            _ => ("-".into(), "-".into()),
        };
        s.push_str(&format!(
            "| {} | {live} | {} | {} | {hand} | {flag} |\n",
            f.label,
            f.date,
            checks.unwrap_or("-"),
        ));
    }
    let live = facts.iter().filter(|f| f.value.is_some()).count();
    s.push_str(&format!(
        "\n- Refreshed: {live} of {} facts.\n- Flagged for review: {flags}.\n",
        facts.len()
    ));
    if flags > 0 {
        s.push_str("- A flag means today differs from a long-run planning assumption by more than 1.5 points. Long-run numbers may still hold; check the source at its next release (`knowledge propose`).\n");
    }
    (s, flags)
}

/// Facts as JSON for the web app and the agent (`data/live-facts.json` shape).
pub fn to_json(facts: &[LiveFact]) -> String {
    let rows: Vec<String> = facts
        .iter()
        .map(|f| {
            let v = f.value.map_or("null".to_string(), |v| format!("{v}"));
            format!(
                "{{\"fact\": {}, \"series\": {}, \"value\": {v}, \"date\": {}}}",
                serde_json::to_string(&f.fact).unwrap_or_default(),
                serde_json::to_string(&f.series).unwrap_or_default(),
                serde_json::to_string(&f.date).unwrap_or_default(),
            )
        })
        .collect();
    format!("[{}]\n", rows.join(", "))
}

/// Write the latest facts to `<private root>/live/latest.json` (mode 600).
/// A series that failed this time keeps its last good row, so one bad reply
/// never erases a good value (`as_facts` still drops it once it is too old).
pub fn save(facts: &[LiveFact]) -> Result<std::path::PathBuf, AppError> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(AppError::io)?;
    let path = dir.join("latest.json");
    let old = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| pyjson::loads(&t));
    let mut merged = facts.to_vec();
    if let Some(Json::List(rows)) = old {
        for f in merged.iter_mut().filter(|f| f.value.is_none()) {
            let prev = rows
                .iter()
                .find(|r| matches!(r.get("fact"), Some(Json::Str(x)) if *x == f.fact));
            let value = match prev.and_then(|r| r.get("value")) {
                Some(Json::Float(v)) => Some(*v),
                Some(Json::Int(v)) => Some(*v as f64),
                _ => None,
            };
            if let (Some(v), Some(Json::Str(d))) = (value, prev.and_then(|r| r.get("date"))) {
                f.value = Some(v);
                f.date = d.clone();
                f.note = format!("kept from an earlier refresh ({})", f.note);
            }
        }
    }
    crate::write_private(&path, &to_json(&merged)).map_err(AppError::io_text)?;
    Ok(path)
}

/// The automatic refresh run by the start screen and `sync all`: one line, never
/// an error. Off with setting `live.refresh = false` or `CASHCAST_LIVE=off`
/// (the test harnesses set it, so tests never touch the network).
pub fn auto(today: NaiveDate) -> Option<String> {
    if std::env::var("CASHCAST_LIVE").is_ok_and(|v| v == "off") {
        return None;
    }
    if !crate::core::settings::get("live.refresh").is_ok_and(|v| v.truthy()) {
        return None;
    }
    let tried = cache_dir().join(format!("failed-{today}"));
    if tried.exists() {
        return Some("- Live rates: not refreshed today (offline earlier); facts unchanged".into());
    }
    let evidence = crate::knowledge::load("invest_evidence").unwrap_or_default();
    Some(match refresh(today) {
        Ok(facts) => {
            let _ = save(&facts);
            let (_, flags) = render(&facts, &evidence);
            let live = facts.iter().filter(|f| f.value.is_some()).count();
            format!(
                "- Live rates: {live} of {} refreshed from the Bank of Canada, {flags} flagged (details: bin/cashcast knowledge refresh)",
                facts.len()
            )
        }
        Err(e) => {
            // Try at most once a day while offline.
            let _ = std::fs::create_dir_all(cache_dir());
            let _ = crate::write_private(&tried, "");
            format!(
                "- Live rates: not refreshed ({}); facts unchanged",
                e.message
            )
        }
    })
}

/// Saved live facts no older than 90 days, as `invest_evidence`-shaped facts
/// with id `live-<fact>`, so `invest-plan` cites them like any other fact.
pub fn as_facts(today: NaiveDate) -> Vec<(String, Json)> {
    let Ok(text) = std::fs::read_to_string(cache_dir().join("latest.json")) else {
        return Vec::new();
    };
    let Some(Json::List(rows)) = pyjson::loads(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        let (Some(Json::Str(fact)), Some(Json::Str(date))) = (row.get("fact"), row.get("date"))
        else {
            continue;
        };
        let value = match row.get("value") {
            Some(Json::Float(v)) => *v,
            Some(Json::Int(v)) => *v as f64,
            _ => continue,
        };
        let Some(series) = SERIES.iter().find(|s| s.fact == fact) else {
            continue;
        };
        let Ok(day) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            continue;
        };
        let age = (today - day).num_days();
        if !(0..=series.max_age_days).contains(&age)
            || value < series.range.0
            || value > series.range.1
        {
            continue;
        }
        let s = |t: &str| Json::Str(t.to_string());
        out.push((
            format!("live-{fact}"),
            Json::Obj(vec![
                ("id".into(), s(&format!("live-{fact}"))),
                ("value".into(), Json::Float(value)),
                (
                    "text".into(),
                    s(&format!("{} (Bank of Canada, live).", series.label)),
                ),
                ("source".into(), s(&url(series))),
                ("as_of".into(), s(date)),
                (
                    "review_by".into(),
                    s(&(day + chrono::Duration::days(series.max_age_days)).to_string()),
                ),
                ("confidence".into(), s("verified")),
            ]),
        ));
    }
    out
}

/// A yearly expert source with no API: a public page that names its newest
/// edition. `needle` holds `{next}`, the year after the fact's `as_of` year.
pub struct Edition {
    pub name: &'static str,
    pub url: &'static str,
    pub needle: &'static str,
    /// The `invest_evidence.toml` fact whose `as_of` gives the current edition.
    pub fact: &'static str,
}

/// Watched editions. SPIVA (403 to scripts) and CRA limit pages (time out)
/// cannot be fetched reliably; they stay on `review_by` dates.
pub const EDITIONS: &[Edition] = &[Edition {
    name: "FP Canada Projection Assumption Guidelines",
    url: "https://www.fpcanada.ca/projection-assumption-guidelines",
    needle: "{next} Projection Assumption Guidelines",
    fact: "pag-equity",
}];

/// The needle for the edition after the one `as_of` (YYYY-MM-DD) belongs to.
pub fn next_needle(e: &Edition, as_of: &str) -> Option<String> {
    let year: i32 = as_of.get(0..4)?.parse().ok()?;
    Some(e.needle.replace("{next}", &(year + 1).to_string()))
}

/// One line per watched edition: "new edition out", "current", or "unknown".
pub fn editions(evidence: &toml::Table, today: NaiveDate) -> Vec<String> {
    let dir = cache_dir();
    EDITIONS
        .iter()
        .map(|e| {
            let Some((_, as_of)) = hand_value(evidence, e.fact) else {
                return format!("- {}: unknown (fact {} missing)", e.name, e.fact);
            };
            let Some(needle) = next_needle(e, &as_of) else {
                return format!("- {}: unknown (bad as_of {as_of})", e.name);
            };
            let cache = dir.join(format!("edition-{}-{today}.html", e.fact));
            let current = e.needle.replace("{next}", as_of.get(0..4).unwrap_or(""));
            let body = std::fs::read_to_string(&cache).ok().or_else(|| {
                let headers = vec![("User-Agent".to_string(), "Mozilla/5.0 cashcast".to_string())];
                let r = send("GET", e.url, &headers, None, TIMEOUT * 2).ok()?;
                (r.status < 400).then(|| {
                    let text = r.text();
                    // Cache only a real guidelines page: a captive portal or
                    // proxy page must not stick for the whole day.
                    let page = plain(&text);
                    if page.contains(&needle) || page.contains(&current) {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = crate::write_private(&cache, &text);
                    }
                    text
                })
            });
            match body.map(|b| plain(&b)) {
                None => format!("- {}: unknown (page not reachable)", e.name),
                Some(b) if b.contains(&needle) => format!(
                    "- {}: NEW EDITION \"{needle}\" is out. Update `{}` and related facts with `knowledge propose` ({})",
                    e.name, e.fact, e.url
                ),
                Some(b) if !b.contains(&current) => format!(
                    "- {}: unknown (page layout changed; check {} by hand)",
                    e.name, e.url
                ),
                Some(_) => format!("- {}: current (edition of {as_of}; no \"{needle}\" yet)", e.name),
            }
        })
        .collect()
}

/// Page text with tags removed and `&nbsp;` and whitespace runs as one space.
pub fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.replace("&nbsp;", " ").replace("&#160;", " ").chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
