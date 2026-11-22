//! US-10 API clients against recorded synthetic responses. No network.
//! Ported from tests/unit/test_api.py (the two signature tests live in
//! engine/src/api.rs).

mod app;

use app::{Env, grant_by_hand, mode, read, repo, set, today};
use cashcast_engine::api;
use cashcast_engine::core::http::{self, Response};
use cashcast_engine::core::pyjson::Json;
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

type Calls = Rc<RefCell<Vec<(String, String, Vec<(String, String)>, Option<String>)>>>;

/// Routes: URL substring → (status, body). Records every call.
fn fake(routes: Vec<(&'static str, Vec<u8>)>) -> Calls {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    http::set_transport(Some(Box::new(move |method, url, headers, body| {
        seen.borrow_mut().push((
            method.to_string(),
            url.to_string(),
            headers.to_vec(),
            body.map(|b| String::from_utf8_lossy(b).into_owned()),
        ));
        for (key, data) in &routes {
            if url.contains(key) {
                return Ok(Response {
                    status: 200,
                    body: data.clone(),
                });
            }
        }
        panic!("unexpected call {url}");
    })));
    calls
}

fn j(v: serde_json::Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

fn saved_balances(u: &cashcast_engine::core::paths::UserHome) -> serde_json::Value {
    serde_json::from_str(&read(&u.data().join("api-balances.json"))).unwrap()
}

#[test]
fn wise_balances_and_statement_download() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_WISE_TOKEN", "test-token");
    let calls = fake(vec![
        (
            "/v2/profiles",
            j(serde_json::json!([{"id": 7, "type": "PERSONAL"}])),
        ),
        (
            "/balances",
            j(
                serde_json::json!([{"id": 1, "amount": {"currency": "CAD", "value": 10.5}}, {"id": 2, "amount": {"currency": "USD", "value": 2}}]),
            ),
        ),
        (
            "statement.csv",
            b"TransferWise ID,Date,Amount\nCANARY-1,01-09-2026,1.00\n".to_vec(),
        ),
    ]);
    let bal = api::wise_balances(&u, today()).unwrap();
    assert_eq!(
        bal,
        vec![("CAD".to_string(), 10.5), ("USD".to_string(), 2.0)]
    );
    let saved = api::wise_statements(&u, 30, today()).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(mode(&u.inbox().join(&saved[0])), 0o600);
    assert!(calls.borrow().iter().all(|c| {
        c.2.iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer test-token")
    }));
    assert_eq!(saved_balances(&u)["wise"]["balances"]["CAD"], 10.5);
    http::set_transport(None);
    app::unset("CASHCAST_WISE_TOKEN");
}

fn kraken_env() {
    set("CASHCAST_KRAKEN_KEY", "k");
    set("CASHCAST_KRAKEN_SECRET", "c2VjcmV0");
}

#[test]
fn kraken_export_saves_the_ledger_csv() {
    let env = Env::new();
    let u = env.user("alex");
    kraken_env();
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        z.start_file("ledgers.csv", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"\"txid\",\"refid\",\"time\"\n").unwrap();
        z.finish().unwrap();
    }
    fake(vec![
        (
            "AddExport",
            j(serde_json::json!({"error": [], "result": {"id": "R1"}})),
        ),
        (
            "ExportStatus",
            j(serde_json::json!({"error": [], "result": [{"id": "R1", "status": "Processed"}]})),
        ),
        ("RetrieveExport", buf.into_inner()),
    ]);
    let name = api::kraken_export_ledgers(&u, 0, today()).unwrap();
    assert!(read(&u.inbox().join(name)).starts_with("\"txid\""));
    http::set_transport(None);
}

