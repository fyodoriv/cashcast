# Agent rules for cashcast

## Start here

cashcast is a private, local-first financial assistant for people in Canada.
It covers:

- the cash forecast (the original cashcast)
- a money strategy: cash back, credit, reserve, tax, investing, payments, budget and alternatives
- a local TypeScript web app (`web/`, `bin/cashcast web`), the main view, with plan-ahead
- a monthly review
- one Apple Reminders list per user
- connectors for 18 services
- a sourced knowledge base

Each person's data lives in `private/users/<name>/` and never leaves the
computer without their consent.

- Canonical branch: `main`.
- Read this file, then only the doc the routing table names.
- `CLAUDE.md` holds `@AGENTS.md`.
- Docs map: [`docs/README.md`](docs/README.md).
- Read [`docs/VISION.md`](docs/VISION.md) before you propose a feature.

**Starting the app for a user:** on `/cashcast-start`, "start cashcast", or a
request for a forecast or strategy, follow `.claude/skills/cashcast/SKILL.md`
from Step 0. This works in any agent.

## Commands

Run from the repo root.

| Task | Command |
|---|---|
| Start the app (auto-updates a clean checkout; `tests/repo/auto-update.sh`) | `bin/cashcast` · then `config`, `actions` · `bin/cashcast update` |
| One-line setup (idempotent; installs missing tools via `scripts/install-tools.sh`) | `scripts/setup.sh` |
| A new person's Mac: README one-liner in a fresh HOME, then first use (CI job `fresh-mac`) | `scripts/fresh-mac.sh` |
| Public readiness (leaks, private paths, workflows, licence; `--history` for every commit) | `scripts/public-readiness.sh` (gate step `readiness`; rules in `SECURITY.md`) |
| Health check | `scripts/doctor.sh` (repo) · `bin/cashcast doctor` (app, builds the engine) |
| Checkup (open PRs and branches, health, cached gate, stories, speed, P0s, monthly audit) | `scripts/checkup.sh` |
| One engine test file / one test | `cargo test --manifest-path engine/Cargo.toml --test app_users` · `... --test app_users only_canadian` |
| Engine tests (unit, app and forecast) | `cargo test --manifest-path engine/Cargo.toml` |
| Repo tests (scripts, docs, evals, web fixture) | `tests/repo/run.sh` · `tests/repo/run.sh evals` |
| Lint, format | `rustfmt --edition 2024 --config skip_children=true FILE.rs` · web: `cd web && pnpm lint` |
| Web app tests / build | `cd web && pnpm test` / `pnpm build` (fixture: `scripts/web-fixture.sh`) |
| Fix loop / fast gate | `scripts/check.sh changed` / `fast` |
| Full gate with e2e (before "done"; cached by input hash) | `scripts/check.sh` (`NO_CACHE=1` reruns all) |
| E2E only (arguments pick stories by file name) | `scripts/e2e.sh` · `scripts/e2e.sh us29` |
| Speed report | `scripts/metrics.sh` |
| Parity with the frozen Python outputs (0 mismatches; `--record` after an intended change) | `scripts/rust-parity.sh` · `--runs 5` for medians · `--only WORD` · test: `tests/parity/harness-test.sh` |
| Python lines left (must stay 0) | `scripts/python-lines.sh --by-dir` |
| README Proof numbers (saves `docs/proof/metrics.json`, rewrites the section; `engine/tests/proof.rs` fails when stale) | `engine/target/release/cashcast-engine proof measure --events <main checkout>/.metrics/events.jsonl` · `field --private <main checkout>/private` (this machine's real-use totals, local only) · `render` |
| Judge evals (Ollama) | `CHECK_EVALS=1 scripts/check.sh` · `evals/run.sh --agreement` |
| Standards score | `bash ~/.claude/skills/new-project/scripts/check-setup.sh . --profile assistant --strict` |

## Where to change what

| I want to… | Change | Update this doc too |
|---|---|---|
| Read a new export format | `engine/src/export.rs` (cash), `engine/src/pdf.rs` (bank PDF statements; `pdf/etrade.rs` for E*TRADE stock plan statements; tests in `engine/tests/pdf_statements.rs`) or `engine/src/ledger.rs` (investments, crypto), a fixture with canaries, a test | `connectors/<service>.toml` `formats`, `README.md` |
| Add or change a service | `connectors/<id>.toml` + `connectors/playbooks/<id>.md`; an API client in `engine/src/api.rs` | `connectors/README.md` |
| Add a strategy section | `engine/src/strategy/<id>.rs`, register it in `PORTED` in `engine/src/strategy/mod.rs`, a test naming the id (`tests/repo/contributions.sh` checks) | `CONTRIBUTING.md` list, `docs/user-stories.md` |
| Change a fact (rates, limits, cards) | `knowledge/data/*.toml` with `source`, `as_of`, `review_by`, `confidence` | `knowledge/README.md` |
| Add or change a live fact (Bank of Canada series) | `engine/src/live_facts.rs` `SERIES` (sane range, the fact it checks), `engine/tests/live_facts.rs` | `knowledge/README.md`, decision 0015 |
| Change what leaves the computer | `engine/src/core/consent.rs` scopes, `engine/src/core/gate.rs`, an API client | `docs/privacy.md` exits table |
| Change the MoneyWiz sync or how it reads MoneyWiz | `engine/src/moneywiz.rs`, `engine/src/moneywiz_view.rs` and `web/src/views/BankSync.tsx` (the Bank sync view), `engine/tests/moneywiz.rs` | `connectors/playbooks/moneywiz.md`, `docs/user-stories.md` US-14 |
| Change in-place MoneyWiz edits | `engine/src/moneywiz_edit.rs`, `engine/src/tasks/moneywiz_app.rs` (Rust runs edit, categories, probe, the applying sync), `engine/moneywiz-writer/`, `engine/tests/moneywiz_edit.rs` | playbook `edit-in-place`, decision 0007 |
| Change MoneyWiz category migrations or suggestions | `engine/src/moneywiz_categories.rs`, `engine/src/tasks/moneywiz_app.rs` (`categories`), `engine/tests/moneywiz_categories.rs` | playbook `manage-categories`, decision 0012 |
| Change the Notion import or the Documents view | `engine/src/docs.rs`, `web/src/views/Documents.tsx`, `tests/e2e/us-15-notion.sh` | `skills/notion-import/SKILL.md`, `docs/user-stories.md` US-15, decision 0008 |
| Change `cards now` (caps, rent count) | `engine/src/cards.rs`, `engine/tests/cards.rs`, `engine/src/core/cli.rs` (`cards`) | US-29 |
| Change groceries (prices, best pack, store totals, orders, import) | `engine/src/groceries.rs`, `engine/tests/groceries.rs`, `engine/src/core/cli.rs` (`grocery`), `web/src/views/Groceries.tsx`, `tests/e2e/us-38-groceries.sh` | `skills/grocery-cart/SKILL.md`, US-38, `README.md` |
| Change `charges` (duplicate subscriptions, new or pricier charges, `find`) | `engine/src/charges.rs`, `engine/tests/charges.rs`, `knowledge/data/merchants.toml` (aliases), `engine/src/core/cli.rs` (`charges`) | US-32, `README.md` |
| Change the net total, its boundaries or option ranking | `engine/src/net_total.rs`, `engine/tests/net_total.rs`, `engine/src/strategy/net_total.rs` (sources, options) | US-31, decision 0014, `profile/profile.example.toml` |
| Change the task sync | `engine/src/tasks/sync.rs` (`sync all`, `status`, `install`, `uninstall`, `sheets`), `notion.rs`, `apple.rs`, and the rest of `engine/src/tasks/` (`tasks`, `changes`, the previews, the start screen; parity rows) | `docs/user-stories.md` US-21 |
| Add or change a web view | `web/src/views/`, register it in `web/src/App.tsx` `VIEWS`, a case in `web/src/test/views.test.tsx`; wrap a scrolling table in `TableWrap` (keyboard focus, `web/src/test/a11y.test.tsx`) | `README.md` views list |
| Change the web API shape | `engine/src/web.rs`, `web/src/types.ts`, then `scripts/web-fixture.sh` (`tests/repo/web-fixture.sh` fails when stale) | `docs/user-stories.md` US-13 |
| Add a CLI command | `engine/src/core/commands.rs` (parser, `SUMMARY`; the action catalog follows), `engine/src/core/cli.rs` (`build`, `execute`), a parity row (`--record` its golden), an e2e test in `tests/e2e/` | `README.md` commands table |
| Change the bank transfer plan | `engine/src/transfers.rs`, `engine/tests/transfers.rs`, `tests/e2e/us-24-transfers.sh` | `skills/bank-balances/SKILL.md`, `docs/user-stories.md` US-24 |
| Add an app setting | `engine/src/core/settings.rs` | `README.md` |
| Change users, consent, config, telemetry or knowledge commands | `engine/src/core/`, `engine/src/knowledge.rs`; add a row to `tests/parity/commands.txt` | decision 0010 |
| Add a log event | the code via `events.log` | `docs/observability.md` |
| Change the forecast model | `engine/src/forecast/` (`status`, `forecast`, `backtest`, `verify-numbers`), `engine/tests/forecast_model.rs`; parity rows; it must lower the `bin/cashcast backtest` miss (decision 0012) | `README.md` "How the forecast works", decision 0010 |
| Change assistant behavior | `.claude/skills/cashcast/SKILL.md`, `skills/`, `prompts/`; run the evals | `evals/rubric.md` |
| Change paperwork (citizenship steps, presence count, official sources, the vault) | `engine/src/paperwork/` (`store.rs`, `facts.rs`, `vault.rs`, `presence.rs`, `cli.rs`), `engine/tests/paperwork.rs`, `engine/tests/paperwork_vault.rs`, `skills/paperwork/SKILL.md` | US-36, `docs/paperwork-threat-model.md` |
| Change the guided citizenship application | `engine/src/paperwork/citizenship/` (`answers`, `collect`, `draft`, `evidence`, `judge`), `engine/tests/citizenship.rs`, `skills/citizenship-application/SKILL.md`, `evals/cases/paperwork.yaml` | US-36, `docs/paperwork-threat-model.md` (A7) |
| Record a hard-to-undo decision | `docs/decisions/NNNN-*.md` | none |
| Add or close a task | `TASKS.md` | none |

## Rules

1. **Privacy first.** Never open, read, search, print, copy or upload anything
   in a user's `inbox/`, `data/`, `out/` or `docs/` (`private/inbox|data|out|docs` and
   `private/users/*/inbox|data|out|docs`). Learn about data only from
   `bin/cashcast` output. Never open a user's `paperwork/` folder or any
   `.sqv` file: use the redacted `paperwork` and `citizenship` output.
