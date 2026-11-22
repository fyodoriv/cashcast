//! Finance tasks in one Notion database the owner made (decision 0009), as
//! `api/notion_tasks.py`.
//!
//! Only task titles with every amount removed, due dates, the risk kind and
//! the done state leave the computer, under the `cloud:notion` consent. The
//! token is `CASHCAST_NOTION_TOKEN`; the database id is `[notion]
//! tasks_database` in `cashcast.toml`. Pages without a cashcast id, or with
//! another person's id, are never touched.

use std::sync::LazyLock;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use regex::Regex;

use crate::core::http::{self, api_error};
use crate::core::paths::{AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::core::{consent, events};

pub const API: &str = "https://api.notion.com/v1";
const VERSION: &str = "2022-06-28";
pub const PROPS: [(&str, &str); 5] = [
    ("Name", "title"),
    ("Due", "date"),
    ("Done", "checkbox"),
    ("Kind", "select"),
    ("cashcast id", "rich_text"),
];

// The `regex` crate keeps Python's leftmost-first choice between
// alternatives and its Unicode `\d`, `\s` and `\b`, so these are the Python
// patterns unchanged.
const NUM: &str = r"(?:\d{1,3}(?:[ ,.\x{a0}]\d{3})+(?:[.,]\d+)?|\d+(?:[.,]\d+)?|[.,]\d+)";
static MONEY: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        // $1,234.56, CA$500, CAD 500, $.50
        Regex::new(&format!(
            r"[-+]?(?:(?:CA|US|C)?\$|\b(?:CAD|USD|EUR)\s?)\s?{NUM}"
        ))
        .unwrap(),
        // 500 $, 1 234,56 $, 20 USD, 500CAD
        Regex::new(&format!(r"[-+]?{NUM}\s?(?:\$|(?:CAD|USD|EUR)\b)")).unwrap(),
        // 12,000 and 12.50
        Regex::new(r"\b\d{1,3}(?:,\d{3})+(?:\.\d+)?\b|\b\d+\.\d{2}\b").unwrap(),
    ]
});
static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap());
static BIG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{3,}\b").unwrap());
static HOLE: LazyLock<Regex> = LazyLock::new(|| Regex::new("\x00(\\d+)\x00").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

/// The title with every amount replaced by "…". Small counts ("3 cards"),
/// ISO dates and years within two of this one ("2026") stay.
pub fn scrub(title: &str, today: NaiveDate) -> String {
    let year = chrono::Datelike::year(&today) as i64;
    let mut dates: Vec<String> = Vec::new();
    let mut text = DATE
        .replace_all(title, |c: &regex::Captures| {
            dates.push(c[0].to_string());
            format!("\x00{}\x00", dates.len() - 1)
        })
        .into_owned();
    for pattern in MONEY.iter() {
        text = pattern.replace_all(&text, "…").into_owned();
    }
    text = BIG
        .replace_all(&text, |c: &regex::Captures| {
            let m = &c[0];
            let keep =
                m.chars().count() == 4 && m.parse::<i64>().is_ok_and(|n| (n - year).abs() <= 2);
            if keep {
                m.to_string()
            } else {
                "…".to_string()
            }
        })
        .into_owned();
    text = HOLE
        .replace_all(&text, |c: &regex::Captures| {
            dates[c[1].parse::<usize>().unwrap_or(0)].clone()
        })
        .into_owned();
    SPACES.replace_all(&text, " ").trim().to_string()
}

fn first(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

fn call(user: &UserHome, method: &str, path: &str, body: Option<&Json>) -> Result<Json, AppError> {
    let token = http::secret(user, "CASHCAST_NOTION_TOKEN")?;
    let headers: Vec<(String, String)> = [
        ("Authorization", format!("Bearer {token}")),
        ("Notion-Version", VERSION.to_string()),
        ("Content-Type", "application/json".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let data = body.map(|b| pyjson::dumps(b, None, false));
    let r = http::request(
        method,
        &format!("{API}/{path}"),
        &headers,
        data.as_deref().map(str::as_bytes),
    )?;
    pyjson::loads(&r.text()).ok_or_else(|| api_error("Notion sent a reply that is not JSON"))
}

/// Missing or mistyped properties, as sentences. Empty when the database fits.
pub fn check_schema(user: &UserHome, database: &str) -> Result<Vec<String>, AppError> {
    let db = call(user, "GET", &format!("databases/{database}"), None)?;
    let empty = Json::Obj(Vec::new());
    let props = db.get("properties").unwrap_or(&empty);
    Ok(PROPS
        .iter()
        .filter_map(|(name, kind)| {
            let have = props
                .get(name)
                .and_then(|p| p.get("type"))
                .filter(|t| !matches!(t, Json::Null));
            if have.is_some_and(|h| h.py_str() == *kind && matches!(h, Json::Str(_))) {
                return None;
            }
            Some(format!(
                "property '{name}' must be {kind}{}",
                match have.filter(|h| h.truthy()) {
                    Some(h) => format!(" (it is {})", h.py_str()),
                    None => " (missing)".to_string(),
                }
            ))
        })
        .collect())
}

fn text(prop: Option<&Json>) -> String {
    let Some(prop) = prop else {
        return String::new();
    };
    let parts = match prop.get("rich_text").filter(|r| r.truthy()) {
        Some(r) => r,
        None => prop.get("title").unwrap_or(&Json::Null),
    };
    match parts {
        Json::List(items) => items
            .iter()
            .map(|x| x.get("plain_text").map_or(String::new(), Json::py_str))
            .collect(),
        _ => String::new(),
    }
}

#[derive(Clone, Debug)]
pub struct Page {
    pub page: String,
    pub done: bool,
    pub edited: String,
    pub title: String,
    pub due: Option<String>,
    pub kind: Option<String>,
}

/// `((d.get(a) or {}).get(b) or {}).get(c)`.
fn dig(d: &Json, a: &str, b: &str, c: &str) -> Option<String> {
    let x = d.get(a).filter(|v| v.truthy())?;
    let y = x.get(b).filter(|v| v.truthy())?;
    y.get(c)
        .filter(|v| !matches!(v, Json::Null))
        .map(Json::py_str)
}

/// This database's pages with a cashcast id, in the order Notion lists them.
pub fn pages(user: &UserHome, database: &str) -> Result<Vec<(String, Page)>, AppError> {
    let mut out: Vec<(String, Page)> = Vec::new();
    let mut cursor: Option<Json> = None;
    loop {
        let mut body = vec![("page_size".to_string(), Json::Int(100))];
        if let Some(c) = cursor.clone().filter(|c| c.truthy()) {
            body.push(("start_cursor".into(), c));
        }
        let r = call(
            user,
            "POST",
            &format!("databases/{database}/query"),
            Some(&Json::Obj(body)),
        )?;
        for p in crate::strategy::py::list(&r, "results") {
            let empty = Json::Obj(Vec::new());
            let props = p.get("properties").unwrap_or(&empty);
            let cid = text(props.get("cashcast id"));
            if cid.is_empty() || p.get("archived").is_some_and(Json::truthy) {
                continue;
            }
            let page = Page {
                page: p
                    .get("id")
                    .map(Json::py_str)
                    .ok_or_else(|| api_error("a Notion page without an id"))?,
                done: props
                    .get("Done")
                    .and_then(|d| d.get("checkbox"))
                    .is_some_and(Json::truthy),
                edited: p
                    .get("last_edited_time")
                    .map_or(String::new(), Json::py_str),
                title: text(props.get("Name")),
                due: dig(props, "Due", "date", "start"),
                kind: dig(props, "Kind", "select", "name"),
            };
            match out.iter_mut().find(|(k, _)| *k == cid) {
                Some(slot) => slot.1 = page,
                None => out.push((cid, page)),
            }
        }
        if !r.get("has_more").is_some_and(Json::truthy) {
            return Ok(out);
        }
        cursor = r.get("next_cursor").cloned();
    }
}

fn s(x: &str) -> Json {
    Json::Str(x.to_string())
}

fn obj(items: Vec<(&str, Json)>) -> Json {
    Json::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn field(t: &Json, key: &str) -> Option<String> {
    t.get(key).filter(|v| v.truthy()).map(Json::py_str)
}

fn props(t: &Json, done: bool, today: NaiveDate) -> Json {
    let title = first(&scrub(&field(t, "title").unwrap_or_default(), today), 200);
    let id = t.get("id").map_or(String::new(), Json::py_str);
    obj(vec![
        (
            "Name",
            obj(vec![(
                "title",
                Json::List(vec![obj(vec![("text", obj(vec![("content", s(&title))]))])]),
            )]),
        ),
        (
            "Due",
            obj(vec![(
                "date",
                field(t, "due").map_or(Json::Null, |d| obj(vec![("start", s(&d))])),
            )]),
        ),
        ("Done", obj(vec![("checkbox", Json::Bool(done))])),
        (
            "Kind",
            obj(vec![(
                "select",
                obj(vec![(
                    "name",
                    s(&field(t, "kind").unwrap_or_else(|| "read".into())),
                )]),
            )]),
        ),
        (
            "cashcast id",
            obj(vec![(
                "rich_text",
                Json::List(vec![obj(vec![("text", obj(vec![("content", s(&id))]))])]),
            )]),
        ),
    ])
}

/// The page already shows what `props` would write (Due compares by day).
fn same(page: &Page, t: &Json, today: NaiveDate) -> bool {
    let due = page
        .due
        .as_deref()
        .map(|d| first(d, 10))
        .filter(|d| !d.is_empty());
    let want_due = t
        .get("due")
        .filter(|d| !matches!(d, Json::Null))
        .map(Json::py_str);
    page.title == first(&scrub(&field(t, "title").unwrap_or_default(), today), 200)
        && due == want_due
        && page.kind == Some(field(t, "kind").unwrap_or_else(|| "read".into()))
}

fn parse_local(since: &str) -> Option<DateTime<chrono::FixedOffset>> {
    if let Ok(d) = DateTime::parse_from_rfc3339(since) {
        return Some(d);
    }
    let naive = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
    ]
    .iter()
    .find_map(|f| NaiveDateTime::parse_from_str(since, f).ok())?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.fixed_offset())
}

/// True when a page was edited after the last sync (or there was none).
/// Notion rounds `last_edited_time` down to the minute, so the minute the
/// sync started counts as after it.
fn after(edited: &str, since: Option<&str>) -> Result<bool, AppError> {
    let (Some(since), false) = (since.filter(|s| !s.is_empty()), edited.is_empty()) else {
        return Ok(true);
    };
    let bad =
        |what: &str| AppError::new("valueerror", format!("Invalid isoformat string: '{what}'"));
    let start = parse_local(since).ok_or_else(|| bad(since))?;
    let start = start
        .with_second(0)
        .and_then(|d| d.with_nanosecond(0))
        .ok_or_else(|| bad(since))?;
    let edit =
        DateTime::parse_from_rfc3339(&edited.replace('Z', "+00:00")).map_err(|_| bad(edited))?;
    Ok(edit >= start)
}

fn ids(v: Option<&Json>) -> Vec<String> {
    match v {
        Some(Json::List(items)) => items.iter().map(Json::py_str).collect(),
        _ => Vec::new(),
    }
}

/// The counts and ids one Notion sync reports.
#[derive(Debug, Default)]
pub struct Outcome {
    pub completed: Vec<String>,
    pub created: i64,
    pub updated: i64,
    pub reopened: i64,
    pub closed: i64,
    pub closed_ids: Vec<String>,
    pub removed: i64,
    pub held: i64,
}

/// Reads ticks, then writes the open tasks. `data` is the reminders payload
/// (owner, done, open, since, max_remove, closed_by_sync).
pub fn sync(user: &UserHome, database: &str, data: &Json) -> Result<Outcome, AppError> {
    let today = Local::now().date_naive();
    consent::require(user, "cloud:notion", today)?;
    let problems = check_schema(user, database)?;
    if !problems.is_empty() {
        return Err(api_error(format!(
            "the Notion tasks database does not fit: {} (see docs/decisions/0009)",
            problems.join("; ")
        )));
    }
    let have = pages(user, database)?;
    let owner = format!("{}:", data.get("owner").map_or(String::new(), Json::py_str));
    let since = data
        .get("since")
        .filter(|v| !matches!(v, Json::Null))
        .map(Json::py_str);
    let ours = ids(data.get("closed_by_sync"));
    let tasks = crate::strategy::py::list(data, "tasks");
    let task_id = |t: &Json| t.get("id").map_or(String::new(), Json::py_str);
    let mut open_ids = ids(data.get("open"));
    open_ids.extend(tasks.iter().map(task_id));
    let mut out = Outcome::default();
    for (cid, p) in &have {
        if p.done
            && open_ids.contains(cid)
            && !ours.contains(cid)
            && after(&p.edited, since.as_deref())?
        {
            out.completed.push(cid.clone());
        }
    }
    let mut done = ids(data.get("done"));
    done.extend(out.completed.iter().cloned());
    let find = |cid: &str| have.iter().find(|(k, _)| k == cid).map(|(_, p)| p);
    let mut wanted: Vec<String> = Vec::new();
    for t in tasks {
        let id = task_id(t);
        wanted.push(id.clone());
        match find(&id) {
            None => {
                let body = obj(vec![
                    ("parent", obj(vec![("database_id", s(database))])),
                    ("properties", props(t, false, today)),
                ]);
                call(user, "POST", "pages", Some(&body))?;
                out.created += 1;
            }
            Some(page) if !page.done && same(page, t, today) => {} // unchanged: no write
            Some(page) if !page.done || !out.completed.contains(&id) => {
                let body = obj(vec![("properties", props(t, false, today))]);
                call(user, "PATCH", &format!("pages/{}", page.page), Some(&body))?;
                out.reopened += i64::from(page.done);
                out.updated += i64::from(!page.done);
            }
            Some(_) => {}
        }
    }
    let gone: Vec<&(String, Page)> = have
        .iter()
        .filter(|(cid, p)| {
            !wanted.contains(cid) && !open_ids.contains(cid) && cid.starts_with(&owner) && !p.done
        })
        .collect();
    let deletions = gone.iter().filter(|(cid, _)| !done.contains(cid)).count() as i64;
    let max_remove = match data.get("max_remove") {
        Some(Json::Int(n)) => *n,
        Some(Json::Float(f)) => *f as i64,
        _ => 10,
    };
    for (cid, page) in gone {
        if done.contains(cid) {
            let body = obj(vec![(
                "properties",
                obj(vec![("Done", obj(vec![("checkbox", Json::Bool(true))]))]),
            )]);
            call(user, "PATCH", &format!("pages/{}", page.page), Some(&body))?;
            out.closed += 1;
            out.closed_ids.push(cid.clone());
        } else if deletions > max_remove {
            out.held += 1; // too many at once: the task list may be incomplete
        } else {
            call(
                user,
                "PATCH",
                &format!("pages/{}", page.page),
                Some(&obj(vec![("archived", Json::Bool(true))])),
            )?;
            out.removed += 1;
        }
    }
    events::log(
        user,
        "notion_sync",
        &[
            ("result", s("ok")),
            ("created", Json::Int(out.created)),
            ("updated", Json::Int(out.updated)),
            ("closed", Json::Int(out.closed)),
            ("removed", Json::Int(out.removed)),
            ("held", Json::Int(out.held)),
            ("completed", Json::Int(out.completed.len() as i64)),
        ],
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_removes_every_amount_but_keeps_years() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        // The cases of tests/unit/test_sync.py and the docstring.
        let cases = [
            ("Pay $1,234.56 to the card", "Pay … to the card"),
            ("Withdraw the -$2,500.00 excess", "Withdraw the … excess"),
            ("Move 12,000 to the TFSA", "Move … to the TFSA"),
            ("Fee of 12.50 a month", "Fee of … a month"),
            ("Copy your 2026 RRSP limit", "Copy your 2026 RRSP limit"),
            ("Move 2500 to TFSA", "Move … to TFSA"),
            ("Pay 500 $ now", "Pay … now"),
            ("Pay 1 234,56 $ now", "Pay … now"),
            ("Send CAD 500 and 20 USD", "Send … and …"),
            ("Tip $.50", "Tip …"),
            ("Keep 3 cards for 12 months", "Keep 3 cards for 12 months"),
            ("Move 2000 to TFSA", "Move … to TFSA"),
            ("Pay 1999", "Pay …"),
            ("Send 500CAD", "Send …"),
            (
                "Deadline 2027-03-01 for 2026",
                "Deadline 2027-03-01 for 2026",
            ),
            ("Send CA$500 now", "Send … now"),
            ("Pay  the   card", "Pay the card"),
        ];
        for (title, want) in cases {
            assert_eq!(scrub(title, today), want, "{title}");
        }
    }

    #[test]
    fn a_tick_in_the_minute_the_sync_started_counts() {
        let since = "2026-10-25T09:00:40";
        let start = parse_local(since).unwrap().with_second(0).unwrap();
        let utc = start
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:00.000Z")
            .to_string();
        assert!(after(&utc, Some(since)).unwrap());
        assert!(!after("2026-02-01T10:00:00.000Z", Some(since)).unwrap());
        assert!(
            after("", Some(since)).unwrap() && after("2026-02-01T10:00:00.000Z", None).unwrap()
        );
    }
}
