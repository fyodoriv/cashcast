# User stories

Each story is something a person does end to end. Each one names its metric:
an outcome a user feels, read from the user's content-free event log
(`private/users/<name>/logs/events.jsonl` and `errors.jsonl`). A test count is
only a guard. Each story has an e2e test per case in `tests/e2e/`, and each
test names its story id. `scripts/check-docs.sh` fails when this file and
`tests/e2e/` drift apart. A change to what the user does or sees updates its
story and its tests in the same commit.

## US-01: Set up with one command

- **As** a new person (or contributor), **I want** one command, **so that** the tools work in minutes.
- **Metric**: time from clone to `doctor: all checks passed`. **Target**: under 5 minutes (first engine build excluded).

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-01-setup.sh` | `check.step` |
| error | `tests/e2e/us-01-setup.sh` (a broken connector spec makes the doctor print the fix) | `check.step` |

## US-02: Import exports from any of my banks and apps

- **As** a person, **I want** to drop exports into my inbox and import them, **so that** the app knows my history without anyone seeing my transactions.
- **Metric**: share of `command` events for `import` with `result: failed`. **Target**: under 5%. Guard: canary leak tests.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-02-forecast.sh` (CSV exports → verified forecast) and `engine/tests/formats.rs` (every service format) | `command` |
| error | `tests/e2e/us-06-consent.sh` (unreadable-row debugging needs `agent:raw-row`) | `error` |

## US-03: Answer only the questions the app cannot work out

- **As** a person, **I want** a short list of open questions, **so that** the forecast is right with little typing.
- **Metric**: questions asked per new user. **Target**: at most 5.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-02-forecast.sh` (status lists questions; answers count without re-import) | `command` |
| error | `engine/tests/forecast_model.rs` (missing balance blocks the forecast with a named fix) | `command` |

## US-04: See my cash forecast and editable workbook

- **As** a person, **I want** a 15-month forecast checked to the cent, **so that** I know how much I can invest.
- **Metric**: formula-check differences. **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-02-forecast.sh` | `report_shown` |
| error | `engine/tests/forecast_model.rs` | `command` |
| forecast vs actual (`backtest`, saved snapshots) | `engine/tests/forecast_model.rs`, `tests/e2e/us-02-forecast.sh` | `command` |

## US-05: Share one copy with other people in Canada

- **As** a household, **I want** a private folder per person, Canadian residents only, **so that** nobody sees another's data.
- **Metric**: cross-user reads. **Target**: 0. Guard: province validation.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-05-users.sh` | `user_created` |
| error | `tests/e2e/us-05-users.sh` (non-Canadian province; ambiguous user) | `error` |

## US-06: Decide what leaves my computer

- **As** an owner, **I want** reports to reach an AI agent or a cloud service only after I say yes at my terminal, **so that** my data stays private by default.
- **Metric**: reports shown to an agent without a grant. **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-06-consent.sh` | `report_withheld`, `report_shown`, `consent_granted` |
| error | `tests/e2e/us-06-consent.sh` (agent cannot grant itself) | `error` |

## US-07: Get a detailed money strategy

- **As** a person, **I want** card routing, credit targets, reserve, tax, investing, payments, budget and alternatives in one plan, **so that** every dollar is routed for the most gain.
- **Metric**: net yearly gain found (see `docs/VISION.md`). **Target**: ≥ $800 for the owner.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-07-strategy.sh` | `section_built` |
| happy | `tests/e2e/us-07-strategy.sh` (three subscriptions become "Payments #1" to "#3" with a yearly total and three review items in the alternatives; no payee text in any output) and `engine/tests/recurring.rs` | `section_built` |
| error | `tests/e2e/us-07-strategy.sh` (missing profile names the fix) | `error` |
| agent | `evals/cases/card-calendar.yaml` (the card routing needs `agent:strategy`; an empty wallet asks for the cards) and `skills/card-calendar/SKILL.md` (which card for what, month by month, from the routing and dated tasks) | none |

## US-08: Do the monthly review in five minutes

- **As** a person, **I want** one dated checklist with signals (stale data, missed predictions, stale facts), **so that** I keep the plan on track and pivot when it stops fitting.
- **Metric**: monthly reviews done per month. **Target**: 1.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-07-strategy.sh` | `report_shown` |
| error | `engine/tests/app_strategy_sections.rs` (two missed predictions flag a pivot) | `section_built` |

## US-09: Get financial tasks in their own Reminders list

- **As** a person, **I want** dated tasks in a separate Apple Reminders list, **so that** money tasks never mix with other lists.
- **Metric**: duplicate reminders after re-sync. **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-09-reminders-connectors-knowledge.sh` | `reminders_sync` |
| error | `tests/e2e/us-09-reminders-connectors-knowledge.sh` (no `osascript` explains how to run it) | `reminders_sync` |

## US-10: Work with my 18 services by browser or API, safely

- **As** a person, **I want** export guides, playbooks and read-only APIs for each service, **so that** an agent can do the safe work and I do only the money moves.
- **Metric**: money actions done by an agent. **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-09-reminders-connectors-knowledge.sh` | `action_planned` |
| error | `tests/e2e/us-09-reminders-connectors-knowledge.sh` (API without consent) | `error` |

## US-11: Trust and update the knowledge

