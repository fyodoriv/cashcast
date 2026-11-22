//! Bank PDF statements (`engine/src/pdf.rs`): TD credit card, TD chequing,
//! Scotiabank Visa and Scotiabank chequing. Each layout is synthetic text as
//! `pdftotext -layout` prints it; every private field holds a CANARY.
//! The round trip builds a real PDF and needs pdftotext (skipped without it).

use std::path::{Path, PathBuf};

use cashcast_engine::export::{Export, LoadOptions};
use cashcast_engine::{config, inbox, ledger, pdf};

const TD_CREDIT: &str = "\
TD Aeroplan Visa Infinite Card
CANARY HOLDER NAME
Account Number: 4520 XXXX XXXX 4321
STATEMENT PERIOD: December 12, 2023 to January 11, 2024
Credit Limit                                             $5,000.00
TRANSACTION   POSTING
DATE          DATE       ACTIVITY DESCRIPTION                      AMOUNT($)
                         PREVIOUS STATEMENT BALANCE                $1,000.00
DEC 14        DEC 15     CANARY COFFEE SHOP TORONTO ON             $12.34      Minimum Payment     $10.00
DEC 30        JAN 2      PAYMENT - THANK YOU                      -$1,000.00
JAN 5         JAN 6      CANARY GROCER                             $87.66
                         FOREIGN CURRENCY 64.00 USD
                         NEW BALANCE                               $100.00
";

const TD_CHEQUING: &str = "\
TD Canada Trust
CANARY HOLDER NAME
Account No. 1234-5015678
Statement From - To    DEC 29/23 - JAN 31/24
Description                  Withdrawals      Deposits      Date        Balance
STARTING BALANCE                                            DEC29      2,000.00
CANARY PAYROLL                                1,500.00      JAN02      3,500.00
TFR-TO C/C CANARY                 500.00                    JAN03
CANARY RENT                     1,200.00                    JAN03      1,800.00
MONTHLY FEE                         4.95                    JAN31      1,795.05
CLOSING BALANCE                                             JAN31      1,795.05
";

const SCOTIA_VISA: &str = "\
Scotiabank Scene+ Visa card
CANARY HOLDER NAME
Card number 4538 XXXX XXXX 8765
Statement Period Nov 12, 2023 - Dec 11, 2023
Credit limit                                                $5,000.00
REF.#  TRANS.   POST
       DATE     DATE    DETAILS                                AMOUNT($)
       Previous balance                                         250.00
001    Nov 14   Nov 15  CANARY BOOKS                              20.00
002    Nov 20   Nov 20  PAYMENT FROM - *****12*3456              250.00-
003    Dec 1    Dec 2   CANARY TRANSIT                             5.50
       New Balance                                               25.50
";

const SCOTIA_CHEQUING: &str = "\
Scotiabank
CANARY HOLDER NAME
Ultimate Package    Account number: 01234 0002468
Statement Period Dec 1, 2023 to Dec 31, 2023
Date      Transactions                    Amounts withdrawn ($)   Amounts deposited ($)   Balance ($)
Dec 1     Opening Balance                                                                   1,000.00
Dec 4     POINT OF SALE PURCHASE                         45.67                                954.33
          CANARY STORE
Dec 15    PAYROLL DEPOSIT                                                        2,000.00   2,954.33
          CANARY EMPLOYER
Dec 20    CANARY E-TRANSFER                             100.00                              2,854.33
Dec 31    Closing Balance                                                                   2,854.33
";

fn parse(text: &str) -> Export {
    pdf::parse(text, "statement.pdf", "statement", &LoadOptions::default())
        .unwrap_or_else(|e| panic!("{e}"))
}

fn rows(e: &Export) -> Vec<(String, i64)> {
    e.txns
        .iter()
        .map(|t| (t.date.to_string(), t.cents))
        .collect()
}

fn account(e: &Export) -> &str {
    e.txns[0].account.as_str()
}

#[test]
fn td_credit_card_rows_signs_and_year_rollover() {
    let e = parse(TD_CREDIT);
    assert_eq!(e.format, pdf::TD_CREDIT);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    // December rows on a January statement are from the year before.
    assert_eq!(
        rows(&e),
        [
            ("2023-12-14".into(), -1_234),
            ("2023-12-30".into(), 100_000),
            ("2024-01-05".into(), -8_766),
        ]
    );
    // Same name as the TD QFX download, so it links to the same account.
    assert_eq!(account(&e), "Credit card …4321");
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );
    // A summary box beside a row never leaks into its description.
    assert_eq!(e.txns[0].text, "CANARY COFFEE SHOP TORONTO ON");
}

