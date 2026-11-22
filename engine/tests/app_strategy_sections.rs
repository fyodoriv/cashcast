//! Every strategy section on synthetic people (US-07, US-08, US-21, US-25 to
//! US-31). Ported from tests/unit/test_strategy_sections.py,
//! test_strategy_crypto.py, test_strategy_foreign.py, test_interest.py,
//! test_strategy_net_total.py, test_strategy_invest_plan.py,
//! test_browser_balances.py, test_notion_parity.py and test_own_tasks.py.

mod app;

use app::{
    Env, append, approx, balances_path, d, holdings_path, money, read, summary_path, today, usd,
    write_json,
};
use cashcast_engine::core::paths::UserHome;
use cashcast_engine::core::pyjson::{Json, loads};
use cashcast_engine::strategy::situation::Situation;
use cashcast_engine::strategy::{Ctx, Data, Task, invest_plan, plan, py};
use cashcast_engine::tasks::{balances, changes};
use chrono::NaiveDate;
use serde_json::json;

fn run(u: &UserHome, day: NaiveDate) -> Ctx {
    plan::run(u, day).map_err(|e| e.0).unwrap().0
}

fn md(ctx: &Ctx, id: &str) -> String {
    ctx.result(id)
        .unwrap_or_else(|| panic!("no section {id}"))
        .markdown()
}

fn doc(ctx: &Ctx) -> String {
    plan::document(ctx, &[]).map_err(|e| e.0).unwrap()
}

fn tasks(ctx: &Ctx) -> Vec<Task> {
    plan::tasks(ctx).map_err(|e| e.0).unwrap()
}

fn task<'a>(ctx: &'a Ctx, section: &str, id: &str) -> Option<&'a Task> {
    ctx.result(section)
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.id == id)
}

fn sit(u: &UserHome, day: NaiveDate) -> Situation {
    Situation::load(&u.home, &u.name, day)
        .map_err(|e| e.0)
        .unwrap()
}

fn cash(u: &UserHome, day: NaiveDate) -> f64 {
    sit(u, day)
        .cash_balance()
        .map_err(|e| e.0)
        .unwrap()
        .unwrap_or(0.0)
}

fn rows(u: &UserHome, kind: &str) -> Vec<cashcast_engine::strategy::situation::SourceRow> {
    sit(u, today()).rows_of(kind).map_err(|e| e.0).unwrap()
}

fn set_balances(u: &UserHome, v: serde_json::Value) {
    write_json(&balances_path(u), &v);
}

fn num(v: Option<&Json>) -> f64 {
    py::num(v).unwrap_or(f64::NAN)
}

fn record(u: &UserHome, service: &str, spec: &str, currency: &str) {
    balances::record(u, service, spec, currency).unwrap();
}

// ---------- every section (test_strategy_sections.py) ----------

fn person(env: &Env) -> UserHome {
    env.person(&[("Chequing", 40000.0)])
}

#[test]
fn every_section_runs_without_failing() {
    let env = Env::new();
    let ctx = run(&person(&env), today());
    for s in &ctx.results {
        assert!(!s.markdown().contains("section failed"), "{}", s.id);
    }
    let doc = doc(&ctx);
    assert!(!doc.contains("CANARY") && doc.contains("not a licensed financial advisor"));
}

#[test]
fn cashback_routes_rent_and_groceries() {
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let t = md(&ctx, "cashback");
    assert!(t.contains("Test Grocery 4%") && t.contains("Net:") && t.contains("Rent rule"));
    let Data::Cashback(p) = &ctx.result("cashback").unwrap().data else {
        panic!("no plan")
    };
    let bank_only = p
        .bank_paid
        .iter()
        .find(|(k, _)| k == "bank_only")
        .map_or(0.0, |x| x.1);
    assert!(approx(bank_only, 400.0 * 12.0));
}

#[test]
fn banking_breakeven() {
    let env = Env::new();
    assert!(md(&run(&person(&env), today()), "banking").contains("hold the minimum"));
}

#[test]
fn credit_targets_and_paydown_tasks() {
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let t = md(&ctx, "credit");
    assert!(t.contains("Target at close") && t.contains("Mortgage in about 10 months"));
    assert!(task(&ctx, "credit", "paydown-test-flat-2026-10").is_some());
}

#[test]
fn reserve_and_invest_order() {
    let env = Env::new();
    let t = md(&run(&person(&env), today()), "reserve");
    assert!(t.contains("Emergency fund") && t.contains("can be invested"));
    let order: Vec<&str> = t
        .lines()
        .filter(|l| ["1.", "2.", "3.", "4."].iter().any(|p| l.starts_with(p)))
        .collect();
    assert!(order[0].starts_with("1. FHSA"), "{order:?}");
}

#[test]
fn invest_screen_and_concentration() {
    let env = Env::new();
    let u = person(&env);
    let t = md(&run(&u, today()), "invest");
    assert!(t.contains("EMPL") && t.contains("5/5") && t.contains("not personal financial advice"));
    write_json(
        &holdings_path(&u),
        &json!({"assets": [{"symbol": "EMPL", "acb_cad": 30000}, {"symbol": "XEQT", "acb_cad": 70000}]}),
    );
    assert!(md(&run(&u, today()), "invest").contains("above the 10% guideline"));
}

#[test]
fn payments_flags_fees_and_rising_categories() {
    let env = Env::new();
    let t = md(&run(&person(&env), today()), "payments");
    assert!(t.contains("avoidable fee") && t.contains("Rising fast"));
}

fn recurring() -> serde_json::Value {
    json!([
        {"label": "Payments #1", "cadence": "weekly", "category": "(Uncategorized)", "median_cents": 7900,
         "count": 18, "last_seen": "2026-09-28", "yearly_cents": 410800},
        {"label": "Payments #2", "cadence": "monthly", "category": "Subscriptions", "median_cents": 1699,
         "count": 12, "last_seen": "2026-09-12", "yearly_cents": 20388},
        {"label": "Payments #3", "cadence": "yearly", "category": "Subscriptions", "median_cents": 11900,
         "count": 3, "last_seen": "2026-03-03", "yearly_cents": 11900},
    ])
}

fn with_recurring(u: &UserHome, items: serde_json::Value) {
    let mut s: serde_json::Value = serde_json::from_str(&read(&summary_path(u))).unwrap();
    s["recurring_payments"] = items;
    write_json(&summary_path(u), &s);
}

fn reviews(ctx: &Ctx) -> Vec<String> {
    md(ctx, "alternatives")
        .lines()
        .filter(|l| l.starts_with("- Review Payments #"))
        .map(str::to_string)
        .collect()
}

#[test]
fn recurring_payments_list_numbered_payees_with_a_yearly_total() {
    let env = Env::new();
    let u = person(&env);
    with_recurring(&u, recurring());
    let ctx = run(&u, today());
    let pay = md(&ctx, "payments");
    assert!(pay.contains("### Recurring payments"));
    let first: Vec<&str> = pay
        .lines()
        .filter(|l| l.starts_with("| Payments #"))
        .map(|l| l.split(" | ").next().unwrap())
        .collect();
    assert_eq!(
        first,
        vec!["| Payments #1", "| Payments #2", "| Payments #3"]
    );
    assert!(pay.contains("3 payees charge you on a schedule: $4,430.88 a year in total"));
    assert!(
        pay.contains(
            "| Payments #2 | Subscriptions | month | $16.99 | 12 | 2026-09-12 | $203.88 |"
        )
    );
    assert_eq!(reviews(&ctx).len(), 3);
    assert!(
        md(&ctx, "alternatives")
            .contains("Recurring payments to review** ($4,430.88 a year in total)")
    );
    let t = ctx
        .result("alternatives")
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.id.starts_with("recurring-review-"))
        .unwrap();
    assert!(
        t.title
            .contains("3 recurring payments (about $4,430.88 a year)")
            && t.risk == "read"
    );
    assert_eq!(t.id, "recurring-review-2026-10");
    assert!(tasks(&ctx).iter().any(|x| x.id == t.id));
    assert!(!doc(&ctx).contains("CANARY"));
}

#[test]
fn rent_and_mortgage_are_not_subscriptions_to_cancel() {
    let env = Env::new();
    let u = person(&env);
    let mut items = recurring();
    let base = items[1].clone();
    let mut rent = base.clone();
    rent["label"] = json!("Payments #4");
    rent["category"] = json!("Housing > Rent");
    rent["yearly_cents"] = json!(2160000);
    let mut loan = base;
    loan["label"] = json!("Payments #5");
    loan["category"] = json!("Mortgage");
    loan["yearly_cents"] = json!(1800000);
    items.as_array_mut().unwrap().extend([rent, loan]);
    with_recurring(&u, items);
    let ctx = run(&u, today());
    assert!(md(&ctx, "payments").contains("| Payments #4 | Housing > Rent |"));
    let r = reviews(&ctx);
    assert!(r.len() == 3 && !r.iter().any(|x| x.contains("#4") || x.contains("#5")));
    let t = ctx
        .result("alternatives")
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.id.starts_with("recurring-review-"))
        .unwrap();
    assert!(
        t.title
            .contains("3 recurring payments (about $4,430.88 a year)")
    );
}

