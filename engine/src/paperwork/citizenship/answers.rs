//! Judges for the Canadian citizenship application (ported from sidequest
//! `citizenship-answers.ts`). The owner keeps every answer in one sealed file;
//! Claude guides the owner through the IRCC form. Two judges:
//!   `judge_answers`: is every answer there, well formed and within the rules?
//!   `judge_screen`:  does what the owner typed on a form screen match?
//! Both are pure: same input, same verdict. They never print a value.

use chrono::{Datelike, Duration, Months, NaiveDate};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

/// The answers, as parsed from the TOML draft. Keys are the `QUESTIONS` ids,
/// plus `_sources`, `_marks`, `_collected` and `_proofs`.
pub type Answers = Map<String, Value>;

// Official rules (CIT 0002; the paperwork facts hold the sources).
pub const PRESENCE_DAYS: i64 = 1095;
pub const WINDOW_YEARS: i32 = 5;
pub const TEMP_CAP: i64 = 365;
pub const POLICE_DAYS: i64 = 183;
pub const POLICE_YEARS: i32 = 4;
pub const AGE_MIN: i32 = 18;
pub const AGE_MAX: i32 = 54;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub id: String,
    pub level: &'static str, // OK, WARN, FAIL, ASK
    pub msg: String,
}

pub const MARITAL: &[&str] = &[
    "single",
    "married",
    "common-law",
    "divorced",
    "separated",
    "widowed",
    "annulled",
];
pub const LANGUAGE_PROOF: &[&str] = &[
    "diploma",
    "transcript",
    "celpip",
    "celpip-ls",
    "ielts",
    "pte-core",
    "tef",
    "tcf",
    "linc",
    "clic",
    "provincial",
    "none",
];
/// With a PR card, the other ID must come from a Canadian government.
pub const CANADIAN_GOV_IDS: &[&str] = &[
    "drivers-licence",
    "health-card",
    "provincial-photo-id",
    "other-canadian-government",
];
pub fn id_kinds() -> Vec<&'static str> {
    let mut v = vec!["pr-card"];
    v.extend_from_slice(CANADIAN_GOV_IDS);
    v.extend_from_slice(&["foreign-passport", "foreign-id"]);
    v
}

const BOOL_KEYS: &[&str] = &[
    "name_changed_since_pr",
    "prohibitions_none",
    "representative",
    "consent_mp",
    "consent_electors",
];

/// The questions, one per answer. Missing ones are the owner's batch.
pub fn questions() -> Vec<(&'static str, String)> {
    let q = |k: &'static str, s: &str| (k, s.to_string());
    vec![
        q(
            "method",
            "Will you apply online or on paper? (most people apply online)",
        ),
        q("service_language", "English or French for IRCC's service?"),
        q(
            "accommodation",
            "Do you need an accommodation (wheelchair access, interpreter, large print)? Write none if not",
        ),
        q(
            "uci",
            "Your UCI (client ID, 8 or 10 digits): next to the photo on your PR card, or top right on your Confirmation of PR",
        ),
        q(
            "name",
            "Your family and given names, exactly as on your PR card or Confirmation of PR",
        ),
        q(
            "other_names",
            "Every other name you have used (birth name, name before marriage, nicknames); an empty list if none",
        ),
        q(
            "name_changed_since_pr",
            "Did you legally change your name since you became a PR? (yes or no)",
        ),
        (
            "marital_status",
            format!("Your marital status today ({})", MARITAL.join(", ")),
        ),
        q("birth_date", "Your date of birth (YYYY-MM-DD)"),
        q(
            "pr_date",
            "The day you became a permanent resident (YYYY-MM-DD, on your Confirmation of PR)",
        ),
        q(
            "sign_date",
            "The day you plan to sign and submit (YYYY-MM-DD)",
        ),
        q(
            "contact",
            "Your home address in Canada, postal code, phone with country code, and email",
        ),
        q(
            "temp_periods",
            "Time in Canada before PR as a visitor, student, worker or protected person: from, to, status; an empty list if none",
        ),
        q(
            "trips",
            "Every trip outside Canada in the 5 years before you sign: day you left, day you came back, country; an empty list if none",
        ),
        q(
            "addresses",
            "Every address in the 5 years before you sign, inside and outside Canada: from, to (or present), street address, city, country, postal code; no gaps",
        ),
        q(
            "activities",
            "Everything you did in the 5 years: from, to (or present), what (work, study, language class, unemployed, caregiver...), employer or school, city, country; no gaps",
        ),
        q(
            "taxes",
            "Your SIN, and for each of the 5 tax years before you sign: did you have to file, did you file (from your CRA account); and may the CRA share your tax details with IRCC?",
        ),
        q(
            "other_status",
            "Each other country where you held citizenship, PR, or student, worker or refugee status, and which status",
        ),
        q(
            "passports",
            "Every passport or travel document you held in the 5 years: number, country, issue date, expiry date",
        ),
        (
            "ids",
            format!(
                "Which two IDs you will copy (kinds: {})",
                id_kinds().join(", ")
            ),
        ),
        (
            "language_proof",
            format!(
                "Your proof of English or French ({})",
                LANGUAGE_PROOF.join(", ")
            ),
        ),
        q(
            "prohibitions_none",
            "After reading the official list of situations that prevent citizenship: does none apply to you? (yes, or no and talk to a lawyer)",
        ),
        q(
            "representative",
            "Will a person, firm or organization help you fill in the application or act for you? (yes or no)",
        ),
        q(
            "police_certificates",
            "Police certificates you have: country and the day it was issued; an empty list if none",
        ),
        q(
            "certificate",
            "Paper or electronic citizenship certificate?",
        ),
        q(
            "consent_mp",
            "May IRCC share your name and address with your federal MP for a congratulations letter? (yes or no)",
        ),
        q(
            "consent_electors",
            "May IRCC add you to the National Register of Electors? (yes or no)",
        ),
        q("city", "The city you will apply from"),
    ]
}

