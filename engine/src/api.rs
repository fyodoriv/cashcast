//! Read-only API clients for the services that offer a personal API (port of
//! `api/services.py`).
//!
//! Rules every client follows:
//! - Read endpoints only, plus the one sync write the owner asks for (a Google
//!   Sheets range). No client can move money.
//! - Downloads of transactions are saved unparsed into the inbox; the engine
//!   parses them. Balances are summed into `data/api-balances.json`.
//! - Tokens come from `http::secret`; they are never printed or logged.

use std::io::Read;

use chrono::{Duration, NaiveDate};

use crate::core::events;
use crate::core::http::{self, api_error, parse_qsl, quote, request, secret, urlencode};
use crate::core::paths::{AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::digest;

type Headers = Vec<(String, String)>;

fn hdr(pairs: &[(&str, &str)]) -> Headers {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn json(r: http::Response) -> Result<Json, AppError> {
    pyjson::loads(&r.text()).ok_or_else(|| api_error("the service sent a reply that is not JSON"))
}

fn num(v: Option<&Json>) -> f64 {
    match v {
        Some(Json::Int(i)) => *i as f64,
        Some(Json::Float(f)) => *f,
        Some(Json::Str(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn list(v: Option<&Json>) -> &[Json] {
    match v {
        Some(Json::List(items)) => items,
        _ => &[],
    }
}

fn s<'a>(v: Option<&'a Json>) -> &'a str {
    match v {
        Some(Json::Str(s)) => s,
        _ => "",
    }
}

/// Adds `amount` to `name`, keeping first-seen order (a Python dict).
fn add(out: &mut Vec<(String, f64)>, name: &str, amount: f64) {
    match out.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 += amount,
        None => out.push((name.to_string(), amount)),
    }
}

fn floats(values: &[(String, f64)]) -> Json {
    Json::Obj(
        values
            .iter()
            .map(|(k, v)| (k.clone(), Json::Float(*v)))
            .collect(),
    )
}

/// `_save_balances`: this service's entry, with today's date and any extras.
fn save_balances(
    user: &UserHome,
    service: &str,
    balances: &[(String, f64)],
    extra: Vec<(String, Json)>,
    today: NaiveDate,
) -> Result<(), AppError> {
    let mut entry = vec![
        ("balances".to_string(), floats(balances)),
        ("fetched_on".to_string(), Json::Str(today.to_string())),
    ];
    entry.extend(extra);
    crate::tasks::balances::save(user, service, Json::Obj(entry))
}

// --- Wise -------------------------------------------------------------------

const WISE: &str = "https://api.wise.com";

/// The personal profile id and its STANDARD balances.
fn wise_profile(user: &UserHome) -> Result<(Headers, String, Vec<Json>), AppError> {
    let token = secret(user, "CASHCAST_WISE_TOKEN")?;
    let h = hdr(&[("Authorization", &format!("Bearer {token}"))]);
    let profiles = json(request("GET", &format!("{WISE}/v2/profiles"), &h, None)?)?;
    let profiles = list(Some(&profiles));
    let personal = profiles
        .iter()
        .find(|p| s(p.get("type")).to_uppercase() == "PERSONAL")
        .or(profiles.first())
        .ok_or_else(|| api_error("Wise: no profile on this token"))?;
    let pid = personal.get("id").map(Json::py_str).unwrap_or_default();
    let rows = json(request(
        "GET",
        &format!("{WISE}/v4/profiles/{pid}/balances?types=STANDARD"),
        &h,
        None,
    )?)?;
    Ok((h, pid, list(Some(&rows)).to_vec()))
}

pub fn wise_balances(user: &UserHome, today: NaiveDate) -> Result<Vec<(String, f64)>, AppError> {
    let (_, _, rows) = wise_profile(user)?;
    let mut out = Vec::new();
    for b in &rows {
        let amt = b.get("amount");
        let cur = amt
            .and_then(|a| a.get("currency"))
            .map(Json::py_str)
            .unwrap_or_else(|| "?".into());
        add(&mut out, &cur, num(amt.and_then(|a| a.get("value"))));
    }
    save_balances(user, "wise", &out, vec![], today)?;
    Ok(out)
}

pub fn wise_statements(
    user: &UserHome,
    days: i64,
    today: NaiveDate,
) -> Result<Vec<String>, AppError> {
    let (h, pid, rows) = wise_profile(user)?;
    let mut saved = Vec::new();
    for b in &rows {
        let cur = b
            .get("amount")
            .and_then(|a| a.get("currency"))
            .map(Json::py_str)
            .unwrap_or_default();
        let id = b.get("id").map(Json::py_str).unwrap_or_default();
        // Wise gives at most 469 days per statement: ask in pieces of 400,
        // oldest first, and keep the header of the first piece only.
        let mut body: Vec<u8> = Vec::new();
        let mut start = today - Duration::days(days);
        while start <= today {
            let end = (start + Duration::days(399)).min(today);
            let q = urlencode(&[
                ("currency".into(), cur.clone()),
                ("intervalStart".into(), format!("{start}T00:00:00.000Z")),
                ("intervalEnd".into(), format!("{end}T23:59:59.999Z")),
                ("type".into(), "COMPACT".into()),
            ]);
            let url =
                format!("{WISE}/v1/profiles/{pid}/balance-statements/{id}/statement.csv?{q}");
            let piece = request("GET", &url, &h, None)?.body;
            if body.is_empty() {
                body = piece;
            } else if let Some(nl) = piece.iter().position(|b| *b == b'\n') {
                if !body.ends_with(b"\n") {
                    body.push(b'\n');
                }
                body.extend_from_slice(&piece[nl + 1..]);
            }
            start = end + Duration::days(1);
        }
        saved.push(http::save_download(
            user,
            &format!("wise-{cur}.csv"),
            &body,
        )?);
        // Files from before the stable name hold the same rows: remove them.
        remove_matching(user, &format!("wise-{cur}-20"), ".csv", &[]);
    }
    events::log(
        user,
        "api_download",
        &[
            ("service", Json::Str("wise".into())),
            ("result", Json::Str("ok".into())),
        ],
    );
    Ok(saved)
}

// --- Kraken -----------------------------------------------------------------

const KRAKEN: &str = "https://api.kraken.com";

/// `API-Sign`: HMAC-SHA512 of path + SHA256(nonce + body), keyed by the decoded secret.
pub fn kraken_sign(
    path: &str,
    data: &[(String, String)],
    secret_b64: &str,
) -> Result<String, AppError> {
    let nonce = data
        .iter()
        .find(|(k, _)| k == "nonce")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let mut message = path.as_bytes().to_vec();
    message.extend_from_slice(&digest::sha256(
        format!("{nonce}{}", urlencode(data)).as_bytes(),
    ));
    let key = digest::base64_decode(secret_b64)
        .ok_or_else(|| api_error("CASHCAST_KRAKEN_SECRET is not base64"))?;
    Ok(digest::base64(&digest::hmac_sha512(&key, &message)))
}

fn kraken(
    user: &UserHome,
    method: &str,
    extra: &[(&str, &str)],
) -> Result<http::Response, AppError> {
    let key = secret(user, "CASHCAST_KRAKEN_KEY")?;
    let secret_b64 = secret(user, "CASHCAST_KRAKEN_SECRET")?;
    let path = format!("/0/private/{method}");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut data = vec![("nonce".to_string(), nanos.to_string())];
    data.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let sign = kraken_sign(&path, &data, &secret_b64)?;
    let h = hdr(&[
        ("API-Key", &key),
        ("API-Sign", &sign),
        ("Content-Type", "application/x-www-form-urlencoded"),
    ]);
    request(
        "POST",
        &format!("{KRAKEN}{path}"),
        &h,
        Some(urlencode(&data).as_bytes()),
    )
}

const KRAKEN_FIAT: [&str; 7] = ["CAD", "USD", "EUR", "GBP", "JPY", "AUD", "CHF"];
const KRAKEN_PREFIXED: [&str; 11] = [
    "XXBT", "XETH", "XLTC", "XXRP", "XXLM", "XXMR", "XZEC", "XETC", "XREP", "XMLN", "XXDG",
];

/// Kraken's asset code as a plain symbol (XXBT → BTC, ZCAD → CAD, ETH.S → ETH).
pub fn kraken_asset(raw: &str) -> String {
    let upper = raw.trim().to_uppercase();
    let mut a = upper.split('.').next().unwrap_or("").to_string();
    if KRAKEN_PREFIXED.contains(&a.as_str()) || KRAKEN_FIAT.iter().any(|c| a == format!("Z{c}")) {
        a.remove(0);
    }
    match a.as_str() {
        "XBT" => "BTC".into(),
        "XDG" => "DOGE".into(),
        "ETH2" => "ETH".into(),
        _ => a,
    }
}

fn kraken_errors(r: &Json) -> Result<(), AppError> {
    let errs = list(r.get("error"));
    if errs.is_empty() {
        return Ok(());
    }
    Err(api_error(format!(
        "Kraken: {}",
        errs.iter().map(Json::py_str).collect::<Vec<_>>().join("; ")
    )))
}

/// Last trade price per crypto asset from Kraken's public ticker: in CAD, else in USD.
pub fn kraken_prices(assets: &[String]) -> Json {
    let mut out = Vec::new();
    for asset in assets {
        let pair_name = match asset.as_str() {
            "BTC" => "XBT",
            "DOGE" => "XDG",
            a => a,
        };
        for q in ["CAD", "USD"] {
            let Ok(r) = request(
                "GET",
                &format!("{KRAKEN}/0/public/Ticker?pair={pair_name}{q}"),
                &[],
                None,
            ) else {
                continue;
            };
            let Ok(r) = json(r) else { continue };
            let first = match r.get("result") {
                Some(Json::Obj(items)) => items.first().map(|(_, v)| v.clone()),
                _ => None,
            };
            if !r.get("error").is_some_and(Json::truthy)
                && let Some(t) = first
            {
                let price = num(list(t.get("c")).first());
                out.push((
                    asset.clone(),
                    Json::Obj(vec![
                        ("price".into(), Json::Float(price)),
                        ("currency".into(), Json::Str(q.into())),
                    ]),
                ));
                break;
            }
        }
    }
    Json::Obj(out)
}

pub fn kraken_balances(user: &UserHome, today: NaiveDate) -> Result<Vec<(String, f64)>, AppError> {
    let r = json(kraken(user, "Balance", &[])?)?;
    kraken_errors(&r)?;
    let mut out = Vec::new();
    if let Some(Json::Obj(items)) = r.get("result") {
        for (k, v) in items {
            let amount = num(Some(v));
            if amount != 0.0 {
                add(&mut out, &kraken_asset(k), amount);
            }
        }
    }
    let crypto: Vec<String> = out
        .iter()
        .map(|(a, _)| a.clone())
        .filter(|a| !KRAKEN_FIAT.contains(&a.as_str()))
        .collect();
    let prices = kraken_prices(&crypto);
    save_balances(user, "kraken", &out, vec![("prices".into(), prices)], today)?;
    Ok(out)
}

pub fn kraken_export_ledgers(
    user: &UserHome,
    wait_seconds: u64,
    today: NaiveDate,
) -> Result<String, AppError> {
    let r = json(kraken(
        user,
        "AddExport",
        &[
            ("report", "ledgers"),
            ("format", "CSV"),
            ("description", "cashcast"),
        ],
    )?)?;
    kraken_errors(&r)?;
    let rid = r
        .get("result")
        .and_then(|x| x.get("id"))
        .map(Json::py_str)
        .unwrap_or_default();
    let mut ready = false;
    for _ in 0..(wait_seconds / 5).max(1) {
        let st = json(kraken(user, "ExportStatus", &[("report", "ledgers")])?)?;
        if list(st.get("result")).iter().any(|x| {
            x.get("id").map(Json::py_str).as_deref() == Some(&rid)
                && s(x.get("status")) == "Processed"
        }) {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    if !ready {
        return Err(api_error(
            "Kraken: export not ready yet; run the same command again in a few minutes",
        ));
    }
    let blob = kraken(user, "RetrieveExport", &[("id", &rid)])?.body;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(blob))
        .map_err(|e| api_error(format!("Kraken: export is not a zip ({e})")))?;
    let mut csv = None;
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| api_error(format!("Kraken: {e}")))?;
        if f.name().is_ok_and(|n| n.ends_with(".csv")) {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| api_error(format!("Kraken: {e}")))?;
            csv = Some(buf);
            break;
        }
    }
    let csv = csv.ok_or_else(|| api_error("Kraken: no CSV in the export"))?;
    let saved = http::save_download(user, &format!("kraken-ledgers-{today}.csv"), &csv)?;
    events::log(
        user,
        "api_download",
        &[
            ("service", Json::Str("kraken".into())),
            ("result", Json::Str("ok".into())),
        ],
    );
    Ok(saved)
}

// --- PayPal -----------------------------------------------------------------

const PAYPAL: &str = "https://api-m.paypal.com";

pub fn paypal_balances(user: &UserHome, today: NaiveDate) -> Result<Vec<(String, f64)>, AppError> {
    let cid = secret(user, "CASHCAST_PAYPAL_CLIENT_ID")?;
    let sec = secret(user, "CASHCAST_PAYPAL_SECRET")?;
    let basic = digest::base64(format!("{cid}:{sec}").as_bytes());
    let h = hdr(&[
        ("Authorization", &format!("Basic {basic}")),
        ("Content-Type", "application/x-www-form-urlencoded"),
    ]);
    let tok = json(request(
        "POST",
        &format!("{PAYPAL}/v1/oauth2/token"),
        &h,
        Some(b"grant_type=client_credentials"),
    )?)?;
    let access = tok
        .get("access_token")
        .map(Json::py_str)
        .ok_or_else(|| AppError::new("keyerror", "'access_token'"))?;
    let bearer = format!("Bearer {access}");
    let r = json(request(
        "GET",
        &format!("{PAYPAL}/v1/reporting/balances"),
        &hdr(&[("Authorization", &bearer)]),
        None,
    )?)?;
    let mut out: Vec<(String, f64)> = Vec::new();
    for b in list(r.get("balances")) {
        let cur = b.get("currency").map(Json::py_str).unwrap_or_default();
        let v = num(b.get("total_balance").and_then(|t| t.get("value")));
        match out.iter_mut().find(|(n, _)| *n == cur) {
            Some(slot) => slot.1 = v,
            None => out.push((cur, v)),
        }
    }
    save_balances(user, "paypal", &out, vec![], today)?;
    Ok(out)
}

// --- E*TRADE (OAuth 1.0a) ---------------------------------------------------

const ETRADE: &str = "https://api.etrade.com";

/// An OAuth 1.0a `Authorization` header signed with HMAC-SHA1.
pub fn oauth1_header(
    method: &str,
    url: &str,
    consumer: (&str, &str),
    token: Option<(&str, &str)>,
    extra: &[(&str, &str)],
    nonce: Option<&str>,
    timestamp: Option<&str>,
) -> String {
    let nonce = nonce.map(str::to_string).unwrap_or_else(|| {
        let mut b = [0u8; 8];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut b))
            .expect("the OS random source must work: a fixed nonce would repeat");
        digest::hex(&b)
    });
    let ts = timestamp.map(str::to_string).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .to_string()
    });
    let mut params: Vec<(String, String)> = vec![
        ("oauth_consumer_key".into(), consumer.0.into()),
        ("oauth_nonce".into(), nonce),
        ("oauth_signature_method".into(), "HMAC-SHA1".into()),
        ("oauth_timestamp".into(), ts),
        ("oauth_version".into(), "1.0".into()),
    ];
    params.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    if let Some((t, _)) = token {
        params.push(("oauth_token".into(), t.into()));
    }
    let (base_url, query) = url.split_once('?').unwrap_or((url, ""));
    let mut all: Vec<(String, String)> = params.clone();
    for (k, v) in parse_qsl(query) {
        match all.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = v,
            None => all.push((k, v)),
        }
    }
    all.sort();
    let norm = all
        .iter()
        .map(|(k, v)| format!("{}={}", quote(k), quote(v)))
        .collect::<Vec<_>>()
        .join("&");
    let base = [method.to_uppercase(), quote(base_url), quote(&norm)].join("&");
    let key = format!(
        "{}&{}",
        quote(consumer.1),
        token.map(|t| quote(t.1)).unwrap_or_default()
    );
    params.push((
        "oauth_signature".into(),
        digest::base64(&digest::hmac_sha1(key.as_bytes(), base.as_bytes())),
    ));
    params.sort();
    "OAuth ".to_string()
        + &params
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", quote(k), quote(v)))
            .collect::<Vec<_>>()
            .join(", ")
}