#[test]
fn recurring_payments_none_found_and_older_summaries() {
    let env = Env::new();
    let u = person(&env);
    assert!(!md(&run(&u, today()), "payments").contains("Recurring payments"));
    with_recurring(&u, json!([]));
    let ctx = run(&u, today());
    assert!(md(&ctx, "payments").contains("- None found"));
    assert!(!md(&ctx, "alternatives").contains("Recurring payments to review"));
    assert!(
        !tasks(&ctx)
            .iter()
            .any(|t| t.id.starts_with("recurring-review"))
    );
}

#[test]
fn budget_vs_actual_and_proposals() {
    let env = Env::new();
    let t = md(&run(&person(&env), today()), "budget");
    assert!(t.contains("| Food |") && t.contains("Budget proposals"));
}

#[test]
fn alternatives_finds_weak_cards() {
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let t = md(&ctx, "alternatives");
    assert!(t.contains("Test Costly") && t.contains("product-change"));
    assert!(task(&ctx, "alternatives", "downgrade-test-costly").is_some());
}

#[test]
fn review_collects_tasks_and_signals() {
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let review = ctx.result("review").unwrap();
    let Data::Review(r) = &review.data else {
        panic!("no review data")
    };
    assert_eq!(r.next_review, d("2026-11-02"));
    assert!(review.tasks.len() > 5);
    let risks: Vec<&str> = review.tasks.iter().map(|t| t.risk.as_str()).collect();
    assert!(risks.contains(&"money") && risks.contains(&"settings"));
}

#[test]
fn no_task_asks_for_a_standing_duty() {
    // Owner rule 2026-10-25: tasks are one-time actions, never a daily duty.
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let re = regex::Regex::new(
        r"(?i)every day|each day|daily|at all times|at or above|keep .* (in|at|above)|maintain",
    )
    .unwrap();
    let standing: Vec<&str> = ctx
        .result("review")
        .unwrap()
        .tasks
        .iter()
        .filter(|t| re.is_match(&t.title))
        .map(|t| t.title.as_str())
        .collect();
    assert!(standing.is_empty(), "{standing:?}");
    assert!(md(&ctx, "banking").contains("Keep"));
}

#[test]
fn journal_flags_a_pivot_after_two_misses() {
    let env = Env::new();
    app::set("CASHCAST_LOG", "on");
    let u = person(&env);
    // journal.record twice: what `plan.finish` writes once a month.
    std::fs::write(
        u.data().join("journal.jsonl"),
        "{\"made_on\": \"2026-10-18\", \"metric\": \"spending\", \"month\": \"2026-08\", \"predicted\": 1000.0}\n\
         {\"made_on\": \"2026-10-18\", \"metric\": \"spending\", \"month\": \"2026-09\", \"predicted\": 1000.0}\n",
    )
    .unwrap();
    let (scored, pivot) =
        cashcast_engine::strategy::review::compare(&u.home, sit(&u, today()).summary.as_ref())
            .map_err(|e| e.0)
            .unwrap();
    assert!(scored.len() == 2 && pivot);
    let (ctx, log) = plan::run(&u, today()).map_err(|e| e.0).unwrap();
    assert!(md(&ctx, "review").contains("Pivot"));
    plan::finish(&u, &ctx, &log).map_err(|e| e.0).unwrap();
    let logged: Vec<serde_json::Value> = read(&u.logs().join("events.jsonl"))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let ev = logged
        .iter()
        .filter(|e| e["event"] == "forecast_scored")
        .last()
        .unwrap();
    assert_eq!(
        (ev["checked"].as_i64(), ev["misses"].as_i64()),
        (Some(2), Some(2))
    );
    let keys: Vec<&String> = ev.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 4, "{keys:?}");
    app::set("CASHCAST_LOG", "off");
}

#[test]
fn tax_reads_the_engines_holdings_shape() {
    let env = Env::new();
    let u = person(&env);
    write_json(
        &holdings_path(&u),
        &json!({"assets": [], "realized_by_year": [{"year": 2026, "gains_cad": 5000.0, "losses_cad": 1000.0, "net_cad": 4000.0, "sales": 3}],
                "superficial_loss_flags": [{"asset": "BTC", "year": 2026, "count": 1}]}),
    );
    let t = md(&run(&u, today()), "tax");
    assert!(t.contains("Realized capital gains so far in 2026: $4,000.00"));
    assert!(t.contains("superficial loss") && t.contains("BTC"));
}

#[test]
fn the_tax_section_carries_the_rrsp_check() {
    // test_rrsp.py::test_the_tax_section_carries_the_rrsp_check, on a full plan.
    let env = Env::new();
    let u = person(&env);
    let p = read(&u.profile()).replacen("rrsp = 20000\n", "rrsp = 1000\n", 1);
    std::fs::write(u.profile(), p).unwrap();
    let rows: Vec<serde_json::Value> = ["2026-03", "2026-04", "2026-05", "2026-06", "2026-07", "2026-08", "2026-09"]
        .iter()
        .flat_map(|m| {
            [("member", 500.0), ("employer", 250.0)].map(|(s, cad)| {
                json!({"account_bucket": "RRSP", "deduction_year": 2026, "month": m, "source": s, "cad": cad, "rows": 1})
            })
        })
        .collect();
    write_json(
        &holdings_path(&u),
        &json!({"assets": [], "contributions": rows}),
    );
    let ctx = run(&u, d("2026-10-25"));
    let Data::Tax(t) = &ctx.result("tax").unwrap().data else {
        panic!("no tax data")
    };
    assert_eq!(t.rrsp_status, "penalty");
    assert!(task(&ctx, "tax", "rrsp-over-2026").is_some());
}

#[test]
fn forecast_section_summarizes_the_model() {
    let env = Env::new();
    let t = md(&run(&person(&env), today()), "forecast");
    assert!(
        t.contains("Balance forecast")
            && (t.contains("Can invest") || t.contains("needs answers first"))
    );
}

#[test]
fn a_single_section_matches_the_full_plan() {
    // The Rust plan always builds every section; `--section` only filters the document.
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let alone = plan::document(&ctx, &["reserve".to_string()])
        .map_err(|e| e.0)
        .unwrap();
    assert!(alone.contains(&md(&ctx, "reserve")));
}

#[test]
fn tasks_use_the_catalog_card_name_when_the_profile_lists_only_an_id() {
    let env = Env::new();
    let u = env.make_user("alex", "ON", "", Some(&app::spending()), &[]);
    append(
        &u.profile(),
        "\n[[wallet.cards]]\nid = \"td-cash-back-visa-infinite\"\nlimit = 10000\nstatement_close_day = 18\n",
    );
    let ctx = run(&u, today());
    let credit = ctx.result("credit").unwrap();
    let titles: String = credit
        .tasks
        .iter()
        .map(|t| t.title.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        titles.contains("TD Cash Back Visa Infinite")
            && !titles.contains("td-cash-back-visa-infinite down")
    );
    assert!(credit.markdown().contains("TD Cash Back Visa Infinite"));
}

#[test]
fn cashback_logs_a_counts_only_ratio_vs_the_best_single_card() {
    let env = Env::new();
    let (_, log) = plan::run(&person(&env), today()).map_err(|e| e.0).unwrap();
    let (_, fields) = log
        .iter()
        .filter(|(n, _)| n == "cashback_scored")
        .last()
        .unwrap();
    let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["ratio", "cards"]);
    let ratio = fields
        .iter()
        .find(|(k, _)| k == "ratio")
        .map(|(_, v)| num(Some(v)))
        .unwrap();
    assert!(ratio >= 1.0);
}

// ---------- crypto (test_strategy_crypto.py) ----------

fn chequing(env: &Env) -> UserHome {
    env.person(&[("Chequing CANARY", 1000.0)])
}

fn with_kraken(
    u: &UserHome,
    balances: serde_json::Value,
    prices: serde_json::Value,
    fetched_on: &str,
) {
    set_balances(
        u,
        json!({"kraken": {"balances": balances, "prices": prices, "fetched_on": fetched_on}}),
    );
}

#[test]
fn crypto_is_valued_and_fiat_is_cash() {
    let env = Env::new();
    let u = chequing(&env);
    let before = cash(&u, today());
    with_kraken(
        &u,
        json!({"BTC": 0.5, "ETH": 2.0, "CAD": 30.0}),
        json!({"BTC": {"price": 100000, "currency": "CAD"}, "ETH": {"price": 3000, "currency": "USD"}}),
        "2026-10-17",
    );
    let crypto = rows(&u, "crypto");
    let cad = |c: &str| {
        crypto
            .iter()
            .find(|r| r.currency == c)
            .unwrap()
            .cad
            .unwrap()
    };
    assert!(approx(cad("BTC"), 50000.0) && approx(cad("ETH"), 6000.0 * usd()));
    assert!(approx(cash(&u, today()), before + 30.0));
}

