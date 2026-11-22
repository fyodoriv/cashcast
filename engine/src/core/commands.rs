//! The CLI parser, as Python's `cli.parser()` declared it, and the action
//! catalog generated from it (`bin/cashcast actions --json`, the web app's
//! Actions view). One source: a new command is an action the moment it exists.

use super::argparse::{Arg, Parser, Sub, Val};
use super::consent;
use super::pyjson::Json;

/// One line per command, in parser order.
pub const SUMMARY: [(&str, &str); 49] = [
    (
        "start",
        "set up, start the web app, refresh, and print what matters now (also: no command)",
    ),
    ("stop", "stop the web app that `bin/cashcast` started"),
    (
        "weekly",
        "the Sunday session in one run: fresh data, MoneyWiz, forecast, transfers, cards, charges, investing, tasks, and this week's moves",
    ),
    ("help", "print the command overview"),
    ("init", "create your data folder"),
    ("import", "read the exports in your inbox"),
    ("status", "import, then the forecast summary"),
    ("forecast", "import, then the month-by-month forecast"),
    (
        "backtest",
        "import, then the last forecast vs actual, a backtest per line, and the spending methods learned",
    ),
    (
        "numbers",
        "make an Apple Numbers copy of the forecast workbook",
    ),
    ("totals", "totals per category and month"),
    ("first-error", "print the first unreadable export row"),
    (
        "columns",
        "header names and masked cell shapes of each inbox file (no values)",
    ),
    (
        "holdings",
        "investments and crypto: quantity, cost base (ACB) and gains",
    ),
    (
        "actions",
        "list every action with its arguments (for agents: --json)",
    ),
    (
        "config",
        "show the settings and where each value comes from, or set one",
    ),
    ("users", "list, add or adopt people"),
    (
        "consent",
        "list, grant or revoke what may leave this computer",
    ),
    (
        "telemetry",
        "opt-in anonymous usage counts to PostHog EU: status, on, off, send (--dry-run shows the body)",
    ),
    ("strategy", "the full money plan (saved to out/strategy.md)"),
    (
        "net-total",
        "your net total (all you have minus all you owe), its trend, and the options ranked by what they add, inside your boundaries",
    ),
    ("review", "the monthly review and checklist"),
    ("budget", "the budget section"),
    ("tax", "rates, room and tax moves"),
    (
        "rrsp",
        "this year's RRSP contributions (Sun Life and others) against your deduction limit",
    ),
    ("alternatives", "cheaper cards, banks and services"),
    ("payments", "fees, interest and recurring payments"),
    (
        "spending",
        "spending checkup: more than usual, spend vs earn, interest and fees paid",
    ),
    (
        "invest-plan",
        "ranked investment moves: gain ranges, evidence, risk, cost and do-nothing (override in [invest])",
    ),
    (
        "invest",
        "long-term investing: core, placement and watchlist screen",
    ),
    (
        "reminders",
        "preview or sync your tasks to one Apple Reminders list",
    ),
    (
        "calendar",
        "preview or sync your dated deadlines to one Apple Calendar",
    ),
    (
        "tasks",
        "your open finance tasks: who does each (list), resolve what cashcast can and name every blocker (resolve), or record an agent check (verdict)",
    ),
    (
        "changes",
        "bank and card settings to change, ready for a browser session",
    ),
    (
        "transfers",
        "e-Transfers that bring each bank to its keep amount, from balances you read (daily limit, amounts quick to type)",
    ),
    (
        "charges",
        "unexpected charges on every account: duplicate subscriptions, new recurring charges, price rises, charges after a pause; find WORD lists one merchant's charges",
    ),
    (
        "cards",
        "which card to use now per spending category: cap used and left, reset date, rent charges, credit left",
    ),
    (
        "optimize",
        "where new money goes to reach your target weights (you place the trades)",
    ),
    (
        "connectors",
        "the services cashcast knows, and how to export from each",
    ),
    ("action", "the safe steps for one service action, by risk"),
    (
        "api",
        "read balances or download exports through a service's API, or record balances read in the browser",
    ),
    (
        "fetch",
        "which exports are stale and how to get each (plan), or move new downloads into the inbox unopened (collect)",
    ),
    (
        "sync",
        "keep Reminders, Calendar and Notion in step (all, status, install hourly), or write the forecast to a Google Sheet",
    ),
    (
        "moneywiz",
        "compare, sync and fix MoneyWiz against your exports",
    ),
    ("notion", "import a Notion Markdown & CSV export, offline"),
    ("web", "run the web app in this terminal"),
    (
        "knowledge",
        "check, list, propose or refresh (live rates) the sourced facts",
    ),
    (
        "paperwork",
        "citizenship steps, presence count and official sources, sealed on this Mac (terminal only)",
    ),
    (
        "citizenship",
        "the guided citizenship application: answers, judges, proofs and the local evidence page (terminal only)",
    ),
];

