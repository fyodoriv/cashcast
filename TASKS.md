# Tasks

Format: [tasks.md](https://github.com/tasksmd/tasks.md). Remove a task block when it is done.
Every task carries Hypothesis, Measurement, Success, Pivot, and Anchor before work starts.
Privacy: never put amounts, balances, account names, payees or employer names in a task.
Every new tracked path needs a `!` line in the allowlist `.gitignore`.

## P1

- [ ] A sent link MoneyWiz never saved is sent again, even after the plan changed in between
  - **ID**: moneywiz-sent-marked-saved
  - **Tags**: moneywiz, bug, sync
  - **Details**: Found 2026-11-16. `SentList::mark_saved(&links)` (engine/src/moneywiz.rs, at
    load and after each confirm) marks every sent key that is not among the links missing now as
    saved. A balance correction's id holds its gap, so when an inbox change moves the gap for one
    run, the earlier key drops out and is marked saved although MoneyWiz never kept it. When the
    gap comes back, the link reads "sent today already", and from the next day "sent on an earlier
    day … deleted there?": a plain sync never sends it again. On the owner's data a TFSA balance
    correction (owner chose bank wins, 2026-11-16) is stuck this way: run `bin/cashcast moneywiz
    sync --resend` on or after 2026-11-17 (MoneyWiz quit first, then dry-run until the TFSA row
    reads "matches the bank"). Weekly step 2b also does this.
  - **Files**: `engine/src/moneywiz.rs` (`SentList`, `mark_saved`, `sync`), `engine/tests/moneywiz.rs`
  - **Acceptance**: a failing synthetic test first: send a balance link with `fake_app` that does
    not save it, change the inbox so the gap moves for one apply run, restore it; the next day's
    plain sync sends the link again. Rows MoneyWiz did save and the owner deleted still stay "gone".
  - **Hypothesis**: links stuck as "saved" while MoneyWiz lacks them fall from 1 (owner data,
    2026-11-16) to 0, with no row sent twice.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test moneywiz sent_` and,
    on the owner's data, `bin/cashcast moneywiz sync --dry-run` ("sent on an earlier day" count)
  - **Success**: the new test passes and the dry run lists 0 "sent on an earlier day" links that
    MoneyWiz never held
  - **Pivot**: if telling a moved gap from a deletion needs the link's amount, key a second hash
    on the link family (kind and account) instead of patching `mark_saved`
  - **Anchor**: Gray & Reuter 1993, *Transaction Processing*, ch. 10 (idempotent resend after
    an unacknowledged write)

- [ ] The forecast's spending and opening cash match the owner's real accounts
  - **ID**: forecast-spending-double-count
  - **Tags**: forecast, bug, moneywiz
  - **Details**: Found 2026-11-15 by the weekly MoneyWiz sync. After the sync, `bin/cashcast
    forecast` sets monthly expenses well above the owner's `baseline` floor (the last-12-month
    average is about 2.7 times it), so month-end cash falls below zero in every forecast month,
    while `moneywiz diagnose` shows every bank and card account matching the bank. The opening
    balance comes from one Wealthsimple statement file ("Opening balance of
    Chequing-monthly-statement-… comes from the export"). The import also stops at Sep 2026
    ("Jan 2025 to Sep 2026 imported"): `import` reads the MoneyWiz report CSV in the inbox,
    which ends 2026-10-01, and with a MoneyWiz CSV there it skips the live MoneyWiz database
    (playbook `export-csv` step 4). Likely causes of the high spending: the dated Wealthsimple
    downloads (monthly statement, `credit-card-activities-*`) are new accounts to the forecast
    and count again beside the MoneyWiz rows, or transfers to investment and crypto accounts
    count as spending. First check: does `import` read the database once the report CSV is
    older than the newest bank export? 2026-11-16: with every dated Scotia, TD, CIBC and
    Wealthsimple download marked `excluded` (prefix keys), the WAPE fell to 54.6 %.
    2026-11-22 backtest (Jan 2025 to Oct 2026): each line alone misses by about a quarter
    (net salary 24.3 %, extra income 26.2 %, spending excl. rent 23.2 %), so rows no longer
    count twice; net cash flow is their difference and misses by 69.2 %. The biggest misses
    are lumpy Travel and Uncategorized spending, and RSU/ESPP and "other" income that come in
    some months only. The salary floor (`[pay] min_salary`) moves rows between salary and
    other income, so it does not change net cash flow. Next lever: forecast lumpy categories
    and stock-plan income by month of year, not as a flat average.
    2026-11-22 evening: `backtest` now scores every method ("Net cash flow WAPE by method"):
    same month last year 66.4 %, trailing-12 average 69.2 %, median 74.4 %, flat baseline
    75.5 %. Two blends were tried and dropped (pre-registered pivot: none beat 66.4 % by 3
    points): half average + half last 3 months 69.0 %, half average + half same month 66.4 %.
    So the spending method is not the lever. The largest misses by calendar month are in
    extra income (March, September and October, each a month's salary or more): RSU and ESPP money counts only when it
    moves from the stock plan account to cash, and that month varies. Next: model stock-plan
    income at the month it reaches cash (or keep it out of net cash flow until it does).
    Owner, 2026-11-22: from now on every new vest moves to cash as soon as it lands, so the
    vest months in `[extra_income]` are the cash months going forward. Past misses stay in the
    backtest; recheck the WAPE after the next vests (Dec 2026, Jan 2027).
    Judge panel, 2026-11-22 night: rent paid ahead by a card service now counts in the month it
    is due (`[rent] paid_ahead_from_day`). Same history, fresh import: net cash flow WAPE 69.2 %
    to 58.2 % (best method 66.4 % to 57.5 %), Rent 50.3 % to 16.1 %, spending unchanged at
    23.2 %. Mean absolute monthly miss fell 9.5 %. Month end (cumulative) rose 61.3 % to 69.7 %:
    the double and empty rent months had cancelled out, and the forecast's optimistic lean now
    shows (tasks `forecast-salary-point-in-time`, `forecast-spending-low-bias`).
    Follow-ups, 2026-11-22 late: pay known at each origin was dropped by its pivot (net salary
    24.3 % to 30.9 %, net cash flow 58.2 % to 62.4 %). The new Net salary row in "Mean miss by
    calendar month" showed the last month empty: the MoneyWiz CSV the import reads ends on the
    1st. The default window now ends at the last month most active accounts cover: net cash
    flow 58.2 % to 48.3 % (best method, same month last year, 57.5 % to 41.8 %), net salary
    24.3 % to 10.1 %, month end 69.7 % to 58.3 %, mean absolute miss down 24.5 %; spending
    23.2 % to 23.7 %; 9 origins instead of 10. A "damped trend average" method was dropped by
    its pivot (49.2 %, above 47.3 %). Next: refresh the MoneyWiz CSV export (or read the live
    database) so the newest months are in the history.
  - **Files**: `engine/src/summarize.rs` (classes, roles), `engine/src/forecast/model.rs`
    (opening cash), `engine/tests/classes.rs`, `engine/tests/forecast_model.rs`
  - **Acceptance**: a failing synthetic test first (a bank export that repeats MoneyWiz rows
    counts once); then on the owner's data `bin/cashcast forecast` lists every cash account's
    opening balance, and its 12-month spending average is within 25 % of the MoneyWiz
    spending report for the same months (owner checks the MoneyWiz side).
  - **Hypothesis**: the backtest "Net cash flow" WAPE falls from 71.6 % to 40 % or less once
    each row counts once.
  - **Measurement**: `bin/cashcast backtest` ("Net cash flow" WAPE line)
  - **Success**: WAPE 40 % or less
  - **Pivot**: if WAPE stays above 60 % after removing double counts, the trailing-12 method is
    the cause: try the seasonal method (decision 0012) instead of more de-duplication
  - **Anchor**: Hyndman & Athanasopoulos 2021, *Forecasting: Principles and Practice*, 5.8
    (scale-free accuracy measures such as WAPE/MAPE)

- [ ] The forecast follows a spending level that moved, so the monthly cash flow is 65 % accurate
  - **ID**: forecast-level-shift
  - **Tags**: forecast, accuracy
  - **Details**: Owner goal 2026-11-22: "at least reach 65% accuracy" (the Forecast view's
    monthly cash flow accuracy, 100 % minus the backtest WAPE; now 51.7 %). A new backtest
    line, "Net cash flow WAPE if one line were exact", shows where the miss comes from: an exact
    Travel forecast alone would give 29.8 % (an exact extra income line is not in the top 6).
    Travel was near zero each month until late 2025 and has been large most months since, so
    the trailing-12 average lags the new level for a year. Try methods that follow a moved
    level: the average of the last 6 months, and simple exponential smoothing.
  - **Files**: `engine/src/forecast/model.rs` (`METHODS`, `run`), `engine/src/forecast/backtest.rs`
    (`if_exact`), `engine/src/forecast/mod.rs`, `engine/tests/forecast_model.rs`
  - **Acceptance**: a failing synthetic test first (a category that jumps to a new level is
    forecast near the new level); `backtest` scores the new methods beside the old ones; the
    owner's `[forecast] method` is set to the lowest one (owner said yes, 2026-11-22).
  - **Hypothesis**: a level-following method lowers net cash flow WAPE from 48.3 % to 35 % or
    less, because Travel moved to a higher level a year ago and the 12-month average still
    mixes in the old one.
  - **Measurement**: `bin/cashcast backtest` ("Net cash flow WAPE by method" lines; guard:
    "Month end (cumulative net)" WAPE for the chosen method)
  - **Success**: net cash flow WAPE 35.0 % or lower for the best new method, with month-end
    WAPE not above 58.3 %
  - **Pivot**: above 45 %: a moved level is not the cause; look at Travel planned ahead
    (known trips) instead of history
  - **Anchor**: Hyndman & Athanasopoulos 2021, *Forecasting: Principles and Practice*, 8.1
    (simple exponential smoothing for a level that moves)
  - **Result 1** (2026-11-22): dropped by its pivot. Net cash flow WAPE 50.1 % (last 6 months)
    and 50.8 % (smoothing, 0.3), above 45 %. Spending excl. rent 23.7 % to 19.3-20.2 %, month
    end 58.3 % to 50.9-53.3 %, but an exact Travel line still gives about 32 %: what is left
    is when trips happen, not their level.
  - **Approach 2, pre-registered 2026-11-22**: a "trailing-6 median" method (the median is the
    point forecast that minimizes absolute error, so it fits WAPE; Hyndman & Athanasopoulos
    5.4). Success: net cash flow WAPE 35.0 % or lower, month end not above 58.3 %. Pivot:
    above 45 %, drop it.
  - **Approach 3, pre-registered 2026-11-22**: correct each line by its mean miss over the
    backtest months that ended before the forecast date (model output statistics, Glahn and
    Lowry 1972), scored in `backtest` as a separate line first. Success: net cash flow WAPE
    35.0 % or lower. Pivot: above 45 %, drop it; then 65 % monthly accuracy is out of reach
    from history alone, and the next step is trips planned ahead.

- [ ] The owner answers the open invest-plan questions, and the judge loop reaches 10 of 10
  - **ID**: invest-owner-answers
  - **Tags**: invest-plan, owner
  - **Details**: Asked 2026-11-09, parked by the owner. Done already: `high_interest_debt = 0`
    in `[invest]`; ETH is sold (Kraken shows none). Open:
    1. Set `target_equity = 1.0` (all stocks)? The owner wants 8-10% a year; FP Canada 2026
       planning returns give about 6.5% for all stocks and 5.8% for 80%, so no diversified mix
       expects 8-10%. Closer levers: all stocks, RRSP room, refill the reserve first.
    2. May the agent read investment market values from Wealthsimple in Chrome (read only) and
       record them with `bin/cashcast api record wealthsimple`, plus `prices_as_of`?
    Then run judge panel 17 (workflow script `invest-panel-14`, copies in the session scratchpad)
    and fix until all 10 judges say NOTHING-TO-IMPROVE. Panel 16: 8 of 10.
  - **Files**: `private/profile.toml` (`[invest]`), `engine/src/invest_plan.rs`
  - **Acceptance**: both answers in the profile; `bin/cashcast strategy --section invest-plan` prices
    every holding; a panel returns 10 NOTHING-TO-IMPROVE.
  - **Hypothesis**: judge findings fall from 2 (panel 16) to 0 within two panels.
  - **Measurement**: the panel workflow's verdict count
  - **Success**: 10 of 10 NOTHING-TO-IMPROVE
  - **Pivot**: findings stay at 2+ for three panels → change the judge brief, not the engine
  - **Anchor**: FP Canada 2026 Projection Assumption Guidelines

- [ ] `strategy --section` rejects or maps an unknown section name instead of printing nothing
  - **ID**: strategy-invest-plan-section
  - **Tags**: invest-plan, bug, cli
  - **Details**: Found 2026-11-14. `bin/cashcast strategy --section invest-plan` prints only the
    net total, the monthly review and the dated checklist, with no "Investment moves" heading.
    The section id is `invest_plan` (underscore, `engine/src/strategy/mod.rs` `PORTED`), and
    `--section invest_plan` works. `--section` accepts any text, and `plan::document`
    (`engine/src/strategy/plan.rs`) keeps only exact id matches, so an unknown or hyphenated
    name is dropped with no message. The command name `invest-plan` (hyphen) makes the mistake
    easy. The task `invest-owner-answers` uses the broken form in its Acceptance.
  - **Files**: `engine/src/core/commands.rs` (`--section` choices or a hyphen alias),
    `engine/src/core/cli.rs`, `engine/src/strategy/plan.rs`,
    `engine/tests/app_strategy_sections.rs`
  - **Acceptance**: a failing test first; then `strategy --section invest-plan` either prints
    "Investment moves: ranked, with evidence" on the fixture (hyphen alias) or exits non-zero
    and lists the valid section ids. Every id in `PORTED` still works.
  - **Hypothesis**: `--section` names that print no section and no error fall from every unknown
    name to 0.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test app_strategy_sections section_filter`
    (new test: each `PORTED` id prints its heading; `invest-plan` and a made-up name print the
    section or an error, never neither)
  - **Success**: 0 names print neither the section nor an error
  - **Pivot**: if aliases clash with existing ids, drop aliases and only reject unknown names
  - **Anchor**: Nielsen 1994, heuristic 1 (visibility of system status): a filter that silently
    shows nothing misleads the user

- [ ] The tax section phases down the federal basic personal amount for high incomes
  - **ID**: tax-bpa-phase-down
  - **Tags**: tax, strategy, bug
  - **Details**: Found 2026-11-14. `income_tax` in `engine/src/strategy/tax.rs` always uses
    `basic_personal_amount` (16,452). CRA shrinks it toward `basic_personal_amount_min` (14,829)
    as net income goes from 181,440 to 258,482, and uses 14,829 at or above 258,482
    (`knowledge/data/tax_ca_2026.toml` already holds both). So above 181,440 cashcast's federal
    tax is too low: by up to about $227 (1,623 × 14%) at and above 258,482. Inside the band its
    marginal rate is also about 0.29 points too low, which skews the RRSP refund and
    capital-gain numbers it prints. Not covered by this task: Yukon mirrors the federal
    phase-down, and Manitoba's credit phases out between 200,000 and 400,000 (same file).
  - **Files**: `engine/src/strategy/tax.rs`, `engine/tests/app_strategy_math.rs`, parity goldens
    that print tax (`--record` after the fix)
  - **Acceptance**: a failing test first. On synthetic Ontario salaries the federal credit uses:
    16,452 at 150,000; 16,452 − (200,000 − 181,440) / (258,482 − 181,440) × 1,623 at 200,000;
    14,829 at 300,000.
  - **Hypothesis**: the gap between cashcast's tax and a CRA-rule reference falls from up to
    about $227 per income (0 at 150,000) to under $1 at all three incomes.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test app_strategy_math bpa`
    (new test, three synthetic incomes: 150,000, 200,000, 300,000)
  - **Success**: all three match the reference within $1 of tax
  - **Pivot**: if other provinces' credits break, scope the fix to the federal credit only and
    file the provincial ones apart
  - **Anchor**: CRA, Federal income tax and benefit guide, line 30000 (basic personal amount
    reduction for net income over the 4th bracket threshold)

- [ ] The owner sets up real data for every MoneyWiz account of the last two years
  - **ID**: real-data-setup
  - **Tags**: connectors, moneywiz, owner
  - **Details**: Code shipped 2026-11-03: stable Wise names (`wise-<CUR>.csv`), Kraken fiat legs as MoneyWiz
    rows, `api download etrade` (transactions JSON), the Tinkoff reader (`tinkoff-csv`) and 24-month
    playbook steps. Owner choice: banks without a personal API use the browser robot, never an aggregator.
    Steps, with the owner at the keyboard: (1) link in `cashcast.toml` `[moneywiz.accounts]`: the three Wise
    accounts to `wise-CAD`, `wise-USD`, `wise-EUR`, and Kraken to `Kraken CAD`; delete the old dated
    `wise-*-2026-*.csv` files in the inbox; (2) `api download wise`; (3) E*TRADE: the owner adds the keys,
    runs `api etrade-auth`, then `api download etrade`; (4) Kraken: `api download kraken`; (5) the
    `fetch-exports` run for CIBC, TD, Scotiabank (incl. TFSA), Wealthsimple, Sun Life and Tinkoff, 24 months;
    confirm the Tinkoff header with `bin/cashcast columns`; (6) `moneywiz sync --dry-run`, then `moneywiz sync`.
    Casa, Telegram Wallet and Modulbank Eur stay hand-entered (crypto only or no export; inactive since 2025).
  - **Blocked**: owner step (password manager approvals, 2FA, API keys in `.env`).
  - **Files**: `private/users/*/cashcast.toml` (owner), `connectors/playbooks/`
  - **Acceptance**: `moneywiz coverage` shows a Source for every active non-cash account, back to 2024-11.
  - **Hypothesis**: last-12-month coverage rises from 12.5 % (270 of 2,166 rows) to 70 %+.
  - **Measurement**: `bin/cashcast moneywiz coverage`
  - **Success**: 70 %+
  - **Pivot**: under 40 % → the sites cap history; use statement PDFs or accept the cap per account
  - **Anchor**: Kitchenham, Dybå, Jørgensen 2004 (evidence-based SE)

- [ ] The owner records the first real-use Proof numbers
  - **ID**: proof-real-use-outcomes
  - **Tags**: proof, docs, privacy, owner
  - **Details**: Since 2026-11-01 `review` logs `forecast_scored` and each strategy run logs `cashback_scored`
    (counts and a ratio only). `cashcast-engine proof field` turns them into the two README outcome rows.
  - **Blocked**: owner step. After 2 monthly reviews, run
    `engine/target/release/cashcast-engine proof field --private private` in Terminal, then `... proof render`, and commit
    `docs/proof/metrics.json` and `README.md`.
  - **Files**: `docs/proof/metrics.json`, `README.md`
  - **Acceptance**: both outcome rows show a number with kind `real use`, and no amount leaves `private/`.
  - **Hypothesis**: the miss rate falls under 50 % and the cash-back ratio is at least 1.05 on real use.
  - **Measurement**: `jq .field docs/proof/metrics.json`
  - **Success**: both rows show `real use` values after 2 monthly reviews
  - **Pivot**: the owner declines to commit field totals → keep `not measured yet` and drop the outcome claims
  - **Anchor**: Kohavi, Tang, Xu 2020 (trustworthy online experiments: measure the outcome, not a proxy)

- [ ] Users can run the MoneyWiz sync on their real MoneyWiz store
  - **ID**: moneywiz-real-store
  - **Tags**: engine, connectors, moneywiz
  - **Details**: `engine/src/moneywiz.rs` reads the Core Data layout that open-source readers document,
    tested only on synthetic stores. The owner runs `bin/cashcast moneywiz status` and
    `bin/cashcast moneywiz sync --dry-run` on their Mac. The agent reads only the aggregate report. If the
    reader refuses the layout or counts look wrong, fix it with a synthetic store of the same shape. Then run
    one `moneywiz sync` and confirm "MoneyWiz saved N of N". Also confirm that the transfer link honours `date`
    and `description`, which the MoneyWiz docs do not list.
  - **Files**: `engine/src/moneywiz.rs`, `engine/tests/moneywiz.rs`, `connectors/moneywiz.toml`
  - **Acceptance**: status lists every MoneyWiz account with the right transaction count; a sync leaves
    0 rows to add; `connectors/moneywiz.toml` `confidence` becomes "verified".
  - **Hypothesis**: Picking columns by entity name and value counts reads the owner's store without a
    code change.
  - **Measurement**: `bin/cashcast moneywiz status` transaction counts vs the MoneyWiz account list (owner compares)
  - **Success**: every account's count matches, and the second dry run shows 0 to add
  - **Pivot**: the layout is refused or counts differ after two fixes → read through a MoneyWiz CSV export
    instead and keep the URL-scheme writer
  - **Blocked**: checked 2026-11-01: `moneywiz status` reads the real store (every account listed). Left: one
    live `moneywiz sync` after `moneywiz-history-exports`, and the transfer `date`/`description` check.
  - **Anchor**: owner goal 2026-10-19 ("record only cash; one command updates everything")

- [ ] The inbox holds a full history export of every active bank and card, so MoneyWiz needs no typing
  - **ID**: moneywiz-history-exports
  - **Tags**: moneywiz, connectors, browser, owner
  - **Details**: `bin/cashcast moneywiz coverage` on 2026-11-01: 0 % of the MoneyWiz rows (not cash) of the
    last 12 months are covered. The inbox holds about one month of Scotiabank and TD rows, and no export is
    linked to a MoneyWiz account. Steps on the Mac: (1) download, with the browser, the longest history each
    bank allows for every active account: CIBC (OFX), TD (CSV), Scotiabank (CSV), Wealthsimple (monthly
    statement CSV), and Wise through `api download wise`; (2) run `moneywiz coverage` and show the owner the
    "Suggested links" lines; (3) with the owner's yes in chat, add them to `cashcast.toml`; (4) run coverage again.
  - **Files**: `private/users/<you>/cashcast.toml` (owner's yes), `connectors/playbooks/*.md` (fix any wrong step)
  - **Acceptance**: `moneywiz coverage` shows each active bank and card account with a source and no "no export" gap.
  - **Hypothesis**: with full exports and links, coverage of the last 12 months rises from 0 % to at least 95 %.
  - **Measurement**: `bin/cashcast moneywiz coverage` (the "Coverage, last 12 months" line; `moneywiz_coverage` events)
  - **Success**: at least 95 % covered in the last 12 months
  - **Pivot**: under 60 % after all exports are linked → the gap is matching, not exports: work on
    `moneywiz-match-rate` first
  - **Blocked**: owner, deferred on 2026-11-06 ("History: later"). Needs owner sign-ins in the browser back to
    2023-09-30 (`[moneywiz] since` is set to that day); the owner confirms the account links in chat.
  - **Anchor**: owner goal 2026-11-01 ("stop entering data (besides cash) into my moneywiz")

- [ ] MoneyWiz rows the owner typed pair with the bank rows they stand for
  - **ID**: moneywiz-match-rate
  - **Blocked**: after `moneywiz-history-exports` (needs a full real export history to read the match table).
  - **Tags**: engine, moneywiz
  - **Details**: on 2026-11-01, with the suggested links, the match table showed 0 of 8 rows paired for one card
    export and 0 of 3 for one chequing export; every unpaired row had no MoneyWiz row alike within 30 days
    ("nothing alike"), and only 2 had one within 1.00. The sample is one month. With `moneywiz-history-exports`
    done, read the match table again; if "nothing alike" stays high, check (with counts only) whether the owner
    types one row per several bank rows, other dates, or another account, and teach `plan` that pattern with a
    synthetic test. Never pair on a guess: unclear pairs stay conflicts.
  - **Files**: `engine/src/moneywiz.rs`, `engine/src/moneywiz_coverage.rs`, `engine/tests/moneywiz.rs`
  - **Acceptance**: the match rate per linked export is at least 90 %, and conflicts are under 2 % of bank rows.
  - **Hypothesis**: the low rate comes from one or two typing patterns of the owner; handling them lifts the
    match rate per export from under 60 % to at least 90 %.
  - **Measurement**: `bin/cashcast moneywiz coverage` ("Match rate per export" table)
  - **Success**: at least 90 % on every linked export with 30 or more bank rows
  - **Pivot**: still under 60 % after two patterns → the owner's old rows are not bank rows; set `[moneywiz]
    since` to the day the owner stops typing, so only new rows are compared
  - **Anchor**: Christen 2012, *Data Matching* (record linkage: blocking, then pairwise scores, clerical review of ties)

- [ ] Two separate banks get connectors so their MoneyWiz rows sync
  - **ID**: moneywiz-bank-connectors
  - **Blocked**: owner step: a real export from the card's bank, so `bin/cashcast columns` can learn its layout.
  - **Tags**: moneywiz, connectors
  - **Details**: `moneywiz coverage` marks two accounts out of scope until they have connectors (owner,
    2026-11-02): a virtual card at a separate bank (63 rows in the last 12 months) and a Russian bank account
    (27 rows). The Russian bank (Tinkoff) has a connector and reader since 2026-11-03; the card is left. Per bank: learn its export (CSV or OFX) with `bin/cashcast columns`, add `connectors/<id>.toml`
    and `connectors/playbooks/<id>.md`, a reader in `engine/src/export.rs` if no existing format fits, a canary
    fixture and a test. Then move the account from `[moneywiz.out_of_scope]` to `[moneywiz.connectors]` in the
    owner's `cashcast.toml` (the account names live only there).
  - **Files**: `connectors/*.toml`, `connectors/playbooks/`, `engine/src/export.rs`, `engine/tests/formats.rs`
  - **Acceptance**: both accounts show a linked export in `moneywiz coverage`, and `moneywiz sync --dry-run`
    pairs their rows.
  - **Hypothesis**: both banks export a format an existing reader (OFX or generic CSV) reads, so a connector
    file and a link cover 80 %+ of their 90 recent rows.
  - **Measurement**: `bin/cashcast moneywiz coverage` (covered share of the two accounts, last 12 months)
  - **Success**: 80 % or more covered for each account
  - **Pivot**: a bank offers no export at all → keep it out of scope with that reason
  - **Anchor**: owner request 2026-11-02 ("add a task to implement sync later")

- [ ] Users can edit MoneyWiz rows and categories in place on their real store
  - **ID**: moneywiz-edits-real-store
  - **Tags**: moneywiz, privacy
  - **Details**: `bin/cashcast moneywiz edit` and `fixes = "edit"` work on synthetic stores with a fake writer.
    Left: one category add and one memo change through `moneywiz edit` (the copy test, then a live run), then
    "Up to Date" in MoneyWiz sync and the change on a second device. If the writer fails, `moneywiz probe --ui`
    names the fallback (the window through Accessibility).
  - **Research**: 2026-10-20 on the Mac. The writer compiles with swiftc 6.4 and needs no change. The installed
    model is MoneyWizDataModel 48 with the checksum moneywiz-tools verified (app 2026.38.1 build 452 is newer than
    their 2026.32.1; the probe now says so instead of "no"). Every key the writer uses exists; `CategoryAssigment`
    has no GID or objectCreationDate (the writer sets those only when present); the required `assigmentNumber`
    and `displayOrder` default to 0. The engine's column names match moneywiz-tools' field mappings. Risk:
    `Account.ballance` is a stored running balance that amount and opening-balance edits do not update; check
    the account balance in MoneyWiz after the first amount edit, before `fixes = "edit"`.
  - **Blocked**: owner steps first. 1. Write `private/inbox/moneywiz-edits.toml` with a `[[category]] add =
    "cashcast test"` and one `[[row]]` with `set_memo` (playbook `edit-in-place`). 2. Add `[moneywiz]` with
    `writer = "core-data"` to `private/cashcast.toml`. 3. In Terminal: `bin/cashcast consent grant local:moneywiz`.
    4. Open MoneyWiz. Then the agent runs `moneywiz probe --ui`, `moneywiz edit --dry-run` and `moneywiz edit`
    (copy test), the owner adds the printed `writer_checksum`, and the agent runs the live write.
  - **Resume**: `/next-task moneywiz-edits-real-store` after the four owner steps
  - **Files**: `engine/moneywiz-writer/MoneyWizWriter.swift`, `engine/src/tasks/moneywiz_app.rs`, `engine/src/moneywiz_edit.rs`
  - **Acceptance**: a live `moneywiz edit` reports "Verified N of N changes", MoneyWiz opens without errors, and a
    second device shows the edited row and the new category after sync.
  - **Hypothesis**: Saving through MoneyWiz's own Core Data model with persistent history (as moneywiz-tools does
    for payees) also syncs amount, category and category-tree edits.
  - **Measurement**: verified changes per run, and the edited rows on a second device after sync (owner checks)
  - **Success**: 10 edits of each kind apply over 3 runs with 0 sync conflicts and 0 MoneyWiz errors
  - **Pivot**: a sync conflict, a crash, or edits that never reach the second device → restore the backup, set
    `fixes = "adjust"`, and build the Accessibility window driver instead
  - **Anchor**: owner goal 2026-10-19; decision 0007; marcomc/moneywiz-tools `doc/LIVE-WRITE-COMPATIBILITY.md`

- [ ] Run a MoneyWiz category migration and its rollback on the real store
  - **ID**: moneywiz-categories-real-store
  - **Tags**: moneywiz, privacy
  - **Details**: `bin/cashcast moneywiz categories` reads the real store (2026-11-01: 145 categories, types right:
    salary shows as income). `suggest`, `migrate` and `rollback` work on synthetic stores with a fake writer.
    Left: the first suggested migration on the real store, then its rollback. Also add `--user alex moneywiz
    categories` to `tests/parity/commands.txt` (the Rust port session owns that file).
  - **Blocked**: after `moneywiz-edits-real-store` (the writer's first live run and `writer_checksum`). Owner steps:
    1. `bin/cashcast moneywiz categories suggest`, then review `inbox/moneywiz-migrations/0001-suggested.toml`.
    2. `bin/cashcast moneywiz categories migrate --dry-run`, then `migrate`.
  - **Resume**: `/next-task moneywiz-categories-real-store` after step 2
  - **Files**: `engine/src/moneywiz_categories.rs`, `engine/src/tasks/moneywiz_app.rs`, `engine/moneywiz-writer/MoneyWizWriter.swift`
  - **Acceptance**: `migrate` reports "Recorded … as applied", `rollback` reports "Rolled back …", and
    `moneywiz categories` shows the first tree fingerprint again; MoneyWiz sync shows Up to Date both times.
  - **Hypothesis**: numbered migrations with a computed inverse change the tree with no manual fix-up: the tree
    fingerprint after `rollback` equals the one before `migrate`.
  - **Measurement**: `bin/cashcast moneywiz categories` "Tree:" line before `migrate` and after `rollback`, and
    `moneywiz_migrate` events with `result: ok`
  - **Success**: same fingerprint, 0 failed `moneywiz_migrate` events over 2 migrations
  - **Pivot**: a fingerprint that differs after rollback, or a sync conflict → restore the backup, and keep
    category changes to one `moneywiz edit` at a time
  - **Anchor**: owner goal 2026-11-01; decision 0012; Fowler, "Evolutionary Database Design" (reversible migrations)

- [ ] Confirm the bank and broker export layouts against real exports
  - **ID**: confirm-export-formats
  - **Tags**: engine, connectors, privacy
  - **Details**: The Scotiabank CSV, TD CSV, Wealthsimple statement, E*TRADE stock-plan and Casa layouts were
    built from public docs and marked "best guess" or `confidence = "estimate"` in `connectors/*.toml`. The owner
    drops one real export per service into their inbox and runs `bin/cashcast import`. The agent reads only the
    report's per-file format label and unreadable-row count. It never opens the files. A mismatch is fixed with a
    synthetic fixture of the same shape (red/green), never the real values.
  - **Files**: `engine/src/export.rs`, `engine/src/ledger.rs`, `engine/tests/`, `connectors/*.toml`
  - **Acceptance**: every service's real export imports with its own format label and 0 unreadable rows; the
    connector's `confidence` becomes "verified".
  - **Hypothesis**: Header detection built from public docs already reads at least 4 of the 6 layouts; fixing
    the rest takes at most one header-alias change each.
  - **Measurement**: `bin/cashcast import` → per-file `format` and the "unreadable rows" check (aggregates only)
  - **Success**: 6 of 6 layouts read with 0 unreadable rows
  - **Pivot**: a layout still fails after two alias fixes → ask the owner for `agent:raw-row` consent and one
    `first-error` row, or treat that service as playbook-only
  - **Blocked**: checked 2026-10-26: owner at the keyboard to approve 1Password logins and enter 2FA codes; then run the fetch-exports skill (`bin/cashcast fetch plan --all`).
  - **Anchor**: Basili, Caldiera, Rombach 1994 (GQM); owner request 2026-10-18

- [ ] The owner labels the judge cases, so the local judge's agreement is measured
  - **ID**: judge-evals-agreement
  - **Tags**: evals, owner
  - **Details**: The judge cases run on the local Ollama model (`qwen3:14b`) since 2026-10-21: 7 of 7 judge cases
    passed in the first real run. Agreement with the owner is still unmeasured: every entry in `evals/labels.yaml`
    says "owner: fill in". Since 2026-10-25 each case also has a deliberately bad answer (`<id>-bad`, 14 labels in
    all), and the eval run fails a judge that passes one.
  - **Blocked**: checked 2026-11-01: 17 entries still say "owner: fill in". Owner step first. Fill `pass` or
    `fail` per case in `evals/labels.yaml`, then run
    `evals/run.sh --agreement` in Terminal (the agent sandbox blocks local ports).
  - **Files**: `evals/labels.yaml`, `evals/cases/`, `evals/run.sh`
  - **Acceptance**: `evals/run.sh --agreement` prints k of n with n ≥ 14 (a good and a bad response per case).
  - **Hypothesis**: A local 14B judge agrees with the owner's own labels on at least 5 of 7 judge cases.
  - **Measurement**: `evals/run.sh --agreement` (judge verdicts vs `evals/labels.yaml`)
  - **Success**: ≥ 5/7 agreement on good responses and ≥ 5/7 on bad ones
  - **Pivot**: < 4/7 → keep these cases human-graded and use the judge only for regression diffs
  - **Anchor**: Zheng et al. 2023 (LLM-as-a-judge agreement with human labels)

- [ ] Users can read all their Notion finance pages in cashcast, and Notion holds none of it
  - **ID**: notion-real-export
  - **Tags**: notion, privacy, web
  - **Details**: `bin/cashcast notion import` and the Documents view are tested on a synthetic export only. The
    owner's Notion Finances tree has these sections (read-only check on 2026-10-19):
    - Credit Cards & Banking Plan
    - Banks (banks comparison, best credit cards)
    - Taxes (taxable income, 2025 taxes, deductibles, GST/HST, consultations, past years)
    - Crypto (exchanges, records)
    - Investments
    - Budgets (monthly, annual, proposals)
    - Pre-arrival archive
    On the Mac: in Notion, open Finances → ••• → Export (Markdown & CSV, include subpages, folders for
    subpages). Then run `bin/cashcast notion import ~/Downloads/<export>.zip --delete-source`. The agent reads
    only the printed counts. If a page or table is missing or renders badly, fix the importer with a synthetic
    export of the same shape (never the real file). Then the owner decides whether to archive the Notion copy.
    Never fetch the pages through the Notion connector: their text would pass through the model provider.
    While the web app is open, also check Home, Plan ahead and Forecast with real data (redesign of
    2026-10-19, `docs/design.md`); fix layout issues with the synthetic fixture.
  - **Files**: `engine/src/docs.rs`, `web/src/views/Documents.tsx`, `web/src/markdown.tsx`, `tests/e2e/us-15-notion.sh`
  - **Acceptance**: the import counts match the export (every page and table), 0 are skipped as "unsafe path",
    and the owner finds no page that reads worse than in Notion.
  - **Hypothesis**: The importer reads a real Notion export without a code change.
  - **Measurement**: `notion_import` event counts vs the number of pages and databases in the Notion tree
  - **Success**: every page and table arrives in the first or second import
  - **Pivot**: a Notion export shape the importer cannot read after two fixes → import the Notion HTML export instead
  - **Blocked**: checked 2026-10-26: owner at the keyboard to approve 1Password logins and enter 2FA codes; then run the fetch-exports skill (`bin/cashcast fetch plan --all`).
  - **Anchor**: owner goal 2026-10-19 ("fully migrate … Notion documents … super private"); decision 0008

- [ ] The Sunday sync keeps investment accounts in MoneyWiz in step
  - **ID**: moneywiz-investment-sync
  - **Tags**: moneywiz, investments, weekly
  - **Details**: The owner wants every account in MoneyWiz current each Sunday (2026-11-16).
    `moneywiz coverage` shows the investment accounts (Wealthsimple TFSA, FHSA and
    non-registered, E*TRADE, Scotiabank TFSA) at 0 % covered: `ledger.rs` reads their exports
    for holdings, but no reader turns them into MoneyWiz rows. Add that path (buys, sells,
    contributions, dividends, fees as MoneyWiz investment rows), linked like the cash accounts.
    Until it lands, the weekly "Not up to date" list should name these accounts.
  - **Files**: `engine/src/ledger.rs`, `engine/src/moneywiz.rs` (plan, links),
    `engine/moneywiz-writer/` (investment transactions), `engine/tests/moneywiz.rs` (synthetic
    fixture with canaries)
  - **Acceptance**: a failing test first (a synthetic Wealthsimple TFSA statement makes 0
    MoneyWiz rows); then a dry run on the owner's Mac lists the missing investment rows, and a
    real sync brings each linked investment account to "matches the bank".
  - **Hypothesis**: last-12-month coverage of the five investment accounts rises from 0 % to at
    least 90 %, so the Sunday run keeps them current with no hand entry.
  - **Measurement**: `bin/cashcast moneywiz coverage` (the "Last 12 months · Covered" column for
    those accounts), run by the owner in Terminal
  - **Success**: at least 90 % covered for each of the five, and 0 amount differences in
    `moneywiz sync --dry-run`
  - **Pivot**: if MoneyWiz's investment model cannot hold a row type through Core Data (decision
    0007), sync balance adjustments per account instead of single trades
  - **Anchor**: Basili, Caldiera, Rombach 1994 (GQM); decision 0007

- [ ] Loyalty points and miles show in cashcast next to the money
  - **ID**: points-awardwallet-connector
  - **Tags**: points, connector, strategy
  - **Details**: Owner choice 2026-11-22 ("plan it as a task"): travel points are money-like
    value, but today they live only in a separate AwardWallet skill outside cashcast. Add an
    AwardWallet connector: `connectors/awardwallet.toml` + `connectors/playbooks/awardwallet.md`,
    a consent scope for the AwardWallet Business API (`engine/src/core/consent.rs`, exits table in
    `docs/privacy.md`), an API client in `engine/src/api.rs` that saves the
    `GET https://business.awardwallet.com/api/export/v1/connectedUser/<id>` reply unparsed into
    the inbox (rule 7), a reader for it, and `bin/cashcast points` (program, kind, balance, elite
    status, expiry, last update error), with a parity row, an e2e story and a web view row.
    Key and user id go in the person's `.env` (`AWARDWALLET_API_KEY`, `AWARDWALLET_USER_ID`);
    only the owner adds them. Without them the command names the missing key and the setup step.
    Later: value points in dollars with a sourced cents-per-point fact in `knowledge/data/` and
    let `cards now` count them.
  - **Files**: `engine/src/api.rs`, a new `engine/src/points.rs`, `engine/src/core/commands.rs`,
    `engine/src/core/cli.rs`, `connectors/`, `tests/e2e/`, `web/src/views/`
  - **Acceptance**: a synthetic AwardWallet reply fixture with canary program names gives the
    right table; a canary never reaches any other output; no key means one clear setup line.
  - **Hypothesis**: questions about points answered from cashcast instead of a separate skill go
    from 0 of N to N of N, and the owner sees points beside cash in one place.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test points` and
    `scripts/e2e.sh points`; in use, the `points` event count in `.metrics/events.jsonl`.
  - **Success**: tests green, and at least one real `points` run a month after the owner adds the key.
  - **Pivot**: if the Business API needs a paid plan the owner does not want, read the
    AwardWallet CSV export into the inbox instead of the API.
  - **Anchor**: Basili, Caldiera, Rombach 1994 (Goal Question Metric).

## P2

- [ ] Find the flat cash gap in two MoneyWiz investment accounts
  - **ID**: moneywiz-invest-cash-gap
  - **Tags**: moneywiz, investments
  - **Details**: Found 2026-11-22 after the weekly `edit --fixes`. Units match the broker in both
    accounts, but `moneywiz diagnose` "Gap by month" shows a flat cash gap: SunLife RRSP since
    2025-11 and Non-registered simple since its first buy (its opening-balance fix was skipped
    because another fix already sets it). A flat gap is a wrong start, not a missing row.
  - **Files**: `engine/src/moneywiz_invest.rs`, `engine/src/moneywiz_edit.rs`, `engine/tests/moneywiz_edit.rs`
  - **Acceptance**: a failing synthetic test where two fixes touch one opening balance, then the fix.
  - **Hypothesis**: `moneywiz diagnose` "Last day off: none" goes from 17 of 19 accounts to 19 of 19.
  - **Measurement**: `bin/cashcast moneywiz diagnose` (count of "Last day off" cells reading none)
  - **Success**: 19 of 19 after one weekly `edit --fixes`
  - **Pivot**: if the gap is a cash leg the broker file lacks, report it as a hand entry and stop
  - **Anchor**: Fellegi & Sunter 1969, *JASA* 64(328) (record linkage with match weights)

- [ ] Paperwork and the citizenship judge share one presence-day counter
  - **ID**: paperwork-one-presence-counter
  - **Tags**: paperwork, refactor
  - **Details**: Found 2026-11-22 in the move from sidequest. `engine/src/paperwork/store.rs`
    and `engine/src/paperwork/citizenship/answers.rs` each count days in Canada (1,095 needed,
    half credit before PR capped at 365, the earliest sign day). Two copies can drift apart.
  - **Files**: `engine/src/paperwork/store.rs`, `engine/src/paperwork/citizenship/answers.rs`
  - **Acceptance**: one function, called by both; a synthetic test feeds the same trips to
    `paperwork presence` and `citizenship answers` and gets the same count and sign day.
  - **Hypothesis**: copies of the counter fall from 2 to 1, with 0 test changes in results.
  - **Measurement**: `rg -c "1095|1_095" engine/src/paperwork` and
    `cargo test --manifest-path engine/Cargo.toml --test paperwork --test citizenship`
  - **Success**: one counter, all tests green
  - **Pivot**: if the two rules really differ (sign day vs application day), keep both and
    name the difference in each function
  - **Anchor**: Hunt & Thomas 1999, *The Pragmatic Programmer* (DRY)

- [ ] The weekly keeps the bank balances it reads until `weekly --balance` runs
  - **ID**: weekly-balances-survive-long-run
  - **Tags**: weekly, transfers, agent
  - **Details**: Found 2026-11-22 in the attended Sunday run. Step 1c reads each balance in the
    browser, but step 2 runs after every download (about 2 hours later). The agent's context was
    compacted in between and the balances were gone, so `weekly` ran without `--balance` and made
    no transfer plan. Options: run `bin/cashcast transfers --balance …` right after the last
    balance read and keep only its output; or a `balances record ID=AMOUNT` command that saves
    them to the user's private data for that day, which `weekly` reads.
  - **Files**: `.claude/skills/cashcast/SKILL.md` (weekly steps 1c and 2), `skills/bank-balances/SKILL.md`, maybe `engine/src/transfers.rs`
  - **Acceptance**: the Sunday run makes a transfer plan with every balance read that day, even
    after a context compaction; no balance enters a file outside `private/`.
  - **Hypothesis**: Sunday runs that read balances but end with no transfer plan fall from 1 of 1
    (2026-11-22) to 0 of the next 4.
  - **Measurement**: the "Transfers" section of `private/.../out/weekly.html` each Sunday (`bin/cashcast weekly` prints it)
  - **Success**: 4 of 4 Sundays with balances read also show a transfer plan
  - **Pivot**: if a stored balance goes stale in use, drop storage and run `transfers` per bank right after each read
  - **Anchor**: Gray & Reuter 1993, *Transaction Processing*, ch. 1 (durable state across a long-running activity)

- [ ] `fetch collect` unpacks a Kraken ledgers ZIP and Kraken prices have one command
  - **ID**: kraken-zip-and-prices
  - **Tags**: fetch, kraken, crypto
  - **Details**: Found 2026-11-22. (1) Kraken's export is a ZIP; `fetch collect` moves only plain
    files, so the agent unzips by hand (playbook step 6) and old ZIPs pile up in Downloads.
    (2) The price top-up has no cashcast command: agents hand-write a script into the user's
    prices folder each week, and its ETHW pairs fail (ETHWEUR and ETHWUSD are not valid Kraken pairs).
  - **Files**: the `fetch collect` code in `engine/src/` (`rg -n "fn collect" engine/src`), `engine/src/api.rs` (Kraken public ticker), `connectors/playbooks/kraken.md`
  - **Acceptance**: a synthetic ZIP with a ledgers CSV and a canary lands in the inbox as the CSV, the ZIP is parked; `bin/cashcast api prices kraken` saves today's prices for every held asset with a valid pair and names the assets it skipped.
  - **Hypothesis**: hand steps per Sunday for Kraken fall from 2 (unzip, price script) to 0.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test fetch` plus the weekly gap log
  - **Success**: 0 hand steps for Kraken on the next Sunday run
  - **Pivot**: if Kraken's public ticker lacks a held asset, read its price from the ledger's last trade instead
  - **Anchor**: Hunt & Thomas 1999, *The Pragmatic Programmer*, tip 61 ("Don't use manual procedures")

- [ ] The sync worktree has one documented command to run the engine on the owner's data
  - **ID**: sync-worktree-runner
  - **Tags**: weekly, moneywiz, agent
  - **Details**: Found 2026-11-22. The skill says to use `~/.local/state/cashcast/sync` when the
    checkout is not clean `main`, but names no command; each run writes its own wrapper
    (`CASHCAST_ROOT=$HOME/apps/cashcast <sync>/engine/target/release/cashcast-engine cli …`). The
    scheduled run on 2026-11-22 ran some commands with the main checkout's engine (a feature
    branch with uncommitted edits) instead.
  - **Files**: `.claude/skills/cashcast/SKILL.md` step 0 (release-owned), `connectors/playbooks/moneywiz.md`, maybe a `scripts/sync-run.sh`
  - **Acceptance**: the skill names one command; a test runs it on the parity fixture and checks it used the sync worktree's binary.
  - **Hypothesis**: weekly commands run with an engine other than the sync worktree's fall from some (2026-11-22) to 0.
  - **Measurement**: the run transcript's engine paths (`rg -c "apps/cashcast/engine/target" <transcript>`) on the next two Sundays
  - **Success**: 0 commands on the main checkout's engine while it is not clean `main`
  - **Pivot**: if a wrapper still drifts, make `bin/cashcast` itself refuse a dirty or non-main checkout for `weekly`
  - **Anchor**: Humble & Farley 2010, *Continuous Delivery*, ch. 5 (one artifact, built once, run everywhere)

- [ ] The scheduled Sunday run reaches the owner when Remote Control is off
  - **ID**: sunday-run-start-push
  - **Tags**: weekly, scheduled
  - **Details**: Found 2026-11-22. The scheduled run waited 30 minutes for 1Password ("This tool
    requires an attended session"), but its start push never reached the phone because Remote
    Control was inactive. Also add the Chrome check (`tabs_context_mcp`) to the start push, so
    the owner connects Chrome before opening the run.
  - **Files**: `~/.claude/scheduled-tasks/cashcast-moneywiz-weekly-sync/SKILL.md`, `.claude/skills/cashcast/SKILL.md` weekly step 1
  - **Acceptance**: the start push reaches the owner on a Sunday with Remote Control off (macOS notification or a Reminders item due now).
  - **Hypothesis**: Sunday runs whose bank sign-ins time out unattended fall from 1 of 1 to 0 of the next 4.
  - **Measurement**: `mcp__scheduled-tasks__list_task_runs` for `cashcast-moneywiz-weekly-sync`, plus the run's "blocked" list
  - **Success**: 0 of 4 runs list the browser banks as blocked for want of an attended session
  - **Pivot**: if no channel reaches the owner, move the run to a time the owner sits at the Mac
  - **Anchor**: Forsgren, Humble & Kim 2018, *Accelerate*, ch. 13 (monitoring and alerting that reaches a person)

- [ ] PayPal sign-in works with the account's passkey
  - **ID**: paypal-passkey-signin
  - **Tags**: fetch, paypal, owner
  - **Details**: Found 2026-11-22. PayPal now asks for Face ID or Touch ID; "Try another way"
    did nothing and the page hung, so the PayPal export was skipped. The owner needs to be at the
    Mac to answer the passkey prompt, or to turn on password sign-in for PayPal.
  - **Files**: `connectors/playbooks/paypal.md`
  - **Acceptance**: the playbook says what to do at the passkey prompt; the next Sunday run gets a PayPal CSV.
  - **Hypothesis**: Sunday runs with no PayPal export fall from 1 of 1 to 0 of the next 4.
  - **Measurement**: `bin/cashcast fetch plan --all` (PayPal's last download date) each Sunday
  - **Success**: a PayPal file under 8 days old on 4 of 4 Sundays
  - **Pivot**: if a passkey always needs the owner, move PayPal to a monthly owner step
  - **Anchor**: W3C 2021, *Web Authentication Level 2* (user-presence step a script cannot complete)

- [ ] `sync all` restarts a hung Calendar or Reminders and retries once
  - **ID**: sync-restart-hung-app
  - **Tags**: sync, apple, weekly, unattended
  - **Details**: On 2026-11-16 Calendar did not answer `sync all` twice (the morning sync and
    the weekly run's last step): "Calendar is not responding (no answer in 90 s)". After
    `killall Calendar` and `open -g -a Calendar` the same sync finished in 10 s. The Sunday run
    is unattended, so the engine should do this itself: on a timeout, quit the app, reopen it in
    the background, and run the script once more before it reports the step as stopped. Until
    then, the skill's weekly step 4 does it by hand.
  - **Files**: `engine/src/tasks/apple.rs` (`run_jxa`, `TIMEOUT`), `tests/fakes/` (a fake
    osascript that hangs once), `engine/tests/` (a sync test)
  - **Acceptance**: a failing test first (the fake hangs on the first call: the sync must
    restart and pass on the second); then a weekly run shows no stopped `sync all` step.
  - **Hypothesis**: weekly runs whose `sync all` step stops fall from 2 of 2 (2026-11-16) to 0
    of the next 4 Sundays.
  - **Measurement**: `rg -c '"event":"weekly"' .metrics/events.jsonl` with the sync step result, or the
    weekly page's "Steps that stopped" each Sunday
  - **Success**: 0 stopped sync steps in 4 Sunday runs
  - **Pivot**: if a restarted app still hangs, the cause is the script, not the app: profile the
    JXA calls (event count per call) instead of adding more retries
  - **Anchor**: Nygard 2018, *Release It!* 2nd ed., ch. 5 (timeouts and fail-fast with
    bounded retries)

- [ ] Old worktrees and merged branches are cleaned up
  - **ID**: worktree-cleanup
  - **Tags**: git, hygiene, owner
  - **Details**: On 2026-11-06 an agent removed 18 worktrees that were clean and merged (32 → 14 left, plus
    agent worktrees of the running session). What is left needs the owner: unmerged work, uncommitted files,
    or a running session. The owner decides per worktree; the agent never force-removes these.
    - `~/apps/cashcast-live` (detached, clean, merged): the owner's live checkout? Keep or remove.
    - `.claude/worktrees/cards-now` (`feat/cards-now`, squash-merged, 1 uncommitted file): look, then remove.
    - `.claude/worktrees/agent-a1895…`, `agent-a9c0…`, `agent-af15…` (`worktree-agent-*`, unmerged, clean),
      `optimistic-golick-807471` (detached, unmerged): read `git log origin/main..HEAD`, keep or remove.
    - `agent-a2501…` (`feat/rust-11-python-free`), `agent-a8bf…` (`feat/rust-9-sync-all`), `agent-ae0b…`
      (`feat/rust-10-no-fallback`), `distracted-tesla-7501c5` (`feat/unexpected-charges`): merged, but a
      process still runs in them. Remove after those sessions end.
    A hook blocks force-deleting local branches: the owner runs `git branch -D` for merged branches,
    including `fix/ledger-dedupe-exports` (PR #94).
  - **Blocked**: owner step (decisions on unmerged work; sessions still running).
  - **Files**: none in the repo (git metadata only)
  - **Acceptance**: `git worktree list` shows only worktrees with open PRs, unmerged work, or running sessions.
  - **Hypothesis**: worktrees drop from 26 to 8 or fewer, so old checkouts stop holding stale engine builds
    and branch views stay short.
  - **Measurement**: `git worktree list | wc -l`
  - **Success**: 8 or fewer
  - **Pivot**: if more than half hold unmerged work, stop and ask the owner which to keep
  - **Anchor**: Forsgren, Humble, Kim 2018 (DORA: small batches, short-lived branches)

- [ ] Add the 2027 tax year when the CRA publishes indexation
  - **ID**: tax-2027
  - **Tags**: knowledge, tax
  - **Details**: Copy `knowledge/data/tax_ca_2026.toml` to `tax_ca_2027.toml` with the new brackets, BPA, CPP/EI
    and TFSA/RRSP limits from canada.ca, and point `engine/src/strategy/tax.rs` at the file for the current year.
  - **Files**: `knowledge/data/`, `engine/src/strategy/tax.rs`
  - **Acceptance**: `bin/cashcast knowledge check` shows 0 problems; the tax tests pass for all 13 jurisdictions.
  - **Hypothesis**: Updating within a month of the CRA release keeps marginal-rate errors under 0.5 points.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test app_strategy_math tax` plus a spot check of 3 provinces against the CRA page
  - **Success**: all 13 jurisdictions within 0.5 points of the CRA rates at 3 income levels
  - **Pivot**: a provincial rule the model cannot express (a new surtax shape) → model it, or mark that province "estimate"
  - **Blocked**: checked 2026-11-01: the CRA has not published the 2027 indexation yet (the canada.ca
    indexation page still ends at 2026, 2.0 %; usually mid to late November). Look again 2026-11-20.
  - **Anchor**: CRA "Canadian income tax rates for individuals" (yearly release)

- [ ] `cards now` reads the same fresh rows as `cards missed`
  - **ID**: cards-now-fresh-rows
  - **Tags**: cards, bug, moneywiz
  - **Details**: Found 2026-11-22 in the Sunday weekly. `cards now` counts caps and rent from the
    forecast import, which can end weeks before today and uses export account names. `cards
    missed` now reads MoneyWiz rows mapped through `[moneywiz.accounts]` (engine/src/main.rs
    `cards`), so the two can disagree on this month's spend toward a cap.
  - **Files**: `engine/src/main.rs` (`cards`), `engine/src/cards.rs` (`now`, `moneywiz_rows`), `engine/tests/cards.rs`
  - **Acceptance**: a failing synthetic test first: import rows that end before this month,
    MoneyWiz rows inside it; `cards now` counts the MoneyWiz rows toward the cap.
  - **Hypothesis**: weeks where `cards now` reports 0 spend this month while MoneyWiz holds card
    rows fall from 1 (2026-11-22) to 0.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test cards`
  - **Success**: the new test passes and the next weekly's `cards now` cap spend is above 0
  - **Pivot**: if MoneyWiz rows lack the reward category the cap needs, map categories with
    `knowledge/data/merchants.toml` instead of switching the source
  - **Anchor**: Kimball & Ross 2013, *The Data Warehouse Toolkit*, ch. 1 (one conformed source per fact)

- [ ] Review nits from the 2026-11-22 MoneyWiz sync fixes
  - **ID**: moneywiz-sync-review-nits
  - **Tags**: moneywiz, quality
  - **Details**: Found 2026-11-22 in the review of the weekly sync fixes. (1) a sell trade with a
    commission rarely pairs with its cash leg in `add_pending_trades`; (2) `file_named` also
    matches PDF statements; (3) `rows_already` does not compare the payee.
  - **Files**: `engine/src/moneywiz.rs`, `engine/tests/moneywiz.rs`
  - **Acceptance**: one failing synthetic test per nit first, then the fix.
  - **Hypothesis**: false holds and false "already done" counts in the dry run fall to 0 on the
    three synthetic cases.
  - **Measurement**: `cargo test --manifest-path engine/Cargo.toml --test moneywiz`
  - **Success**: three new tests pass and parity shows 0 mismatches
  - **Pivot**: if a commission makes the pair ambiguous, keep the hold and name the trade in the report
  - **Anchor**: Fellegi & Sunter 1969, *JASA* 64(328) (record linkage with match weights)

## P3

- [ ] Grocery browser steps wait for product cards, not a fixed sleep
  - **ID**: grocery-wait-for-cards
  - **Tags**: grocery, speed
  - **Details**: Moved from sidequest on 2026-11-22. Uber Eats and DoorDash steps slept 5-6 s per page (25 pages a run). `cardsReady()` in `skills/grocery-cart/cart.js` polls for the first card for up to 6 s. The measurement needs one live grocery run in the owner's signed-in Chrome: ask Claude to order groceries for 5 items on Uber Eats and DoorDash, then time the browser batches.
  - **Hypothesis**: a 5-item, 2-app search drops from about 3 min to under 2 min.
  - **Measurement**: the browser batch times in `node ~/.config/agentbrew/scripts/agent-perf.mjs report --session <id>` for that run.
  - **Success**: under 2 min.
  - **Pivot**: pages still render late, so keep the sleep.
  - **Anchor**: Card, Moran, Newell 1983, keystroke-level model (task time).


- [ ] The standards score reaches 38 of 38 (`check-setup.sh --profile assistant --strict`)
  - **ID**: standards-score-full
  - **Tags**: docs, setup
  - **Details**: Found 2026-11-15 at 34/38 on `main`. Failing: AGENTS.md is 15.4 kB (limit
    10 kB: move tables to linked docs); `scripts/setup.sh` is not first in the README quickstart
    with a `docs/README.md` link in the first 60 lines; `scripts/checkup.sh` does not end with a
    local HTML page; CONTRIBUTING.md has no "Report problems and ideas" section linked from
    AGENTS.md, and `Agentfile.yaml` does not install `report-upstream`.
  - **Files**: `AGENTS.md`, `README.md`, `scripts/checkup.sh`, `CONTRIBUTING.md`, `Agentfile.yaml`
  - **Acceptance**: the checker prints `score: 38/38`.
  - **Hypothesis**: the score rises from 34/38 to 38/38 with no lost AGENTS.md rule.
  - **Measurement**: `bash ~/.claude/skills/new-project/scripts/check-setup.sh . --profile assistant --strict`
  - **Success**: 38/38
  - **Pivot**: below 36/38 after one PR → split AGENTS.md by area first
  - **Anchor**: agentbrew `docs/agent-guide-baseline.md`