fn consumer(user: &UserHome) -> Result<(String, String), AppError> {
    Ok((
        secret(user, "CASHCAST_ETRADE_KEY")?,
        secret(user, "CASHCAST_ETRADE_SECRET")?,
    ))
}

fn qs_pair(text: &str) -> Result<(String, String), AppError> {
    let q = parse_qsl(text);
    let get = |k: &str| {
        q.iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| AppError::new("keyerror", format!("'{k}'")))
    };
    Ok((get("oauth_token")?, get("oauth_token_secret")?))
}

/// Step 1 of E*TRADE sign-in: a request token and the URL the owner opens.
pub fn etrade_authorize_url(user: &UserHome) -> Result<(String, (String, String)), AppError> {
    let c = consumer(user)?;
    let url = format!("{ETRADE}/oauth/request_token");
    let auth = oauth1_header(
        "GET",
        &url,
        (&c.0, &c.1),
        None,
        &[("oauth_callback", "oob")],
        None,
        None,
    );
    let req = qs_pair(&request("GET", &url, &hdr(&[("Authorization", &auth)]), None)?.text())?;
    Ok((
        format!(
            "https://us.etrade.com/e/t/etws/authorize?key={}&token={}",
            c.0, req.0
        ),
        req,
    ))
}

/// Step 2: trades the code the owner copied for an access token (valid until midnight US Eastern).
pub fn etrade_access_token(
    user: &UserHome,
    req: &(String, String),
    verifier: &str,
) -> Result<(String, String), AppError> {
    let c = consumer(user)?;
    let url = format!("{ETRADE}/oauth/access_token");
    let auth = oauth1_header(
        "GET",
        &url,
        (&c.0, &c.1),
        Some((&req.0, &req.1)),
        &[("oauth_verifier", verifier)],
        None,
        None,
    );
    qs_pair(&request("GET", &url, &hdr(&[("Authorization", &auth)]), None)?.text())
}

