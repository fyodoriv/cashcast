//! Evaluates the workbook formulas, as `verify.py` does with pycel.
//!
//! Only what `build` writes: SUM, SUMIFS, IF, MAX, ROUND, ROUNDUP, EDATE,
//! YEAR, MONTH, arithmetic, comparisons, strings, cell references and ranges.
//! Numbers are floats, as in a spreadsheet. ROUND rounds the shortest float
//! text half away from zero, and SUM adds with compensation, like pycel on
//! Python 3.13.

use super::book::{Book, V, col_number};
use super::model::add_months;
use chrono::{Datelike, Duration, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};
use std::cell::RefCell;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Empty,
    Num(f64),
    Str(String),
    Err(String),
}

type Key = (usize, u32, u32);

pub struct Eval<'a> {
    book: &'a Book,
    memo: RefCell<HashMap<Key, Val>>,
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid")
}

pub fn serial(d: NaiveDate) -> f64 {
    (d - epoch()).num_days() as f64
}

fn date_of(x: f64) -> NaiveDate {
    epoch() + Duration::days(x.floor() as i64)
}

/// Python's `sum` of floats on 3.12 and later: Neumaier compensated addition.
pub fn py_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for x in values {
        let t = s + x;
        if s.abs() >= x.abs() {
            c += (s - t) + x;
        } else {
            c += (x - t) + s;
        }
        s = t;
    }
    s + c
}

/// The shortest text of a float, as a decimal (Python `Decimal(repr(x))`).
pub fn dec_of(x: f64) -> Decimal {
    Decimal::from_str(&format!("{x}"))
        .unwrap_or_else(|_| Decimal::from_f64_retain(x).unwrap_or_default())
}

fn to_f64(d: Decimal) -> f64 {
    d.to_string().parse().unwrap_or(0.0)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ref(Option<String>, String), // sheet, "A1" or "A1:B2" without "$"
    Func(String),
    Op(String),
    LParen,
    RParen,
    Comma,
}

fn tokens(src: &str) -> Result<Vec<Tok>, String> {
    let s: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < s.len() {
        let c = s[i];
        match c {
            ' ' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1
            }
            '"' => {
                let mut t = String::new();
                i += 1;
                while i < s.len() && s[i] != '"' {
                    t.push(s[i]);
                    i += 1;
                }
                i += 1;
                out.push(Tok::Str(t));
            }
            '<' | '>' => {
                if i + 1 < s.len() && (s[i + 1] == '=' || (c == '<' && s[i + 1] == '>')) {
                    out.push(Tok::Op(format!("{c}{}", s[i + 1])));
                    i += 2;
                } else {
                    out.push(Tok::Op(c.to_string()));
                    i += 1;
                }
            }
            '+' | '-' | '*' | '/' | '=' | '&' => {
                out.push(Tok::Op(c.to_string()));
                i += 1
            }
            '0'..='9' | '.' => {
                let st = i;
                while i < s.len() && (s[i].is_ascii_digit() || s[i] == '.') {
                    i += 1;
                }
                let t: String = s[st..i].iter().collect();
                out.push(Tok::Num(t.parse().map_err(|_| format!("bad number {t}"))?));
            }
            _ if c.is_ascii_alphabetic() || c == '$' => {
                let st = i;
                while i < s.len()
                    && (s[i].is_ascii_alphanumeric() || matches!(s[i], '$' | '_' | '.' | ':' | '!'))
                {
                    i += 1;
                }
                let word: String = s[st..i].iter().collect();
                if i < s.len() && s[i] == '(' && !word.contains('!') {
                    out.push(Tok::Func(word.to_ascii_uppercase()));
                } else {
                    let clean = word.replace('$', "");
                    match clean.split_once('!') {
                        Some((sh, r)) => out.push(Tok::Ref(Some(sh.to_string()), r.to_string())),
                        None => out.push(Tok::Ref(None, clean)),
                    }
                }
            }
            _ => return Err(format!("unexpected {c:?} in {src}")),
        }
    }
    Ok(out)
}

