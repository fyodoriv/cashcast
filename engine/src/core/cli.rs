//! The commands ported to Rust, run by `bin/cashcast` (decision 0010).
//!
//! `cashcast-engine cli ARGS…` runs any command and exits with its code. It
//! parses as Python's argparse did (`argparse.rs`, `commands.rs`): usage
//! errors, `--help` per command and odd argument orders included. Output,
//! exit codes, files and log events match the Python code byte for byte
//! (`scripts/rust-parity.sh`). Where Python stopped with a traceback (exit 1),
//! Rust prints one `error:` line and exits 1 (AppError kind `crash`, and the
//! kinds of the errors Python did not catch: `oserror`, `tomldecodeerror`…).

use super::argparse::{self, Namespace, Val};
use super::paths::{self, AppError, UserHome};
use super::pyjson::{self, Json};
use super::{commands, consent, events, settings, telemetry, users};
use chrono::NaiveDate;

/// Commands that run without a resolved user (Python `cli.NO_USER`).
const NO_USER: [&str; 9] = [
    "users",
    "connectors",
    "knowledge",
    "web",
    "start",
    "stop",
    "help",
    "actions",
    "config",
];

/// `cli.HELP`, word for word.
const HELP: &str = r#"usage: bin/cashcast [--user NAME] [COMMAND]

Start (no command does this): set up, start the web app, refresh, print what matters now
  start | stop | actions [--json] | config [show | set KEY VALUE] | help
  weekly [--dry-run] [--balance ID=AMOUNT ...]   the Sunday session in one run: every check, the forecast, this week's moves

Forecast (the original cashcast):
  doctor | init | import | status | forecast | backtest | numbers | totals | first-error
  columns                                   header names and cell shapes of each file, no values
Users and privacy:
  users list | users add NAME --province XX [--display NAME] | users adopt --province XX
  consent list | consent grant SCOPE | consent revoke SCOPE
  telemetry [status | on | off | send [--dry-run]]   opt-in anonymous usage counts (PostHog EU)
Strategy (the main number is your net total; every option is ranked by what it adds to it):
  net-total                                 net total, its trend, and the options inside your [boundaries]
  strategy [--section ID ...] [--market]   full plan → out/strategy.md
  review | budget | tax | invest [--market] | alternatives | payments | spending
  rrsp                                      this year's RRSP contributions against your limit
  holdings                                  investments and crypto: ACB and gains
  cards now [--balance CARD=AMOUNT ...]     which card to use now: cap left, reset date, credit left
  charges [find WORD]                       duplicate subscriptions, new or pricier charges; find: one merchant's charges
Groceries (prices you read on store pages; carts filled in your browser, you check out):
  grocery items | grocery show ITEM | grocery best ITEM | grocery stale --items A,B
  grocery compare --items A,B [--days N]   each store's total with fees and promos, full baskets first
  grocery save --file ROWS.json | grocery fee STORE --delivery N [--free-over N] [--service N] | grocery fees
  grocery order STORE --total N | grocery orders | grocery prefs | grocery import --from OLD.sqlite
MoneyWiz (record cash by hand; one command adds and fixes the rest):
  moneywiz status | moneywiz coverage | moneywiz sync [--dry-run] [--no-download] [--resend]
  moneywiz edit [--dry-run] [--fixes] [--account NAME --archived yes|no] | moneywiz probe [--ui]
Documents (private notes, for example moved from Notion):
  notion import EXPORT.zip [--delete-source]   read a Notion "Markdown & CSV" export, offline
Paperwork (private, sealed on this Mac, Terminal only):
  paperwork [list | steps | sources | doctor | show | presence | mark | set | trip | attach | files ...]
  citizenship [status | answers | questions | collect-all DIR | put FILE | proof | report ...]
Reminders, services, knowledge:
  reminders preview | reminders sync          one Apple Reminders list per user
  calendar preview | calendar sync            dated deadlines in one Apple Calendar per user
  changes [list | show ID | done ID | undo ID]   bank and card settings to change (browser, with your yes)
  optimize [--input FILE]                     where new money goes to reach your target weights
  connectors list | connectors show SERVICE | action SERVICE ACTION
  api balances SERVICE | api download SERVICE | api etrade-auth | api record SERVICE --set NAME=AMOUNT
  fetch plan [--all] | fetch collect [--minutes N]
  sync all | sync status | sync install | sync uninstall   Reminders, Calendar and Notion tasks in step (hourly once installed)
  sync sheets SHEET_ID [RANGE]
  knowledge check | knowledge list | knowledge propose FILE   (review a fact update before a PR)
  knowledge refresh   (live Bank of Canada rates and inflation vs the planning facts)
Web app (the main view; this computer only):
  web [--port N]                          plan ahead, forecast, strategy and reference views
"#;

