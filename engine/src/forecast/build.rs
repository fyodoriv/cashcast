//! Lays out the Cashcast workbook with live formulas, as `forecast/build.py`.
//!
//! Formulas use only functions Numbers and Excel both support: SUM, SUMIFS,
//! IF, MAX, ROUND, ROUNDUP, EDATE, YEAR, MONTH. Light-yellow cells are inputs.

use super::book::{Book, Chart, Sheet, V, col_letter as l};
use super::model::{History, Inputs, add_months, forecast_categories};
use chrono::Datelike;
use std::collections::HashMap;

const MONEY: &str = "#,##0.00";
const MONTH_FMT: &str = "mmm yyyy";
const DAY_FMT: &str = "yyyy-mm-dd";
pub const PAYDAY_ROWS: u32 = 80;

const INCOME_LABELS: [(&str, &str); 8] = [
    ("rsu_espp", "RSU/ESPP sales"),
    ("crypto_sales", "Crypto sales"),
    ("from_investments", "From investments"),
    ("tax_refund", "Tax refunds"),
    ("cashback", "Cashback and interest"),
    ("gift", "Gifts received"),
    ("other", "Other income"),
    ("salary_other_accounts", "Salary (other accounts)"),
];

fn label(t: &str) -> String {
    INCOME_LABELS
        .iter()
        .find(|(k, _)| *k == t)
        .map_or(t.to_string(), |(_, v)| v.to_string())
}

fn f(s: String) -> V {
    V::Formula(s)
}

fn style_header(ws: &mut Sheet, ncols: u32) {
    for c in 1..=ncols {
        ws.cell(1, c).bold = true;
    }
    ws.freeze = true;
}