// ---- small helpers shared by the citizenship modules ----

pub fn parse_day(s: &str) -> Option<NaiveDate> {
    let b = s.as_bytes();
    let shape = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shape {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}
pub fn is_day(s: &str) -> bool {
    parse_day(s).is_some()
}
/// The value as a valid day string.
pub fn vday(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| is_day(s))
}
pub fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}
fn d(s: &str) -> NaiveDate {
    parse_day(s).unwrap_or_default()
}
pub fn minus_months(s: &str, m: u32) -> String {
    iso(d(s).checked_sub_months(Months::new(m)).unwrap_or_default())
}
pub fn plus_days(s: &str, n: i64) -> String {
    iso(d(s) + Duration::days(n))
}
fn up_to<'a>(s: &'a str, sign: &'a str) -> &'a str {
    if s == "present" { sign } else { s }
}
/// Present and not null.
pub fn get<'a>(a: &'a Answers, k: &str) -> Option<&'a Value> {
    a.get(k).filter(|v| !v.is_null())
}
pub fn sfield<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}
pub fn list<'a>(a: &'a Answers, k: &str) -> &'a [Value] {
    a.get(k)
        .and_then(Value::as_array)
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}
/// JavaScript `String(x)`: the text of a value, as messages showed it.
pub fn js(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(x) => x.to_string(),
    }
}
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(_) => true,
    }
}
fn strip(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}
fn all_digits(s: &str, lens: &[usize]) -> bool {
    lens.contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit())
}

/// Days in [from, to] (inclusive) that the spans do not cover. `to` of a span may be "present".
pub fn gaps(spans: &[(String, String)], from: &str, to: &str, sign: &str) -> Vec<(String, String)> {
    let mut s: Vec<(String, String)> = spans
        .iter()
        .filter(|(f, t)| is_day(f) && (t == "present" || is_day(t)))
        .map(|(f, t)| (f.clone(), up_to(t, sign).to_string()))
        .collect();
    s.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = vec![];
    let mut cursor = from.to_string(); // first day not yet covered
    for (f, t) in s {
        if t < cursor {
            continue;
        }
        if f.as_str() > to {
            break; // a row after the window leaves the rest of it open
        }
        if f > cursor {
            out.push((cursor.clone(), plus_days(&f, -1)));
        }
        if t >= cursor {
            cursor = plus_days(&t, 1);
        }
        if cursor.as_str() > to {
            return out;
        }
    }
    if cursor.as_str() <= to {
        out.push((cursor, to.to_string()));
    }
    out
}

fn age_on(birth: &str, on: &str) -> i32 {
    let (b, o) = (d(birth), d(on));
    o.year() - b.year() - i32::from((o.month(), o.day()) < (b.month(), b.day()))
}
fn days(a: &str, b: &str) -> i64 {
    (d(b) - d(a)).num_days() + 1
}

#[derive(Debug, Clone)]
pub struct Presence {
    pub total: i64,
    pub needed: i64,
    pub met: bool,
    pub short_by: i64,
    pub earliest: Option<String>,
}

