//! Generic CSV, headerless CSV and OFX inputs, pay detection and transfer
//! matching. Synthetic data only; every description carries a canary.

use std::path::PathBuf;

use cashcast_engine::detect::Class;
use cashcast_engine::{config, inbox, inspect, summarize};
use chrono::{Duration, NaiveDate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Writes files into a fresh inbox folder named after `case`.
fn inbox_dir(case: &str, files: &[(&str, String)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cashcast-generic-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    dir
}

const WINDOW: &str = "[window]\nfirst_month = \"2026-01\"\nlast_month = \"2026-09\"\n";

fn run(
    case: &str,
    files: &[(&str, String)],
    extra_config: &str,
) -> (summarize::Summary, String, inbox::Inbox) {
    let dir = inbox_dir(case, files);
    let cfg = config::parse(&format!("{WINDOW}{extra_config}")).unwrap();
    let inbox = inbox::load_dir(&dir, &cfg).unwrap();
    let s = summarize::summarize(&inbox, &cfg, d("2026-10-02")).unwrap();
    let text = summarize::render(&s);
    (s, text, inbox)
}

fn total(map: &std::collections::BTreeMap<String, Vec<i64>>, key: &str) -> i64 {
    map.get(key)
        .unwrap_or_else(|| {
            panic!(
                "missing series {key}; have {:?}",
                map.keys().collect::<Vec<_>>()
            )
        })
        .iter()
        .sum()
}

/// A bank CSV with signed amounts and a running balance, plus a card CSV
/// that exports purchases as positive amounts.
fn bank_and_card() -> Vec<(&'static str, String)> {
    let mut rows = vec!["Date,Description,Amount,Balance".to_string()];
    let mut events: Vec<(NaiveDate, &str, i64)> = Vec::new();
    for k in 0..19 {
        events.push((
            d("2026-01-09") + Duration::days(14 * k),
            "ACME CORP PAYROLL CANARY01",
            200_000,
        ));
    }
    for m in 1..=9 {
        events.push((
            d(&format!("2026-{m:02}-03")),
            "LOBLAWS #55 CANARY02",
            -15_000,
        ));
        events.push((d(&format!("2026-{m:02}-01")), "RENT CANARY04", -120_000));
    }
    events.push((d("2026-09-10"), "BANK VISA PAYMENT CANARY03", -50_000));
    events.push((d("2026-06-20"), "INTERAC E-TRANSFER CANARY05", 10_000));
    events.sort_by_key(|e| e.0);
    let mut balance = 100_000;
    for (date, text, cents) in &events {
        balance += cents;
        rows.push(format!(
            "{date},{text},{:.2},{:.2}",
            *cents as f64 / 100.0,
            balance as f64 / 100.0
        ));
    }
    let card = [
        "Transaction Date,Posted Date,Description,Amount",
        "2026-09-01,2026-09-02,STARBUCKS CANARY06,5.50",
        "2026-09-02,2026-09-03,UBER EATS CANARY07,30.00",
        "2026-09-03,2026-09-04,AMAZON CANARY08,80.00",
        "2026-09-04,2026-09-05,STARBUCKS CANARY06,6.00",
        "2026-09-05,2026-09-06,UBER EATS CANARY07,25.00",
        "2026-09-06,2026-09-07,AMAZON CANARY08,40.00",
        "2026-09-11,2026-09-11,PAYMENT THANK YOU CANARY09,-500.00",
    ]
    .join("\n");
    vec![("chequing.csv", rows.join("\n")), ("visa.csv", card)]
}

#[test]
fn bank_csv_finds_pay_categories_balance_and_card_payment() {
    let (s, text, _) = run("bank", &bank_and_card(), "");
    assert_eq!(s.pay.schedule.as_deref(), Some("biweekly"));
    assert_eq!(s.pay.source, "Deposits #1");
    assert_eq!(
        s.pay
            .half_years
            .iter()
            .map(|h| h.median_cents)
            .collect::<Vec<_>>(),
        [200_000, 200_000]
    );
    assert_eq!(total(&s.flows_cents, "salary"), 19 * 200_000);
    assert_eq!(total(&s.spending_cents, "Groceries"), 9 * 15_000);
    assert_eq!(total(&s.spending_cents, "Housing"), 9 * 120_000);
    assert_eq!(
        total(&s.spending_cents, "Dining"),
        3_000 + 2_500 + 550 + 600
    );
    assert_eq!(total(&s.spending_cents, "Shopping"), 12_000);
    assert_eq!(total(&s.flows_cents, "card_payments"), 50_000);
    assert!(
        !s.spending_cents.contains_key("(Uncategorized)"),
        "the card payment must not count as spending"
    );
    assert_eq!(total(&s.flows_cents, "uncategorized_in"), 10_000);

    let chequing = s.accounts.iter().find(|a| a.name == "chequing").unwrap();
    assert_eq!((chequing.class, chequing.guessed), (Class::Cash, true));
    assert_eq!(chequing.balance_source.as_deref(), Some("running balance"));
    let expected_balance = 100_000 + 19 * 200_000 - 9 * 15_000 - 9 * 120_000 - 50_000 + 10_000;
    assert_eq!(chequing.balance_cents, Some(expected_balance));
    let visa = s.accounts.iter().find(|a| a.name == "visa").unwrap();
    assert_eq!(visa.class, Class::Card);

    let card_file = s.files.iter().find(|f| f.name == "visa.csv").unwrap();
    assert!(card_file.notes.iter().any(|n| n.contains("signs flipped")));
    assert!(text.contains("Schedule: biweekly"));
}

#[test]
fn no_canary_reaches_reports_but_the_private_review_has_samples() {
    let mut files = bank_and_card();
    files.extend(other_formats());
    let (s, text, inbox) = run("leak", &files, "");
    let json = serde_json::to_string(&s).unwrap();
    let totals = inspect::render_totals(&inspect::totals(&inbox));
    for out in [&json, &text, &totals] {
        assert!(!out.contains("CANARY"), "description text leaked");
        assert!(!out.contains("000111222333"), "full account number leaked");
    }
    assert!(summarize::render_review(&inbox, &s).contains("CANARY01"));
}

fn ofx(acct_block: &str, list: &str, balance: &str) -> String {
    format!(
        "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n<OFX>\n<BANKMSGSRSV1><STMTTRNRS><STMTRS>\n<CURDEF>CAD\n{acct_block}\n<BANKTRANLIST>\n{list}</BANKTRANLIST>\n<LEDGERBAL><BALAMT>{balance}<DTASOF>20260930</LEDGERBAL>\n</STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n"
    )
}

fn other_formats() -> Vec<(&'static str, String)> {
    let acct = "<BANKACCTFROM><BANKID>9<ACCTID>000111222333<ACCTTYPE>CHECKING</BANKACCTFROM>";
    let a = ofx(
        acct,
        "<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260905120000[-5:EST]<TRNAMT>-42.10<FITID>A1<NAME>LOBLAWS CANARY10</STMTTRN>\n\
         <STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20260910<TRNAMT>1500.00<FITID>A2<NAME>PAYROLL CANARY11</STMTTRN>\n",
        "2345.67",
    );
    let b = ofx(
        acct,
        "<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20260910<TRNAMT>1500.00<FITID>A2<NAME>PAYROLL CANARY11</STMTTRN>\n\
         <STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261001<TRNAMT>-10.00<FITID>A3<NAME>COFFEE CANARY12</STMTTRN>\n",
        "2345.67",
    );
    let card = "OFXHEADER:100\n<OFX><CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><CURDEF>CAD\n\
        <CCACCTFROM><ACCTID>4500123412349876</CCACCTFROM><BANKTRANLIST>\n\
        <STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260912<TRNAMT>-64.00<FITID>C1<NAME>NETFLIX CANARY13</STMTTRN>\n\
        </BANKTRANLIST></CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>\n"
        .to_string();
    let mint = [
        "\"Date\",\"Description\",\"Original Description\",\"Amount\",\"Transaction Type\",\"Category\",\"Account Name\",\"Labels\",\"Notes\"",
        "\"09/01/2026\",\"Employer CANARY14\",\"EMPLOYER CANARY14\",\"3000.00\",\"credit\",\"Paycheck\",\"Mint Checking\",\"\",\"\"",
        "\"09/03/2026\",\"Grocer CANARY15\",\"GROCER CANARY15\",\"120.00\",\"debit\",\"Groceries\",\"Mint Card\",\"\",\"\"",
        "\"09/05/2026\",\"Card payment CANARY16\",\"CARD CANARY16\",\"120.00\",\"debit\",\"Credit Card Payment\",\"Mint Checking\",\"\",\"\"",
        "\"09/05/2026\",\"Card payment CANARY16\",\"CARD CANARY16\",\"120.00\",\"credit\",\"Credit Card Payment\",\"Mint Card\",\"\",\"\"",
        "\"09/15/2026\",\"Employer CANARY14\",\"EMPLOYER CANARY14\",\"3000.00\",\"credit\",\"Paycheck\",\"Mint Checking\",\"\",\"\"",
    ]
    .join("\n");
    let ynab = [
        "\"Account\",\"Flag\",\"Date\",\"Payee\",\"Category Group/Category\",\"Category Group\",\"Category\",\"Memo\",\"Outflow\",\"Inflow\",\"Cleared\"",
        "\"YNAB Chequing\",\"\",\"2026-09-01\",\"Employer CANARY17\",\"Inflow: Ready to Assign\",\"Inflow\",\"Ready to Assign\",\"\",\"$0.00\",\"$2,500.00\",\"Cleared\"",
        "\"YNAB Chequing\",\"\",\"2026-09-02\",\"Transfer : YNAB Savings\",\"\",\"\",\"\",\"\",\"$200.00\",\"$0.00\",\"Cleared\"",
        "\"YNAB Savings\",\"\",\"2026-09-02\",\"Transfer : YNAB Chequing\",\"\",\"\",\"\",\"\",\"$0.00\",\"$200.00\",\"Cleared\"",
        "\"YNAB Chequing\",\"\",\"2026-09-04\",\"Grocer CANARY18\",\"Everyday: Groceries\",\"Everyday\",\"Groceries\",\"CANARY19\",\"$55.00\",\"$0.00\",\"Cleared\"",
    ]
    .join("\n");
    let headerless = [
        "09/13/2026,COFFEE SHOP CANARY20,4.50,,995.50",
        "09/14/2026,DEPOSIT CANARY21,,2000.00,2995.50",
        "09/15/2026,HYDRO CANARY22,80.00,,2915.50",
        "09/16/2026,AMAZON CANARY23,20.00,,2895.50",
        "09/17/2026,PHARMACY CANARY24,15.00,,2880.50",
    ]
    .join("\n");
    vec![
        ("a-statement.ofx", a),
        ("b-statement.qfx", b),
        ("card.ofx", card),
        ("mint.csv", mint),
        ("ynab.csv", ynab),
        ("td-bank.csv", headerless),
    ]
}

#[test]
fn ofx_mint_ynab_and_headerless_files_read() {
    let (s, _, inbox) = run("formats", &other_formats(), "");
    let file = |name: &str| s.files.iter().find(|f| f.name == name).unwrap();
    assert_eq!(file("a-statement.ofx").format, "OFX");
    assert_eq!(
        inbox.duplicates_dropped, 1,
        "FITID A2 appears in two OFX files"
    );
    let checking = s
        .accounts
        .iter()
        .find(|a| a.name == "Checking …2333")
        .unwrap();
    assert_eq!(checking.class, Class::Cash);
    assert_eq!(
        checking.balance_cents,
        Some(234_567 - 1_000),
        "statement balance plus later rows"
    );
    let card = s
        .accounts
        .iter()
        .find(|a| a.name == "Credit card …9876")
        .unwrap();
    assert_eq!(card.class, Class::Card);
    assert_eq!(total(&s.spending_cents, "Subscriptions"), 6_400);

    // Mint: signs from the type column; card payment category is internal.
    assert_eq!(
        s.category_roles
            .get("Credit Card Payment")
            .map(String::as_str),
        Some("own-account transfer")
    );
    assert_eq!(
        s.category_roles.get("Paycheck").map(String::as_str),
        Some("income (other)")
    );
    // YNAB: group and category combine; payee transfers are own-account moves.
    assert_eq!(
        s.category_roles
            .get("Inflow > Ready to Assign")
            .map(String::as_str),
        Some("income (other)")
    );
    assert_eq!(total(&s.spending_cents, "Everyday"), 5_500);
    assert_eq!(total(&s.spending_cents, "Groceries"), 12_000 + 4_210);

    let td = file("td-bank.csv");
    assert_eq!(td.format, "CSV (no header row)");
    assert!(
        td.notes[0].contains("date=1, debit=3, credit=4, balance=5, description=2"),
        "{:?}",
        td.notes
    );
    let td_account = s.accounts.iter().find(|a| a.name == "td-bank").unwrap();
    assert_eq!(td_account.balance_cents, Some(288_050));
    assert_eq!(total(&s.spending_cents, "Housing"), 8_000);
}

#[test]
fn transfers_between_files_are_matched() {
    let chequing = "Date,Description,Amount\n2026-09-10,ONLINE BANKING CANARY30,-700.00\n2026-09-11,COFFEE CANARY31,-3.00";
    let savings = "Date,Description,Amount\n2026-09-12,DEPOSIT CANARY32,700.00\n2026-09-13,INTEREST PAID CANARY33,1.00";
    let (s, _, _) = run(
        "pairs",
        &[
            ("chequing.csv", chequing.into()),
            ("savings.csv", savings.into()),
        ],
        "",
    );
    let matched = s
        .checks
        .iter()
        .find(|c| c.name.starts_with("rows matched as own-account transfers"))
        .unwrap();
    assert_eq!(matched.count, 2);
    assert_eq!(total(&s.flows_cents, "uncategorized_in"), 0);
    assert!(!s.spending_cents.contains_key("(Uncategorized)"));
    assert_eq!(total(&s.income_cents, "cashback"), 100);
}

fn pay_file(dates: &[NaiveDate]) -> String {
    let mut rows = vec!["Date,Description,Amount".to_string()];
    for date in dates {
        rows.push(format!("{date},SALARY CANARY40,3000.00"));
        rows.push(format!("{date},GROCER CANARY41,-50.00"));
    }
    rows.join("\n")
}

#[test]
fn semimonthly_and_monthly_pay_are_found() {
    let semi: Vec<NaiveDate> = (1..=9)
        .flat_map(|m| {
            [
                d(&format!("2026-{m:02}-15")),
                cashcast_engine::detect::last_day(2026, m),
            ]
        })
        .collect();
    let (s, _, _) = run("semi", &[("chequing.csv", pay_file(&semi))], "");
    assert_eq!(s.pay.schedule.as_deref(), Some("semimonthly"));
    assert_eq!(s.pay.cycles_per_month, vec![2; 9]);

    let monthly: Vec<NaiveDate> = (1..=9).map(|m| d(&format!("2026-{m:02}-25"))).collect();
    let (s, _, _) = run("monthly", &[("chequing.csv", pay_file(&monthly))], "");
    assert_eq!(s.pay.schedule.as_deref(), Some("monthly"));
    assert_eq!(total(&s.flows_cents, "salary"), 9 * 300_000);
}

#[test]
fn first_problem_shows_one_ofx_block_or_names_a_bad_file() {
    let broken = ofx(
        "<BANKACCTFROM><ACCTID>1<ACCTTYPE>CHECKING</BANKACCTFROM>",
        "<STMTTRN><DTPOSTED>20260901<TRNAMT>-1.00<NAME>OK CANARY50</STMTTRN><STMTTRN><DTPOSTED>bad<TRNAMT>-2.00<NAME>BAD CANARY51</STMTTRN><STMTTRN><DTPOSTED>20260903<TRNAMT>-3.00<NAME>LATER CANARY52</STMTTRN>",
        "1.00",
    );
    let dir = inbox_dir("first-ofx", &[("x.ofx", broken)]);
    match inbox::first_problem(&dir, &config::Config::default()).unwrap() {
        Some(inbox::FirstProblem::Row { row, .. }) => {
            assert_eq!(row.reason, "date not parseable");
            assert_eq!(row.records_before, 1);
            let shown = &row.fields[0].1;
            assert!(
                shown.contains("CANARY51")
                    && !shown.contains("CANARY50")
                    && !shown.contains("CANARY52")
            );
        }
        _ => panic!("expected a row"),
    }
    let ambiguous = "Date,Amount\n01/02/2026,-1.00\n03/04/2026,-2.00";
    let dir = inbox_dir("first-file", &[("y.csv", ambiguous.into())]);
    assert!(matches!(
        inbox::first_problem(&dir, &config::Config::default()).unwrap(),
        Some(inbox::FirstProblem::File { .. })
    ));
}

#[test]
fn deposit_only_headerless_savings_keeps_its_signs() {
    // Withdrawals column (3) is empty in every row; deposits are column 4.
    let savings = [
        "10/15/2026,TRANSFER FROM CHEQUING CANARY60,,500.00,10500.00",
        "10/28/2026,INTEREST PAID CANARY61,,12.00,10512.00",
        "11/15/2026,TRANSFER FROM CHEQUING CANARY60,,500.00,11012.00",
        "11/28/2026,INTEREST PAID CANARY61,,12.00,11024.00",
        "12/15/2026,TRANSFER FROM CHEQUING CANARY60,,500.00,11524.00",
        "12/28/2026,INTEREST PAID CANARY61,,12.00,11536.00",
    ]
    .join("\n");
    let dir = inbox_dir("savings", &[("savings.csv", savings)]);
    let inbox = inbox::load_dir(&dir, &config::Config::default()).unwrap();
    let f = &inbox.files[0];
    assert!(
        f.notes[0].contains("debit=3, credit=4, balance=5"),
        "{:?}",
        f.notes
    );
    assert!(
        !f.notes.iter().any(|n| n.contains("signs flipped")),
        "{:?}",
        f.notes
    );
    assert!(inbox.txns.iter().all(|t| t.cents > 0));
}

#[test]
fn mostly_positive_amounts_flip_only_for_card_like_accounts() {
    let rows = |name: &'static str| {
        let mut r = vec!["Date,Description,Amount".to_string()];
        for d in 1..=6 {
            r.push(format!("2026-09-{d:02},DEPOSIT CANARY62,{d}0.00"));
        }
        r.push("2026-09-20,FEE CANARY63,-1.00".to_string());
        (name, r.join("\n"))
    };
    let dir = inbox_dir("signs", &[rows("savings-account.csv"), rows("my-card.csv")]);
    let inbox = inbox::load_dir(&dir, &config::Config::default()).unwrap();
    let notes = |name: &str| {
        inbox
            .files
            .iter()
            .find(|f| f.file_name == name)
            .unwrap()
            .notes
            .join("; ")
    };
    assert!(notes("my-card.csv").contains("signs flipped"));
    assert!(!notes("savings-account.csv").contains("signs flipped"));
    assert!(
        notes("savings-account.csv").contains("invert = true"),
        "{}",
        notes("savings-account.csv")
    );
}

#[test]
fn uncategorized_check_ignores_pay_and_transfers() {
    let (s, _, _) = run("uncat", &bank_and_card(), "");
    let n = s
        .checks
        .iter()
        .find(|c| c.name.starts_with("rows left uncategorized"))
        .map(|c| c.count);
    assert_eq!(n, Some(1), "only the e-transfer deposit is uncategorized");
}