- **As** a person or contributor, **I want** every fact sourced and dated, with a check for stale ones, **so that** advice stays current.
- **Metric**: stale or unsourced facts. **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-09-reminders-connectors-knowledge.sh` | `command` |
| error | `engine/tests/app_tasks.rs` (stale, unsourced and estimate facts are flagged) | `command` |

## US-13: Plan ahead in the web app

- **As** a person, **I want** a local web app with a plan-ahead timeline, a what-if control and every strategy view, **so that** I can plan months ahead and switch views without reading a long document.
- **Metric**: views rendered from the API, guarded by `web/src/test/views.test.tsx` (15 of 15). **Target**: 15/15, and only this computer can open it.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-13-web.sh` | `web_started` |
| error | `tests/e2e/us-13-web.sh` (no token, no data) | `web_started` |

## US-14: Record only cash in MoneyWiz; one command adds the rest

- **As** a MoneyWiz user, **I want** one command to add every bank and card row MoneyWiz lacks and to flag (or correct) wrong amounts, **so that** I only type cash.
- **Metric**: rows still missing after a sync (`moneywiz sync --dry-run` "to add"), `moneywiz_sync` events with `result: failed`, and the share of MoneyWiz rows (not cash) the exports cover (`moneywiz coverage`, `moneywiz_coverage` events). **Target**: 0 missing, under 5% failed, 95% covered in the last 12 months.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-14-moneywiz.sh` (one sync adds rows and one transfer and corrects an amount and the opening balance; `fixes = "list"` only lists) and `engine/tests/moneywiz.rs` | `moneywiz_sync` |
| happy | `tests/e2e/us-14-moneywiz.sh` (the sync collects an export from Downloads first and lists rows with no category) | `fetch_collect`, `moneywiz_sync` |
| happy | `tests/e2e/us-14-moneywiz.sh` (`moneywiz edit` changes a row and adds, renames and deletes categories after a test on a copy; `fixes = "edit"` edits a wrong amount and the opening balance in place) and `engine/tests/moneywiz_edit.rs` | `moneywiz_edit` |
| happy | `tests/e2e/us-14-moneywiz.sh` (`moneywiz categories` lists the tree, `suggest` writes a migration, `migrate` applies it after a test on a copy, `rollback` undoes it and stops on drift; without consent it writes nothing; the web app's picks become a migration that applies only with `--yes`) and `engine/tests/moneywiz_categories.rs`; the Categories view in `web/src/test/views.test.tsx` | `moneywiz_migrate` |
| happy | `tests/e2e/us-14-moneywiz.sh` (the web app's Bank sync view lists every row to add, one confirmed send saves them, then nothing is left; the engine refuses the view outside the owner's web app) and the Bank sync cases in `web/src/test/views.test.tsx` | `moneywiz_sync` |
| happy | `tests/e2e/us-14-moneywiz.sh` (`moneywiz probe` names the ways in for edits) and `engine/tests/app_probe.rs` | `moneywiz_probe` |
| happy | `tests/e2e/us-14-moneywiz.sh` (a row deleted in MoneyWiz is not added back, `--resend` sends it; a MoneyWiz CSV in the inbox is not a bank export; `moneywiz coverage` counts) and `engine/tests/moneywiz.rs` (the keyed sent list is mode 600 and holds no rows; equally good matches are conflicts; a second sync waits; a cash account links only by config) | `moneywiz_sync`, `moneywiz_coverage` |
| happy | `tests/e2e/us-14-moneywiz.sh` (`moneywiz weekly` sends; the ledger keeps each bank row's status and check dates and is mode 600; an unchanged dry run is cached; `moneywiz resync` sends again a deleted row in one range; `moneywiz diagnose` and `moneywiz categorize --dry-run` print counts only) and `engine/tests/moneywiz_categorize.rs` (categories learned from the owner's rows and the bank's words; added rows get them; the edits file is never overwritten) and `engine/tests/moneywiz.rs` (overlapping ledger downloads fold; loose matching prefers the closest amount and reaches rough dates; the gate holds a wrong plan and a missing statement, but not an early row standing in for bank rows, nor a month the bank never issued that MoneyWiz accounts for (its rows are left alone), nor an account in `[moneywiz.trust_bank]`; only the app's own database sends to the real app; reconcile rows are offset; 0.00 rows are never added; an older bank balance still corrects a wrong start); the held account, ledger line and history in `web/src/test/views.test.tsx` | `moneywiz_sync` |
| happy | `engine/tests/moneywiz.rs` `wealthsimple_investment_accounts_sync_like_a_bank` (a Wealthsimple activity file links each investment account `[moneywiz.investments]` maps to its own cash rows; another person's account in the same file stays out; trade legs are left to the buy and sell rows; coverage counts the paired trades; the sync dry run names the trades to add) and `engine/tests/ledger_cash.rs` | `moneywiz_sync`, `moneywiz_coverage` |
| happy | `engine/tests/pdf_statements.rs` (TD credit card, TD chequing, Scotiabank Visa and Scotiabank chequing PDF statements read with their signs and years; Sun Life statements read units per fund over a page break and refuse a table that does not add up, and their units check MoneyWiz and cover the rows before them; E*TRADE stock plan statements read their cash flow rows, refuse a file whose rows miss the net, give their closing cash as the balance only when the opening and the rows lead to it, link to the plan's `[moneywiz.investments]` mapping and stay out of the cash import; `engine/tests/moneywiz.rs` offsets a stock plan account's buy and sell rows, sets its start cash from the statements, holds it when a statement is missing, and `edit --fixes` deletes nothing there; a statement whose closing cash MoneyWiz matches, with every share move paired by the broker files, covers the rows to its day (`stock_plan_statement_cash_with_paired_shares_covers_the_rows_before_it`); Scotia Smart Investor year-end statements read their market value summary (refused when it does not add up; a first statement runs from its quarter start; quarter statements read nothing) and no rows (`scotia_smart_investor_*`); rows the owner accepts in `[moneywiz.accepted]` and 0.00 cash rows count as covered (`accepted_rows_count_as_covered_up_to_their_day`); coverage lists each open row by account, day, amount and kind, never its text (`coverage_since_names_every_gap_from_the_start_day`); `edit` deletes one named row with `delete = true`, alone and never a transfer (`engine/tests/moneywiz_edit.rs` `delete_removes_one_named_row_alone`); a yearly file holds several statements; a statement whose balances do not add up is refused whole; PDF rows yield to CSV and QFX days of the same account; `columns` prints masked shapes only; `fetch collect` moves statements and leaves other PDFs) | `moneywiz_sync`, `moneywiz_coverage` |
| happy | `engine/tests/moneywiz.rs` `coverage_since_names_every_gap_from_the_start_day` (with `[moneywiz] since`, coverage counts every row from that day on, names the days each account must download, the bank rows to add and the MoneyWiz rows with no bank row, and leaves out-of-scope accounts out of the goal), `transfers_into_a_matched_account_cover_an_account_with_no_export` (a transfer from an account with no export counts when its other side matches a bank row) and `engine/src/tasks/weekly.rs` (the weekly run lists the history gaps under "Not up to date") | `moneywiz_coverage` |
| error | `tests/e2e/us-14-moneywiz.sh` (no `local:moneywiz` consent: nothing is sent; a missing database names the fix) and `engine/tests/moneywiz.rs` (an unknown layout is refused); `moneywiz probe` without the app names the fix; `moneywiz edit` names unmatched edits by number and writes nothing without consent | `error` |

## US-15: Move my Notion finance pages here, privately

- **As** someone who kept finance notes in Notion, **I want** to move every page, table and image into cashcast and read it in the web app, **so that** nothing private stays in a cloud workspace, and nothing private passes through an AI agent.
- **Metric**: `notion_import` events whose page and table counts match the export, with 0 skipped as "unsafe path". **Target**: every import.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-15-notion.sh` (an offline import of a Notion "Markdown & CSV" export, then the Documents API) and `engine/tests/app_web.rs` | `notion_import` |
| error | `tests/e2e/us-15-notion.sh` (a file that is not an export changes nothing) and `engine/tests/app_web.rs` (unsafe paths and reads outside the documents are refused) | `error` |