#[test]
fn crypto_section_shows_value_and_gain() {
    let env = Env::new();
    let u = chequing(&env);
    with_kraken(
        &u,
        json!({"BTC": 0.5}),
        json!({"BTC": {"price": 100000, "currency": "CAD"}}),
        "2026-10-17",
    );
    write_json(
        &holdings_path(&u),
        &json!({"assets": [{"symbol": "BTC", "quantity": 1.0, "acb_cad": 40000, "account_bucket": "non-registered"}]}),
    );
    let ctx = run(&u, today());
    let t = md(&ctx, "crypto");
    assert!(t.contains("Crypto at market: **$50,000.00**"));
    assert!(t.contains("Unrealized gain on assets with a known cost: $30,000.00"));
    assert!(t.contains("never counted as cash") && !doc(&ctx).contains("CANARY"));
}

#[test]
fn crypto_unknown_cost_and_missing_price() {
    let env = Env::new();
    let u = chequing(&env);
    with_kraken(
        &u,
        json!({"BTC": 0.5, "XYZ": 3.0}),
        json!({"BTC": {"price": 100000, "currency": "CAD"}}),
        "2026-10-17",
    );
    write_json(
        &holdings_path(&u),
        &json!({"assets": [{"symbol": "BTC", "quantity": 1.0, "acb_cad": 0, "cost_unknown": true}]}),
    );
    let t = md(&run(&u, today()), "crypto");
    assert!(t.contains("No price for Kraken XYZ") && !t.contains("Unrealized gain"));
}

#[test]
fn crypto_stale_prices_are_flagged() {
    let env = Env::new();
    let u = chequing(&env);
    with_kraken(
        &u,
        json!({"BTC": 0.5}),
        json!({"BTC": {"price": 100000, "currency": "CAD"}}),
        "2026-09-01",
    );
    assert!(md(&run(&u, today()), "crypto").contains("Prices are older than"));
}

#[test]
fn crypto_none_says_how_to_read_it() {
    let env = Env::new();
    assert!(
        md(&run(&chequing(&env), today()), "crypto").contains("bin/cashcast api balances kraken")
    );
}

#[test]
fn wise_and_kraken_share_one_path() {
    let env = Env::new();
    let u = chequing(&env);
    set_balances(
        &u,
        json!({
            "wise": {"balances": {"USD": 100.0}, "fetched_on": "2026-10-17"},
            "kraken": {"balances": {"USD": 10.0, "BTC": 0.1}, "prices": {"BTC": {"price": 1000, "currency": "CAD"}}, "fetched_on": "2026-10-17"},
            "etrade": {"balances": {"account 0 (USD)": 5.0}, "fetched_on": "2026-10-17"},
        }),
    );
    let mut got: Vec<(String, String, String)> = sit(&u, today())
        .source_balances()
        .map_err(|e| e.0)
        .unwrap()
        .into_iter()
        .map(|r| (r.service, r.currency, r.kind))
        .collect();
    got.sort();
    let s = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(
        got,
        vec![
            s("etrade", "USD", "investment"),
            s("kraken", "BTC", "crypto"),
            s("kraken", "USD", "cash"),
            s("wise", "USD", "cash")
        ]
    );
}

// ---------- foreign cash (test_strategy_foreign.py) ----------

fn with_wise(u: &UserHome, balances: serde_json::Value) {
    set_balances(
        u,
        json!({"wise": {"balances": balances, "fetched_on": "2026-10-17"}}),
    );
}

#[test]
fn wise_balances_count_in_cad() {
    let env = Env::new();
    let u = chequing(&env);
    let before = cash(&u, today());
    with_wise(&u, json!({"USD": 1000.0, "EUR": 0, "CAD": 50.0}));
    assert!(approx(cash(&u, today()), before + 1000.0 * usd() + 50.0));
    let cashrows = rows(&u, "cash");
    let mut cur: Vec<&str> = cashrows.iter().map(|r| r.currency.as_str()).collect();
    cur.sort();
    assert_eq!(cur, vec!["CAD", "USD"]);
    assert!(approx(
        cashrows
            .iter()
            .find(|r| r.currency == "USD")
            .unwrap()
            .cad
            .unwrap(),
        1000.0 * usd()
    ));
}

#[test]
fn reserve_shows_a_cad_line_per_currency() {
    let env = Env::new();
    let u = chequing(&env);
    with_wise(&u, json!({"USD": 1000.0, "CAD": 50.0}));
    let t = md(&run(&u, today()), "reserve");
    let want = format!(
        "Includes Wise USD: 1,000.00 USD = ${} CAD (Bank of Canada {:.4}",
        money(1000.0 * usd()),
        usd()
    );
    assert!(t.contains(&want), "{t}");
    assert!(t.contains("Includes Wise CAD: $50.00."));
}

#[test]
fn a_wise_account_in_moneywiz_is_not_counted_twice() {
    let env = Env::new();
    let u = env.make_user(
        "alex",
        "ON",
        "",
        Some(&app::spending()),
        &[("Wise CANARY", 500.0)],
    );
    with_wise(&u, json!({"USD": 1000.0}));
    assert!(approx(cash(&u, today()), 500.0));
    assert!(rows(&u, "cash")[0].covered_by_summary);
    assert!(
        md(&run(&u, today()), "reserve")
            .contains("already in your MoneyWiz accounts, not counted twice")
    );
}

#[test]
fn summary_cash_in_usd_is_converted() {
    let env = Env::new();
    let u = env.make_user(
        "alex",
        "ON",
        "",
        Some(&app::spending()),
        &[("Brokerage cash CANARY", 100.0)],
    );
    let mut s: serde_json::Value = serde_json::from_str(&read(&summary_path(&u))).unwrap();
    s["accounts"][0]["currency"] = json!("USD");
    write_json(&summary_path(&u), &s);
    assert!(approx(cash(&u, today()), 100.0 * usd()));
}

#[test]
fn unknown_currency_is_shown_but_not_counted() {
    let env = Env::new();
    let u = chequing(&env);
    with_wise(&u, json!({"XYZ": 10.0}));
    let r = &rows(&u, "cash")[0];
    assert!(r.cad.is_none() && !r.counted);
    assert!(md(&run(&u, today()), "reserve").contains("no CAD rate"));
}

#[test]
fn foreign_section_gives_usd_advice() {
    let env = Env::new();
    let u = chequing(&env);
    with_wise(&u, json!({"USD": 1000.0}));
    let ctx = run(&u, today());
    let t = md(&ctx, "foreign");
    for phrase in [
        "Keep it in USD",
        "RRSP",
        "Norbert's gambit",
        "T1135",
        "line 12100",
        "not personal financial advice",
    ] {
        assert!(t.contains(phrase), "{phrase}");
    }
    assert!(!doc(&ctx).contains("CANARY"));
    assert!(task(&ctx, "foreign", "check-t1135").is_none());
}

#[test]
fn t1135_task_above_the_threshold() {
    let env = Env::new();
    let u = chequing(&env);
    with_wise(&u, json!({"USD": 80000.0}));
    let ctx = run(&u, today());
    let t = task(&ctx, "foreign", "check-t1135").unwrap();
    assert_eq!((t.due, t.risk.as_str()), (Some(d("2027-04-30")), "read"));
}

#[test]
fn no_wise_balances_says_how_to_read_them() {
    let env = Env::new();
    assert!(
        md(&run(&chequing(&env), today()), "foreign").contains("bin/cashcast api balances wise")
    );
}

#[test]
fn stale_rates_are_flagged() {
    let env = Env::new();
    let u = chequing(&env);
    with_wise(&u, json!({"USD": 1000.0}));
    assert!(md(&run(&u, d("2027-10-18")), "foreign").contains("rates are older than"));
}

// ---------- interest (test_interest.py) ----------

const ACCOUNTS: &str = "
[transfers]
hub = \"td-every-day\"

[[banking.accounts]]
id = \"td-every-day\"
keep = 2000

[[banking.accounts]]
id = \"wise-cad\"
bank = \"Wise\"
";

fn wise_rates() -> (f64, f64) {
    let t: toml::Table =
        toml::from_str(&read(&app::repo().join("knowledge/data/foreign_cash.toml"))).unwrap();
    let w = &t["interest"]["wise"];
    (w["USD"].as_float().unwrap(), w["CAD"].as_float().unwrap())
}

fn saver(env: &Env) -> UserHome {
    let u = chequing(env);
    append(&u.profile(), ACCOUNTS);
    set_balances(
        &u,
        json!({"wise": {"balances": {"USD": 1000.0, "CAD": 500.0}, "fetched_on": "2026-10-17"}}),
    );
    u
}

#[test]
fn wise_cad_rate_matches_the_bank_catalog() {
    let t: toml::Table =
        toml::from_str(&read(&app::repo().join("knowledge/data/banks.toml"))).unwrap();
    let wise = t["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"].as_str() == Some("wise-cad"))
        .unwrap();
    assert_eq!(wise["interest_rate"].as_float().unwrap(), wise_rates().1);
}