enum Cmd {
    Actions(bool),
    /// `weekly [--dry-run] [--balance ID=AMOUNT ...]`: the Sunday session in one run.
    /// `--dry-run`, balances, `--sync` (write Reminders and Calendar).
    Weekly(bool, Vec<String>, bool),
    /// `api record SERVICE --set NAME=AMOUNT,... [--currency CAD]`
    ApiRecord {
        service: Option<String>,
        set: Option<String>,
        currency: String,
    },
    /// `api balances|download|etrade-auth SERVICE [TARGET] [GID]`: the network calls.
    Api {
        action: String,
        service: String,
        target: Option<String>,
        gid: Option<String>,
    },
    TelemetrySend,
    /// `transfers --balance ID=AMOUNT ...`
    Transfers(Vec<String>),
    FirstError,
    Numbers,
    /// `cards now --balance CARD=AMOUNT ...` and `--today`, as given.
    /// Balances, `--today`, then the engine words (`missed`, `--since DATE`).
    Cards(Vec<String>, Option<String>, Vec<String>),
    /// `charges [find WORD]` and `--today`, as given.
    Charges(Option<String>, Option<String>),
    /// `grocery ACTION [NAME] [options]`: the grocery price database.
    Grocery(Box<Grocery>),
    Help,
    Stop,
    Init,
    /// An engine report behind the aggregates gate: import, totals, columns,
    /// holdings. The second field is `--today`, as given.
    Report(&'static str, Option<String>),
    /// import, then `forecast` or `backtest`; `--today` as given.
    Forecast(&'static str, Option<String>),
    UsersList,
    UsersAdd {
        name: Option<String>,
        province: String,
        display: Option<String>,
    },
    UsersAdopt {
        province: String,
        display: Option<String>,
    },
    ConfigShow,
    ConfigSet {
        key: Option<String>,
        value: Option<String>,
    },
    ConsentList,
    ConsentRevoke(Option<String>),
    ConsentGrant(Option<String>),
    TelemetryStatus,
    TelemetrySwitch(bool),
    TelemetryDryRun,
    Knowledge {
        action: String,
        file: Option<String>,
    },
    Strategy(Vec<String>),
    Section(&'static str),
    NetTotal,
    InvestPlan,
    Rrsp,
    Optimize(Option<String>),
    Tasks {
        action: String,
        id: Option<String>,
        result: Option<String>,
        reason: Option<String>,
        source: String,
        json: bool,
    },
    Changes {
        action: String,
        id: Option<String>,
    },
    Preview(&'static str),
    /// `sync all|install|uninstall|sheets [DEST] [RANGE] [--allow-removals]`.
    Sync {
        action: String,
        dest: Option<String>,
        range: Option<String>,
        allow_removals: bool,
    },
    Start(Option<String>),
    Connectors(String, Option<String>),
    Action(String, String),
    FetchPlan(bool),
    FetchCollect(i64),
    Moneywiz(Box<Mw>),
    Notion {
        source: String,
        delete: bool,
    },
    Web {
        port: i64,
    },
}

/// `grocery ACTION [NAME]` and its options, as argparse reads them.
#[derive(Debug, Default)]
pub struct Grocery {
    action: String,
    name: Option<String>,
    items: Option<String>,
    file: Option<String>,
    from: Option<String>,
    days: Option<i64>,
    organic_premium: Option<String>,
    delivery: Option<String>,
    free_over: Option<String>,
    service: Option<String>,
    pct_off: Option<String>,
    max_off: Option<String>,
    min_order: Option<String>,
    total: Option<String>,
    note: Option<String>,
}

/// `moneywiz ACTION [STEP]` and its options, as argparse reads them.
#[derive(Debug, Default)]
pub struct Mw {
    action: String,
    step: Option<String>,
    name: Option<String>,
    ops: Option<String>,
    dry_run: bool,
    yes: bool,
    ui: bool,
    fixes: bool,
    no_download: bool,
    resend: bool,
    account: Option<String>,
    archived: Option<String>,
    from: Option<String>,
    to: Option<String>,
    check: bool,
    list: bool,
    record_since: Option<String>,
    today: Option<String>,
}

impl Cmd {
    fn name(&self) -> &'static str {
        match self {
            Cmd::UsersList | Cmd::UsersAdd { .. } | Cmd::UsersAdopt { .. } => "users",
            Cmd::ConfigShow | Cmd::ConfigSet { .. } => "config",
            Cmd::ConsentList | Cmd::ConsentRevoke(_) | Cmd::ConsentGrant(_) => "consent",
            Cmd::Knowledge { .. } => "knowledge",
            Cmd::Strategy(_) => "strategy",
            Cmd::Section(name) => name,
            Cmd::NetTotal => "net-total",
            Cmd::InvestPlan => "invest-plan",
            Cmd::Rrsp => "rrsp",
            Cmd::Weekly(..) => "weekly",
            Cmd::Optimize(_) => "optimize",
            Cmd::Tasks { .. } => "tasks",
            Cmd::Changes { .. } => "changes",
            Cmd::Preview(name) => name.trim_end_matches("-sync"),
            Cmd::Sync { .. } => "sync",
            Cmd::Start(_) => "start",
            Cmd::Connectors(..) => "connectors",
            Cmd::Action(..) => "action",
            Cmd::FetchPlan(_) | Cmd::FetchCollect(_) => "fetch",
            Cmd::Moneywiz(_) => "moneywiz",
            Cmd::Notion { .. } => "notion",
            Cmd::Web { .. } => "web",
            Cmd::Help => "help",
            Cmd::Actions(_) => "actions",
            Cmd::ApiRecord { .. } | Cmd::Api { .. } => "api",
            Cmd::TelemetrySend => "telemetry",
            Cmd::Transfers(_) => "transfers",
            Cmd::FirstError => "first-error",
            Cmd::Numbers => "numbers",
            Cmd::Cards(..) => "cards",
            Cmd::Charges(..) => "charges",
            Cmd::Grocery(_) => "grocery",
            Cmd::Stop => "stop",
            Cmd::Init => "init",
            Cmd::Report(name, _) | Cmd::Forecast(name, _) => name,
            _ => "telemetry",
        }
    }
}

/// Python `date.fromisoformat` for `--today`: `YYYY-MM-DD`, `YYYYMMDD` and
/// the ISO week forms (`YYYY-Www[-D]`, `YYYYWww[D]`); Err is Python's message.
fn iso_date(text: &str) -> Result<NaiveDate, String> {
    let bad = || format!("Invalid isoformat string: {}", argparse::py_repr(text));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let num = |s: &str| s.parse::<u32>().unwrap_or(0);
    if !text.is_ascii() || text.len() < 5 || !digits(&text[..4]) {
        return Err(bad());
    }
    let year = num(&text[..4]) as i32;
    if year == 0 && text.contains('W') {
        return Err(bad());
    }
    let dashed = text.as_bytes()[4] == b'-';
    let rest = if dashed { &text[5..] } else { &text[4..] };
    if let Some(w) = rest.strip_prefix('W') {
        let (week, day) = match (w.len(), dashed) {
            (2, _) => (w, "1"),
            (4, true) if w.as_bytes()[2] == b'-' => (&w[..2], &w[3..]),
            (3, false) => (&w[..2], &w[2..]),
            _ => return Err(bad()),
        };
        if !digits(week) || !digits(day) {
            return Err(bad());
        }
        let (week, day) = (num(week), num(day));
        if !(1..=7).contains(&day) {
            return Err(format!("Invalid weekday: {day} (range is [1, 7])"));
        }
        let wd = chrono::Weekday::try_from((day - 1) as u8).unwrap_or(chrono::Weekday::Mon);
        return NaiveDate::from_isoywd_opt(year, week, wd)
            .ok_or_else(|| format!("Invalid week: {week}"));
    }
    let (month, day) = match (rest.len(), dashed) {
        (5, true) if rest.as_bytes()[2] == b'-' => (&rest[..2], &rest[3..]),
        (4, false) => (&rest[..2], &rest[2..]),
        _ => return Err(bad()),
    };
    if !digits(month) || !digits(day) {
        return Err(bad());
    }
    let (month, day) = (num(month), num(day));
    if year == 0 {
        return Err("year 0 is out of range".into());
    }
    if !(1..=12).contains(&month) {
        return Err("month must be in 1..12".into());
    }
    NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| "day is out of range for month".into())
}

/// `--today` as the engine subprocesses read it (`%Y-%m-%d`), or None when
/// it is not a date.
fn engine_today(text: &str) -> Option<String> {
    iso_date(text)
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

fn build(command: &str, ns: &Namespace) -> Cmd {
    let s = |d: &str| ns.str(d);
    let filled = |d: &str| ns.str(d).filter(|v| !v.is_empty());
    let action = s("action").unwrap_or_default();
    match command {
        "start" => Cmd::Start(None),
        "stop" => Cmd::Stop,
        "help" => Cmd::Help,
        "init" => Cmd::Init,
        "import" => Cmd::Report("import", None),
        "totals" => Cmd::Report("totals", None),
        "columns" => Cmd::Report("columns", None),
        "holdings" => Cmd::Report("holdings", None),
        "status" => Cmd::Forecast("status", None),
        "forecast" => Cmd::Forecast("forecast", None),
        "backtest" => Cmd::Forecast("backtest", None),
        "numbers" => Cmd::Numbers,
        "first-error" => Cmd::FirstError,
        "actions" => Cmd::Actions(ns.flag("json")),
        "config" if action == "set" => Cmd::ConfigSet {
            key: filled("key"),
            value: s("value"),
        },
        "config" => Cmd::ConfigShow,
        "users" => {
            let province = s("province").unwrap_or_default();
            let display = s("display");
            match action.as_str() {
                "list" => Cmd::UsersList,
                "add" => Cmd::UsersAdd {
                    name: filled("name"),
                    province,
                    display,
                },
                _ => Cmd::UsersAdopt { province, display },
            }
        }
        "consent" => match action.as_str() {
            "list" => Cmd::ConsentList,
            "revoke" => Cmd::ConsentRevoke(s("scope")),
            _ => Cmd::ConsentGrant(s("scope")),
        },
        "telemetry" => match (action.as_str(), ns.flag("dry_run")) {
            ("on", _) => Cmd::TelemetrySwitch(true),
            ("off", _) => Cmd::TelemetrySwitch(false),
            ("send", true) => Cmd::TelemetryDryRun,
            ("send", false) => Cmd::TelemetrySend,
            _ => Cmd::TelemetryStatus,
        },
        "strategy" => {
            if ns.flag("market") {
                live_market();
            }
            Cmd::Strategy(ns.list("section"))
        }
        "invest" => {
            if ns.flag("market") {
                live_market();
            }
            Cmd::Section("invest")
        }
        "review" => Cmd::Section("review"),
        "budget" => Cmd::Section("budget"),
        "tax" => Cmd::Section("tax"),
        "alternatives" => Cmd::Section("alternatives"),
        "payments" => Cmd::Section("payments"),
        "spending" => Cmd::Section("spending"),
        "net-total" => Cmd::NetTotal,
        "invest-plan" => Cmd::InvestPlan,
        "weekly" => Cmd::Weekly(ns.flag("dry_run"), ns.list("balance"), ns.flag("sync")),
        "rrsp" => Cmd::Rrsp,
        "reminders" => Cmd::Preview(if action == "sync" {
            "reminders-sync"
        } else {
            "reminders"
        }),
        "calendar" => Cmd::Preview(if action == "sync" {
            "calendar-sync"
        } else {
            "calendar"
        }),
        "tasks" => Cmd::Tasks {
            action,
            id: s("id"),
            result: s("result"),
            reason: s("reason"),
            source: s("source").unwrap_or_else(|| "local".into()),
            json: ns.flag("json"),
        },
        "changes" => Cmd::Changes {
            action,
            id: s("id"),
        },
        "transfers" => Cmd::Transfers(ns.list("balance")),
        "cards" => {
            let mut extra = Vec::new();
            if ns.str("action").as_deref() == Some("missed") {
                extra.push("missed".to_string());
                if let Some(d) = ns.str("since") {
                    extra.extend(["--since".to_string(), d]);
                }
            }
            Cmd::Cards(ns.list("balance"), None, extra)
        }
        "charges" => Cmd::Charges(
            (action == "find").then(|| s("word").unwrap_or_default()),
            None,
        ),
        "grocery" => Cmd::Grocery(Box::new(Grocery {
            action,
            name: s("name"),
            items: s("items"),
            file: s("file"),
            from: s("from"),
            days: match ns.get("days") {
                Val::Int(n) => Some(*n),
                _ => None,
            },
            organic_premium: s("organic_premium"),
            delivery: s("delivery"),
            free_over: s("free_over"),
            service: s("service"),
            pct_off: s("pct_off"),
            max_off: s("max_off"),
            min_order: s("min_order"),
            total: s("total"),
            note: s("note"),
        })),
        "optimize" => Cmd::Optimize(s("input")),
        "connectors" => Cmd::Connectors(action, s("service")),
        "action" => Cmd::Action(
            s("service").unwrap_or_default(),
            s("action_id").unwrap_or_default(),
        ),
        "api" if action == "record" => Cmd::ApiRecord {
            service: s("service"),
            set: s("set"),
            currency: s("currency").unwrap_or_else(|| "CAD".into()),
        },
        "api" => Cmd::Api {
            action,
            service: s("service").unwrap_or_default(),
            target: s("target"),
            gid: s("gid"),
        },
        "fetch" if action == "plan" => Cmd::FetchPlan(ns.flag("all")),
        "fetch" => Cmd::FetchCollect(match ns.get("minutes") {
            Val::Int(n) => *n,
            _ => 60,
        }),
        "sync" if action == "status" => Cmd::Preview("sync"),
        "sync" => Cmd::Sync {
            action,
            dest: s("dest"),
            range: s("range"),
            allow_removals: ns.flag("allow_removals"),
        },
        "moneywiz" => Cmd::Moneywiz(Box::new(Mw {
            action,
            step: s("step"),
            name: s("name"),
            ops: s("ops"),
            dry_run: ns.flag("dry_run"),
            yes: ns.flag("yes"),
            ui: ns.flag("ui"),
            fixes: ns.flag("fixes"),
            no_download: ns.flag("no_download"),
            resend: ns.flag("resend"),
            account: s("account"),
            archived: s("archived"),
            from: s("from"),
            to: s("to"),
            check: ns.flag("check"),
            list: ns.flag("list"),
            record_since: s("record_since"),
            today: None,
        })),
        "notion" => Cmd::Notion {
            source: s("source").unwrap_or_default(),
            delete: ns.flag("delete_source"),
        },
        "web" => Cmd::Web {
            port: match ns.get("port") {
                Val::Int(n) => *n,
                _ => 0,
            },
        },
        _ => Cmd::Knowledge {
            action,
            file: s("file"),
        },
    }
}

/// Whether Python's handler reads `--today` as a date (`_today`), so a bad
/// value stops it; the others pass it on as text or never read it.
fn reads_today(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::Strategy(_)
        | Cmd::Section(_)
        | Cmd::NetTotal
        | Cmd::InvestPlan
        | Cmd::Rrsp
        | Cmd::Preview(_)
        | Cmd::Tasks { .. }
        | Cmd::Start(_)
        | Cmd::Weekly(..)
        | Cmd::Grocery(_)
        | Cmd::FetchPlan(_) => true,
        // The engine subprocesses get `--today` only when it is given.
        Cmd::Report(_, given)
        | Cmd::Forecast(_, given)
        | Cmd::Cards(_, given, _)
        | Cmd::Charges(_, given) => given.is_some(),
        // Only `coverage` and `sync` pass `--today` on.
        Cmd::Moneywiz(mw) => {
            mw.today.is_some()
                && !matches!(
                    mw.action.as_str(),
                    "status" | "probe" | "categories" | "edit"
                )
        }
        Cmd::Changes { action, id } => id.is_some() || !matches!(action.as_str(), "done" | "undo"),
        Cmd::Sync { action, dest, .. } => {
            action == "all"
                || (action == "sheets" && dest.as_deref().is_some_and(|d| !d.is_empty()))
        }
        Cmd::Knowledge { action, file } => {
            action == "check"
                || (action == "propose" && file.as_deref().is_some_and(|f| !f.is_empty()))
        }
        _ => false,
    }
}

/// `--market`: the invest section fetches live watchlist prices (no I/O here).
fn live_market() {
    crate::strategy::invest::LIVE_MARKET.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// `paperwork` and `citizenship` take free words after the verb, so they skip
/// the parser. They always use the real today: a pinned date could probe the
/// presence count (threat model T4). Nothing is logged (T3).
fn run_paperwork(front: &[String]) -> Option<i32> {
    let mut i = 0;
    let mut user_arg = None;
    while i < front.len() && (front[i] == "--user" || front[i] == "--today") {
        if front[i] == "--user" {
            user_arg = front.get(i + 1).cloned();
        }
        i += 2;
    }
    let word = front.get(i)?.as_str();
    if word != "paperwork" && word != "citizenship" {
        return None;
    }
    let user = match paths::resolve(user_arg.as_deref()) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("error: {}", e.message);
            return Some(2);
        }
    };
    let args = &front[i + 1..];
    let result = if word == "paperwork" {
        crate::paperwork::cli::run(args, &user.home, today())
    } else {
        crate::paperwork::citizenship::judge::run(args, &user.home, today())
    };
    Some(match result {
        Ok((out, code)) => {
            print!("{out}");
            if !out.is_empty() && !out.ends_with('\n') {
                println!();
            }
            code
        }
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    })
}

/// Runs `bin/cashcast ARGS…` as Python's `cli.main` did; returns the exit code.
pub fn run(argv: &[String]) -> i32 {
    // `--user` and `--today` are accepted before or after the command.
    let mut argv = argv.to_vec();
    let mut front = Vec::new();
    for flag in ["--user", "--today"] {
        if let Some(i) = argv.iter().position(|a| a == flag) {
            let end = (i + 2).min(argv.len());
            front.extend(argv.drain(i..end));
        }
    }
    front.extend(argv);
    if let Some(code) = run_paperwork(&front) {
        return code;
    }
    let (ns, rest) = match commands::parser().parse_known(&front) {
        Ok(parsed) => parsed,
        Err(exit) => {
            print!("{}", exit.stdout);
            eprint!("{}", exit.stderr);
            return exit.code;
        }
    };
    if !rest.is_empty() {
        println!("{HELP}");
        return 2;
    }
    let command = ns.str("command").unwrap_or_else(|| "start".into());
    let user_arg = ns.str("user");
    let today_arg = ns.str("today").filter(|d| !d.is_empty());
    // Engine subprocesses read only `%Y-%m-%d`; a bad value stays as given
    // so `reads_today` stops it below.
    let engine_arg = today_arg
        .as_deref()
        .map(|d| engine_today(d).unwrap_or_else(|| d.to_string()));
    let mut cmd = build(&command, &ns);
    if let Cmd::Start(who) = &mut cmd {
        who.clone_from(&user_arg);
    }
    if let Cmd::Moneywiz(mw) = &mut cmd {
        mw.today.clone_from(&engine_arg);
    }
    if let Cmd::Cards(_, given, _) | Cmd::Charges(_, given) = &mut cmd {
        given.clone_from(&engine_arg);
    }
    if let Cmd::Report(name, given) | Cmd::Forecast(name, given) = &mut cmd
        && matches!(*name, "import" | "status" | "forecast" | "backtest")
    {
        given.clone_from(&engine_arg);
    }
    let name = cmd.name();
    if let Cmd::UsersAdd { province, .. } | Cmd::UsersAdopt { province, .. } = &cmd
        && province.is_empty()
    {
        eprintln!("error: --province is required (AB BC MB NB NL NS NT NU ON PE QC SK YT)");
        return 2;
    }
    if let Cmd::UsersAdd { name: None, .. } = &cmd {
        eprintln!("error: users add needs a NAME");
        return 2;
    }
    let no_user = NO_USER.contains(&name);
    let user = match paths::resolve(user_arg.as_deref()) {
        Ok(u) => u,
        Err(e) if !no_user => {
            eprintln!("error: {}", e.message);
            return 2;
        }
        Err(_) => UserHome::new(paths::LEGACY, paths::private_root()), // these commands need no user
    };
    let day = match today_arg.as_deref().map(iso_date) {
        None => today(),
        Some(Ok(d)) => d,
        // Python's handler raised ValueError here (a traceback, exit 1).
        Some(Err(msg)) if reads_today(&cmd) => {
            eprintln!("error: {msg}");
            return 1;
        }
        Some(Err(_)) => today(),
    };
    let start = std::time::Instant::now();
    let code = match execute(&cmd, &user, day) {
        Ok(code) => code,
        // Where Python stopped with a traceback (exit 1), one line instead.
        Err(e)
            if matches!(
                e.kind,
                "crash" | "oserror" | "tomldecodeerror" | "valueerror" | "urlerror" | "httperror"
            ) =>
        {
            eprintln!("error: {}", e.message);
            return 1;
        }
        Err(e) if e.kind == "apierror" => {
            events::error(&user, name, e.kind);
            eprintln!("error: {}", e.message);
            return 4;
        }
        Err(e) if e.kind == "permissionerror" => {
            events::error(&user, name, e.kind);
            eprintln!(
                "error: no permission for {}. If an AI agent's sandbox blocked it, run the same command in Terminal.",
                e.message
            );
            return 5;
        }
        Err(e) => {
            if !no_user || user.home.exists() {
                events::error(&user, name, e.kind);
            }
            let msg = if e.kind == "keyerror" {
                format!("unknown service {}", e.message)
            } else {
                e.message
            };
            eprintln!("error: {msg}");
            return 3;
        }
    };
    if user.home.exists() && !no_user && name != "init" {
        let result = if code == 0 { "ok" } else { "failed" };
        events::log(
            &user,
            "command",
            &[
                ("command", Json::Str(name.into())),
                ("result", Json::Str(result.into())),
                ("ms", Json::Int(start.elapsed().as_millis() as i64)),
            ],
        );
    }
    code
}

fn execute(cmd: &Cmd, user: &UserHome, day: NaiveDate) -> Result<i32, AppError> {
    match cmd {
        Cmd::UsersList => {
            let names = paths::list_users();
            if names.is_empty() {
                println!("No users yet. Run: bin/cashcast users add NAME --province XX");
            } else {
                println!(
                    "{}",
                    names
                        .iter()
                        .map(|n| format!("- {n}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
        }
        Cmd::UsersAdd {
            province, display, ..
        }
        | Cmd::UsersAdopt { province, display } => {
            let adopted = matches!(cmd, Cmd::UsersAdopt { .. });
            let u = match cmd {
                Cmd::UsersAdd { name, .. } => users::add(
                    name.as_deref().unwrap_or_default(),
                    province,
                    display.as_deref(),
                )?,
                _ => users::adopt(province, display.as_deref())?,
            };
            events::log(
                &u,
                "user_created",
                &[(
                    "user_kind",
                    Json::Str(if adopted { "adopted" } else { "added" }.into()),
                )],
            );
            let rel = paths::rel(&u.home);
            println!(
                "User '{}' ready in {rel}. Edit {rel}/profile.toml (cards, bank accounts, income, room, goals).",
                u.name
            );
        }
        Cmd::ConfigShow => {
            for (key, value, source, about) in settings::show()? {
                println!("- {key} = {}  ({source}): {about}", value.py_str());
            }
        }
        Cmd::ConfigSet {
            key: Some(key),
            value: Some(value),
        } => {
            let saved = settings::set_value(key, value)?;
            let file = settings::path()
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            println!("{key} = {} (saved to {file})", saved.py_str());
        }
        Cmd::ConfigSet { .. } => {
            return Err(AppError::new(
                "runtimeerror",
                "usage: bin/cashcast config set KEY VALUE",
            ));
        }
        Cmd::ConsentList => {
            let act = consent::active(user, today())?;
            for s in &consent::SCOPES {
                let state = act
                    .get(s.name)
                    .map_or("not granted".to_string(), |d| format!("granted until {d}"));
                println!(
                    "- {}: {state}. Shares {} → {}.",
                    s.name, s.shares, s.recipient
                );
            }
        }
        Cmd::ConsentRevoke(scope) => {
            println!(
                "{}",
                if consent::revoke(user, scope.as_deref())? {
                    "Revoked."
                } else {
                    "It was not granted."
                }
            );
        }
        Cmd::ConsentGrant(scope) => {
            let (mut tty_in, mut tty_out) = consent::open_tty()?;
            let scope = scope.clone().unwrap_or_default();
            let expires = consent::grant(user, &scope, &mut tty_in, &mut tty_out, today())?;
            println!("Granted {scope} until {expires}.");
        }
        Cmd::Knowledge { action, file } => return knowledge(action, file.as_deref(), day),
        Cmd::TelemetryStatus => {
            let st = telemetry::status(user, today())?;
            println!(
                "telemetry: {}",
                if st.on {
                    "on".to_string()
                } else {
                    format!("off ({})", st.why)
                }
            );
            let last = st
                .last_send_day
                .filter(Json::truthy)
                .map_or("never".to_string(), |v| v.py_str());
            println!("pending: {} event(s)\nlast send: {last}", st.pending);
        }
        Cmd::TelemetrySwitch(on) => {
            settings::set_value("telemetry.enabled", if *on { "true" } else { "false" })?;
            println!("telemetry: turned {}", if *on { "on" } else { "off" });
        }
        Cmd::Strategy(_) | Cmd::Section(_) | Cmd::NetTotal | Cmd::InvestPlan | Cmd::Rrsp => {
            return strategy(cmd, user, day);
        }
        Cmd::Optimize(input) => return optimize(user, input.as_deref()),
        Cmd::Tasks { .. } | Cmd::Changes { .. } | Cmd::Preview(_) => return tasks(cmd, user, day),
        Cmd::Sync {
            action,
            dest,
            range,
            allow_removals,
        } => {
            return sync(
                user,
                action,
                dest.as_deref(),
                range.as_deref(),
                *allow_removals,
                day,
            );
        }
        Cmd::Start(who) => return crate::tasks::start::run(who.as_deref(), day),
        Cmd::Weekly(dry_run, balances, sync) => {
            return weekly(user, day, *dry_run, balances, *sync);
        }
        Cmd::Moneywiz(mw) => return moneywiz(user, mw),
        Cmd::Notion { source, delete } => return notion(user, source, *delete),
        Cmd::Web { port } => {
            // Python's socket bind raised OverflowError (a traceback, exit 1).
            let port = u16::try_from(*port)
                .map_err(|_| AppError::new("crash", "bind(): port must be 0-65535."))?;
            crate::web::serve(port)?;
            return Ok(0);
        }
        Cmd::Connectors(..) | Cmd::Action(..) | Cmd::FetchPlan(_) | Cmd::FetchCollect(_) => {
            return connectors(cmd, user, day);
        }
        Cmd::Help => println!("{HELP}"),
        Cmd::ApiRecord {
            service,
            set,
            currency,
        } => {
            let (Some(service), Some(set)) = (
                service.as_deref().filter(|s| !s.is_empty()),
                set.as_deref().filter(|s| !s.is_empty()),
            ) else {
                return Err(AppError::new(
                    "runtimeerror",
                    "usage: bin/cashcast api record SERVICE --set NAME=AMOUNT,NAME=AMOUNT [--currency CAD]",
                ));
            };
            let n = crate::tasks::balances::record(user, service, set, currency)?;
            println!(
                "Recorded {n} balance(s) for {service}, read in the browser. Next: bin/cashcast strategy"
            );
        }
        Cmd::Actions(true) => println!("{}", pyjson::dumps(&commands::catalog(), Some(1), false)),
        Cmd::Actions(false) => println!("{}", render_actions(&commands::catalog())),
        Cmd::Transfers(balances) => {
            let mut args = vec!["transfers"];
            for b in balances {
                args.extend(["--balance", b.as_str()]);
            }
            let (code, out) = engine(user, &args)?;
            // The balances came from the bank sites the agent read: strategy-level.
            super::gate::emit(user, &out, "agent:strategy", "transfers", day)?;
            return Ok(code);
        }
        Cmd::Cards(given, today_given, extra) => {
            let (mut args, _) = card_inputs(user, given)?;
            let catalog = paths::root()
                .join("knowledge")
                .join("data")
                .join("credit_cards.toml")
                .to_string_lossy()
                .to_string();
            let mut full: Vec<&str> = vec!["cards", "--catalog", catalog.as_str()];
            full.extend(extra.iter().map(String::as_str));
            full.extend(args.iter_mut().map(|a| a.as_str()));
            if let Some(d) = today_given {
                full.extend(["--today", d.as_str()]);
            }
            let (code, out) = engine(user, &full)?;
            super::gate::emit(user, &out, "agent:strategy", "cards", day)?;
            return Ok(code);
        }
        Cmd::Charges(find, today_given) => {
            if find.as_deref() == Some("") {
                println!(
                    "charges find needs a word, for example: bin/cashcast charges find anthropic"
                );
                return Ok(2);
            }
            let merchants = paths::root()
                .join("knowledge")
                .join("data")
                .join("merchants.toml")
                .to_string_lossy()
                .to_string();
            let mut full: Vec<&str> = vec!["charges", "--merchants", merchants.as_str()];
            if let Some(word) = find {
                full.extend(["find", word.as_str()]);
            }
            if let Some(d) = today_given {
                full.extend(["--today", d.as_str()]);
            }
            let (code, out) = engine(user, &full)?;
            // It names merchants, accounts and amounts: owner-only, like `cards`.
            super::gate::emit(user, &out, "agent:strategy", "charges", day)?;
            return Ok(code);
        }
        Cmd::Grocery(g) => return grocery(user, g, day),
        Cmd::FirstError => {
            if consent::agent_context() && !consent::allowed(user, "agent:raw-row", today())? {
                consent::require(user, "agent:raw-row", today())?;
            }
            let (code, out) = engine(user, &["first-error"])?;
            print!("{out}");
            return Ok(code);
        }
        Cmd::Numbers => {
            let o = std::process::Command::new("bash")
                .arg(paths::root().join("forecast").join("to_numbers.sh"))
                .arg(user.out())
                .output()
                .map_err(|e| AppError::new("oserror", e.to_string()))?;
            let mut out = String::from_utf8_lossy(&o.stdout).to_string();
            out.push_str(&String::from_utf8_lossy(&o.stderr));
            let mut code = o.status.code().unwrap_or(1);
            if code == 0 {
                let (c2, o2) = forecast_run(user, "verify-numbers", &[])?;
                code = c2;
                out.push_str(&o2);
            }
            super::gate::emit(user, &out, "agent:aggregates", "numbers", day)?;
            return Ok(code);
        }
        Cmd::Stop => println!(
            "{}",
            if crate::tasks::start::stop().map_err(|e| {
                let kind = if e.contains("ermission") {
                    "permissionerror"
                } else {
                    "oserror"
                };
                AppError::new(kind, e)
            })? {
                "Stopped the web app."
            } else {
                "The web app was not running."
            }
        ),
        Cmd::Init => {
            users::create_folders(user)?;
            let rel = paths::rel(&user.home);
            println!("Data folder ready: {rel} (only your user can read it; git never tracks it).");
            println!(
                "Put your exports (.csv, .ofx, .qfx; .xlsx for E*TRADE stock plan files) in: {rel}/inbox"
            );
        }
        Cmd::Report(name, given) | Cmd::Forecast(name, given) => {
            let forecast = matches!(cmd, Cmd::Forecast(..));
            let mut args = vec![if forecast { "import" } else { name }];
            if let Some(d) = given {
                args.extend(["--today", d.as_str()]);
            }
            let (code, out) = engine(user, &args)?;
            if !forecast || code != 0 {
                super::gate::emit(user, &out, "agent:aggregates", args[0], day)?;
                return Ok(code);
            }
            let (code, out) = forecast_run(user, name, &args[1..])?;
            super::gate::emit(user, &out, "agent:aggregates", name, day)?;
            return Ok(code);
        }
        Cmd::TelemetrySend => {
            let (sent, status, why) = telemetry::send(user, today());
            if status == "ok" {
                println!("telemetry: sent {sent} event(s)");
            } else {
                println!(
                    "telemetry: not sent ({})",
                    if why.is_empty() {
                        status.to_string()
                    } else {
                        why
                    }
                );
            }
        }
        Cmd::Api {
            action,
            service,
            target,
            gid,
        } => {
            return api(
                user,
                action,
                service,
                target.as_deref(),
                gid.as_deref(),
                day,
            );
        }
        Cmd::TelemetryDryRun => println!(
            "{}",
            pyjson::dumps(&telemetry::dry_run(user)?, Some(1), false)
        ),
    }
    Ok(0)
}

/// The owner-run token of one `weekly` run; its file goes when the run ends.
struct OwnerRun(String);

impl Drop for OwnerRun {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(consent::owner_run_file());
    }
}

/// `weekly [--dry-run]`: the Sunday session in one run. Each step runs as
/// its own `cli` child (so each keeps its consent gate and a failed step
/// does not stop the rest); then the overlaps and this week's moves. The
/// whole page is saved to `out/weekly.md` and `out/weekly.html`.
/// The weekly's task-list step: `sync all` only with `--sync` on a real run.
fn weekly_sync_step(dry_run: bool, sync: bool) -> &'static [&'static str] {
    if sync && !dry_run {
        &["sync", "all"]
    } else {
        &["sync", "status"]
    }
}

fn weekly(
    user: &UserHome,
    day: NaiveDate,
    dry_run: bool,
    balances: &[String],
    sync_tasks: bool,
) -> Result<i32, AppError> {
    use crate::tasks::{changes, weekly as wk};
    let back = |e: crate::strategy::situation::Unported| AppError::new("crash", e.0);
    let exe = std::env::current_exe().map_err(|e| AppError::new("oserror", e.to_string()))?;
    let mut base: Vec<String> = vec!["cli".into()];
    if user.name != paths::LEGACY {
        base.extend(["--user".into(), user.name.clone()]);
    }
    if day != today() {
        base.extend(["--today".into(), day.format("%Y-%m-%d").to_string()]);
    }
    // Run by the owner in a terminal: the steps' output is captured, so pass
    // them a one-run owner token (a run an agent starts gets none).
    let owner_run = if consent::agent_context() {
        None
    } else {
        let token = crate::web::new_token()?;
        let file = consent::owner_run_file();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(AppError::io)?;
        }
        crate::write_private(&file, &token).map_err(|e| AppError::new("oserror", e))?;
        Some(OwnerRun(token))
    };
    let run = |args: &[&str]| -> (i32, String) {
        let mut cmd = std::process::Command::new(&exe);
        if let Some(OwnerRun(t)) = &owner_run {
            cmd.env("CASHCAST_OWNER_RUN", t);
        }
        match cmd
            .args(&base)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(o) => {
                let mut out = String::from_utf8_lossy(&o.stdout).to_string();
                out.push_str(&String::from_utf8_lossy(&o.stderr));
                (o.status.code().unwrap_or(1), out)
            }
            Err(e) => (1, format!("error: {e}\n")),
        }
    };
    let mw: &[&str] = if dry_run {
        &["moneywiz", "sync", "--dry-run"]
    } else {
        &["moneywiz", "weekly"]
    };
    let resolve: &[&str] = if dry_run {
        &["tasks"]
    } else {
        &["tasks", "resolve"]
    };
    // `forecast` imports the inbox first, so there is no separate import step.
    // Transfers and cards use the balances read today (`--balance`).
    // `cards now` takes only card ids; bank balances go to transfers alone.
    let card_ids: Vec<String> = users::load_profile(user)
        .ok()
        .and_then(|p| p.get("wallet")?.get("cards")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| Some(c.get("id")?.as_str()?.to_string()))
        .collect();
    let config_table = std::fs::read_to_string(user.config())
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok());
    let uses_moneywiz = config_table
        .as_ref()
        .is_some_and(|t| t.contains_key("moneywiz"))
        || crate::moneywiz::find_db(None).is_ok();
    let mut transfers: Vec<&str> = vec!["transfers"];
    let mut cards: Vec<&str> = vec!["cards", "now"];
    for b in balances {
        transfers.extend(["--balance", b.as_str()]);
        let id = b.split('=').next().unwrap_or_default();
        if card_ids.iter().any(|c| c == id) {
            cards.extend(["--balance", b.as_str()]);
        }
    }
    let mut steps: Vec<(&str, &[&str])> = vec![
        (
            "Fresh data: what is stale and how to get it",
            &["fetch", "plan"],
        ),
        ("Forecast", &["forecast"]),
        ("MoneyWiz", mw),
        ("Transfers between your banks", &transfers),
        ("Which card to use now", &cards),
        (
            "Charges on a card that earns less (last 7 days)",
            &["cards", "missed"],
        ),
        ("Charges you did not expect", &["charges"]),
        ("Investments against your targets", &["holdings"]),
    ];
    // With `[moneywiz] since`, every row from that day on must match its bank.
    let since_set = std::fs::read_to_string(user.config())
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .is_some_and(|t| t.get("moneywiz").and_then(|m| m.get("since")).is_some());
    if since_set {
        steps.insert(
            3,
            (
                "MoneyWiz history since your start day",
                &["moneywiz", "coverage"][..],
            ),
        );
    }
    let mut page = vec![format!(
        "# Weekly cashcast session, {}",
        day.format("%a %b %-d, %Y")
    )];
    let mut failed = Vec::new();
    let mut step = |page: &mut Vec<String>, title: &str, args: &[&str]| -> String {
        let (code, out) = run(args);
        page.push(format!(
            "\n## {} · `bin/cashcast {}`\n",
            title,
            args.join(" ")
        ));
        page.push(out.trim_end().to_string());
        if code != 0 {
            page.push(format!(
                "\n(This step stopped with exit {code}; the rest still ran.)"
            ));
            failed.push(args.join(" "));
        }
        out
    };
    let mut holdings_failed = false;
    let mut status = String::new();
    let (mut fetch_out, mut mw_out) = (String::new(), String::new());
    for (title, args) in steps {
        if args == ["transfers"] {
            page.push(format!(
                "\n## {title}\n\nSkipped: transfers need today's balances. Read them on the bank \
                 and card sites, then run `bin/cashcast weekly --balance ID=AMOUNT ...` \
                 (ids: your profile's [[banking.accounts]] and [[wallet.cards]])."
            ));
            continue;
        }
        // A person without MoneyWiz (no database, no [moneywiz] in cashcast.toml)
        // or without saved cards gets a plain skip line, not a failed step.
        if title == "MoneyWiz" && !uses_moneywiz {
            page.push(
                "\n## MoneyWiz\n\nSkipped: MoneyWiz is not on this computer, and cashcast works without it. \
                 Your bank and card downloads in the inbox are the record."
                    .into(),
            );
            continue;
        }
        if args.starts_with(&["cards", "now"]) && card_ids.is_empty() {
            page.push(format!(
                "\n## {title} · `bin/cashcast cards now`\n\nSkipped: no cards in profile.toml yet. Add each card as a [[wallet.cards]] \
                 block (see profile/profile.example.toml) to see which card to use for what."
            ));
            continue;
        }
        // No investment downloads yet: a plain skip line, not a failed step.
        if args == ["holdings"] {
            let (code, out) = run(args);
            if code != 0 && out.contains("no ledger files") {
                page.push(format!(
                    "\n## {title}\n\nSkipped: no investment downloads in the inbox yet. Save a brokerage \
                     activity export there (see `connectors/README.md`) to compare holdings with your targets."
                ));
                continue;
            }
            page.push(format!(
                "\n## {} · `bin/cashcast {}`\n",
                title,
                args.join(" ")
            ));
            page.push(out.trim_end().to_string());
            if code != 0 {
                page.push(format!(
                    "\n(This step stopped with exit {code}; the rest still ran.)"
                ));
                holdings_failed = true;
            }
            continue;
        }
        let out = step(&mut page, title, args);
        if args == ["forecast"] {
            status = out;
        } else if args == ["fetch", "plan"] {
            fetch_out = out;
        } else if title == "MoneyWiz" {
            mw_out = out;
        } else if args == ["moneywiz", "coverage"] {
            mw_out.push('\n');
            mw_out.push_str(&out);
        }
    }
    match wk::floor_clear(&status) {
        Some(true) => {
            step(&mut page, "Where new money goes", &["optimize"]);
            step(&mut page, "What to invest where", &["invest-plan"]);
        }
        Some(false) => page.push(
            "\n## What to invest where\n\nNo new money this week: a month in the forecast dips below the cash floor, \
             so `optimize` and `invest-plan` did not run."
                .into(),
        ),
        None if status.contains("Report saved for the owner only") => page.push(
            "\n## What to invest where\n\nSkipped: the forecast was withheld, so the safe amount is unknown here and \
             `optimize` and `invest-plan` did not run. Run `bin/cashcast weekly` in your own terminal, or grant `agent:aggregates`."
                .into(),
        ),
        None => page.push(
            "\n## What to invest where\n\nSkipped: the forecast did not run, so the safe amount is unknown and \
             `optimize` and `invest-plan` did not run."
                .into(),
        ),

    }
    step(&mut page, "Tasks cashcast closes itself", resolve);
    // Reminders and Calendar change only after the owner confirms the week's
    // direction (`weekly --sync`, or `sync all` later); owner rule, 2026-11-22.
    let sync = weekly_sync_step(dry_run, sync_tasks);
    // Nothing turned on yet is a setup choice, not a failed step.
    let (sync_code, sync_out) = run(sync);
    page.push(format!(
        "\n## Reminders, Calendar and Notion · `bin/cashcast {}`\n",
        sync.join(" ")
    ));
    page.push(sync_out.trim_end().to_string());
    let reads_only = sync == ["sync", "status"];
    if reads_only && !dry_run {
        page.push(
            "\nNot written to Reminders or Calendar yet: confirm this week's direction, then run \
             `bin/cashcast sync all` (or next time `bin/cashcast weekly --sync`)."
                .into(),
        );
    }
    if holdings_failed {
        failed.push("holdings".into());
    }
    if sync_code != 0 && !(reads_only && sync_out.contains("Nothing is on yet")) {
        page.push(format!(
            "\n(This step stopped with exit {sync_code}; the rest still ran.)"
        ));
        failed.push(sync.join(" "));
    }
    // A run is done only when the data is current: name what is not.
    let cfg = std::fs::read_to_string(user.config())
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok());
    let pin = |table: &str, key: &str| {
        cfg.as_ref()
            .and_then(|t| Some(t.get(table)?.get(key)?.as_str()?.to_string()))
    };
    let (start_month, last_month) = (pin("forecast", "start_month"), pin("window", "last_month"));
    let stale = wk::not_current(
        &fetch_out,
        &mw_out,
        start_month.as_deref(),
        last_month.as_deref(),
        day,
    );
    page.push(format!("\n## Not up to date ({})\n", stale.len()));
    if stale.is_empty() {
        page.push(
            "- nothing: every export is fresh, MoneyWiz matches each bank, and the forecast starts this month"
                .into(),
        );
    }
    for s in &stale {
        page.push(format!("- {s}"));
    }
    // The Sunday downloads need bank site access: say so before it lapses.
    let browser = consent::active(user, day)?
        .get("agent:browser-read")
        .copied();
    if let Some(note) = wk::browser_read_note(browser, day) {
        page.push(format!("\n## Bank site access\n\n- {note}"));
    }

    // Overlaps and this week's moves, from the open task list.
    users::load_profile(user)?;
    let (ctx, log) = crate::strategy::plan::run(user, day).map_err(back)?;
    let on = ctx.sit.today;
    let finished = changes::done(user).map_err(back)?;
    let mut open = Vec::new();
    for t in crate::strategy::plan::tasks(&ctx).map_err(back)? {
        if changes::is_open(&finished, &t, on).map_err(back)? {
            open.push(t);
        }
    }
    crate::strategy::plan::finish(user, &ctx, &log).map_err(back)?;
    let found = wk::overlaps(&open);
    let mut tail = vec![format!("\n## Overlaps ({})\n", found.len())];
    if found.is_empty() {
        tail.push("- none: no two tasks ask for the same work".into());
    }
    for o in &found {
        let fix = if wk::dated(&o.drop) {
            "Tick it done when you do the other."
        } else {
            "Remove it from your profile."
        };
        tail.push(format!(
            "- `{}` repeats `{}`: {}. {fix}",
            o.drop, o.keep, o.why
        ));
    }
    let dropped: Vec<String> = found.iter().map(|o| o.drop.clone()).collect();
    tail.push(String::new());
    tail.push(wk::moves(&open, on, &dropped));
    if let Some(notes) = ctx
        .sit
        .profile
        .get("weekly")
        .and_then(|w| w.get("notes"))
        .map(Json::py_str)
    {
        tail.push(format!("\n## Your weekly notes\n\n{notes}"));
    }
    if !failed.is_empty() {
        tail.push(format!("\nSteps that stopped: {}.", failed.join("; ")));
    }
    if !stale.is_empty() {
        tail.push(format!(
            "\nNot up to date: {} item(s), listed above. Refresh them before you trust this week's numbers.",
            stale.len()
        ));
    }
    let tail = tail.join("\n");
    // The steps passed their own gates; the overlaps and moves name amounts.
    println!("{}", page.join("\n"));
    let shown = super::gate::emit(user, &tail, "agent:strategy", "weekly", today())?;
    if shown {
        page.push(tail);
    }
    let text = page.join("\n") + "\n";
    let io = |e: std::io::Error| AppError::new("oserror", e.to_string());
    std::fs::create_dir_all(user.out()).map_err(io)?;
    let fail = |e: String| AppError::new("oserror", e);
    crate::write_private(&user.out().join("weekly.md"), &text).map_err(fail)?;
    let esc = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let html = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>Weekly cashcast session</title><style>body{{font:16px/1.5 -apple-system,system-ui,sans-serif;\
         max-width:60rem;margin:2rem auto;padding:0 1rem;color-scheme:light dark}}pre{{white-space:pre-wrap}}</style>\
         <pre>{esc}</pre>"
    );
    crate::write_private(&user.out().join("weekly.html"), &html).map_err(fail)?;
    events::log(
        user,
        "weekly",
        &[
            ("overlaps", Json::Int(found.len() as i64)),
            ("failed_steps", Json::Int(failed.len() as i64)),
        ],
    );
    println!(
        "\nSaved: {}/weekly.html ({} overlap(s), {} step(s) stopped).",
        paths::rel(&user.out()),
        found.len(),
        failed.len()
    );
    Ok(0)
}

