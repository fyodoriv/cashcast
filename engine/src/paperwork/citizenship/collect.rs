//! Collectors for the citizenship answers (ported from sidequest
//! `citizenship-collect.ts`). Each turns one of the owner's own sources into
//! part of the answers: nomads.com trips, a LinkedIn export, the PR card and
//! passport zones, a language result, CRA pages, leases, a separation
//! agreement. All pure: text in, a partial answers map out. In a partial,
//! `Value::Null` removes a derived answer that no longer holds.

use super::answers::{
    Answers, get, is_day, iso, list, plus_days, police_countries, presence, sfield, temp_spans,
    trip_spans, truthy, vday,
};
use super::draft::mark;
use chrono::{DateTime, Duration, Months, NaiveDate};
use regex::Regex;
use serde_json::{Map, Value, json};

pub type Partial = Map<String, Value>;
pub type Row = Vec<(String, String)>;

fn canada(c: Option<&str>) -> bool {
    matches!(
        c.map(|c| c.trim().to_lowercase()).as_deref(),
        Some("canada" | "ca" | "can")
    )
}
pub fn cell<'a>(r: &'a Row, k: &str) -> Option<&'a str> {
    r.iter().find(|(h, _)| h == k).map(|(_, v)| v.as_str())
}

/// RFC 4180 CSV to rows of (header, cell). A notes line above the header is skipped.
pub fn csv(text: &str) -> Vec<Row> {
    let t: Vec<char> = text.trim_start_matches('\u{feff}').chars().collect();
    let (mut rows, mut row, mut c, mut q) = (vec![], vec![], String::new(), false);
    let mut i = 0;
    while i < t.len() {
        let ch = t[i];
        if q {
            if ch == '"' && t.get(i + 1) == Some(&'"') {
                c.push('"');
                i += 1;
            } else if ch == '"' {
                q = false;
            } else {
                c.push(ch);
            }
        } else if ch == '"' {
            q = true;
        } else if ch == ',' {
            row.push(std::mem::take(&mut c));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && t.get(i + 1) == Some(&'\n') {
                i += 1;
            }
            row.push(std::mem::take(&mut c));
            rows.push(std::mem::take(&mut row));
        } else {
            c.push(ch);
        }
        i += 1;
    }
    if !c.is_empty() || !row.is_empty() {
        row.push(c);
        rows.push(row);
    }
    let Some(start) = rows
        .iter()
        .position(|r: &Vec<String>| r.iter().filter(|x| !x.trim().is_empty()).count() > 1)
    else {
        return vec![];
    };
    let head: Vec<String> = rows[start].iter().map(|h| h.trim().to_string()).collect();
    rows[start + 1..]
        .iter()
        .filter(|r| r.iter().any(|x| !x.trim().is_empty()))
        .map(|r| {
            head.iter()
                .enumerate()
                .map(|(i, h)| (h.clone(), r.get(i).map_or("", |x| x.trim()).to_string()))
                .collect()
        })
        .collect()
}

fn month_end(d: NaiveDate) -> NaiveDate {
    d.checked_add_months(Months::new(1)).unwrap_or(d) - Duration::days(1)
}

/// A day from the many ways sources write one. `end` picks the last day of a month or year.
pub fn day(s: &str, end: bool) -> Option<String> {
    let v = s.trim();
    if v.is_empty() {
        return None;
    }
    for f in ["%Y-%m-%d", "%Y/%m/%d", "%B %d, %Y", "%d %B %Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(v, f) {
            return Some(iso(d));
        }
    }
    let first = NaiveDate::parse_from_str(&format!("1 {v}"), "%d %B %Y")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{v}-01"), "%Y-%m-%d"));
    if let Ok(d) = first {
        return Some(iso(if end { month_end(d) } else { d }));
    }
    if v.len() == 4 && v.bytes().all(|c| c.is_ascii_digit()) {
        return Some(format!("{v}-{}", if end { "12-31" } else { "01-01" }));
    }
    None
}
/// A day from a JSON value: text, or epoch seconds (nomads.com `epoch_start`).
pub fn day_value(v: Option<&Value>, end: bool) -> Option<String> {
    match v? {
        Value::String(s) => day(s, end),
        Value::Number(n) => DateTime::from_timestamp(n.as_i64()?, 0).map(|t| iso(t.date_naive())),
        _ => None,
    }
}