#[test]
fn kraken_balances_use_plain_symbols_and_save_prices() {
    let env = Env::new();
    let u = env.user("alex");
    kraken_env();
    let calls = fake(vec![
        (
            "private/Balance",
            j(
                serde_json::json!({"error": [], "result": {"XXBT": "0.5", "XBT.F": "0.1", "ZCAD": "20", "DOT": "0", "ETH2.S": "1"}}),
            ),
        ),
        (
            "pair=XBTCAD",
            j(serde_json::json!({"error": [], "result": {"XXBTZCAD": {"c": ["100000.0", "1"]}}})),
        ),
        (
            "pair=ETHCAD",
            j(serde_json::json!({"error": ["EQuery:Unknown asset pair"], "result": {}})),
        ),
        (
            "pair=ETHUSD",
            j(serde_json::json!({"error": [], "result": {"XETHZUSD": {"c": ["3000.0", "1"]}}})),
        ),
    ]);
    let mut bal = api::kraken_balances(&u, today()).unwrap();
    bal.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        bal.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["BTC", "CAD", "ETH"]
    );
    assert!(app::approx(bal[0].1, 0.6) && bal[1].1 == 20.0 && bal[2].1 == 1.0);
    let saved = &saved_balances(&u)["kraken"]["prices"];
    assert_eq!(
        *saved,
        serde_json::json!({"BTC": {"price": 100000.0, "currency": "CAD"}, "ETH": {"price": 3000.0, "currency": "USD"}})
    );
    let calls = calls.borrow();
    let public: Vec<_> = calls
        .iter()
        .filter(|c| c.1.contains("public/Ticker"))
        .collect();
    assert!(
        !public.is_empty() && public.iter().all(|c| c.2.is_empty()),
        "the ticker gets no key or token"
    );
    http::set_transport(None);
}

#[test]
fn kraken_docs_name_the_variables_the_client_reads() {
    for doc in ["connectors/kraken.toml", "connectors/playbooks/kraken.md"] {
        let text = read(&repo().join(doc));
        assert!(
            text.contains("CASHCAST_KRAKEN_KEY")
                && text.contains("CASHCAST_KRAKEN_SECRET")
                && !text.contains("_API_"),
            "{doc}"
        );
    }
}

#[test]
fn secrets_come_from_the_private_env_file() {
    let env = Env::new();
    let u = env.user("alex");
    app::unset("CASHCAST_PAYPAL_SECRET");
    let e = http::secret(&u, "CASHCAST_PAYPAL_SECRET").unwrap_err();
    assert!(e.message.contains("CASHCAST_PAYPAL_SECRET"));
    std::fs::write(u.home.join(".env"), "CASHCAST_PAYPAL_SECRET='abc'\n").unwrap();
    assert_eq!(http::secret(&u, "CASHCAST_PAYPAL_SECRET").unwrap(), "abc");
}

#[test]
fn cloud_sync_needs_consent() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_GOOGLE_TOKEN", "t");
    let row = |v: &[i64]| Json::List(v.iter().map(|x| Json::Int(*x)).collect());
    let e = api::sheets_write(&u, "sheet", "A1", vec![row(&[1])], today()).unwrap_err();
    assert_eq!(e.kind, "consenterror", "{}", e.message);
    grant_by_hand(&u, &["cloud:sheets"]);
    let calls = fake(vec![(
        "/values/",
        j(serde_json::json!({"updatedCells": 2})),
    )]);
    assert_eq!(
        api::sheets_write(&u, "sheet", "A1", vec![row(&[1, 2])], today()).unwrap(),
        2
    );
    let body: serde_json::Value =
        serde_json::from_str(calls.borrow()[0].3.as_deref().unwrap()).unwrap();
    assert_eq!(body["values"], serde_json::json!([[1, 2]]));
    http::set_transport(None);
    app::unset("CASHCAST_GOOGLE_TOKEN");
}

#[test]
fn market_data_sends_only_the_symbol() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_ALPHAVANTAGE_KEY", "demo");
    assert_eq!(
        api::market(&u, "MSFT", today()).unwrap_err().kind,
        "consenterror"
    );
    grant_by_hand(&u, &["net:market-data"]);
    let calls = fake(vec![
        (
            "GLOBAL_QUOTE",
            j(
                serde_json::json!({"Global Quote": {"05. price": "400.5", "07. latest trading day": "2026-10-16"}}),
            ),
        ),
        (
            "OVERVIEW",
            j(
                serde_json::json!({"AssetType": "Common Stock", "PERatio": "30.1", "ProfitMargin": "0.35"}),
            ),
        ),
    ]);
    let m = api::market(&u, "MSFT", today()).unwrap();
    assert_eq!(m.get("price").map(Json::py_str).as_deref(), Some("400.5"));
    assert_eq!(m.get("pe").map(Json::py_str).as_deref(), Some("30.1"));
    assert_eq!(api::market(&u, "MSFT", today()).unwrap(), m);
    assert_eq!(calls.borrow().len(), 2, "cached for the day");
    assert!(calls.borrow().iter().all(|c| c.3.is_none()));
    // av_symbol("XEQT.TO") == "XEQT.TRT"
    let calls = fake(vec![(
        "GLOBAL_QUOTE",
        j(serde_json::json!({"Global Quote": {"05. price": "30"}})),
    )]);
    api::market(&u, "XEQT.TO", today()).unwrap();
    assert!(calls.borrow()[0].1.contains("symbol=XEQT.TRT"));
    http::set_transport(None);
    app::unset("CASHCAST_ALPHAVANTAGE_KEY");
}