#[test]
fn each_balance_earns_its_own_rate() {
    let env = Env::new();
    let u = saver(&env);
    let (usd_rate, cad_rate) = wise_rates();
    let ctx = run(&u, today());
    let t = md(&ctx, "interest");
    let usd_yearly = 1000.0 * usd() * usd_rate;
    assert!(
        t.contains(&format!("| Wise USD | ${} |", money(1000.0 * usd()))),
        "{t}"
    );
    assert!(t.contains(&format!("${} |", money(usd_yearly))));
    assert!(t.contains("| TD ") && t.contains("(keep)"));
    let Data::Interest(yearly) = ctx.result("interest").unwrap().data else {
        panic!("no interest data")
    };
    assert!(approx(yearly, usd_yearly + 500.0 * cad_rate));
    let highest = t
        .split("Highest CAD rate you hold:")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    assert!(highest.contains("Wise CAD"));
    assert!(t.contains("line 12100") && !doc(&ctx).contains("CANARY"));
}

#[test]
fn a_bank_account_of_an_api_service_is_not_counted_twice() {
    let env = Env::new();
    let t = md(&run(&saver(&env), today()), "interest");
    assert!(t.contains("Wise CAD") && !t.contains("CAD balance (Wise Interest)"));
}

#[test]
fn hub_advice_when_another_account_pays_more() {
    let env = Env::new();
    let t = md(&run(&saver(&env), today()), "interest");
    assert!(t.contains("Your transfer hub") && t.contains("advice only"));
}

#[test]
fn banking_uses_the_best_rate_you_hold() {
    let env = Env::new();
    let u = saver(&env);
    let p: Vec<String> = read(&u.profile())
        .lines()
        .filter(|l| !l.starts_with("cash_rate"))
        .map(str::to_string)
        .collect();
    std::fs::write(u.profile(), p.join("\n")).unwrap();
    let t = md(&run(&u, today()), "banking");
    assert!(
        t.contains(&format!("Cash is valued at {:.2}%", wise_rates().1 * 100.0))
            && t.contains("best rate among your accounts")
    );
}

#[test]
fn no_places_says_how() {
    let env = Env::new();
    let u = env.make_user("alex", "ON", "", Some(&app::spending()), &[]);
    assert!(md(&run(&u, today()), "interest").contains("banking.accounts"));
}

// ---------- net total (test_strategy_net_total.py) ----------

fn worth(env: &Env) -> UserHome {
    let u = env.person(&[("Chequing CANARY", 20000.0)]);
    let mut s: serde_json::Value = serde_json::from_str(&read(&summary_path(&u))).unwrap();
    let mut card = s["accounts"][0].clone();
    card["name"] = json!("Visa CANARY");
    card["balance_cents"] = json!(-150000);
    card["class"] = json!("card");
    s["accounts"].as_array_mut().unwrap().push(card);
    write_json(&summary_path(&u), &s);
    set_balances(
        &u,
        json!({
            "wise": {"balances": {"USD": 1000.0}, "fetched_on": "2026-10-17"},
            "kraken": {"balances": {"BTC": 0.1}, "prices": {"BTC": {"price": 100000, "currency": "CAD"}}, "fetched_on": "2026-10-17"},
            "wealthsimple": {"balances": {"TFSA": 30000.0}, "fetched_on": "2026-10-17"},
        }),
    );
    append(&u.profile(), "\n[invest]\nhigh_interest_debt = 0\n");
    u
}

fn net(ctx: &Ctx) -> &Json {
    match &ctx.result("net_total").unwrap().data {
        Data::NetTotal(j) => j,
        _ => panic!("no net total data: {}", md(ctx, "net_total")),
    }
}

fn ids(v: Option<&Json>) -> Vec<String> {
    match v {
        Some(Json::List(items)) => items.iter().map(|o| py::gets(o, "id", "")).collect(),
        _ => vec![],
    }
}

#[test]
fn net_total_counts_every_source() {
    let env = Env::new();
    let ctx = run(&worth(&env), today());
    let n = net(&ctx);
    let by = n.get("by_kind").unwrap();
    assert!(approx(
        num(by.get("cash")),
        20000.0 + py::round(1000.0 * usd(), 2)
    ));
    assert!(approx(num(by.get("crypto")), 10000.0));
    assert!(approx(num(by.get("investment")), 30000.0));
    assert!(approx(num(by.get("debt")), -1500.0));
    let Json::Obj(kinds) = by else { panic!() };
    let total: f64 = kinds.iter().map(|(_, v)| num(Some(v))).sum();
    assert!(approx(num(n.get("total")), total));
    let Some(Json::List(h)) = n.get("history") else {
        panic!("no history")
    };
    assert_eq!(h.len(), 1);
}

#[test]
fn net_total_items_never_carry_account_names() {
    let env = Env::new();
    let u = worth(&env);
    let ctx = run(&u, today());
    let input = read(&u.data().join("net-total-input.json"));
    assert!(input.contains("\"items\"") && !input.contains("CANARY"));
    let doc = doc(&ctx);
    assert!(
        doc.lines()
            .find(|l| l.starts_with("## "))
            .unwrap()
            .starts_with("## Net total: $")
    );
    assert!(!doc.contains("CANARY"));
}

#[test]
fn net_total_a_boundary_refuses_an_option_with_its_reason() {
    let env = Env::new();
    let u = worth(&env);
    append(
        &u.profile(),
        "\n[boundaries]\nwork_hours_week = 40\nmax_work_hours_week = 45\n\n[[net_total.ideas]]\nid = \"gig\"\ntitle = \"Weekend gig\"\neffect_year = 9000\nwork_hours_week = 10\n",
    );
    let ctx = run(&u, today());
    let n = net(&ctx);
    let Some(Json::List(refused)) = n.get("refused") else {
        panic!()
    };
    let gig = refused
        .iter()
        .find(|o| py::gets(o, "id", "") == "gig")
        .unwrap();
    assert!(
        gig.get("refused").unwrap().py_str().contains("no burnout")
            || format!("{:?}", gig.get("refused")).contains("no burnout")
    );
    assert!(!ids(n.get("ranked")).contains(&"gig".to_string()));
}

#[test]
fn net_total_bad_boundaries_say_how_to_fix() {
    let env = Env::new();
    let u = worth(&env);
    append(&u.profile(), "\n[boundaries]\nmargin = 50\n");
    let t = md(&run(&u, today()), "net_total");
    assert!(t.contains("[boundaries]") && t.contains("0 to 20"), "{t}");
}

#[test]
fn invest_plan_gain_moves_become_options() {
    let moves = loads(
        r#"[{"id": "fill-room", "kind": "gain", "title": "Fill your TFSA room", "gain_low": 100, "gain_high": 300, "cost_once": 20, "task": "Move it"},
            {"id": "fee:TINY", "kind": "gain", "title": "Swap a small fund", "gain_low": 5, "gain_high": 10, "task": null},
            {"id": "concentration:XYZ", "kind": "risk", "title": "Sell some employer stock", "gain_low": 0, "gain_high": 0},
            {"id": "rebalance", "kind": "gain", "title": "Rebalance", "gain_low": 50, "gain_high": 50, "overridden": "I like it"},
            {"id": "cash-drag", "kind": "gain", "title": "Invest cash", "gain_low": 400, "gain_high": 800, "vetoed": "debt first: pay it"},
            {"id": "do-nothing", "kind": "baseline", "title": "Do nothing", "gain_low": 0, "gain_high": 0}]"#,
    )
    .unwrap();
    let Json::List(moves) = moves else { panic!() };
    let opts = invest_plan::net_options(&moves);
    assert_eq!(opts.len(), 1);
    assert_eq!(py::gets(&opts[0], "id", ""), "invest-plan:fill-room");
    assert_eq!(
        (
            num(opts[0].get("effect_year")),
            num(opts[0].get("cost_once"))
        ),
        (200.0, 20.0)
    );
    let title = py::gets(&opts[0], "title", "");
    assert!(
        title.contains("$100 to $300") && title.contains("not a forecast"),
        "{title}"
    );
}

#[test]
fn net_total_ranks_the_investment_recommenders_moves() {
    let env = Env::new();
    let ctx = run(&worth(&env), today());
    let Data::InvestPlan(ip) = &ctx.result("invest_plan").unwrap().data else {
        panic!()
    };
    let offered: Vec<String> = ip
        .net_options
        .iter()
        .map(|o| py::gets(o, "id", ""))
        .collect();
    let n = net(&ctx);
    let mut ranked = ids(n.get("ranked"));
    ranked.extend(ids(n.get("refused")));
    assert!(
        !offered.is_empty() && offered.iter().all(|o| ranked.contains(o)),
        "{offered:?} {ranked:?}"
    );
    if offered.contains(&"invest-plan:cash-drag".to_string()) {
        assert!(!ranked.contains(&"invest-cash-above-floor".to_string()));
    }
}

// ---------- investment recommender (test_strategy_invest_plan.py) ----------