pub fn etrade_balances(user: &UserHome, today: NaiveDate) -> Result<Vec<(String, f64)>, AppError> {
    let c = consumer(user)?;
    let t = (
        secret(user, "CASHCAST_ETRADE_TOKEN")?,
        secret(user, "CASHCAST_ETRADE_TOKEN_SECRET")?,
    );
    let signed = |url: &str| -> Result<Json, AppError> {
        let auth = oauth1_header(
            "GET",
            url,
            (&c.0, &c.1),
            Some((&t.0, &t.1)),
            &[],
            None,
            None,
        );
        json(request(
            "GET",
            url,
            &hdr(&[("Authorization", &auth)]),
            None,
        )?)
    };
    let accts = signed(&format!("{ETRADE}/v1/accounts/list.json"))?;
    let accounts = accts
        .get("AccountListResponse")
        .and_then(|x| x.get("Accounts"))
        .and_then(|x| x.get("Account"));
    let mut out = Vec::new();
    for (i, a) in list(accounts).iter().enumerate() {
        let key = a.get("accountIdKey").map(Json::py_str).unwrap_or_default();
        let b = signed(&format!(
            "{ETRADE}/v1/accounts/{key}/balance.json?instType=BROKERAGE&realTimeNAV=true"
        ))?;
        let total = b
            .get("BalanceResponse")
            .and_then(|x| x.get("Computed"))
            .and_then(|x| x.get("RealTimeValues"))
            .and_then(|x| x.get("totalAccountValue"));
        out.push((format!("account {} (USD)", i + 1), num(total)));
    }
    save_balances(user, "etrade", &out, vec![], today)?;
    Ok(out)
}