#[test]
fn a_rewards_box_previous_balance_does_not_break_the_check() {
    // TD prints a cash back box above the rows with its own "Previous
    // Balance"; the card's own balance is the one in the row list.
    let text = TD_CREDIT.replace(
        "Credit Limit ",
        "CANARY CASH BACK DOLLARS                          Previous Balance      +   $7.00\n\
         Credit Limit ",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 3);
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );

    // The box can sit on the period line itself; it is the only period line.
    // A statement-date line above names two dates too, but holds nothing.
    let text = text.replace(
        "STATEMENT PERIOD: December 12, 2023 to January 11, 2024\n",
        "STATEMENT DATE: January 11, 2024     PREVIOUS STATEMENT: December 11, 2023\n\
         STATEMENT PERIOD: December 12, 2023 to January 11, 2024      Cash Back Dollars   =   $7.00\n",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert!(
        e.notes[0].contains("1 statement(s), balances checked on 1"),
        "{:?}",
        e.notes
    );
}

#[test]
fn a_card_row_whose_amount_wraps_to_the_next_line_reads() {
    // A long description pushes the amount onto its own line below.
    let text = TD_CREDIT.replace(
        "JAN 5         JAN 6      CANARY GROCER                             $87.66\n",
        "JAN 5         JAN 6      CANARY GROCER*AB12CD34EF56\n\
         \x20                                                        $87.66\n",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 3);
    assert_eq!(e.txns[2].cents, -8_766);
    assert_eq!(e.txns[2].text, "CANARY GROCER*AB12CD34EF56");
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );

    // A second description line may sit between the row and its amount.
    let text = TD_CREDIT.replace(
        "JAN 5         JAN 6      CANARY GROCER                             $87.66\n",
        "JAN 5         JAN 6      CANARY GROCER*AB12CD34EF56\n\
         \x20                        WWW.CANARY.CA\n\
         \x20                                                        $87.66\n",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns[2].cents, -8_766);
    assert_eq!(e.txns[2].text, "CANARY GROCER*AB12CD34EF56 WWW.CANARY.CA");
}

#[test]
fn td_chequing_signs_from_running_balance_and_columns() {
    let e = parse(TD_CHEQUING);
    assert_eq!(e.format, pdf::TD_CHEQUING);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(
        rows(&e),
        [
            ("2024-01-02".into(), 150_000),
            ("2024-01-03".into(), -50_000),
            ("2024-01-03".into(), -120_000),
            ("2024-01-31".into(), -495),
        ]
    );
    assert_eq!(account(&e), "Checking …5678");
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );
}

#[test]
fn td_chequing_reads_the_boxed_header_with_labels_above_values() {
    // The real layout: labels on one line, values two lines below, and the
    // period dates under a "Statement From - To" label with a blank between.
    let text = TD_CHEQUING.replace(
        "Account No. 1234-5015678\nStatement From - To    DEC 29/23 - JAN 31/24\n",
        "Statement of Account          CANARY HOLDER       Statement From - To\n\
         \n\
         \x20 Branch No.     Account No.                    DEC 29/23 - JAN 31/24\n\
         \x20                                    CANARY\n\
         \x20     9999       1234-5015678                    Page 1 of 1\n",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 4);
    assert_eq!(account(&e), "Checking …5678");
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );
}

#[test]
fn scotiabank_visa_trailing_minus_is_a_payment() {
    let e = parse(SCOTIA_VISA);
    assert_eq!(e.format, pdf::SCOTIA_VISA);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(
        rows(&e),
        [
            ("2023-11-14".into(), -2_000),
            ("2023-11-20".into(), 25_000),
            ("2023-12-01".into(), -550),
        ]
    );
    assert_eq!(account(&e), "Credit card …8765");
}

#[test]
fn scotiabank_visa_row_with_its_details_on_the_next_line_reads() {
    // A long description moves the details and the amount under the dates.
    let text = SCOTIA_VISA.replace(
        "003    Dec 1    Dec 2   CANARY TRANSIT                             5.50\n",
        "003    Dec 1    Dec 2\n\
         \x20                       CANARY TRANSIT & PARKING AUTHORITY     5.50\n",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 3);
    assert_eq!(e.txns[2].cents, -550);
    assert_eq!(e.txns[2].text, "CANARY TRANSIT & PARKING AUTHORITY");
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );
}

#[test]
fn a_foreign_amount_in_the_details_is_not_the_row_amount() {
    // The amount is the one under the AMOUNT($) header, not the first one.
    let text = SCOTIA_VISA.replace(
        "003    Dec 1    Dec 2   CANARY TRANSIT                             5.50\n",
        "003    Dec 1    Dec 2   CANARY TRANSIT USD 4.00                    5.50\n",
    );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns[2].cents, -550);
    assert_eq!(e.txns[2].text, "CANARY TRANSIT USD 4.00");

    // A stray page-margin digit before the reference still reads.
    let text = SCOTIA_VISA.replace("\n003    Dec 1", "\n0012345  003    Dec 1");
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 3);
}

