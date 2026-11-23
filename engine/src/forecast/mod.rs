//! The forecast commands, as `forecast/cashcast_forecast.py`. Reads
//! data/summary.json and cashcast.toml only; never the raw exports.
//!
//! `cashcast-engine forecast-run COMMAND --home DIR [--today YYYY-MM-DD]`
//! - status: what the app knows, and which answers are still missing.
//! - forecast: builds out/Cashcast.xlsx, checks its formulas, prints the
//!   report and saves this run's monthly lines in data/forecasts/.
//! - backtest: the last saved forecast vs the months imported since, a
//!   rolling backtest per line, and the misses by calendar month.
//! - verify-numbers: compares out/numbers_readback.tsv with the model.
//!
//! A file the model cannot read (where the Python model raised a traceback)
//! prints one `error:` line on stderr and exits 1, before any side effect.

pub mod backtest;
pub mod book;
pub mod build;
pub mod formula;
pub mod model;
pub mod verify;

use crate::core::pyjson::{self, Json};
use chrono::{Local, NaiveDate};
use model::{Failure, History, Inputs, MissingInputs, Row, ZERO, fmt, money};
use rust_decimal::Decimal;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Output so far, and the exit code.
struct Out {
    text: String,
    code: i32,
}

macro_rules! say {
    ($o:expr) => { $o.text.push('\n') };
    ($o:expr, $($a:tt)*) => {{ let _ = writeln!($o.text, $($a)*); }};
}

pub fn main(args: &[String]) -> i32 {
    let mut cmd = None;
    let mut home = None;
    let mut today = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--home" => home = it.next().cloned(),
            "--today" => today = it.next().cloned(),
            c if cmd.is_none()
                && ["status", "forecast", "backtest", "verify-numbers"].contains(&c) =>
            {
                cmd = Some(c.to_string())
            }
            other => {
                eprintln!("error: forecast-run: unknown argument {other}");
                return 2;
            }
        }
    }
    let (Some(cmd), Some(home)) = (cmd, home) else {
        eprintln!(
            "usage: forecast-run status|forecast|backtest|verify-numbers --home DIR [--today YYYY-MM-DD]"
        );
        return 2;
    };
    let today = match today {
        Some(t) => match NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                eprintln!("error: --today must be YYYY-MM-DD");
                return 2;
            }
        },
        None => Local::now().date_naive(),
    };
    let home = std::fs::canonicalize(&home).unwrap_or_else(|_| PathBuf::from(&home));
    let mut o = Out {
        text: String::new(),
        code: 0,
    };
    let result = match cmd.as_str() {
        "status" => cmd_status(&home, today, &mut o),
        "forecast" => cmd_forecast(&home, today, &mut o),
        "backtest" => cmd_backtest(&home, today, &mut o),
        _ => cmd_verify_numbers(&home, today, &mut o),
    };
    match result {
        Ok(()) => {
            print!("{}", o.text);
            o.code
        }
        Err(Failure(why)) => {
            eprintln!("error: {why}");
            1
        }
    }
}

fn summary_path(home: &Path) -> PathBuf {
    home.join("data").join("summary.json")
}

/// None: no imported data yet; Python exits 1 with a message on stderr.
fn read_summary(home: &Path) -> Result<Option<Json>, Failure> {
    let path = summary_path(home);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| Failure(e.to_string()))?;
    pyjson::loads(&text)
        .map(Some)
        .ok_or_else(|| Failure("bad summary.json".into()))
}

fn no_data(o: &mut Out) {
    eprintln!("No imported data yet. Run: bin/cashcast import");
    o.code = 1;
}

pub(crate) fn read_toml(path: &Path) -> Result<toml::Table, Failure> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map_err(|e| Failure(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(Failure(e.to_string())),
    }
}

fn cfg_has(cfg: &toml::Table, section: &str, key: &str) -> bool {
    cfg.get(section)
        .and_then(|v| v.as_table())
        .is_some_and(|t| t.contains_key(key))
}

fn items(v: Option<&Json>) -> Result<&Vec<Json>, Failure> {
    match v {
        Some(Json::List(l)) => Ok(l),
        _ => Err(Failure("KeyError in summary.json".into())),
    }
}

fn s(v: Option<&Json>) -> Result<String, Failure> {
    v.map(Json::py_str)
        .ok_or_else(|| Failure("KeyError in summary.json".into()))
}