/// Whole commands, or one choice of a command ("consent.grant"), that need a terminal.
const TERMINAL_ONLY: [(&str, &str); 7] = [
    (
        "paperwork",
        "paperwork is private: run `bin/cashcast paperwork` in Terminal on this Mac",
    ),
    (
        "citizenship",
        "the application holds ID numbers: run `bin/cashcast citizenship` in Terminal on this Mac",
    ),
    ("web", "it is the web app itself"),
    (
        "start",
        "it starts the web app you are using; run `bin/cashcast` in a terminal",
    ),
    (
        "stop",
        "it stops the web app you are using; run `bin/cashcast stop` in a terminal",
    ),
    (
        "consent.grant",
        "consent is typed into your own terminal, so no web page or agent can grant it",
    ),
    (
        "api.etrade-auth",
        "the E*TRADE code goes to a prompt in your own terminal",
    ),
];

const CONFIRM: [(&str, &str); 16] = [
    ("reminders.sync", "writes your tasks to Apple Reminders"),
    (
        "calendar.sync",
        "writes your dated deadlines to Apple Calendar",
    ),
    ("moneywiz.sync", "adds rows to MoneyWiz"),
    ("moneywiz.edit", "changes rows in MoneyWiz"),
    ("sync.sheets", "writes the forecast to your Google Sheet"),
    (
        "sync.all",
        "writes your tasks to Reminders, Calendar and Notion, and records the ones you ticked done",
    ),
    (
        "sync.install",
        "adds a LaunchAgent that runs sync all every hour",
    ),
    ("sync.uninstall", "removes the hourly sync"),
    (
        "notion.import",
        "imports documents (with delete_source it deletes the export)",
    ),
    ("first-error", "shows one raw row of your data on screen"),
    ("consent.revoke", "turns off a consent you granted"),
    (
        "telemetry.send",
        "sends anonymous usage counts to PostHog EU (with consent net:usage-counts and a key)",
    ),
    (
        "tasks.resolve",
        "marks due tasks done that cashcast resolved itself (undo: changes undo ID)",
    ),
    ("users.add", "creates a person's data folder"),
    (
        "weekly",
        "adds and fixes MoneyWiz rows, marks resolved tasks done and writes them to Reminders, Calendar and Notion (--dry-run changes nothing)",
    ),
    (
        "users.adopt",
        "moves the single-user data folder into a person's folder",
    ),
];

const AGENT_NOTES: [(&str, &str); 10] = [
    (
        "telemetry",
        "agents: ask the owner in chat before `telemetry send` (without --dry-run) or `telemetry on`",
    ),
    (
        "consent",
        "agents: never grant; give the owner the `consent grant SCOPE` command to run in a terminal",
    ),
    (
        "first-error",
        "agents: only with the owner's OK in chat and the agent:raw-row scope",
    ),
    (
        "changes",
        "agents: follow skills/bank-change; ask for a yes in chat right before each final click",
    ),
    (
        "tasks",
        "agents: use /cashcast-tasks; report every BLOCKED task to the owner, never call it done",
    ),
    (
        "action",
        "agents: money actions become reminders; never move money",
    ),
    (
        "optimize",
        "agents: show the plan; the owner places every trade",
    ),
    (
        "reminders",
        "agents: preview here; the owner runs `sync` in Terminal (the sandbox blocks osascript)",
    ),
    (
        "calendar",
        "agents: preview here; the owner runs `sync` in Terminal (the sandbox blocks osascript)",
    ),
    (
        "sync",
        "agents: run `sync status`; the owner runs `sync all` and `sync install` in Terminal (the sandbox blocks osascript and launchctl)",
    ),
];