#[test]
fn scotiabank_chequing_joins_two_line_descriptions() {
    let e = parse(SCOTIA_CHEQUING);
    assert_eq!(e.format, pdf::SCOTIA_CHEQUING);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(
        rows(&e),
        [
            ("2023-12-04".into(), -4_567),
            ("2023-12-15".into(), 200_000),
            ("2023-12-20".into(), -10_000),
        ]
    );
    assert_eq!(e.txns[0].text, "POINT OF SALE PURCHASE CANARY STORE");
    assert_eq!(account(&e), "Checking …2468");
}

#[test]
fn scotiabank_chequing_takes_its_period_from_the_balance_lines() {
    // The e-statement names no period: the summary says "Opening balance on"
    // and "Closing balance on" a date, and the account number sits under its
    // label, a few columns to the left, in three groups.
    let text = SCOTIA_CHEQUING
        .replace(
            "Ultimate Package    Account number: 01234 0002468\n",
            "Ultimate Package                    Your account number:\n\
             \x20                              01234 00024 68\n",
        )
        .replace(
            "Statement Period Dec 1, 2023 to Dec 31, 2023\n",
            "Opening balance on Nov 30, 2023                     1,000.00\n\
             \x20  Minus total withdrawals                         145.67\n\
             \x20  Plus total deposits                           2,000.00\n\
             Closing balance on Dec 31, 2023                     2,854.33\n",
        );
    let e = parse(&text);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 3);
    assert_eq!(e.txns[0].date.to_string(), "2023-12-04");
    assert_eq!(account(&e), "Checking …2468");
    assert!(
        e.notes[0].contains("balances checked on 1"),
        "{:?}",
        e.notes
    );
}

#[test]
fn a_yearly_file_holds_several_statements() {
    let next = SCOTIA_VISA
        .replace("Nov 12, 2023 - Dec 11, 2023", "Dec 12, 2023 - Jan 11, 2024")
        .replace(
            "Previous balance                                         250.00",
            "Previous balance                                          25.50",
        )
        .replace("001    Nov 14   Nov 15", "001    Dec 14   Dec 15")
        .replace("002    Nov 20   Nov 20", "002    Dec 20   Dec 20")
        .replace("003    Dec 1    Dec 2 ", "003    Jan 3    Jan 4 ")
        .replace("250.00-", "25.50-")
        .replace(
            "New Balance                                               25.50",
            "New Balance                                               25.50",
        );
    let both = format!("{SCOTIA_VISA}\u{c}{next}");
    let e = parse(&both);
    assert!(e.bad_lines.is_empty(), "{:?}", e.bad_lines);
    assert_eq!(e.txns.len(), 6);
    assert!(e.notes[0].starts_with("2 statement(s)"), "{:?}", e.notes);
    assert_eq!(e.txns[5].date.to_string(), "2024-01-03");
}

#[test]
fn a_statement_whose_balances_do_not_add_up_is_refused() {
    let wrong = TD_CREDIT.replace("$87.66", "$87.67");
    let err = pdf::parse(&wrong, "s.pdf", "s", &LoadOptions::default())
        .err()
        .expect("refused");
    assert!(err.contains("do not add up"), "{err}");
    assert!(err.contains("2023-12-12 to 2024-01-11"), "{err}");
    assert!(!err.contains("CANARY"), "{err}");
    // A chequing row whose sign the balance cannot confirm is refused too.
    let wrong = TD_CHEQUING.replace("1,800.00", "1,900.00");
    assert!(pdf::parse(&wrong, "s.pdf", "s", &LoadOptions::default()).is_err());
}

#[test]
fn other_pdfs_are_not_statements() {
    let err = pdf::parse(
        "Boarding pass\nCANARY\n",
        "x.pdf",
        "x",
        &LoadOptions::default(),
    )
    .err()
    .unwrap();
    assert!(err.contains("not a TD or Scotiabank statement"), "{err}");
}

#[test]
fn columns_prints_masked_shapes_only() {
    for text in [TD_CREDIT, TD_CHEQUING, SCOTIA_VISA, SCOTIA_CHEQUING] {
        let out = pdf::columns_text(text, "s.pdf", "s");
        assert!(!out.contains("CANARY"), "{out}");
        for value in ["87.66", "1,795.05", "250.00", "2,854.33", "4520", "5015678"] {
            assert!(!out.contains(value), "{value} leaked: {out}");
        }
        assert!(out.contains("[row]"), "{out}");
        assert!(out.contains("[header]"), "{out}");
    }
}

// ------------------------------------------------------------ real PDFs