/// Like `fake`, with a status per route.
fn fake_status(routes: Vec<(&'static str, u16, Vec<u8>)>) -> Calls {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    http::set_transport(Some(Box::new(move |method, url, headers, body| {
        seen.borrow_mut().push((
            method.to_string(),
            url.to_string(),
            headers.to_vec(),
            body.map(|b| String::from_utf8_lossy(b).into_owned()),
        ));
        for (key, status, data) in &routes {
            if url.contains(key) {
                return Ok(Response {
                    status: *status,
                    body: data.clone(),
                });
            }
        }
        panic!("unexpected call {url}");
    })));
    calls
}

fn inbox_names(u: &cashcast_engine::core::paths::UserHome) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(u.inbox())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn wise_routes(body: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "/v2/profiles",
            j(serde_json::json!([{"id": 7, "type": "PERSONAL"}])),
        ),
        (
            "/balances",
            j(serde_json::json!([{"id": 1, "amount": {"currency": "CAD", "value": 1}}])),
        ),
        ("statement.csv", body.to_vec()),
    ]
}

#[test]
fn wise_statements_keep_one_account_name_per_currency() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_WISE_TOKEN", "test-token");
    fake(wise_routes(
        b"TransferWise ID,Date,Amount\nCANARY-2,01-09-2026,1.00\n",
    ));
    let first = api::wise_statements(&u, 365, app::d("2026-11-01")).unwrap();
    let second = api::wise_statements(&u, 365, app::d("2026-11-03")).unwrap();
    assert_eq!(first, vec!["wise-CAD.csv"]);
    assert_eq!(first, second);
    assert_eq!(inbox_names(&u), vec!["wise-CAD.csv"]);
    http::set_transport(None);
    app::unset("CASHCAST_WISE_TOKEN");
}

/// Wise gives at most 469 days per statement. A longer history (back to
/// `[moneywiz] since`) comes in pieces, oldest first, saved as one file with
/// one header.
#[test]
fn wise_statements_reach_back_in_pieces() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_WISE_TOKEN", "t");
    let calls = fake(wise_routes(
        b"TransferWise ID,Date,Amount\nCANARY-3,01-09-2026,1.00\n",
    ));
    let saved = api::wise_statements(&u, 900, app::d("2026-11-16")).unwrap();
    let asks: Vec<String> = calls
        .borrow()
        .iter()
        .filter(|c| c.1.contains("statement.csv"))
        .map(|c| c.1.clone())
        .collect();
    assert_eq!(asks.len(), 3, "{asks:?}");
    assert!(asks[0].contains("intervalStart=2024-05-30"), "{}", asks[0]);
    assert!(asks[2].contains("intervalEnd=2026-11-16"), "{}", asks[2]);
    let body = read(&u.inbox().join(&saved[0]));
    assert_eq!(body.matches("TransferWise ID").count(), 1, "{body}");
    assert_eq!(body.matches("CANARY-3").count(), 3, "{body}");
    http::set_transport(None);
    app::unset("CASHCAST_WISE_TOKEN");
}

/// `api download wise` reaches back to `[moneywiz] since` when it is older
/// than a year.
#[test]
fn wise_download_reaches_back_to_the_moneywiz_start_day() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_WISE_TOKEN", "t");
    std::fs::write(u.config(), "[moneywiz]\nsince = \"2023-09-30\"\n").unwrap();
    let calls = fake(wise_routes(b"TransferWise ID,Date,Amount\n"));
    api::download(&u, "wise", app::d("2026-11-16")).unwrap();
    let first = calls
        .borrow()
        .iter()
        .find(|c| c.1.contains("statement.csv"))
        .map(|c| c.1.clone())
        .unwrap();
    assert!(first.contains("intervalStart=2023-09-30"), "{first}");
    http::set_transport(None);
    app::unset("CASHCAST_WISE_TOKEN");
}

