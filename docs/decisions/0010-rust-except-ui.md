# 10. Everything except the web UI moves to Rust, one proven step at a time

- **Status**: accepted; amends 0002
- **Date**: 2026-11-01

## Context

Decision 0002 kept Rust for raw rows and Python for everything else. On
2026-11-01 the owner asked for no Python in the repo: every part except the
TypeScript UI (`web/`) moves to Rust. The Python code is about 14,800 lines
(`scripts/python-lines.sh`), with years of edge cases that the tests only
partly cover. A rewrite in one go would lose some of them without notice.

## Decision

- The port is a strangler fig: `bin/cashcast` stays the one entry point, and
  each module group moves behind it on its own. The steps are the
  `rust-*` tasks in `TASKS.md`, from the core (paths, users, consent) to the
  CLI switch, then the tests and scripts.
- Each step is proven by `scripts/rust-parity.sh` before its Python is
  deleted. The harness runs every command in `tests/parity/commands.txt` on
  the synthetic users in `tests/parity/fixture/`, once with the Python code
  (`CASHCAST_IMPL=python`) and once with the candidate (`CASHCAST_IMPL=rust`).
  It compares the output, the exit code, the files in `data/` and `out/`,
  and the log events, after removing times and durations. A step ships only
  with 0 mismatches.
- Step 1 adds the routing: with `CASHCAST_IMPL=rust` (the default once a
  command is ported), `bin/cashcast` runs the commands the engine lists as
  ported in Rust, and the rest in Python. Until then both sides run the same
  Python code, so the harness must print 0 (it does, on 3 runs out of 3).
- Each step also records two numbers: the median time per command
  (`scripts/rust-parity.sh --runs 5`, written to `.metrics/parity.jsonl`) and
  the Python lines left (`scripts/python-lines.sh`).
- The privacy rules do not change: only the reader modules parse raw rows,
  API clients save downloads unparsed, and the consent gate guards every
  report, in whichever language the code is.

## Consequences

- Step 9, part 11 (2026-11-03) finished the port: `scripts/python-lines.sh`
  prints 0. Before the delete, `scripts/rust-parity.sh --record` froze the
  Python side's outputs as goldens in `tests/parity/golden/` (206 commands,
  the strategy sections and the web routes); the harness now runs only the
  Rust CLI and compares with them. `src/cashcast/`, the Python in
  `forecast/`, `pyproject.toml`, `uv.lock`, ruff and pyright are gone, and
  `bin/cashcast` execs the engine with no exit-64 fallback and no
  `CASHCAST_IMPL`. The Python tests became engine tests
  (`engine/tests/app_*.rs`, `forecast_model.rs`, `proof.rs`, tests in
  `engine/src/tasks/sync.rs`), shell stories (`tests/e2e/us-*.sh`) and repo
  checks (`tests/repo/*.sh`). `scripts/proof.py` is `cashcast-engine proof`,
  `evals/run.py` is `evals/run.sh` (bash, yq, jq, curl), the web fixture is
  `scripts/web-fixture.sh`, the xlsx fixtures are built by
  `engine/tests/xlsx_fixtures.rs`, and `cashcast.health` is
  `cashcast-engine health`. Dropped, with their reason: the per-section
  `author` and `sources` metadata (only a test read it), the Python-vs-Rust
  card category test (one side is left), and the 90-second frozen-app
  timeout tests.
- A new command needs a row in `tests/parity/commands.txt` and a recorded
  golden (`scripts/rust-parity.sh --record`, with the diff in the PR).
- One timed run is noisy on a busy Mac (the two sides differed by up to 40 %
  on the same code), so a speed claim uses `--runs 5` medians.
- The forecast writes and checks a spreadsheet with openpyxl and pycel.
  Step 3 (2026-11-02) writes it with `rust_xlsxwriter` and evaluates its
  formulas with `engine/src/forecast/formula.rs` (pycel's ROUND and Python's
  compensated `sum`). Two libraries never write the same zip bytes, so the
  harness compares a workbook by its cells: each sheet's values and formulas
  (`cashcast-engine xlsx-cells FILE`), not its byte size (owner's decision,
  2026-11-02). Styles and the chart are not compared.