/// `api balances`, `api download` and `api etrade-auth` (port of `cmd_api`).
fn api(
    user: &UserHome,
    action: &str,
    service: &str,
    target: Option<&str>,
    gid: Option<&str>,
    day: NaiveDate,
) -> Result<i32, AppError> {
    use crate::api as services;
    consent::require(user, "actions:api", day)?;
    let real = today();
    match action {
        "balances" => {
            let bal = services::balances(user, service, real)?;
            let text = bal
                .iter()
                .map(|(k, v)| format!("- {k}: {}", crate::strategy::py::commas(*v, 2)))
                .collect::<Vec<_>>()
                .join("\n");
            let text = if text.is_empty() {
                "No balances.".to_string()
            } else {
                text
            };
            super::gate::emit(user, &text, "agent:aggregates", "api", day)?;
        }
        "download" => {
            let saved = if service == "google-sheets" {
                let Some(sheet) = target else {
                    return Err(AppError::new(
                        "runtimeerror",
                        "usage: bin/cashcast api download google-sheets SHEET_ID [GID]",
                    ));
                };
                vec![services::sheets_download(user, sheet, gid.unwrap_or("0"))?]
            } else {
                services::download(user, service, real)?
            };
            println!(
                "Saved {} file(s) to your inbox. Next: bin/cashcast import",
                saved.len()
            );
        }
        _ => {
            use std::io::{BufRead, Write};
            let (url, req) = services::etrade_authorize_url(user)?;
            let (mut tty_in, mut tty_out) = consent::open_tty()?;
            let _ = write!(
                tty_out,
                "Open this link, sign in to E*TRADE, accept, and paste the code here:\n{url}\nCode: "
            );
            let _ = tty_out.flush();
            let mut code = String::new();
            let _ = tty_in.read_line(&mut code);
            let token = services::etrade_access_token(user, &req, code.trim())?;
            let env = user.home.join(".env");
            // A .env that exists but cannot be read is an error, never "empty":
            // rewriting it would drop the other tokens.
            let old = match std::fs::read_to_string(&env) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => {
                    return Err(AppError::new(
                        "oserror",
                        format!("cannot read {}: {e}", env.display()),
                    ));
                }
            };
            let mut lines: Vec<String> = old
                .lines()
                .filter(|l| {
                    !l.starts_with("CASHCAST_ETRADE_TOKEN=")
                        && !l.starts_with("CASHCAST_ETRADE_TOKEN_SECRET=")
                })
                .map(str::to_string)
                .collect();
            lines.push(format!("CASHCAST_ETRADE_TOKEN={}", token.0));
            lines.push(format!("CASHCAST_ETRADE_TOKEN_SECRET={}", token.1));
            crate::write_private(&env, &(lines.join("\n") + "\n"))
                .map_err(|e| AppError::new("oserror", e))?;
            println!(
                "E*TRADE access saved to your private .env (valid until midnight US Eastern)."
            );
        }
    }
    Ok(0)
}