/// A one-page PDF that prints `text` in Courier, one line per text line.
fn write_pdf(path: &Path, text: &str) {
    let mut content = String::from("BT /F1 8 Tf 10 TL 20 770 Td\n");
    for line in text.lines() {
        let esc = line
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        content.push_str(&format!("({esc}) Tj T*\n"));
    }
    content.push_str("ET\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 700 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    std::fs::write(path, out).unwrap();
}

/// The TD card's QFX: the newest rows, overlapping the statement's last row.
const TD_QFX: &str = "OFXHEADER:100\n<OFX><CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><CURDEF>CAD\
<CCACCTFROM><ACCTID>4520000000004321</CCACCTFROM><BANKTRANLIST>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20240105<TRNAMT>-87.66<FITID>A1<NAME>CANARY GROCER</STMTTRN>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20240109<TRNAMT>-3.00<FITID>A2<NAME>CANARY KIOSK</STMTTRN>\
</BANKTRANLIST></CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>\n";

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cashcast-pdf-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn pdf_statements_join_the_inbox_and_yield_to_csv_days() {
    if !pdf::tool_available() {
        eprintln!("skipped: pdftotext is not installed");
        return;
    }
    let dir = scratch("inbox");
    write_pdf(&dir.join("View PDF Statement_2024-01-11.pdf"), TD_CREDIT);
    // The same statement downloaded twice adds no rows.
    write_pdf(
        &dir.join("View PDF Statement_2024-01-11 (1).pdf"),
        TD_CREDIT,
    );
    write_pdf(&dir.join("Scotia_statements_2023.pdf"), SCOTIA_CHEQUING);
    std::fs::write(dir.join("td.qfx"), TD_QFX).unwrap();
    let cfg = config::parse(
        "[moneywiz.export_aliases]\n\"Checking …2468\" = \"Ultimate_Package_2468\"\n",
    )
    .unwrap();
    let inbox = inbox::load_exports(&dir, &cfg).unwrap();
    assert!(inbox.failed.is_empty(), "{:?}", inbox.failed);
    let mut got: Vec<(String, String, i64)> = inbox
        .txns
        .iter()
        .map(|t| (t.account.clone(), t.date.to_string(), t.cents))
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("Credit card …4321".into(), "2023-12-14".into(), -1_234),
            ("Credit card …4321".into(), "2023-12-30".into(), 100_000),
            // The QFX covers 2024-01-05 to 2024-01-09: its rows win there.
            ("Credit card …4321".into(), "2024-01-05".into(), -8_766),
            ("Credit card …4321".into(), "2024-01-09".into(), -300),
            ("Ultimate_Package_2468".into(), "2023-12-04".into(), -4_567),
            ("Ultimate_Package_2468".into(), "2023-12-15".into(), 200_000),
            ("Ultimate_Package_2468".into(), "2023-12-20".into(), -10_000),
        ]
    );
    let qfx_rows = inbox
        .txns
        .iter()
        .filter(|t| inbox.files[t.file].format == "OFX")
        .count();
    assert_eq!(qfx_rows, 2, "the QFX keeps both of its rows");

    // `columns` describes a real PDF by masked shapes only.
    let out = ledger::columns(&dir.join("View PDF Statement_2024-01-11.pdf"));
    assert!(out.contains(pdf::TD_CREDIT), "{out}");
    assert!(out.contains("[row]"), "{out}");
    assert!(!out.contains("CANARY"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn collect_moves_bank_statements_and_leaves_other_pdfs() {
    if !pdf::tool_available() {
        eprintln!("skipped: pdftotext is not installed");
        return;
    }
    let root = scratch("collect");
    let (src, dest) = (root.join("dl"), root.join("inbox"));
    std::fs::create_dir_all(&src).unwrap();
    write_pdf(&src.join("statement.pdf"), SCOTIA_VISA);
    write_pdf(&src.join("ticket.pdf"), "Boarding pass\nCANARY\n");
    let moved = cashcast_engine::tasks::fetch::collect(&dest, &src, 60).unwrap();
    assert_eq!(moved, [(".pdf".to_string(), 1)]);
    assert!(dest.join("statement.pdf").exists());
    assert!(src.join("ticket.pdf").exists());
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------ E*TRADE

/// An E*TRADE stock plan statement over two pages: a quarter that crosses
/// the new year, a sale with a comment line, wires, a fee and a dividend.
const ETRADE_PLAN: &str = "\
CANARY HOLDER NAME
CLIENT STATEMENT     For the Period November 1- January 31, 2025          Page 1 of 3
Account Detail     123-456789-321
Cash, BDP, MMFs        $1.00     $72.33        OPENING CASH, BDP, MMFs       $72.33     —
CASH FLOW ACTIVITY BY DATE
Activity Settlement
Date     Date        Activity Type         Description            Comments      Quantity      Price     Credits/(Debits)
 11/14   11/15       Sold                  CANARY CORP            CANARY LOT    (10.000)      $50.00         499.95
                                                                  CANARY FEE NOTE
 11/18               Funds Transferred     WIRE OUT                                                        (480.00)
 12/2                Service Fee           CANARY WIRE FEE                                                  (25.00)
CANARY HOLDER NAME
CLIENT STATEMENT     For the Period November 1- January 31, 2025          Page 2 of 3
CASH FLOW ACTIVITY BY DATE (CONTINUED)
 1/10                Qualified Dividend    CANARY CORP                                                       12.34
 1/10                Tax Withholding       CANARY CORP                                                       -1.85
NET CREDITS/(DEBITS)                                                                                         $5.44
MONEY MARKET FUND (MMF) AND BANK DEPOSIT PROGRAM ACTIVITY
 11/15               Automatic Investment  BANK DEPOSIT PROGRAM                                            (499.95)
SECURITY TRANSFERS
 12/20               Transfer into Account CANARY CORP                          5.000
CLOSING CASH, BDP, MMFs                                                                                      $77.77
";

#[test]
fn etrade_stock_plan_statement_reads_cash_flow_rows() {
    let e = parse(ETRADE_PLAN);
    assert_eq!(e.format, pdf::ETRADE_STOCKPLAN);
    assert_eq!(
        rows(&e),
        [
            ("2024-11-14".into(), 49_995),
            ("2024-11-18".into(), -48_000),
            ("2024-12-02".into(), -2_500),
            ("2025-01-10".into(), 1_234),
            ("2025-01-10".into(), -185),
        ]
    );
    assert_eq!(account(&e), "E*TRADE Stock Plan …9321");
    let texts: Vec<&str> = e.txns.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "sold",
            "funds transferred",
            "service fee",
            "qualified dividend",
            "tax withholding"
        ]
    );
    assert!(e.txns.iter().all(|t| t.currency == "USD"));
    // The plan id links the export to `[moneywiz.investments]`.
    let ids = &e.ledger_ids["E*TRADE Stock Plan …9321"];
    assert!(ids.contains(&("non-registered".to_string(), "123456789321".to_string())));
    assert!(
        e.payee_labels.values().all(|l| !l.contains("CANARY")),
        "a description leaked"
    );
}

#[test]
fn etrade_statement_closing_cash_is_the_balance_when_the_rows_lead_to_it() {
    let e = parse(ETRADE_PLAN);
    let acct = "E*TRADE Stock Plan …9321";
    let b = &e.balances[acct];
    assert_eq!(
        (b.cents, b.as_of.map(|d| d.to_string())),
        (7_777, Some("2025-01-31".into()))
    );
    assert_eq!(e.statements.len(), 1);
    let s = &e.statements[0];
    assert_eq!(
        (s.start.to_string(), s.end.to_string(), s.opening, s.closing),
        ("2024-11-01".into(), "2025-01-31".into(), 7_233, 7_777)
    );
    // A dash is no cash: the opening is 0, and 0 + 5.44 misses 77.77.
    let dash = ETRADE_PLAN.replace(
        "OPENING CASH, BDP, MMFs       $72.33",
        "OPENING CASH, BDP, MMFs       —",
    );
    let e = parse(&dash);
    assert!(e.balances.is_empty() && e.statements.is_empty());
    assert!(
        e.notes.iter().any(|n| n.contains("miss the closing")),
        "{:?}",
        e.notes
    );
    // No closing line: rows only, no balance.
    let none = ETRADE_PLAN.replace("CLOSING CASH, BDP, MMFs", "CANARY TOTAL");
    let e = parse(&none);
    assert_eq!(e.txns.len(), 5);
    assert!(e.balances.is_empty());
}

#[test]
fn etrade_monthly_statement_names_its_month_once() {
    let monthly = ETRADE_PLAN
        .replace("November 1- January 31, 2025", "December 1-31, 2024")
        .replace(" 11/14   11/15", " 12/14   12/15")
        .replace(" 11/18", " 12/18")
        .replace(" 1/10 ", " 12/30");
    let dates: Vec<String> = rows(&parse(&monthly)).into_iter().map(|r| r.0).collect();
    assert_eq!(
        dates,
        [
            "2024-12-14",
            "2024-12-18",
            "2024-12-02",
            "2024-12-30",
            "2024-12-30"
        ]
    );
}

#[test]
fn etrade_statement_whose_rows_miss_the_net_is_refused() {
    let wrong = ETRADE_PLAN.replace("(25.00)", "(26.00)");
    let err = pdf::parse(&wrong, "s.pdf", "s", &LoadOptions::default())
        .err()
        .unwrap();
    assert!(err.contains("do not add up"), "{err}");
    // A row split over two lines is missed, so the net catches it too.
    let split = ETRADE_PLAN.replace(
        "WIRE OUT                                                        (480.00)",
        "WIRE OUT\n                                                                                    (480.00)",
    );
    assert!(pdf::parse(&split, "s.pdf", "s", &LoadOptions::default()).is_err());
}

#[test]
fn etrade_statement_columns_print_masked_shapes_only() {
    let out = pdf::columns_text(ETRADE_PLAN, "s.pdf", "s");
    assert!(out.contains(pdf::ETRADE_STOCKPLAN), "{out}");
    assert!(out.contains("[row]"), "{out}");
    assert!(!out.contains("CANARY"), "{out}");
    assert!(out.contains("[cash]"), "{out}");
    for value in ["499.95", "480.00", "456789", "12.34", "72.33", "77.77"] {
        assert!(!out.contains(value), "{value} leaked: {out}");
    }
}

#[test]
fn etrade_statement_feeds_the_sync_not_the_cash_import() {
    use cashcast_engine::moneywiz_invest::spec_takes;
    let plan = "etrade-benefit-espp:non-registered, etrade-gains-losses:non-registered";
    assert!(spec_takes(
        plan,
        pdf::ETRADE_STOCKPLAN,
        "non-registered",
        "123456789321"
    ));
    assert!(!spec_takes(
        plan,
        pdf::ETRADE_STOCKPLAN,
        "TFSA",
        "123456789321"
    ));
    assert!(!spec_takes(
        "wealthsimple-activity-csv:non-registered",
        pdf::ETRADE_STOCKPLAN,
        "non-registered",
        "123456789321"
    ));
    if !pdf::tool_available() {
        eprintln!("skipped the PDF part: pdftotext is not installed");
        return;
    }
    let dir = scratch("etrade");
    write_pdf(&dir.join("ClientStatements_1.pdf"), ETRADE_PLAN);
    write_pdf(&dir.join("scotia.pdf"), SCOTIA_VISA);
    let cfg = config::parse("").unwrap();
    let sync = inbox::load_exports(&dir, &cfg).unwrap();
    assert!(sync.failed.is_empty(), "{:?}", sync.failed);
    let etrade = |i: &inbox::Inbox| {
        i.txns
            .iter()
            .filter(|t| t.account.starts_with("E*TRADE"))
            .count()
    };
    assert_eq!(etrade(&sync), 5);
    let cash = inbox::load_dir(&dir, &cfg).unwrap();
    assert_eq!(etrade(&cash), 0, "the cash import skips it");
    assert!(
        cash.ledger_files
            .iter()
            .any(|(_, f)| f == pdf::ETRADE_STOCKPLAN)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn td_month(period: &str, open: &str, fee_day: &str, close: &str) -> String {
    format!(
        "\
TD Canada Trust
CANARY HOLDER NAME
Account No. 1234-5015678
Statement From - To    {period}
Description                  Withdrawals      Deposits      Date        Balance
STARTING BALANCE                                            {fee_day}      {open}
MONTHLY FEE                         4.95                    {fee_day}      {close}
CLOSING BALANCE                                             {fee_day}      {close}
"
    )
}

#[test]
fn statements_that_do_not_chain_name_the_missing_month() {
    let jan = parse(TD_CHEQUING);
    let feb = parse(&td_month(
        "FEB 01/24 - FEB 29/24",
        "1,795.05",
        "FEB29",
        "1,790.10",
    ));
    // March is missing: April opens where March closed, not where February did.
    let apr = parse(&td_month(
        "APR 01/24 - APR 30/24",
        "1,700.00",
        "APR30",
        "1,695.05",
    ));
    let name = account(&jan).to_string();
    assert_eq!(jan.statements.len(), 1);
    assert_eq!(
        (jan.statements[0].opening, jan.statements[0].closing),
        (200_000, 179_505)
    );
    let chained = inbox::Inbox::merge(vec![parse(TD_CHEQUING), feb], Vec::new());
    let sts = cashcast_engine::moneywiz_diagnose::statements(&chained, &name);
    assert_eq!(sts.len(), 2);
    assert!(cashcast_engine::moneywiz_diagnose::breaks(&sts).is_empty());

    let feb = parse(&td_month(
        "FEB 01/24 - FEB 29/24",
        "1,795.05",
        "FEB29",
        "1,790.10",
    ));
    let gap = inbox::Inbox::merge(vec![jan, feb, apr], Vec::new());
    let sts = cashcast_engine::moneywiz_diagnose::statements(&gap, &name);
    let breaks = cashcast_engine::moneywiz_diagnose::breaks(&sts);
    assert_eq!(breaks.len(), 1);
    assert_eq!(breaks[0].2, 1_700_00 - 1_790_10);
    assert_eq!(
        cashcast_engine::moneywiz_diagnose::missing_days(breaks[0]),
        "2024-03-01 to 2024-03-31"
    );
}

#[test]
fn a_missing_month_moneywiz_accounts_for_holds_nothing() {
    use cashcast_engine::moneywiz_diagnose::{breaks, statements, unexplained_breaks};
    let feb = parse(&td_month(
        "FEB 01/24 - FEB 29/24",
        "20,000.00",
        "FEB29",
        "19,995.05",
    ));
    // The bank never issued March: April opens 14,995.05 lower.
    let apr = parse(&td_month(
        "APR 01/24 - APR 30/24",
        "5,000.00",
        "APR30",
        "4,995.05",
    ));
    let name = account(&feb).to_string();
    let gap = inbox::Inbox::merge(vec![feb, apr], Vec::new());
    let sts = statements(&gap, &name);
    assert_eq!(breaks(&sts).len(), 1);
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    // The owner typed March's rows (a few days off, 11.00 short): from the
    // day before February to the end of April MoneyWiz moves as the bank did.
    let typed = |d: chrono::NaiveDate| {
        if d < day("2024-02-01") {
            2_000_000
        } else if d < day("2024-03-05") {
            1_999_505
        } else {
            500_605
        }
    };
    assert!(unexplained_breaks(&sts, typed).is_empty());
    // MoneyWiz lacks March: it still moves only by the two fees.
    let lacking = |d: chrono::NaiveDate| {
        if d < day("2024-02-01") {
            2_000_000
        } else {
            1_999_010
        }
    };
    let left = unexplained_breaks(&sts, lacking);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].2, 500_000 - 1_999_505);
}

#[test]
fn a_card_statement_keeps_its_balances_as_the_account_balance() {
    let e = parse(TD_CREDIT);
    // Owed 1,000.00 then 100.00: the account stands at -1,000.00, then -100.00.
    assert_eq!(e.statements.len(), 1);
    assert_eq!(
        (e.statements[0].opening, e.statements[0].closing),
        (-100_000, -10_000)
    );
}

// ------------------------------------------------------- Sun Life statements

/// A Sun Life half-year statement as `pdftotext -layout` prints it: the
/// investments table splits over a page break with its header repeated, and
/// a note names another plan in passing.
const SUNLIFE: &str = "\
Sun Life Financial
CANARY HOLDER NAME
Group Retirement Savings Plan
Your statement for January 1, 2026 to June 30, 2026
Contributions to a Deferred Profit Sharing Plan are reported separately.
Your investments
                                NUMBER OF UNITS   PRICE ON June 30, 2026   VALUE ON June 30, 2026
CANARY Equity Index Fund              12.3456                 $20.0000                  $246.91
CANARY Bond Fund                       4.0000                 $10.5000                   $42.00
Page 1 of 2
                                NUMBER OF UNITS   PRICE ON June 30, 2026   VALUE ON June 30, 2026
CANARY Money Market Fund               1.5000                  $2.0000                    $3.00
Total investments                                                                        $291.91
";

#[test]
fn sunlife_statement_reads_units_over_a_page_break() {
    let s = pdf::sunlife::read(SUNLIFE, "s.pdf").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.as_of.to_string(), "2026-06-30");
    assert_eq!(
        s.plan, "RRSP",
        "a passing mention of another plan does not win"
    );
    let funds: Vec<(&str, f64, i64)> = s
        .funds
        .iter()
        .map(|f| (f.name.as_str(), f.units, f.value_cents))
        .collect();
    assert_eq!(
        funds,
        [
            ("CANARY Equity Index Fund", 12.3456, 24691),
            ("CANARY Bond Fund", 4.0, 4200),
            ("CANARY Money Market Fund", 1.5, 300),
        ]
    );
    assert_eq!(s.total_cents, 29191);
    // As an export: no rows, one note with the day.
    let e = parse(SUNLIFE);
    assert_eq!(e.format, pdf::SUNLIFE_STATEMENT);
    assert!(e.txns.is_empty());
    assert!(
        e.notes
            .iter()
            .any(|n| n.contains("3 fund(s) on 2026-06-30 (RRSP)")),
        "{:?}",
        e.notes
    );
}