const MARKET: &str = "use live market data (needs net:market-data)";

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// The arguments of one command, in the order Python added them.
fn command_args(name: &str) -> Vec<Arg> {
    let s = |v: &str| Val::Str(v.to_string());
    match name {
        "actions" => vec![Arg::switch("--json").help("the catalog as JSON")],
        "config" => vec![
            Arg::positional("action")
                .maybe()
                .default(s("show"))
                .choices(&["show", "set"]),
            Arg::positional("key")
                .maybe()
                .help("a setting key, for example web.port"),
            Arg::positional("value").maybe().help("the new value"),
        ],
        "users" => vec![
            Arg::positional("action").choices(&["list", "add", "adopt"]),
            Arg::positional("name")
                .maybe()
                .help("a short name (letters, digits)"),
            Arg::option("--province")
                .default(s(""))
                .help("two-letter province or territory, for example ON"),
            Arg::option("--display").help("the name shown in reports"),
        ],
        "consent" => {
            let scopes: Vec<&'static str> = consent::SCOPES.iter().map(|s| s.name).collect();
            vec![
                Arg::positional("action").choices(&["list", "grant", "revoke"]),
                Arg::positional("scope").maybe().choices(&scopes),
            ]
        }
        "telemetry" => vec![
            Arg::positional("action")
                .maybe()
                .default(s("status"))
                .choices(&["status", "on", "off", "send"]),
            Arg::switch("--dry-run")
                .help("send: print the exact body with the key hidden; send nothing"),
        ],
        "strategy" => vec![
            Arg::option("--section")
                .append()
                .help("limit the report to one section id"),
            Arg::switch("--market").help(MARKET),
        ],
        "invest" => vec![Arg::switch("--market").help(MARKET)],
        "weekly" => vec![
            Arg::switch("--dry-run")
                .help("show what MoneyWiz and the task list would change; change neither"),
            Arg::option("--balance")
                .append()
                .metavar("ID=AMOUNT")
                .help("a balance read today on the bank or card site, for transfers and cards; repeat per account"),
        ],
        "reminders" | "calendar" => vec![Arg::positional("action").choices(&["preview", "sync"])],
        "tasks" => vec![
            Arg::positional("action")
                .maybe()
                .default(s("list"))
                .choices(&["list", "resolve", "verdict"]),
            Arg::positional("id")
                .maybe()
                .help("verdict: the check's task id"),
            Arg::positional("result")
                .maybe()
                .choices(&["pass", "fail"])
                .help("verdict: what the evidence showed"),
            Arg::option("--reason").help(
                "verdict: what the evidence showed, in words; no amounts, balances or scores",
            ),
            Arg::option("--source")
                .choices(&["local", "browser"])
                .default(s("local"))
                .help("verdict: browser needs agent:browser-read"),
            Arg::switch("--json")
                .help("list: one JSON row per task (id, due, lane, step, risk)"),
        ],
        "changes" => vec![
            Arg::positional("action")
                .maybe()
                .default(s("list"))
                .choices(&["list", "show", "done", "undo"]),
            Arg::positional("id")
                .maybe()
                .help("a change id from the list"),
        ],
        "transfers" => vec![
            Arg::option("--balance")
                .append()
                .metavar("ID=AMOUNT")
                .help("a balance read from the bank site; repeat per account or card"),
        ],
        "charges" => vec![
            Arg::positional("action")
                .maybe()
                .default(s("check"))
                .choices(&["check", "find"]),
            Arg::positional("word")
                .maybe()
                .help("find: a merchant name or billing word, for example anthropic"),
        ],
        "cards" => vec![
            Arg::positional("action")
                .maybe()
                .default(s("now"))
                .choices(&["now"]),
            Arg::option("--balance").append().metavar("CARD=AMOUNT").help(
                "a card balance read on the card site; repeat per card (default: recorded balances)",
            ),
        ],
        "optimize" => vec![
            Arg::option("--input").help("a TOML file with [optimize]; default: profile.toml"),
        ],
        "connectors" => vec![
            Arg::positional("action").choices(&["list", "show"]),
            Arg::positional("service")
                .maybe()
                .help("a service id, for example td"),
        ],
        "action" => vec![
            Arg::positional("service").help("a service id, for example td"),
            Arg::positional("action_id").help("an action id from `connectors show SERVICE`"),
        ],
        "api" => vec![
            Arg::positional("action").choices(&["balances", "download", "etrade-auth", "record"]),
            Arg::positional("service")
                .maybe()
                .default(s("etrade"))
                .help("a service id"),
            Arg::positional("target")
                .maybe()
                .help("Google Sheets: the sheet id"),
            Arg::positional("gid")
                .maybe()
                .help("Google Sheets: the tab id"),
            Arg::option("--set").help(
                "record: balances read in the browser, NAME=AMOUNT,NAME=AMOUNT (for example TFSA=1000,RRSP=500)",
            ),
            Arg::option("--currency")
                .default(s("CAD"))
                .help("record: the currency of the amounts"),
        ],
        "fetch" => vec![
            Arg::positional("action").choices(&["plan", "collect"]),
            Arg::switch("--all")
                .help("plan: every service with an export, not only those in the inbox"),
            Arg::option("--minutes")
                .int()
                .default(Val::Int(60))
                .help("collect: files saved in the last N minutes"),
        ],
        "sync" => vec![
            Arg::positional("action").choices(&["all", "status", "install", "uninstall", "sheets"]),
            Arg::positional("dest")
                .maybe()
                .help("sheets: the Google Sheet id"),
            Arg::switch("--allow-removals").help("all: remove more than 10 items in one run"),
            Arg::positional("range")
                .maybe()
                .help("where to write, for example Cashcast!A1"),
        ],
        "moneywiz" => vec![
            Arg::positional("action").choices(&[
                "status",
                "coverage",
                "diagnose",
                "categorize",
                "sync",
                "weekly",
                "resync",
                "edit",
                "probe",
                "categories",
            ]),
            Arg::switch("--dry-run").help("show what would change, change nothing"),
            Arg::positional("step")
                .maybe()
                .choices(&["list", "suggest", "new", "migrate", "rollback"])
                .help("categories: what to do"),
            Arg::option("--name").help("categories new: a short name for the migration file"),
            Arg::option("--ops").help(
                "categories new: the changes as JSON, a list of [[op]] tables (the web app sends them)",
            ),
            Arg::switch("--yes").help(
                "categories migrate, rollback: change MoneyWiz (after a --dry-run preview)",
            ),
            Arg::switch("--ui").help("probe: also check the window fallback"),
            Arg::switch("--fixes").help("edit: also apply the fixes a sync found"),
            Arg::switch("--no-download").help("sync: skip the API downloads"),
            Arg::option("--account")
                .help("resync: only this MoneyWiz or export account; edit: the account --archived changes"),
            Arg::option("--archived").choices(&["yes", "no"]).help(
                "edit: archive the --account in MoneyWiz (yes), or show it again so the sync can add rows (no)",
            ),
            Arg::option("--from").help("resync: first day to check again, YYYY-MM-DD"),
            Arg::option("--to").help("resync: last day to check again, YYYY-MM-DD"),
            Arg::switch("--check").help(
                "categorize: hide every 5th row you categorized and report how often learning agrees",
            ),
            Arg::switch("--list").help(
                "categorize: also list the rows with no category found (private: day, account, amount, payee)",
            ),
            Arg::switch("--resend").help(
                "sync: send again the rows MoneyWiz held once and lacks now (deleted there?)",
            ),
            Arg::option("--record-since").help(
                "categorize: note the rows categorized since this backup folder as cashcast's",
            ),
        ],
        "notion" => vec![
            Arg::positional("action").choices(&["import"]),
            Arg::positional("source").help("path to the export .zip or folder"),
            Arg::switch("--delete-source").help("delete the export after the import"),
        ],
        "web" => vec![
            Arg::option("--port")
                .int()
                .default(Val::Int(0))
                .help("0 picks a free port"),
        ],
        "paperwork" | "citizenship" => vec![Arg::positional("verb")
            .maybe()
            .help("a verb, then its words; `help` lists them")],
        "knowledge" => vec![
            Arg::positional("action").choices(&["check", "list", "propose", "refresh"]),
            Arg::positional("file")
                .maybe()
                .help("propose: path to a <topic>.toml"),
        ],
        _ => Vec::new(),
    }
}

