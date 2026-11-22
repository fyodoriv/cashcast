//! The local web app's server, as `web.py`: a read-only JSON API plus the
//! built TypeScript UI (`web/dist`).
//!
//! `bin/cashcast web` serves on 127.0.0.1 only. Every API call needs the token
//! made at start-up (a new one each run), and the Host header must be the
//! loopback address, which blocks DNS-rebinding pages from reading it. The only
//! write endpoint runs a CLI action (`POST /api/actions/<name>`, with a confirm
//! for the risky ones); everything else is read-only. The person who opens the
//! page is the owner at their own screen, so the agent-consent gate does not
//! apply here.
//!
//! Lifecycle: the token lives in `serve`'s stack frame (process scope) and in
//! `private/out/web.json` (mode 600) while the server runs; `serve` removes the
//! file when it stops. Action runs get it as `CASHCAST_OWNER_UI`, which marks
//! that child process as the owner's (`tasks::start::owner_token_ok`).

use crate::core::consent;
use crate::core::paths::{self, AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::strategy::py;
use crate::strategy::{self, DISCLAIMER, Data, plan};
use chrono::NaiveDate;
use std::path::{Path, PathBuf};

/// The page may load nothing from the internet: not a font, an image in a
/// Notion document, or a script. Links still open when the owner clicks them.
pub const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob:; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

const FILE_TYPES: [&str; 7] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".pdf"];

pub type Reply = (u16, String, Vec<u8>);

fn text(status: u16, body: &str) -> Reply {
    (status, "text/plain".into(), body.as_bytes().to_vec())
}

fn json_reply(status: u16, v: &Json) -> Reply {
    (
        status,
        "application/json".into(),
        pyjson::dumps(v, None, false).into_bytes(),
    )
}

