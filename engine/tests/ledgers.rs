//! Investment and crypto ledgers and the `holdings` command. Synthetic
//! fixtures in `tests/fixtures/ledger-*.csv`; every private field holds a CANARY.

use std::path::{Path, PathBuf};
use std::process::Command;

use cashcast_engine::ledger::{self, AssetHolding, Holdings};
use cashcast_engine::{config, inbox, summarize};
use chrono::NaiveDate;

const RATES: &str = "[currencies]\nbase = \"CAD\"\nrates = { USD = 1.4, RUB = 0.015 }\n";
const ETH_OPENING: &str = "[ledger.opening.ETH]\nquantity = 1.0\nacb = 3000\n";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Copies fixtures (`formats/x.csv` or `ledgers/y.csv`) into a fresh folder
/// under their short names. They sit flat in `tests/fixtures/` as
/// `format-x.csv` and `ledger-y.csv`, because the .gitignore allowlist
/// covers only that level.
fn inbox_dir(case: &str, names: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cashcast-ledgers-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in names {
        let (group, short) = name.split_once('/').expect("group/name");
        let prefix = if group == "formats" {
            "format"
        } else {
            "ledger"
        };
        std::fs::copy(
            fixtures().join(format!("{prefix}-{short}")),
            dir.join(short),
        )
        .unwrap();
    }
    dir
}

fn holdings(case: &str, names: &[&str], cfg: &str) -> Holdings {
    let cfg = config::parse(cfg).unwrap();
    ledger::load_dir(&inbox_dir(case, names), &cfg).unwrap()
}

fn asset<'a>(h: &'a Holdings, symbol: &str, bucket: &str) -> &'a AssetHolding {
    h.assets
        .iter()
        .find(|a| a.symbol == symbol && a.account_bucket == bucket)
        .unwrap_or_else(|| {
            panic!(
                "no {symbol} in {bucket}; have {:?}",
                h.assets
                    .iter()
                    .map(|a| (&a.symbol, &a.account_bucket))
                    .collect::<Vec<_>>()
            )
        })
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

fn needs(h: &Holdings) -> Vec<(&str, &str, usize)> {
    h.needs_cost
        .iter()
        .map(|n| (n.asset.as_str(), n.reason.as_str(), n.rows))
        .collect()
}

#[test]
fn kraken_trades_staking_withdrawals_and_an_opening_position() {
    let h = holdings(
        "kraken",
        &["ledgers/kraken.csv"],
        &format!("{RATES}{ETH_OPENING}"),
    );
    let f = &h.files[0];
    assert_eq!(f.format, "kraken-ledger-csv");
    assert_eq!(
        (f.rows, f.used_rows, f.ignored_rows, f.unhandled_rows),
        (16, 11, 3, 1)
    );
    assert_eq!(f.unhandled_types.get("margin"), Some(&1));
    assert_eq!(f.bad_lines, [17]);

    // Buy 0.1 for 6,000 + 24 fee; sell half for 4,000 - 16 fee; send 0.02 out;
    // swap 0.01 BTC for ETH.
    let btc = asset(&h, "BTC", "non-registered");
    close(btc.quantity, 0.04);
    close(btc.acb_cad, 2_409.6);
    close(btc.held_outside_files, 0.02);
    assert!(!btc.cost_unknown);
    close(btc.acb_per_unit_cad.unwrap(), 60_240.0);

    // With `sent_out_is_gone`, the 0.02 sent out leaves the holding at its
    // cost per unit.
    let gone = holdings(
        "kraken",
        &["ledgers/kraken.csv"],
        &format!("{RATES}{ETH_OPENING}\n[ledger]\nsent_out_is_gone = true\n"),
    );
    let b = asset(&gone, "BTC", "non-registered");
    close(b.quantity, 0.02);
    close(b.held_outside_files, 0.0);
    close(b.acb_cad, 1_204.8);

    // `[ledger.held]` ends the holding at the number read on the site; the
    // cost per unit stays.
    let held = holdings(
        "kraken",
        &["ledgers/kraken.csv"],
        &format!(
            "{RATES}{ETH_OPENING}\n[ledger]\nsent_out_is_gone = true\n[ledger.held]\nBTC = 0.01\n"
        ),
    );
    let b = asset(&held, "BTC", "non-registered");
    close(b.quantity, 0.01);
    close(b.acb_cad, 602.4);

    // The deposit uses up the opening position; the swap carries cost over.
    let eth = asset(&h, "ETH", "non-registered");
    close(eth.quantity, 0.75);
    close(eth.acb_cad, 1_500.0 + 602.4);
    assert!(eth.cost_unknown);
    let dot = asset(&h, "DOT", "non-registered");
    close(dot.quantity, 1.5);
    assert!(dot.cost_unknown);
    assert!(
        !h.assets
            .iter()
            .any(|a| ["CAD", "USD"].contains(&a.symbol.as_str())),
        "fiat is cash, not a holding"
    );

    let y = &h.realized_by_year[0];
    assert_eq!((y.year, y.sales, y.incomplete_sales), (2025, 3, 1));
    close(y.net_by_asset_cad["BTC"], 4_000.0 - 16.0 - 3_012.0);
    close(y.net_by_asset_cad["ETH"], 2_100.0 - 8.4 - 1_500.0);
    close(y.gains_cad, 972.0 + 591.6);
    close(y.losses_cad, 0.0);

    let staking = &h.income_by_year[0];
    assert_eq!(
        (
            staking.year,
            staking.kind.as_str(),
            staking.taxable,
            staking.rows,
            staking.rows_without_value
        ),
        (2025, "staking", true, 1, 1)
    );
    assert_eq!(
        needs(&h),
        [
            ("DOT", "reward without a CAD value", 1),
            (
                "ETH",
                "crypto-to-crypto trade without a CAD value (cost carried over)",
                1
            )
        ]
    );
    assert!(h.missing_rates.is_empty());
}