// ---- nomads.com ----

#[derive(Debug, Clone, PartialEq)]
pub struct Stay {
    pub from: String,
    pub to: String,
    pub city: Option<String>,
    pub country: String,
}

/// nomads.com trips: the profile JSON (`{trips: [...]}` or a list) or a CSV export.
pub fn parse_nomads(text: &str) -> Result<Vec<Stay>, String> {
    let t = text.trim();
    let items: Vec<Map<String, Value>> = if t.starts_with('{') || t.starts_with('[') {
        let j: Value = serde_json::from_str(t).map_err(|e| format!("nomads: not JSON: {e}"))?;
        let arr = if j.is_array() {
            j
        } else {
            j.get("trips")
                .or_else(|| j.get("data"))
                .cloned()
                .unwrap_or(json!([]))
        };
        arr.as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_object().cloned())
            .collect()
    } else {
        csv(t)
            .into_iter()
            .map(|r| r.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
            .collect()
    };
    let pick = |o: &Map<String, Value>, re: &str| {
        let re = Regex::new(&format!("(?i)^({re})$")).unwrap();
        // The site's own CSV export writes "Arrival date", with a space.
        o.iter()
            .find(|(k, _)| re.is_match(&k.trim().replace(' ', "_")))
            .map(|(_, v)| v.clone())
    };
    let mut out = vec![];
    for o in &items {
        let from = day_value(
            pick(
                o,
                r"date_?start|start(_?date)?|from|arrival(_?date)?|check_?in|epoch_?start",
            )
            .as_ref(),
            false,
        );
        let to = day_value(
            pick(
                o,
                r"date_?end|end(_?date)?|to|departure(_?date)?|check_?out|epoch_?end",
            )
            .as_ref(),
            false,
        );
        let text = |v: Option<Value>| match v {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Null) | None => String::new(),
            Some(x) => x.to_string(),
        };
        let country = text(pick(o, "country(_name)?"));
        let city = Some(text(pick(o, "place|city|place_name|location"))).filter(|c| !c.is_empty());
        if let (Some(from), Some(to)) = (from, to)
            && !country.is_empty()
            && to >= from
        {
            out.push(Stay {
                from,
                to,
                city,
                country,
            });
        }
    }
    out.sort_by(|a, b| a.from.cmp(&b.from));
    Ok(out)
}

fn ndays(a: &str, b: &str) -> i64 {
    let p = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_default();
    (p(b) - p(a)).num_days()
}