// --- Google Sheets ------------------------------------------------------------

pub fn sheets_download(user: &UserHome, sheet_id: &str, gid: &str) -> Result<String, AppError> {
    let token = secret(user, "CASHCAST_GOOGLE_TOKEN")?;
    let url =
        format!("https://docs.google.com/spreadsheets/d/{sheet_id}/export?format=csv&gid={gid}");
    let body = request(
        "GET",
        &url,
        &hdr(&[("Authorization", &format!("Bearer {token}"))]),
        None,
    )?
    .body;
    let short: String = sheet_id.chars().take(8).collect();
    let saved = http::save_download(user, &format!("sheet-{short}-{gid}.csv"), &body)?;
    events::log(
        user,
        "api_download",
        &[
            ("service", Json::Str("google-sheets".into())),
            ("result", Json::Str("ok".into())),
        ],
    );
    Ok(saved)
}

const SHEETS: &str = "https://sheets.googleapis.com/v4/spreadsheets";

/// Writes `rows` to one range of the owner's Google Sheet (`sync sheets`).
/// Returns the count of cells Google updated.
pub fn sheets_write(
    user: &UserHome,
    sheet_id: &str,
    range_a1: &str,
    rows: Vec<Json>,
    today: NaiveDate,
) -> Result<i64, AppError> {
    crate::core::consent::require(user, "cloud:sheets", today)?;
    let token = secret(user, "CASHCAST_GOOGLE_TOKEN")?;
    // `urllib.parse.quote` keeps "/" by default.
    let url = format!(
        "{SHEETS}/{sheet_id}/values/{}?valueInputOption=USER_ENTERED",
        quote(range_a1).replace("%2F", "/")
    );
    let body = Json::Obj(vec![
        ("range".into(), Json::Str(range_a1.into())),
        ("majorDimension".into(), Json::Str("ROWS".into())),
        ("values".into(), Json::List(rows)),
    ]);
    let r = json(request(
        "PUT",
        &url,
        &hdr(&[
            ("Authorization", &format!("Bearer {token}")),
            ("Content-Type", "application/json"),
        ]),
        Some(pyjson::dumps(&body, None, false).as_bytes()),
    )?)?;
    events::log(
        user,
        "cloud_sync",
        &[
            ("service", Json::Str("google-sheets".into())),
            ("result", Json::Str("ok".into())),
        ],
    );
    Ok(num(r.get("updatedCells")) as i64)
}