/// `strategy`, the section commands, `net-total`, `invest-plan` and `rrsp`:
/// every section runs, because later ones read earlier results.
fn strategy(cmd: &Cmd, user: &UserHome, day: NaiveDate) -> Result<i32, AppError> {
    use crate::strategy::{plan, rrsp};
    let back = |e: crate::strategy::situation::Unported| AppError::new("crash", e.0);
    if matches!(cmd, Cmd::InvestPlan | Cmd::Rrsp) {
        plan::refresh_holdings(&user.home);
    }
    users::load_profile(user)?; // Python's situation reads it first: ProfileError, exit 3
    let (ctx, log) = plan::run(user, day).map_err(back)?;
    let markdown = |id: &str| ctx.result(id).map_or(String::new(), |s| s.markdown());
    let (text, name) = match cmd {
        Cmd::Strategy(only) => (plan::document(&ctx, only).map_err(back)?, "strategy"),
        Cmd::Section(id) => {
            let mut text = markdown(id);
            if *id == "review"
                && let Some(sec) = ctx.result("review")
            {
                let mut tasks = sec.tasks.clone();
                tasks.sort_by(|a, b| {
                    (a.due.unwrap_or(NaiveDate::MAX), &a.id)
                        .cmp(&(b.due.unwrap_or(NaiveDate::MAX), &b.id))
                });
                let lines: Vec<String> = tasks
                    .iter()
                    .map(|t| {
                        format!(
                            "- [ ] {}: {}",
                            t.due.map_or("when known".to_string(), |d| d.to_string()),
                            t.title
                        )
                    })
                    .collect();
                text += &format!("\n{}", lines.join("\n"));
            }
            (text, *id)
        }
        Cmd::NetTotal => (markdown("net_total"), "net-total"),
        Cmd::InvestPlan => (markdown("invest_plan"), "invest-plan"),
        _ => {
            let tax = ctx.sit.knowledge("tax_ca_2026").map_err(back)?;
            let check = rrsp::check(
                &ctx.sit.profile,
                ctx.sit.holdings.as_ref(),
                ctx.sit.today,
                crate::strategy::py::obj(&tax, "limits"),
            );
            (rrsp::report(&check), "rrsp")
        }
    };
    plan::finish(user, &ctx, &log).map_err(back)?;
    if let (Cmd::InvestPlan, Some(sec)) = (cmd, ctx.result("invest_plan")) {
        let (moves, wins) = match &sec.data {
            crate::strategy::Data::InvestPlan(p) => (p.moves.len() as i64, p.do_nothing_wins),
            _ => (0, false),
        };
        events::log(
            user,
            "invest_plan",
            &[
                ("moves", Json::Int(moves)),
                ("do_nothing_wins", Json::Bool(wins)),
            ],
        );
    }
    let saved = if let Cmd::Strategy(_) = cmd {
        let out = user.out();
        let io = |e: std::io::Error| AppError::new("oserror", e.to_string());
        std::fs::create_dir_all(&out).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
        }
        let path = out.join("strategy.md");
        crate::write_private(&path, &text).map_err(|e| AppError::new("oserror", e))?;
        Some(path)
    } else {
        None
    };
    super::gate::emit(user, &text, "agent:strategy", name, today())?;
    if let Some(path) = saved {
        println!("\nSaved: {}", paths::rel(&path));
    }
    Ok(0)
}