#[test]
fn wise_statements_remove_the_old_dated_files() {
    let env = Env::new();
    let u = env.user("alex");
    set("CASHCAST_WISE_TOKEN", "t");
    std::fs::create_dir_all(u.inbox()).unwrap();
    std::fs::write(u.inbox().join("wise-CAD-2026-11-01.csv"), "old").unwrap();
    std::fs::write(u.inbox().join("wise-CADX-notes.csv"), "kept").unwrap();
    fake(wise_routes(b"x"));
    api::wise_statements(&u, 365, today()).unwrap();
    assert_eq!(inbox_names(&u), vec!["wise-CAD.csv", "wise-CADX-notes.csv"]);
    http::set_transport(None);
    app::unset("CASHCAST_WISE_TOKEN");
}

fn etrade_env() {
    for k in ["KEY", "SECRET", "TOKEN", "TOKEN_SECRET"] {
        set(&format!("CASHCAST_ETRADE_{k}"), "x");
    }
}

fn etrade_unset() {
    for k in ["KEY", "SECRET", "TOKEN", "TOKEN_SECRET"] {
        app::unset(&format!("CASHCAST_ETRADE_{k}"));
    }
}

#[test]
fn etrade_transactions_saves_each_page_unparsed() {
    let env = Env::new();
    let u = env.user("alex");
    etrade_env();
    let page2 = serde_json::json!({"TransactionListResponse": {"moreTransactions": false, "Transaction": [{"transactionId": "CANARY-ET2"}]}});
    let page1 = serde_json::json!({"TransactionListResponse": {"moreTransactions": true, "marker": "M1", "Transaction": [{"transactionId": "CANARY-ET1"}]}});
    let list = serde_json::json!({"AccountListResponse": {"Accounts": {"Account": [{"accountIdKey": "K1"}]}}});
    let calls = fake(vec![
        ("/list.json", j(list.clone())),
        ("marker=M1", j(page2.clone())),
        ("/transactions.json", j(page1)),
    ]);
    let saved = api::etrade_transactions(&u, 730, app::d("2026-11-03")).unwrap();
    let pages: Vec<&str> = saved
        .iter()
        .map(|s| s.rsplit_once('-').unwrap().1)
        .collect();
    assert_eq!(pages, vec!["1.json", "2.json"]);
    assert_eq!(
        saved[0].rsplit_once('-').unwrap().0,
        saved[1].rsplit_once('-').unwrap().0
    );
    let got: serde_json::Value = serde_json::from_str(&read(&u.inbox().join(&saved[1]))).unwrap();
    assert_eq!(got, page2);
    let first = calls
        .borrow()
        .iter()
        .find(|c| c.1.contains("/transactions.json"))
        .unwrap()
        .1
        .clone();
    assert!(
        first.contains("startDate=11032024") && first.contains("endDate=11032026"),
        "{first}"
    );
    // A shorter download later removes the stale page.
    fake(vec![
        ("/list.json", j(list)),
        ("/transactions.json", j(page2)),
    ]);
    let again = api::etrade_transactions(&u, 730, app::d("2026-11-04")).unwrap();
    assert_eq!(again, vec![saved[0].clone()]);
    assert_eq!(inbox_names(&u), again);
    http::set_transport(None);
    etrade_unset();
}

#[test]
fn etrade_transactions_skip_an_empty_page_and_a_repeated_marker() {
    let env = Env::new();
    let u = env.user("alex");
    etrade_env();
    let looping = serde_json::json!({"TransactionListResponse": {"moreTransactions": true, "marker": "M1", "Transaction": []}});
    fake_status(vec![
        (
            "/list.json",
            200,
            j(
                serde_json::json!({"AccountListResponse": {"Accounts": {"Account": [{"accountIdKey": "QUIET"}, {"accountIdKey": "LOOP"}]}}}),
            ),
        ),
        ("QUIET/transactions.json", 204, Vec::new()),
        ("LOOP/transactions.json", 200, j(looping)),
    ]);
    let saved = api::etrade_transactions(&u, 730, app::d("2026-11-03")).unwrap();
    // The quiet account saves nothing; the looping marker stops after one repeat.
    assert_eq!(saved.len(), 2);
    assert!(
        saved
            .iter()
            .all(|s| s.ends_with("-1.json") || s.ends_with("-2.json"))
    );
    http::set_transport(None);
    etrade_unset();
}