#[derive(Debug)]
enum Expr {
    Num(f64),
    Str(String),
    Ref(Option<String>, String),
    Neg(Box<Expr>),
    Bin(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn op_at(&self, ops: &[&str]) -> Option<String> {
        match self.peek() {
            Some(Tok::Op(o)) if ops.contains(&o.as_str()) => Some(o.clone()),
            _ => None,
        }
    }
    fn compare(&mut self) -> Result<Expr, String> {
        let mut l = self.concat()?;
        while let Some(o) = self.op_at(&["=", "<", ">", "<=", ">=", "<>"]) {
            self.i += 1;
            l = Expr::Bin(o, Box::new(l), Box::new(self.concat()?));
        }
        Ok(l)
    }
    fn concat(&mut self) -> Result<Expr, String> {
        let mut l = self.additive()?;
        while let Some(o) = self.op_at(&["&"]) {
            self.i += 1;
            l = Expr::Bin(o, Box::new(l), Box::new(self.additive()?));
        }
        Ok(l)
    }
    fn additive(&mut self) -> Result<Expr, String> {
        let mut l = self.term()?;
        while let Some(o) = self.op_at(&["+", "-"]) {
            self.i += 1;
            l = Expr::Bin(o, Box::new(l), Box::new(self.term()?));
        }
        Ok(l)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        while let Some(o) = self.op_at(&["*", "/"]) {
            self.i += 1;
            l = Expr::Bin(o, Box::new(l), Box::new(self.unary()?));
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if let Some(o) = self.op_at(&["-", "+"]) {
            self.i += 1;
            let e = self.unary()?;
            return Ok(if o == "-" { Expr::Neg(Box::new(e)) } else { e });
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Ref(sh, r)) => Ok(Expr::Ref(sh, r)),
            Some(Tok::LParen) => {
                let e = self.compare()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    t => Err(format!("expected ) got {t:?}")),
                }
            }
            Some(Tok::Func(name)) => {
                self.next(); // (
                let mut args = Vec::new();
                if self.peek() != Some(&Tok::RParen) {
                    loop {
                        args.push(self.compare()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            t => return Err(format!("expected , or ) got {t:?}")),
                        }
                    }
                } else {
                    self.next();
                }
                Ok(Expr::Call(name, args))
            }
            t => Err(format!("unexpected {t:?}")),
        }
    }
}

fn parse(src: &str) -> Result<Expr, String> {
    let mut p = Parser {
        t: tokens(src)?,
        i: 0,
    };
    let e = p.compare()?;
    if p.i != p.t.len() {
        return Err(format!("trailing tokens in {src}"));
    }
    Ok(e)
}

fn split_ref(r: &str) -> (u32, u32) {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row: u32 = r[letters.len()..].parse().unwrap_or(0);
    (row, col_number(&letters))
}

enum Arg {
    One(Val),
    Range(Vec<Val>),
}

fn num(v: &Val) -> Result<f64, String> {
    match v {
        Val::Empty => Ok(0.0),
        Val::Num(n) => Ok(*n),
        Val::Str(s) => s.trim().parse().map_err(|_| "#VALUE!".to_string()),
        Val::Err(e) => Err(e.clone()),
    }
}