/// `sync all|install|uninstall|sheets`, as `cmd_sync` in `cli.py`.
fn sync(
    user: &UserHome,
    action: &str,
    dest: Option<&str>,
    range: Option<&str>,
    allow_removals: bool,
    day: NaiveDate,
) -> Result<i32, AppError> {
    use crate::strategy::{Data, plan};
    use crate::tasks::{changes, sync};
    let back = |e: crate::strategy::situation::Unported| AppError::new("crash", e.0);
    match action {
        "install" => {
            let pause = || std::thread::sleep(std::time::Duration::from_secs(1));
            println!("{}", sync::install(user, None, &sync::launchctl, &pause)?);
            return Ok(0);
        }
        "uninstall" => {
            println!("{}", sync::uninstall(user, None, &sync::launchctl));
            return Ok(0);
        }
        "sheets" if dest.is_none_or(str::is_empty) => {
            return Err(AppError::new(
                "runtimeerror",
                "usage: bin/cashcast sync sheets SHEET_ID [RANGE]",
            ));
        }
        "all" => {
            // Parse new exports first, so the RRSP check needs no manual step;
            // the plan still runs on the last holdings.json when this fails.
            let _ = engine(user, &["holdings"]);
        }
        _ => {}
    }
    users::load_profile(user)?; // Python's situation reads it first: ProfileError, exit 3
    let (ctx, log) = plan::run(user, day).map_err(back)?;
    plan::finish(user, &ctx, &log).map_err(back)?;
    if action == "sheets" {
        let mut rows = vec![Json::List(
            ["Month", "Income", "Expenses", "To invest", "Month end"]
                .iter()
                .map(|h| Json::Str(h.to_string()))
                .collect(),
        )];
        if let Some(sec) = ctx.result("forecast")
            && let Data::Forecast(fc) = &sec.data
        {
            use crate::strategy::forecast::f;
            for r in &fc.rows {
                rows.push(Json::List(vec![
                    Json::Str(r.month.format("%Y-%m").to_string()),
                    Json::Float(f(r.income)),
                    Json::Float(f(r.expenses)),
                    Json::Float(f(r.to_invest)),
                    Json::Float(f(r.month_end)),
                ]));
            }
        }
        let range = range.filter(|r| !r.is_empty()).unwrap_or("Cashcast!A1");
        let cells = crate::api::sheets_write(user, dest.unwrap_or_default(), range, rows, today())?;
        println!("Updated {cells} cells.");
        return Ok(0);
    }
    let on = ctx.sit.today;
    let mut open_tasks = || -> Result<Vec<crate::strategy::Task>, AppError> {
        let finished = changes::done(user).map_err(back)?;
        let mut open = Vec::new();
        for t in plan::tasks(&ctx).map_err(back)? {
            if changes::is_open(&finished, &t, on).map_err(back)? {
                open.push(t);
            }
        }
        weekly_fold(open, on, &finished)
    };
    let now = || chrono::Local::now().naive_local();
    let rep = sync::run(
        user,
        &ctx.sit.profile,
        &mut open_tasks,
        on,
        &now,
        allow_removals,
    )?;
    let live = crate::live_facts::auto(on);
    println!(
        "{}",
        [
            "# Sync: Reminders, Calendar and Notion".to_string(),
            String::new()
        ]
        .into_iter()
        .chain(rep.lines)
        .chain(live)
        .collect::<Vec<_>>()
        .join("\n")
    );
    Ok(if rep.failed.is_empty() { 0 } else { 1 })
}

/// `tasks [list|resolve|verdict]` and `changes [list|show|done|undo]`.
fn tasks(cmd: &Cmd, user: &UserHome, day: NaiveDate) -> Result<i32, AppError> {
    use crate::strategy::{Data, plan};
    use crate::tasks::{changes, triage};
    let back = |e: crate::strategy::situation::Unported| AppError::new("crash", e.0);
    if let Cmd::Changes { action, id } = cmd
        && (action == "done" || action == "undo")
    {
        let Some(id) = id else {
            return Err(AppError::new(
                "runtimeerror",
                format!("usage: bin/cashcast changes {action} ID"),
            ));
        };
        println!("{}", changes::mark(user, id, day, action == "undo")?);
        return Ok(0);
    }
    users::load_profile(user)?; // Python's situation reads it first: ProfileError, exit 3
    let (ctx, log) = plan::run(user, day).map_err(back)?;
    let on = ctx.sit.today;
    let finished = changes::done(user).map_err(back)?;
    let mut open = Vec::new();
    for t in plan::tasks(&ctx).map_err(back)? {
        if changes::is_open(&finished, &t, on).map_err(back)? {
            open.push(t);
        }
    }
    let pending = changes::pending(user, &open, on).map_err(back)?;
    let synced = weekly_fold(open.clone(), on, &finished)?;
    plan::finish(user, &ctx, &log).map_err(back)?;
    let text = match cmd {
        Cmd::Preview(which @ ("reminders" | "reminders-sync")) => {
            let recent: Vec<_> = synced
                .iter()
                .filter(|t| t.due.is_none_or(|d| d >= on - chrono::Duration::days(7)))
                .cloned()
                .collect();
            let data = crate::tasks::apple::reminders_payload(
                user,
                &ctx.sit.profile,
                &recent,
                &[],
                &[],
                None,
            )
            .map_err(back)?;
            if *which == "reminders-sync" {
                use crate::tasks::apple::{count, reminders_sync};
                let out = reminders_sync(user, &data)?;
                println!(
                    "Reminders list '{}': {} created, {} updated, {} already completed.",
                    out.get("list")
                        .map(Json::py_str)
                        .unwrap_or_else(|| "None".into()),
                    count(&out, "created"),
                    count(&out, "updated"),
                    count(&out, "completed_kept")
                );
                return Ok(0);
            }
            let text = crate::tasks::apple::reminders_preview(&data);
            super::gate::emit(user, &text, "agent:strategy", "reminders", today())?;
            return Ok(0);
        }
        Cmd::Preview("sync") => {
            let (ok, lines) =
                crate::tasks::sync::status(user, &ctx.sit.profile, &synced, on, today())?;
            println!(
                "{}",
                [
                    format!(
                        "# Sync: {}",
                        if ok {
                            "everything matches"
                        } else {
                            "out of step"
                        }
                    ),
                    String::new(),
                ]
                .into_iter()
                .chain(lines)
                .collect::<Vec<_>>()
                .join("\n")
            );
            println!(
                "{}",
                if ok {
                    ""
                } else {
                    "\nTo catch up run in Terminal: bin/cashcast sync all"
                }
            );
            return Ok(if ok { 0 } else { 1 });
        }
        Cmd::Preview(which) => {
            let data = crate::tasks::apple::calendar_payload(user, &ctx.sit.profile, &synced, on);
            if *which == "calendar-sync" {
                use crate::tasks::apple::{calendar_sync, count};
                let out = calendar_sync(user, &data)?;
                println!(
                    "Calendar '{}': {} created, {} updated, {} removed (no longer due).",
                    out.get("calendar")
                        .map(Json::py_str)
                        .unwrap_or_else(|| "None".into()),
                    count(&out, "created"),
                    count(&out, "updated"),
                    count(&out, "removed")
                );
                return Ok(0);
            }
            let text = crate::tasks::apple::calendar_preview(&data);
            super::gate::emit(user, &text, "agent:strategy", "calendar", today())?;
            return Ok(0);
        }
        Cmd::Changes { action, id } => {
            if action == "show" && id.is_none() {
                return Err(AppError::new(
                    "runtimeerror",
                    "usage: bin/cashcast changes show ID",
                ));
            }
            let detail = if action == "show" {
                id.as_deref()
            } else {
                None
            };
            super::gate::emit(
                user,
                &changes::render(&pending, detail)?,
                "agent:strategy",
                "changes",
                today(),
            )?;
            return Ok(0);
        }
        Cmd::Tasks {
            action,
            id,
            result,
            reason,
            source,
            json,
        } => {
            let ids: Vec<String> = pending
                .iter()
                .filter(|c| c.allowed)
                .map(|c| c.task.id.clone())
                .collect();
            let browser = super::consent::allowed(user, "actions:browser", today())?;
            let rows = triage::classify(&open, browser, &ids, on).map_err(back)?;
            match action.as_str() {
                "verdict" => {
                    let (Some(id), Some(result), Some(reason)) =
                        (id, result, reason.as_ref().filter(|r| !r.is_empty()))
                    else {
                        return Err(AppError::new(
                            "runtimeerror",
                            "usage: bin/cashcast tasks verdict ID pass|fail --reason \"what it showed, no amounts\" [--source browser]",
                        ));
                    };
                    let read_ok = super::consent::allowed(user, "agent:browser-read", today())?;
                    return match triage::verdict(
                        user, &rows, id, result, reason, source, on, read_ok,
                    )? {
                        Ok(msg) => {
                            println!("{msg}");
                            Ok(0)
                        }
                        Err(msg) => {
                            eprintln!("error: {msg}");
                            Ok(1)
                        }
                    };
                }
                "resolve" => {
                    let run = |section: &str| -> Result<bool, AppError> {
                        let Some(sec) = ctx
                            .result(section)
                            .filter(|s| !matches!(s.data, Data::None))
                        else {
                            return Ok(false);
                        };
                        let io = |e: std::io::Error| AppError::new("oserror", e.to_string());
                        std::fs::create_dir_all(user.out()).map_err(io)?;
                        crate::write_private(
                            &user.out().join(format!("{section}.md")),
                            &sec.markdown(),
                        )
                        .map_err(|e| AppError::new("oserror", e))?;
                        Ok(true)
                    };
                    let (resolved, blocked) = triage::resolve(user, &rows, on, run)?;
                    events::log(
                        user,
                        "tasks_resolved",
                        &[
                            ("resolved", Json::Int(resolved.len() as i64)),
                            ("blocked", Json::Int(blocked.len() as i64)),
                        ],
                    );
                    triage::render(&rows, Some((&resolved, &blocked)))
                }
                _ if *json => pyjson::dumps(&triage::as_json(&rows), Some(1), false),
                _ => triage::render(&rows, None),
            }
        }
        _ => unreachable!("tasks() runs only tasks and changes"),
    };
    super::gate::emit(user, &text, "agent:strategy", "tasks", today())?;
    Ok(0)
}