- Step 3 routes in `src/cashcast/engine.py` `forecast()`: it runs
  `cashcast-engine forecast-run CMD --home DIR` first and the Python script
  only on exit 64 (an input the Python model would fail on). `forecast/` stays
  until step 4: `strategy/forecast.py` imports `model.py`.
- Step 9, part 10 (2026-11-03): the Rust CLI never goes back to Python.
  `engine/src/core/argparse.rs` ports the part of Python 3.13's argparse the
  CLI uses (the A/O pattern, prefixes, `--name=value`, `--`, choices,
  `type=int`, `-h` per command, usage errors and help wrapped to the terminal
  width), and `engine/src/core/commands.rs` declares the parser as
  `cli.parser()` did. The action catalog (`actions --json`, the web app's
  Actions view) is generated from it; `engine/data/actions.json` is gone. A
  differential check of 9,800 random argument lists at six widths found no
  difference with Python's parser. Where Python stopped with a traceback
  (a file that does not parse, a bad `--today` date, `web --port 70000`),
  Rust prints `error: MESSAGE` with Python's exception message and exits 1;
  the parity harness compares a traceback by that message. `forecast-run`
  exits 1 the same way instead of 64, so no Python model runs either.
  `bin/cashcast` still honors exit 64 and `CASHCAST_IMPL=python` until the
  Python is deleted in the next part.
  Review fixes: both sides now pass `--today` to engine subprocesses as
  `YYYY-MM-DD` (a bad value stops with Python's ValueError, exit 1). A denied
  file (`AppError::io`) exits 5 with the sandbox hint, as Python's
  `PermissionError`. An int past 64 bits saturates, so the command's range
  check fails as Python's does.
- Step 9, part 9 (2026-11-03) ported the last `sync` actions: `all` (with
  `--allow-removals`), `install`, `uninstall` and `sheets`
  (`engine/src/tasks/sync.rs`), the Notion tasks client with `scrub`
  (`engine/src/tasks/notion.rs`) and `sheets_write` (`engine/src/api.rs`).
  `scrub` uses the `regex` crate: it keeps Python's leftmost-first
  alternatives and Unicode `\d`, `\s` and `\b`, so the patterns stay the
  same text; a hand-written scanner would need its own backtracking. The
  LaunchAgent is written with the `plist` crate in plistlib's sorted XML.
- Step 9, part 8 (2026-11-03) ported `reminders sync` and `calendar sync`.
  The two JXA scripts moved to `engine/data/*.js`: Rust embeds them and
  Python reads the same files, so there is one copy of each.
- Step 9, part 7 (2026-11-03) ported `status` and the start screen's daily
  telemetry send, so a bare `bin/cashcast` never goes back to Python.
- Step 9, part 6 (2026-11-03) ported `strategy --market` and `invest --market`
  (Alpha Vantage, `api::market`, the same per-day cache in `data/market/`) and
  the Wise download inside `moneywiz sync`.
- Step 9, part 5 (2026-11-03) ported the network calls: `api balances`,
  `download`, `etrade-auth` (`engine/src/api.rs`) and `telemetry send`. They
  go through `engine/src/core/http.rs`, which runs the system `curl` with the
  URL, headers and body on stdin (`-K -`), so tokens never reach the command
  line, and no TLS crate is added. SHA-1, SHA-512, HMAC and base64 are in
  `digest.rs`, checked against the FIPS and RFC test vectors.
- Step 9, part 4 (2026-11-03) ported `api record` (`engine/src/tasks/balances.rs`):
  balances read in the browser, no network. An API error exits 4, as in Python.
  `api balances`, `download` and `etrade-auth` still call the services from Python.
- Step 9, part 3 (2026-11-03) ported `actions`. The Rust CLI reads the
  catalog from `engine/data/actions.json`, which `tests/unit/test_actions_catalog.py`
  keeps equal to the Python parser's until the Rust parser owns it.
- Step 9, part 2 (2026-11-03) ported `transfers`, `first-error`, `numbers`
  and `cards now` (with `card_inputs`). Left: `api` and `actions`, the HTTPS
  client, then deleting the Python.
- Step 9, part 1 (2026-11-03) ported `help`, `stop`, `init`, `import`,
  `forecast`, `backtest`, `totals`, `columns` and `holdings` to the Rust CLI.
  When `forecast-run` exits 64, the Rust CLI runs the Python model itself
  (no second import). Left for part 2: `numbers`, `first-error`, `cards`, `transfers`,
  `api` and `actions`, the HTTPS client, then deleting the Python.
- Step 8 (2026-11-03) ported the web server (`engine/src/web.rs`): the token
  check, the JSON API, the actions runner and serving `web/dist`. `bin/cashcast
  web` and the start screen run it. The server passes its token to the
  commands it runs as `CASHCAST_OWNER_UI`; both engines honor it only when it
  matches `private/out/web.json`. `cashcast-engine web-get` answers one route
  for the parity harness and `scripts/web-fixture.py`.
- Step 7, part 2 (2026-11-03) ported `notion import` (`engine/src/docs.rs`,
  zip and folder exports, nested zips, links rewritten) and the MoneyWiz app
  glue (`engine/src/tasks/moneywiz_app.rs`): the applying sync, `moneywiz
  edit`, `moneywiz categories` (list, suggest, new, migrate, rollback), the
  probe and a dry run with `fixes = "edit"`. The Swift writer stays. An
  applying sync that may download from Wise (consent `actions:api`) still
  goes to Python until the HTTP client lands in step 9. The parity harness
  now also compares each user's `docs/` folder.
- Step 7, part 1 (2026-11-02) ported `moneywiz status`, `moneywiz
  coverage` and `moneywiz sync --dry-run`. The engine runs itself for the
  report, as Python did. A dry run with `fixes = "edit"` still goes to
  Python, because it asks the Core Data writer. The rest of step 7 stays a
  task: the applying sync, edits, categories, the probe and `notion import`.
- Step 6 (2026-11-02) ported `connectors list|show`, `action SERVICE ID`
  (with the money-step queue in `data/manual-tasks.json`), `fetch plan` and
  `fetch collect` to `engine/src/tasks/connectors.rs` and `fetch.rs`. The
  `api` commands stay in Python: they call the services over the network,
  and `api record` writes balances through the same client module. They
  move with the web server or in step 9. `fetch collect` has no parity row,
  because it reads the real Downloads folder; a unit test covers it.
- Step 5 (2026-11-02) ported `tasks` (list, resolve, verdict), `changes`,
  `reminders preview`, `calendar preview`, `sync status` and the start
  screen to `engine/src/tasks/`. What leaves the process stays in Python for
  now: the osascript syncs (`reminders sync`, `calendar sync`, `sync all`,
  `sync install`), the Notion task client, and a start screen that would
  send usage counts (it goes back to Python before any output). The start
  screen still starts the Python web app (`python -m cashcast web`).
- Step 4 (2026-11-02) ported the situation, all 16 strategy sections,
  `optimize` and `rrsp` to `engine/src/strategy/` and routes `strategy`,
  the section commands, `net-total`, `invest-plan`, `rrsp` and `optimize`.
  Log events wait until every section has built, so a section that falls
  back (exit 64) leaves no half log. `strategy --market` stays in Python.
  The Python sections stay as side A until step 9.
- Step 1 (2026-11-02) ported the core to `engine/src/core/` and added the
  routing: `bin/cashcast` runs `cashcast-engine cli ARGS…`, which exits 64
  before any side effect when a command or argument form is not ported, and
  Python runs it then. A step cannot delete its Python while other Python
  imports it and the harness's reference side (A) runs it. So each step
  deletes only Python nothing else uses; the shared core goes in step 9.
- The port is finished when `scripts/python-lines.sh` prints 0 and the full
  gate passes on a machine with no Python.