## US-12: Know my investment and crypto cost base

- **As** an investor, **I want** positions, adjusted cost base and gains by tax year from my broker and crypto exports, **so that** I report capital gains right and spot concentration.
- **Metric**: assets with unknown cost. **Target**: 0 after opening positions are entered.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-12-holdings.sh` | `command` |
| error | `tests/e2e/us-12-holdings.sh` (crypto received without a cost is flagged, not guessed) | `command` |

## US-16: Get dated deadlines in my own Apple Calendar

- **As** someone who plans the week in a calendar, **I want** card, tax and bank deadlines as all-day events with an alert in my own calendar, **so that** I see them next to everything else and never miss one.
- **Metric**: `calendar_sync` events with `result = ok`, against the dated tasks. **Target**: every dated task is in the calendar after a sync.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (preview lists only dated deadlines) and `engine/tests/app_apple.rs` | `calendar_sync` |
| error | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (sync needs `local:calendar`, then macOS) and `engine/tests/app_apple.rs` | `error` |

## US-17: Make the bank and card changes the strategy asks for

- **As** someone with a costly bank package or card, **I want** the needed setting changes listed with their sign-in page and steps, and done in my browser with my yes, **so that** I stop paying fees I do not need.
- **Metric**: `change_done` events against open changes. **Target**: every open change done or dismissed within a month.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (list, then done) and `engine/tests/app_tasks.rs` | `change_done` |
| error | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (an unknown change id explains) | `error` |

## US-18: Know where my new money goes

- **As** an investor with target weights, **I want** a buy list per account (TFSA, RRSP, FHSA, non-registered) for my new cash, **so that** my portfolio moves toward my targets in the right accounts without selling.
- **Metric**: the largest gap to target after the plan. **Target**: lower than before, every run.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (a plan from profile.toml) and `engine/tests/app_strategy_math.rs` (200 random portfolios) | `optimize` |
| error | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (no inputs yet explains) | `error` |

## US-19: Start everything with one command

- **As** the owner or an agent, **I want** `cashcast` alone to set up, start the web app, refresh, and say what it needs from me, **so that** I never look up a sequence of commands.
- **Metric**: commands typed until the first useful screen. **Target**: 1.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (start, reuse, stop; `config set` round trip; the action catalog) and `engine/tests/app_actions.rs` | `start` |
| error | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (no people yet says what to do) | `start` |

## US-20: Stay within my RRSP limit

- **As** someone with a group RRSP (Sun Life payroll and employer match), **I want** this year's contributions checked against my deduction limit, with a projection from my payroll pace, **so that** I never pay the 1% a month over-contribution tax.
- **Metric**: windows that end above the limit plus the $2,000 buffer. **Target**: 0, with a reminder at least two weeks before the projected crossing.
- **No routine check**: the hourly `sync all` parses new exports first and reruns the check. A task appears only when you must act: the limit is missing, you are over or on pace to go over, or the newest contribution is over two months old ("Download the Sun Life RRSP transaction history").

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-12-holdings.sh`, `engine/tests/app_strategy_math.rs` (a stale export asks for a fresh one), the tests in `engine/src/tasks/sync.rs` (`sync all` parses exports first), `engine/tests/ledgers.rs` (contributions per deduction year) | `command` |
| error | `tests/e2e/us-12-holdings.sh` (no limit asks for the NOA figure) | `command` |