fn cmd_status(home: &Path, today: NaiveDate, o: &mut Out) -> Result<(), Failure> {
    let Some(summary) = read_summary(home)? else {
        no_data(o);
        return Ok(());
    };
    let cfg = read_toml(&home.join("cashcast.toml"))?;
    let accounts = items(summary.get("accounts"))?;
    say!(o, "## Known\n");
    let files = items(summary.get("files"))?;
    let mut tx = 0i64;
    for f in files {
        match f.get("transactions") {
            Some(Json::Int(n)) => tx += n,
            _ => return Err(Failure("transactions".into())),
        }
    }
    let months = items(summary.get("months"))?;
    let (Some(m0), Some(m1)) = (months.first(), months.last()) else {
        return Err(Failure("IndexError".into()));
    };
    say!(
        o,
        "- Files: {}; transactions: {tx}; months: {} to {}.",
        files.len(),
        m0.py_str(),
        m1.py_str()
    );
    let pay = summary
        .get("pay")
        .ok_or_else(|| Failure("KeyError: pay".into()))?;
    if pay.get("schedule").is_some_and(Json::truthy) {
        let anchor = pay.get("anchor").map_or("None".into(), Json::py_str);
        say!(
            o,
            "- Pay: {}, from {}; last payday {anchor}.",
            s(pay.get("schedule"))?,
            s(pay.get("source"))?
        );
    }
    let suggested = match summary.get("suggested_extra_income") {
        Some(Json::List(l)) => l.len(),
        None => 0,
        Some(_) => return Err(Failure("suggested_extra_income".into())),
    };
    say!(o, "- Recurring extra income found: {suggested} item(s).");

    let mut questions: Vec<String> = Vec::new();
    let cfg_accounts = cfg.get("accounts").and_then(|v| v.as_table());
    let mut guessed = Vec::new();
    for a in accounts {
        let name = s(a.get("name"))?;
        if a.get("guessed").is_some_and(Json::truthy)
            && !cfg_accounts.is_some_and(|t| t.contains_key(&name))
        {
            guessed.push(format!(
                "{name} = {} ({})",
                s(a.get("class"))?,
                s(a.get("class_reason"))?
            ));
        }
    }
    if !guessed.is_empty() {
        questions.push(format!(
            "accounts: confirm account types: {}",
            guessed.join("; ")
        ));
    }
    let given = cfg.get("balances").and_then(|v| v.as_table());
    let mut no_balance = Vec::new();
    for a in accounts {
        let name = s(a.get("name"))?;
        let no_cents = matches!(a.get("balance_cents"), None | Some(Json::Null));
        if s(a.get("class"))? == "cash" && !given.is_some_and(|t| t.contains_key(&name)) && no_cents
        {
            no_balance.push(name);
        }
    }
    if !no_balance.is_empty() {
        questions.push(format!(
            "balances: current balance needed for: {}",
            no_balance.join(", ")
        ));
    }
    if !pay.get("schedule").is_some_and(Json::truthy) && !cfg_has(&cfg, "pay", "schedule") {
        questions.push("pay: no regular pay found; ask for schedule, a recent payday and net pay per cheque (or confirm there is none)".into());
    }
    let truthy_cfg = |v: Option<&toml::Value>| match v {
        None => false,
        Some(toml::Value::Boolean(b)) => *b,
        Some(toml::Value::String(t)) => !t.is_empty(),
        Some(toml::Value::Integer(n)) => *n != 0,
        Some(toml::Value::Float(x)) => *x != 0.0,
        Some(toml::Value::Array(a)) => !a.is_empty(),
        Some(toml::Value::Table(t)) => !t.is_empty(),
        Some(toml::Value::Datetime(_)) => true,
    };
    let use_suggestions = truthy_cfg(
        cfg.get("forecast")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("use_suggestions")),
    );
    if suggested > 0 && !truthy_cfg(cfg.get("extra_income")) && !use_suggestions {
        questions.push(
            "extra_income: confirm which recurring extra income to keep (see the import report)"
                .into(),
        );
    }
    if !cfg.contains_key("floor") && !cfg_has(&cfg, "forecast", "floor") {
        questions
            .push("floor: how much cash to keep untouched (default: 3 months of spending)".into());
    }
    if !cfg.contains_key("rent") {
        questions.push(
            "rent (optional): fixed monthly rent or mortgage, if it will differ from history"
                .into(),
        );
    }
    if !cfg_has(&cfg, "forecast", "baseline") {
        questions.push(
            "spending (optional): your own monthly spending estimate, and the spending method"
                .into(),
        );
    }
    say!(o, "\n## Questions to ask\n");
    for (i, q) in questions.iter().enumerate() {
        say!(o, "{}. {q}", i + 1);
    }
    if questions.is_empty() {
        say!(o, "None. Run: bin/cashcast forecast");
    }
    match model::inputs_from(&cfg, &summary, today)? {
        Ok(_) => say!(
            o,
            "\nReady: the forecast can run now; unanswered items use defaults."
        ),
        Err(MissingInputs(p)) => say!(o, "\nBlocked: the forecast needs: {}", p.join("; ")),
    }
    Ok(())
}