/// Absences from Canada: back-to-back stays abroad are one trip. `country` names every
/// country in the trip; `stays` keeps each one for police certificates.
pub fn trips_from_stays(stays: &[Stay]) -> Vec<Value> {
    struct T {
        left: String,
        returned: String,
        stays: Vec<(String, String, String)>,
    }
    let mut trips: Vec<T> = vec![];
    for s in stays.iter().filter(|s| !canada(Some(&s.country))) {
        if let Some(last) = trips.last_mut() {
            let gap = ndays(&last.returned, &s.from);
            // A stay in Canada between splits them; with no record between, a gap of a day
            // or less is travel between countries.
            let in_canada = stays
                .iter()
                .any(|c| canada(Some(&c.country)) && c.from >= last.returned && c.to <= s.from);
            if gap <= 1 && !in_canada {
                if s.to > last.returned {
                    last.returned = s.to.clone();
                }
                last.stays
                    .push((s.country.clone(), s.from.clone(), s.to.clone()));
                continue;
            }
        }
        trips.push(T {
            left: s.from.clone(),
            returned: s.to.clone(),
            stays: vec![(s.country.clone(), s.from.clone(), s.to.clone())],
        });
    }
    trips
        .into_iter()
        .map(|t| {
            let mut names: Vec<&str> = vec![];
            for (c, _, _) in &t.stays {
                if !names.contains(&c.as_str()) {
                    names.push(c);
                }
            }
            json!({
                "left": t.left,
                "returned": t.returned,
                "country": names.join(", "),
                "stays": t.stays.iter().map(|(c, f, to)| json!({"country": c, "from": f, "to": to})).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// Days in Canada before PR, from stays in Canada (status: the owner confirms it).
pub fn temp_from_stays(stays: &[Stay], pr: &str) -> Vec<Value> {
    stays
        .iter()
        .filter(|s| canada(Some(&s.country)) && s.from.as_str() < pr)
        .map(|s| {
            let to = if s.to.as_str() < pr {
                s.to.clone()
            } else {
                plus_days(pr, -1)
            };
            json!({"from": s.from, "to": to, "status": "visitor"})
        })
        .collect()
}

// ---- LinkedIn data export ----

fn place(loc: &str, o: &mut Map<String, Value>) {
    let parts: Vec<&str> = loc
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect();
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 && (canada(Some(parts[0])) || ICAO.iter().any(|(_, n)| *n == parts[0])) {
        o.insert("country".into(), json!(parts[0]));
        return;
    }
    let city = Regex::new(r"(?i)\s+(Area|Metropolitan Area|Region)$")
        .unwrap()
        .replace(parts[0], "");
    o.insert("city".into(), json!(city));
    if parts.len() > 1 {
        o.insert("country".into(), json!(parts[parts.len() - 1]));
    }
}

/// LinkedIn "Get a copy of your data": Positions, Education, Email Addresses, PhoneNumbers.
/// Month dates become the first and last day of the month.
pub fn parse_linkedin(files: &[(String, String)]) -> Partial {
    let get = |re: &str| {
        let re = Regex::new(&format!("(?i){re}")).unwrap();
        files
            .iter()
            .find(|(n, _)| re.is_match(n))
            .map(|(_, t)| csv(t))
            .unwrap_or_default()
    };
    let mut acts: Vec<Value> = vec![];
    for r in get("positions") {
        let Some(from) = day(cell(&r, "Started On").unwrap_or(""), false) else {
            continue;
        };
        let mut o = Map::new();
        o.insert("from".into(), json!(from));
        o.insert(
            "to".into(),
            json!(day(cell(&r, "Finished On").unwrap_or(""), true).unwrap_or("present".into())),
        );
        let title = cell(&r, "Title")
            .filter(|t| !t.is_empty())
            .unwrap_or("work");
        o.insert("what".into(), json!(title));
        if let Some(c) = cell(&r, "Company Name") {
            o.insert("organization".into(), json!(c));
        }
        place(cell(&r, "Location").unwrap_or(""), &mut o);
        acts.push(Value::Object(o));
    }
    for r in get("education") {
        let Some(from) = day(cell(&r, "Start Date").unwrap_or(""), false) else {
            continue;
        };
        let deg = cell(&r, "Degree Name").filter(|d| !d.is_empty());
        let mut o = Map::new();
        o.insert("from".into(), json!(from));
        o.insert(
            "to".into(),
            json!(day(cell(&r, "End Date").unwrap_or(""), true).unwrap_or("present".into())),
        );
        o.insert(
            "what".into(),
            json!(deg.map_or("study".into(), |d| format!("study: {d}"))),
        );
        if let Some(s) = cell(&r, "School Name") {
            o.insert("organization".into(), json!(s));
        }
        acts.push(Value::Object(o));
    }
    acts.sort_by(|a, b| sfield(b, "from").cmp(&sfield(a, "from")));
    let mut out = Partial::new();
    if !acts.is_empty() {
        out.insert("activities".into(), Value::Array(acts));
    }
    let yes = Regex::new("(?i)yes").unwrap();
    let mobile = Regex::new("(?i)mobile").unwrap();
    let emails = get("email");
    let email = emails
        .iter()
        .find(|r| yes.is_match(cell(r, "Primary").unwrap_or("")))
        .or(emails.first())
        .and_then(|r| cell(r, "Email Address"))
        .filter(|e| !e.is_empty());
    let phones = get("phone");
    let phone = phones
        .iter()
        .find(|r| mobile.is_match(cell(r, "Type").unwrap_or("")))
        .or(phones.first())
        .and_then(|r| cell(r, "Number"))
        .filter(|p| !p.is_empty());
    if email.is_some() || phone.is_some() {
        let mut c = Map::new();
        if let Some(e) = email {
            c.insert("email".into(), json!(e));
        }
        if let Some(p) = phone {
            c.insert("phone".into(), json!(p));
        }
        out.insert("contact".into(), Value::Object(c));
    }
    out
}

// ---- Machine readable zone (passport TD3, PR card TD1) ----

fn val(c: u8) -> u32 {
    match c {
        b'<' => 0,
        b'0'..=b'9' => u32::from(c - b'0'),
        _ => u32::from(c) - 55, // A=10
    }
}
/// ICAO 9303 check digit (weights 7, 3, 1).
pub fn check(s: &str) -> u32 {
    s.bytes()
        .enumerate()
        .map(|(i, c)| val(c) * [7, 3, 1][i % 3])
        .sum::<u32>()
        % 10
}
fn ok(field: &str, digit: u8) -> bool {
    (digit as char).to_digit(10) == Some(check(field))
}
/// YYMMDD to a day: a birth year in the future is last century; an expiry is this century.
fn yymmdd(s: &str, birth: bool, today: &str) -> Option<String> {
    let mut y = 2000 + s[..2].parse::<i32>().ok()?;
    if birth && format!("{y}{}", &s[2..]) > today.replace('-', "") {
        y -= 100;
    }
    day(&format!("{y}-{}-{}", &s[2..4], &s[4..6]), false).filter(|d| is_day(d))
}
fn names(s: &str) -> (String, String) {
    let (fam, giv) = s.split_once("<<").unwrap_or((s, ""));
    let re = Regex::new("<+").unwrap();
    let clean = |x: &str| re.replace_all(x, " ").trim().to_string();
    (clean(fam), clean(giv))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mrz {
    pub passport: bool,
    pub country: String,
    pub nationality: String,
    pub number: String,
    pub family: String,
    pub given: String,
    pub birth: Option<String>,
    pub expires: Option<String>,
    /// Every check digit right: a misread letter fails here.
    pub valid: bool,
}

/// Parses a TD3 (passport, 2 lines of 44) or TD1 (card, 3 lines of 30) zone.
pub fn parse_mrz(text: &str, today: &str) -> Option<Mrz> {
    let shape = Regex::new("^[A-Z0-9<]{30,44}$").unwrap();
    let lines: Vec<String> = text
        .to_uppercase()
        .lines()
        .map(|l| {
            l.chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| if c == '«' { '<' } else { c })
                .collect::<String>()
        })
        .filter(|l| shape.is_match(l))
        .collect();
    let nolt = |s: &str| s.replace('<', "");
    if let Some(i) = (0..lines.len())
        .find(|&i| lines[i].len() == 44 && lines.get(i + 1).is_some_and(|l| l.len() == 44))
    {
        let (l1, l2) = (&lines[i], &lines[i + 1]);
        let b = l2.as_bytes();
        let (family, given) = names(&l1[5..]);
        let valid = ok(&l2[0..9], b[9])
            && ok(&l2[13..19], b[19])
            && ok(&l2[21..27], b[27])
            && ok(
                &format!("{}{}{}", &l2[0..10], &l2[13..20], &l2[21..43]),
                b[43],
            );
        return Some(Mrz {
            passport: true,
            country: nolt(&l1[2..5]),
            nationality: nolt(&l2[10..13]),
            number: nolt(&l2[0..9]),
            family,
            given,
            birth: yymmdd(&l2[13..19], true, today),
            expires: yymmdd(&l2[21..27], false, today),
            valid,
        });
    }
    let i = (0..lines.len()).find(|&k| {
        lines[k].len() == 30
            && lines.get(k + 1).is_some_and(|l| l.len() == 30)
            && lines.get(k + 2).is_some_and(|l| l.len() == 30)
    })?;
    let (l1, l2, l3) = (&lines[i], &lines[i + 1], &lines[i + 2]);
    let (b1, b2) = (l1.as_bytes(), l2.as_bytes());
    let valid = ok(&l1[5..14], b1[14])
        && ok(&l2[0..6], b2[6])
        && ok(&l2[8..14], b2[14])
        && ok(
            &format!("{}{}{}{}", &l1[5..30], &l2[0..7], &l2[8..15], &l2[18..29]),
            b2[29],
        );
    let (family, given) = names(l3);
    Some(Mrz {
        passport: false,
        country: nolt(&l1[2..5]),
        nationality: nolt(&l2[15..18]),
        number: nolt(&l1[5..14]),
        family,
        given,
        birth: yymmdd(&l2[0..6], true, today),
        expires: yymmdd(&l2[8..14], false, today),
        valid,
    })
}

/// ICAO country codes to the names the IRCC form lists (common ones; others stay codes).
pub const ICAO: &[(&str, &str)] = &[
    ("CAN", "Canada"),
    ("USA", "United States"),
    ("GBR", "United Kingdom"),
    ("RUS", "Russia"),
    ("UKR", "Ukraine"),
    ("BLR", "Belarus"),
    ("KAZ", "Kazakhstan"),
    ("IND", "India"),
    ("CHN", "China"),
    ("MEX", "Mexico"),
    ("BRA", "Brazil"),
    ("FRA", "France"),
    ("DEU", "Germany"),
    ("D", "Germany"),
    ("ESP", "Spain"),
    ("ITA", "Italy"),
    ("PRT", "Portugal"),
    ("NLD", "Netherlands"),
    ("POL", "Poland"),
    ("TUR", "Turkey"),
    ("GEO", "Georgia"),
    ("ARM", "Armenia"),
    ("SRB", "Serbia"),
    ("THA", "Thailand"),
    ("IDN", "Indonesia"),
    ("VNM", "Vietnam"),
    ("PHL", "Philippines"),
    ("ISR", "Israel"),
    ("ARE", "United Arab Emirates"),
    ("AUS", "Australia"),
    ("JPN", "Japan"),
    ("KOR", "South Korea"),
    ("ARG", "Argentina"),
    ("COL", "Colombia"),
    ("UTO", "Utopia"), // ICAO's own specimen country
];
fn country(code: &str) -> String {
    ICAO.iter()
        .find(|(c, _)| *c == code)
        .map_or(code.to_string(), |(_, n)| n.to_string())
}

/// A day the way the card prints it: "01 JUN / JUIN 21" (month in two languages, a
/// 2-digit year). A 2-digit year after today's is read as the last century.
fn card_day(s: &str, today: &str) -> Option<String> {
    let c = Regex::new(r"^(\d{1,2}) ([A-Za-z]{3})[A-Za-z]*(?: ?/ ?\p{L}+)? (\d{4}|\d{2})$")
        .unwrap()
        .captures(s.trim())?;
    let mut year: i32 = c[3].parse().ok()?;
    if c[3].len() == 2 {
        let now: i32 = today.get(..4)?.parse().ok()?;
        year += now / 100 * 100;
        if year > now {
            year -= 100;
        }
    }
    NaiveDate::parse_from_str(&format!("{} {} {year}", &c[1], &c[2]), "%d %b %Y")
        .ok()
        .map(iso)
}

/// The PR card: the back's TD1 zone (name, birth); the front's "PR since" and UCI, which
/// Claude transcribes from the owner's photo. Also returns whether the zone's digits hold.
pub fn parse_pr_card(text: &str, today: &str) -> (Partial, Option<bool>) {
    let mut out = Partial::new();
    let uci = Regex::new(r"(?i)\b(?:UCI|IUC|client)[^\d\n]{0,20}(\d[\d -]{6,12}\d)\b").unwrap();
    if let Some(c) = uci.captures(text) {
        out.insert("uci".into(), json!(c[1].replace([' ', '-'], "")));
    }
    let since = Regex::new(
        r"(?i)(?:PR since|RP depuis|permanent resident since|date of landing)[^\d]{0,20}(\d{4}[-/ ]\d{2}[-/ ]\d{2}|\d{1,2} [A-Za-z]{3,9}(?: ?/ ?\p{L}{3,9})? (?:\d{4}|\d{2})\b|[A-Za-z]{3,9} \d{1,2},? \d{4})",
    )
    .unwrap();
    if let Some(c) = since.captures(text) {
        let s = c[1].replace('/', "-");
        let s = Regex::new(r"(\d{4}) (\d{2}) (\d{2})")
            .unwrap()
            .replace(&s, "$1-$2-$3");
        if let Some(d) = card_day(&c[1], today).or_else(|| day(&s, false)) {
            out.insert("pr_date".into(), json!(d));
        }
    }
    let mut valid = None;
    if let Some(m) = parse_mrz(text, today) {
        valid = Some(m.valid);
        if m.valid {
            out.insert("name".into(), json!({"family": m.family, "given": m.given}));
            if let Some(b) = m.birth {
                out.insert("birth_date".into(), json!(b));
            }
        }
    }
    (out, valid)
}

/// A passport's data page zone: one row of `passports`, and the citizenship it shows.
pub fn parse_passport(text: &str, today: &str) -> (Partial, Option<bool>) {
    let mut out = Partial::new();
    let Some(m) = parse_mrz(text, today).filter(|m| m.passport) else {
        return (out, None);
    };
    if !m.valid {
        return (out, Some(false));
    }
    let issued = Regex::new(
        r"(?i)(?:date of issue|issued|d[ée]livrance)[^\d]{0,20}(\d{4}-\d{2}-\d{2}|\d{1,2} [A-Za-z]{3,9} \d{4}|\d{1,2}\.\d{1,2}\.\d{4})",
    )
    .unwrap()
    .captures(text)
    .and_then(|c| {
        // Many passports print the day first, with dots: 16.04.2018.
        NaiveDate::parse_from_str(&c[1], "%d.%m.%Y")
            .ok()
            .map(iso)
            .or_else(|| day(&c[1], false))
    });
    let mut p = Map::new();
    p.insert(
        "holder".into(),
        json!({"family": m.family, "given": m.given}),
    );
    p.insert("number".into(), json!(m.number));
    p.insert("country".into(), json!(country(&m.country)));
    if let Some(i) = issued {
        p.insert("issued".into(), json!(i));
    }
    if let Some(e) = m.expires {
        p.insert("expires".into(), json!(e));
    }
    out.insert("passports".into(), json!([p]));
    if !m.nationality.is_empty() && m.nationality != "CAN" {
        out.insert(
            "other_status".into(),
            json!([{"country": country(&m.nationality), "status": "citizen"}]),
        );
    }
    (out, Some(true))
}

fn first_kind(text: &str, kinds: &[(&str, &str)]) -> Option<String> {
    let t = text.to_lowercase();
    kinds
        .iter()
        .find(|(re, _)| Regex::new(re).unwrap().is_match(&t))
        .map(|(_, k)| k.to_string())
}

/// A second ID (driver's licence, health card, provincial photo card): the two IDs to copy.
pub fn parse_id(text: &str) -> Partial {
    let k = first_kind(
        text,
        &[
            (
                r"driver'?s? licen[cs]e|permis de conduire|licence class",
                "drivers-licence",
            ),
            (r"health card|ohip|carte sant", "health-card"),
            (r"photo card|identification card", "provincial-photo-id"),
        ],
    );
    k.map(|k| Partial::from_iter([("ids".to_string(), json!(["pr-card", k]))]))
        .unwrap_or_default()
}

/// A test result or diploma: which proof it is.
pub fn parse_language(text: &str) -> Partial {
    let k = first_kind(
        text,
        &[
            (r"celpip[- ]?ls", "celpip-ls"),
            (r"celpip", "celpip"),
            (r"ielts", "ielts"),
            (r"pte core|pte-core", "pte-core"),
            (r"\btef\b", "tef"),
            (r"\btcf\b", "tcf"),
            (r"\blinc\b", "linc"),
            (r"\bclic\b", "clic"),
            (r"transcript", "transcript"),
            (r"diploma|degree", "diploma"),
        ],
    );
    k.map(|k| Partial::from_iter([("language_proof".to_string(), json!(k))]))
        .unwrap_or_default()
}

/// A year in the line that is not part of a date (2024-04-15).
fn bare_year(line: &str) -> Option<i64> {
    let b = line.as_bytes();
    Regex::new(r"20\d{2}")
        .unwrap()
        .find_iter(line)
        .find_map(|m| {
            let edge = |c: Option<&u8>| c.is_some_and(|c| c.is_ascii_digit() || *c == b'-');
            let before = m.start().checked_sub(1).and_then(|i| b.get(i));
            (!edge(before) && !edge(b.get(m.end()))).then(|| m.as_str().parse().ok())?
        })
}

/// CRA My Account "Tax returns": which years were filed (assessed, reassessed, filed,
/// received or processed). A year marked not filed is not filed.
pub fn parse_cra(text: &str) -> Partial {
    let tax_year = Regex::new(r"(?i)tax year\D{0,3}(20\d{2})").unwrap();
    let filed_re = Regex::new(r"(?i)assessed|filed|received|processed|reassessed").unwrap();
    let not_re = Regex::new(r"(?i)not filed|no return").unwrap();
    let not_any = Regex::new(r"(?i)not filed|no return|not on file").unwrap();
    let mut years = std::collections::BTreeMap::<i64, bool>::new();
    for line in text.lines() {
        let y = tax_year
            .captures(line)
            .and_then(|c| c[1].parse().ok())
            .or_else(|| bare_year(line));
        let Some(y) = y else { continue };
        let filed = filed_re.is_match(line) && !not_re.is_match(line);
        if not_any.is_match(line) || filed {
            let e = years.entry(y).or_insert(false);
            *e = *e || filed;
        }
    }
    let sin = Regex::new(r"(?i)social insurance number[^\d]{0,10}(\d{3}[\s-]?\d{3}[\s-]?\d{3})\b")
        .unwrap()
        .captures(text)
        .map(|c| {
            c[1].chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
        });
    if years.is_empty() && sin.is_none() {
        return Partial::new();
    }
    let mut t = Map::new();
    if let Some(s) = sin {
        t.insert("sin".into(), json!(s));
    }
    t.insert(
        "years".into(),
        years
            .iter()
            .map(|(y, f)| json!({"year": y, "required": f, "filed": f}))
            .collect(),
    );
    Partial::from_iter([("taxes".to_string(), Value::Object(t))])
}

/// Leases, bills and moving mail: one row per home (from, to, address, city, country,
/// postal_code) as CSV.
pub fn parse_leases(text: &str) -> Partial {
    let present = Regex::new("(?i)present").unwrap();
    let mut rows: Vec<Value> = vec![];
    for r in csv(text) {
        let f = |a: &str, b: &str| cell(&r, a).or_else(|| cell(&r, b));
        let from = day(f("from", "From").unwrap_or(""), false);
        let to_s = f("to", "To").unwrap_or("");
        let to = if present.is_match(to_s) {
            Some("present".into())
        } else {
            day(to_s, true)
        };
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };
        let mut o = Map::new();
        o.insert("from".into(), json!(from));
        o.insert("to".into(), json!(to));
        for (k, a, b) in [
            ("address", "address", "Address"),
            ("city", "city", "City"),
            ("country", "country", "Country"),
        ] {
            if let Some(v) = f(a, b) {
                o.insert(k.into(), json!(v));
            }
        }
        if let Some(p) = f("postal_code", "Postal").filter(|p| !p.is_empty()) {
            o.insert("postal_code".into(), json!(p));
        }
        rows.push(Value::Object(o));
    }
    rows.sort_by(|a, b| sfield(b, "from").cmp(&sfield(a, "from")));
    if rows.is_empty() {
        return Partial::new();
    }
    Partial::from_iter([("addresses".to_string(), Value::Array(rows))])
}

/// A separation agreement, a divorce or marriage certificate: the status today.
pub fn parse_marital(text: &str) -> Partial {
    let k = first_kind(
        text,
        &[
            (
                r"certificate of divorce|divorce order|judgment of divorce",
                "divorced",
            ),
            (
                r"separation agreement|separated on|date of separation",
                "separated",
            ),
            (r"common[- ]law", "common-law"),
            (r"marriage certificate|certificate of marriage", "married"),
        ],
    );
    k.map(|k| Partial::from_iter([("marital_status".to_string(), json!(k))]))
        .unwrap_or_default()
}

// ---- Derived answers ----

/// Answers that follow from the others: the contact address from the current home, the
/// city, other names, the sign day, police certificates, stays before PR, tax years.
pub fn derive(a: &Answers, stays: &[Stay], today: &str) -> Partial {
    let mut out = Partial::new();
    let cur = list(a, "addresses")
        .iter()
        .find(|x| sfield(x, "to") == Some("present") && canada(sfield(x, "country")));
    if let Some(cur) = cur {
        // Merge keeps a contact the owner wrote and fills only its missing fields.
        if let Some(addr) = sfield(cur, "address").filter(|s| !s.is_empty()) {
            let mut c = Map::new();
            c.insert("address".into(), json!(addr));
            if let Some(p) = cur.get("postal_code") {
                c.insert("postal_code".into(), p.clone());
            }
            out.insert("contact".into(), Value::Object(c));
        }
        if let Some(city) = sfield(cur, "city").filter(|s| !s.is_empty()) {
            out.insert("city".into(), json!(city));
        }
    }
    // A passport in another name means another name; all in the PR card's name means none.
    let key = |n: Option<&Value>| {
        let f = |k| n.and_then(|n| sfield(n, k)).unwrap_or("").to_string();
        format!("{}|{}", f("family"), f("given"))
            .to_uppercase()
            .chars()
            .filter(|c| c.is_ascii_uppercase() || *c == '|')
            .collect::<String>()
    };
    let holders: Vec<&Value> = list(a, "passports")
        .iter()
        .filter_map(|p| p.get("holder"))
        .filter(|h| truthy(h.get("family")))
        .collect();
    if let Some(name) = get(a, "name")
        && !holders.is_empty()
    {
        let others: Vec<Value> = holders
            .iter()
            .filter(|h| key(Some(h)) != key(Some(name)))
            .map(|h| {
                let mut o = Map::new();
                for k in ["family", "given"] {
                    if let Some(v) = h.get(k) {
                        o.insert(k.into(), v.clone());
                    }
                }
                Value::Object(o)
            })
            .collect();
        // Another name may be a change since PR: then the owner answers.
        out.insert(
            "name_changed_since_pr".into(),
            if others.is_empty() {
                json!(false)
            } else {
                Value::Null
            },
        );
        out.insert("other_names".into(), Value::Array(others));
    }
    // The sign day: the first day with enough days in Canada. Merge keeps one the owner wrote.
    let pr = vday(get(a, "pr_date"));
    if let (Some(pr), Some(_)) = (pr, get(a, "trips"))
        && let Some(p) = presence(pr, today, &trip_spans(a), &temp_spans(a), today)
        && let Some(e) = p.earliest
    {
        out.insert("sign_date".into(), json!(e));
    }
    // The owner's sign day: one they wrote, or a derived one they edited (its mark differs).
    let src = |m: &str| {
        a.get(m)
            .and_then(|s| s.get("sign_date"))
            .and_then(Value::as_str)
    };
    let sd = get(a, "sign_date");
    let derived = src("_sources") == Some("derived")
        && src("_marks") == Some(&mark(sd.unwrap_or(&Value::Null)));
    let mine = truthy(sd) && !derived;
    let sign = if mine {
        sd.cloned()
    } else {
        out.get("sign_date").cloned().or(sd.cloned())
    };
    if let Some(sign) = sign.filter(|s| truthy(Some(s)))
        && get(a, "trips").is_some()
        && get(a, "addresses").is_some()
    {
        let mut b = a.clone();
        b.insert("sign_date".into(), sign);
        // None needed: the list is []; needed: a derived [] goes, and the owner lists theirs.
        let need = !police_countries(&b).is_empty();
        out.insert(
            "police_certificates".into(),
            if need { Value::Null } else { json!([]) },
        );
    }
    if let Some(pr) = pr
        && !stays.is_empty()
        && get(a, "temp_periods").is_none()
    {
        out.insert(
            "temp_periods".into(),
            Value::Array(temp_from_stays(stays, pr)),
        );
    }
    // A tax year you worked in Canada had to be filed.
    if let (Some(Value::Object(t)), Some(_)) = (get(a, "taxes"), get(a, "activities"))
        && let Some(ys) = t.get("years").and_then(Value::as_array)
    {
        let years: Vec<Value> = ys
            .iter()
            .map(|y| {
                let yr = y.get("year").and_then(Value::as_i64).unwrap_or(0);
                let worked = list(a, "activities").iter().any(|x| {
                    let from = sfield(x, "from").unwrap_or("");
                    let to = sfield(x, "to").unwrap_or("");
                    canada(sfield(x, "country"))
                        && !sfield(x, "what").unwrap_or("").starts_with("study")
                        && from <= format!("{yr}-12-31").as_str()
                        && (to == "present" || to >= format!("{yr}-01-01").as_str())
                });
                let mut y = y.clone();
                if let Some(o) = y.as_object_mut()
                    && !truthy(o.get("required"))
                {
                    o.insert("required".into(), json!(worked));
                }
                y
            })
            .collect();
        let mut t = t.clone();
        t.insert("years".into(), Value::Array(years));
        out.insert("taxes".into(), Value::Object(t));
    }
    out
}