/// `optimize [--input FILE]`. An input file that is missing or does not
/// parse stops with one error line (Python: a traceback).
fn optimize(user: &UserHome, input: Option<&str>) -> Result<i32, AppError> {
    use crate::strategy::{optimize as opt, py};
    let profile = py::from_toml(&toml::Value::Table(super::users::load_profile(user)?));
    let file = match input {
        Some(f) => {
            let path = match f.strip_prefix("~/") {
                Some(rest) => {
                    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest)
                }
                None => std::path::PathBuf::from(f),
            };
            let text = std::fs::read_to_string(&path).map_err(|e| {
                AppError::new("crash", paths::py_os_error(&e, &path.to_string_lossy()))
            })?;
            let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
                AppError::new("crash", format!("{}: {}", path.display(), e.message()))
            })?;
            Some(py::from_toml(&toml::Value::Table(table)))
        }
        None => None,
    };
    let fail = |e: opt::OptimizeError| AppError::new("optimizeerror", e.0);
    let spec = opt::load(&profile, file.as_ref()).map_err(fail)?;
    let holdings_path = user.home.join("data").join("holdings.json");
    let holdings = match std::fs::read_to_string(&holdings_path) {
        Ok(text) => Some(
            pyjson::loads(&text)
                .ok_or_else(|| AppError::new("crash", "data/holdings.json is not JSON"))?,
        ),
        Err(_) => None,
    };
    let plan = opt::optimize(&spec, holdings.as_ref()).map_err(fail)?;
    let sells = plan.trades.iter().filter(|t| t.amount < 0.0).count() as i64;
    events::log(
        user,
        "optimize",
        &[
            ("trades", Json::Int(plan.trades.len() as i64)),
            ("sells", Json::Int(sells)),
        ],
    );
    super::gate::emit(
        user,
        &opt::render(&plan),
        "agent:strategy",
        "optimize",
        today(),
    )?;
    Ok(0)
}

/// `knowledge list|check|propose`. A file that does not parse stops with one
/// error line (Python: a traceback); the command has no side effects.
fn knowledge(action: &str, file: Option<&str>, day: NaiveDate) -> Result<i32, AppError> {
    use crate::knowledge as kb;
    let back = |e: kb::Unparsed| AppError::new("crash", format!("cannot read {}", e.0.display()));
    match action {
        "list" => {
            print!("{}", kb::render_list().map_err(back)?);
            Ok(0)
        }
        "refresh" => {
            let facts = crate::live_facts::refresh(day)?;
            let evidence = kb::load("invest_evidence").map_err(back)?;
            let (text, _) = crate::live_facts::render(&facts, &evidence);
            let saved = crate::live_facts::save(&facts)?;
            print!("{text}");
            println!("\n## Expert sources (new editions)\n");
            for line in crate::live_facts::editions(&evidence, day) {
                println!("{line}");
            }
            println!("- Saved: {}", paths::rel(&saved));
            Ok(0)
        }
        "propose" => {
            let Some(file) = file else {
                return Err(AppError::new(
                    "runtimeerror",
                    "usage: bin/cashcast knowledge propose PATH/TO/<topic>.toml",
                ));
            };
            let path = std::path::Path::new(file);
            if let Err(e) = std::fs::read(path) {
                return Err(AppError::new("crash", paths::py_os_error(&e, file)));
            }
            let p = kb::propose(path, day, &kb::data_dir()).map_err(back)?;
            println!("{}", kb::render_proposal(&p));
            Ok(if p.ok() { 0 } else { 1 })
        }
        _ => {
            let r = kb::check(day, &kb::data_dir()).map_err(back)?;
            println!("{}", kb::render_check(&r));
            Ok(if r.ok() { 0 } else { 1 })
        }
    }
}

/// `connectors list|show`, `action SERVICE ID` and `fetch plan|collect`.
fn connectors(cmd: &Cmd, user: &UserHome, day: NaiveDate) -> Result<i32, AppError> {
    use crate::tasks::{connectors as c, fetch};
    let back = |e: crate::strategy::situation::Unported| AppError::new("crash", e.0);
    let specs = c::load_all().map_err(back)?;
    match cmd {
        Cmd::Connectors(action, service) => {
            if action == "list" {
                println!("{}", c::render_list(&specs));
            } else {
                let spec = c::get(&specs, service.as_deref())
                    .map_err(|m| AppError::new("connectorerror", m))?;
                println!("{}", c::render_show(spec));
            }
            Ok(0)
        }
        Cmd::Action(service, id) => {
            let err = |m: String| AppError::new("connectorerror", m);
            let spec = c::get(&specs, Some(service)).map_err(err)?;
            let acts = crate::strategy::py::list(spec, "actions");
            let text = |a: &Json, k: &str| a.get(k).map_or(String::new(), Json::py_str);
            let Some(action) = acts.iter().find(|a| text(a, "id") == *id) else {
                let ids: Vec<String> = acts.iter().map(|a| text(a, "id")).collect();
                return Err(err(format!(
                    "{service} has no action {}; actions: {}",
                    paths::py_repr(id),
                    ids.join(", ")
                )));
            };
            let steps = c::playbook_section(spec, id).map_err(back)?;
            let risk = text(action, "risk");
            let via: Vec<String> = crate::strategy::py::list(action, "via")
                .iter()
                .map(Json::py_str)
                .collect();
            // Check consent before any side effect, as Python returns early only after logging.
            let browser_ok = risk == "money"
                || !via.iter().any(|v| v == "browser")
                || super::consent::allowed(user, "actions:browser", today())?;
            events::log(
                user,
                "action_planned",
                &[
                    ("service", Json::Str(service.clone())),
                    ("action", Json::Str(id.clone())),
                    ("risk", Json::Str(risk.clone())),
                ],
            );
            let (allowed, message) = if risk == "money" {
                queue_manual(user, service, &text(action, "title"), id)?;
                (
                    false,
                    "This moves money, so an agent never does it. It was added to your financial reminders \
                    (run: bin/cashcast reminders sync). You do it yourself.",
                )
            } else if !browser_ok {
                (
                    false,
                    "Browser actions show your account pages to the agent. The owner grants that in a terminal: \
                    bin/cashcast consent grant actions:browser",
                )
            } else if risk == "settings" {
                (
                    true,
                    "This changes a setting. The agent must ask you to confirm this exact change, in chat, right before the final click.",
                )
            } else {
                (true, "Read-only. The agent may do it now.")
            };
            println!(
                "# {service}: {} (risk: {risk})\n\n{message}\n",
                text(action, "title")
            );
            if allowed && !steps.is_empty() {
                println!("{steps}");
            }
            Ok(if allowed { 0 } else { 3 })
        }
        Cmd::FetchPlan(all) => {
            let limit = match settings::get("fetch.stale_days")? {
                settings::Value::Int(n) => n,
                _ => {
                    return Err(AppError::new(
                        "crash",
                        "fetch.stale_days is not a whole number",
                    ));
                }
            };
            let crypto_limit = match settings::get("fetch.crypto_stale_days")? {
                settings::Value::Int(n) => n,
                _ => limit,
            };
            let needs =
                fetch::plan(&user.home, &specs, day, *all, limit, crypto_limit).map_err(back)?;
            let shared = fetch::unnamed(&user.home, &specs).map_err(back)?;
            events::log(
                user,
                "fetch_plan",
                &[
                    ("services", Json::Int(needs.len() as i64)),
                    (
                        "stale",
                        Json::Int(needs.iter().filter(|n| n.stale).count() as i64),
                    ),
                ],
            );
            println!("{}", fetch::render_plan(&needs, limit, shared));
            Ok(0)
        }
        _ => {
            let Cmd::FetchCollect(minutes) = cmd else {
                unreachable!()
            };
            let moved = collect_downloads(user, *minutes)?;
            println!("{}", fetch::render_collect(&moved, *minutes));
            Ok(0)
        }
    }
}

/// `actions.render`: one line per action with its arguments and notes.
fn render_actions(cat: &Json) -> String {
    let s = |v: Option<&Json>| v.filter(|v| v.truthy()).map(Json::py_str);
    let mut lines = vec![
        "# cashcast actions (run each as `bin/cashcast …`; `--json` for the arguments)".to_string(),
        String::new(),
    ];
    let empty = Vec::new();
    let list = |v: Option<&Json>| match v {
        Some(Json::List(l)) => l.clone(),
        _ => empty.clone(),
    };
    let obj = |v: Option<&Json>| match v {
        Some(Json::Obj(o)) => o.clone(),
        _ => Vec::new(),
    };
    for a in list(Some(cat)) {
        let args: Vec<String> = list(a.get("args"))
            .iter()
            .map(|x| {
                let name = x.get("name").map(Json::py_str).unwrap_or_default();
                if let Some(flag) = s(x.get("flag")) {
                    if x.get("kind").map(Json::py_str).as_deref() == Some("flag") {
                        format!("[{flag}]")
                    } else {
                        format!("[{flag} {}]", name.to_uppercase())
                    }
                } else {
                    let choices: Vec<String> =
                        list(x.get("choices")).iter().map(Json::py_str).collect();
                    let base = if choices.is_empty() {
                        name.to_uppercase()
                    } else {
                        format!("{{{}}}", choices.join("|"))
                    };
                    let required = x.get("required").is_some_and(Json::truthy);
                    format!("{base}{}", if required { "" } else { "?" })
                }
            })
            .collect();
        let args = args.join(" ");
        let mut tags = Vec::new();
        if let Some(t) = s(a.get("terminal_only")) {
            tags.push(format!("terminal only: {t}"));
        }
        for (c, _) in obj(a.get("terminal_only_choices")) {
            tags.push(format!("{c}: terminal only"));
        }
        if let Some(c) = s(a.get("confirm")) {
            tags.push(format!("confirm: {c}"));
        }
        for (c, w) in obj(a.get("confirm_choices")) {
            tags.push(format!("{c}: confirm ({})", w.py_str()));
        }
        if let Some(n) = s(a.get("agent_note")) {
            tags.push(n);
        }
        let command = a.get("command").map(Json::py_str).unwrap_or_default();
        let help = a.get("help").map(Json::py_str).unwrap_or_default();
        lines.push(format!(
            "- `{command}{}`: {help}{}",
            if args.is_empty() {
                String::new()
            } else {
                format!(" {args}")
            },
            if tags.is_empty() {
                String::new()
            } else {
                format!(" ({})", tags.join("; "))
            }
        ));
    }
    lines.join("\n")
}

