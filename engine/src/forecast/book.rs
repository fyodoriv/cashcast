//! An in-memory workbook: what `build` lays out, what `formula` evaluates and
//! what `save` writes as `.xlsx` (rust_xlsxwriter).

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Empty,
    Num(Decimal),
    Int(i64),
    Float(f64),
    Str(String),
    Date(NaiveDate),
    Formula(String), // without the leading "="
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub v: Option<V>,
    pub fmt: Option<&'static str>,
    pub bold: bool,
    pub fill: bool,
}

#[derive(Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<(u32, u32), Cell>, // (row, col), both 1-based
    pub widths: Vec<(u32, f64)>,
    pub freeze: bool,
    pub max_row: u32,
}

impl Sheet {
    pub fn new(name: &str) -> Self {
        Sheet {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn cell(&mut self, row: u32, col: u32) -> &mut Cell {
        self.max_row = self.max_row.max(row);
        self.cells.entry((row, col)).or_default()
    }

    /// openpyxl `ws.cell(row, col, value)`: a None value keeps the old one.
    pub fn set(&mut self, row: u32, col: u32, v: V) -> &mut Cell {
        let c = self.cell(row, col);
        c.v = Some(v);
        c
    }

    /// openpyxl `ws.append(values)`: the next row after the last one used.
    pub fn append(&mut self, values: Vec<V>) -> u32 {
        let row = self.max_row + 1;
        for (i, v) in values.into_iter().enumerate() {
            self.set(row, i as u32 + 1, v);
        }
        self.max_row = row;
        row
    }

    pub fn get(&self, row: u32, col: u32) -> Option<&V> {
        self.cells.get(&(row, col)).and_then(|c| c.v.as_ref())
    }
}

/// openpyxl `get_column_letter`.
pub fn col_letter(mut n: u32) -> String {
    let mut out = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

pub fn col_number(letters: &str) -> u32 {
    letters.bytes().fold(0, |acc, b| {
        acc * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32
    })
}

pub struct Chart {
    pub sheet: usize,
    pub title: String,
    pub series_cols: Vec<u32>,
    pub first_row: u32,
    pub last_row: u32,
    pub anchor_row: u32,
}

pub struct Book {
    pub sheets: Vec<Sheet>,
    pub active: usize,
    pub chart: Option<Chart>,
}

impl Book {
    pub fn sheet(&self, name: &str) -> Option<&Sheet> {
        self.sheets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Writes the workbook. The creation time is fixed, so the same inputs give the same bytes.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        use rust_xlsxwriter::{
            Chart as XChart, ChartType, DocProperties, ExcelDateTime, Format, FormatPattern,
            Formula, Workbook,
        };
        let err = |e: rust_xlsxwriter::XlsxError| format!("cannot write {}: {e}", path.display());
        let mut wb = Workbook::new();
        let fixed = ExcelDateTime::from_ymd(2000, 1, 1).map_err(err)?;
        wb.set_properties(&DocProperties::new().set_creation_datetime(&fixed));
        for (i, s) in self.sheets.iter().enumerate() {
            let ws = wb.add_worksheet();
            ws.set_name(&s.name).map_err(err)?;
            for &(col, w) in &s.widths {
                ws.set_column_width((col - 1) as u16, w).map_err(err)?;
            }
            if s.freeze {
                ws.set_freeze_panes(1, 1).map_err(err)?;
            }
            for (&(row, col), c) in &s.cells {
                let mut f = Format::new();
                if let Some(n) = c.fmt {
                    f = f.set_num_format(n);
                }
                if c.bold {
                    f = f.set_bold();
                }
                if c.fill {
                    f = f
                        .set_pattern(FormatPattern::Solid)
                        .set_background_color(0xFFF2CC);
                }
                let (r, k) = (row - 1, (col - 1) as u16);
                match &c.v {
                    None | Some(V::Empty) => ws.write_blank(r, k, &f).map(|_| ()),
                    Some(V::Num(d)) => ws
                        .write_number_with_format(
                            r,
                            k,
                            d.to_string().parse::<f64>().unwrap_or(0.0),
                            &f,
                        )
                        .map(|_| ()),
                    Some(V::Int(n)) => ws.write_number_with_format(r, k, *n as f64, &f).map(|_| ()),
                    Some(V::Float(x)) => ws.write_number_with_format(r, k, *x, &f).map(|_| ()),
                    Some(V::Str(t)) => ws.write_string_with_format(r, k, t, &f).map(|_| ()),
                    Some(V::Date(d)) => {
                        let dt = ExcelDateTime::from_ymd(
                            d.year_ce().1 as u16,
                            d.month0() as u8 + 1,
                            d.day0() as u8 + 1,
                        )
                        .map_err(err)?;
                        ws.write_datetime_with_format(r, k, &dt, &f).map(|_| ())
                    }
                    Some(V::Formula(t)) => ws
                        .write_formula_with_format(r, k, Formula::new(format!("={t}")), &f)
                        .map(|_| ()),
                }
                .map_err(err)?;
            }
            if i == self.active {
                ws.set_active(true);
            }
            if let Some(ch) = self.chart.as_ref().filter(|c| c.sheet == i) {
                let mut chart = XChart::new(ChartType::Line);
                chart.title().set_name(&ch.title);
                chart.set_width(832).set_height(340); // 22 cm x 9 cm, as the Python workbook
                for &col in &ch.series_cols {
                    let c = (col - 1) as u16;
                    chart
                        .add_series()
                        .set_name((s.name.as_str(), 0, c))
                        .set_values((s.name.as_str(), ch.first_row - 1, c, ch.last_row - 1, c))
                        .set_categories((s.name.as_str(), ch.first_row - 1, 0, ch.last_row - 1, 0));
                }
                ws.insert_chart(ch.anchor_row - 1, 0, &chart).map_err(err)?;
            }
        }
        wb.save(path).map_err(err)
    }
}

use chrono::Datelike;

/// One line per non-empty cell: the formula, or the value. The parity
/// harness compares workbooks this way (decision 0010), not by bytes.
pub fn dump_xlsx(path: &std::path::Path) -> Result<String, String> {
    use calamine::{Data, Reader, Xlsx, open_workbook};
    let mut wb: Xlsx<_> =
        open_workbook(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out = String::new();
    for name in wb.sheet_names().to_owned() {
        let formulas = wb.worksheet_formula(&name).map_err(|e| e.to_string())?;
        let values = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
        let mut cells: BTreeMap<(u32, u32), String> = BTreeMap::new();
        for (r, c, v) in values.used_cells() {
            let (r, c) = (
                r as u32 + values.start().unwrap_or((0, 0)).0,
                c as u32 + values.start().unwrap_or((0, 0)).1,
            );
            let text = match v {
                Data::Empty => continue,
                Data::String(s) => format!("{s:?}"),
                Data::Float(f) => format!("{f}"),
                Data::Int(i) => format!("{i}"),
                Data::Bool(b) => format!("{b}"),
                Data::DateTime(d) => format!("date {}", d.as_f64()),
                other => format!("{other:?}"),
            };
            cells.insert((r, c), text);
        }
        let (fr, fc) = formulas.start().unwrap_or((0, 0));
        for (r, c, f) in formulas.used_cells() {
            if !f.is_empty() {
                cells.insert((r as u32 + fr, c as u32 + fc), format!("={f}"));
            }
        }
        for ((r, c), text) in cells {
            out += &format!("{name}!{}{} {text}\n", col_letter(c + 1), r + 1);
        }
    }
    Ok(out)
}