2. You may read and edit a user's `profile.toml` and `cashcast.toml`. Never
   edit `consent.toml` or `.env`: only the owner grants consent and stores tokens.
3. When `bin/cashcast` says a report was withheld, tell the user the
   `consent grant` command to run themselves. Never work around the gate.
4. `bin/cashcast first-error` prints one raw row. Run it only with the user's
   explicit OK. It also needs the `agent:raw-row` scope.
5. Never commit anything from `private/`. Never put amounts, balances,
   account names, payees, employers or tokens in code, tests, fixtures,
   commits, memory or notes. Tests use synthetic data with canary strings.
6. Actions by risk: `read` → do it; `settings` → ask the user to confirm the
   exact change in chat before the final click; `money` → never do it
   (it becomes a reminder for the user). Never type passwords or 2FA codes.
7. `engine/src/export.rs`, `ofx.rs`, `pdf.rs` (with `pdf/etrade.rs`), `ledger.rs`, `moneywiz.rs` and `moneywiz_edit.rs` are the only code that
   reads raw rows. API clients save downloads unparsed into the inbox. Only
   `engine/moneywiz-writer` writes the MoneyWiz database, through Core Data and
   never raw SQL (decision 0007).
8. Every new format, detector, section or connector needs a test (the leak
   tests fail if a canary reaches any output).