/// Open tasks with these id prefixes change a card (`downgrade-<card id>`).
const CARD_CHANGE_PREFIXES: [&str; 4] = ["downgrade-", "product-change-", "upgrade-", "cancel-"];

/// `cli.card_inputs`: engine args for `cards now` (card balances: `--balance`
/// first, then recorded balances named by a wallet card id; then the open
/// tasks that change a card) and the wallet card ids.
pub fn card_inputs(
    user: &UserHome,
    given: &[String],
) -> Result<(Vec<String>, Vec<String>), AppError> {
    let back =
        |e: toml::de::Error| AppError::new("crash", format!("profile.toml: {}", e.message()));
    let profile: toml::Table = match std::fs::read_to_string(user.profile()) {
        Ok(t) => t.parse().map_err(back)?,
        Err(_) => toml::Table::new(),
    };
    let mut ids = Vec::new();
    let cards = profile
        .get("wallet")
        .and_then(|w| w.get("cards"))
        .and_then(|c| c.as_array());
    for c in cards.into_iter().flatten() {
        match c.get("id") {
            Some(toml::Value::String(s)) if !s.is_empty() => ids.push(s.clone()),
            Some(toml::Value::Integer(i)) if *i != 0 => ids.push(i.to_string()),
            None | Some(toml::Value::String(_)) | Some(toml::Value::Integer(_)) => {}
            Some(_) => {
                return Err(AppError::new(
                    "crash",
                    "profile.toml: a [[wallet.cards]] id is not text",
                ));
            }
        }
    }
    // A dict keyed by card: a later `--balance` for the same card wins, in the first one's place.
    let mut balances: Vec<(String, String)> = Vec::new();
    let mut put =
        |k: String, v: String, replace: bool| match balances.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) if replace => slot.1 = v,
            Some(_) => {}
            None => balances.push((k, v)),
        };
    for b in given {
        put(
            b.split_once('=').map_or(b.as_str(), |(k, _)| k).to_string(),
            b.clone(),
            true,
        );
    }
    let saved = user.data().join("api-balances.json");
    if let Ok(text) = std::fs::read_to_string(&saved) {
        let Some(Json::Obj(services)) = pyjson::loads(&text) else {
            return Err(AppError::new(
                "crash",
                "data/api-balances.json is not a JSON object",
            ));
        };
        for (_, service) in services {
            if let Some(Json::Obj(items)) = service.get("balances") {
                for (name, amount) in items {
                    if ids.contains(name) {
                        put(name.clone(), format!("{name}={}", amount.py_str()), false);
                    }
                }
            }
        }
    }
    let finished: Vec<String> = crate::tasks::changes::done(user)
        .map_err(|e| AppError::new("crash", e.0))?
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let tasks = profile.get("tasks").and_then(|t| t.as_array());
    let mut pending = Vec::new();
    for t in tasks.into_iter().flatten() {
        let Some(tid) = t.as_table().and_then(|t| t.get("id")) else {
            continue;
        };
        let Some(tid) = tid
            .as_str()
            .map(String::from)
            .or_else(|| tid.as_integer().map(|i| i.to_string()))
        else {
            continue;
        };
        for card in &ids {
            if CARD_CHANGE_PREFIXES
                .iter()
                .any(|p| format!("{p}{card}") == tid)
                && !finished.contains(&tid)
            {
                pending.push(format!("{card}={tid}"));
            }
        }
    }
    let mut args: Vec<String> = Vec::new();
    for (_, b) in balances {
        args.extend(["--balance".into(), b]);
    }
    for p in pending {
        args.extend(["--pending".into(), p]);
    }
    Ok((args, ids))
}

/// `engine.forecast`: `forecast-run NAME ARGS`. A file the model cannot
/// read stops it with exit 1, where the Python model raised a traceback.
fn forecast_run(user: &UserHome, name: &str, extra: &[&str]) -> Result<(i32, String), AppError> {
    let mut args = vec!["forecast-run", name];
    args.extend(extra);
    engine(user, &args)
}

/// `fetch.collect`: moves exports changed in the last `minutes` from the
/// downloads folder into the inbox, unopened, and logs `fetch_collect`.
fn collect_downloads(user: &UserHome, minutes: i64) -> Result<Vec<(String, usize)>, AppError> {
    let src = settings::get("fetch.downloads")?.py_str();
    let src = match src.strip_prefix("~/") {
        Some(rest) => {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest)
        }
        None => std::path::PathBuf::from(src),
    };
    let moved = crate::tasks::fetch::collect(&user.inbox(), &src, minutes)
        .map_err(|e| AppError::new("oserror", e))?;
    events::log(
        user,
        "fetch_collect",
        &[(
            "files",
            Json::Int(moved.iter().map(|(_, n)| *n as i64).sum()),
        )],
    );
    Ok(moved)
}

/// `connectors.queue_manual`: a money step joins `data/manual-tasks.json` once.
fn queue_manual(user: &UserHome, service: &str, title: &str, id: &str) -> Result<(), AppError> {
    let io = |e: std::io::Error| AppError::new("oserror", e.to_string());
    let path = user.data().join("manual-tasks.json");
    let mut items = match std::fs::read_to_string(&path) {
        Ok(t) => match pyjson::loads(&t) {
            Some(Json::List(v)) => v,
            _ => {
                return Err(AppError::new("crash", "manual-tasks.json is not a list"));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(io(e)),
    };
    let tid = format!("{service}-{id}");
    if !items
        .iter()
        .any(|i| i.get("id").is_some_and(|v| v.py_str() == tid))
    {
        items.push(Json::Obj(vec![
            ("id".into(), Json::Str(tid)),
            ("title".into(), Json::Str(format!("{title} ({service})"))),
            ("due".into(), Json::Null),
        ]));
    }
    std::fs::create_dir_all(user.data()).map_err(io)?;
    std::fs::write(&path, pyjson::dumps(&Json::List(items), Some(1), false)).map_err(io)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
    Ok(())
}

/// `engine.engine`: this binary run as `ARGS… --home DIR`, stdout then stderr.
fn engine(user: &UserHome, args: &[&str]) -> Result<(i32, String), AppError> {
    let exe = std::env::current_exe().map_err(|e| AppError::new("oserror", e.to_string()))?;
    let o = std::process::Command::new(exe)
        .args(args)
        .arg("--home")
        .arg(&user.home)
        .output()
        .map_err(|e| AppError::new("oserror", e.to_string()))?;
    let mut out = String::from_utf8_lossy(&o.stdout).to_string();
    out.push_str(&String::from_utf8_lossy(&o.stderr));
    Ok((o.status.code().unwrap_or(1), out))
}

/// `moneywiz status`, `coverage` and `sync --dry-run`, as `moneywiz.py`.
fn moneywiz(user: &UserHome, mw: &Mw) -> Result<i32, AppError> {
    // Python checks consent and the gate against the real date, not --today.
    let day = today();
    use crate::tasks::moneywiz_app as app;
    let mut extra: Vec<&str> = Vec::new();
    if let Some(d) = &mw.today {
        extra.extend(["--today", d.as_str()]);
    }
    let run = |args: &[&str]| engine(user, args);
    match mw.action.as_str() {
        "probe" => return Ok(app::probe_command(user, mw.ui)),
        "edit" => {
            let archive = mw.account.as_deref().zip(mw.archived.as_deref());
            return app::edit(user, &run, mw.dry_run, mw.fixes, archive, day);
        }
        "categories" => {
            return app::categories(
                user,
                &run,
                mw.step.as_deref().unwrap_or("list"),
                mw.dry_run,
                mw.name.as_deref(),
                mw.ops.as_deref(),
                mw.yes,
                day,
            );
        }
        "sync" | "weekly" if !mw.dry_run => {
            // Collect: new exports in Downloads go to the inbox first.
            let mut notes = Vec::new();
            if !mw.no_download {
                consent::require(user, "local:moneywiz", day)?;
                let moved = collect_downloads(user, 60)?;
                if !moved.is_empty() {
                    let n: usize = moved.iter().map(|(_, n)| n).sum();
                    let kinds: Vec<String> =
                        moved.iter().map(|(e, n)| format!("{e} ×{n}")).collect();
                    notes.push(format!(
                        "Collected {n} export file(s) from Downloads ({}).",
                        kinds.join(", ")
                    ));
                }
                // Download: fresh Wise statements, when set up, back to `[moneywiz] since`
                // like `api download wise` (a fixed 365 days cut the history). A failure
                // never blocks the sync.
                if consent::allowed(user, "actions:api", day).unwrap_or(false)
                    && super::http::secret(user, "CASHCAST_WISE_TOKEN").is_ok()
                {
                    match crate::api::download(user, "wise", today()) {
                        Ok(saved) => notes.push(format!(
                            "Downloaded {} Wise statement(s) into the inbox.",
                            saved.len()
                        )),
                        Err(e) => {
                            events::error(user, "moneywiz", e.kind);
                            notes.push(
                                "Wise download failed; the sync uses the exports already in the inbox."
                                    .into(),
                            );
                        }
                    }
                }
            }
            return app::sync_apply(user, &run, &extra, mw.resend, &notes, day);
        }
        _ => {}
    }
    let dir = crate::tasks::connectors::dir();
    let dir = dir.to_string_lossy().to_string();
    let args: Vec<&str> = match mw.action.as_str() {
        "status" => vec!["moneywiz", "status"],
        "diagnose" => vec!["moneywiz", "diagnose"],
        "categorize" => {
            let mut a = vec!["moneywiz", "categorize"];
            if mw.dry_run {
                a.push("--dry-run");
            }
            if mw.check {
                a.push("--check");
            }
            if mw.list {
                a.push("--list");
            }
            if let Some(d) = &mw.record_since {
                a.extend(["--record-since", d.as_str()]);
            }
            a
        }
        "resync" => {
            let mut a = vec!["moneywiz", "resync"];
            for (flag, v) in [
                ("--account", &mw.account),
                ("--from", &mw.from),
                ("--to", &mw.to),
            ] {
                if let Some(v) = v {
                    a.extend([flag, v.as_str()]);
                }
            }
            a
        }
        "coverage" => [
            &["moneywiz", "coverage", "--connectors", dir.as_str()][..],
            &extra,
        ]
        .concat(),
        _ => [&["moneywiz", "sync"][..], &extra].concat(),
    };
    let (code, out) = engine(user, &args)?;
    super::gate::emit(user, &out, "agent:aggregates", "moneywiz", day)?;
    if mw.action == "coverage" && code == 0 {
        let mut nums = coverage_counts(&out);
        nums.resize(4, 0);
        events::log(
            user,
            "moneywiz_coverage",
            &[
                ("covered", Json::Int(nums[0])),
                ("rows", Json::Int(nums[1])),
                ("covered_recent", Json::Int(nums[2])),
                ("rows_recent", Json::Int(nums[3])),
            ],
        );
    }
    if mw.action == "sync"
        && code == 0
        && app::settings(user)?.get("fixes").and_then(|f| f.as_str()) == Some("edit")
    {
        return app::edit(user, &run, true, true, None, day);
    }
    Ok(code)
}

/// `re.findall(r"(\d+) of (\d+) rows", report)[:2]`, flattened.
fn coverage_counts(report: &str) -> Vec<i64> {
    let b = report.as_bytes();
    let back = |end: usize| (0..end).rev().take_while(|&i| b[i].is_ascii_digit()).last();
    let mut nums = Vec::new();
    for (at, _) in report.match_indices(" rows") {
        let Some(s2) = back(at) else { continue };
        if s2 < 4 || &report[s2 - 4..s2] != " of " {
            continue;
        }
        let Some(s1) = back(s2 - 4) else { continue };
        nums.push(report[s1..s2 - 4].parse().unwrap_or(0));
        nums.push(report[s2..at].parse().unwrap_or(0));
        if nums.len() == 4 {
            break;
        }
    }
    nums
}

#[cfg(test)]
mod tests {
    use super::{
        Cmd, Mw, coverage_counts, engine_today, iso_date, reads_today, weekly_sync_step,
    };
    use chrono::NaiveDate;

    #[test]
    fn engine_subprocesses_get_today_as_y_m_d() {
        for given in ["2026-10-18", "20261018", "2026-W42-7", "2026W427"] {
            assert_eq!(
                engine_today(given).as_deref(),
                Some("2026-10-18"),
                "{given}"
            );
        }
        assert_eq!(engine_today("nope"), None);
    }

    #[test]
    fn engine_commands_stop_on_a_bad_today() {
        let t = Some("x".to_string());
        assert!(reads_today(&Cmd::Report("import", t.clone())));
        assert!(reads_today(&Cmd::Forecast("forecast", t.clone())));
        assert!(reads_today(&Cmd::Cards(Vec::new(), t.clone(), Vec::new())));
        let mw = Mw {
            today: t,
            ..Mw::default()
        };
        assert!(reads_today(&Cmd::Moneywiz(Box::new(mw))));
        assert!(!reads_today(&Cmd::Report("totals", None)));
    }

    #[test]
    fn weekly_writes_reminders_only_after_the_owner_confirms() {
        assert_eq!(weekly_sync_step(false, false), ["sync", "status"]);
        assert_eq!(weekly_sync_step(true, true), ["sync", "status"]);
        assert_eq!(weekly_sync_step(false, true), ["sync", "all"]);
    }

    #[test]
    fn week_dates_reject_year_zero() {
        for s in ["0000-W01-1", "0000W011", "0000-W01"] {
            assert_eq!(iso_date(s), Err(format!("Invalid isoformat string: '{s}'")));
        }
    }

    #[test]
    fn today_reads_like_date_fromisoformat() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 18);
        assert_eq!(iso_date("2026-10-18").ok(), d);
        assert_eq!(iso_date("20261018").ok(), d);
        assert_eq!(iso_date("2026-W42-7").ok(), d);
        assert_eq!(iso_date("2026W427").ok(), d);
        assert_eq!(
            iso_date("2026-W42").ok(),
            NaiveDate::from_ymd_opt(2026, 10, 12)
        );
        assert_eq!(iso_date("x"), Err("Invalid isoformat string: 'x'".into()));
        assert_eq!(
            iso_date("2026-1-1"),
            Err("Invalid isoformat string: '2026-1-1'".into())
        );
        assert_eq!(
            iso_date("2026-02-30"),
            Err("day is out of range for month".into())
        );
    }

    #[test]
    fn coverage_counts_match_the_python_regex() {
        let r = "Covered: 12 of 30 rows.\nLast 12 months: x7 of 9 rows\nAlso 1 of 2 rows";
        assert_eq!(coverage_counts(r), vec![12, 30, 7, 9]);
        assert_eq!(coverage_counts("no numbers of rows"), Vec::<i64>::new());
    }
}