/// The inputs, or None after printing what is missing (exit 2) or that no data exists (exit 1).
fn load_inputs(
    home: &Path,
    today: NaiveDate,
    o: &mut Out,
) -> Result<Option<(Inputs, History)>, Failure> {
    let Some(summary) = read_summary(home)? else {
        no_data(o);
        return Ok(None);
    };
    let cfg = read_toml(&home.join("cashcast.toml"))?;
    match model::inputs_from(&cfg, &summary, today)? {
        Ok(inputs) => Ok(Some((inputs, model::load_history(&summary)?))),
        Err(MissingInputs(problems)) => {
            say!(
                o,
                "The forecast needs these answers first (add them to cashcast.toml):"
            );
            for p in problems {
                say!(o, "- {p}");
            }
            o.code = 2;
            Ok(None)
        }
    }
}

struct Summary {
    rows: Vec<Row>,
    income: Decimal,
    expenses: Decimal,
    taxes: Decimal,
    to_invest: Decimal,
    low: Decimal,
    low_month: String,
}

fn run_summary(inputs: &Inputs, hist: &History) -> Result<Summary, Failure> {
    let rows = model::run(inputs, hist)?;
    let mut low = rows
        .first()
        .ok_or_else(|| Failure("ValueError: min() arg is an empty sequence".into()))?;
    for r in &rows {
        if r.month_end < low.month_end {
            low = r;
        }
    }
    let total = |f: fn(&Row) -> Decimal| rows.iter().map(f).fold(ZERO, |a, b| a + b);
    Ok(Summary {
        income: total(|r| r.income),
        expenses: total(|r| r.expenses),
        taxes: total(|r| r.taxes),
        to_invest: total(|r| r.to_invest),
        low: low.month_end,
        low_month: model::month_label(low.month),
        rows,
    })
}