#[test]
fn sunlife_statement_whose_funds_miss_the_total_is_refused() {
    let wrong = SUNLIFE.replace("$291.91", "$391.91");
    let err = pdf::sunlife::read(&wrong, "s.pdf").err().unwrap();
    assert!(err.contains("do not add up"), "{err}");
    let wrong = SUNLIFE
        .replace("$42.00", "$52.00")
        .replace("$291.91", "$301.91");
    let err = pdf::sunlife::read(&wrong, "s.pdf").err().unwrap();
    assert!(err.contains("units times price"), "{err}");
    let wrong = SUNLIFE.replacen("June 30, 2026   VALUE", "May 31, 2026   VALUE", 1);
    assert!(pdf::sunlife::read(&wrong, "s.pdf").is_err());
}

#[test]
fn sunlife_statement_columns_print_masked_shapes_only() {
    let out = pdf::columns_text(SUNLIFE, "s.pdf", "s");
    assert!(out.contains("3 fund(s) on 2026-06-30 (RRSP)"), "{out}");
    for value in ["CANARY", "12.3456", "246.91", "291.91"] {
        assert!(!out.contains(value), "{value} leaked: {out}");
    }
}

const SCOTIA_SSI: &str = "\
Scotia Smart Investor
CANARY HOLDER NAME
CANARY GIC     issued December 5, 2023     matures December 5, 2025
Your statement for the period    October 1, 2025      to      December 31, 2025
Your plan at a glance
                                 From October 1, to
                                 December 31, 2025         Year to date ($)