fn notion(user: &UserHome, source: &str, delete: bool) -> Result<i32, AppError> {
    use crate::docs;
    use std::path::{Path, PathBuf};
    users::create_folders(user)?;
    let source = match source.strip_prefix("~") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => PathBuf::from(format!(
            "{}{rest}",
            std::env::var("HOME").unwrap_or_default()
        )),
        // `~name/…` is another user's home, as Python's `expanduser`.
        Some(rest) => {
            let (name, tail) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            match paths::home_of(name) {
                Some(home) => PathBuf::from(format!("{}{tail}", home.trim_end_matches('/'))),
                None => PathBuf::from(source),
            }
        }
        _ => PathBuf::from(source),
    };
    let report = docs::import_notion(user, &source)?;
    let root = paths::root();
    let rel = user.docs();
    let rel = rel
        .strip_prefix(&root)
        .map_or(rel.clone(), Path::to_path_buf);
    println!(
        "{}",
        report.render(&format!("{}/{}", rel.display(), docs::IMPORT_DIR))
    );
    if delete {
        let gone = if source.is_dir() {
            std::fs::remove_dir_all(&source)
        } else {
            std::fs::remove_file(&source)
        };
        gone.map_err(|e| AppError::new("oserror", e.to_string()))?;
        println!("Deleted the export. Empty the Trash too, and check Downloads for other copies.");
    } else {
        println!(
            "Delete the export file when you have checked the documents (it holds the same text, unprotected)."
        );
    }
    events::log(
        user,
        "notion_import",
        &[
            ("pages", Json::Int(report.pages)),
            ("tables", Json::Int(report.tables)),
            ("files", Json::Int(report.files)),
            ("skipped", Json::Int(report.skipped.len() as i64)),
        ],
    );
    Ok(0)
}

/// The tasks the synced targets get: weekly and monthly ones fold into one
/// weekly session reminder (setting `reminders.weekly`; `off` keeps them apart).
fn weekly_fold(
    open: Vec<crate::strategy::Task>,
    on: chrono::NaiveDate,
    finished: &[(String, Json)],
) -> Result<Vec<crate::strategy::Task>, AppError> {
    use crate::tasks::weekly;
    Ok(
        match weekly::weekday(&settings::get("reminders.weekly")?.py_str()) {
            Some(day) => {
                let ids: Vec<String> = finished.iter().map(|(k, _)| k.clone()).collect();
                weekly::fold(open, on, day, &ids)
            }
            None => open,
        },
    )
}

/// `grocery …`: the grocery price database (moved from sidequest). Usage
/// mistakes print one line and exit 2; "nothing found" exits 1.
fn grocery(user: &UserHome, g: &Grocery, day: NaiveDate) -> Result<i32, AppError> {
    use crate::groceries as gr;
    let fail = |e: String| AppError::new("runtimeerror", e);
    let usage = |msg: &str| {
        println!("{msg}");
        Ok(2)
    };
    let num = |flag: &str, v: &Option<String>| -> Result<Option<f64>, String> {
        match v.as_deref() {
            None => Ok(None),
            Some(t) => match t.trim().trim_start_matches('$').parse::<f64>() {
                Ok(n) if n.is_finite() && n >= 0.0 => Ok(Some(n)),
                _ => Err(format!("{flag} must be a number of 0 or more, got '{t}'")),
            },
        }
    };
    let prefs = users::load_profile(user)
        .map(|p| gr::prefs(&p))
        .unwrap_or_else(|_| gr::prefs(&toml::Table::new()));
    let days = match g.days {
        Some(n) if n < 0 => return usage(&format!("--days must be a whole number of 0 or more, got '{n}'")),
        Some(n) => n,
        None => prefs.fresh_days,
    };
    let premium = match num("--organic-premium", &g.organic_premium) {
        Ok(v) => v.unwrap_or(prefs.organic_premium),
        Err(e) => return usage(&e),
    };
    let path = gr::file(&user.home);
    let name = g.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let items: Vec<String> = g
        .items
        .as_deref()
        .or(name)
        .unwrap_or("")
        .split(',')
        .map(|x| x.trim().to_lowercase())
        .filter(|x| !x.is_empty())
        .collect();
    let event = |action: &str, n: usize| {
        events::log(
            user,
            "grocery",
            &[
                ("action", Json::Str(action.into())),
                ("rows", Json::Int(n as i64)),
            ],
        );
    };
    match g.action.as_str() {
        "show" | "best" => {
            let Some(item) = name else {
                return usage(&format!("grocery {} needs an item, for example: bin/cashcast grocery {} oats", g.action, g.action));
            };
            let rows = gr::latest(&path, item, days, day).map_err(fail)?;
            if rows.is_empty() {
                println!("no fresh prices for '{item}' (last {days} days). Search the stores, then: bin/cashcast grocery save --file rows.json");
                return Ok(1);
            }
            if g.action == "show" {
                for r in &rows {
                    println!("{:<26} {}", r.store, gr::line(r));
                }
            } else {
                let stores: std::collections::BTreeSet<&str> = rows.iter().map(|r| r.store.as_str()).collect();
                for st in stores {
                    let here: Vec<gr::Priced> = rows.iter().filter(|r| r.store == st).cloned().collect();
                    let (best, sub) = gr::pick(&here, premium).expect("rows at this store");
                    println!("{st}\n  best: {}\n  sub:  {}", gr::line(&best), sub.as_ref().map_or_else(|| "-".into(), gr::line));
                }
            }
        }
        "stale" | "compare" => {
            if items.is_empty() {
                return usage(&format!("grocery {} needs --items, for example: bin/cashcast grocery {} --items oats,tea", g.action, g.action));
            }
            if g.action == "stale" {
                let st = gr::stale(&path, &items, days, day).map_err(fail)?;
                println!("{}", if st.is_empty() { "all fresh".to_string() } else { st.join("\n") });
            } else {
                let res = gr::compare(&path, &items, premium, days, day).map_err(fail)?;
                if res.is_empty() {
                    println!("no fresh prices for {} (last {days} days)", items.join(", "));
                    return Ok(1);
                }
                println!("{}", gr::render_compare(&res));
                event("compare", res.len());
            }
        }
        "save" => {
            let Some(file) = g.file.as_deref() else {
                return usage("grocery save needs --file ROWS.json (a JSON array of {item, store, name, price, ...})");
            };
            let text = std::fs::read_to_string(file).map_err(AppError::io)?;
            let rows: Vec<gr::Row> = serde_json::from_str(&text)
                .map_err(|e| AppError::new("valueerror", format!("{file}: {e}")))?;
            let n = gr::save(&path, &rows, day).map_err(fail)?;
            event("save", n);
            println!("saved {n} prices -> {}", paths::rel(&path));
            return Ok(if n > 0 { 0 } else { 1 });
        }
        "fee" => {
            let (Some(store), Some(_)) = (name, g.delivery.as_ref()) else {
                return usage("usage: bin/cashcast grocery fee STORE --delivery N [--free-over N] [--service N] [--pct-off N --max-off N --min-order N] [--note TEXT]");
            };
            let parsed = (|| -> Result<gr::Fee, String> {
                Ok(gr::Fee {
                    store: store.to_string(),
                    delivery: num("--delivery", &g.delivery)?.unwrap_or(0.0),
                    service: num("--service", &g.service)?.unwrap_or(0.0),
                    free_over: num("--free-over", &g.free_over)?,
                    pct_off: num("--pct-off", &g.pct_off)?,
                    max_off: num("--max-off", &g.max_off)?,
                    min_order: num("--min-order", &g.min_order)?,
                    note: g.note.clone().unwrap_or_default(),
                    checked: String::new(),
                })
            })();
            let fee = match parsed {
                Ok(f) => f,
                Err(e) => return usage(&e),
            };
            gr::set_fee(&path, &fee, day).map_err(fail)?;
            println!("fee saved for {store}");
        }
        "fees" => {
            let f = gr::fees(&path).map_err(fail)?;
            if f.is_empty() {
                println!("no fee rules yet. Read a store's checkout page, then: bin/cashcast grocery fee STORE --delivery N");
            }
            for x in &f {
                println!("{}", gr::render_fee(x));
            }
        }
        "order" => {
            let total = match num("--total", &g.total) {
                Ok(Some(t)) if t > 0.0 => t,
                Ok(_) => return usage("usage: bin/cashcast grocery order STORE --total N [--note TEXT]"),
                Err(e) => return usage(&e),
            };
            let Some(store) = name else {
                return usage("usage: bin/cashcast grocery order STORE --total N [--note TEXT]");
            };
            gr::record_order(
                &path,
                &gr::Order {
                    day: day.to_string(),
                    store: store.to_string(),
                    total,
                    note: g.note.clone().unwrap_or_default(),
                },
            )
            .map_err(fail)?;
            event("order", 1);
            println!("order saved: {day} {store} {total:.2}. See: bin/cashcast grocery orders");
        }
        "orders" => {
            let all = gr::orders(&path).map_err(fail)?;
            let mut out = Vec::new();
            if all.is_empty() {
                out.push("no grocery orders yet. After checkout: bin/cashcast grocery order STORE --total N".to_string());
            } else {
                out.push("## Grocery spend by month".to_string());
                for m in gr::months(&all) {
                    out.push(format!("- {}: {:.2} in {} order(s)", m.month, m.total, m.orders));
                }
                out.push(String::new());
                out.push("## Orders".to_string());
                for o in all.iter().take(20) {
                    let note = if o.note.is_empty() { String::new() } else { format!(" ({})", o.note) };
                    out.push(format!("- {} {} {:.2}{note}", o.day, o.store, o.total));
                }
            }
            let mut text = out.join("\n");
            text.push('\n');
            // Amounts the owner paid: owner-only, like `charges`.
            super::gate::emit(user, &text, "agent:strategy", "grocery", day)?;
        }
        "prefs" => {
            println!("organic_premium: {}", prefs.organic_premium);
            println!("fresh_days: {}", prefs.fresh_days);
            for l in &prefs.lines {
                println!("{l}");
            }
            if prefs.lines.is_empty() {
                println!("(add rules, quantities and memberships under [groceries] in {})", paths::rel(&user.profile()));
            }
        }
        "import" => {
            let Some(from) = g.from.as_deref() else {
                return usage("grocery import needs --from OLD.sqlite, for example sidequest's private/groceries/groceries.sqlite");
            };
            let from = match from.strip_prefix("~/") {
                Some(rest) => std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
                None => std::path::PathBuf::from(from),
            };
            let (p, f) = gr::import(&path, &from).map_err(fail)?;
            event("import", p);
            println!("imported {p} prices and {f} fee rules -> {} (the old file is unchanged)", paths::rel(&path));
        }
        _ => {
            let k = gr::known(&path).map_err(fail)?;
            if k.is_empty() {
                println!("no grocery prices yet. Ask Claude \"order groceries\" (skills/grocery-cart), or: bin/cashcast grocery import --from OLD.sqlite");
            }
            for x in &k {
                println!("{:<30} {} store(s), checked {}", x.item, x.stores, x.checked);
            }
        }
    }
    Ok(0)
}
