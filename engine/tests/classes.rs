//! Account classes, currencies and separate categories. Synthetic data only.

use std::path::PathBuf;

use cashcast_engine::{config, detect::Class, export, inbox::Inbox, summarize};
use chrono::NaiveDate;

/// Same column set as a MoneyWiz "net worth" report export.
const CSV: &str = r#""Account","Transfers","Category","Date","Amount","Currency"
"Bank A","","Employer ► Salary","2026-05-08","1,000.00","CAD"
"Wallet USD","","Employer ► Salary","2026-05-15","100.00","USD"
"Wallet USD","","Food ► Groceries","2026-05-20","-10.00","USD"
"Bank A","Wallet USD","","2026-05-21","-138.00","CAD"
"Wallet USD","Bank A","","2026-05-21","100.00","USD"
"Bank A","TFSA X","","2026-05-22","-500.00","CAD"
"Wallet USD","Brokerage X","","2026-05-23","200.00","USD"
"Card A","","Housing ► Rent","2026-05-01","-2,000.00","CAD"
"Card A","","Housing ► Furniture","2026-05-02","-300.00","CAD"
"#;

const CONFIG: &str = r#"
[window]
first_month = "2026-05"
last_month = "2026-05"

[currencies]
base = "CAD"
rates = { USD = 1.38 }

[accounts]
"Bank A" = "cash"
"Card A" = "card"
"Wallet USD" = "other"
"TFSA X" = "excluded"
"Brokerage X" = "excluded"

[categories]
salary = ["Employer > Salary"]
separate = ["Housing > Rent"]

[transfers]
from_excluded = { "Brokerage X" = "rsu_espp" }

[pay]
anchor = "2026-05-08"
"#;

fn csv_path(case: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cashcast-classes-{}-{case}.csv",
        std::process::id()
    ));
    std::fs::write(&path, CSV).unwrap();
    path
}

fn run(config_text: &str, case: &str) -> Result<summarize::Summary, String> {
    let e = export::load(&csv_path(case), &export::LoadOptions::default()).unwrap();
    let cfg = config::parse(config_text).unwrap();
    summarize::summarize(
        &Inbox::single(e),
        &cfg,
        NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
    )
}

fn may(map: &std::collections::BTreeMap<String, Vec<i64>>, key: &str) -> i64 {
    map.get(key)
        .unwrap_or_else(|| panic!("missing series {key}"))[0]
}

#[test]
fn other_accounts_count_flows_in_base_currency() {
    let s = run(CONFIG, "ok").unwrap();
    assert_eq!(may(&s.flows_cents, "salary"), 100_000);
    assert_eq!(may(&s.income_cents, "salary_other_accounts"), 13_800);
    assert_eq!(may(&s.spending_cents, "Food"), 1_380);
    assert_eq!(may(&s.income_cents, "rsu_espp"), 27_600);
    assert_eq!(may(&s.flows_cents, "invested"), 50_000);
    let wallet = s.accounts.iter().find(|a| a.name == "Wallet USD").unwrap();
    assert_eq!(
        (wallet.currency.as_str(), wallet.sum_balance_cents),
        ("USD", Some(39_000))
    );
    let pairs = s
        .checks
        .iter()
        .find(|c| c.name.starts_with("own-account transfer pairs"))
        .unwrap();
    assert_eq!(pairs.count, 0, "cross-currency transfers are not compared");
}

#[test]
fn separate_paths_get_their_own_spending_series() {
    let s = run(CONFIG, "separate").unwrap();
    assert_eq!(may(&s.spending_cents, "Housing > Rent"), 200_000);
    assert_eq!(may(&s.spending_cents, "Housing"), 30_000);
}

#[test]
fn transfer_only_accounts_are_guessed() {
    let s = run(
        &CONFIG.replace(r#""TFSA X" = "excluded""#, ""),
        "unclassified",
    )
    .unwrap();
    let tfsa = s.accounts.iter().find(|a| a.name == "TFSA X").unwrap();
    assert!(tfsa.guessed);
    assert_eq!(may(&s.flows_cents, "invested"), 50_000);
}

#[test]
fn a_key_ending_in_a_star_classes_accounts_by_prefix() {
    // Banks name each download by date ("activities-2026-11-08"); one `*` key covers them all.
    let s = run(
        &CONFIG.replace(
            r#""TFSA X" = "excluded""#,
            "\"TFSA*\" = \"excluded\"\n\"Wallet*\" = \"cash\"",
        ),
        "prefix",
    )
    .unwrap();
    let tfsa = s.accounts.iter().find(|a| a.name == "TFSA X").unwrap();
    assert_eq!((tfsa.class, tfsa.guessed), (Class::Excluded, false));
    let wallet = s.accounts.iter().find(|a| a.name == "Wallet USD").unwrap();
    assert_eq!(wallet.class, Class::Other, "an exact key wins over a prefix");
    assert_eq!(may(&s.flows_cents, "invested"), 50_000);
}

#[test]
fn a_currency_without_a_rate_stops_the_run() {
    let err = run(
        &CONFIG.replace("rates = { USD = 1.38 }", "rates = {}"),
        "rate",
    )
    .err()
    .expect("must fail");
    assert!(err.contains("no rate for USD"), "{err}");
}

#[test]
fn only_whole_transfer_or_tax_category_names_leave_spending() {
    let csv = r#""Account","Transfers","Category","Date","Amount","Currency"
"Bank A","","Financial ► Transfer Commission","2026-05-03","-10.00","CAD"
"Bank A","","Housing ► Property tax","2026-05-04","-100.00","CAD"
"Bank A","","Transfer","2026-05-05","-50.00","CAD"
"Card A","","Transfer","2026-05-05","50.00","CAD"
"Bank A","","Government ► Income Tax","2026-05-06","-300.00","CAD"
"#;
    let path =
        std::env::temp_dir().join(format!("cashcast-classes-{}-names.csv", std::process::id()));
    std::fs::write(&path, csv).unwrap();
    let e = export::load(&path, &export::LoadOptions::default()).unwrap();
    let cfg =
        config::parse("[window]\nfirst_month = \"2026-05\"\nlast_month = \"2026-05\"\n").unwrap();
    let s = summarize::summarize(
        &Inbox::single(e),
        &cfg,
        NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        may(&s.spending_cents, "Financial"),
        1_000,
        "a transfer fee is spending"
    );
    assert_eq!(
        may(&s.spending_cents, "Housing"),
        10_000,
        "property tax is spending"
    );
    assert_eq!(
        s.category_roles.get("Transfer").map(String::as_str),
        Some("own-account transfer")
    );
    assert_eq!(
        s.category_roles
            .get("Government > Income Tax")
            .map(String::as_str),
        Some("income tax")
    );
    assert_eq!(s.category_roles.len(), 2, "{:?}", s.category_roles);
}