/// `cli.parser()`.
pub fn parser() -> Parser {
    let mut p = Parser::new("bin/cashcast", false);
    p.add(Arg::option("--user"));
    p.add(Arg::option("--today"));
    let parsers = SUMMARY
        .iter()
        .map(|(name, help)| {
            let mut sp = Parser::new(&format!("bin/cashcast {name}"), true);
            for a in command_args(name) {
                sp.add(a);
            }
            (*name, *help, sp)
        })
        .collect();
    p.sub = Some(Sub {
        dest: "command",
        parsers,
    });
    p
}

fn opt_str(v: Option<&str>) -> Json {
    v.map_or(Json::Null, |s| Json::Str(s.into()))
}

/// `actions._arg`: one argument as the catalog lists it.
fn arg_json(a: &Arg) -> Json {
    use super::argparse::Kind;
    let flag = a.flags.iter().find(|f| f.starts_with("--")).copied();
    let kind = if a.kind == Kind::StoreTrue {
        "flag"
    } else if a.choices.is_some() {
        "choice"
    } else if a.int {
        "int"
    } else {
        "text"
    };
    // Python: `None if a.default in (None, SUPPRESS, False)`, and 0 == False.
    let default = match &a.default {
        Val::None | Val::Bool(false) | Val::Int(0) => Json::Null,
        Val::Bool(true) => Json::Bool(true),
        Val::Int(n) => Json::Int(*n),
        Val::Str(s) => Json::Str(s.clone()),
        Val::List(l) => Json::List(l.iter().map(|s| Json::Str(s.clone())).collect()),
    };
    Json::Obj(vec![
        ("name".into(), Json::Str(a.dest.into())),
        ("flag".into(), opt_str(flag)),
        ("kind".into(), Json::Str(kind.into())),
        (
            "choices".into(),
            a.choices.as_ref().map_or(Json::Null, |c| {
                Json::List(c.iter().map(|s| Json::Str((*s).into())).collect())
            }),
        ),
        (
            "required".into(),
            Json::Bool(a.flags.is_empty() && !a.optional),
        ),
        ("help".into(), Json::Str(a.help.unwrap_or("").into())),
        ("default".into(), default),
    ])
}