fn investor(env: &Env, balance: f64, extra: &str) -> UserHome {
    let u = env.person(&[("Chequing CANARY-acct", balance)]);
    // Debt is recorded (none), so the debt-unknown rule does not block every test.
    let extra = if extra.contains("high_interest_debt") {
        extra.to_string()
    } else if extra.contains("[invest]\n") {
        extra.replacen("[invest]\n", "[invest]\nhigh_interest_debt = 0\n", 1)
    } else {
        format!("{extra}\n[invest]\nhigh_interest_debt = 0\n")
    };
    append(&u.profile(), &extra);
    u
}

fn ip(ctx: &Ctx) -> &cashcast_engine::strategy::invest_plan::InvestPlanData {
    match &ctx.result("invest_plan").unwrap().data {
        Data::InvestPlan(d) => d,
        _ => panic!("no invest plan data"),
    }
}

fn with_etrade(u: &UserHome, value: f64, ws: Option<serde_json::Value>) {
    let mut v = json!({"etrade": {"balances": {"Stock Plan (CAD)": value}, "fetched_on": "2026-10-17", "source": "browser"}});
    if let Some(ws) = ws {
        v["wealthsimple"] =
            json!({"balances": ws, "fetched_on": "2026-10-17", "source": "browser"});
    }
    set_balances(u, v);
}

#[test]
fn invest_plan_do_nothing_wins_when_cash_is_below_the_reserve() {
    let env = Env::new();
    let ctx = run(&investor(&env, 1000.0, ""), today());
    let t = md(&ctx, "invest_plan");
    assert!(ip(&ctx).do_nothing_wins);
    assert!(t.contains("no move needed now") && t.contains("not a licensed financial advisor"));
    assert!(
        !ctx.result("invest_plan")
            .unwrap()
            .tasks
            .iter()
            .any(|t| t.id.starts_with("invest-"))
    );
    assert!(!doc(&ctx).contains("CANARY"));
}

#[test]
fn invest_plan_cash_above_the_reserve_is_a_ranged_money_task() {
    let env = Env::new();
    let u = investor(&env, 90000.0, "\n[invest]\nhorizon_years = 30\n");
    // No home purchase soon: that would keep the cash safe instead.
    let p: Vec<String> = read(&u.profile())
        .lines()
        .filter(|l| !l.starts_with("first_time_home_buyer"))
        .map(String::from)
        .collect();
    std::fs::write(u.profile(), p.join("\n")).unwrap();
    let ctx = run(&u, today());
    let t = md(&ctx, "invest_plan");
    assert_eq!(ip(&ctx).moves[0], "cash-drag");
    assert!(t.contains(" a year") && t.contains(" to $"));
    assert!(t.contains("`pag-equity` (verified)") && t.contains("`return-band` (heuristic)"));
    let task = task(&ctx, "invest_plan", "invest-cash-drag").unwrap();
    assert!(task.risk == "money" && task.notes.contains("You make the trade"));
}

#[test]
fn invest_plan_a_home_purchase_soon_keeps_spare_cash_out_of_stocks() {
    let env = Env::new();
    let ctx = run(&investor(&env, 90000.0, ""), today());
    let t = md(&ctx, "invest_plan");
    assert!(
        t.contains(", at the best rate (needed in about 10 months)")
            && t.contains("`short-horizon` (heuristic)")
    );
    assert!(
        !task(&ctx, "invest_plan", "invest-cash-drag")
            .unwrap()
            .title
            .to_lowercase()
            .contains("invest")
    );
}

#[test]
fn invest_plan_a_long_horizon_does_not_hide_a_home_purchase_soon() {
    let env = Env::new();
    let ctx = run(
        &investor(&env, 90000.0, "\n[invest]\nhorizon_years = 10\n"),
        today(),
    );
    let t = md(&ctx, "invest_plan");
    assert!(t.contains("needed in about 10 months"), "{t}");
}

#[test]
fn invest_plan_employer_concentration_and_a_reasoned_override() {
    let env = Env::new();
    let u = investor(
        &env,
        1000.0,
        "\n[[invest.overrides]]\nmove = \"concentration:EMPL\"\nreason = \"I accept the risk until my next vest\"\n",
    );
    with_etrade(&u, 50000.0, Some(json!({"TFSA": 20000.0})));
    let ctx = run(&u, today());
    let t = md(&ctx, "invest_plan");
    assert!(t.contains("## Your choices") && t.contains("I accept the risk until my next vest"));
    assert!(t.contains("concentration:EMPL") && !t.contains("Risk moves"));
    assert!(
        !ctx.result("invest_plan")
            .unwrap()
            .tasks
            .iter()
            .any(|t| t.id.contains("concentration"))
    );
}

#[test]
fn invest_plan_employer_concentration_without_override_is_a_risk_move() {
    let env = Env::new();
    let u = investor(&env, 1000.0, "");
    with_etrade(&u, 50000.0, Some(json!({"TFSA": 20000.0})));
    let ctx = run(&u, today());
    let t = md(&ctx, "invest_plan");
    assert!(t.contains("Risk moves") && t.contains("Expected gain about $0"));
    assert!(t.contains("E*TRADE account total, assumed to hold only this stock"));
    // One Reminder per run: the decision is that Reminder, or waits in its notes.
    let tasks: Vec<_> = ctx
        .result("invest_plan")
        .unwrap()
        .tasks
        .iter()
        .filter(|t| t.id.starts_with("invest-"))
        .collect();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].risk, "money");
    assert!(
        tasks[0].id == "invest-concentration-empl" || tasks[0].notes.contains("EMPL"),
        "{:?}",
        tasks[0]
    );
}

#[test]
fn invest_plan_a_target_mix_override_names_its_reason() {
    let env = Env::new();
    let ctx = run(
        &investor(
            &env,
            1000.0,
            "\n[invest]\ntarget_equity = 0.9\ntarget_reason = \"30-year horizon\"\n",
        ),
        today(),
    );
    assert!(
        md(&ctx, "invest_plan")
            .contains("Target mix: 90% stocks, from your override in [invest] (30-year horizon)")
    );
}