/// Days in Canada in the 5 years before `sign` (the sign day does not count): each day as a
/// PR counts once, each day before PR in a temporary status counts half (at most 365).
/// Trip days strictly between leaving and coming back are away. `earliest` is the first
/// sign day from today (or the day after PR) with enough days, with the trips so far.
pub fn presence(
    pr: &str,
    sign: &str,
    trips: &[(String, String)],
    temps: &[(String, String)],
    today: &str,
) -> Option<Presence> {
    if !is_day(pr) || !is_day(sign) {
        return None;
    }
    let dn = |s: &str| i64::from(d(s).num_days_from_ce());
    let from = std::cmp::max(d(today), d(pr) + Duration::days(1));
    let horizon = 366 * i64::from(WINDOW_YEARS);
    let lo = dn(sign).min(dn(&iso(from))) - horizon - 2;
    let hi = dn(sign).max(dn(&iso(from)) + horizon) + 2;
    let n = (hi - lo + 1) as usize;
    let mut away = vec![false; n];
    let mut temp = vec![false; n];
    for (l, r) in trips.iter().filter(|(l, r)| is_day(l) && is_day(r)) {
        for x in (dn(l) + 1).max(lo)..=(dn(r) - 1).min(hi) {
            away[(x - lo) as usize] = true;
        }
    }
    for (f, t) in temps.iter().filter(|(f, t)| is_day(f) && is_day(t)) {
        for x in dn(f).max(lo)..=dn(t).min(hi) {
            temp[(x - lo) as usize] = true;
        }
    }
    let pr_day = dn(pr);
    let mut pr_sum = vec![0i64; n + 1];
    let mut temp_sum = vec![0i64; n + 1];
    for i in 0..n {
        let in_pr = lo + i as i64 >= pr_day;
        pr_sum[i + 1] = pr_sum[i] + i64::from(!away[i] && in_pr);
        temp_sum[i + 1] = temp_sum[i] + i64::from(!away[i] && !in_pr && temp[i]);
    }
    let total_at = |s: NaiveDate| {
        let start = s
            .checked_sub_months(Months::new(12 * WINDOW_YEARS as u32))
            .unwrap_or_default();
        let a = (i64::from(start.num_days_from_ce()) - lo) as usize;
        let b = (i64::from(s.num_days_from_ce()) - lo) as usize;
        let credit = TEMP_CAP.min((temp_sum[b] - temp_sum[a]) / 2);
        pr_sum[b] - pr_sum[a] + credit
    };
    let total = total_at(d(sign));
    let mut earliest = None;
    let mut x = from;
    for _ in 0..=horizon {
        if total_at(x) >= PRESENCE_DAYS {
            earliest = Some(iso(x));
            break;
        }
        x += Duration::days(1);
    }
    Some(Presence {
        total,
        needed: PRESENCE_DAYS,
        met: total >= PRESENCE_DAYS,
        short_by: (PRESENCE_DAYS - total).max(0),
        earliest,
    })
}