#[test]
fn a_crypto_deposit_without_a_known_cost_is_flagged_not_guessed() {
    let h = holdings("kraken-no-opening", &["ledgers/kraken.csv"], RATES);
    assert!(
        needs(&h).contains(&("ETH", "deposit with unknown cost", 1)),
        "{:?}",
        needs(&h)
    );
    let y = &h.realized_by_year[0];
    assert_eq!(
        (y.sales, y.incomplete_sales),
        (3, 2),
        "the ETH sale has no known cost"
    );
    close(y.net_cad, 972.0);
    assert!(asset(&h, "ETH", "non-registered").cost_unknown);
    assert!(ledger::render(&h).contains("cost unknown"));
}

#[test]
fn wealthsimple_registered_accounts_dividends_and_superficial_loss() {
    let h = holdings("ws", &["ledgers/wealthsimple-activity.csv"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "wealthsimple-activity-csv");
    assert_eq!(
        (f.rows, f.used_rows, f.ignored_rows, f.unhandled_rows),
        (10, 8, 1, 1)
    );
    assert!(f.bad_lines.is_empty());

    let vfv = asset(&h, "VFV", "TFSA");
    close(vfv.quantity, 15.0);
    close(vfv.acb_cad, 1_500.0);
    let xeqt = asset(&h, "XEQT", "non-registered");
    close(xeqt.quantity, 5.0);
    close(xeqt.acb_cad, 125.0);
    close(asset(&h, "BTC", "non-registered").acb_cad, 600.0);

    // Only the non-registered sale counts: 250 - 300 = -50.
    let y = &h.realized_by_year[0];
    assert_eq!((y.year, y.sales), (2025, 1));
    close(y.losses_cad, -50.0);
    let flags: Vec<(&str, i32, usize)> = h
        .superficial_loss_flags
        .iter()
        .map(|s| (s.asset.as_str(), s.year, s.count))
        .collect();
    assert_eq!(flags, [("XEQT", 2025, 1)]);

    let income: Vec<(&str, bool, f64)> = h
        .income_by_year
        .iter()
        .map(|i| (i.kind.as_str(), i.taxable, i.cad))
        .collect();
    assert_eq!(
        income,
        [("dividend", false, 12.0), ("dividend", true, 1.75)]
    );
}

#[test]
fn etrade_transactions_after_a_preamble_newest_first_in_usd() {
    let h = holdings("etrade", &["ledgers/etrade.csv"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "etrade-transactions-csv");
    assert_eq!((f.rows, f.used_rows, f.unhandled_rows), (4, 3, 1));
    assert_eq!(f.unhandled_types.get("transfer"), Some(&1));
    let acme = asset(&h, "ACME", "non-registered");
    close(acme.quantity, 10.0);
    close(acme.acb_cad, 2_005.0 * 1.4 / 2.0);
    close(
        h.realized_by_year[0].net_cad,
        1_500.0 * 1.4 - 5.0 * 1.4 - 2_005.0 * 1.4 / 2.0,
    );
    close(h.income_by_year[0].cad, 12.5 * 1.4);
}

#[test]
fn etrade_stock_plan_lots_cost_the_vest_date_value() {
    let h = holdings("plan", &["ledgers/etrade-stockplan.csv"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "etrade-stockplan-csv");
    assert_eq!((f.rows, f.used_rows, f.ignored_rows), (4, 2, 2));
    let acme = asset(&h, "ACME", "non-registered");
    close(acme.quantity, 16.0);
    close(acme.acb_cad, (8.0 * 140.0 + 8.0 * 145.5) * 1.4);
}

/// E*TRADE downloads stock plan files as .xlsx. The fixture holds the same
/// rows as the CSV one, after a summary sheet, with real Excel dates.
#[test]
fn etrade_stock_plan_xlsx_reads_the_sheet_with_the_ledger_header() {
    let h = holdings("plan-xlsx", &["ledgers/etrade-stockplan.xlsx"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "etrade-stockplan-csv");
    assert_eq!((f.rows, f.used_rows, f.ignored_rows), (4, 2, 2));
    let acme = asset(&h, "ACME", "non-registered");
    close(acme.quantity, 16.0);
    close(acme.acb_cad, (8.0 * 140.0 + 8.0 * 145.5) * 1.4);
}

#[test]
fn a_cash_import_skips_a_non_ledger_xlsx_with_a_reason() {
    let dir = inbox_dir("xlsx-cash", &["formats/td.csv"]);
    std::fs::write(dir.join("budget.xlsx"), b"not a workbook").unwrap();
    let cfg = config::parse("").unwrap();
    let inbox = inbox::load_dir(&dir, &cfg).unwrap();
    assert_eq!(inbox.failed.len(), 1);
    assert_eq!(inbox.failed[0].0, "budget.xlsx");
    assert!(inbox.failed[0].1.contains("CSV"), "{}", inbox.failed[0].1);
}

#[test]
fn wealthsimple_activity_export_with_effective_date_reads_the_same() {
    let h = holdings(
        "ws-2026",
        &["ledgers/wealthsimple-activity-2026.csv"],
        RATES,
    );
    let f = &h.files[0];
    assert_eq!(f.format, "wealthsimple-activity-csv");
    assert_eq!((f.rows, f.used_rows), (10, 8));
    assert!(f.bad_lines.is_empty());
    close(asset(&h, "VFV", "TFSA").quantity, 15.0);
    close(asset(&h, "XEQT", "non-registered").acb_cad, 125.0);
}

/// Benefit History gives the buys (net released shares, ESPP purchases);
/// Gains & Losses gives the sales. Withheld shares are never bought.
#[test]
fn etrade_benefit_history_and_gains_losses_match_the_sellable_count() {
    let h = holdings(
        "benefit",
        &[
            "ledgers/etrade-benefit-history.xlsx",
            "ledgers/etrade-gains-losses.csv",
        ],
        RATES,
    );
    let formats: Vec<(&str, &str, usize)> = h
        .files
        .iter()
        .map(|f| (f.name.as_str(), f.format.as_str(), f.used_rows))
        .collect();
    assert_eq!(
        formats,
        [
            (
                "etrade-benefit-history.xlsx [ESPP]",
                "etrade-benefit-espp",
                1
            ),
            (
                "etrade-benefit-history.xlsx [Restricted Stock]",
                "etrade-benefit-rs",
                1
            ),
            ("etrade-gains-losses.csv", "etrade-gains-losses", 2),
        ]
    );
    assert!(h.files.iter().all(|f| f.bad_lines.is_empty()));
    // 6 net shares at 140 (1400 taxable gain / 10 vested), 5 sold; 3 ESPP
    // shares at the 150 market value, 2 sold. Pooled: 590 / 4 × 2 left.
    let acme = asset(&h, "ACME", "non-registered");
    close(acme.quantity, 2.0);
    close(acme.acb_cad, 295.0 * 1.4);
    close(
        h.realized_by_year[0].net_cad,
        ((800.0 - 5.0 * 140.0) + (300.0 - 2.0 * 147.5)) * 1.4,
    );
    assert_eq!(
        h.checks,
        ["ACME: E*TRADE reports 2 sellable shares; the files give 2 non-registered (match)."]
    );
    assert!(ledger::render(&h).contains("## Checks"));
}

#[test]
fn the_same_export_downloaded_twice_counts_once() {
    let names = [
        "ledgers/etrade-benefit-history.xlsx",
        "ledgers/etrade-gains-losses.csv",
    ];
    let once = holdings("dup-once", &names, RATES);
    let dir = inbox_dir("dup-twice", &names);
    std::fs::copy(
        dir.join("etrade-benefit-history.xlsx"),
        dir.join("etrade-benefit-history-1.xlsx"),
    )
    .unwrap();
    std::fs::copy(
        dir.join("etrade-gains-losses.csv"),
        dir.join("etrade-gains-losses (1).csv"),
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let twice = ledger::load_dir(&dir, &cfg).unwrap();
    let a = asset(&once, "ACME", "non-registered");
    let b = asset(&twice, "ACME", "non-registered");
    close(b.quantity, a.quantity);
    close(b.acb_cad, a.acb_cad);
    close(
        twice.realized_by_year[0].net_cad,
        once.realized_by_year[0].net_cad,
    );
    assert!(
        twice.checks.contains(
            &"ACME: E*TRADE reports 2 sellable shares; the files give 2 non-registered (match)."
                .to_string()
        )
    );
    assert!(
        twice
            .checks
            .iter()
            .any(|c| c.starts_with("4 rows also appear in another file of the same format")),
        "{:?}",
        twice.checks
    );
}

#[test]
fn casa_vault_amounts_with_units_move_coins_in_and_out() {
    let h = holdings("casa-vault", &["ledgers/casa-vault.csv"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "casa-csv");
    assert_eq!((f.rows, f.used_rows), (2, 2));
    let btc = asset(&h, "BTC", "non-registered");
    close(btc.quantity, 0.05 - 0.0001);
    assert!(btc.cost_unknown, "Casa has no CAD values");
}

/// Sun Life group plan history: your and your employer's contributions buy
/// fund units in the RRSP; a fee redeems units; a switch sells one fund and
/// buys another; interest adds units; a withdrawal sells. No taxable gains.
#[test]
fn sunlife_group_rrsp_contributions_fees_switches_and_withdrawals() {
    let h = holdings("sunlife", &["ledgers/sunlife-transactions.csv"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "sunlife-transactions-csv");
    assert_eq!(
        (f.rows, f.used_rows, f.ignored_rows, f.unhandled_rows),
        (13, 10, 1, 1)
    );
    assert_eq!(f.unhandled_types.get("unknown type"), Some(&1));
    assert_eq!(f.bad_lines, [16]);

    // 20 + 10 units for 375; the fee redeems 0.4 units (5 of ACB).
    let target = asset(&h, "Target Date 2055 Fund", "RRSP");
    close(target.quantity, 29.6);
    close(target.acb_cad, 370.0);
    assert!(!target.cost_unknown);
    // 37.5 units for 375; 10 switched out (100 of ACB) to the money market.
    let bond = asset(&h, "Canadian Bond Index Fund", "RRSP");
    close(bond.quantity, 27.5);
    close(bond.acb_cad, 275.0);
    // 105 switched in + 1.05 interest - 50 withdrawn.
    let mm = asset(&h, "Money Market Fund", "RRSP");
    close(mm.quantity, 56.05);
    close(mm.acb_cad, 56.05);
    let tfsa = asset(&h, "Target Date 2055 Fund", "TFSA");
    close(tfsa.quantity, 8.0);
    close(tfsa.acb_cad, 100.0);

    assert!(
        h.realized_by_year.is_empty(),
        "registered: no taxable gains"
    );
    let income: Vec<(i32, &str, bool, f64)> = h
        .income_by_year
        .iter()
        .map(|i| (i.year, i.kind.as_str(), i.taxable, i.cad))
        .collect();
    assert_eq!(income, [(2025, "interest", false, 1.05)]);
    assert!(h.needs_cost.is_empty());

    let out = ledger::render(&h) + &serde_json::to_string(&h).unwrap();
    assert!(!out.contains("CANARY"), "a private value leaked:\n{out}");
    assert!(!out.contains("####"), "an account number leaked");
}

/// Other common column names; no plan column means the group RRSP; units
/// come from amount / unit price when the units cell is empty; spelled-out
/// plan names pick the bucket.
#[test]
fn a_fund_ledger_not_named_sun_life_is_not_assumed_registered() {
    // Another provider's file with the same columns and no plan column: its
    // gains must stay taxable, so it is non-registered, never a silent RRSP.
    let dir = inbox_dir("fund-not-sunlife", &[]);
    std::fs::write(
        dir.join("fund-history.csv"),
        "Date,Type,Fund,Units,Price\n\
         2025-01-31,Contribution,Global Equity Fund,10,20.00\n\
         2025-06-30,Withdrawal,Global Equity Fund,-5,30.00\n",
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let h = ledger::load_dir(&dir, &cfg).unwrap();
    close(
        asset(&h, "Global Equity Fund", "non-registered").quantity,
        5.0,
    );
    assert!(h.assets.iter().all(|a| a.account_bucket != "RRSP"));
    assert_eq!(h.realized_by_year.len(), 1, "the sale is a taxable gain");
}

#[test]
fn sunlife_aliases_default_to_the_rrsp_and_derive_units() {
    let dir = inbox_dir("sunlife-aliases", &[]);
    std::fs::write(
        dir.join("Sun Life history.csv"),
        "Transaction Date,Transaction,Investment option,Number of units,Unit value,Amount ($)\n\
         2025-03-01,Employer matching contribution,Global Equity Fund,,20.00,$200.00\n\
         2025-03-15,Rebalance,Global Equity Fund,-2.5,20.00,-$50.00\n\
         13/04/2025,Fee rebate,Global Equity Fund,0.1,20.00,$2.00\n\
         2025-05-01,Member voluntary,Global Equity Fund,1,20.00,$20.00\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("plans.csv"),
        "Date,Type,Fund,Units,Price,Plan type\n\
         2025-01-31,Employer required,Global Equity Fund,2,20.00,Deferred Profit Sharing Plan\n\
         2025-01-31,Member required,Global Equity Fund,3,20.00,Defined Contribution Pension Plan\n\
         2025-01-31,Contribution,Global Equity Fund,4,20.00,Non-registered savings plan\n",
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let h = ledger::load_dir(&dir, &cfg).unwrap();
    let files: Vec<(&str, usize, usize)> = h
        .files
        .iter()
        .map(|f| (f.format.as_str(), f.rows, f.used_rows))
        .collect();
    assert_eq!(
        files,
        [
            ("sunlife-transactions-csv", 4, 4),
            ("sunlife-transactions-csv", 3, 3)
        ]
    );
    let fund = asset(&h, "Global Equity Fund", "RRSP");
    close(fund.quantity, 8.6);
    close(fund.acb_cad, 172.0);
    close(asset(&h, "Global Equity Fund", "DPSP").acb_cad, 40.0);
    close(asset(&h, "Global Equity Fund", "RPP").quantity, 3.0);
    close(
        asset(&h, "Global Equity Fund", "non-registered").acb_cad,
        80.0,
    );
}

#[test]
fn columns_prints_header_names_and_shapes_but_no_values() {
    let dir = inbox_dir("columns", &["ledgers/etrade-stockplan.xlsx"]);
    let csv = dir.join("odd.csv");
    std::fs::write(
        &csv,
        "Account holder,CANARY Person\n\
         CANARY Owner,CANARY Two,CANARY Three\n\
         Date,Type,Amount,Description\n\
         2025-03-15,Buy,12.50,CANARY Payee 4417\n\
         Mar-16-2025,Sell,-3.00,CANARY Shop\n",
    )
    .unwrap();
    let out = ledger::columns(&csv) + &ledger::columns(&dir.join("etrade-stockplan.xlsx"));
    assert!(!out.contains("CANARY"), "a value leaked:\n{out}");
    assert!(!out.contains("12.50") && !out.contains("4417"), "{out}");
    assert!(
        out.contains("line 3: 4 cells | Date | Type | Amount | Description"),
        "{out}"
    );
    assert!(
        out.contains("line 4: 4 cells | date | a | num | a a 9"),
        "{out}"
    );
    assert!(
        out.contains("line 5: 4 cells | a-9-9 | a | num | a a"),
        "{out}"
    );
    assert!(out.contains("sheet Lots (etrade-stockplan-csv)"), "{out}");
    assert!(out.contains("Record Type | Symbol | Plan Type"), "{out}");
}

#[test]
fn a_missing_currency_rate_is_reported_and_costs_stay_unknown() {
    let h = holdings("no-rate", &["ledgers/etrade-stockplan.csv"], "");
    assert_eq!(h.missing_rates, ["USD"]);
    assert!(asset(&h, "ACME", "non-registered").cost_unknown);
    assert!(ledger::render(&h).contains("Missing rates"));
}

#[test]
fn generic_crypto_wallet_buy_send_receive_sell_and_reward() {
    let h = holdings("casa", &["ledgers/casa.csv"], RATES);
    let f = &h.files[0];
    assert_eq!(f.format, "crypto-generic-csv");
    assert_eq!((f.rows, f.used_rows, f.unhandled_rows), (6, 5, 1));
    let btc = asset(&h, "BTC", "non-registered");
    close(btc.quantity, 0.0102);
    close(btc.acb_cad, 1_505.0 / 2.0 + 20.0);
    assert!(!btc.cost_unknown, "the receive matches the earlier send");
    close(h.realized_by_year[0].net_cad, 1_000.0 - 2.0 - 1_505.0 / 2.0);
    let income: Vec<(&str, f64)> = h
        .income_by_year
        .iter()
        .map(|i| (i.kind.as_str(), i.cad))
        .collect();
    assert_eq!(income, [("staking", 20.0)]);
}

#[test]
fn acb_pools_one_asset_across_non_registered_files() {
    let h = holdings(
        "pool",
        &[
            "ledgers/kraken.csv",
            "ledgers/casa.csv",
            "ledgers/wealthsimple-activity.csv",
        ],
        &format!("{RATES}{ETH_OPENING}"),
    );
    assert_eq!(h.assets.iter().filter(|a| a.symbol == "BTC").count(), 1);
    assert_eq!(h.files.len(), 3);
}

#[test]
fn the_cash_import_lists_ledgers_but_does_not_read_them() {
    let dir = inbox_dir(
        "split",
        &[
            "formats/google-sheets.csv",
            "ledgers/kraken.csv",
            "ledgers/etrade.csv",
            "ledgers/casa.csv",
        ],
    );
    let cfg =
        config::parse("[window]\nfirst_month = \"2026-01\"\nlast_month = \"2026-09\"\n").unwrap();
    let inbox = inbox::load_dir(&dir, &cfg).unwrap();
    assert_eq!(inbox.files.len(), 1);
    let ledgers: Vec<(&str, &str)> = inbox
        .ledger_files
        .iter()
        .map(|(n, f)| (n.as_str(), f.as_str()))
        .collect();
    assert_eq!(
        ledgers,
        [
            ("casa.csv", "crypto-generic-csv"),
            ("etrade.csv", "etrade-transactions-csv"),
            ("kraken.csv", "kraken-ledger-csv")
        ]
    );
    let s =
        summarize::summarize(&inbox, &cfg, NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()).unwrap();
    assert!(summarize::render(&s).contains("kraken.csv (kraken-ledger-csv): ledger file"));
    assert!(matches!(inbox::first_problem(&dir, &cfg), Ok(None)));

    let only = inbox_dir("only-ledgers", &["ledgers/kraken.csv"]);
    let err = inbox::load_dir(&only, &cfg).err().expect("no cash files");
    assert!(err.contains("holdings"), "{err}");
}

fn run(home: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_cashcast-engine"))
        .args(args)
        .arg("--home")
        .arg(home)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{args:?} failed: {stderr}");
    assert!(!stderr.contains("CANARY"), "private text in stderr");
    stdout
}

#[test]
fn cli_import_and_holdings_leak_no_canary_and_write_owner_only_files() {
    let home = std::env::temp_dir().join(format!("cashcast-ledgers-{}-cli", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let mut names: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter_map(|n| {
            n.strip_prefix("format-")
                .map(|s| format!("formats/{s}"))
                .or_else(|| n.strip_prefix("ledger-").map(|s| format!("ledgers/{s}")))
        })
        .collect();
    names.sort();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let inbox = inbox_dir("cli-inbox", &refs);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::rename(&inbox, home.join("inbox")).unwrap();
    let cfg = format!(
        "[window]\nfirst_month = \"2025-01\"\nlast_month = \"2026-09\"\n{RATES}{ETH_OPENING}"
    );
    std::fs::write(home.join("cashcast.toml"), cfg).unwrap();

    let import = run(&home, &["import", "--today", "2026-10-02"]);
    let holdings = run(&home, &["holdings"]);
    let summary_json = std::fs::read_to_string(home.join("data/summary.json")).unwrap();
    let holdings_json = std::fs::read_to_string(home.join("data/holdings.json")).unwrap();
    for out in [&import, &holdings, &summary_json, &holdings_json] {
        assert!(!out.contains("CANARY"), "private text leaked");
        assert!(!out.contains("example.com"), "an email leaked");
        assert!(!out.contains("####"), "an account number line leaked");
    }
    assert!(import.contains("ledger file"));
    assert!(
        holdings.contains("# Holdings report") && holdings.contains("| BTC | non-registered |")
    );
    let parsed: serde_json::Value = serde_json::from_str(&holdings_json).unwrap();
    for key in [
        "files",
        "assets",
        "realized_by_year",
        "income_by_year",
        "superficial_loss_flags",
        "needs_cost",
        "missing_rates",
    ] {
        assert!(parsed.get(key).is_some(), "holdings.json lacks {key}");
    }
    assert_eq!(
        parsed["files"].as_array().unwrap().len(),
        14,
        "13 CSV ledgers plus the E*TRADE API page"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join("data/holdings.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// RRSP room: new money in (yours and your employer's) per deduction year,
/// where the first 60 days of a year count for the year before. Fund
/// switches, transfers, rollovers, fees and interest are not contributions,
/// and a TFSA contribution is reported under its own bucket.
#[test]
fn contributions_are_summed_per_deduction_year_and_month() {
    let h = holdings(
        "sunlife-contrib",
        &["ledgers/sunlife-transactions.csv"],
        RATES,
    );
    let rows: Vec<(&str, i32, &str, &str, f64, usize)> = h
        .contributions
        .iter()
        .map(|c| {
            (
                c.account_bucket.as_str(),
                c.deduction_year,
                c.month.as_str(),
                c.source.as_str(),
                c.cad,
                c.rows,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("RRSP", 2024, "2025-01", "employer", 125.0, 1),
            ("RRSP", 2024, "2025-01", "member", 250.0, 1),
            ("RRSP", 2024, "2025-02", "employer", 125.0, 1),
            ("RRSP", 2024, "2025-02", "member", 250.0, 1),
            ("TFSA", 2025, "2025-08", "member", 100.0, 1),
        ]
    );
    let json = serde_json::to_string(&h).unwrap();
    assert!(json.contains("\"contributions\""));
    assert!(!json.contains("CANARY"));
}

#[test]
fn the_sixty_day_rule_splits_march_and_reversals_subtract() {
    let dir = inbox_dir("sunlife-sixty", &[]);
    std::fs::write(
        dir.join("sunlife-2026.csv"),
        "Date,Transaction type,Fund name,Units,Unit price,Amount\n\
         2026-03-01,Employee contribution,Global Equity Fund,1,100.00,$100.00\n\
         2026-03-02,Employee contribution,Global Equity Fund,2,100.00,$200.00\n\
         2026-03-02,Employer matching contribution,Global Equity Fund,1,100.00,$100.00\n\
         2026-03-20,Contribution reversal,Global Equity Fund,-0.5,100.00,-$50.00\n\
         2026-04-01,Transfer in from another RRSP,Global Equity Fund,10,100.00,$1000.00\n\
         2026-04-01,Fund switch in,Global Equity Fund,1,100.00,$100.00\n\
         2024-02-29,Employee contribution,Global Equity Fund,1,100.00,$100.00\n\
         2026-05-01,Member voluntary withdrawal,Global Equity Fund,1,100.00,$100.00\n\
         2026-05-02,Interest - employer required,Global Equity Fund,0.1,100.00,$10.00\n\
         2026-05-03,Employee contribution refund,Global Equity Fund,-1,100.00,-$100.00\n",
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let h = ledger::load_dir(&dir, &cfg).unwrap();
    let rows: Vec<(i32, &str, &str, f64)> = h
        .contributions
        .iter()
        .map(|c| (c.deduction_year, c.month.as_str(), c.source.as_str(), c.cad))
        .collect();
    assert_eq!(
        rows,
        [
            (2023, "2024-02", "member", 100.0),
            (2025, "2026-03", "member", 100.0),
            (2026, "2026-03", "employer", 100.0),
            (2026, "2026-03", "member", 150.0),
        ]
    );
}

#[test]
fn wealthsimple_cash_deposits_into_a_registered_account_are_contributions() {
    let dir = inbox_dir("ws-contrib", &[]);
    std::fs::write(
        dir.join("activity.csv"),
        "transaction_date,settlement_date,account_id,account_type,activity_type,activity_sub_type,direction,symbol,name,currency,quantity,unit_price,commission,net_cash_amount\n\
         2026-02-10,2026-02-10,CANARY-A,RRSP,Deposit,,,,,CAD,,,,300.00\n\
         2026-05-10,2026-05-10,CANARY-A,RRSP,Deposit,,,,,CAD,,,,400.00\n\
         2026-05-11,2026-05-11,CANARY-A,RRSP,MoneyMovement,TRANSFER_IN,,,,CAD,,,,900.00\n\
         2026-05-12,2026-05-12,CANARY-B,Non-registered,Deposit,,,,,CAD,,,,500.00\n",
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let h = ledger::load_dir(&dir, &cfg).unwrap();
    let rows: Vec<(&str, i32, f64)> = h
        .contributions
        .iter()
        .map(|c| (c.account_bucket.as_str(), c.deduction_year, c.cad))
        .collect();
    assert_eq!(rows, [("RRSP", 2025, 300.0), ("RRSP", 2026, 400.0)]);
}

#[test]
fn contributions_in_a_file_downloaded_twice_count_once() {
    let names = ["ledgers/sunlife-transactions.csv"];
    let sums = |h: &Holdings| -> Vec<(String, f64, usize)> {
        h.contributions
            .iter()
            .map(|c| (c.month.clone(), c.cad, c.rows))
            .collect()
    };
    let once = holdings("contrib-once", &names, RATES);
    let dir = inbox_dir("contrib-twice", &names);
    std::fs::copy(
        dir.join("sunlife-transactions.csv"),
        dir.join("sunlife-transactions-1.csv"),
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let twice = ledger::load_dir(&dir, &cfg).unwrap();
    assert_eq!(sums(&twice), sums(&once));
}

#[test]
fn every_coin_movement_counts_toward_the_account_shares() {
    // Deposits, withdrawals, fees, rewards and swaps move coins in or out of
    // the account, so the share count that `moneywiz diagnose` compares must
    // hold them, not only buys and sells.
    let h = holdings(
        "kraken-moves",
        &["ledgers/kraken.csv"],
        &format!("{RATES}{ETH_OPENING}"),
    );
    for sym in ["BTC", "ETH", "DOT"] {
        let a = asset(&h, sym, "non-registered");
        let moved: f64 = h
            .trades
            .iter()
            .filter(|t| t.symbol == sym)
            .map(|t| t.shares)
            .sum();
        close(moved, a.quantity - a.held_outside_files);
    }
}

/// A Sun Life interfund transfer names each fund "From X" or "To X": the
/// units belong to fund X, not to a fund called "From X".
#[test]
fn sunlife_interfund_transfer_strips_from_and_to() {
    let dir = inbox_dir("sunlife-interfund", &[]);
    std::fs::write(
        dir.join("sunlife-transactions.csv"),
        "Date,Plan,Transaction type,Fund name,Units,Unit price,Amount\n\
         2025-01-31,RRSP,Contribution (Member),Alpha Fund,10,20.00,200.00\n\
         2025-02-28,RRSP,Transfer (interfund),from Alpha Fund,-4,25.00,-100.00\n\
         2025-02-28,RRSP,Transfer (interfund),To Beta Fund,5,20.00,100.00\n",
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let h = ledger::load_dir(&dir, &cfg).unwrap();
    close(asset(&h, "Alpha Fund", "RRSP").quantity, 6.0);
    close(asset(&h, "Beta Fund", "RRSP").quantity, 5.0);
    assert!(
        h.assets
            .iter()
            .all(|a| !a.symbol.to_lowercase().starts_with("from ")
                && !a.symbol.to_lowercase().starts_with("to "))
    );
}

/// A Sun Life interfund transfer moves money between funds inside the plan:
/// its legs are no cash rows for `moneywiz sync` (the trades carry it).
#[test]
fn sunlife_interfund_transfer_makes_no_cash_rows() {
    let dir = inbox_dir("sunlife-interfund-cash", &[]);
    let path = dir.join("sunlife-transactions.csv");
    std::fs::write(
        &path,
        "Date,Plan,Transaction type,Fund name,Units,Unit price,Amount\n\
         2025-01-31,RRSP,Contribution (Member),Canary Alpha Fund,10,20.00,200.00\n\
         2025-02-28,RRSP,Transfer (interfund),from Canary Alpha Fund,-4,25.00,-100.00\n\
         2025-02-28,RRSP,Transfer (interfund),To Canary Beta Fund,5,20.00,100.00\n",
    )
    .unwrap();
    let e = ledger::cash_export(&path).unwrap();
    let rows: Vec<(String, i64)> = e.txns.iter().map(|t| (t.text.clone(), t.cents)).collect();
    assert_eq!(rows, vec![("Canary Alpha Fund".to_string(), 20_000)]);
}

/// A coin deposit has no price in the ledger; a daily price file in
/// `inbox/prices/` (Kraken OHLCVT) gives it a CAD value, through USD when
/// there is no CAD pair.
#[test]
fn coin_moves_get_a_cad_value_from_daily_price_files() {
    let dir = inbox_dir("coin-prices", &[]);
    std::fs::write(
        dir.join("kraken-ledgers.csv"),
        "\"txid\",\"refid\",\"time\",\"type\",\"subtype\",\"aclass\",\"asset\",\"amount\",\"fee\",\"balance\"\n\
         \"T1\",\"R1\",\"2025-03-02 10:00:00\",\"deposit\",\"\",\"currency\",\"XXBT\",\"0.5\",\"0\",\"0.5\"\n\
         \"T2\",\"R2\",\"2025-03-02 11:00:00\",\"deposit\",\"\",\"currency\",\"CANARYCOIN\",\"10\",\"0\",\"10\"\n",
    )
    .unwrap();
    let prices = dir.join("prices");
    std::fs::create_dir_all(&prices).unwrap();
    // 2025-03-01 00:00 UTC
    std::fs::write(
        prices.join("XBTUSD_1440.csv"),
        "1740787200,1,1,1,80000,1,1\n",
    )
    .unwrap();
    std::fs::write(
        prices.join("USDCAD_1440.csv"),
        "1740787200,1,1,1,1.25,1,1\n",
    )
    .unwrap();
    let cfg = config::parse(RATES).unwrap();
    let h = ledger::load_dir(&dir, &cfg).unwrap();
    let btc = h.trades.iter().find(|t| t.symbol == "BTC").unwrap();
    assert_eq!(btc.cad, Some(50000.0));
    let other = h.trades.iter().find(|t| t.symbol == "CANARYCOIN").unwrap();
    assert_eq!(other.cad, None, "no price file: no value");
}

#[test]
fn a_kraken_sell_moves_its_proceeds_less_the_fee() {
    // The cash export holds the sale net of its fee, so the trade that
    // `moneywiz diagnose` pairs with a MoneyWiz sell must too.
    let h = holdings(
        "kraken-sell-cash",
        &["ledgers/kraken.csv"],
        &format!("{RATES}{ETH_OPENING}"),
    );
    let sell = h
        .trades
        .iter()
        .find(|t| t.symbol == "BTC" && t.shares < 0.0 && t.cash.as_deref() == Some("CAD"))
        .expect("the BTC sale");
    close(sell.cad.unwrap(), 4_000.0 - 16.0);
    let buy = h
        .trades
        .iter()
        .find(|t| t.symbol == "BTC" && t.shares > 0.0 && t.cad.is_some())
        .expect("the BTC buy");
    close(buy.cad.unwrap(), 6_000.0 + 24.0);
}