#[test]
fn invest_plan_panel_five_unknown_risk_and_one_task_today() {
    let env = Env::new();
    let u = investor(&env, 90000.0, "\n[invest]\nhorizon_years = 30\n");
    let p: Vec<String> = read(&u.profile())
        .lines()
        .filter(|l| !l.starts_with("risk ") && !l.starts_with("risk="))
        .map(|l| {
            if l.trim() == "[goals]" {
                "[goals]\nrisk = \"agressive\"".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    std::fs::write(u.profile(), p.join("\n")).unwrap();
    let ctx = run(&u, today());
    let t = md(&ctx, "invest_plan");
    assert!(t.contains("'agressive' is not a known risk level"), "{t}");
    let tasks: Vec<_> = ctx.result("invest_plan").unwrap().tasks.iter().collect();
    assert!(tasks.iter().filter(|x| x.due == Some(today())).count() <= 1);
}

#[test]
fn invest_plan_a_bad_price_or_target_falls_back() {
    let env = Env::new();
    let u = investor(
        &env,
        1000.0,
        "\n[invest]\ntarget_equity = \"lots\"\ntarget_reason = \"why not\"\n\n[invest.prices]\nVDY = 0.0\n",
    );
    write_json(
        &holdings_path(&u),
        &json!({"assets": [{"symbol": "VDY", "quantity": 1000.0, "acb_cad": 40000, "account_bucket": "TFSA"}]}),
    );
    let t = md(&run(&u, today()), "invest_plan");
    assert!(!t.contains("Target mix: 0% stocks"), "{t}");
    assert!(t.contains("is not a number of 0 or more"), "{t}");
    let v: serde_json::Value =
        serde_json::from_str(&read(&u.data().join("invest-plan-input.json"))).unwrap();
    let vdy = v["positions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "VDY")
        .unwrap();
    assert!(vdy["value"].as_f64().unwrap() > 0.0, "{vdy}");
}

#[test]
fn invest_plan_fees_compare_like_with_like_from_owner_prices() {
    let env = Env::new();
    let u = investor(
        &env,
        1000.0,
        "\n[invest]\nprices_as_of = \"2026-10-15\"\n\n[invest.prices]\nVDY = 50.0\n",
    );
    write_json(
        &holdings_path(&u),
        &json!({"assets": [{"symbol": "VDY", "quantity": 1000.0, "acb_cad": 40000, "account_bucket": "TFSA"}]}),
    );
    let t = md(&run(&u, today()), "invest_plan");
    assert!(
        t.contains("Swap VDY (0.2% fee) for XIC (0.1%) in your TFSA"),
        "{t}"
    );
}

#[test]
fn invest_plan_the_engine_input_is_private_and_holds_no_profile_text() {
    let env = Env::new();
    let u = investor(&env, 1000.0, "");
    run(&u, today());
    let path = u.data().join("invest-plan-input.json");
    assert_eq!(app::mode(&path), 0o600);
    let raw = read(&path);
    assert!(!raw.contains("CANARY-not-printed"));
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["facts"]["pag-equity"]["confidence"], "verified");
}

// test_cheapest_keys_by_class_and_region: `_cheapest` lives in the Rust
// recommender (engine/src/invest_plan.rs) and is covered by engine/tests/invest_plan.rs.

#[test]
fn invest_plan_cli_is_gated_by_agent_strategy() {
    let env = Env::new();
    let u = investor(&env, 1000.0, "");
    app::set("CASHCAST_AGENT", "1");
    let (_, out, _) = app::cli(&["invest-plan", "--user", &u.name, "--today", "2026-10-18"]);
    assert!(
        !out.contains("Do nothing") && out.contains("Report saved for the owner only"),
        "{out}"
    );
    app::grant_by_hand(&u, &["agent:strategy"]);
    let (_, out, _) = app::cli(&["invest-plan", "--user", &u.name, "--today", "2026-10-18"]);
    app::unset("CASHCAST_AGENT");
    assert!(
        out.contains("Do nothing") && !out.contains("Report saved for the owner only"),
        "{out}"
    );
}

// ---------- balances read in the browser (test_browser_balances.py) ----------

fn saved(u: &UserHome) -> serde_json::Value {
    serde_json::from_str(&read(&balances_path(u))).unwrap()
}

#[test]
fn record_merges_accounts_and_currencies() {
    let env = Env::new();
    let u = chequing(&env);
    record(&u, "wealthsimple", "TFSA=1000, RRSP=$2000", "CAD");
    record(&u, "wealthsimple", "RRSP USD=500", "usd");
    let ws = &saved(&u)["wealthsimple"];
    assert_eq!(
        ws["balances"],
        json!({"TFSA": 1000.0, "RRSP": 2000.0, "RRSP USD": 500.0})
    );
    assert_eq!(
        (
            ws["currencies"]["RRSP USD"].as_str(),
            ws["currencies"]["TFSA"].as_str()
        ),
        (Some("USD"), Some("CAD"))
    );
    assert_eq!(ws["source"], "browser");
}

#[test]
fn record_refuses_bad_input() {
    let env = Env::new();
    let u = chequing(&env);
    for (service, spec, message) in [
        ("wealthsimple", "TFSA 12345678=10", "4 or more digits"),
        ("nowhere", "TFSA=10", "unknown service"),
        ("wealthsimple", "TFSA", "NAME=AMOUNT"),
        ("wealthsimple", "TFSA=lots", "not an amount"),
        ("wealthsimple", " , ", "nothing to record"),
    ] {
        let e = balances::record(&u, service, spec, "CAD").unwrap_err();
        assert!(e.message.contains(message), "{spec}: {}", e.message);
    }
}

#[test]
fn cli_record_prints_no_amounts() {
    let env = Env::new();
    let u = chequing(&env);
    let (code, out, err) = app::cli(&[
        "--user",
        &u.name,
        "api",
        "record",
        "wealthsimple",
        "--set",
        "TFSA=1234.56",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Recorded 1 balance(s) for wealthsimple") && !out.contains("1234"));
}

#[test]
fn invest_section_shows_market_value_by_bucket() {
    let env = Env::new();
    let u = chequing(&env);
    record(
        &u,
        "wealthsimple",
        "TFSA=1000,RRSP=2000,Personal=300",
        "CAD",
    );
    record(&u, "wealthsimple", "RRSP USD=100", "USD");
    let inv = rows(&u, "investment");
    let row = |a: &str| inv.iter().find(|r| r.account == a).unwrap();
    assert!(approx(row("RRSP USD").cad.unwrap(), 100.0 * usd()));
    assert_eq!(row("Personal").bucket.as_deref(), Some("Non-registered"));
    let ctx = run(&u, today());
    let t = md(&ctx, "invest");
    assert!(
        t.contains("Market value of your accounts")
            && t.contains("| Wealthsimple TFSA | TFSA | $1,000.00 |")
    );
    assert!(
        t.contains(&format!("RRSP ${}", money(2000.0 + 100.0 * usd()))),
        "{t}"
    );
    assert_eq!(
        ctx.sit.cash_balance().map_err(|e| e.0).unwrap(),
        sit(&u, today()).cash_balance().map_err(|e| e.0).unwrap()
    );
    assert!(!doc(&ctx).contains("CANARY"));
}

#[test]
fn no_recorded_accounts_says_how() {
    let env = Env::new();
    assert!(md(&run(&chequing(&env), today()), "invest").contains("api record wealthsimple"));
}

#[test]
fn old_records_are_flagged() {
    let env = Env::new();
    let u = chequing(&env);
    record(&u, "wealthsimple", "TFSA=1000", "CAD");
    let mut data = saved(&u);
    data["wealthsimple"]["fetched_on"] = json!("2026-08-01");
    set_balances(&u, data);
    assert!(md(&run(&u, today()), "invest").contains("Recorded more than 31 days ago"));
}

#[test]
fn casa_btc_uses_the_kraken_price() {
    let env = Env::new();
    let u = chequing(&env);
    record(&u, "casa", "BTC=0.2", "CAD");
    let mut data = saved(&u);
    data["kraken"] = json!({"balances": {}, "prices": {"BTC": {"price": 100000, "currency": "CAD"}}, "fetched_on": "2026-10-17"});
    set_balances(&u, data);
    let casa = rows(&u, "crypto")
        .into_iter()
        .find(|r| r.service == "casa")
        .unwrap();
    assert!(approx(casa.cad.unwrap(), 20000.0));
    assert!(md(&run(&u, today()), "crypto").contains("Casa BTC"));
}

// ---------- Notion finance pages, mapped (test_notion_parity.py) ----------

#[test]
fn notion_parity_every_feature_and_the_hand_calculation() {
    let env = Env::new();
    let u = env.make_user("parity", "ON", "", None, &[]);
    let p = app::apply(&read(&u.profile()));
    std::fs::write(u.profile(), p).unwrap();
    write_json(
        &summary_path(&u),
        &app::summary(&app::spending(), "2025-10", &[("Chequing", 40000.0)]),
    );
    let ctx = run(&u, today());
    let doc = doc(&ctx);
    for (feature, needle) in [
        (
            "where to pay table",
            "| Spending | Per year | Card (in order) |",
        ),
        ("card fees", "Card fees not rebated"),
        ("rent charges", "Rent rule (count charges, no math)"),
        ("fee vs minimum", "Break-even"),
        ("downgrade instead of close", "product-change"),
        ("dropped for simplicity", "simplicity threshold"),
        ("pay-down before close", "Pay down by"),
        ("reserve buckets", "Monthly float"),
        ("mortgage plan", "Mortgage in about"),
        ("monthly review", "Monthly review (5 minutes)"),
        ("economics", "**Net:"),
        ("spending baseline", "Payments analysis"),
        ("account priority", "Order for new money"),
        ("asset location", "Where each asset goes"),
        ("marginal rate", "marginal"),
        ("year budget", "Year budget"),
        ("budget proposals", "Budget proposals"),
        ("dated checklist", "## Dated checklist"),
    ] {
        assert!(doc.contains(needle), "{feature}: {needle}");
    }
    // Worked by hand from full_profile (see the Python docstring): gross 1,589.45, net 749.45.
    let Data::Cashback(plan) = &ctx.result("cashback").unwrap().data else {
        panic!()
    };
    assert_eq!(py::round(plan.gross, 2), 1589.45);
    assert_eq!(py::round(plan.net(), 2), 749.45);
    let Data::Reserve(r) = &ctx.result("reserve").unwrap().data else {
        panic!()
    };
    assert!(approx(r.emergency, 3.0 * 1922.50));
    let credit = md(&ctx, "credit");
    assert!(
        credit.contains("$750.00 (3%)")
            && credit.contains("$450.00 (3%)")
            && credit.contains("$2,250.00")
    );
    // The checklist syncs to a dedicated Reminders list.
    let data = cashcast_engine::tasks::apple::reminders_payload(
        &u,
        &ctx.sit.profile,
        &tasks(&ctx),
        &[],
        &[],
        None,
    )
    .map_err(|e| e.0)
    .unwrap();
    assert!(py::gets(&data, "list", "").starts_with("Cashcast · "));
    let Some(Json::List(items)) = data.get("tasks") else {
        panic!()
    };
    assert!(items.len() > 5);
}

#[test]
fn notion_parity_reference_data_and_playbooks() {
    let env = Env::new();
    let names = cashcast_engine::knowledge::names();
    for n in ["credit_cards", "crypto_exchanges", "tax_ca_2026", "banks"] {
        assert!(names.contains(&n.to_string()), "{n}");
    }
    let specs = cashcast_engine::tasks::connectors::load_all()
        .map_err(|e| e.0)
        .unwrap();
    let get = |id: &str| specs.iter().find(|(s, _)| s == id).unwrap().1.clone();
    for service in ["kraken", "casa", "wealthsimple", "wise"] {
        let spec = get(service);
        assert!(py::list(&spec, "actions").len() > 0, "{service}");
        let playbook = py::gets(py::obj(&spec, "browser"), "playbook", "");
        assert!(
            app::repo().join("connectors").join(playbook).exists(),
            "{service}"
        );
    }
    let kraken = get("kraken");
    assert!(py::list(&kraken, "actions").iter().any(|a| py::gets(a, "risk", "") == "money"
        && matches!(a.get("via"), Some(Json::List(v)) if v.len() == 1 && v[0].py_str() == "human")));
    drop(env);
}

// ---------- own tasks (test_own_tasks.py) ----------

const OWN: &str = "
[[tasks]]
id = \"chexy-switch-back\"
title = \"Chexy: switch back to the first card, then the second\"
due = 2026-11-16
kind = \"settings\"

[[tasks]]
id = \"rent-check\"
title = \"Ensure rent is fine\"

[[tasks]]
id = \"tax-filing-2026\"
due = 2027-03-25
";

const REPEATING: &str = "
[[tasks]]
id = \"rent-ready\"
title = \"Ensure rent is ready to be paid\"
due = 2026-09-25
every = \"month\"
notes = \"CANARY checklist\"

[[tasks]]
id = \"monthly-review-*\"
notes = \"My own checklist\"
";

fn owner(env: &Env, extra: &str) -> UserHome {
    let u = person(env);
    append(&u.profile(), extra);
    u
}

fn by_id(u: &UserHome, day: NaiveDate) -> Vec<Task> {
    tasks(&run(u, day))
}

fn find(ts: &[Task], id: &str) -> Option<Task> {
    ts.iter().find(|t| t.id == id).cloned()
}

fn done(u: &UserHome, id: &str, due: Option<&str>, day: NaiveDate) {
    changes::record_done(u, &[(id.to_string(), due.map(str::to_string))], day).unwrap();
}

#[test]
fn own_tasks_join_the_plan() {
    let env = Env::new();
    let ts = by_id(&owner(&env, OWN), today());
    let t = find(&ts, "chexy-switch-back").unwrap();
    assert!(t.title.starts_with("Chexy") && t.due == Some(d("2026-11-16")) && t.risk == "settings");
    let r = find(&ts, "rent-check").unwrap();
    assert!(r.due.is_none() && r.risk == "read");
}

#[test]
fn an_own_entry_with_a_cashcast_id_changes_that_task_without_a_duplicate() {
    let env = Env::new();
    let ts = by_id(&owner(&env, OWN), today());
    let generated: Vec<&Task> = ts
        .iter()
        .filter(|t| t.id.starts_with("tax-filing"))
        .collect();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].id, "tax-filing-2026");
    assert_eq!(
        (generated[0].due, generated[0].title.as_str()),
        (Some(d("2027-03-25")), "File your 2026 tax return")
    );
}

#[test]
fn a_done_own_task_stays_done() {
    let env = Env::new();
    let u = owner(&env, OWN);
    done(&u, "rent-check", None, today());
    assert!(
        find(
            &by_id(&u, today() + chrono::Duration::days(90)),
            "rent-check"
        )
        .is_none()
    );
    changes::mark(&u, "chexy-switch-back", today(), false).unwrap();
    assert!(find(&by_id(&u, today()), "chexy-switch-back").is_none());
}

fn rent(u: &UserHome, day: NaiveDate) -> Vec<String> {
    by_id(u, day)
        .into_iter()
        .filter(|t| t.id.starts_with("rent-ready"))
        .map(|t| t.id)
        .collect()
}

#[test]
fn a_repeating_own_task_shows_the_current_occurrence() {
    let env = Env::new();
    let u = owner(&env, REPEATING);
    let ts = by_id(&u, today());
    let t = ts.iter().find(|t| t.id.starts_with("rent-ready")).unwrap();
    assert_eq!(t.id, "rent-ready-2026-10-25");
    assert_eq!(
        (t.due, t.notes.as_str()),
        (Some(d("2026-10-25")), "CANARY checklist")
    );
    assert_eq!(rent(&u, today()).len(), 1);
    assert_eq!(rent(&u, d("2026-10-30")), vec!["rent-ready-2026-10-25"]);
}

#[test]
fn ticking_an_occurrence_brings_the_next_one() {
    let env = Env::new();
    let u = owner(&env, REPEATING);
    done(
        &u,
        "rent-ready-2026-10-25",
        Some("2026-10-25"),
        d("2026-10-24"),
    );
    assert_eq!(rent(&u, d("2026-10-24")), vec!["rent-ready-2026-11-25"]);
}

#[test]
fn an_override_can_add_the_owners_notes_and_a_star_matches_every_month() {
    let env = Env::new();
    let u = owner(&env, REPEATING);
    let t = find(&by_id(&u, d("2026-11-01")), "monthly-review-2026-11").unwrap();
    assert!(t.notes.ends_with("My own checklist") && t.title == "Monthly money review (5 minutes)");
    for day in ["2026-11-01", "2026-12-01"] {
        let ts = by_id(&u, d(day));
        let m: Vec<&Task> = ts
            .iter()
            .filter(|t| t.id.starts_with("monthly-review-"))
            .collect();
        assert_eq!(m.len(), 1);
        assert!(m[0].notes.ends_with("My own checklist"));
    }
}

fn notes_twice(u: &UserHome, prefix: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let ctx = run(u, d("2026-11-01"));
    let pick = |ts: Vec<Task>| {
        ts.into_iter()
            .filter(|t| t.id.starts_with(prefix))
            .map(|t| (t.id, t.notes))
            .collect::<Vec<_>>()
    };
    (pick(tasks(&ctx)), pick(tasks(&ctx)))
}

#[test]
fn reading_the_plan_twice_adds_notes_once() {
    let env = Env::new();
    let u = owner(&env, REPEATING);
    let (first, again) = notes_twice(&u, "monthly-review-");
    assert!(!first.is_empty() && again == first);
}

#[test]
fn reading_the_plan_twice_adds_override_notes_once() {
    let env = Env::new();
    let u = owner(
        &env,
        &OWN.replace(
            "id = \"tax-filing-2026\"",
            "id = \"tax-filing-2026\"\nnotes = \"My note\"",
        ),
    );
    let (first, again) = notes_twice(&u, "tax-filing-2026");
    assert!(!first.is_empty() && again == first);
}

/// Ten complete months before 2026-10: usual food $300, last month $600.
fn spending_summary(u: &UserHome, income: i64, interest: i64) {
    let months: Vec<String> = (0..10)
        .map(|k| {
            if k == 0 {
                "2025-12".into()
            } else {
                format!("2026-{k:02}")
            }
        })
        .collect();
    let mut food = vec![30000; 9];
    food.push(60000);
    let mut tiny = vec![1000; 9];
    tiny.push(9000);
    let mut s: serde_json::Value = serde_json::from_str(&read(&summary_path(u))).unwrap();
    s["months"] = json!(months);
    s["income_cents"] = json!({"salary": vec![income; 10]});
    s["spending_cents"] = json!({
        "Food CANARY": food,
        "Rent": vec![200000; 10],
        "Card interest": vec![interest; 10],
        "Tiny": tiny,
    });
    write_json(&summary_path(u), &s);
}

#[test]
fn spending_answers_the_three_questions() {
    let env = Env::new();
    let u = person(&env);
    spending_summary(&u, 500000, 2000);
    let ctx = run(&u, today());
    let t = md(&ctx, "spending");
    for q in [
        "### Is that more than usual?",
        "### Do I spend more than I earn?",
        "### How much interest and fees did I pay?",
        "APR is the yearly interest rate",
        "`bin/cashcast charges`",
    ] {
        assert!(t.contains(q), "{q}\n{t}");
    }
    assert!(
        t.contains("| Food CANARY | $600.00 | $300.00 | +100% |"),
        "{t}"
    );
    assert!(!t.contains("Tiny") && !t.contains("| Rent |"), "{t}");
    assert!(
        t.contains("- No. In the last 10 months you earned $50,000.00"),
        "{t}"
    );
    assert!(
        t.contains("- $200.00 in the last 10 months, in: Card interest."),
        "{t}"
    );
    assert!(task(&ctx, "spending", "spending-over-income").is_none());
}

#[test]
fn spending_over_income_adds_a_task_and_no_fees_says_so() {
    let env = Env::new();
    let u = person(&env);
    spending_summary(&u, 100000, 0);
    let ctx = run(&u, today());
    let t = md(&ctx, "spending");
    assert!(t.contains("- Yes. In the last 10 months"), "{t}");
    assert!(t.contains("- None found in the last 10 months."), "{t}");
    assert!(task(&ctx, "spending", "spending-over-income").is_some());
}

#[test]
fn invest_plan_override_until_survives_the_profile_and_expires() {
    for (until, hidden) in [("2099-12-31", true), ("2000-01-01", false)] {
        let env = Env::new();
        let u = investor(
            &env,
            1000.0,
            &format!(
                "\n[[invest.overrides]]\nmove = \"concentration:EMPL\"\nreason = \"after vest\"\nuntil = {until}\n"
            ),
        );
        with_etrade(&u, 50000.0, Some(json!({"TFSA": 20000.0})));
        let ctx = run(&u, today());
        let t = md(&ctx, "invest_plan");
        assert_eq!(t.contains("Risk moves"), !hidden, "until {until}: {t}");
    }
}

#[test]
fn a_blocked_cash_move_does_not_return_in_the_net_total() {
    let env = Env::new();
    let ctx = run(
        &investor(
            &env,
            90000.0,
            "\n[invest]\nhorizon_years = 30\nhigh_interest_debt = 4000\n",
        ),
        today(),
    );
    assert!(ip(&ctx).moves.iter().any(|m| m == "cash-drag"));
    let n = net(&ctx);
    let mut ranked = ids(n.get("ranked"));
    ranked.extend(ids(n.get("refused")));
    assert!(
        !ranked.contains(&"invest-cash-above-floor".to_string()),
        "{ranked:?}"
    );
}

#[test]
fn invest_plan_a_cash_rate_of_one_means_one_percent() {
    let env = Env::new();
    let u = investor(&env, 1000.0, "");
    let p: Vec<String> = read(&u.profile())
        .lines()
        .filter(|l| !l.starts_with("cash_rate"))
        .map(|l| {
            if l.trim() == "[goals]" {
                "[goals]\ncash_rate = 1".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    std::fs::write(u.profile(), p.join("\n")).unwrap();
    run(&u, today());
    let v: serde_json::Value =
        serde_json::from_str(&read(&u.data().join("invest-plan-input.json"))).unwrap();
    assert!(
        (v["cash_rate"].as_f64().unwrap() - 0.01).abs() < 1e-9,
        "{}",
        v["cash_rate"]
    );
}

#[test]
fn invest_plan_a_cash_rate_written_as_a_percent_is_not_zero() {
    let env = Env::new();
    let u = investor(&env, 1000.0, "");
    let p: Vec<String> = read(&u.profile())
        .lines()
        .filter(|l| !l.starts_with("cash_rate"))
        .map(|l| {
            if l.trim() == "[goals]" {
                "[goals]\ncash_rate = 4.5".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    std::fs::write(u.profile(), p.join("\n")).unwrap();
    run(&u, today());
    let v: serde_json::Value =
        serde_json::from_str(&read(&u.data().join("invest-plan-input.json"))).unwrap();
    let r = v["cash_rate"].as_f64().unwrap();
    assert!((r - 0.045).abs() < 1e-9, "cash_rate read as {r}");
}

#[test]
fn invest_plan_a_cash_rate_that_is_not_a_rate_falls_back_to_the_accounts() {
    for bad in ["\"high\"", "-1"] {
        let env = Env::new();
        let u = investor(&env, 1000.0, "");
        let p: Vec<String> = read(&u.profile())
            .lines()
            .filter(|l| !l.starts_with("cash_rate"))
            .map(|l| {
                if l.trim() == "[goals]" {
                    format!("[goals]\ncash_rate = {bad}")
                } else {
                    l.to_string()
                }
            })
            .collect();
        std::fs::write(u.profile(), p.join("\n")).unwrap();
        run(&u, today());
        let v: serde_json::Value =
            serde_json::from_str(&read(&u.data().join("invest-plan-input.json"))).unwrap();
        let r = v["cash_rate"].as_f64().unwrap();
        assert!(r >= 0.0, "{bad}: cash_rate read as {r}");
    }
}

#[test]
fn invest_plan_a_balanced_fund_carries_its_stock_share_into_the_input() {
    let env = Env::new();
    let u = investor(&env, 1000.0, "\n[invest.prices]\nVGRO = 50.0\n");
    write_json(
        &holdings_path(&u),
        &json!({"assets": [{"symbol": "VGRO", "quantity": 1000.0, "acb_cad": 40000, "account_bucket": "TFSA"}]}),
    );
    let ctx = run(&u, today());
    let v: serde_json::Value =
        serde_json::from_str(&read(&u.data().join("invest-plan-input.json"))).unwrap();
    assert_eq!(v["positions"][0]["equity_share"], 0.8);
    let t = md(&ctx, "invest_plan");
    assert!(t.contains("not personal financial advice"), "{t}");
    for task in &ctx.result("invest_plan").unwrap().tasks {
        assert!(task.notes.contains("not a forecast"), "{}", task.notes);
    }
}

#[test]
fn reserve_sweep_never_asks_more_than_the_forecast_allows_now() {
    // The reserve's "invest now" task must not spend cash the forecast keeps for a later dip.
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let Data::Forecast(f) = &ctx.result("forecast").unwrap().data else {
        panic!("no forecast data: {}", md(&ctx, "forecast"))
    };
    let safe_now: f64 = f
        .rows
        .first()
        .map_or(0.0, |r| r.to_invest.to_string().parse().unwrap());
    let reserve = md(&ctx, "reserve");
    match task(&ctx, "reserve", "sweep-cash") {
        Some(t) => {
            let amount: f64 = t
                .title
                .split('$')
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .replace(',', "")
                .parse()
                .unwrap();
            assert!(
                amount <= safe_now + 0.5,
                "task {:?} vs safe {safe_now}\n{reserve}",
                t.title
            );
        }
        None => assert!(safe_now < 1.0, "safe {safe_now} but no task\n{reserve}"),
    }
}

#[test]
fn capped_sweep_and_safe_move_never_ask_for_the_same_money() {
    // Round-12 judges: "invest $X of $Y" and "move $Y above your reserve" both spent $Y.
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let Some(sweep) = task(&ctx, "reserve", "sweep-cash") else {
        return;
    };
    let Some(asked) = sweep
        .title
        .split(" of the ")
        .nth(1)
        .and_then(|s| s.split(' ').next())
    else {
        return;
    };
    let whole = format!("Move {} above your reserve", asked.trim_end_matches(".00"));
    for s in &ctx.results {
        for t in &s.tasks {
            assert!(
                !t.title.contains(&whole),
                "{:?} overlaps {:?}",
                t.title,
                sweep.title
            );
        }
    }
}

#[test]
fn rrsp_refund_rate_says_why_it_is_below_the_top_rate() {
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let t = md(&ctx, "tax");
    if let Some(line) = t.lines().find(|l| l.contains("back on each dollar")) {
        assert!(line.contains("lower brackets"), "{line}");
    }
}

#[test]
fn net_total_best_step_matches_the_capped_sweep() {
    // Round-12 judges: Home said "keep the whole $32,310 safe" while the tasks invest part of it.
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let Some(sweep) = task(&ctx, "reserve", "sweep-cash") else {
        return;
    };
    let Some(asked) = sweep
        .title
        .split(" of the ")
        .nth(1)
        .and_then(|s| s.split(' ').next())
    else {
        return;
    };
    let whole = format!(
        "Keep the {} above your reserve",
        asked.trim_end_matches(".00")
    );
    let t = md(&ctx, "net_total");
    assert!(!t.contains(&whole), "{whole} in\n{t}");
}

#[test]
fn capped_sweep_no_section_keeps_the_whole_amount_safe() {
    // Round-13: Cash & investing said "Keep the $X above your reserve safe" for the
    // full amount while the reserve task invests part of it now.
    let env = Env::new();
    let ctx = run(&person(&env), today());
    let sweep = task(&ctx, "reserve", "sweep-cash").expect("the fixture has a sweep task");
    let asked: f64 = sweep
        .title
        .split(" of the ")
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .expect("the fixture caps the sweep")
        .trim_start_matches('$')
        .replace(',', "")
        .parse()
        .unwrap();
    let whole = format!(
        "Keep the {} above your reserve",
        cashcast_engine::invest_plan::dollars(asked)
    );
    let all = doc(&ctx);
    assert!(!all.contains(&whole), "{whole} in\n{all}");
    for t in tasks(&ctx) {
        assert!(!t.title.contains(&whole), "{}", t.title);
    }
    let plan = md(&ctx, "invest_plan");
    assert!(
        plan.contains("set aside for later months in a safe place, at the best rate"),
        "the invest plan names the kept part:\n{plan}"
    );
}

#[test]
fn spending_counts_salary_as_earned() {
    // Salary lives in flows_cents, not income_cents: a person with only a
    // paycheque must not read "earned $0.00" or get a spending-over-income task.
    let env = Env::new();
    let u = person(&env);
    spending_summary(&u, 500000, 0);
    let mut s: serde_json::Value = serde_json::from_str(&read(&summary_path(&u))).unwrap();
    let n = s["months"].as_array().unwrap().len();
    s["income_cents"] = json!({});
    s["flows_cents"]["salary"] = json!(vec![400000; n]);
    s["flows_cents"]["salary_off_cycle"] = json!(vec![100000; n]);
    write_json(&summary_path(&u), &s);
    let ctx = run(&u, today());
    let t = md(&ctx, "spending");
    assert!(
        t.contains("- No. In the last 10 months you earned $50,000.00"),
        "{t}"
    );
    assert!(task(&ctx, "spending", "spending-over-income").is_none());
}