/// Trips and stays as (left, returned) pairs with valid days.
pub fn trip_spans(a: &Answers) -> Vec<(String, String)> {
    list(a, "trips")
        .iter()
        .filter_map(|t| {
            let (l, r) = (vday(t.get("left"))?, vday(t.get("returned"))?);
            Some((l.to_string(), r.to_string()))
        })
        .collect()
}
pub fn temp_spans(a: &Answers) -> Vec<(String, String)> {
    list(a, "temp_periods")
        .iter()
        .filter_map(|t| {
            let (f, to) = (vday(t.get("from"))?, vday(t.get("to"))?);
            Some((f.to_string(), to.to_string()))
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Stay {
    country: String,
    from: String,
    to: String,
}

/// Countries that need a police certificate: 183+ days in a row there in the 4 years before
/// signing, since age 18, before or after PR; not the country lived in right before PR.
pub fn police_countries(a: &Answers) -> Vec<(String, String)> {
    let Some(sign) = vday(get(a, "sign_date")) else {
        return vec![];
    };
    let back = minus_months(sign, 12 * POLICE_YEARS as u32);
    let adult = vday(get(a, "birth_date"))
        .map(|b| {
            iso(d(b)
                .checked_add_months(Months::new(12 * AGE_MIN as u32))
                .unwrap_or_default())
        })
        .unwrap_or_else(|| back.clone());
    let from = if adult > back { adult } else { back };
    let mut stays: Vec<Stay> = vec![];
    for t in list(a, "trips") {
        let inner = t
            .get("stays")
            .and_then(Value::as_array)
            .filter(|s| !s.is_empty());
        if let Some(inner) = inner {
            for s in inner {
                if let (Some(f), Some(to)) = (vday(s.get("from")), vday(s.get("to"))) {
                    stays.push(Stay {
                        country: sfield(s, "country").unwrap_or("").into(),
                        from: f.into(),
                        to: to.into(),
                    });
                }
            }
        } else if let (Some(l), Some(r), Some(c)) = (
            vday(t.get("left")),
            vday(t.get("returned")),
            sfield(t, "country"),
        ) {
            // The day you leave and the day you come back count too: a missing certificate
            // costs more than an extra one.
            if !c.is_empty() {
                stays.push(Stay {
                    country: c.into(),
                    from: l.into(),
                    to: r.into(),
                });
            }
        }
    }
    let mut homes: Vec<Stay> = vec![];
    for x in list(a, "addresses") {
        let c = sfield(x, "country").unwrap_or("");
        if let (false, Some(f)) = (
            c.is_empty() || c.eq_ignore_ascii_case("canada"),
            vday(x.get("from")),
        ) {
            let to = sfield(x, "to").unwrap_or("");
            homes.push(Stay {
                country: c.into(),
                from: f.into(),
                to: up_to(to, sign).into(),
            });
        }
    }
    stays.extend(homes.iter().cloned());
    // Join back-to-back stays in one country.
    stays.sort_by(|p, q| p.from.cmp(&q.from));
    let mut merged: Vec<Stay> = vec![];
    for s in stays {
        if let Some(last) = merged.last_mut()
            && last.country.eq_ignore_ascii_case(&s.country)
            && s.from <= plus_days(&last.to, 1)
        {
            if s.to > last.to {
                last.to = s.to;
            }
            continue;
        }
        merged.push(s);
    }
    // The country of origin: where you lived (an address, never a trip) right before PR,
    // when that home ends within a month of it.
    let pr = vday(get(a, "pr_date"));
    let origin = pr.and_then(|pr| {
        let mut before: Vec<&Stay> = homes.iter().filter(|s| s.from.as_str() < pr).collect();
        before.sort_by(|p, q| p.to.cmp(&q.to));
        before
            .last()
            .filter(|b| b.to >= plus_days(pr, -31))
            .map(|b| b.country.to_lowercase())
    });
    let mut need: Vec<(String, String)> = vec![];
    for s in &merged {
        let lo = if s.from > from {
            s.from.clone()
        } else {
            from.clone()
        };
        let hi = if s.to.as_str() < sign {
            s.to.clone()
        } else {
            sign.to_string()
        };
        if hi < lo || days(&lo, &hi) < POLICE_DAYS {
            continue;
        }
        // Origin is left out only for the time before PR.
        if origin.as_deref() == Some(&s.country.to_lowercase()) {
            let after = match pr {
                Some(p) if p > lo.as_str() => p.to_string(),
                _ => lo.clone(),
            };
            if hi < after || days(&after, &hi) < POLICE_DAYS {
                continue;
            }
        }
        let k = s.country.to_lowercase();
        match need.iter_mut().find(|(c, _)| c.to_lowercase() == k) {
            Some(n) => {
                if n.1 < s.to {
                    n.1 = s.to.clone();
                }
            }
            None => {
                let name = merged
                    .iter()
                    .find(|m| m.country.to_lowercase() == k)
                    .unwrap();
                need.push((name.country.clone(), s.to.clone()));
            }
        }
    }
    need
}

fn as_bool(v: &Value) -> Value {
    if let Value::String(s) = v {
        let t = s.trim().to_lowercase();
        if ["yes", "y", "true", "oui"].contains(&t.as_str()) {
            return Value::Bool(true);
        }
        if ["no", "n", "false", "non"].contains(&t.as_str()) {
            return Value::Bool(false);
        }
    }
    v.clone()
}

/// A yes or no written as text counts as the answer.
pub fn normalize(a: &Answers) -> Answers {
    let mut out = a.clone();
    for k in BOOL_KEYS {
        if let Some(v) = out.get_mut(*k) {
            *v = as_bool(v);
        }
    }
    if let Some(Value::Object(t)) = out.get_mut("taxes") {
        if let Some(v) = t.get_mut("cra_consent") {
            *v = as_bool(v);
        }
        if let Some(Value::Array(ys)) = t.get_mut("years") {
            for y in ys.iter_mut().filter_map(Value::as_object_mut) {
                for k in ["required", "filed"] {
                    if let Some(v) = y.get_mut(k) {
                        *v = as_bool(v);
                    }
                }
            }
        }
    }
    out
}

struct F(Vec<Finding>);
impl F {
    fn push(&mut self, id: &str, level: &'static str, msg: impl Into<String>) {
        self.0.push(Finding {
            id: id.into(),
            level,
            msg: msg.into(),
        });
    }
    fn ok(&mut self, id: &str, m: impl Into<String>) {
        self.push(id, "OK", m)
    }
    fn warn(&mut self, id: &str, m: impl Into<String>) {
        self.push(id, "WARN", m)
    }
    fn fail(&mut self, id: &str, m: impl Into<String>) {
        self.push(id, "FAIL", m)
    }
}

fn spans_of(v: &[Value], from: &str, to: &str) -> Vec<(String, String)> {
    v.iter().map(|x| (js(x.get(from)), js(x.get(to)))).collect()
}

pub fn judge_answers(raw: &Answers, today: &str) -> Vec<Finding> {
    let a = normalize(raw);
    let mut f = F(vec![]);
    let has = |k: &str| get(&a, k).is_some();
    for (k, q) in questions() {
        if !has(k) {
            f.push(k, "ASK", q);
        }
    }
    for k in BOOL_KEYS {
        if has(k) && !a[*k].is_boolean() {
            f.fail(k, format!("{k}: answer yes or no"));
        }
    }

    // Simple answers.
    let s = |k: &str| get(&a, k).and_then(Value::as_str);
    if has("method") && !matches!(s("method"), Some("online" | "paper")) {
        f.fail("method", "method is online or paper");
    }
    if has("service_language") && !matches!(s("service_language"), Some("English" | "French")) {
        f.fail("service_language", "service language is English or French");
    }
    // An unquoted number drops its leading zeros.
    if get(&a, "uci").is_some_and(Value::is_number) {
        f.fail(
            "uci",
            "write uci in quotes (\"...\") so its leading zeros stay",
        );
    }
    let taxes = get(&a, "taxes")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    if taxes.get("sin").is_some_and(Value::is_number) {
        f.fail(
            "taxes",
            "taxes: write the SIN in quotes (\"...\") so its leading zeros stay",
        );
    }
    if let Some(u) = get(&a, "uci")
        && !u.is_number()
        && !all_digits(&strip(&js(Some(u))), &[8, 10])
    {
        f.fail(
            "uci",
            "the UCI is 8 or 10 digits (PR card: next to the photo)",
        );
    }
    if let Some(n) = get(&a, "name")
        && !(truthy(n.get("family")) || truthy(n.get("given")))
    {
        f.fail("name", "the name is empty");
    }
    if has("marital_status") && !MARITAL.contains(&js(get(&a, "marital_status")).as_str()) {
        f.fail(
            "marital_status",
            format!("marital status is one of: {}", MARITAL.join(", ")),
        );
    }
    for k in ["birth_date", "pr_date", "sign_date"] {
        if has(k) && vday(get(&a, k)).is_none() {
            f.fail(k, format!("{k} must be a date YYYY-MM-DD"));
        }
    }
    if let Some(c) = get(&a, "contact") {
        for k in ["address", "postal_code", "phone"] {
            if !truthy(c.get(k)) {
                f.fail("contact", format!("contact.{k} is missing"));
            }
        }
        let postal = Regex::new(r"^[A-Za-z]\d[A-Za-z] ?\d[A-Za-z]\d$").unwrap();
        if let Some(p) = sfield(c, "postal_code").filter(|p| !p.is_empty())
            && !postal.is_match(p)
        {
            f.fail(
                "contact",
                "contact.postal_code is not a Canadian postal code (A1A 1A1)",
            );
        }
        let email = Regex::new(r"^\S+@\S+\.\S+$").unwrap();
        if (s("certificate") == Some("electronic") || s("method") == Some("online"))
            && !email.is_match(sfield(c, "email").unwrap_or(""))
        {
            f.fail(
                "contact",
                "an email is needed (online applications and e-certificates use it)",
            );
        }
    }
    if has("ids") {
        let kinds = id_kinds();
        let ids: Vec<String> = list(&a, "ids").iter().map(|x| js(Some(x))).collect();
        let bad: Vec<&str> = ids
            .iter()
            .map(String::as_str)
            .filter(|x| !kinds.contains(x))
            .collect();
        if ids.len() != 2 {
            f.fail("ids", "copy exactly two IDs");
        } else if !bad.is_empty() {
            f.fail(
                "ids",
                format!(
                    "unknown ID kind: {}; kinds: {}",
                    bad.join(", "),
                    kinds.join(", ")
                ),
            );
        } else if ids.iter().any(|x| x == "pr-card")
            && !ids.iter().any(|x| CANADIAN_GOV_IDS.contains(&x.as_str()))
        {
            f.fail(
                "ids",
                "with a PR card, the other ID must be issued by a Canadian federal, provincial or territorial government",
            );
        } else {
            f.ok("ids", "two IDs fit the rule");
        }
    }
    let b = |k: &str| get(&a, k).and_then(Value::as_bool);
    if b("prohibitions_none") == Some(false) {
        f.fail(
            "prohibitions_none",
            "a prohibition may apply: talk to a lawyer before you apply",
        );
    }
    if b("representative") == Some(true) {
        f.warn(
            "representative",
            "send the Use of a Representative form (IMM 5476); to have them submit for you, apply on paper",
        );
    }
    if b("name_changed_since_pr") == Some(true) {
        f.warn("name_changed_since_pr", "add the name change documents");
    }
    if let Some(acc) = s("accommodation")
        && !acc.is_empty()
        && !acc.eq_ignore_ascii_case("none")
    {
        f.warn(
            "accommodation",
            "pick the accommodation in the form; IRCC may contact you about it",
        );
    }

    let Some(sign) = vday(get(&a, "sign_date")).map(str::to_string) else {
        return f.0;
    };
    let sign = sign.as_str();
    let win_from = minus_months(sign, 12 * WINDOW_YEARS as u32);
    let win_to = plus_days(sign, -1);
    if sign < today {
        f.fail(
            "sign_date",
            "the sign day is in the past: pick a new one and recount",
        );
    }

    // Age, language and test.
    if let Some(birth) = vday(get(&a, "birth_date")) {
        let age = age_on(birth, sign);
        if age < AGE_MIN {
            f.fail(
                "birth_date",
                "under 18 on the sign day: this is the adult process",
            );
        }
        let needs_lang = (AGE_MIN..=AGE_MAX).contains(&age);
        if needs_lang && has("language_proof") {
            let lp = js(get(&a, "language_proof"));
            if !LANGUAGE_PROOF.contains(&lp.as_str()) {
                f.fail(
                    "language_proof",
                    format!("language proof is one of: {}", LANGUAGE_PROOF.join(", ")),
                );
            } else if lp == "none" {
                f.fail(
                    "language_proof",
                    format!(
                        "you are {age} on the sign day: ages 18 to 54 must send proof of English or French (CLB/NCLC 4)"
                    ),
                );
            } else {
                f.ok("language_proof", "language proof chosen");
            }
        }
        if age >= AGE_MIN {
            f.ok(
                "age",
                if needs_lang {
                    "18 to 54 on the sign day: language proof and the test apply"
                } else {
                    "55 or older on the sign day: no language proof and no test"
                },
            );
        }
    }

    // Trips: well formed, in order, not overlapping.
    for t in list(&a, "trips") {
        let (l, r) = (vday(t.get("left")), vday(t.get("returned")));
        let good = matches!((l, r), (Some(l), Some(r)) if r >= l) && truthy(t.get("country"));
        if !good {
            f.fail(
                "trips",
                format!(
                    "a trip needs left <= returned dates and a country: {} to {}",
                    js(t.get("left")),
                    js(t.get("returned"))
                ),
            );
        }
    }
    let mut st = trip_spans(&a);
    st.sort_by(|p, q| p.0.cmp(&q.0));
    for w in st.windows(2) {
        if w[1].0 < w[0].1 {
            f.fail("trips", format!("trips overlap: {} and {}", w[0].0, w[1].0));
        }
    }

    // Physical presence.
    if let (Some(pr), true) = (vday(get(&a, "pr_date")), has("trips"))
        && let Some(p) = presence(pr, sign, &st, &temp_spans(&a), today)
    {
        if !p.met {
            let e = p
                .earliest
                .as_ref()
                .map(|e| format!("; earliest sign day {e}"))
                .unwrap_or_default();
            f.fail(
                "presence",
                format!(
                    "{} days in Canada in the 5 years before {sign}: {} short of {}{e}",
                    p.total, p.short_by, p.needed
                ),
            );
        } else if p.total - p.needed < 30 {
            f.warn(
                "presence",
                format!(
                    "{} days, only {} over {}: IRCC suggests applying with more; a later sign day adds days",
                    p.total,
                    p.total - p.needed,
                    p.needed
                ),
            );
        } else {
            f.ok(
                "presence",
                format!("{} days in Canada ({} needed)", p.total, p.needed),
            );
        }
    }

    // Address and activity history: the whole 5 years, no gaps.
    for k in ["addresses", "activities"] {
        if !has(k) {
            continue;
        }
        let rows = list(&a, k);
        for x in rows {
            let (fr, to) = (js(x.get("from")), js(x.get("to")));
            if !is_day(&fr) || !(to == "present" || is_day(&to)) || up_to(&to, sign) < fr.as_str() {
                f.fail(
                    k,
                    format!("{k}: each row needs from <= to (a date or present): {fr} to {to}"),
                );
            }
            let need: &[&str] = if k == "addresses" {
                &["address", "city", "country"]
            } else {
                &["what", "city", "country"]
            };
            if need.iter().any(|n| !truthy(x.get(*n))) {
                let what = if k == "addresses" { "address" } else { "what" };
                f.fail(
                    k,
                    format!("{k}: the row from {fr} needs {what}, city and country"),
                );
            }
        }
        let g = gaps(&spans_of(rows, "from", "to"), &win_from, sign, sign);
        if g.is_empty() {
            f.ok(k, format!("{k} cover {win_from} to {sign}"));
        } else {
            let list: Vec<String> = g.iter().map(|(x, y)| format!("{x} to {y}")).collect();
            f.fail(
                k,
                format!(
                    "{k} leave gaps in {win_from} to {sign}: {} (a gap sends the application back)",
                    list.join(", ")
                ),
            );
        }
    }

    // Taxes: each of the 5 tax years; filed at least 3 of the years you had to.
    if has("taxes") {
        let years: &[Value] = taxes
            .get("years")
            .and_then(Value::as_array)
            .map_or(&[], |v| v);
        let sy = d(sign).year();
        let want: Vec<i64> = (0..WINDOW_YEARS)
            .map(|i| i64::from(sy - WINDOW_YEARS + i))
            .collect();
        let year_of = |x: &Value| x.get("year").and_then(Value::as_i64);
        let miss: Vec<String> = want
            .iter()
            .filter(|y| !years.iter().any(|x| year_of(x) == Some(**y)))
            .map(i64::to_string)
            .collect();
        if !miss.is_empty() {
            f.fail(
                "taxes",
                format!(
                    "taxes: add the years {} (had to file? filed?)",
                    miss.join(", ")
                ),
            );
        }
        // Only the years in the window count.
        let required: Vec<&Value> = years
            .iter()
            .filter(|x| truthy(x.get("required")) && year_of(x).is_some_and(|y| want.contains(&y)))
            .collect();
        let sin = strip(&taxes.get("sin").map(|v| js(Some(v))).unwrap_or_default());
        if !required.is_empty() && !all_digits(&sin, &[9]) {
            f.fail("taxes", "taxes: the SIN is 9 digits");
        }
        let filed = required.iter().filter(|x| truthy(x.get("filed"))).count();
        let short = required.len() >= 3 && filed < 3;
        if short {
            f.fail(
                "taxes",
                format!("taxes: filed {filed} of the years you had to; you need at least 3 in the 5 years"),
            );
        }
        for x in required.iter().filter(|x| !truthy(x.get("filed"))) {
            f.warn(
                "taxes",
                format!(
                    "taxes: {} had to be filed but was not; file it before you apply",
                    js(x.get("year"))
                ),
            );
        }
        if !taxes.get("cra_consent").is_some_and(Value::is_boolean) {
            f.fail(
                "taxes",
                "taxes: answer yes or no to sharing tax details (cra_consent)",
            );
        }
        if miss.is_empty() && !short {
            f.ok("taxes", "taxes answered for each year");
        }
    }

    // Passports: every one held in the window, with dates that cover it.
    if has("passports") {
        let ps = list(&a, "passports");
        for x in ps {
            if !truthy(x.get("number"))
                || !truthy(x.get("country"))
                || vday(x.get("issued")).is_none()
                || vday(x.get("expires")).is_none()
            {
                f.fail(
                    "passports",
                    "passports: each needs number, country, issued and expires dates",
                );
            }
        }
        let spans: Vec<(String, String)> = ps
            .iter()
            .filter_map(|x| {
                Some((
                    vday(x.get("issued"))?.to_string(),
                    vday(x.get("expires"))?.to_string(),
                ))
            })
            .collect();
        let g = gaps(&spans, &win_from, &win_to, sign);
        if g.is_empty() {
            f.ok("passports", "passports cover the 5 years");
        } else {
            let list: Vec<String> = g.iter().map(|(x, y)| format!("{x} to {y}")).collect();
            f.warn(
                "passports",
                format!(
                    "no passport listed for {}: list every passport you held in the 5 years",
                    list.join(", ")
                ),
            );
        }
    }

    // Police certificates: each country with 183+ days in a row in the last 4 years.
    if has("trips") && has("addresses") {
        let need = police_countries(&a);
        let have = list(&a, "police_certificates");
        let six = minus_months(sign, 6);
        for (country, last) in &need {
            let c = have
                .iter()
                .find(|x| sfield(x, "country").is_some_and(|c| c.eq_ignore_ascii_case(country)));
            match c.map(|c| vday(c.get("issued"))) {
                None => f.fail(
                    "police",
                    format!(
                        "police certificate needed from {country} (183+ days in a row in the last 4 years); ask early, it can take months"
                    ),
                ),
                Some(issued) if issued.is_none_or(|i| i < last.as_str() && i < six.as_str()) => f.fail(
                    "police",
                    format!(
                        "the {country} certificate must be issued after your last stay there ({last}) or within 6 months of signing"
                    ),
                ),
                Some(_) => f.ok("police", format!("police certificate from {country} is in time")),
            }
        }
        if need.is_empty() {
            f.ok("police", "no police certificate needed from the answers");
        }
    }
    f.0
}

// ---- The screen judge ----

#[derive(Debug, Clone, Serialize)]
pub struct ScreenFinding {
    pub label: String,
    pub level: &'static str, // MATCH, MISMATCH, EMPTY, UNKNOWN
    pub msg: String,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Text,
    Digits,
    Phone,
    Code,
    Day,
    Bool,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Digits => "digits",
            Kind::Phone => "phone",
            Kind::Code => "code",
            Kind::Day => "day",
            Kind::Bool => "bool",
        }
    }
}

fn path<'a>(a: &'a Answers, p: &[&str]) -> Option<&'a Value> {
    let mut v = get(a, p[0])?;
    for k in &p[1..] {
        v = v.get(*k)?;
    }
    Some(v)
}