// --- Market data (Alpha Vantage) ---------------------------------------------

const ALPHA: &str = "https://www.alphavantage.co/query";

fn av_num(v: Option<&Json>) -> Json {
    match v {
        Some(Json::Int(i)) => Json::Float(*i as f64),
        Some(Json::Float(f)) => Json::Float(*f),
        Some(Json::Str(t)) => {
            let t = t.trim();
            match t.parse::<f64>() {
                Ok(x) if !t.is_empty() && !t.contains('_') => Json::Float(x),
                _ => Json::Null,
            }
        }
        _ => Json::Null,
    }
}

/// Price and basic fundamentals for one ticker (port of `marketdata.fetch`).
/// Only the symbol leaves the computer; replies are cached per day in
/// `data/market/`, so each symbol is fetched at most once a day.
pub fn market(user: &UserHome, symbol: &str, today: NaiveDate) -> Result<Json, AppError> {
    crate::core::consent::require(user, "net:market-data", today)?;
    let upper = symbol.to_uppercase();
    let dir = user.data().join("market");
    let cache = dir.join(format!("{}-{today}.json", upper.replace('/', "_")));
    if let Ok(text) = std::fs::read_to_string(&cache) {
        return pyjson::loads(&text).ok_or_else(|| api_error("market cache is not JSON"));
    }
    let key = secret(user, "CASHCAST_ALPHAVANTAGE_KEY")?;
    let sym = if upper.ends_with(".TO") {
        format!("{}.TRT", &symbol[..symbol.len() - 3])
    } else {
        symbol.to_string()
    };
    let query = |function: &str| -> Result<Json, AppError> {
        let q = urlencode(&[
            ("function".into(), function.into()),
            ("symbol".into(), sym.clone()),
            ("apikey".into(), key.clone()),
        ]);
        json(request("GET", &format!("{ALPHA}?{q}"), &[], None)?)
    };
    let quote = query("GLOBAL_QUOTE")?;
    let q = quote.get("Global Quote");
    let mut out = vec![
        ("symbol".to_string(), Json::Str(upper.clone())),
        (
            "price".to_string(),
            av_num(q.and_then(|q| q.get("05. price"))),
        ),
        (
            "as_of".to_string(),
            q.and_then(|q| q.get("07. latest trading day"))
                .cloned()
                .unwrap_or(Json::Null),
        ),
    ];
    if !upper.ends_with(".TO") {
        let ov = query("OVERVIEW")?;
        out.push((
            "asset_type".into(),
            ov.get("AssetType").cloned().unwrap_or(Json::Null),
        ));
        for (k, field) in [
            ("pe", "PERatio"),
            ("dividend_yield", "DividendYield"),
            ("profit_margin", "ProfitMargin"),
            ("roe", "ReturnOnEquityTTM"),
            ("revenue_growth", "QuarterlyRevenueGrowthYOY"),
            ("beta", "Beta"),
        ] {
            out.push((k.into(), av_num(ov.get(field))));
        }
    }
    let out = Json::Obj(out);
    std::fs::create_dir_all(&dir).map_err(AppError::io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    crate::write_private(&cache, &pyjson::dumps(&out, None, false)).map_err(AppError::io_text)?;
    events::log(
        user,
        "market_fetch",
        &[("service", Json::Str("alphavantage".into()))],
    );
    Ok(out)
}

/// Removes inbox files named `<prefix>...<suffix>`, except `keep`.
fn remove_matching(user: &UserHome, prefix: &str, suffix: &str, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(user.inbox()) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) && name.ends_with(suffix) && !keep.contains(&name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Saves each page of each account's transactions unparsed, as
/// `etrade-transactions-<account>-<page>.json`.
pub fn etrade_transactions(
    user: &UserHome,
    days: i64,
    today: NaiveDate,
) -> Result<Vec<String>, AppError> {
    let c = consumer(user)?;
    let t = (
        secret(user, "CASHCAST_ETRADE_TOKEN")?,
        secret(user, "CASHCAST_ETRADE_TOKEN_SECRET")?,
    );
    let get = |url: &str| -> Result<http::Response, AppError> {
        let auth = oauth1_header(
            "GET",
            url,
            (&c.0, &c.1),
            Some((&t.0, &t.1)),
            &[],
            None,
            None,
        );
        request("GET", url, &hdr(&[("Authorization", &auth)]), None)
    };
    let accts = json(get(&format!("{ETRADE}/v1/accounts/list.json"))?)?;
    let accounts = accts
        .get("AccountListResponse")
        .and_then(|x| x.get("Accounts"))
        .and_then(|x| x.get("Account"));
    let mut saved = Vec::new();
    for a in list(accounts) {
        let id = a.get("accountIdKey").map(Json::py_str).unwrap_or_default();
        // Named by a hash of the account key, so a closed account or a new order never mixes pages.
        let key = digest::hex(&digest::sha256(id.as_bytes()))[..8].to_string();
        let mut q: Vec<(String, String)> = vec![
            (
                "startDate".into(),
                (today - Duration::days(days)).format("%m%d%Y").to_string(),
            ),
            ("endDate".into(), today.format("%m%d%Y").to_string()),
            ("count".into(), "50".into()),
        ];
        let mut names = Vec::new();
        for page in 1..=200 {
            let r = get(&format!(
                "{ETRADE}/v1/accounts/{id}/transactions.json?{}",
                urlencode(&q)
            ))?;
            if r.status == 204 || r.body.iter().all(u8::is_ascii_whitespace) {
                break; // no transactions in the range
            }
            names.push(http::save_download(
                user,
                &format!("etrade-transactions-{key}-{page}.json"),
                &r.body,
            )?);
            let page_json = json(r)?;
            let resp = page_json.get("TransactionListResponse");
            let more = resp
                .and_then(|x| x.get("moreTransactions"))
                .is_some_and(Json::truthy);
            let marker = resp
                .and_then(|x| x.get("marker"))
                .map(Json::py_str)
                .filter(|m| !m.is_empty() && m != "None");
            let last = q
                .iter()
                .find(|(k, _)| k == "marker")
                .map(|(_, v)| v.clone());
            match marker {
                Some(m) if more && Some(&m) != last.as_ref() => {
                    q.retain(|(k, _)| k != "marker");
                    q.push(("marker".into(), m));
                }
                _ => break,
            }
        }
        // A shorter download than last time leaves stale pages: remove them.
        remove_matching(
            user,
            &format!("etrade-transactions-{key}-"),
            ".json",
            &names,
        );
        saved.extend(names);
    }
    events::log(
        user,
        "api_download",
        &[
            ("service", Json::Str("etrade".into())),
            ("result", Json::Str("ok".into())),
        ],
    );
    Ok(saved)
}

pub fn balances(
    user: &UserHome,
    service: &str,
    today: NaiveDate,
) -> Result<Vec<(String, f64)>, AppError> {
    match service {
        "wise" => wise_balances(user, today),
        "kraken" => kraken_balances(user, today),
        "paypal" => paypal_balances(user, today),
        "etrade" => etrade_balances(user, today),
        other => Err(AppError::new("keyerror", format!("'{other}'"))),
    }
}

/// Days to download: `default`, or back to `[moneywiz] since` when that is
/// older, so the MoneyWiz sync can check every row from that day on.
fn history_days(user: &UserHome, default: i64, today: NaiveDate) -> i64 {
    std::fs::read_to_string(user.config())
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| Some(t.get("moneywiz")?.get("since")?.as_str()?.to_string()))
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .map_or(default, |s| (today - s).num_days().max(default))
}

pub fn download(user: &UserHome, service: &str, today: NaiveDate) -> Result<Vec<String>, AppError> {
    match service {
        "wise" => wise_statements(user, history_days(user, 365, today), today),
        "kraken" => Ok(vec![kraken_export_ledgers(user, 120, today)?]),
        "etrade" => etrade_transactions(user, 730, today),
        other => Err(AppError::new("keyerror", format!("'{other}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `moneywiz sync` refreshed Wise with a fixed 365 days and cut the
    /// history `api download wise` had saved back to `[moneywiz] since`.
    /// Every caller outside this file goes through `download`.
    #[test]
    fn only_download_picks_the_wise_history_length() {
        let cli = include_str!("core/cli.rs");
        assert!(
            !cli.contains("wise_statements("),
            "cli.rs calls wise_statements directly: use api::download(user, \"wise\", day)"
        );
    }

    #[test]
    fn kraken_signature_matches_the_documented_example() {
        let data: Vec<(String, String)> = [
            ("nonce", "1616492376594"),
            ("ordertype", "limit"),
            ("pair", "XBTUSD"),
            ("price", "37500"),
            ("type", "buy"),
            ("volume", "1.25"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let secret = "kQH5HW/8p1uGOVjbgWA7FunAmGO8lsSUXNsu3eow76sz84Q18fWxnyRzBHCd3pd5nE9qa99HAZtuZuj6F1huXg=="; // gitleaks:allow (Kraken docs example)
        assert_eq!(
            kraken_sign("/0/private/AddOrder", &data, secret).unwrap(),
            "4/dpxb3iT4tp/ZCVEwSnEsLxx0bqyhLpdfOpc6fn7OR8+UClSV5n9E6aSS8MPtnRfp32bAb0nmbRn6H8ndwLUQ=="
        );
    }

    #[test]
    fn oauth1_signature_matches_the_classic_example() {
        let url = "https://api.twitter.com/1.1/statuses/update.json?include_entities=true&status=Hello%20Ladies%20%2B%20Gentlemen%2C%20a%20signed%20OAuth%20request%21";
        let header = oauth1_header(
            "POST",
            url,
            (
                "xvz1evFS4wEEPTGEFPHBog",
                "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw",
            ), // gitleaks:allow (public OAuth example)
            Some((
                "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb",
                "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE",
            )), // gitleaks:allow (public OAuth example)
            &[],
            Some("kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg"),
            Some("1318622958"),
        );
        assert!(
            header.contains("oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\""),
            "{header}"
        ); // gitleaks:allow (public OAuth example)
    }

    #[test]
    fn kraken_assets_read_as_plain_symbols() {
        for (raw, want) in [
            ("XXBT", "BTC"),
            ("ZCAD", "CAD"),
            ("ETH.S", "ETH"),
            ("ETH2.S", "ETH"),
            ("XXDG", "DOGE"),
            ("SOL", "SOL"),
        ] {
            assert_eq!(kraken_asset(raw), want);
        }
    }
}

const YAHOO: &str = "https://query1.finance.yahoo.com/v8/finance/chart";

/// The last market price of one Yahoo ticker (XEQT.TO, AAPL, BTC-CAD,
/// USDCAD=X) and its currency. Only the ticker leaves the computer, with
/// consent `net:market-data`; replies are cached per day in `data/market/`.
pub fn yahoo_price(
    user: &UserHome,
    ticker: &str,
    today: NaiveDate,
) -> Result<Option<(f64, String)>, AppError> {
    crate::core::consent::require(user, "net:market-data", today)?;
    let dir = user.data().join("market");
    let safe: String = ticker
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cache = dir.join(format!("yahoo-{safe}-{today}.json"));
    let body = match std::fs::read_to_string(&cache) {
        Ok(text) => text,
        Err(_) => {
            let url = format!("{YAHOO}/{}?range=5d&interval=1d", quote(ticker));
            let headers = hdr(&[("User-Agent", "cashcast")]);
            let r = request("GET", &url, &headers, None)?;
            let text = String::from_utf8_lossy(&r.body).to_string();
            std::fs::create_dir_all(&dir).map_err(AppError::io)?;
            crate::write_private(&cache, &text).map_err(AppError::io_text)?;
            text
        }
    };
    let j = pyjson::loads(&body).ok_or_else(|| api_error("market reply is not JSON"))?;
    let meta = j
        .get("chart")
        .and_then(|c| c.get("result"))
        .and_then(|r| list(Some(r)).first())
        .and_then(|r| r.get("meta"));
    let price = meta
        .map(|m| num(m.get("regularMarketPrice")))
        .unwrap_or(0.0);
    let currency = s(meta.and_then(|m| m.get("currency"))).to_uppercase();
    Ok((price > 0.0).then_some((price, currency)))
}