9. Keep `.gitignore` an allowlist: every new top-level path needs a `!` line.
10. Fix with `check.sh changed`; run `scripts/check.sh` once per commit; show its output.
    A bug fix starts with a failing test.
11. Each change updates its docs in the same PR (see `## Docs upkeep`).
12. Facts in `knowledge/` need a source and dates. Mark unverified numbers
    `confidence = "estimate"`.
13. Agentfile: `Agentfile.yaml` declares this repo's agent needs. Preview with
    `agentbrew sync --dry-run`, apply with `agentbrew sync`. Generated
    per-agent files are outputs. Edit skill sources, never installed mirrors.
14. Tasks live in `TASKS.md` (tasks.md format, pre-registered Hypothesis,
    Measurement, Success, Pivot, Anchor). Remove a block when it is done.
15. **A UI with more than one view is built in TypeScript** (`web/`); Rust keeps the logic
    and serve JSON. No Notion integration (decision 0005).
16. **Public repo.** The code is public; data never is. `scripts/public-readiness.sh`
    must pass (it runs in the gate and in CI). Never weaken a check in it to make
    a change pass: fix the change, or add a synthetic value to
    `config/public-readiness-allow.txt` (`SECURITY.md`, "Allowed exceptions").
17. **Report problems upstream.** When cashcast fails, surprises you, or could
    clearly work better, offer once to report it (`CONTRIBUTING.md`, "Report
    problems and ideas"; the `report-upstream` skill). Post only after a yes.

## Gotchas

| Date | Symptom | Fix |
|---|---|---|
| 2026-10-18 | `git switch -c X origin/main` half-fails ("could not lock config file") | the sandbox blocks the upstream-tracking write to `.git/config`; run `git switch --no-track -c X origin/main`, or switch outside it. Never `git symbolic-ref HEAD refs/heads/X` alone: on a missing branch it makes an unborn branch, and it leaves the index at the old commit |
| 2026-10-18 | `numbers` / `reminders sync` / `calendar sync` fail (-600, -10822); judge evals see no Ollama | the sandbox blocks `osascript` and local ports; run them in Terminal |
| 2026-10-18 | `consent grant` says it needs a real terminal | by design: agents cannot grant; the owner runs it in Terminal |
| 2026-10-18 | CI fails US-01 with no output, but it passes on the Mac | CI is Linux: no BSD-only flags (`stat -f`, `sed -i ''`) in scripts; test with GNU coreutils on PATH |
| 2026-10-20 | `check.sh` `forecast` step times out (900 s) in a fresh worktree | the first import builds the engine; run `cargo build --release --manifest-path engine/Cargo.toml` first |
| 2026-10-19 | E*TRADE holdings too high; sold shares looked held | Benefit History `Released Qty` is gross; net shares are the `Shares released` events (holdings `## Checks` compares) |
| 2026-10-19 | a parser needs a private file's layout | `bin/cashcast columns`: header names and masked shapes only |
| 2026-10-30 | `calendar sync` fails "-10025 start date must be before the end date" | a task moved later: write the event end before the start (`tests/fakes/jxa_reminders_calendar.js` now enforces start < end) |
| 2026-11-02 | `heavy-run` fails "Operation not permitted" in an agent sandbox; `$TMPDIR` differs inside and outside it | run `heavy-run` outside the sandbox and write its output under the repo or an absolute path |
| 2026-11-04 | parity goldens held real MoneyWiz rows | the harness ran with the real `$HOME`, so MoneyWiz found the owner's database; `scripts/rust-parity.sh` now runs from `env -i` with a scratch HOME and no MoneyWiz app, and fails (exit 3, file names only) when an output holds real data or a MoneyWiz table name that is not a fixture or engine word (`tests/parity/known-names.sh`). The pre-push gate scans only for employer names, not money data |
| 2026-11-04 | `check.sh` in a fresh worktree fails 3 `app_actions` tests ("cashcast-engine: No such file") with `CARGO_TARGET_DIR` set | `bin/cashcast` looks for `engine/target` in its own checkout: link it (`ln -s <main checkout>/engine/target engine/target`) instead of setting `CARGO_TARGET_DIR` |
| 2026-11-05 | the parity self-test took ~10 min; the fixture was not the cause | `snapshot` ran a sed and an awk per file (~4,000 per run); keep per-file work batched (`snapshot_text`). The self-test runs its cases in parallel and checks the goldens once (`PARITY_CACHE_DIR`) |
| 2026-11-06 | `parity-test` fails on every PR the day after goldens were recorded (`- [ ] <NOW>: Verify the monthly transfer`) | `rust-parity.sh` turned every real-today date into `<NOW>`, also a task date derived from the fixed `--today`; it now replaces only `fetched_on`/`as_of`/`generated_on` stamps and table cells (harness test 13) |
| 2026-11-13 | A knowledge fact checked today would read `"as_of": "<NOW>"` in a golden and fail the next day | `rust-parity.sh` keeps an `as_of` whose next line is `"review_by"` (a fact); balance and report stamps still read `<NOW>` (harness test 14) |
| 2026-11-06 | a real MoneyWiz sync sent 1,397 links and MoneyWiz saved 0 | a "went to negative balance" alert blocked MoneyWiz, and it dropped every later link; the sync now sends in confirmed batches and stops (playbook item 15). In the background MoneyWiz may show new rows before it writes them to disk: never resend until a dry run shows them missing |
| 2026-11-08 | parity `--record` leaves goldens unchanged after a settings or CLI change | the harness runs the release binary: `cargo build --release` first, then record |
| 2026-11-09 | Amazon "Start chatting now" opened nothing the Chrome tools could see | the chat opens in a pop-up outside the tab group; ask the person to open it as a tab in the group (`skills/refund` step 4) |
| 2026-11-15 | `parity` fails with the new command missing from `help`, though `--record` included it | a worktree that links `engine/target` shares the release binary with the main checkout: a build there replaces yours, and cargo in the worktree still calls its own build fresh and does not copy it back. Before a parity record or the full gate, give the worktree its own target (`rm engine/target`, then `cargo build --release`) |
| 2026-11-15 | a new CLI command fails `repo` twice after the parity record | a new action also changes `web/src/test/fixture.json` (run `scripts/web-fixture.sh`), and a new judge eval needs its two rows in `evals/labels.yaml` |
| 2026-11-16 | `moneywiz sync --resend` sent 0 links though a dry run after a quit still listed the stock plan start correction ("sent today, not sent again until tomorrow") | a confirm that ran while MoneyWiz was still writing saw the start link under another amount and marked the old id seen; `--resend` now retries every link sent today that MoneyWiz lacks (test `resend_retries_a_link_sent_today_that_was_marked_seen`) |
| 2026-11-16 | a new file format passed its tests but failed the gate three times | a format name also goes in `engine/src/core/health.rs` `FORMATS`, `web/src/test/fixture.json` (`scripts/web-fixture.sh`) and the parity goldens (`cargo build --release`, then `scripts/rust-parity.sh --record`) |
| 2026-11-16 | `moneywiz weekly` said "saved 11 of 11", but hours later a dry run still listed the same 11 rows to add | MoneyWiz had not kept them. Quit MoneyWiz (Quit flushes to disk), then dry-run again. Only if the rows are still listed, reopen MoneyWiz and run `moneywiz sync --resend`. Quit once more and dry-run to confirm 0 to add |
| 2026-11-16 | the long Wise history from `api download wise` shrank back to a year after a `moneywiz sync` | the sync refreshed Wise itself with a fixed 365 days; every caller now goes through `api::download` (test `only_download_picks_the_wise_history_length`) |
| 2026-11-16 | `check.sh` step `web-types` fails in a worktree: "Cannot find module 'recharts'" | the worktree's `web/node_modules` predates a dependency; run `cd web && pnpm install --frozen-lockfile` |
| 2026-11-16 | a rebuilt engine's `moneywiz sync --dry-run` printed the old plan | the report cache key had no engine build; it now holds the binary's size and time (`engine_build`). A stale report reads "Cached:" at the end |
| 2026-11-16 | a stock plan start correction counted a MoneyWiz row twice | loose matching pairs rows up to 12 days before the first bank row; code that sums "rows before the period" must skip `matched_mw` |
| 2026-11-16 | rows the sync "sent" never reached MoneyWiz, and a balance stayed off by a fixed amount | they were transfers with a held account (they wait with it), and the start was wrong; read `moneywiz diagnose` "Gap by month" and "Statement chain" before blaming MoneyWiz. A flat gap is a wrong start; a statement break is a missing month |
| 2026-11-16 | a new MoneyWiz test sent 5 synthetic links to the owner's real MoneyWiz (none saved: the accounts did not exist) | a test that calls `moneywiz::sync` with apply on needs `fake_app` (it sets `CASHCAST_MONEYWIZ_OPEN`); `sync` now refuses to send to the real app unless it read the app's own database |
| 2026-11-16 | E*TRADE stock plan showed MoneyWiz 33.29 vs bank 0.00 | the 0.00 came from At Work "Current Account Value", which leaves cash out; the statements' closing cash is the balance now (`pdf/etrade.rs`). The Documents list defaults to 90 days: set Timeframe to Last 12 Months |
| 2026-11-16 | a fresh CIBC download's balance was 173.49 below its rows | CIBC downloads leave out the last 3 business days, but the balance counts them. A day or two out of sync is fine (owner): the next Sunday download holds the row and the sync settles it. No hold feature |
| 2026-11-16 | an export moved from the inbox back to `~/Downloads` reappeared in the inbox | a real `moneywiz sync` first collects exports from the last 60 minutes of Downloads (top level only); park a file you take out in a subfolder such as `~/Downloads/cashcast-duplicates/` |
| 2026-11-16 | Chrome saved 1 of 9 statement PDFs, then all 8 blocked ones twice after the owner allowed multiple downloads | Chrome queues blocked automatic downloads and releases them on "Allow": move "name (1).pdf" copies out of the inbox. A zsh glob with `" ("*").pdf"` fails to parse: use `*\ \(*\).pdf` |
| 2026-11-16 | Wealthsimple chequing and card showed "matches the bank" at a balance the site did not show; MoneyWiz held the same rows twice | two downloads joined by `[moneywiz.export_aliases]` kept each shared row twice (each row's key held its file name), and the sync sent both. `apply_aliases` now keys a renamed row by its target (test `overlapping_alias_downloads_keep_one_copy_of_each_row`). Check a balance against the live site, not only the dry run |
| 2026-11-16 | a new Sun Life download added 6 rows that sum to 0.00 | "Transfer (interfund)" legs are fund switches inside the plan: the cash reader now skips them (test `sunlife_interfund_transfer_makes_no_cash_rows`). Reach the old member pages through the new site's Manage plan menu: a direct link shows a maintenance page |
| 2026-11-17 | `edit --fixes` with new trades and a holding price failed "a holding's price changed since the plan was made" | a new trade moves its holding's price inside the writer; the writer now checks every holding before any change. the engine now runs the writer from its own checkout (`writer_dir`); `CASHCAST_MONEYWIZ_WRITER=<built writer>` still overrides it |
| 2026-11-17 | `moneywiz sync` sent links to an account and MoneyWiz saved none, with no alert, twice | the account was archived in MoneyWiz, and MoneyWiz drops links to it; the sync now warns. Run `moneywiz edit --account NAME --archived no`, sync, then `--archived yes` |
| 2026-11-17 | the Sunday scheduled run: `api download`, `moneywiz sync` and `sync status` failed "needs a person's approval" | project `ask` rules prompt even in bypass mode, and nobody answers a scheduled run; the owner removed those three (consent grants still guard them). `request_credentials` needs an attended run: the run pings the owner and waits up to 30 min |
| 2026-11-17 | a scheduled `weekly` stopped 7 steps right after a `git pull` | `bin/cashcast` built only a missing engine; it now rebuilds when `engine/src` or Cargo files are newer, and leaves a linked `engine/target` alone (`tests/repo/launcher.sh`) |
| 2026-11-22 | a new person with only a paycheque read "you earned $0.00" and got a "spending above income" task | salary rows go to `flows_cents` (`salary`, `salary_off_cycle`), not `income_cents`; code that totals income must add both (test `spending_counts_salary_as_earned`) |
| 2026-11-22 | `moneywiz sync` held a deposit into an investment account ("moves MoneyWiz away from the bank's balance"), and `edit --fixes` held the buy it paid for | the gate counted the deposit but not the buy `edit --fixes` adds; it now counts pending broker trades whose cash leg the export holds (test `a_deposit_spent_on_a_buy_the_edit_adds_is_not_held`) |
| 2026-11-22 | "Not up to date" listed MoneyWiz accounts named `Download (1)` and `accountactivity`; `edit --fixes` planned 382 category edits every week | TD and CIBC CSVs name no account: download QFX (the sync now lists such files apart). Row edits MoneyWiz already shows are now counted as "already done" |
| 2026-11-22 | `request_credentials` with 8 logins failed "invalid schema"; PayPal sign-in hung on a passkey prompt | at most 4 logins per call; PayPal needs the owner's Touch ID (task `paypal-passkey-signin`). Kraken exports a ZIP: unzip it before `fetch collect` (task `kraken-zip-and-prices`) |
| 2026-11-22 | `cards missed` checked 0 charges in a week with many card rows | the forecast import ended weeks earlier and uses export account names; `cards missed` now reads MoneyWiz rows mapped through `[moneywiz.accounts]` (task `cards-now-fresh-rows` for `cards now`) |
| 2026-11-22 | `moneywiz edit --fixes` exited 3 "MoneyWiz did not quit" | an open MoneyWiz window blocks the scripted quit (-128); nothing was written. Ask the owner to quit MoneyWiz (Cmd-Q), then rerun. The Sunday weekly writes MoneyWiz without asking, but never closes its windows by keystrokes |
| 2026-11-22 | `moneywiz categorize` refused: "moneywiz-edits.toml exists", though every edit in it was already in MoneyWiz | nothing moved an applied edits file out of the inbox, and agents may not touch the inbox; categorize now parks a file MoneyWiz already shows in full as `moneywiz-edits-applied-YYYYMMDD.toml` (test `categorize_parks_an_edits_file_moneywiz_already_shows`) |
| 2026-11-22 | MoneyWiz showed 400+ "pending" rows at the top of All transactions | the writer added broker trades with no status (MoneyWiz `status` 1 = pending, 2 = cleared); it now saves them cleared, and `edit --fixes` clears pending rows (test `fixes_mark_pending_rows_cleared`) |
| 2026-11-22 | 8 "cashcast fix" offsets for $1 card checks MoneyWiz already held | a charge of $1 or less and its same-day reversal (same account and text) is skipped on the bank side and the MoneyWiz side (tests `a_same_day_charge_and_its_reversal_are_not_bank_rows`, `a_card_check_pair_moneywiz_holds_needs_no_bank_row`) |
| 2026-11-22 | the transfer plan said to pay a card in full that the owner had already paid | it used a 6-day-old download's balance; skill step 2c: compare each sync balance with the live site and plan transfers from live balances only |
| 2026-11-22 | a test with a bare `PATH` still found the real `jq` and `curl` (and ran the real rustup installer) | this Mac's `BASH_ENV` hook prepends tool folders to every bash; run such tests with `/usr/bin/env -i` (`tests/repo/install-tools.sh`) |
| 2026-10-02 | the pre-push privacy gate flags a word | do not add accounting-file formats that match it; see memory |

## Cloud sessions

A cloud session cannot reach the owner's Mac, data or accounts. When the rest of
a goal needs them (a real export, the MoneyWiz app, a consent grant), do not wait
or keep asking:

1. Push the work to the session branch.
2. Write each unfinished item as a task in `TASKS.md` on `main`, with its steps
   on the Mac and the pre-registered fields.
3. Stop with one short message: what is done, and which task ids are left.

## Delivery

- Branch: `feat/…`, `fix/…`, `docs/…` from `main`. Conventional commits.
- Gate: `scripts/check.sh` green, plus `bash …/check-setup.sh . --profile assistant --strict`.
- PR body: `## Summary` (2–3 sentences: why), `## Details`, `## Docs`, a
  validation transcript, and the agent footer.
- Merge on green CI with a squash merge, then delete the branch.

## Docs upkeep

A behavior change updates its doc in the same PR (routing table above). A
gotcha that cost time gets a dated row in `## Gotchas`, or becomes a test.
New `docs/*.md` files get a row in [`docs/README.md`](docs/README.md);
`scripts/check-docs.sh` enforces it.