/// Form labels (CIT 0002 wording and the online form's) to answers. First match wins.
fn rule(label: &str) -> Option<(Kind, &'static [&'static str])> {
    let rules: &[(&str, Kind, &[&str])] = &[
        (
            r"unique client identifier|\buci\b|client id",
            Kind::Digits,
            &["uci"],
        ),
        (
            r"family name|last name|surname",
            Kind::Text,
            &["name", "family"],
        ),
        (r"given name|first name", Kind::Text, &["name", "given"]),
        (r"date of birth|birth date", Kind::Day, &["birth_date"]),
        (r"marital status", Kind::Text, &["marital_status"]),
        (
            r"date.*(became a )?permanent resident|became a permanent resident|pr date|date.*landing",
            Kind::Day,
            &["pr_date"],
        ),
        (r"social insurance|\bsin\b", Kind::Digits, &["taxes", "sin"]),
        (r"e-?mail", Kind::Text, &["contact", "email"]),
        ("PHONE", Kind::Phone, &["contact", "phone"]),
        (r"postal code", Kind::Code, &["contact", "postal_code"]),
        (
            r"language.*(service|correspond)",
            Kind::Text,
            &["service_language"],
        ),
        (r"city.*(apply|signed)|applying from", Kind::Text, &["city"]),
        (
            r"certificate (type|format)|type of certificate",
            Kind::Text,
            &["certificate"],
        ),
        (r"member of parliament|\bmp\b", Kind::Bool, &["consent_mp"]),
        (
            r"elections canada|register of electors",
            Kind::Bool,
            &["consent_electors"],
        ),
        (
            r"canada revenue agency|\bcra\b",
            Kind::Bool,
            &["taxes", "cra_consent"],
        ),
        (
            r"date.*sign|application date|signature date",
            Kind::Day,
            &["sign_date"],
        ),
    ];
    let m = |re: &str| Regex::new(&format!("(?i){re}")).unwrap().is_match(label);
    rules.iter().find_map(|(re, k, p)| {
        let hit = if *re == "PHONE" {
            m("telephone|phone") && !m(r"\b(type|ext|extension|country code)\b")
        } else {
            m(re)
        };
        hit.then_some((*k, *p))
    })
}