/// `actions.catalog()`: every command with its arguments, terminal-only
/// reasons, confirmations and agent notes.
pub fn catalog() -> Json {
    let mut out = Vec::new();
    for (name, help) in SUMMARY {
        let args = command_args(name);
        let choices: Vec<&str> = args
            .iter()
            .find(|a| a.dest == "action" && a.choices.is_some())
            .and_then(|a| a.choices.clone())
            .unwrap_or_default();
        let terminal_choices: Vec<(String, Json)> = choices
            .iter()
            .filter_map(|c| {
                lookup(&TERMINAL_ONLY, &format!("{name}.{c}"))
                    .map(|r| ((*c).to_string(), Json::Str(r.into())))
            })
            .collect();
        let confirm_choices: Vec<(String, Json)> = choices
            .iter()
            .filter_map(|c| {
                lookup(&CONFIRM, &format!("{name}.{c}"))
                    .map(|r| ((*c).to_string(), Json::Str(r.into())))
            })
            .collect();
        out.push(Json::Obj(vec![
            ("name".into(), Json::Str(name.into())),
            ("command".into(), Json::Str(format!("bin/cashcast {name}"))),
            ("help".into(), Json::Str(help.into())),
            (
                "args".into(),
                Json::List(args.iter().map(arg_json).collect()),
            ),
            (
                "ui".into(),
                Json::Bool(lookup(&TERMINAL_ONLY, name).is_none()),
            ),
            (
                "terminal_only".into(),
                opt_str(lookup(&TERMINAL_ONLY, name)),
            ),
            ("terminal_only_choices".into(), Json::Obj(terminal_choices)),
            ("confirm".into(), opt_str(lookup(&CONFIRM, name))),
            ("confirm_choices".into(), Json::Obj(confirm_choices)),
            ("agent_note".into(), opt_str(lookup(&AGENT_NOTES, name))),
        ]));
    }
    Json::List(out)
}