## US-21: Keep Reminders, Calendar and Notion in step

- **As** someone who checks money tasks in Reminders, Calendar and Notion, **I want** one hourly sync that keeps all three the same, **so that** a task I tick in one place is done everywhere and I never act on a stale copy.
- **Metric**: targets out of step after one `sync all` (`sync status`). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-21-sync.sh` (status names each target) and the tests in `engine/src/tasks/sync.rs` (the real scripts against fake apps: ticks flow both ways, 0 drift) | `sync` |
| error | `tests/e2e/us-21-sync.sh` (Notion without its consent) and the tests in `engine/src/tasks/sync.rs` (one target fails, the others still run) | `sync` |
| weekly session | the tests in `engine/src/tasks/weekly.rs` (weekly and monthly tasks fold into one Sunday reminder listing them by date; yearly and one-time tasks stay; a ticked session moves to next week; a missed week rolls to the next Sunday, whose title counts the overdue tasks) | `sync` |
| own tasks | `engine/tests/app_strategy_sections.rs` (`[[tasks]]` in profile.toml join the plan; a cashcast id only moves that task's date; an id ending in `*` sets title and notes on every month's copy; done stays done) and `engine/tests/app_strategy_sections.rs` (no task asks for a daily duty) | `sync` |

## US-22: Get fresh exports either way

- **As** someone whose data goes stale every month, **I want** to choose each time between exporting the files myself and approving the agent to fetch them in the browser, **so that** refreshing takes one approval instead of a sign-in, a download and a file move per service.
- **Metric**: manual file moves per refresh (`fetch_collect` events vs files imported). **Target**: 0, with every fetched service "fresh" in `fetch plan`.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-22-fetch.sh` (a download reaches the inbox unopened, no file name printed, and imports) and `engine/tests/app_tasks.rs` (stale services first, with their route and login URL) | `fetch_collect`, `fetch_plan` |
| error | `tests/e2e/us-22-fetch.sh` (nothing to collect; `--all` lists every service) and `engine/tests/app_tasks.rs` (partial, old and .zip files stay in Downloads; a shared format names no service) | `fetch_collect` |
| agent | `evals/cases/fetch-exports.yaml` (asks once, both ways, before any sign-in; never types codes or reads the files) | none |

## US-23: Work through my open finance tasks

- **As** someone with many open money tasks, **I want** one agent command (`/cashcast-tasks`) that resolves what cashcast can and names who must do the rest, **so that** no task sits open without a next step.
- **Metric**: open tasks without a lane and a step (`bin/cashcast tasks --json`). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` (resolve closes the due review and lists each blocker) and `engine/tests/app_tasks.rs` (lanes, only due `auto` tasks close) | `tasks_resolved` |
| error | `engine/tests/app_tasks.rs` (a failed report leaves its task open) | `tasks_resolved` |
| agent | `engine/tests/app_tasks.rs` (confirm/check/ensure tasks go to the `agent` lane with the service to check; decisions and money stay blocked) and `commands/cashcast-tasks.md` (done only with evidence) | none |
| verdict | `tests/e2e/us-16-19-calendar-changes-optimize-start.sh` and `engine/tests/app_tasks.rs` (a pass closes the check; a fail keeps it open; a reason with an amount or score is refused; a check is not taken before its date; a browser check needs `agent:browser-read`) | `task_verdict` |

## US-24: Keep every bank at its amount with few, quick transfers

- **As** someone with four banks and a $3,000 daily e-Transfer limit, **I want** one plan from today's balances that says what to send where, day by day, in amounts quick to type, **so that** no bank drops below its fee-free minimum and no card misses its paydown.
- **Metric**: banks below `keep` after the plan, and moves over the daily limit (`engine/tests/transfers.rs`). **Target**: 0 and 0, with every amount within 10 % (or $50) of the need.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-24-transfers.sh` (a large gap spreads over days; the card paydown is included) and `engine/tests/transfers.rs` (keep plus card, the safety margin and its 20 % cap, surplus back to the hub, quick amounts) | `command` |
| error | `tests/e2e/us-24-transfers.sh` (an unknown id; withheld without `agent:strategy`) and `engine/tests/transfers.rs` (the hub would fall below its keep) | `report_withheld` |
| agent | `evals/cases/bank-balances.yaml` (the plan needs consent; an e-Transfer is never sent by the agent) | none |

## US-25: Count my Wise USD in my strategy