fn obj(items: Vec<(&str, Json)>) -> Json {
    Json::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(x: impl Into<String>) -> Json {
    Json::Str(x.into())
}

/// `_num`: rounded to cents, or null.
fn num(x: Option<f64>) -> Json {
    x.map_or(Json::Null, |v| Json::Float(py::round(v, 2)))
}

fn as_f(v: &Json) -> Option<f64> {
    match v {
        Json::Int(i) => Some(*i as f64),
        Json::Float(f) => Some(*f),
        _ => None,
    }
}

fn opt_str(d: Option<NaiveDate>) -> Json {
    d.map_or(Json::Null, |d| s(d.to_string()))
}

/// A web route that cannot be answered the Python way yet.
#[derive(Debug)]
pub struct Unported(pub String);

impl From<strategy::situation::Unported> for Unported {
    fn from(e: strategy::situation::Unported) -> Self {
        Unported(e.0)
    }
}

/// `plan_payload`: everything the UI shows for one person, as plain JSON.
pub fn plan_payload(user: &UserHome, today: NaiveDate) -> Result<Json, Unported> {
    let (ctx, log) = plan::run(user, today)?;
    plan::finish(user, &ctx, &log)?;
    let sit = &ctx.sit;
    let forecast: Vec<Json> = match ctx.result("forecast").map(|x| &x.data) {
        Some(Data::Forecast(fc)) => fc
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let f = strategy::forecast::f;
                let end = f(r.month_end);
                let at = |d: f64| ((end + d) * 100.0).round() / 100.0;
                let range = fc.ranges.get(i).copied();
                obj(vec![
                    ("low", num(range.map(|(below, _)| at(below)))),
                    ("high", num(range.map(|(_, above)| at(above)))),
                    ("month", s(r.month.format("%Y-%m").to_string())),
                    ("paycheques", Json::Int(r.paycheques)),
                    ("income", num(Some(f(r.income)))),
                    ("expenses", num(Some(f(r.expenses)))),
                    ("net", num(Some(f(r.net_cash_flow)))),
                    ("start", num(Some(f(r.month_start)))),
                    ("toInvest", num(Some(f(r.to_invest)))),
                    ("end", num(Some(f(r.month_end)))),
                ])
            })
            .collect(),
        _ => Vec::new(),
    };
    let accuracy = match ctx.result("forecast").map(|x| &x.data) {
        Some(Data::Forecast(strategy::forecast::ForecastData {
            accuracy: Some((me, net, q)),
            ..
        })) => obj(vec![
            ("monthEnd", Json::Float((1.0 - me).max(0.0))),
            ("netMonthly", Json::Float((1.0 - net).max(0.0))),
            ("netQuarterly", Json::Float((1.0 - q).max(0.0))),
        ]),
        _ => Json::Null,
    };
    let cashback = match ctx.result("cashback").map(|x| &x.data) {
        Some(Data::Cashback(p)) => {
            let name = |id: &str| {
                p.names
                    .iter()
                    .rev()
                    .find(|(i, _)| i == id)
                    .map_or(id.to_string(), |(_, n)| n.clone())
            };
            Some(obj(vec![
                ("gross", num(Some(p.gross))),
                ("net", num(Some(p.net()))),
                ("cardFees", num(Some(p.card_fees))),
                ("serviceFees", num(Some(p.service_fees))),
                ("membershipFees", num(Some(p.membership_fees))),
                (
                    "routes",
                    Json::List(
                        p.allocations
                            .iter()
                            .map(|a| {
                                obj(vec![
                                    ("category", s(&a.category)),
                                    ("card", s(name(&a.card))),
                                    ("amount", num(Some(a.amount))),
                                    ("rate", Json::Float(a.rate)),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "bankPaid",
                    Json::Obj(
                        p.bank_paid
                            .iter()
                            .map(|(k, v)| (k.clone(), num(Some(*v))))
                            .collect(),
                    ),
                ),
            ]))
        }
        _ => None,
    };
    let net_total = match ctx.result("net_total").map(|x| &x.data) {
        Some(Data::NetTotal(nd)) if nd.get("headline").is_some_and(Json::truthy) => {
            Some(net_total_json(nd))
        }
        _ => None,
    };
    let low = forecast.iter().fold(None::<&Json>, |m, r| {
        let end = |x: &Json| x.get("end").and_then(as_f).unwrap_or(f64::INFINITY);
        match m {
            Some(b) if end(b) <= end(r) => Some(b),
            _ => Some(r),
        }
    });
    let (next_review, pivot) = match ctx.result("review").map(|x| &x.data) {
        Some(Data::Review(r)) => (s(r.next_review.to_string()), Json::Bool(r.pivot)),
        _ => (Json::Null, Json::Bool(false)),
    };
    let (emergency, above) = match ctx.result("reserve").map(|x| &x.data) {
        Some(Data::Reserve(r)) => (num(Some(r.emergency)), num(Some(r.threshold))),
        _ => (Json::Null, Json::Null),
    };
    let marginal = match ctx.result("tax").map(|x| &x.data) {
        Some(Data::Tax(t)) => Json::Float(t.marginal),
        _ => Json::Null,
    };
    let headline = obj(vec![
        (
            "netCashBack",
            cashback
                .as_ref()
                .and_then(|c| c.get("net").cloned())
                .unwrap_or(Json::Null),
        ),
        (
            "canInvest",
            if forecast.is_empty() {
                Json::Null
            } else {
                num(Some(py::sum(
                    forecast
                        .iter()
                        .filter_map(|r| r.get("toInvest").and_then(as_f)),
                )))
            },
        ),
        (
            "lowestMonthEnd",
            low.and_then(|r| r.get("end").cloned())
                .unwrap_or(Json::Null),
        ),
        (
            "lowestMonth",
            low.and_then(|r| r.get("month").cloned())
                .unwrap_or(Json::Null),
        ),
        ("nextReview", next_review),
        ("pivot", pivot),
        ("emergencyFund", emergency),
        ("investAbove", above),
        ("marginalRate", marginal),
    ]);
    let sections = ctx
        .results
        .iter()
        .map(|x| {
            obj(vec![
                ("id", s(&x.id)),
                ("title", s(&x.title)),
                ("markdown", s(x.markdown())),
            ])
        })
        .collect();
    let mut tasks = plan::tasks(&ctx)?;
    tasks.sort_by(|a, b| {
        (a.due.unwrap_or(NaiveDate::MAX), &a.id).cmp(&(b.due.unwrap_or(NaiveDate::MAX), &b.id))
    });
    let tasks = tasks
        .iter()
        .map(|t| {
            obj(vec![
                ("id", s(&t.id)),
                ("title", s(&t.title)),
                ("notes", s(&t.notes)),
                ("risk", s(&t.risk)),
                ("service", s(&t.service)),
                ("action", s(&t.action)),
                ("due", opt_str(t.due)),
            ])
        })
        .collect();
    let display = sit
        .profile
        .get("display_name")
        .map(Json::py_str)
        .unwrap_or_else(|| user.name.clone());
    Ok(obj(vec![
        (
            "user",
            obj(vec![
                ("name", s(&user.name)),
                ("display", s(display)),
                ("province", s(sit.province())),
            ]),
        ),
        ("today", s(sit.today.to_string())),
        ("disclaimer", s(DISCLAIMER)),
        ("headline", headline),
        ("sections", Json::List(sections)),
        ("forecast", Json::List(forecast)),
        ("forecastAccuracy", accuracy),
        ("tasks", Json::List(tasks)),
        ("netTotal", net_total.unwrap_or(Json::Null)),
        ("cashback", cashback.unwrap_or(Json::Null)),
        ("holdings", sit.holdings.clone().unwrap_or(Json::Null)),
        ("connectors", connectors_json()?),
        ("knowledge", knowledge_json(sit.today)?),
        ("consent", consent_json(user, sit.today)?),
    ]))
}

fn net_total_json(nd: &Json) -> Json {
    let list = |k: &str| py::list(nd, k);
    let opt = |x: &Json| {
        let cash = x.get("cash_out").and_then(as_f).map(|v| py::round(v, 2));
        obj(vec![
            ("id", x.get("id").cloned().unwrap_or(Json::Null)),
            ("title", x.get("title").cloned().unwrap_or(Json::Null)),
            ("section", s(py::gets(x, "section", ""))),
            ("netEffect", num(x.get("net_effect").and_then(as_f))),
            // `_num(...) or None`: no cash out reads as null, 0.0 too.
            (
                "cashOut",
                cash.filter(|v| *v != 0.0).map_or(Json::Null, Json::Float),
            ),
            ("why", s(py::gets(x, "why", ""))),
            (
                "refused",
                x.get("refused").cloned().unwrap_or(Json::List(Vec::new())),
            ),
        ])
    };
    obj(vec![
        ("total", num(nd.get("total").and_then(as_f))),
        (
            "byKind",
            Json::Obj(match nd.get("by_kind") {
                Some(Json::Obj(items)) => items
                    .iter()
                    .map(|(k, v)| (k.clone(), num(as_f(v))))
                    .collect(),
                _ => Vec::new(),
            }),
        ),
        (
            "changes",
            Json::List(
                list("changes")
                    .iter()
                    .map(|c| {
                        obj(vec![
                            ("days", c.get("days").cloned().unwrap_or(Json::Null)),
                            ("since", c.get("since").cloned().unwrap_or(Json::Null)),
                            ("change", num(c.get("change").and_then(as_f))),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "history",
            Json::List(
                list("history")
                    .iter()
                    .map(|p| {
                        obj(vec![
                            ("date", p.get("date").cloned().unwrap_or(Json::Null)),
                            ("total", num(p.get("total").and_then(as_f))),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "ranked",
            Json::List(list("ranked").iter().map(opt).collect()),
        ),
        (
            "refused",
            Json::List(list("refused").iter().map(opt).collect()),
        ),
        ("stale", nd.get("stale").cloned().unwrap_or(Json::Null)),
        ("missing", nd.get("missing").cloned().unwrap_or(Json::Null)),
    ])
}

fn connectors_json() -> Result<Json, Unported> {
    let specs = crate::tasks::connectors::load_all()?;
    Ok(Json::List(
        specs
            .iter()
            .map(|(sid, sp)| {
                let imp = py::obj(sp, "import");
                let api = py::obj(sp, "api");
                obj(vec![
                    ("id", s(sid)),
                    ("name", sp.get("name").cloned().unwrap_or(Json::Null)),
                    ("kinds", sp.get("kinds").cloned().unwrap_or(Json::Null)),
                    ("formats", imp.get("formats").cloned().unwrap_or(Json::Null)),
                    ("noExport", s(py::gets(imp, "no_export_reason", ""))),
                    (
                        "api",
                        if api.get("available").is_some_and(Json::truthy) {
                            api.get("auth").cloned().unwrap_or(Json::Null)
                        } else {
                            Json::Null
                        },
                    ),
                    (
                        "actions",
                        Json::List(
                            py::list(sp, "actions")
                                .iter()
                                .map(|a| {
                                    let g = |k: &str| a.get(k).cloned().unwrap_or(Json::Null);
                                    obj(vec![
                                        ("id", g("id")),
                                        ("title", g("title")),
                                        ("risk", g("risk")),
                                        ("via", g("via")),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    ))
}

fn knowledge_json(today: NaiveDate) -> Result<Json, Unported> {
    use crate::knowledge;
    let err = |e: knowledge::Unparsed| Unported(format!("knowledge: {}", e.0.display()));
    let k = knowledge::check(today, &knowledge::data_dir()).map_err(err)?;
    let strs = |v: &[String]| Json::List(v.iter().map(s).collect());
    let mut topics = Vec::new();
    for n in knowledge::names() {
        let t = knowledge::load(&n).map_err(err)?;
        let meta = t.get("meta").and_then(toml::Value::as_table);
        let field = |key: &str| meta.and_then(|m| m.get(key)).map(knowledge::py_str);
        topics.push(obj(vec![
            ("name", s(&n)),
            ("title", s(field("title").unwrap_or_else(|| n.clone()))),
            ("asOf", s(field("as_of").unwrap_or_default())),
            ("reviewBy", s(field("review_by").unwrap_or_default())),
        ]));
    }
    let mut problems = k.problems.clone();
    problems.extend(k.missing_source.iter().cloned());
    Ok(obj(vec![
        ("files", Json::Int(k.files as i64)),
        ("facts", Json::Int(k.facts as i64)),
        ("estimates", Json::Int(k.estimates.len() as i64)),
        ("heuristics", Json::Int(k.heuristics.len() as i64)),
        ("stale", strs(&k.stale)),
        ("problems", strs(&problems)),
        ("topics", Json::List(topics)),
    ]))
}

fn consent_json(user: &UserHome, today: NaiveDate) -> Result<Json, Unported> {
    let active = consent::active(user, today).map_err(|e| Unported(e.message))?;
    Ok(Json::List(
        consent::SCOPES
            .iter()
            .map(|sc| {
                obj(vec![
                    ("scope", s(sc.name)),
                    ("shares", s(sc.shares)),
                    ("recipient", s(sc.recipient)),
                    (
                        "grantedUntil",
                        active.get(sc.name).map_or(Json::Null, |d| s(d.to_string())),
                    ),
                ])
            })
            .collect(),
    ))
}

/// `users_payload`: each person, and whether their profile is ready.
pub fn users_payload() -> Json {
    let mut out = Vec::new();
    for name in paths::list_users() {
        let ready = paths::resolve(Some(&name))
            .ok()
            .and_then(|u| crate::core::users::load_profile(&u).ok());
        out.push(match ready {
            Some(prof) => {
                let display = prof
                    .get("display_name")
                    .map(crate::knowledge::py_str)
                    .unwrap_or_else(|| name.clone());
                let province = prof
                    .get("province")
                    .map(crate::knowledge::py_str)
                    .unwrap_or_default();
                obj(vec![
                    ("name", s(&name)),
                    ("display", s(display)),
                    ("province", s(province)),
                    ("ready", Json::Bool(true)),
                ])
            }
            None => obj(vec![
                ("name", s(&name)),
                ("display", s(&name)),
                ("province", Json::Null),
                ("ready", Json::Bool(false)),
            ]),
        });
    }
    Json::List(out)
}

// --- Documents (`docs.py` list, read, search, files) -------------------------

/// `_safe`: a path inside the documents folder, never a hidden part.
fn safe(user: &UserHome, rel: &str) -> Option<PathBuf> {
    let root = std::fs::canonicalize(user.docs()).ok()?;
    let target = std::fs::canonicalize(root.join(rel)).ok()?;
    let inside = target.strip_prefix(&root).ok()?;
    if inside.as_os_str().is_empty()
        || inside
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return None;
    }
    Some(target)
}

fn title(path: &Path, text: &str) -> String {
    for line in text.lines().take(5) {
        if let Some(t) = line.strip_prefix("# ") {
            return t.trim().to_string();
        }
    }
    path.file_stem()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(_) if p.extension().is_some_and(|x| x == "md") => out.push(p),
            _ => {}
        }
    }
}

/// `_docs`: (entry, text) for every document, sorted by path.
fn all_docs(user: &UserHome) -> Vec<(Json, String)> {
    let root = user.docs();
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for p in files {
        let Ok(rel) = p.strip_prefix(&root) else {
            continue;
        };
        let hidden = rel
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
        if hidden || p.is_symlink() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let body = String::from_utf8_lossy(&bytes).to_string();
        let rel_s = rel.to_string_lossy().to_string();
        let folder = rel
            .parent()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();
        out.push((
            obj(vec![
                ("path", s(&rel_s)),
                ("title", s(title(&p, &body))),
                ("folder", s(folder)),
            ]),
            body,
        ));
    }
    out
}

/// `body[i - 60 : i + 100]` around the first match of `q` in `body.lower()`,
/// with Python's character offsets and slice clamping.
fn snippet(body: &str, q: &str) -> Option<String> {
    let chars: Vec<char> = body.chars().collect();
    let lower = body.to_lowercase();
    let i = lower[..lower.find(q)?].chars().count();
    // Lower-casing can add characters, so `i` may run past `chars`.
    let a = i.saturating_sub(60).min(chars.len());
    let b = (i + 100).clamp(a, chars.len());
    Some(
        chars[a..b]
            .iter()
            .collect::<String>()
            .replace('\n', " ")
            .trim()
            .to_string(),
    )
}

fn docs_search(user: &UserHome, query: &str) -> Json {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Json::List(Vec::new());
    }
    let mut hits = Vec::new();
    for (d, body) in all_docs(user) {
        let i = body.to_lowercase().find(&q);
        let t = d.get("title").map(Json::py_str).unwrap_or_default();
        if i.is_none() && !t.to_lowercase().contains(&q) {
            continue;
        }
        let snippet = snippet(&body, &q).unwrap_or_default();
        let Json::Obj(mut items) = d else {
            continue;
        };
        items.push(("snippet".into(), s(snippet)));
        hits.push(Json::Obj(items));
        if hits.len() >= 40 {
            break;
        }
    }
    Json::List(hits)
}

fn suffix(p: &Path) -> String {
    p.extension()
        .map(|x| format!(".{}", x.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

fn read_doc(user: &UserHome, rel: &str) -> Option<Json> {
    let p = safe(user, rel)?;
    if suffix(&p) != ".md" || !p.is_file() {
        return None;
    }
    let body = String::from_utf8_lossy(&std::fs::read(&p).ok()?).to_string();
    let root = std::fs::canonicalize(user.docs()).ok()?;
    let rel = p.strip_prefix(&root).ok()?.to_string_lossy().to_string();
    Some(obj(vec![
        ("path", s(rel)),
        ("title", s(title(&p, &body))),
        ("markdown", s(body)),
    ]))
}

/// `mimetypes.guess_type` for the types the server hands out.
fn mime(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/vnd.microsoft.icon",
        "pdf" => "application/pdf",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn read_file(user: &UserHome, rel: &str) -> Option<Reply> {
    let p = safe(user, rel)?;
    if !FILE_TYPES.contains(&suffix(&p).as_str()) || !p.is_file() {
        return None;
    }
    let name = p.file_name()?.to_string_lossy().to_string();
    Some((200, mime(&name).into(), std::fs::read(&p).ok()?))
}

// --- Requests ---------------------------------------------------------------

/// `urllib.parse.unquote` (and `+` as a space for query values).
fn unquote(x: &str, plus: bool) -> String {
    let b = x.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                        continue;
                    }
                    _ => out.push(b'%'),
                }
            }
            b'+' if plus => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// `parse_qs(...).get(key, [default])[0]`: the first non-empty value.
fn query(q: &str, key: &str) -> Option<String> {
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, v)| unquote(k, true) == key && !v.is_empty())
        .map(|(_, v)| unquote(v, true))
}

/// Constant-time compare, as `hmac.compare_digest`.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn error_json(status: u16, msg: &str) -> Reply {
    json_reply(status, &obj(vec![("error", s(msg))]))
}

/// The JSON of one GET API route, or a full reply for errors and files.
fn api_get(path: &str, q: &str, token: &str, today: NaiveDate) -> Result<Json, Reply> {
    let who =
        || paths::resolve(query(q, "user").as_deref()).map_err(|e| error_json(400, &e.message));
    let unported = |e: Unported| error_json(500, &format!("not ported yet: {}", e.0));
    match path {
        "/api/users" => Ok(users_payload()),
        "/api/plan" => {
            let u = who()?;
            crate::core::users::load_profile(&u).map_err(|e| error_json(400, &e.message))?;
            plan_payload(&u, today).map_err(unported)
        }
        "/api/actions" => actions_catalog().map_err(unported),
        "/api/docs" => {
            let u = who()?;
            Ok(match query(q, "q") {
                Some(text) => docs_search(&u, &text),
                None => Json::List(all_docs(&u).into_iter().map(|(d, _)| d).collect()),
            })
        }
        "/api/doc" => {
            let u = who()?;
            read_doc(&u, &query(q, "path").unwrap_or_default())
                .ok_or_else(|| text(404, "not found"))
        }
        "/api/doc-file" => {
            let u = who()?;
            Err(read_file(&u, &query(q, "path").unwrap_or_default())
                .unwrap_or_else(|| text(404, "not found")))
        }
        "/api/groceries" => {
            let u = who()?;
            Ok(groceries_payload(&u, query(q, "items").as_deref(), today))
        }
        "/api/moneywiz-categories" => {
            let u = who()?;
            Ok(engine_json(&u, &["moneywiz", "categories", "json"], None))
        }
        "/api/moneywiz-sync" => {
            let u = who()?;
            let day = today.to_string();
            Ok(engine_json(
                &u,
                &[
                    "moneywiz",
                    "sync-view",
                    "--connectors",
                    &paths::root().join("connectors").to_string_lossy(),
                    "--today",
                    &day,
                ],
                Some(token),
            ))
        }
        _ => Err(text(404, "not found")),
    }
}

/// The Groceries view: known items, fee rules, orders and spend per month,
/// and each store's basket when `items` (comma-separated) is given.
fn groceries_payload(user: &UserHome, items: Option<&str>, today: NaiveDate) -> Json {
    use crate::groceries as gr;
    let path = gr::file(&user.home);
    let prefs = crate::core::users::load_profile(user)
        .map(|p| gr::prefs(&p))
        .unwrap_or_else(|_| gr::prefs(&toml::Table::new()));
    let list: Vec<String> = items
        .unwrap_or("")
        .split(',')
        .map(|x| x.trim().to_lowercase())
        .filter(|x| !x.is_empty())
        .collect();
    let build = || -> Result<serde_json::Value, String> {
        let orders = gr::orders(&path)?;
        let compare = if list.is_empty() {
            Vec::new()
        } else {
            gr::compare(&path, &list, prefs.organic_premium, prefs.fresh_days, today)?
        };
        Ok(serde_json::json!({
            "items": gr::known(&path)?,
            "fees": gr::fees(&path)?,
            "months": gr::months(&orders),
            "orders": orders.iter().take(20).collect::<Vec<_>>(),
            "compare": compare,
            "asked": list,
            "freshDays": prefs.fresh_days,
            "rules": prefs.lines,
        }))
    };
    match build() {
        Ok(v) => pyjson::loads(&v.to_string())
            .unwrap_or_else(|| obj(vec![("error", s("bad grocery JSON"))])),
        Err(e) => obj(vec![("error", s(e))]),
    }
}

/// Runs this engine binary as `web.py` runs it: its JSON, or `{"error": …}`.
fn engine_json(user: &UserHome, args: &[&str], owner_token: Option<&str>) -> Json {
    let Ok(exe) = std::env::current_exe() else {
        return obj(vec![("error", s("no engine"))]);
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).arg("--home").arg(&user.home);
    if let Some(t) = owner_token {
        cmd.env("CASHCAST_OWNER_UI", t);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            pyjson::loads(&out)
                .unwrap_or_else(|| obj(vec![("error", s("bad JSON from the engine"))]))
        }
        Ok(o) => {
            let mut all = String::from_utf8_lossy(&o.stdout).to_string();
            all += &String::from_utf8_lossy(&o.stderr);
            obj(vec![("error", s(all.trim()))])
        }
        Err(e) => obj(vec![("error", s(e.to_string()))]),
    }
}

/// `bin/cashcast`: the one front door both languages share until step 9.
fn front_door() -> PathBuf {
    paths::root().join("bin").join("cashcast")
}

/// The action catalog (`actions --json`, generated from the CLI parser).
pub fn actions_catalog() -> Result<Json, Unported> {
    Ok(crate::core::commands::catalog())
}

/// Python `str(v)` for a form value.
fn py_text(v: &Json) -> String {
    match v {
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Null => "None".into(),
        other => other.py_str(),
    }
}

fn empty(v: &Json) -> bool {
    matches!(v, Json::Null | Json::Bool(false)) || matches!(v, Json::Str(x) if x.is_empty())
}

/// `argv_for`: the CLI words for one action and its form values.
pub fn argv_for(action: &Json, values: &[(String, Json)]) -> Result<Vec<String>, String> {
    let name = py::gets(action, "name", "");
    let args = py::list(action, "args");
    let known: Vec<String> = args.iter().map(|a| py::gets(a, "name", "")).collect();
    let mut unknown: Vec<String> = values
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !known.contains(k))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        let list: Vec<String> = unknown.iter().map(|u| paths::py_repr(u)).collect();
        return Err(format!("unknown field(s) [{}] for {name}", list.join(", ")));
    }
    let mut argv = vec![name.clone()];
    let mut positional = Vec::new();
    for a in args {
        let an = py::gets(a, "name", "");
        let v = values
            .iter()
            .find(|(k, _)| *k == an)
            .map(|(_, v)| v.clone())
            .unwrap_or(Json::Null);
        if empty(&v) {
            if a.get("required").is_some_and(Json::truthy) {
                return Err(format!("{an} is required for {name}"));
            }
            continue;
        }
        let vs = py_text(&v);
        if let Some(Json::List(ch)) = a.get("choices") {
            let ch: Vec<String> = ch.iter().map(Json::py_str).collect();
            if !ch.contains(&vs) {
                let list: Vec<String> = ch.iter().map(|c| paths::py_repr(c)).collect();
                return Err(format!("{an} must be one of [{}]", list.join(", ")));
            }
        }
        let kind = py::gets(a, "kind", "");
        if kind == "int" && vs.trim().parse::<i64>().is_err() {
            return Err(format!("{an} must be a whole number"));
        }
        let flag = a
            .get("flag")
            .filter(|f| !matches!(f, Json::Null))
            .map(Json::py_str);
        if kind == "flag" {
            if v != Json::Bool(true) {
                return Err(format!("{an} is on or off"));
            }
            argv.push(flag.unwrap_or_default());
        } else if let Some(f) = flag {
            argv.push(format!("{f}={vs}"));
        } else {
            if vs.starts_with('-') {
                return Err(format!("{an} cannot start with '-'"));
            }
            positional.push(vs);
        }
    }
    argv.extend(positional);
    Ok(argv)
}

/// `actions.run`: one action as the owner, its output and exit code.
fn run_action(raw: &str, headers: &[(String, String)], body: &[u8], token: &str) -> Reply {
    let ctype = header(headers, "Content-Type").unwrap_or_default();
    if ctype.split(';').next().unwrap_or("").trim() != "application/json" {
        return text(415, "send JSON");
    }
    let (path, q) = raw.split_once('?').unwrap_or((raw, ""));
    let name = unquote(path.trim_start_matches("/api/actions/"), false);
    let user = query(q, "user");
    let bad = |m: String| error_json(400, &m);
    let text_body = String::from_utf8_lossy(body);
    let req = if text_body.trim().is_empty() {
        Json::Obj(Vec::new())
    } else {
        match pyjson::loads(&text_body) {
            Some(v) => v,
            None => return bad("the request is not JSON".into()),
        }
    };
    let values = match req.get("values") {
        None => Vec::new(),
        Some(Json::Obj(v)) => v.clone(),
        Some(_) => return bad("values must be an object".into()),
    };
    let confirm = req.get("confirm") == Some(&Json::Bool(true));
    let catalog = match actions_catalog() {
        Ok(Json::List(c)) => c,
        _ => return bad("the action list is not available".into()),
    };
    let Some(action) = catalog.iter().find(|a| py::gets(a, "name", "") == name) else {
        return bad(format!(
            "unknown action {name} (see `bin/cashcast actions`)"
        ));
    };
    if let Some(why) = action.get("terminal_only").filter(|w| w.truthy()) {
        return bad(format!("{name} runs in a terminal only: {}", why.py_str()));
    }
    let choice = values
        .iter()
        .find(|(k, _)| k == "action")
        .map(|(_, v)| {
            if matches!(v, Json::Null) {
                String::new()
            } else {
                py_text(v)
            }
        })
        .unwrap_or_default();
    if let Some(why) = py::obj(action, "terminal_only_choices").get(&choice) {
        return bad(format!(
            "{name} {choice} runs in a terminal only: {}",
            why.py_str()
        ));
    }
    let why = action
        .get("confirm")
        .filter(|w| w.truthy())
        .or_else(|| py::obj(action, "confirm_choices").get(&choice));
    if let Some(why) = why
        && !confirm
    {
        return bad(format!("{name} {}: confirm to run it", why.py_str()));
    }
    if values
        .iter()
        .any(|(k, v)| k == "yes" && *v == Json::Bool(true))
        && !confirm
    {
        return bad(format!(
            "{name} with yes changes something hard to undo: confirm to run it"
        ));
    }
    let mut argv = match argv_for(action, &values) {
        Ok(a) => a,
        Err(m) => return bad(m),
    };
    if let Some(u) = user {
        argv.splice(0..0, ["--user".to_string(), u]);
    }
    let out = std::process::Command::new(front_door())
        .args(&argv)
        .env("CASHCAST_OWNER_UI", token)
        .stdin(std::process::Stdio::null())
        .output();
    let (code, output) = match out {
        Ok(o) => {
            let mut all = String::from_utf8_lossy(&o.stdout).to_string();
            all += &String::from_utf8_lossy(&o.stderr);
            (o.status.code().unwrap_or(1) as i64, all)
        }
        Err(e) => (1, e.to_string()),
    };
    let chars: Vec<char> = output.chars().collect();
    let tail: String = chars[chars.len().saturating_sub(20000)..].iter().collect();
    let mut shown = vec!["bin/cashcast".to_string()];
    shown.extend(argv);
    json_reply(
        200,
        &obj(vec![
            ("action", s(name)),
            (
                "argv",
                Json::List(shown.into_iter().map(Json::Str).collect()),
            ),
            ("exit", Json::Int(code)),
            ("output", s(tail)),
        ]),
    )
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

/// The pure request handler: (status, content type, body). Tested without a socket.
pub fn handle(
    method: &str,
    raw: &str,
    headers: &[(String, String)],
    token: &str,
    port: u16,
    today: NaiveDate,
    body: &[u8],
) -> Reply {
    let host = header(headers, "Host").unwrap_or_default();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return text(403, "forbidden host");
    }
    let no_frag = raw.split('#').next().unwrap_or("");
    let (path, q) = no_frag.split_once('?').unwrap_or((no_frag, ""));
    let given = header(headers, "X-Cashcast-Token").unwrap_or_default();
    if method == "POST" && path.starts_with("/api/actions/") {
        if !same(&given, token) {
            return text(401, "missing or wrong token");
        }
        return run_action(no_frag, headers, body, token);
    }
    if method != "GET" {
        return text(405, "only actions change anything");
    }
    if path.starts_with("/api/") {
        if !same(&given, token) {
            return text(401, "missing or wrong token");
        }
        return match api_get(path, q, token, today) {
            Ok(v) => json_reply(200, &v),
            Err(reply) => reply,
        };
    }
    static_file(path)
}

fn dist() -> PathBuf {
    paths::root().join("web").join("dist")
}

fn static_file(path: &str) -> Reply {
    let dist = dist();
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let root = std::fs::canonicalize(&dist).unwrap_or(dist.clone());
    let target = std::fs::canonicalize(dist.join(rel))
        .ok()
        .filter(|t| t.starts_with(&root) && t.is_file())
        .unwrap_or_else(|| dist.join("index.html"));
    if !target.is_file() {
        return text(
            503,
            "web app not built: run scripts/setup.sh (or: cd web && pnpm install && pnpm build)",
        );
    }
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    match std::fs::read(&target) {
        Ok(b) => (200, mime(&name).into(), b),
        Err(_) => text(404, "not found"),
    }
}

// --- The server ---------------------------------------------------------------

/// A new access token: 24 random bytes, URL-safe base64 (as `secrets.token_urlsafe(24)`).
pub fn new_token() -> Result<String, AppError> {
    use std::io::Read;
    let mut b = [0u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(AppError::io)?;
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        for k in (0..4).rev() {
            out.push(A[((n >> (6 * k)) & 63) as usize] as char);
        }
    }
    Ok(out)
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

fn respond(
    mut stream: std::net::TcpStream,
    token: &str,
    port: u16,
    today: impl Fn() -> NaiveDate,
) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Read, Write};
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    // Python's http.server limits: 64 KiB a line, 100 headers.
    const MAX_LINE: u64 = 65536;
    let mut line = String::new();
    if (&mut reader).take(MAX_LINE).read_line(&mut line)? == 0 || !line.ends_with('\n') {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let (method, raw) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("/").to_string(),
    );
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if (&mut reader).take(MAX_LINE).read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if !h.ends_with('\n') || headers.len() >= 100 {
            return Ok(());
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let (status, ctype, body) = if method == "POST" {
        match header(&headers, "Content-Length")
            .unwrap_or_else(|| "0".into())
            .parse::<usize>()
        {
            // Never read to the end: it would wait for the client to close.
            Ok(n) if n <= 1_000_000 => {
                let mut b = vec![0u8; n];
                reader.read_exact(&mut b)?;
                handle(&method, &raw, &headers, token, port, today(), &b)
            }
            _ => text(400, "bad Content-Length"),
        }
    } else {
        handle(&method, &raw, &headers, token, port, today(), b"")
    };
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
        status_text(status),
        body.len()
    );
    if method != "POST" {
        head += &format!(
            "Referrer-Policy: no-referrer\r\nContent-Security-Policy: {CSP}\r\nX-Frame-Options: DENY\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Resource-Policy: same-origin\r\nPermissions-Policy: camera=(), microphone=(), geolocation=(), interest-cohort=()\r\n"
        );
    }
    head += "\r\n";
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

/// `serve`: runs until stopped. Writes the state file (with the token link)
/// that `bin/cashcast` reads, and removes it on the way out.
pub fn serve(port: u16) -> Result<(), AppError> {
    use std::net::TcpListener;
    let os = AppError::io;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(os)?;
    let port = listener.local_addr().map_err(os)?.port();
    let token = new_token()?;
    let url = format!("http://127.0.0.1:{port}/#t={token}");
    crate::tasks::start::write_state(port, &url).map_err(AppError::io_text)?;
    if consent::agent_context() {
        // The link holds the token: an agent or a log file never gets it.
        println!(
            "cashcast web app: running on port {port}. Its link holds an access token, so it prints only in a terminal: run `bin/cashcast` there. Press Ctrl+C to stop."
        );
    } else {
        println!("cashcast web app: {url}\nOnly this computer can open it. Press Ctrl+C to stop.");
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();
    for name in paths::list_users() {
        if let Ok(u) = paths::resolve(Some(&name)) {
            crate::core::events::log(&u, "web_started", &[]);
        }
    }
    clear_on_signal();
    let token = std::sync::Arc::new(token);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let token = token.clone();
        std::thread::spawn(move || {
            let _ = respond(stream, &token, port, || chrono::Local::now().date_naive());
        });
    }
    crate::tasks::start::clear_state();
    Ok(())
}

/// Ctrl+C and `bin/cashcast stop` (SIGTERM) remove the state file before exit.
fn clear_on_signal() {
    use std::sync::atomic::{AtomicBool, Ordering};
    // Process scope: set by the handler, read by the watcher thread below. The
    // handler only stores a flag, the one async-signal-safe thing it can do.
    static STOP: AtomicBool = AtomicBool::new(false);
    unsafe extern "C" fn on_signal(_: i32) {
        STOP.store(true, Ordering::SeqCst);
    }
    std::thread::spawn(|| {
        while !STOP.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        crate::tasks::start::clear_state();
        eprintln!("stopped");
        std::process::exit(0);
    });
    unsafe extern "C" {
        fn signal(sig: i32, handler: unsafe extern "C" fn(i32)) -> usize;
    }
    // SAFETY: installs a handler for SIGINT (2) and SIGTERM (15) once, at start-up.
    unsafe {
        signal(2, on_signal);
        signal(15, on_signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_and_query_match_parse_qs() {
        assert_eq!(unquote("Save%2010%20more.md", true), "Save 10 more.md");
        assert_eq!(unquote("a+b", true), "a b");
        assert_eq!(unquote("100%", true), "100%");
        assert_eq!(query("user=alex&q=", "q"), None);
        assert_eq!(query("user=alex&user=sam", "user").as_deref(), Some("alex"));
        // parse_qs splits on `&` only.
        assert_eq!(query("path=a;b.md", "path").as_deref(), Some("a;b.md"));
    }

    #[test]
    fn snippet_never_panics_when_lowercase_grows() {
        let body = format!("{}needle", "\u{130}".repeat(200));
        assert_eq!(snippet(&body, "needle"), Some(String::new()));
        assert_eq!(
            snippet("a needle b", "needle").as_deref(),
            Some("a needle b")
        );
        assert_eq!(snippet("abc", "zzz"), None);
    }

    #[test]
    fn host_and_token_are_checked_first() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let h = |host: &str, tok: &str| {
            vec![
                ("Host".to_string(), host.to_string()),
                ("X-Cashcast-Token".to_string(), tok.to_string()),
            ]
        };
        assert_eq!(
            handle(
                "GET",
                "/api/users",
                &h("evil.example:9", "t"),
                "t",
                9,
                day,
                b""
            )
            .0,
            403
        );
        assert_eq!(
            handle(
                "GET",
                "/api/users",
                &h("127.0.0.1:9", "x"),
                "t",
                9,
                day,
                b""
            )
            .0,
            401
        );
        assert_eq!(
            handle("PUT", "/", &h("127.0.0.1:9", "t"), "t", 9, day, b"").0,
            405
        );
        assert_eq!(
            handle(
                "POST",
                "/api/actions/x",
                &h("localhost:9", ""),
                "t",
                9,
                day,
                b""
            )
            .0,
            401
        );
    }

    #[test]
    fn argv_for_builds_flags_choices_and_positionals() {
        let action = pyjson::loads(
            r#"{"name": "moneywiz", "args": [
              {"name": "action", "flag": null, "kind": "choice", "choices": ["sync", "status"], "required": true},
              {"name": "dry_run", "flag": "--dry-run", "kind": "flag", "choices": null, "required": false},
              {"name": "today", "flag": "--today", "kind": "text", "choices": null, "required": false}]}"#,
        )
        .unwrap();
        let v = |pairs: &[(&str, Json)]| -> Vec<(String, Json)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        };
        assert_eq!(
            argv_for(
                &action,
                &v(&[("action", s("sync")), ("dry_run", Json::Bool(true))])
            )
            .unwrap(),
            ["moneywiz", "--dry-run", "sync"]
        );
        assert_eq!(
            argv_for(&action, &v(&[("action", s("nope"))])).unwrap_err(),
            "action must be one of ['sync', 'status']"
        );
        assert_eq!(
            argv_for(&action, &v(&[("bogus", Json::Null)])).unwrap_err(),
            "unknown field(s) ['bogus'] for moneywiz"
        );
        assert_eq!(
            argv_for(&action, &v(&[])).unwrap_err(),
            "action is required for moneywiz"
        );
    }

    #[test]
    fn tokens_are_url_safe_and_32_chars() {
        let t = new_token().unwrap();
        assert_eq!(t.len(), 32);
        assert!(
            t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_ne!(t, new_token().unwrap());
    }
}
