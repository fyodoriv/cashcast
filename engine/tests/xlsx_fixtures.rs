//! Builds the synthetic .xlsx ledger fixtures. Every private field is a CANARY.
//! The test checks that the committed workbooks hold the same cells as this
//! builder. To rebuild them: `XLSX_FIXTURES_WRITE=1` with this test.

use cashcast_engine::forecast::book::dump_xlsx;
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet};
use std::path::{Path, PathBuf};

enum C {
    S(&'static str),
    N(f64),
    D(u16, u8, u8),
    E,
}
use C::{D, E, N, S};

fn row(sheet: &mut Worksheet, r: u32, cells: &[C]) {
    let date = Format::new().set_num_format("yyyy-mm-dd h:mm:ss");
    for (c, cell) in cells.iter().enumerate() {
        let c = c as u16;
        match cell {
            S(s) => {
                sheet.write_string(r, c, *s).unwrap();
            }
            N(n) => {
                sheet.write_number(r, c, *n).unwrap();
            }
            D(y, m, d) => {
                let dt = ExcelDateTime::from_ymd(*y, *m, *d).unwrap();
                sheet.write_datetime_with_format(r, c, &dt, &date).unwrap();
            }
            E => {}
        }
    }
}

fn sheet<'a>(wb: &'a mut Workbook, name: &str, rows: &[Vec<C>]) {
    let s = wb.add_worksheet();
    s.set_name(name).unwrap();
    for (r, cells) in rows.iter().enumerate() {
        row(s, r as u32, cells);
    }
}

fn stockplan(path: &Path) {
    let mut wb = Workbook::new();
    sheet(
        &mut wb,
        "Summary",
        &[vec![S("CANARY summary sheet without a ledger header")]],
    );
    sheet(
        &mut wb,
        "Lots",
        &[
            vec![
                S("Record Type"),
                S("Symbol"),
                S("Plan Type"),
                S("Date Acquired"),
                S("Qty"),
                S("Est. Market Value"),
                S("Vest Date"),
                S("Vest Date FMV"),
                S("Grant Number"),
            ],
            vec![
                S("Grant"),
                S("ACME"),
                S("RS"),
                E,
                E,
                E,
                E,
                E,
                S("CANARY-G001"),
            ],
            vec![
                S("Sellable"),
                S("ACME"),
                S("RS"),
                D(2025, 3, 15),
                N(8.0),
                N(1240.0),
                D(2025, 3, 15),
                N(140.0),
                S("CANARY-G001"),
            ],
            vec![
                S("Sellable"),
                S("ACME"),
                S("RS"),
                S("06/15/2025"),
                N(8.0),
                N(1240.0),
                S("06/15/2025"),
                N(145.5),
                S("CANARY-G002"),
            ],
            vec![S("Total"), E, E, E, N(16.0), N(2480.0), E, E, E],
        ],
    );
    wb.save(path).unwrap();
}

/// E*TRADE Benefit History: an ESPP sheet and a Restricted Stock sheet.
/// Restricted Stock repeats column names (grant summary first, vest schedule
/// last), like the real download.
fn benefit_history(path: &Path) {
    let mut wb = Workbook::new();
    sheet(
        &mut wb,
        "ESPP",
        &[
            [
                "Record Type",
                "Symbol",
                "Purchase Date",
                "Purchase Price",
                "Purchased Qty.",
                "Tax Collection Shares",
                "Net Shares",
                "Sellable Qty.",
                "Grant Date",
                "Purchase Date FMV",
                "Date",
                "Event Type",
                "Qty",
            ]
            .map(S)
            .into(),
            vec![
                S("Purchase"),
                S("ACME"),
                D(2025, 6, 30),
                N(127.5),
                N(3.0),
                N(0.0),
                N(3.0),
                N(1.0),
                D(2025, 1, 1),
                N(150.0),
                E,
                E,
                E,
            ],
            vec![
                S("Event"),
                E,
                E,
                E,
                E,
                E,
                E,
                E,
                E,
                E,
                D(2025, 6, 30),
                S("CANARY event"),
                N(3.0),
            ],
        ],
    );
    let grant = || [S("CANARY-G001")];
    let ev = |d: C, what: &'static str, q: f64| {
        let mut v = vec![S("Event"), E, E, E, E, E];
        v.extend(grant());
        v.extend([d, S(what), N(q)]);
        v
    };
    let vest = |period: f64, d: C, vested: f64, released: f64| {
        let mut v = vec![S("Vest Schedule"), E, E, N(0.0), E, E];
        v.extend(grant());
        v.extend([E, E, E, N(period), d, N(vested), N(released), N(0.0)]);
        v
    };
    let mut tax = vec![S("Tax Withholding"), E, E, E, E, E];
    tax.extend(grant());
    tax.extend([E, E, E, N(1.0), E, E, E, E, S("CANARY tax 1.0"), N(1400.0)]);
    let mut first = vec![
        S("Grant"),
        S("ACME"),
        D(2024, 1, 1),
        N(4.0),
        N(10.0),
        N(1.0),
    ];
    first.extend(grant());
    sheet(
        &mut wb,
        "Restricted Stock",
        &[
            [
                "Record Type",
                "Symbol",
                "Grant Date",
                "Withheld Qty.",
                "Vested Qty.",
                "Sellable Qty.",
                "Grant Number",
                "Date",
                "Event Type",
                "Qty. or Amount",
                "Vest Period",
                "Vest Date",
                "Vested Qty.",
                "Released Qty",
                "Released Amount",
                "Tax Description",
                "Taxable Gain",
            ]
            .map(S)
            .into(),
            first,
            ev(D(2025, 3, 15), "Shares vested", 10.0),
            ev(D(2025, 3, 15), "Shares released", 6.0),
            ev(D(2025, 4, 1), "Shares sold", 5.0),
            vest(1.0, D(2025, 3, 15), 10.0, 10.0),
            tax,
            vest(2.0, D(2026, 9, 15), 0.0, 0.0),
            vec![S("Totals"), E, E, N(4.0), N(10.0), N(1.0)],
        ],
    );
    wb.save(path).unwrap();
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn committed_xlsx_fixtures_hold_the_builders_cells() {
    let write = std::env::var_os("XLSX_FIXTURES_WRITE").is_some();
    let tmp = std::env::temp_dir().join(format!("xlsx-fixtures-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let builders: [(&str, fn(&Path)); 2] = [
        ("ledger-etrade-stockplan.xlsx", stockplan),
        ("ledger-etrade-benefit-history.xlsx", benefit_history),
    ];
    for (name, build) in builders {
        let target = if write {
            fixtures().join(name)
        } else {
            tmp.join(name)
        };
        build(&target);
        assert_eq!(
            dump_xlsx(&target).unwrap(),
            dump_xlsx(&fixtures().join(name)).unwrap(),
            "{name}: rebuild with XLSX_FIXTURES_WRITE=1"
        );
    }
    std::fs::remove_dir_all(&tmp).ok();
}