- **As** someone who keeps most of their Wise balance in US dollars, **I want** the strategy to count it in Canadian dollars and tell me what fits US-dollar cash, **so that** my reserve is right and I do not pay to convert money I will spend or invest in US dollars.
- **Metric**: foreign cash rows from `data/api-balances.json` left out of the reserve without a reason shown (`bin/cashcast strategy`). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-25-foreign-cash.sh` (the reserve shows a CAD line for Wise USD; the foreign section names Norbert's gambit, T1135 and line 12100) and `engine/tests/app_strategy_math.rs` (converted at the dated Bank of Canada rate; T1135 task above $100,000) | `section_built` |
| edge | `engine/tests/app_strategy_math.rs` (a Wise account already in MoneyWiz is not counted twice; USD cash accounts in the summary are converted; an unknown currency is shown, not counted; stale rates are flagged) and `tests/e2e/us-25-foreign-cash.sh` (no balances → how to read them) | `section_built` |

## US-26: See my Kraken crypto at market value

- **As** someone with crypto on Kraken, **I want** the strategy to value it in CAD at a dated price and count the cash I keep there, **so that** I see what I own and what selling would cost in tax, without crypto counting as emergency cash.
- **Metric**: crypto balances in `data/api-balances.json` with a saved price but no CAD value in the Crypto section (`bin/cashcast strategy`). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-26-crypto.sh` (value at market, Kraken CAD in the reserve) and `engine/tests/app_strategy_sections.rs` (CAD and USD prices; gain from the average cost per unit) and `engine/tests/app_api.rs` (plain symbols, prices from the public ticker with no key) | `section_built` |
| edge | `engine/tests/app_strategy_sections.rs` (unknown cost, missing price, stale prices, Wise and Kraken in one path; E*TRADE totals are investments, not cash) and `tests/e2e/us-26-crypto.sh` (no balances → how to read them) | `section_built` |

## US-27: Count my Wealthsimple, Casa and Sun Life without an API

- **As** someone whose investments sit at services with no public API, **I want** an agent to read them in the browser and record them once, **so that** the strategy shows every account at market value, by tax bucket, next to my crypto.
- **Metric**: investment or crypto connectors (`broker`, `crypto`, `retirement`) with neither a balance API nor a read-only browser step that records into the strategy (`engine/tests/app_tasks.rs`). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-27-browser-balances.sh` (`api record` prints no amounts; the Invest section shows the value per bucket; a Casa coin appears) and `engine/tests/app_strategy_sections.rs` (records merge; a USD account converts; a Casa coin uses the Kraken price) | `balances_recorded` |
| error | `tests/e2e/us-27-browser-balances.sh` (an account number is refused) and `engine/tests/app_strategy_sections.rs` (unknown service, no `=`, not an amount, empty) | none |
| edge | `engine/tests/app_strategy_sections.rs` (records older than 31 days are flagged; investments never count as cash) | `section_built` |
| agent | `skills/balances/SKILL.md` (API first, browser second, never an account number) | none |

## US-28: See what my cash earns

- **As** someone with cash at Wise and several banks, **I want** the strategy to show what each balance earns a year at its own rate, **so that** idle cash sits where it earns most and the fee math uses the true lost interest.
- **Metric**: cash places in the Interest section with no sourced rate (neither banks.toml `interest_rate` nor `[interest.<service>]`) (`bin/cashcast strategy`). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-28-interest.sh` and `engine/tests/app_strategy_math.rs` (Wise per currency, banks at `keep`; best CAD rate; transfer-hub advice; line 12100) | `section_built` |
| edge | `engine/tests/app_strategy_math.rs` (a Wise bank account is not counted twice beside the API rows; the holding cost subtracts what the account pays; banking values cash at the best rate you hold; Wise CAD equals the bank catalog) | `section_built` |

## US-29: Know which card to use right now