fn matches(cell: &Val, crit: &Val) -> bool {
    match (cell, crit) {
        (Val::Num(a), Val::Num(b)) => a == b,
        (Val::Str(a), Val::Str(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

impl<'a> Eval<'a> {
    pub fn new(book: &'a Book) -> Self {
        Eval {
            book,
            memo: RefCell::new(HashMap::new()),
        }
    }

    fn sheet_index(&self, name: &str) -> Result<usize, String> {
        self.book
            .sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or(format!("no sheet {name}"))
    }

    /// The value of one cell, by sheet name and "A1" address.
    pub fn cell(&self, sheet: &str, addr: &str) -> Val {
        match self.sheet_index(sheet) {
            Ok(s) => {
                let (r, c) = split_ref(addr);
                self.at(s, r, c)
            }
            Err(e) => Val::Err(e),
        }
    }

    fn at(&self, s: usize, r: u32, c: u32) -> Val {
        if let Some(v) = self.memo.borrow().get(&(s, r, c)) {
            return v.clone();
        }
        let v = match self.book.sheets[s].get(r, c) {
            None | Some(V::Empty) => Val::Empty,
            Some(V::Num(d)) => Val::Num(to_f64(*d)),
            Some(V::Int(n)) => Val::Num(*n as f64),
            Some(V::Float(x)) => Val::Num(*x),
            Some(V::Str(t)) => Val::Str(t.clone()),
            Some(V::Date(d)) => Val::Num(serial(*d)),
            Some(V::Formula(f)) => match parse(f) {
                Ok(e) => self.eval(&e, s),
                Err(e) => Val::Err(e),
            },
        };
        self.memo.borrow_mut().insert((s, r, c), v.clone());
        v
    }

    fn arg(&self, e: &Expr, s: usize) -> Arg {
        if let Expr::Ref(sh, r) = e
            && let Some((a, b)) = r.split_once(':')
        {
            let si = match sh {
                Some(n) => match self.sheet_index(n) {
                    Ok(i) => i,
                    Err(e) => return Arg::One(Val::Err(e)),
                },
                None => s,
            };
            let ((r1, c1), (r2, c2)) = (split_ref(a), split_ref(b));
            let mut out = Vec::new();
            for r in r1.min(r2)..=r1.max(r2) {
                for c in c1.min(c2)..=c1.max(c2) {
                    out.push(self.at(si, r, c));
                }
            }
            return Arg::Range(out);
        }
        Arg::One(self.eval(e, s))
    }

    fn eval(&self, e: &Expr, s: usize) -> Val {
        match self.try_eval(e, s) {
            Ok(v) => v,
            Err(e) => Val::Err(e),
        }
    }

    fn n(&self, e: &Expr, s: usize) -> Result<f64, String> {
        num(&self.eval(e, s))
    }

    fn try_eval(&self, e: &Expr, s: usize) -> Result<Val, String> {
        Ok(match e {
            Expr::Num(n) => Val::Num(*n),
            Expr::Str(t) => Val::Str(t.clone()),
            Expr::Ref(sh, r) => {
                let si = match sh {
                    Some(n) => self.sheet_index(n)?,
                    None => s,
                };
                let (row, col) = split_ref(r);
                match self.at(si, row, col) {
                    Val::Err(e) => return Err(e),
                    v => v,
                }
            }
            Expr::Neg(x) => Val::Num(-self.n(x, s)?),
            Expr::Bin(op, a, b) => {
                let (va, vb) = (self.eval(a, s), self.eval(b, s));
                if let Val::Err(e) = &va {
                    return Err(e.clone());
                }
                if let Val::Err(e) = &vb {
                    return Err(e.clone());
                }
                match op.as_str() {
                    "+" => Val::Num(num(&va)? + num(&vb)?),
                    "-" => Val::Num(num(&va)? - num(&vb)?),
                    "*" => Val::Num(num(&va)? * num(&vb)?),
                    "/" => {
                        let d = num(&vb)?;
                        if d == 0.0 {
                            return Err("#DIV/0!".into());
                        }
                        Val::Num(num(&va)? / d)
                    }
                    "&" => Val::Str(format!("{}{}", text(&va), text(&vb))),
                    _ => {
                        let ord = compare(&va, &vb);
                        let t = match op.as_str() {
                            "=" => ord == std::cmp::Ordering::Equal,
                            "<>" => ord != std::cmp::Ordering::Equal,
                            "<" => ord == std::cmp::Ordering::Less,
                            ">" => ord == std::cmp::Ordering::Greater,
                            "<=" => ord != std::cmp::Ordering::Greater,
                            _ => ord != std::cmp::Ordering::Less,
                        };
                        Val::Num(if t { 1.0 } else { 0.0 })
                    }
                }
            }
            Expr::Call(name, args) => self.call(name, args, s)?,
        })
    }

    fn call(&self, name: &str, args: &[Expr], s: usize) -> Result<Val, String> {
        let flat = |args: &[Expr]| -> Result<Vec<f64>, String> {
            let mut out = Vec::new();
            for a in args {
                match self.arg(a, s) {
                    Arg::One(v) => out.push(num(&v)?),
                    Arg::Range(vs) => {
                        for v in vs {
                            match v {
                                Val::Num(n) => out.push(n),
                                Val::Err(e) => return Err(e),
                                _ => {}
                            }
                        }
                    }
                }
            }
            Ok(out)
        };
        Ok(match name {
            "SUM" => Val::Num(py_sum(flat(args)?)),
            "MAX" => Val::Num(
                flat(args)?
                    .into_iter()
                    .fold(f64::NEG_INFINITY, f64::max)
                    .max(if args.is_empty() {
                        0.0
                    } else {
                        f64::NEG_INFINITY
                    }),
            ),
            "MIN" => Val::Num(
                flat(args)?
                    .into_iter()
                    .fold(f64::INFINITY, f64::min)
                    .min(if args.is_empty() { 0.0 } else { f64::INFINITY }),
            ),
            "IF" => {
                let c = self.n(&args[0], s)?;
                if c != 0.0 {
                    self.eval(&args[1], s)
                } else if args.len() > 2 {
                    self.eval(&args[2], s)
                } else {
                    Val::Num(0.0)
                }
            }
            "ROUND" | "ROUNDUP" => {
                let x = self.n(&args[0], s)?;
                let dp = self.n(&args[1], s)? as u32;
                let strat = if name == "ROUND" {
                    RoundingStrategy::MidpointAwayFromZero
                } else {
                    RoundingStrategy::AwayFromZero
                };
                Val::Num(to_f64(dec_of(x).round_dp_with_strategy(dp, strat)))
            }
            "SUMIFS" => {
                let Arg::Range(sum) = self.arg(&args[0], s) else {
                    return Err("SUMIFS needs a range".into());
                };
                let mut keep = vec![true; sum.len()];
                for pair in args[1..].chunks(2) {
                    let Arg::Range(cells) = self.arg(&pair[0], s) else {
                        return Err("SUMIFS needs a range".into());
                    };
                    let crit = self.eval(&pair[1], s);
                    if let Val::Err(e) = crit {
                        return Err(e);
                    }
                    for (k, c) in cells.iter().enumerate() {
                        if k < keep.len() && !matches(c, &crit) {
                            keep[k] = false;
                        }
                    }
                }
                Val::Num(py_sum(sum.iter().zip(keep).filter(|(_, k)| *k).filter_map(
                    |(v, _)| match v {
                        Val::Num(n) => Some(*n),
                        _ => None,
                    },
                )))
            }
            "EDATE" => {
                let d = date_of(self.n(&args[0], s)?);
                let m = self.n(&args[1], s)? as i32;
                let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).ok_or("#VALUE!")?;
                let target = add_months(first, m);
                let last = add_months(target, 1) - Duration::days(1);
                Val::Num(serial(
                    target + Duration::days((d.day() as i64 - 1).min((last.day() - 1) as i64)),
                ))
            }
            "YEAR" => Val::Num(date_of(self.n(&args[0], s)?).year() as f64),
            "MONTH" => Val::Num(date_of(self.n(&args[0], s)?).month() as f64),
            other => return Err(format!("unknown function {other}")),
        })
    }
}

fn text(v: &Val) -> String {
    match v {
        Val::Num(n) => format!("{n}"),
        Val::Str(s) => s.clone(),
        _ => String::new(),
    }
}

fn compare(a: &Val, b: &Val) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (Val::Str(x), Val::Str(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Val::Str(_), _) => Greater,
        (_, Val::Str(_)) => Less,
        _ => num(a)
            .unwrap_or(0.0)
            .partial_cmp(&num(b).unwrap_or(0.0))
            .unwrap_or(Equal),
    }
}

#[cfg(test)]
mod tests {
    use super::super::book::{Book, Sheet};
    use super::*;

    fn book(cells: &[(&str, V)]) -> Book {
        let mut s = Sheet::new("S");
        for (a, v) in cells {
            let (r, c) = split_ref(a);
            s.set(r, c, v.clone());
        }
        Book {
            sheets: vec![s],
            active: 0,
            chart: None,
        }
    }

    #[test]
    fn evaluates_spreadsheet_formulas() {
        let d = |y, m, dd| V::Date(NaiveDate::from_ymd_opt(y, m, dd).unwrap());
        let b = book(&[
            ("A1", V::Float(2.675)),
            ("A2", V::Formula("ROUND(A1,2)".into())),
            ("A3", d(2026, 1, 31)),
            ("A4", V::Formula("EDATE(A3,1)".into())),
            ("A5", V::Formula("YEAR(A4)*100+MONTH(A4)".into())),
            ("A6", V::Formula("SUMIFS(B1:B3,C1:C3,1)".into())),
            ("B1", V::Int(5)),
            ("B2", V::Int(7)),
            ("C1", V::Int(1)),
            ("A7", V::Formula("IF(D1=\"flat\",1/0,MAX(0,-3))".into())),
            ("D1", V::Str("Monthly".into())),
            ("A8", V::Formula("ROUNDUP((10-3)/2,0)+SUM(B1:B2)".into())),
        ]);
        let e = Eval::new(&b);
        assert_eq!(e.cell("S", "A2"), Val::Num(2.68)); // pycel: the shortest text 2.675 rounds up
        assert_eq!(
            e.cell("S", "A4"),
            Val::Num(serial(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()))
        );
        assert_eq!(e.cell("S", "A5"), Val::Num(202602.0));
        assert_eq!(e.cell("S", "A6"), Val::Num(5.0));
        assert_eq!(e.cell("S", "A7"), Val::Num(0.0));
        assert_eq!(e.cell("S", "A8"), Val::Num(16.0));
        assert_eq!(py_sum([0.1; 10]), 1.0);
    }
}