fn sensitivities(
    inputs: &Inputs,
    hist: &History,
) -> Result<Vec<(String, Summary, Summary)>, Failure> {
    let mut out = Vec::new();
    let tests: Vec<&str> = if inputs.rent_charged.is_some() {
        vec![
            "Spending (excl. rent)",
            "Net pay per cheque",
            "Extra income",
            "Opening cash",
            "Rent",
        ]
    } else {
        vec![
            "Spending (excl. rent)",
            "Net pay per cheque",
            "Extra income",
            "Opening cash",
        ]
    };
    for name in tests {
        let make = |f: Decimal| {
            let (mut i, mut h) = (inputs.clone(), hist.clone());
            match name {
                "Spending (excl. rent)" => {
                    for (c, v) in h.spending.iter_mut() {
                        if Some(&*c) != inputs.rent_category.as_ref() {
                            v.iter_mut().for_each(|x| *x *= f);
                        }
                    }
                }
                "Net pay per cheque" => {
                    i.net_h1 *= f;
                    i.net_h2 *= f;
                }
                "Extra income" => i.extra_income.iter_mut().for_each(|x| x.amount *= f),
                "Opening cash" => i.opening_accounts.iter_mut().for_each(|(_, v)| *v *= f),
                _ => i.rent_charged = i.rent_charged.map(|r| r * f),
            }
            run_summary(&i, &h)
        };
        let lo = make(Decimal::new(9, 1))?;
        let hi = make(Decimal::new(11, 1))?;
        out.push((name.to_string(), lo, hi));
    }
    Ok(out)
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

fn cmd_forecast(home: &Path, today: NaiveDate, o: &mut Out) -> Result<(), Failure> {
    let Some((inputs, hist)) = load_inputs(home, today, o)? else {
        return Ok(());
    };
    let book = build::build(&inputs, &hist);
    let base = run_summary(&inputs, &hist)?;
    let diffs = verify::compare(&base.rows, &verify::from_book(&book, inputs.horizon)?);
    let check = if diffs.is_empty() {
        format!(
            "formulas match the Python model to the cent ({} months x 8 columns)",
            inputs.horizon
        )
    } else {
        format!("{} DIFFERENCES", diffs.len())
    };
    let sens = sensitivities(&inputs, &hist)?;

    // Side effects only after everything that could fail.
    let out = home.join("out");
    std::fs::create_dir_all(&out).map_err(|e| Failure(e.to_string()))?;
    chmod(&out, 0o700);
    let xlsx = out.join("Cashcast.xlsx");
    book.save(&xlsx).map_err(Failure)?;
    chmod(&xlsx, 0o600);

    let cur = if inputs.base_currency.is_empty() {
        String::new()
    } else {
        format!(" ({})", inputs.base_currency)
    };
    say!(
        o,
        "# Cashcast forecast{cur}: {} for {} months\n",
        model::month_label(inputs.start),
        inputs.horizon
    );
    if !inputs.assumptions.is_empty() {
        say!(o, "## Defaults used\n");
        for note in &inputs.assumptions {
            say!(o, "- {note}");
        }
        say!(o);
    }
    say!(
        o,
        "{:9} {:>7} {:>12} {:>12} {:>11} {:>12} {:>11} {:>12}",
        "Month",
        "Cheques",
        "Income",
        "Expenses",
        "Net",
        "Start",
        "To invest",
        "End"
    );
    for r in &base.rows {
        say!(
            o,
            "{} {:>7} {:>12} {:>12} {:>11} {:>12} {:>11} {:>12}",
            model::month_label(r.month),
            r.paycheques,
            money(r.income),
            money(r.expenses),
            money(r.net_cash_flow),
            money(r.month_start),
            money(r.to_invest),
            money(r.month_end)
        );
    }
    say!(
        o,
        "\nTotals: income {}; expenses {}; taxes {}; to invest {}.",
        money(base.income),
        money(base.expenses),
        money(base.taxes),
        money(base.to_invest)
    );
    say!(
        o,
        "Lowest month-end cash: {} in {}. Cash floor: {}.",
        money(base.low),
        base.low_month,
        money(inputs.floor())
    );
    let t12 = hist.monthly_spending(inputs.start);
    say!(
        o,
        "Spending, last 12 months: {} per month on average. Baseline set: {}.",
        money(t12),
        money(inputs.baseline)
    );
    say!(
        o,
        "\n## Sensitivity (-10% | +10%): change in total to invest; lowest month-end cash\n"
    );
    for (name, lo, hi) in &sens {
        say!(
            o,
            "- {name}: {} | {}; lowest {} ({}) | {} ({})",
            fmt(lo.to_invest - base.to_invest, 0, true),
            fmt(hi.to_invest - base.to_invest, 0, true),
            money(lo.low),
            lo.low_month,
            money(hi.low),
            hi.low_month
        );
    }
    let snap = backtest::save_snapshot(home, &inputs, &base.rows, today)?;
    let rel = |p: &Path| p.strip_prefix(home).unwrap_or(p).display().to_string();
    say!(o, "\nWorkbook: {}. Formula check: {check}.", rel(&xlsx));
    say!(
        o,
        "Saved this forecast for later scoring: {}. Score it later with: bin/cashcast backtest",
        rel(&snap)
    );
    for d in diffs.iter().take(20) {
        say!(o, "{d}");
    }
    o.code = if diffs.is_empty() { 0 } else { 1 };
    Ok(())
}

fn pct(v: Option<Decimal>) -> String {
    match v {
        None => "n/a".into(),
        Some(x) => format!(
            "{:.1}%",
            (x * Decimal::from(100))
                .round_dp_with_strategy(1, rust_decimal::RoundingStrategy::MidpointNearestEven)
        ),
    }
}

fn cmd_backtest(home: &Path, today: NaiveDate, o: &mut Out) -> Result<(), Failure> {
    use backtest::{LABELS, LINES, label};
    let Some((inputs, hist)) = load_inputs(home, today, o)? else {
        return Ok(());
    };
    let (Some(h0), Some(h1)) = (hist.months.first(), hist.months.last()) else {
        return Err(Failure("IndexError".into()));
    };
    say!(
        o,
        "# Forecast vs actual ({} to {} imported)\n",
        model::month_label(*h0),
        model::month_label(*h1)
    );
    say!(
        o,
        "Miss = forecast - actual. Plus means the forecast was too high.\n"
    );
    let snap = backtest::latest_snapshot(home, &hist, today)?;
    say!(o, "## Last saved forecast vs actual\n");
    let mut names: Vec<&str> = LINES.to_vec();
    names.push("month_end");
    match snap {
        None => {
            say!(
                o,
                "No saved forecast covers an imported month yet. Each `forecast` run now saves one;"
            );
            say!(
                o,
                "the backtest below scores the model on the past instead.\n"
            );
        }
        Some(snap) => {
            let rows = backtest::compare_snapshot(&snap, &hist)?;
            say!(
                o,
                "Forecast made {} (method: {}); {} month(s) imported since.\n",
                s(snap.get("made"))?,
                s(snap.get("method"))?,
                rows.len()
            );
            let head: Vec<String> = names
                .iter()
                .map(|n| format!("{:>14}", label(n).chars().take(14).collect::<String>()))
                .collect();
            say!(o, "{:8} {}", "Month", head.join(" "));
            for r in &rows {
                let cells: Vec<String> = names
                    .iter()
                    .map(|n| format!("{:>14}", fmt(r.forecast(n) - r.actual(n), 0, true)))
                    .collect();
                say!(o, "{:8} {}", r.month, cells.join(" "));
            }
            let mut cats: Vec<(String, Decimal)> = Vec::new();
            for r in &rows {
                for (c, (f, a)) in &r.categories {
                    match cats.iter_mut().find(|(k, _)| k == c) {
                        Some(slot) => slot.1 += *f - *a,
                        None => cats.push((c.clone(), *f - *a)),
                    }
                }
            }
            cats.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()));
            cats.truncate(8);
            if !cats.is_empty() {
                say!(o, "\nBiggest category misses (sum over those months):");
                for (c, miss) in &cats {
                    say!(o, "- {c}: {}", fmt(*miss, 0, true));
                }
            }
            say!(o);
        }
    }
    let starts = backtest::origins(&hist);
    say!(
        o,
        "## Backtest: {} origins, up to {} months ahead, method {}\n",
        starts.len(),
        backtest::HORIZON,
        inputs.method
    );
    if starts.is_empty() {
        say!(
            o,
            "Needs more than {} months of history.",
            backtest::MIN_HISTORY
        );
        return Ok(());
    }
    let misses = backtest::backtest(&inputs, &hist)?;
    let lines = backtest::by_line(&misses);
    let get = |k: &str| {
        lines
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    };
    say!(
        o,
        "{:28} {:>14} {:>11} {:>7}",
        "Line",
        "Mean abs miss",
        "Mean miss",
        "WAPE"
    );
    for line in &names {
        let sc = backtest::score(get(line));
        say!(
            o,
            "{:28} {:>14} {:>11} {:>7}",
            label(line),
            fmt(sc.mean_abs, 0, false),
            fmt(sc.bias, 0, true),
            pct(sc.wape)
        );
    }
    let net = get("net_cash_flow");
    let q = backtest::quarters(net);
    let qs = backtest::score(&q.iter().collect::<Vec<_>>());
    say!(
        o,
        "{:28} {:>14} {:>11} {:>7}",
        "Net cash flow, 3-month sums",
        fmt(qs.mean_abs, 0, false),
        fmt(qs.bias, 0, true),
        pct(qs.wape)
    );
    say!(o, "\nNet cash flow WAPE by method (set `forecast.method` to the lowest):");
    for (m, wape) in backtest::by_method(&inputs, &hist) {
        let mark = if m == inputs.method { " (now)" } else { "" };
        say!(o, "- {m}: {}{mark}", pct(wape));
    }
    let mut exact = backtest::if_exact(&misses);
    // "(untracked)" is only the baseline padding (forecast side only), not a line to fix.
    exact.retain(|(k, w)| w.is_some() && k != "cat:(untracked)");
    exact.sort_by(|a, b| a.1.cmp(&b.1));
    say!(o, "\nNet cash flow WAPE if one line were exact (lowest = where accuracy is lost):");
    for (k, wape) in exact.iter().take(6) {
        let name = k
            .strip_prefix("cat:")
            .map_or_else(|| label(k).to_string(), |c| format!("category {c}"));
        say!(o, "- {name}: {}", pct(*wape));
    }
    let mut cat_scores: Vec<(String, backtest::Score)> = lines
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("cat:")
                .map(|c| (c.to_string(), backtest::score(v)))
        })
        .collect();
    cat_scores.sort_by(|a, b| b.1.mean_abs.cmp(&a.1.mean_abs));
    say!(o, "\nBiggest category misses (mean per month):");
    for (c, sc) in cat_scores.iter().take(8) {
        say!(
            o,
            "- {c}: {} abs, {} bias",
            fmt(sc.mean_abs, 0, false),
            fmt(sc.bias, 0, true)
        );
    }
    say!(
        o,
        "\nMean miss by calendar month of the forecast month (repeated signs are patterns to learn):"
    );
    let moys = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    say!(
        o,
        "{:22} {}",
        "Line",
        moys.map(|m| format!("{m:>6}")).join(" ")
    );
    let at = |m: NaiveDate| hist.months.iter().position(|x| *x == m);
    for line in [
        "net_salary",
        "extra_income",
        "taxes",
        "spending_excl_rent",
        "net_cash_flow",
    ] {
        let mut by_moy: Vec<Vec<Decimal>> = vec![Vec::new(); 12];
        for m in get(line) {
            if m.ahead == 1
                && let Some(i) = at(m.origin)
            {
                use chrono::Datelike;
                by_moy[hist.months[i].month() as usize - 1].push(m.error());
            }
        }
        let cells: Vec<String> = by_moy
            .iter()
            .map(|v| {
                if v.is_empty() {
                    format!("{:>6}", "")
                } else {
                    let mean = v.iter().fold(ZERO, |a, b| a + b) / Decimal::from(v.len());
                    format!("{:>6}", fmt(mean, 0, true))
                }
            })
            .collect();
        say!(
            o,
            "{:22} {}",
            label(line).chars().take(22).collect::<String>(),
            cells.join(" ")
        );
    }
    say!(
        o,
        "\nExtra income by type, mean per month over the backtest months:"
    );
    let first_origin = misses
        .iter()
        .map(|m| m.origin)
        .min()
        .ok_or_else(|| Failure("IndexError".into()))?;
    let idx: Vec<usize> = (0..hist.months.len())
        .filter(|&i| hist.months[i] >= first_origin)
        .collect();
    let n = Decimal::from(idx.len());
    let mean = |v: &[Decimal]| idx.iter().fold(ZERO, |a, &i| a + v[i]) / n;
    let mut income: Vec<&(String, Vec<Decimal>)> = hist.income.iter().collect();
    income.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in income {
        say!(o, "- {k}: {}", fmt(mean(v), 0, false));
    }
    say!(
        o,
        "- salary_off_cycle: {}",
        fmt(mean(&hist.salary_off_cycle), 0, false)
    );
    let extra = inputs
        .extra_income
        .iter()
        .fold(ZERO, |a, x| a + x.amount * Decimal::from(x.months.len()))
        / Decimal::from(12);
    say!(
        o,
        "- forecast extra income (config): {}",
        fmt(extra, 0, false)
    );
    say!(
        o,
        "- invested (out, not in net cash flow): {}",
        fmt(mean(&hist.invested), 0, false)
    );
    let _ = LABELS;
    Ok(())
}

fn cmd_verify_numbers(home: &Path, today: NaiveDate, o: &mut Out) -> Result<(), Failure> {
    let Some((inputs, hist)) = load_inputs(home, today, o)? else {
        return Ok(());
    };
    let readback = home.join("out").join("numbers_readback.tsv");
    let rows = model::run(&inputs, &hist)?;
    let actual = verify::from_numbers(&readback, inputs.horizon)?;
    let diffs = verify::compare(&rows, &actual);
    say!(
        o,
        "Numbers check: {} months x 8 columns compared; {} differences.",
        inputs.horizon,
        diffs.len()
    );
    for d in diffs.iter().take(20) {
        say!(o, "{d}");
    }
    let _ = std::fs::remove_file(&readback);
    o.code = if diffs.is_empty() { 0 } else { 1 };
    Ok(())
}