- **As** someone who rotates four cards with caps that reset on different days, **I want** each spending kind's best card today from what I actually spent since each cap reset, plus the rent charges counted and the credit left, **so that** I never put a purchase on a card whose bonus is used up or whose balance is too high.
- **Metric**: wallet caps with a `period` the engine does not count, or a pick on a card whose cap, rent count or credit is used up (`engine/tests/cards.rs`). **Target**: 0 and 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-29-cards-now.sh` (cap used since `cap_reset` with a refund netted; rent before a card's `rent_rate_from` still uses its cap; the rent rule moves rent on; a recorded card balance is used; no payee text) and `engine/tests/cards.rs` (one test per cap period: card year, calendar year, monthly, statement; per-category and combined caps; rent posted before a card's `rent_rate_from` still uses its cap) | `report_shown` |
| error | `tests/e2e/us-29-cards-now.sh` (an unknown card; withheld without `agent:strategy`) and `engine/tests/cards.rs` (a card not in the catalog; a bad `cap_reset`) | `report_withheld` |
| happy: missed | `engine/tests/cards.rs` `missed_totals_charges_on_a_card_that_earns_less_since_a_day` (`cards missed`: charges on a card that earns less, per card and category, totals only; rows before the period and off the wallet are not checked) | `report_shown` |
| edge | `engine/tests/cards.rs` (unknown reset counts the last 12 months and says so; stale rows; a car rental under "rent" is not a rent charge; a pending product change keeps the old card; a cap shared with rent is saved for the rent charges left; room beyond them stays usable) and `tests/e2e/us-29-cards-now.sh` (`cards now` and the strategy route groceries off the rent cap alike) and `engine/tests/app_tasks.rs` (the engine's category rules match the strategy's; a done change is no longer pending) | none |
| agent | `evals/cases/cards-now.yaml` (needs consent; the agent never changes the rent card or applies for a card) | none |

## US-30: Get honest investment moves I can argue with

- **As** someone with employer stock, Wealthsimple accounts, crypto and USD cash, **I want** ranked investment moves that say what each could gain as a range, why, on what evidence, at what risk and cost, next to "do nothing", **so that** I move money only when the evidence supports it, and I can challenge or override any move.
- **Metric**: claims in the plan with no cited fact (`missing from knowledge` lines in `bin/cashcast invest-plan`), plus judge-eval failures on certainty, stock picks, money moves and overrides (`CHECK_EVALS=1 scripts/check.sh`). **Target**: 0 and 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-30-invest-plan.sh` (ranked moves with evidence; employer concentration as a risk move) and `engine/tests/app_strategy_sections.rs` (cash above the reserve is a ranged money task; fees compare like with like) | `invest_plan` |
| edge | `engine/tests/invest_plan.rs` (do nothing wins with nothing to do and below the simplicity threshold; money needed soon stays safe; stale or missing evidence is flagged; harvesting employer stock warns about vests; a large RRSP holding a Canadian-listed US fund gets a ranged swap to a US-listed fund with its Norbert cost, a small one gets no task: `rrsp_us_listed`) | `invest_plan` |
| override | `tests/e2e/us-30-invest-plan.sh` and `engine/tests/invest_plan.rs` (a move, a kind or a symbol is skipped with its reason and cost, once, with no task) | `invest_plan` |
| error | `tests/e2e/us-30-invest-plan.sh` (withheld without `agent:strategy`) | `report_withheld` |
| agent | `skills/invest-challenge/SKILL.md` and `evals/cases/invest-challenge.yaml` (never certain, never a stock pick, never moves money, accepts a reasoned override) | none |

## US-31: Grow my net total inside my own limits

- **As** someone with money in banks, Wise, brokers and crypto exchanges and debt on cards, **I want** one number, everything I have minus everything I owe, with its trend and the moves that raise it most, **so that** I focus on growing it without breaking the limits I set (no burnout, a cash floor, risk, debt, my home).
- **Metric**: options shown as recommended that break a `[boundaries]` limit, and sources with a balance that the net total leaves out (`bin/cashcast net-total`, `engine/tests/net_total.rs`). **Target**: 0 and 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-31-net-total.sh` (crypto counted; an idea ranked; extra work refused for burnout; one trend point a day) and `engine/tests/app_strategy_sections.rs` (Wise USD in full, Kraken crypto, Wealthsimple, a card as debt; no account names; it leads the strategy) | `section_built` |
| error | `tests/e2e/us-31-net-total.sh` (a margin above 20 % names the fix; withheld without `agent:strategy`) and `engine/tests/net_total.rs` (bad input is an error, not a panic) | `report_withheld` |
| edge | `engine/tests/net_total.rs` (unknown work hours refuse extra work; investing cash is sized to stay above the floor plus margin; home, debt, risk and refused tags; the trend vs 30 and 365 days ago) and `tests/e2e/us-31-net-total.sh` (`bin/cashcast` prints the net total first; `start.net_total` hides it) | none |

## US-32: Catch charges I did not expect

- **As** someone with subscriptions on several Canadian bank accounts and cards, **I want** a list of duplicate subscriptions (one merchant charging from two series, under any billing name), recurring charges that started in the last 60 days, price rises over 10 % and charges after a pause, plus `charges find WORD` for one merchant's charges per account, **so that** I cancel what I pay twice or no longer want.
- **Metric**: planted duplicates, new series, rises and restarts that `charges` misses, and mirrored copies (a bank export and MoneyWiz) it reports as duplicates (`engine/tests/charges.rs`). **Target**: 0 and 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-32-charges.sh` (two Anthropic plans on two cards under two billing names; `find` lists both per account; no payee text) and `engine/tests/charges.rs` (aliases group billing names; two amounts on one card) | `report_shown` |
| error | `tests/e2e/us-32-charges.sh` (withheld without `agent:strategy`; `find` with no word) | `report_withheld` |
| edge | `engine/tests/charges.rs` (a mirrored copy is not a duplicate; a new series, a price rise on a steady price, a restart after a pause; the leak test) | none |
| agent | `.claude/skills/cashcast/SKILL.md` (asked about subscriptions or a merchant, the agent runs `charges` or `charges find WORD`) | none |

## US-33: Check my spending in plain words

- **As** a person, **I want** to know if last month was more than usual, if I spend more than I earn, and how much interest and fees I paid, **so that** I catch a problem without reading every category.
- **Metric**: of these three questions, the ones `bin/cashcast spending` answers (`engine/tests/app_strategy_sections.rs`). **Target**: 3 of 3.

| Case | Test | Log event |
|---|---|---|
| happy | `engine/tests/app_strategy_sections.rs` (`spending_answers_the_three_questions`: a category at twice its usual month is flagged; one under $50 a month and a steady one are not; income above spending; interest totalled; APR explained) | `section_built` |
| edge | `engine/tests/app_strategy_sections.rs` (`spending_over_income_adds_a_task_and_no_fees_says_so`) | none |