Opening market value                  3,601.11                  3,521.00
+ Deposits                               30.42                    130.42
- Withdrawals                         3,630.42                  3,630.42
= Net invested                           1.11                     21.00
+ Change in market value                 1.11-                    21.00-
= Closing market value                   0.00                      0.00
Details of your transactions
DATE                ACTIVITY         INVESTMENT                       # UNITS   PRICE ($)   AMOUNT ($)
December 5, 2025    *Withdrawal      CANARY CASH                                            3,630.42-
TFSA                #000123454321
";

#[test]
fn scotia_smart_investor_statement_reads_its_market_value_and_no_rows() {
    let e = parse(SCOTIA_SSI);
    assert_eq!(e.format, pdf::SCOTIA_SSI);
    assert!(e.txns.is_empty(), "the trades stay out of the sync");
    let st: Vec<(String, String, String, i64, i64)> = e
        .statements
        .iter()
        .map(|s| {
            (
                s.account.clone(),
                s.start.to_string(),
                s.end.to_string(),
                s.opening,
                s.closing,
            )
        })
        .collect();
    assert_eq!(
        st,
        [(
            "TFSA …4321".to_string(),
            "2025-10-01".to_string(),
            "2025-12-31".to_string(),
            360111,
            0
        )]
    );
    let b = &e.balances["TFSA …4321"];
    assert_eq!(
        (b.cents, b.as_of.map(|d| d.to_string())),
        (0, Some("2025-12-31".into()))
    );
}