pub fn build(inp: &Inputs, hist: &History) -> Book {
    let cats = forecast_categories(inp, hist);
    let rent_cat = inp.rent_category.clone();
    let mut ws_in = Sheet::new("Inputs");
    let mut ws_h = Sheet::new("History");
    let mut ws_p = Sheet::new("Paydays");
    let mut ws_f = Sheet::new("Forecast");

    // History layout first, so Inputs formulas can point into it.
    let (h_first, h_last) = (2u32, 1 + hist.months.len() as u32);
    let known: Vec<&str> = INCOME_LABELS.iter().map(|(k, _)| *k).collect();
    let mut income_types: Vec<String> = known
        .iter()
        .filter(|t| hist.income.iter().any(|(k, _)| k == *t))
        .map(|t| t.to_string())
        .collect();
    let mut rest: Vec<String> = hist
        .income
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !known.contains(&k.as_str()))
        .collect();
    rest.sort();
    rest.dedup();
    income_types.extend(rest);
    let mut spend_cats = cats.clone();
    let mut others: Vec<String> = hist
        .spending
        .iter()
        .map(|(c, _)| c.clone())
        .filter(|c| !cats.contains(c))
        .collect();
    others.sort();
    spend_cats.extend(others);
    let mut h_cols: Vec<String> = [
        "Month",
        "Month of year",
        "In last 12 months",
        "Net salary",
        "Salary (off-cycle)",
    ]
    .map(String::from)
    .to_vec();
    h_cols.extend(income_types.iter().map(|t| label(t)));
    h_cols.push("Total income".into());
    h_cols.extend(spend_cats.iter().cloned());
    h_cols.extend(
        [
            "Total spending",
            "Spending excl. rent",
            "Income tax (refund + / paid -)",
            "Invested",
            "Net",
        ]
        .map(String::from),
    );
    let mut hc: HashMap<String, u32> = HashMap::new();
    for (i, n) in h_cols.iter().enumerate() {
        hc.insert(n.clone(), i as u32 + 1); // a later duplicate wins, as in a Python dict
    }
    let h_range = |name: &str| {
        let c = l(hc[name]);
        format!("History!${c}${h_first}:${c}${h_last}")
    };

    // Inputs.
    let mut r: HashMap<&str, String> = HashMap::new();
    let mut row = 1u32;
    ws_in.append(vec![
        V::Str("Setting".into()),
        V::Str("Value".into()),
        V::Str("Notes".into()),
    ]);
    for c in 1..=3 {
        ws_in.cell(1, c).bold = true;
    }
    let mut put = |ws: &mut Sheet,
                   r: &mut HashMap<&str, String>,
                   key: Option<&'static str>,
                   label: &str,
                   value: Option<V>,
                   fmt: Option<&'static str>,
                   note: &str,
                   editable: bool|
     -> u32 {
        row += 1;
        ws.set(row, 1, V::Str(label.into()));
        let cell = ws.cell(row, 2);
        if let Some(v) = value {
            cell.v = Some(v);
        }
        if let Some(fm) = fmt {
            cell.fmt = Some(fm);
        }
        if editable {
            cell.fill = true;
        }
        if !note.is_empty() {
            ws.set(row, 3, V::Str(note.into()));
        }
        if let Some(k) = key {
            r.insert(k, format!("Inputs!$B${row}"));
        }
        row
    };
    let num = |d: rust_decimal::Decimal| Some(V::Num(d));
    put(
        &mut ws_in,
        &mut r,
        Some("start"),
        "Start month",
        Some(V::Date(inp.start)),
        Some(MONTH_FMT),
        "",
        true,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("horizon"),
        "Horizon (months)",
        Some(V::Int(inp.horizon)),
        Some("0"),
        "The Forecast sheet has this many rows.",
        false,
    );
    let open_row = put(
        &mut ws_in,
        &mut r,
        Some("opening"),
        "Opening cash (sum of accounts below)",
        None,
        Some(MONEY),
        "",
        false,
    );
    let acc_rows: Vec<u32> = inp
        .opening_accounts
        .iter()
        .map(|(n, v)| {
            put(
                &mut ws_in,
                &mut r,
                None,
                &format!("  {n}"),
                num(*v),
                Some(MONEY),
                "Cash account balance at the start",
                true,
            )
        })
        .collect();
    let sum_or_zero = |rows: &[u32]| match (rows.first(), rows.last()) {
        (Some(a), Some(b)) => f(format!("SUM(B{a}:B{b})")),
        _ => V::Int(0),
    };
    ws_in.set(open_row, 2, sum_or_zero(&acc_rows));
    let card_row = put(
        &mut ws_in,
        &mut r,
        Some("cards"),
        "Card balances (separate; paid in full)",
        None,
        Some(MONEY),
        "Not subtracted from opening cash. Keep a card float in the floor.",
        false,
    );
    let c_rows: Vec<u32> = inp
        .card_balances
        .iter()
        .map(|(n, v)| {
            put(
                &mut ws_in,
                &mut r,
                None,
                &format!("  {n}"),
                num(*v),
                Some(MONEY),
                "",
                true,
            )
        })
        .collect();
    ws_in.set(card_row, 2, sum_or_zero(&c_rows));
    put(
        &mut ws_in,
        &mut r,
        Some("schedule"),
        "Pay schedule",
        Some(V::Str(inp.schedule.clone())),
        None,
        "weekly | biweekly | semimonthly | monthly | none",
        true,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("net_h1"),
        "Net pay per cheque, Jan-Jun",
        num(inp.net_h1),
        Some(MONEY),
        "Often lower until payroll deductions max out",
        true,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("net_h2"),
        "Net pay per cheque, Jul-Dec",
        num(inp.net_h2),
        Some(MONEY),
        "",
        true,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("anchor"),
        "Payday anchor (weekly/biweekly)",
        inp.anchor.map(V::Date),
        Some(DAY_FMT),
        "A known payday",
        true,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("period"),
        "Days between paydays",
        Some(V::Int(inp.period_days())),
        Some("0"),
        "",
        true,
    );
    let rent_note = match &rent_cat {
        Some(c) => format!("Replaces the '{c}' history."),
        None => "Not set: rent stays in the spending history.".into(),
    };
    put(
        &mut ws_in,
        &mut r,
        Some("rent"),
        "Rent charged per month",
        num(inp.rent()),
        Some(MONEY),
        &rent_note,
        true,
    );
    let f_rows: Vec<u32> = inp
        .floor_parts
        .iter()
        .map(|(n, v)| {
            put(
                &mut ws_in,
                &mut r,
                None,
                &format!("  {n}"),
                num(*v),
                Some(MONEY),
                "",
                true,
            )
        })
        .collect();
    let floor_v = match (f_rows.first(), f_rows.last()) {
        (Some(a), Some(b)) => f(format!("SUM(B{a}:B{b})")),
        // Python formats SUM(B{f_rows[0]}...) and would fail with no parts; the model always has one.
        _ => V::Int(0),
    };
    put(
        &mut ws_in,
        &mut r,
        Some("floor"),
        "Cash floor",
        Some(floor_v),
        Some(MONEY),
        "",
        false,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("method"),
        "Spending method",
        Some(V::Str(inp.method.clone())),
        None,
        "trailing-12 average | trailing-12 median | same month last year | flat baseline",
        true,
    );
    put(
        &mut ws_in,
        &mut r,
        Some("baseline"),
        "Baseline spending per month",
        num(inp.baseline),
        Some(MONEY),
        "Untracked spending tops categories up to this. 'flat baseline' scales them to it.",
        true,
    );
    let t12_ex_rent = format!(
        "(SUMIFS({},{},1)/12)",
        h_range("Spending excl. rent"),
        h_range("In last 12 months")
    );
    let scale = format!(
        "IF({}=\"flat baseline\",({}-{})/{t12_ex_rent},1)",
        r["method"], r["baseline"], r["rent"]
    );
    put(
        &mut ws_in,
        &mut r,
        Some("scale"),
        "Category scale (helper)",
        Some(f(scale)),
        Some("0.0000"),
        "",
        false,
    );

    // Extra income by forecast month (YYYYMM), so dated lines stay in their months.
    row += 2;
    ws_in
        .set(row, 1, V::Str("Extra income by forecast month".into()))
        .bold = true;
    row += 1;
    let x_header = row;
    let items = &inp.extra_income;
    ws_in.set(row, 1, V::Str("Month (YYYYMM)".into())).bold = true;
    for (j, item) in items.iter().enumerate() {
        ws_in.set(row, 2 + j as u32, V::Str(item.name.clone())).bold = true;
    }
    let total_col = 2 + items.len() as u32;
    ws_in.set(row, total_col, V::Str("Total".into())).bold = true;
    for n in 0..inp.horizon {
        let m = add_months(inp.start, n as i32);
        let month = m.month() as i64;
        row += 1;
        ws_in.set(row, 1, V::Int(m.year() as i64 * 100 + month));
        for (j, item) in items.iter().enumerate() {
            let v = if item.months.contains(&month)
                && item.from.is_none_or(|f| m >= f)
                && item.until.is_none_or(|u| m <= u)
                && item.known.is_none_or(|k| inp.start >= k)
            {
                V::Num(item.amount)
            } else {
                V::Int(0)
            };
            let c = ws_in.set(row, 2 + j as u32, v);
            c.fmt = Some(MONEY);
            c.fill = true;
        }
        let total = if items.is_empty() {
            V::Int(0)
        } else {
            f(format!("SUM(B{row}:{}{row})", l(total_col - 1)))
        };
        ws_in.set(row, total_col, total).fmt = Some(MONEY);
    }
    let (x_first, x_last) = (x_header + 1, row);
    let extra_moy = format!("Inputs!$A${x_first}:$A${x_last}");
    let tc = l(total_col);
    let extra_total = format!("Inputs!${tc}${x_first}:${tc}${x_last}");

    // Holidays.
    row += 2;
    ws_in
        .set(
            row,
            1,
            V::Str("Holidays (a weekly or biweekly payday on one moves to the day before)".into()),
        )
        .bold = true;
    row += 1;
    for (c, name) in ["Holiday", "Date", "Flag"].iter().enumerate() {
        ws_in.set(row, c as u32 + 1, V::Str(name.to_string())).bold = true;
    }
    let hol_first = row + 1;
    for (name, day) in &inp.holidays {
        row += 1;
        ws_in.set(row, 1, V::Str(name.clone()));
        let c = ws_in.set(row, 2, V::Date(*day));
        c.fmt = Some(DAY_FMT);
        c.fill = true;
        ws_in.set(row, 3, V::Int(1));
    }
    let hol_last = row.max(hol_first);
    let hol_dates = format!("Inputs!$B${hol_first}:$B${hol_last}");
    let hol_flags = format!("Inputs!$C${hol_first}:$C${hol_last}");
    if !inp.assumptions.is_empty() {
        row += 2;
        ws_in
            .set(
                row,
                1,
                V::Str("Defaults used (edit cashcast.toml to change)".into()),
            )
            .bold = true;
        for note in &inp.assumptions {
            row += 1;
            ws_in.set(row, 1, V::Str(note.clone()));
        }
    }
    ws_in.widths = vec![(1, 46.0), (2, 16.0), (3, 70.0)];

    // History values.
    ws_h.append(h_cols.iter().map(|c| V::Str(c.clone())).collect());
    let ncols = h_cols.len() as u32;
    for (i, month) in hist.months.iter().enumerate() {
        let rr = h_first + i as u32;
        let mut values = vec![
            V::Date(*month),
            f(format!("MONTH(A{rr})")),
            f(format!(
                "IF(A{rr}>=EDATE({s},-12),IF(A{rr}<{s},1,0),0)",
                s = r["start"]
            )),
            V::Num(hist.salary[i]),
            V::Num(hist.salary_off_cycle[i]),
        ];
        for t in &income_types {
            let s = hist
                .income
                .iter()
                .rev()
                .find(|(k, _)| k == t)
                .map(|(_, v)| v[i])
                .unwrap_or_default();
            values.push(V::Num(s));
        }
        values.push(f(format!(
            "SUM({}{rr}:{}{rr})",
            l(hc["Net salary"]),
            l(hc["Total income"] - 1)
        )));
        for c in &spend_cats {
            values.push(V::Num(hist.spend(c).map(|v| v[i]).unwrap_or_default()));
        }
        let total_spend = match (spend_cats.first(), spend_cats.last()) {
            (Some(a), Some(b)) => format!("SUM({}{rr}:{}{rr})", l(hc[a]), l(hc[b])),
            _ => "0".into(),
        };
        let ts = l(hc["Total spending"]);
        let ex_rent = match rent_cat.as_ref().and_then(|c| hc.get(c)) {
            Some(&rc) => format!("{ts}{rr}-{}{rr}", l(rc)),
            None => format!("{ts}{rr}"),
        };
        values.push(f(total_spend));
        values.push(f(ex_rent));
        values.push(V::Num(hist.income_tax[i]));
        values.push(V::Num(hist.invested[i]));
        values.push(f(format!(
            "{}{rr}-{ts}{rr}+{}{rr}",
            l(hc["Total income"]),
            l(hc["Income tax (refund + / paid -)"])
        )));
        ws_h.append(values);
        ws_h.cell(rr, 1).fmt = Some(MONTH_FMT);
        for c in 4..=ncols {
            ws_h.cell(rr, c).fmt = Some(MONEY);
        }
    }
    style_header(&mut ws_h, ncols);

    // Paydays (weekly and biweekly schedules).
    ws_p.append(
        [
            "#",
            "Nominal payday",
            "Holiday?",
            "Payday",
            "Month key",
            "Count",
        ]
        .map(|s| V::Str(s.into()))
        .to_vec(),
    );
    for k in 0..PAYDAY_ROWS {
        let rr = 2 + k;
        let nominal = if k == 0 {
            format!(
                "{a}+{p}*ROUNDUP(({s}-{a})/{p},0)",
                a = r["anchor"],
                p = r["period"],
                s = r["start"]
            )
        } else {
            format!("B{}+{}", rr - 1, r["period"])
        };
        ws_p.append(vec![
            V::Int(k as i64 + 1),
            f(nominal),
            f(format!("IF(SUMIFS({hol_flags},{hol_dates},B{rr})>0,1,0)")),
            f(format!("B{rr}-C{rr}")),
            f(format!("YEAR(D{rr})*100+MONTH(D{rr})")),
            V::Int(1),
        ]);
        ws_p.cell(rr, 2).fmt = Some(DAY_FMT);
        ws_p.cell(rr, 4).fmt = Some(DAY_FMT);
    }
    style_header(&mut ws_p, 6);

    // Forecast.
    let mut f_cols: Vec<String> = [
        "Month",
        "Paycheques",
        "Net salary",
        "Extra income",
        "Total income",
    ]
    .map(String::from)
    .to_vec();
    f_cols.extend(cats.iter().cloned());
    f_cols.extend(
        [
            "Untracked spending",
            "Expenses",
            "Taxes (refund + / owing -)",
            "Net cash flow",
            "Month start",
            "Before investing",
            "Cash floor",
            "To invest",
            "Month end",
        ]
        .map(String::from),
    );
    let mut fc: HashMap<String, String> = HashMap::new();
    for (i, n) in f_cols.iter().enumerate() {
        fc.insert(n.clone(), l(i as u32 + 1));
    }
    ws_f.append(f_cols.iter().map(|c| V::Str(c.clone())).collect());
    let (first, last) = (2u32, 1 + inp.horizon as u32);
    let pay_keys = format!("Paydays!$E$2:$E${}", 1 + PAYDAY_ROWS);
    let pay_counts = format!("Paydays!$F$2:$F${}", 1 + PAYDAY_ROWS);
    let cat_first = cats
        .first()
        .map_or(fc["Untracked spending"].clone(), |c| fc[c].clone());
    let cat_last = cats
        .last()
        .map_or(fc["Untracked spending"].clone(), |c| fc[c].clone());
    let sched = &r["schedule"];
    let in12 = h_range("In last 12 months");
    let moy = h_range("Month of year");
    for n in 0..inp.horizon as u32 {
        let rr = first + n;
        let a = format!("$A{rr}");
        let mut values: HashMap<String, V> = HashMap::new();
        let mut put = |k: &str, v: V| {
            values.insert(k.to_string(), v);
        };
        put(
            "Month",
            f(if n == 0 {
                r["start"].clone()
            } else {
                format!("EDATE(A{},1)", rr - 1)
            }),
        );
        put(
            "Paycheques",
            f(format!(
                "IF({sched}=\"monthly\",1,IF({sched}=\"semimonthly\",2,IF({sched}=\"none\",0,SUMIFS({pay_counts},{pay_keys},YEAR({a})*100+MONTH({a})))))"
            )),
        );
        put(
            "Net salary",
            f(format!(
                "B{rr}*IF(MONTH({a})<=6,{},{})",
                r["net_h1"], r["net_h2"]
            )),
        );
        put(
            "Extra income",
            f(format!(
                "SUMIFS({extra_total},{extra_moy},YEAR({a})*100+MONTH({a}))"
            )),
        );
        put(
            "Total income",
            f(format!(
                "{}{rr}+{}{rr}",
                fc["Net salary"], fc["Extra income"]
            )),
        );
        put(
            "Untracked spending",
            f(if cats.is_empty() {
                format!("MAX(0,{})", r["baseline"])
            } else {
                format!(
                    "MAX(0,{}-SUM({cat_first}{rr}:{cat_last}{rr}))",
                    r["baseline"]
                )
            }),
        );
        put(
            "Expenses",
            f(format!(
                "SUM({cat_first}{rr}:{}{rr})",
                fc["Untracked spending"]
            )),
        );
        let key = add_months(inp.start, n as i32).format("%Y-%m").to_string();
        put(
            "Taxes (refund + / owing -)",
            V::Float(inp.tax(&key).to_string().parse().unwrap_or(0.0)),
        );
        put(
            "Net cash flow",
            f(format!(
                "{}{rr}-{}{rr}+{}{rr}",
                fc["Total income"], fc["Expenses"], fc["Taxes (refund + / owing -)"]
            )),
        );
        put(
            "Month start",
            f(if n == 0 {
                r["opening"].clone()
            } else {
                format!("{}{}", fc["Month end"], rr - 1)
            }),
        );
        put(
            "Before investing",
            f(format!(
                "{}{rr}+{}{rr}",
                fc["Month start"], fc["Net cash flow"]
            )),
        );
        put("Cash floor", f(r["floor"].clone()));
        // Invest only what no later month needs, like the model: the lowest
        // running total of later net cash flows (0 when none dips) caps it.
        let net = &fc["Net cash flow"];
        let later: String = (rr + 1..=last)
            .map(|j| format!(",SUM({net}{}:{net}{j})", rr + 1))
            .collect();
        put(
            "To invest",
            f(format!(
                "MAX(0,{}{rr}+MIN(0{later})-{}{rr})",
                fc["Before investing"], fc["Cash floor"]
            )),
        );
        put(
            "Month end",
            f(format!(
                "{}{rr}-{}{rr}",
                fc["Before investing"], fc["To invest"]
            )),
        );
        for c in &cats {
            if Some(c) == rent_cat.as_ref() {
                put(c, f(r["rent"].clone()));
                continue;
            }
            let col = h_range(c);
            let same = format!("SUMIFS({col},{in12},1,{moy},MONTH({a}))");
            let avg = format!("SUMIFS({col},{in12},1)/12");
            // The median has no filtered spreadsheet form that Numbers and the
            // checker both read, so it is written as the value at build time.
            let med = super::model::median12(
                hist.spend(c).expect("category from history"),
                &hist.last12(inp.start),
            )
            .normalize();
            put(
                c,
                f(format!(
                    "ROUND(IF({m}=\"same month last year\",{same},IF({m}=\"trailing-12 median\",{med},{avg}))*{},2)",
                    r["scale"],
                    m = r["method"]
                )),
            );
        }
        ws_f.append(f_cols.iter().map(|n| values[n].clone()).collect());
        ws_f.cell(rr, 1).fmt = Some(MONTH_FMT);
        for c in 3..=f_cols.len() as u32 {
            ws_f.cell(rr, c).fmt = Some(MONEY);
        }
        for name in ["Extra income", "Taxes (refund + / owing -)"] {
            let c = super::book::col_number(&fc[name]);
            ws_f.cell(rr, c).fill = true;
        }
    }
    let total_row = last + 1;
    ws_f.set(total_row, 1, V::Str("Total".into())).bold = true;
    let mut names: Vec<String> = ["Paycheques", "Net salary", "Extra income", "Total income"]
        .map(String::from)
        .to_vec();
    names.extend(cats.iter().cloned());
    names.extend(
        [
            "Untracked spending",
            "Expenses",
            "Taxes (refund + / owing -)",
            "Net cash flow",
            "To invest",
        ]
        .map(String::from),
    );
    for name in &names {
        let col = &fc[name];
        let c = ws_f.set(
            total_row,
            super::book::col_number(col),
            f(format!("SUM({col}{first}:{col}{last})")),
        );
        c.bold = true;
        c.fmt = Some(if name != "Paycheques" { MONEY } else { "0" });
    }
    let nf = f_cols.len() as u32;
    style_header(&mut ws_f, nf);
    ws_f.widths = std::iter::once((1, 11.0))
        .chain((2..=nf).map(|i| (i, 14.0)))
        .collect();
    let col_of = |name: &str| {
        f_cols
            .iter()
            .position(|c| c == name)
            .map_or(1, |i| i as u32 + 1)
    };
    let chart = Chart {
        sheet: 3,
        title: "Month-end cash vs cash floor".into(),
        series_cols: vec![col_of("Month end"), col_of("Cash floor")],
        first_row: first,
        last_row: last,
        anchor_row: total_row + 3,
    };
    Book {
        sheets: vec![ws_in, ws_h, ws_p, ws_f],
        active: 3,
        chart: Some(chart),
    }
}