## US-34: Get a missing item without paying twice

- **As** someone whose Amazon.ca order arrived with one item missing, **I want** an agent to ask Amazon in my browser for a free replacement or a refund for that item, **so that** I get it and pay once.
- **Metric**: refund cases with a skill and a safety eval (`evals/cases/refund.yaml`). **Target**: the agent never places a second order or pays (judge case `refund-missing-amazon-item-never-reorders`).

| Case | Test | Log event |
|---|---|---|
| happy | `skills/refund/SKILL.md` (Amazon missing item: replacement at $0.00 or refund, yes before Submit) | none |
| agent | `evals/cases/refund.yaml` (asked to buy it again, the agent refuses and asks Amazon instead) | none |

## US-35: Do the whole Sunday session with one command

- **As** a person with weekly money tasks, **I want** one command to run every check, the forecast and the investing review, and to list this week's moves without duplicates, **so that** Sunday takes one step and no two reminders ask for the same work.
- **Metric**: overlaps in the open task list after the run (`bin/cashcast weekly`, the `weekly` event's `overlaps`). **Target**: 0 once the listed tasks are removed.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-35-weekly.sh` (every step under its own heading; two pay-downs of one card are one overlap; a refresh task is the weekly's own work; moves by day with the overdue one; notes; `weekly.html` saved at mode 600) and the tests in `engine/src/tasks/weekly.rs` (overlaps, the floor line, the moves list) | `weekly` |
| error | `tests/e2e/us-35-weekly.sh` (without `agent:strategy` the steps run but the moves are withheld) | `report_withheld` |
| agent | `tests/repo/weekly-expectations.sh` (`/cashcast-weekly` lists the owner's 7 results in order; MoneyWiz is written without asking; nothing goes to Reminders or Calendar before the owner confirms; the results page comes before improvements) and `engine/src/core/cli.rs` `weekly_writes_reminders_only_after_the_owner_confirms` | none |

## US-36: Track my citizenship paperwork privately on this Mac

- **As** a permanent resident in Canada, **I want** the citizenship steps, my presence day count and a guided application kept sealed on my own Mac, **so that** I know what is left and when I can apply, and my dates and ID numbers never go online.
- **Metric**: requirements whose official quote still matches the live page (`bin/cashcast paperwork sources --live`), and plain paperwork files on disk (`bin/cashcast paperwork doctor`). **Target**: every requirement OK or MANUAL (checked by hand); 0 plain files.
- **Also**: the guided application (`bin/cashcast citizenship`) judges the answers and each form screen. **Metric**: seeded defects caught on synthetic answers. **Target**: 10 of 10, 0 false fails. Collectors fill 95% or more of the facts from a full synthetic set of sources.
- Moved from sidequest US-26 on 2026-11-22 (decision 0017). The browser unlock is not ported: paperwork has no web route. Each test named below is checked to exist (`tests/repo/story-tests.sh`).

| Case | Test | Log event |
|---|---|---|
| happy: steps and facts | `tests/e2e/us-36-paperwork.sh` `us36_happy_steps_doctor_and_the_application_status`; `engine/tests/paperwork.rs` `steps_and_sources_need_no_state`, `every_requirement_cites_an_official_page_a_quote_and_a_day`, `the_online_steps_name_both_reasons_to_apply_on_paper`, `only_the_steps_of_the_chosen_way_to_apply_show`, `the_test_deadline_shows_only_while_the_test_is_to_do` | none (threat model T3) |
| happy: presence count | `engine/tests/paperwork.rs` `travel_days_count_as_in_canada`, `the_window_is_the_5_years_before_the_sign_day`, `days_before_pr_count_as_half_days_up_to_365`, `earliest_sign_day_moves_by_a_trips_full_days_away`, `the_day_count_comes_from_the_dates_only`, `one_day_short_reads_1_day`, `presence_exit_code_says_met_or_not` | none |
| happy: police certificates | `engine/tests/paperwork.rs` `a_stay_of_183_days_abroad_needs_a_police_certificate`, `the_police_rule_counts_the_day_you_left_and_came_back`, `the_police_rule_counts_only_the_4_years_since_age_18`, `a_long_multi_country_trip_can_be_not_needed_until_a_new_long_trip`; `engine/tests/citizenship.rs` `police_skips_origin_before_pr_but_not_after` | none |
| happy: progress and trips | `engine/tests/paperwork.rs` `progress_counts_done_and_not_needed_counts_in_neither`, `not_needed_sticks_only_where_it_can`, `not_needed_comes_back_when_the_rule_starts_to_apply`, `an_old_not_needed_on_a_step_everyone_needs_comes_back`, `trips_and_periods_are_removed_by_dates_once`, `remove_takes_the_trip_with_the_same_place`, `the_cli_removes_the_trip_with_no_country_when_the_owner_types_dash`, `a_mark_without_a_note_keeps_the_items_note` | none |
| happy: guided application | `engine/tests/citizenship.rs` `passes_the_synthetic_example`, `catches_each_seeded_defect`, `ready_answers_say_yes_and_exit_0`, `missing_answers_become_questions`, `early_sign_day_names_the_earliest_and_past_fails`, `older_than_55_needs_no_language_and_yes_text_counts`, `gaps_find_uncovered_days_and_present_runs_to_sign`, `screen_matches_other_formats_and_never_echoes`, `parsers_read_csv_months_mrz_and_pr_card`, `stays_join_into_trips_and_cra_reads_years`, `merge_keeps_the_owners_answers_and_refreshes_its_own`, `collect_all_fills_95_percent_sealed_and_prints_no_canary`, `restore_brings_back_an_older_draft_and_keeps_the_newer_one`, `template_never_writes_over_a_file`, `proofs_are_sealed_and_inlined_in_the_report` | none |
| privacy | `engine/tests/paperwork.rs` `nothing_plaintext_on_disk_and_modes_are_tight`, `the_agent_view_is_redacted`, `the_agent_view_gives_away_no_date`, `agents_and_pipes_cannot_change_dates_try_sign_days_or_move_documents`, `private_values_are_typed_at_a_prompt_never_after_the_command`, `a_terminal_with_no_owner_present_gets_exit_2_and_no_private_value`, `saves_at_the_same_time_keep_every_change`, `a_live_writers_lock_is_waited_for`, `a_save_removes_only_its_own_lock`; `engine/tests/paperwork_vault.rs` `seal_then_open_round_trips`, `opens_a_sidequest_blob`, `write_command_never_replaces_an_item`; `engine/tests/citizenship.rs` `put_seals_owner_answers_and_removes_its_scratch_file`, `an_answers_file_others_can_read_fails`, `show_reveal_outside_a_terminal_shows_nothing`, `an_old_evidence_page_is_removed_by_the_next_run`; `tests/e2e/us-36-paperwork.sh` `us36_error_agents_cannot_set_private_values` | none |
| error | `engine/tests/paperwork.rs` `a_date_field_refuses_a_year_no_one_could_mean`, `bad_input_names_the_rule_not_the_value`, `a_tampered_or_swapped_file_is_refused`, `sources_live_flags_a_page_that_no_longer_says_what_we_cite`, `sources_live_lists_a_pdf_to_check_by_hand`, `page_text_drops_scripts_tags_and_entities`, `live_check_sends_curls_own_user_agent`, `a_lock_left_by_a_gone_writer_is_broken`, `doctor_finds_folders_that_lost_their_mode`, `doctor_warns_on_a_sealed_leftover_and_fails_on_a_plain_file`, `doctor_finds_a_plain_file_or_an_open_folder_at_any_depth`, `doctor_fails_on_links_and_hard_links`, `doctor_fails_when_the_paperwork_folder_is_a_link`, `doctor_warns_about_an_old_save_lock`, `doctor_on_a_new_user_makes_nothing`; `engine/tests/paperwork_vault.rs` `wrong_purpose_or_key_or_tamper_reads_nothing`; `engine/tests/citizenship.rs` `a_screen_field_that_differs_exits_1_and_never_echoes` | none |

## US-37: Use cashcast without MoneyWiz

- **As** a person in Canada who does not use MoneyWiz, **I want** cashcast to read my bank and card downloads, show my spending, a forecast and a plan, **so that** I get the whole app with only my bank sites and Claude.
- **Metric**: weekly steps that stop for a new person with only bank downloads (`bin/cashcast weekly --dry-run`, "Steps that stopped"). **Target**: 0.

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-37-new-user-no-moneywiz.sh` (a fresh HOME with no MoneyWiz; RBC chequing and Tangerine card CSVs; import, spending, forecast, strategy; the weekly dry run skips MoneyWiz, cards, holdings and an unset sync with plain lines; no canary on disk) | `weekly` |
| error | `engine/tests/app_strategy_sections.rs` `spending_counts_salary_as_earned` (salary alone is income, so no false "spent more than you earned" task) | `strategy` |

## US-38: Buy groceries at the cheapest full basket

- **As** a person who orders groceries for delivery, **I want** every store price the agent reads kept on my computer, the best pack per 100 g/ml with a substitute, each store's total with its delivery fee, service fee and promo, and my grocery spend per month, **so that** I order from the cheapest store that has everything, without the agent searching every store again.
- **Metric**: stores ranked out of order by `grocery compare` (a store with a missing item above a full one, or a dearer full basket above a cheaper one) and free delivery or a promo given to a partial basket (`engine/tests/groceries.rs`). **Target**: 0 and 0. In use: the `grocery` event's `action` and `rows` (`compare`, `save`, `order`).

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-38-groceries.sh` (save, fee rules, best pack and substitute, compare full baskets first, stale prices, an order and spend per month, the database at mode 600; an older database imports and stays unchanged) and `engine/tests/groceries.rs` (price per 100 from labels and pack sizes, organic premium, promos) | `grocery` |
| error | `tests/e2e/us-38-groceries.sh` (orders withheld without `agent:strategy`; usage lines for compare without items, a bad fee, no fresh prices, a missing import file) | `report_withheld` |
| edge | `engine/tests/groceries.rs` (free delivery and a promo need a full basket; reading creates no database; an import twice makes no doubles) and `web/src/test/views.test.tsx` (Groceries view: spend, fees, rules, compare) | none |
| agent | `skills/grocery-cart/SKILL.md` and `evals/cases/grocery-cart.yaml` (fills carts, ranks by checkout total, never checks out) | none |
