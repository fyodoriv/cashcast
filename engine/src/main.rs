//! `cashcast-engine`: exports in, aggregates out. Usually run through
//! `bin/cashcast`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cashcast_engine::{
    cards, charges, config, inbox, inspect, invest_plan, ledger, money, moneywiz,
    moneywiz_categorize, moneywiz_coverage, moneywiz_diagnose, moneywiz_edit, moneywiz_ledger,
    net_total, summarize, transfers, write_private,
};

const USAGE: &str = "\
usage: cashcast-engine COMMAND [--home DIR] [--today YYYY-MM-DD]

  import       Reads every file in DIR/inbox and prints the import report:
               files, accounts, pay, categories and data checks. Writes
               DIR/data/summary.json (aggregates) and DIR/data/review.md
               (private payee samples for the owner only).
  totals       Prints sums per account and per category.
  holdings     Reads the investment and crypto ledgers in DIR/inbox (Kraken,
               Wealthsimple activity, E*TRADE, crypto wallet CSVs) and
               prints holdings, average cost (ACB) and realized gains per
               tax year. Writes DIR/data/holdings.json (aggregates).
  first-error  Prints the first unreadable row (or file) and nothing else.
  moneywiz status
               Lists the MoneyWiz accounts and the export each one follows.
  moneywiz sync [--apply] [--resend]
               Compares MoneyWiz with the bank and card exports. With
               --apply, hands the missing rows to MoneyWiz (moneywiz:// links)
               and writes DIR/out/moneywiz-fixes.md for the owner.
  moneywiz edits [--fixes] [--account NAME --archived yes|no]
               Resolves DIR/inbox/moneywiz-edits.toml (and with --fixes the
               wrong amounts and opening balances a sync finds) to exact
               MoneyWiz rows. Writes DIR/data/moneywiz-plan.json for the
               Core Data writer and prints counts. --archived archives the
               account NAME, or shows it again (no).
  moneywiz verify-edits [--db FILE]
               Checks that the MoneyWiz database (or FILE, a test copy)
               holds every change in DIR/data/moneywiz-plan.json.
  moneywiz coverage [--connectors DIR]
               The share of MoneyWiz rows (not cash) that the inbox exports
               match, per account, with the gap of each: counts only.
  moneywiz db  Prints the path of the MoneyWiz database.
  moneywiz categories [list|json|suggest|new [--name N] [--ops FILE]|plan [--rollback]|commit [--backup DIR]]
               list: the category tree with types and row counts, and the
               migrations in DIR/inbox/moneywiz-migrations/. suggest: writes
               proposed changes as the next migration file. new: an empty
               migration for the current tree. plan: the next pending
               migration (or the rollback of the last applied one) as
               DIR/data/moneywiz-plan.json, refused when the tree drifted.
               commit: records it in DIR/data/moneywiz-migrations.json after
               a verified write.
  columns      Prints each inbox file's header names and the shape of its
               first lines (date, num, empty, masked text). No values.
  invest-plan --input FILE
               Ranks investment moves from FILE (aggregates the assistant
               writes: values per account, cost bases, room, rates, evidence
               facts, overrides). Prints JSON with the Markdown plan.
  transfers --balance ID=AMOUNT ...
               Plans the e-Transfers that bring each bank in DIR/profile.toml
               to its keep amount (plus its card's paydown), at most the daily
               limit per sending account, in amounts quick to type.

  cards --catalog FILE [--balance CARD=AMOUNT ...] [--pending CARD=TASK ...]
               Which card to use now per spending category: cap used so far
               in each card's cap period (from the inbox rows), cap left and
               its reset date, rent charges counted, and credit left.

  charges --merchants FILE [find WORD]
               Unexpected charges across every inbox account: duplicate
               subscriptions, new recurring charges (last 60 days), price
               rises over 10% and charges after a pause. With find WORD,
               every charge series of the merchants matching WORD, per
               account. FILE is knowledge/data/merchants.toml (aliases).
               Names merchants and amounts: for the owner only.

  net-total --input FILE
               The net total (all you have minus all you owe) from the
               aggregate items in FILE, its trend (saved one point a day in
               DIR/data/net-total-history.json), and the options in FILE
               ranked by their effect on it. Options that break a boundary in
               [boundaries] of DIR/profile.toml are refused, with the reason.
               Prints JSON with the Markdown report.

DIR defaults to $CASHCAST_HOME, then ./private. Settings come from
DIR/cashcast.toml when it exists.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("cli") {
        std::process::exit(cashcast_engine::core::cli::run(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("proof") {
        std::process::exit(cashcast_engine::proof::main(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("health") {
        std::process::exit(cashcast_engine::core::health::main());
    }
    if args.first().map(String::as_str) == Some("forecast-run") {
        std::process::exit(cashcast_engine::forecast::main(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("strategy-sections") {
        std::process::exit(cashcast_engine::strategy::sections_main(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("web-get") {
        std::process::exit(web_get(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("xlsx-cells") {
        let path = args.get(1).ok_or("usage: xlsx-cells FILE")?;
        print!(
            "{}",
            cashcast_engine::forecast::book::dump_xlsx(std::path::Path::new(path))?
        );
        return Ok(());
    }
    let Some(command) = args.first() else {
        println!("{USAGE}");
        return Ok(());
    };
    let flag = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    let home = PathBuf::from(
        flag("--home")
            .or_else(|| std::env::var("CASHCAST_HOME").ok())
            .unwrap_or_else(|| "private".into()),
    );
    let cfg = load_config(&home.join("cashcast.toml"))?;
    let inbox_dir = home.join("inbox");

    match command.as_str() {
        "import" => {
            let today = match flag("--today") {
                Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .map_err(|_| "--today must be YYYY-MM-DD")?,
                None => chrono::Local::now().date_naive(),
            };
            let inbox = inbox::load_dir(&inbox_dir, &cfg)?;
            let summary = summarize::summarize(&inbox, &cfg, today)?;
            let data = home.join("data");
            std::fs::create_dir_all(&data)
                .map_err(|e| format!("cannot create {}: {e}", data.display()))?;
            let json = serde_json::to_string_pretty(&summary).map_err(|e| format!("{e:?}"))?;
            write_private(&data.join("summary.json"), &json)?;
            write_private(
                &data.join("review.md"),
                &summarize::render_review(&inbox, &summary),
            )?;
            print!("{}", summarize::render(&summary));
            println!(
                "\nWrote data/summary.json. data/review.md is for the owner only; agents must not open it."
            );
        }
        "holdings" => {
            let holdings = ledger::load_dir(&inbox_dir, &cfg)?;
            let data = home.join("data");
            std::fs::create_dir_all(&data)
                .map_err(|e| format!("cannot create {}: {e}", data.display()))?;
            let json = serde_json::to_string_pretty(&holdings).map_err(|e| format!("{e:?}"))?;
            write_private(&data.join("holdings.json"), &json)?;
            print!("{}", ledger::render(&holdings));
            println!("\nWrote data/holdings.json.");
        }
        "totals" => {
            let inbox = inbox::load_dir(&inbox_dir, &cfg)?;
            print!("{}", inspect::render_totals(&inspect::totals(&inbox)));
        }
        "columns" => {
            for path in inbox::inbox_files(&inbox_dir)? {
                print!("{}", ledger::columns(&path));
            }
        }
        "first-error" => match inbox::first_problem(&inbox_dir, &cfg)? {
            None => println!("No unreadable rows."),
            Some(inbox::FirstProblem::File { file, error }) => {
                println!("{file}: the file cannot be read: {error}")
            }
            Some(inbox::FirstProblem::Row { file, row }) => {
                println!(
                    "{file}: {} records read OK. First error on line {}: {}",
                    row.records_before, row.line, row.reason
                );
                for (column, value) in &row.fields {
                    println!("  {column}: {value:?}");
                }
            }
        },
        "invest-plan" => {
            let path = flag("--input").ok_or("invest-plan needs --input FILE")?;
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
            println!("{}", invest_plan::run(&text)?);
        }
        "transfers" => {
            let path = home.join("profile.toml");
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let profile = transfers::parse(&text)?;
            let mut balances = std::collections::BTreeMap::new();
            for w in args.windows(2).filter(|w| w[0] == "--balance") {
                let (id, amount) = w[1]
                    .split_once('=')
                    .ok_or("--balance takes ID=AMOUNT, like td-all-inclusive=1234.56")?;
                let cents = money::parse_cents(amount, '.')
                    .ok_or_else(|| format!("--balance {id}: not an amount"))?;
                balances.insert(id.to_string(), cents);
            }
            if balances.is_empty() {
                return Err("give each balance you read: --balance ID=AMOUNT (ids: your profile's [[banking.accounts]] and [[wallet.cards]])".into());
            }
            let plan = transfers::plan(&profile, &balances)?;
            print!("{}", transfers::render(&profile, &balances, &plan));
        }
        "net-total" => {
            let input_path =
                flag("--input").ok_or("--input FILE (data/net-total-input.json) is required")?;
            let read = |p: &Path| {
                std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))
            };
            let input = read(Path::new(&input_path))?;
            let profile = read(&home.join("profile.toml"))?;
            let data = home.join("data");
            let history_path = data.join("net-total-history.json");
            let history = std::fs::read_to_string(&history_path).ok();
            let (out, history) = net_total::run(&input, &profile, history.as_deref())?;
            std::fs::create_dir_all(&data)
                .map_err(|e| format!("cannot create {}: {e}", data.display()))?;
            write_private(&history_path, &history)?;
            println!("{out}");
        }
        "charges" => {
            let today = match flag("--today") {
                Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .map_err(|_| "--today must be YYYY-MM-DD")?,
                None => chrono::Local::now().date_naive(),
            };
            let path = flag("--merchants")
                .ok_or("--merchants FILE (knowledge/data/merchants.toml) is required")?;
            let aliases = charges::parse_aliases(
                &std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?,
            )?;
            let inbox = inbox::load_dir(&inbox_dir, &cfg)?;
            let all = charges::series(&inbox.txns, &aliases);
            match args.iter().position(|a| a == "find") {
                Some(i) => {
                    let word = args.get(i + 1).ok_or("charges find WORD needs a word")?;
                    print!("{}", charges::render_find(&all, word));
                }
                None => {
                    let found = charges::findings(&all, today);
                    print!("{}", charges::render(&all, &found, today));
                }
            }
        }
        "cards" => {
            let today = match flag("--today") {
                Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .map_err(|_| "--today must be YYYY-MM-DD")?,
                None => chrono::Local::now().date_naive(),
            };
            let read = |p: &Path| {
                std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))
            };
            let catalog = read(Path::new(
                &flag("--catalog")
                    .ok_or("--catalog FILE (knowledge/data/credit_cards.toml) is required")?,
            ))?;
            let own = read(&home.join("knowledge").join("credit_cards.toml")).ok();
            let wallet =
                cards::parse(&read(&home.join("profile.toml"))?, &catalog, own.as_deref())?;
            if wallet.cards.is_empty() {
                return Err("no cards in [[wallet.cards]] of profile.toml".into());
            }
            let pairs = |name: &str| {
                args.windows(2)
                    .filter(|w| w[0] == name)
                    .map(|w| {
                        w[1].split_once('=')
                            .map(|(a, b)| (a.to_string(), b.to_string()))
                            .ok_or_else(|| format!("{name} takes CARD=VALUE"))
                    })
                    .collect::<Result<Vec<_>, String>>()
            };
            let mut balances = std::collections::BTreeMap::new();
            for (id, amount) in pairs("--balance")? {
                if !wallet.cards.iter().any(|c| c.id == id) {
                    return Err(format!("{id} is not a card in [[wallet.cards]]"));
                }
                let cents = money::parse_cents(&amount, '.')
                    .ok_or_else(|| format!("--balance {id}: not an amount"))?;
                balances.insert(id, cents);
            }
            let mut pending: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            for (id, task) in pairs("--pending")? {
                pending.entry(id).or_default().push(task);
            }
            let inbox = inbox::load_dir(&inbox_dir, &cfg)?;
            let rows = summarize::summarize(&inbox, &cfg, today)?.charges;
            let report = cards::now(&wallet, &rows, &balances, &pending, today);
            if args.iter().any(|a| a == "missed") {
                let since = match flag("--since") {
                    Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                        .map_err(|_| "--since must be YYYY-MM-DD")?,
                    None => today - chrono::Days::new(7),
                };
                // MoneyWiz holds this week's card rows with their categories once
                // the sync ran; the forecast import ends at its last full month.
                let mw_rows = moneywiz::find_db(cfg.moneywiz.as_ref())
                    .and_then(|db| moneywiz::open(&db))
                    .map(|store| {
                        let links = cfg.moneywiz.as_ref().map(|m| m.accounts.clone());
                        cards::moneywiz_rows(&wallet, &store, &links.unwrap_or_default())
                    });
                let (rows, from) = match mw_rows {
                    Ok(r) if !r.is_empty() => (r, "MoneyWiz"),
                    Ok(_) => (
                        rows,
                        "the imported exports (no MoneyWiz rows on these cards)",
                    ),
                    Err(e) => (
                        rows,
                        if e.is_empty() {
                            "the imported exports"
                        } else {
                            "the imported exports (MoneyWiz not read)"
                        },
                    ),
                };
                let report = cards::now(&wallet, &rows, &balances, &pending, today);
                let (found, checked) = cards::missed(&wallet, &report, &rows, since);
                print!(
                    "{}",
                    cards::render_missed(&wallet, &found, &checked, since, today)
                );
                println!("\nRows read from {from}.");
            } else {
                print!("{}", cards::render(&wallet, &report));
            }
        }
        "moneywiz" => {
            let today = match flag("--today") {
                Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .map_err(|_| "--today must be YYYY-MM-DD")?,
                None => chrono::Local::now().date_naive(),
            };
            let db = moneywiz::find_db(cfg.moneywiz.as_ref())?;
            match args.get(1).map(String::as_str) {
                Some("status") => {
                    let store = moneywiz::open(&db)?;
                    let exports = inbox::load_exports(&inbox_dir, &cfg).ok();
                    print!(
                        "{}",
                        moneywiz::render_status(&store, exports.as_ref(), cfg.moneywiz.as_ref())
                    );
                }
                Some("sync") => {
                    let exports = inbox::load_exports(&inbox_dir, &cfg)?;
                    let apply = args.iter().any(|a| a == "--apply");
                    let resend = args.iter().any(|a| a == "--resend");
                    let result = moneywiz::sync(&db, &exports, &cfg, &home, today, apply, resend)?;
                    let out = home.join("out");
                    std::fs::create_dir_all(&out)
                        .map_err(|e| format!("cannot create {}: {e}", out.display()))?;
                    if !result.fixes_md.is_empty() {
                        write_private(&out.join("moneywiz-fixes.md"), &result.fixes_md)?;
                    }
                    print!("{}", result.report);
                }
                Some("resync") => {
                    let day = |name: &str| -> Result<Option<chrono::NaiveDate>, String> {
                        flag(name)
                            .map(|d| {
                                chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                                    .map_err(|_| format!("{name} must be YYYY-MM-DD"))
                            })
                            .transpose()
                    };
                    let store = moneywiz::open(&db)?;
                    let exports = inbox::load_exports(&inbox_dir, &cfg)?;
                    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref())?;
                    // A MoneyWiz account name stands for its export account.
                    let account = flag("--account").map(|a| {
                        plan.accounts
                            .iter()
                            .find(|ap| store.account(ap.mw).is_some_and(|m| m.name == a))
                            .map(|ap| ap.export_account.clone())
                            .unwrap_or(a)
                    });
                    print!(
                        "{}",
                        moneywiz_ledger::ask_resync(
                            &home.join("data"),
                            account,
                            day("--from")?,
                            day("--to")?,
                            today
                        )?
                    );
                }
                Some("diagnose") => {
                    let store = moneywiz::open(&db)?;
                    let exports = inbox::load_exports(&inbox_dir, &cfg)?;
                    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref())?;
                    print!("{}", moneywiz_diagnose::render(&store, &exports, &plan));
                    if let Ok(h) = ledger::load_dir(&inbox_dir, &cfg) {
                        use cashcast_engine::moneywiz_invest as inv;
                        let empty = Default::default();
                        let map = cfg.moneywiz.as_ref().map_or(&empty, |m| &m.investments);
                        let empty_h = Default::default();
                        let held = cfg.moneywiz.as_ref().map_or(&empty_h, |m| &m.holdings);
                        let (adjust, problems) = inv::holding_adjustments(&h.trades, map, held);
                        let mut trades = h.trades.clone();
                        trades.extend(adjust);
                        let snaps = cashcast_engine::pdf::sunlife::snapshots(&inbox_dir);
                        let lot_problems = inv::statement_lots(&store, &mut trades, &snaps, map);
                        for p in problems.iter().chain(&lot_problems) {
                            println!("- {p}");
                        }
                        let rows = inv::compare(&store, &trades, map);
                        print!("{}", inv::render(&rows));
                        print!(
                            "{}",
                            inv::render_statement_checks(&inv::statement_checks(
                                &store, &snaps, map
                            ))
                        );
                        if let Some(m) = cfg.moneywiz.as_ref() {
                            let user = cashcast_engine::core::paths::resolve(None)
                                .map_err(|e| format!("{e:?}"))?;
                            let ok = cashcast_engine::core::consent::allowed(
                                &user,
                                "net:market-data",
                                today,
                            )
                            .unwrap_or(false);
                            if ok {
                                let mut tracked: Vec<String> = plan
                                    .accounts
                                    .iter()
                                    .filter_map(|ap| store.account(ap.mw).map(|a| a.name.clone()))
                                    .chain(m.investments.keys().cloned())
                                    .chain(m.balances.keys().map(|k| k.trim().to_string()))
                                    .collect();
                                tracked.sort();
                                tracked.dedup();
                                let mut price = |t: &str| {
                                    cashcast_engine::api::yahoo_price(&user, t, today)
                                        .ok()
                                        .flatten()
                                };
                                let r = cashcast_engine::moneywiz_networth::report(
                                    &store,
                                    &plan.accounts,
                                    &rows,
                                    &tracked,
                                    &m.tickers,
                                    &mut price,
                                );
                                print!("{}", cashcast_engine::moneywiz_networth::render(&r));
                            } else {
                                println!(
                                    "\nNet worth at market prices needs consent `net:market-data` (only ticker symbols leave this Mac): run `bin/cashcast consent grant net:market-data` in Terminal."
                                );
                            }
                        }
                        print!("{}", inv::render_unmapped(&inv::unmapped(&h.trades, map)));
                        print!("{}", inv::render_ambiguous(&inv::ambiguous(&h.trades, map)));
                    }
                }
                Some("categorize") => {
                    let store = moneywiz::open(&db)?;
                    let exports = inbox::load_exports(&inbox_dir, &cfg)?;
                    let plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref())?;
                    let data = home.join("data");
                    let mut record = moneywiz_categorize::load_record(&data);
                    // Seeds the record from a backup made before an earlier run.
                    if let Some(i) = args.iter().position(|a| a == "--record-since") {
                        let dir = args
                            .get(i + 1)
                            .ok_or("--record-since needs a backup folder")?;
                        let before = std::fs::read_dir(dir)
                            .map_err(|e| format!("cannot read {dir}: {e}"))?
                            .flatten()
                            .map(|e| e.path())
                            .find(|p| p.extension().is_some_and(|x| x == "sqlite"))
                            .ok_or("no .sqlite file in that backup folder")?;
                        let found =
                            moneywiz_categorize::record_since(&moneywiz::open(&before)?, &store);
                        let n = found.len();
                        record.extend(found);
                        moneywiz_categorize::save_record(&data, &record)?;
                        println!(
                            "Recorded {n} rows cashcast categorized since that backup ({} in all).",
                            record.len()
                        );
                        return Ok(());
                    }
                    let (picks, left) =
                        moneywiz_categorize::picks_with(&store, &exports, &plan, &record);
                    if args.iter().any(|a| a == "--check") {
                        let ([(pn, pr), (gn, gr)], misses) =
                            moneywiz_categorize::check_misses(&store, &exports, &plan, &record);
                        let pct = |r: usize, n: usize| if n == 0 { 0 } else { r * 100 / n };
                        println!(
                            "# MoneyWiz categorize check\n\nEvery 5th row you categorized was hidden and predicted from the rest.\n\n| Source | Rows | Same as yours |\n|---|---:|---:|\n| Your rows for the same payee | {pn} | {}% |\n| Engine guess | {gn} | {}% |",
                            pct(pr, pn),
                            pct(gr, gn)
                        );
                        let (with, need) = moneywiz::category_coverage(&store, &plan);
                        println!(
                            "\nSynced rows with a category: {with} of {need} ({}%).",
                            pct(with, need)
                        );
                        let changed = picks.iter().filter(|p| !p.row.category.is_empty()).count();
                        println!(
                            "Rows cashcast categorized: {} ({} the rule still picks, {changed} it would change, {} it is unsure of now).",
                            left.agree + changed + left.unsure,
                            left.agree,
                            left.unsure
                        );
                        if !misses.is_empty() {
                            println!(
                                "\nMost common misses:\n\n| Your category | Picked | Rows |\n|---|---|---:|"
                            );
                            for (yours, picked, n) in misses {
                                println!("| {yours} | {picked} | {n} |");
                            }
                        }
                        return Ok(());
                    }
                    let written = if args.iter().any(|a| a == "--dry-run") {
                        None
                    } else {
                        let r = moneywiz_categorize::write_edits_into(
                            &store,
                            &picks,
                            &inbox_dir.join(moneywiz_edit::EDITS_FILE),
                            Some(&mut record),
                        )?;
                        moneywiz_categorize::save_record(&data, &record)?;
                        Some(r)
                    };
                    print!("{}", moneywiz_categorize::render(&picks, &left, written));
                    if args.iter().any(|a| a == "--list") {
                        print!("{}", moneywiz_categorize::render_picks(&store, &picks));
                        print!("{}", moneywiz_categorize::render_left(&store, &left));
                    }
                }
                Some("coverage") => {
                    let store = moneywiz::open(&db)?;
                    let exports = inbox::load_exports(&inbox_dir, &cfg)?;
                    let dir = flag("--connectors").map(PathBuf::from).unwrap_or_else(|| {
                        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../connectors"))
                    });
                    let connectors = moneywiz_coverage::load_connectors(&dir);
                    let mut c = moneywiz_coverage::coverage(
                        &store,
                        &exports,
                        cfg.moneywiz.as_ref(),
                        &connectors,
                        today,
                    )?;
                    moneywiz_coverage::add_trades(&mut c, &store, &inbox_dir, &cfg, today);
                    print!("{}", moneywiz_coverage::render(&c, today));
                }
                // Rows with their bank text: only for the owner's web app, which passes
                // its access token as CASHCAST_OWNER_UI. Agents and the CLI get the
                // counts-only reports.
                Some("sync-view") => {
                    let given = std::env::var("CASHCAST_OWNER_UI").unwrap_or_default();
                    // The token is checked against private_root()/out/web.json, which
                    // CASHCAST_PRIVATE moves: the home must sit under that same root, so a
                    // forged root cannot unlock a real user's rows.
                    let in_root = std::fs::canonicalize(&home).ok().is_some_and(|h| {
                        std::fs::canonicalize(cashcast_engine::core::paths::private_root())
                            .is_ok_and(|r| h.starts_with(r))
                    });
                    if !in_root || !cashcast_engine::tasks::start::owner_token_ok(&given) {
                        return Err("sync-view is for the web app only (it shows bank text); use moneywiz sync --dry-run".into());
                    }
                    let exports = inbox::load_exports(&inbox_dir, &cfg)?;
                    let dir = flag("--connectors").map(PathBuf::from).unwrap_or_else(|| {
                        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../connectors"))
                    });
                    let connectors = moneywiz_coverage::load_connectors(&dir);
                    let v = cashcast_engine::moneywiz_view::view(
                        &db,
                        &exports,
                        &cfg,
                        &home,
                        &connectors,
                        today,
                    )?;
                    println!("{v}");
                }
                Some("db") => println!("{}", db.display()),
                Some("categories") => {
                    use cashcast_engine::moneywiz_categories;
                    let store = moneywiz::open(&db)?;
                    let out = match args.get(2).map(String::as_str) {
                        None | Some("list") => moneywiz_categories::render_list(&store, &home)?,
                        Some("suggest") => moneywiz_categories::write_suggestions(
                            &store,
                            &home,
                            &today.to_string(),
                        )?,
                        Some("json") => moneywiz_categories::view_json(&store, &home)? + "\n",
                        Some("new") => match flag("--ops") {
                            Some(file) => {
                                let text = std::fs::read_to_string(&file)
                                    .map_err(|e| format!("cannot read {file}: {e}"))?;
                                let ops: Vec<moneywiz_categories::Op> = serde_json::from_str(&text)
                                    .map_err(|e| {
                                        format!("the picked changes are not valid: {e}")
                                    })?;
                                moneywiz_categories::write_ops(
                                    &store,
                                    &home,
                                    flag("--name").as_deref(),
                                    &ops,
                                )?
                            }
                            None => moneywiz_categories::new_file(
                                &store,
                                &home,
                                flag("--name").as_deref(),
                            )?,
                        },
                        Some("plan") => moneywiz_categories::plan(
                            &store,
                            &home,
                            args.iter().any(|a| a == "--rollback"),
                        )?,
                        Some("commit") => {
                            moneywiz_categories::commit(&store, &home, flag("--backup"))?
                        }
                        _ => USAGE.to_string() + "\n",
                    };
                    print!("{out}");
                }
                Some("edits") => {
                    let store = moneywiz::open(&db)?;
                    // CASHCAST_MONEYWIZ_EDITS: an edits file outside the inbox, such as the
                    // owner's answers an agent wrote down (agents never write the inbox).
                    let edits_path = std::env::var_os("CASHCAST_MONEYWIZ_EDITS")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| inbox_dir.join(moneywiz_edit::EDITS_FILE));
                    let file = moneywiz_edit::load(&edits_path)?;
                    let exports;
                    let sync_plan;
                    let fixes = if args.iter().any(|a| a == "--fixes") {
                        exports = inbox::load_exports(&inbox_dir, &cfg)?;
                        let mut plan = moneywiz::plan(&store, &exports, cfg.moneywiz.as_ref())?;
                        moneywiz::add_pending_trades(&mut plan, &store, &exports, &cfg, &home);
                        // Held accounts (the sync gate) get no edits either.
                        let mode = moneywiz::mode(cfg.moneywiz.as_ref())?;
                        let amounts = moneywiz::corrected_amounts(&store);
                        plan.accounts.retain(|ap| {
                            let reason =
                                moneywiz_ledger::hold_reason(&store, &exports, ap, &amounts, mode);
                            if let Some(r) = &reason {
                                eprintln!("Held, no edits: {}: {r}", ap.export_account);
                            }
                            reason.is_none()
                        });
                        sync_plan = plan;
                        Some((exports.txns.as_slice(), &sync_plan))
                    } else {
                        None
                    };
                    let mut resolved = moneywiz_edit::resolve(&store, &file, fixes);
                    if args.iter().any(|a| a == "--fixes") {
                        if let (Ok(h), Some(m)) =
                            (ledger::load_dir(&inbox_dir, &cfg), cfg.moneywiz.as_ref())
                        {
                            use cashcast_engine::moneywiz_invest as inv;
                            let (adjust, held_problems) =
                                inv::holding_adjustments(&h.trades, &m.investments, &m.holdings);
                            let mut trades = h.trades.clone();
                            trades.extend(adjust);
                            resolved.problems.extend(held_problems);
                            let snaps = cashcast_engine::pdf::sunlife::snapshots(&inbox_dir);
                            resolved.problems.extend(inv::statement_lots(
                                &store,
                                &mut trades,
                                &snaps,
                                &m.investments,
                            ));
                            // Opening shares go to 0 (the broker file holds every
                            // trade), so the trade fixes count the rows alone.
                            let mut store = store.clone();
                            let held = std::mem::take(&mut store.holdings);
                            let rows = inv::compare(&store, &trades, &m.investments);
                            // Market prices (consent `net:market-data`): MoneyWiz
                            // values a holding at its own price per share.
                            let user = cashcast_engine::core::paths::resolve(None).ok();
                            let quotes = user.as_ref().is_some_and(|u| {
                                cashcast_engine::core::consent::allowed(u, "net:market-data", today)
                                    .unwrap_or(false)
                            });
                            let mut price = |t: &str| {
                                user.as_ref().and_then(|u| {
                                    cashcast_engine::api::yahoo_price(u, t, today)
                                        .ok()
                                        .flatten()
                                })
                            };
                            resolved.plan.holdings = held
                                .iter()
                                .filter_map(|o| {
                                    let a = store.accounts.iter().find(|a| a.pk == o.account)?;
                                    let mapped = rows.iter().any(|r| r.account == a.name);
                                    if !mapped && !m.merge.contains_key(&a.name) {
                                        return None;
                                    }
                                    let market = (quotes && mapped)
                                        .then(|| {
                                            cashcast_engine::moneywiz_networth::market_price(
                                                &o.symbol,
                                                &a.currency,
                                                &m.tickers,
                                                &mut price,
                                            )
                                        })
                                        .flatten()
                                        // No quote: the broker's last trade, as diagnose
                                        // values it.
                                        .or_else(|| {
                                            rows.iter()
                                                .find(|r| {
                                                    r.account == a.name
                                                        && inv::norm(&r.symbol)
                                                            == inv::norm(&o.symbol)
                                                })
                                                .and_then(|r| r.last_price.map(|(_, p)| p))
                                        })
                                        .filter(|p| (p - o.price).abs() > p * 1e-3);
                                    (o.shares != 0.0 || market.is_some()).then(|| {
                                        moneywiz_edit::HoldingEdit {
                                            gid: o.gid.clone(),
                                            expect_opening: o.shares,
                                            opening: 0.0,
                                            price: market,
                                            expect_price: o.price,
                                        }
                                    })
                                })
                                .collect();
                            let (adds, deletes) =
                                inv::trade_fixes(&store, &rows, m.bank_wins.unwrap_or(false));
                            resolved.plan.trades = adds;
                            resolved.plan.rows.extend(deletes);
                        }
                        if let Some(m) = cfg.moneywiz.as_ref() {
                            let taken: std::collections::HashSet<String> =
                                resolved.plan.rows.iter().map(|e| e.gid.clone()).collect();
                            let (moves, problems) = moneywiz_edit::merge_edits(
                                &store,
                                &m.merge,
                                &m.investments,
                                &taken,
                            );
                            resolved.plan.rows.extend(moves);
                            resolved.problems.extend(problems);
                            let taken: std::collections::HashSet<String> =
                                resolved.plan.rows.iter().map(|e| e.gid.clone()).collect();
                            resolved
                                .plan
                                .rows
                                .extend(moneywiz_edit::retire_edits(&store, &m.retire, &taken));
                            let problems = moneywiz_edit::balance_edits(
                                &store,
                                &m.balances,
                                &mut resolved.plan,
                            );
                            resolved.problems.extend(problems);
                        }
                    }
                    // `--account NAME --archived yes|no`: archive the account,
                    // or show it again so the sync can add rows there.
                    if let Some(v) = flag("--archived") {
                        let name = flag("--account").ok_or("--archived needs --account NAME")?;
                        let archived = match v.as_str() {
                            "yes" => true,
                            "no" => false,
                            _ => return Err("--archived takes yes or no".into()),
                        };
                        match moneywiz_edit::archive_edit(&store, &name, archived) {
                            Ok(e) => resolved.plan.archive.push(e),
                            Err(p) => resolved.problems.push(p),
                        }
                    }
                    let data = home.join("data");
                    std::fs::create_dir_all(&data)
                        .map_err(|e| format!("cannot create {}: {e}", data.display()))?;
                    let json = serde_json::to_string_pretty(&resolved.plan)
                        .map_err(|e| format!("{e:?}"))?;
                    write_private(&data.join(moneywiz_edit::PLAN_FILE), &json)?;
                    print!(
                        "{}",
                        moneywiz_edit::render(
                            &resolved,
                            &format!("data/{}", moneywiz_edit::PLAN_FILE)
                        )
                    );
                }
                Some("verify-edits") => {
                    let target = flag("--db").map(PathBuf::from).unwrap_or(db);
                    let text =
                        std::fs::read_to_string(home.join("data").join(moneywiz_edit::PLAN_FILE))
                            .map_err(|_| "no edit plan: run `moneywiz edits` first")?;
                    let plan: moneywiz_edit::WriterPlan = serde_json::from_str(&text)
                        .map_err(|e| format!("the edit plan is damaged: {e}"))?;
                    let (report, missing) = moneywiz_edit::verify(&moneywiz::open(&target)?, &plan);
                    print!("{report}");
                    if missing > 0 {
                        return Err(format!("{missing} changes are not in the database"));
                    }
                }
                _ => println!("{USAGE}"),
            }
        }
        _ => println!("{USAGE}"),
    }
    Ok(())
}

fn load_config(path: &Path) -> Result<config::Config, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => config::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(config::Config::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// `web-get PATH --today D`: one GET through the web handler, as the parity
/// check (`scripts/rust-parity.sh`) compares it with its goldens. The token is the running
/// server's (from `private/out/web.json`), so owner-only routes answer.
fn web_get(args: &[String]) -> i32 {
    let flag = |n: &str| args.windows(2).find(|w| w[0] == n).map(|w| w[1].clone());
    let (Some(path), Some(today)) = (args.first(), flag("--today")) else {
        return 2;
    };
    let Ok(today) = chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d") else {
        return 2;
    };
    let token = flag("--token").unwrap_or_default();
    let headers = vec![
        ("Host".to_string(), "127.0.0.1:1".to_string()),
        ("X-Cashcast-Token".to_string(), token.clone()),
    ];
    let (status, ctype, body) =
        cashcast_engine::web::handle("GET", path, &headers, &token, 1, today, b"");
    println!("{status} {ctype}");
    println!("{}", String::from_utf8_lossy(&body));
    0
}