#[test]
fn scotia_smart_investor_quarter_without_transactions_still_reads() {
    let quiet = SCOTIA_SSI
        .split("Details of your transactions")
        .next()
        .unwrap()
        .to_string()
        + "TFSA                #000123454321\n";
    let e = parse(&quiet);
    assert_eq!(e.format, pdf::SCOTIA_SSI);
    assert_eq!(e.statements.len(), 1);
}

/// The first statement has no period line; its summary header names the day.
#[test]
fn scotia_smart_investor_first_statement_runs_from_its_quarter_start() {
    let first = SCOTIA_SSI
        .replace(
            "Your statement for the period    October 1, 2025      to      December 31, 2025\n",
            "",
        )
        .replace(
            "Opening market value                  3,601.11",
            "Opening market value                      0.00",
        )
        .replace(
            "+ Deposits                               30.42",
            "+ Deposits                            3,631.53",
        );
    let e = parse(&first);
    let s = &e.statements[0];
    assert_eq!(
        (s.start.to_string(), s.end.to_string(), s.opening),
        ("2025-10-01".into(), "2025-12-31".into(), 0)
    );
}

/// A mid-year statement shows the account value only: it is no chequing
/// statement, and it gives no balance.
#[test]
fn scotia_smart_investor_quarter_statement_without_a_summary_reads_nothing() {
    let quarter = "\
CANARY HOLDER NAME
ACCOUNT VALUE                                                      3,601.11
Details of your transactions
DATE                ACTIVITY         INVESTMENT                       # UNITS   PRICE ($)   AMOUNT ($)
March 3, 2025       *Deposit         CANARY CASH                                              100.00
";
    let e = parse(quarter);
    assert_eq!(e.format, pdf::SCOTIA_SSI);
    assert!(e.txns.is_empty() && e.statements.is_empty() && e.balances.is_empty());
    // A quarter with no transactions still shows its holdings table.
    let still = "\
CANARY HOLDER NAME
ACCOUNT VALUE                                                      3,601.11
TYPE OF INVESTMENT        CANARY GIC             Cash and Savings
";
    assert_eq!(parse(still).format, pdf::SCOTIA_SSI);
}

#[test]
fn scotia_smart_investor_statement_whose_summary_misses_its_close_is_refused() {
    let wrong = SCOTIA_SSI.replace(
        "+ Deposits                               30.42",
        "+ Deposits                               40.42",
    );
    let err = pdf::parse(&wrong, "s.pdf", "s", &LoadOptions::default())
        .err()
        .unwrap();
    assert!(err.contains("does not add up"), "{err}");
}

#[test]
fn scotia_smart_investor_columns_print_masked_shapes_only() {
    let out = pdf::columns_text(SCOTIA_SSI, "s.pdf", "s");
    assert!(
        out.contains(
            "(scotiabank-ssi-pdf): 1 statement 2025-10-01 to 2025-12-31; account TFSA …4321"
        ),
        "{out}"
    );
    for value in ["CANARY", "3,601.11", "3,630.42", "123454321"] {
        assert!(!out.contains(value), "{value} leaked: {out}");
    }
}