fn norm(s: &str) -> String {
    Regex::new(r"[\s.,-]+")
        .unwrap()
        .replace_all(&s.to_lowercase(), " ")
        .trim()
        .to_string()
}
fn digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}
/// Same number, or one side leaves out a country code (at most 3 digits).
fn same_phone(x: &str, y: &str) -> bool {
    let (short, long) = if x.len() <= y.len() { (x, y) } else { (y, x) };
    short.len() >= 7 && long.ends_with(short) && long.len() - short.len() <= 3
}
fn yes(s: &str) -> Option<bool> {
    match as_bool(&Value::String(s.into())) {
        Value::Bool(b) => Some(b),
        _ => None,
    }
}
fn as_day(s: &str) -> Option<String> {
    let s = s.trim();
    [
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%d/%m/%Y",
        "%B %d, %Y",
        "%b %d, %Y",
        "%d %B %Y",
    ]
    .iter()
    .find_map(|f| NaiveDate::parse_from_str(s, f).ok())
    .map(iso)
}

/// Compares each typed field with the answers. Never echoes a value: a screen may hold IDs.
pub fn judge_screen(fields: &[(String, String)], raw: &Answers) -> Vec<ScreenFinding> {
    let a = normalize(raw);
    let row = Regex::new(r"(?i)\b(other|previous|former|alias|row \d+)\b").unwrap();
    fields
        .iter()
        .map(|(label, value)| {
            let out = |level, msg: String| ScreenFinding {
                label: label.clone(),
                level,
                msg,
            };
            // Rows of a list are not the main answer: check those by hand.
            let r = if row.is_match(label) {
                None
            } else {
                rule(label)
            };
            let Some((kind, p)) = r else {
                return out(
                    "UNKNOWN",
                    "no rule for this label: check it against the guide by hand".into(),
                );
            };
            let want = path(&a, p).filter(|v| v.as_str() != Some(""));
            let Some(want) = want else {
                return out(
                    "UNKNOWN",
                    "the answers file has no value for it: ask the owner".into(),
                );
            };
            if value.trim().is_empty() {
                return out("EMPTY", "the field is empty".into());
            }
            let w = js(Some(want));
            let same = match kind {
                Kind::Digits => digits(value) == digits(&w),
                Kind::Phone => same_phone(&digits(value), &digits(&w)),
                Kind::Code => {
                    let c = |s: &str| s.split_whitespace().collect::<String>().to_uppercase();
                    c(value) == c(&w)
                }
                Kind::Day => as_day(value).as_deref() == want.as_str(),
                Kind::Bool => want.as_bool().is_some_and(|b| yes(value) == Some(b)),
                Kind::Text => norm(value) == norm(&w),
            };
            if same {
                out("MATCH", "matches the answers".into())
            } else {
                out(
                    "MISMATCH",
                    format!("differs from the answers ({})", kind.name()),
                )
            }
        })
        .collect()
}
