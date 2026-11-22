# Observability

How to see what the app did, what broke, and how fast it is, without reading
anyone's financial data.

## Logs

Two logs, both local and never committed:

- **App events**: `private/users/<name>/logs/events.jsonl`, written by
  `engine/src/core/events.rs`. One JSON object per line.
- **Gate events**: `.metrics/events.jsonl`, written by `scripts/check.sh`.

Rules for app events:

- Content-free. String fields are allow-listed (`scope`, `command`, `story`,
  `error_type`, `format`, `service`, `action`, `risk`, `section`, `user_kind`,
  `result`). Each must match `^[a-z0-9][a-z0-9:._/-]{0,63}$`, otherwise the
  field is dropped. Numbers are counts and milliseconds.
- `CASHCAST_LOG=off` turns logging off. The unit tests run with it off, and
  `engine/tests/app_users.rs` checks the filter.

| Event | When | Fields |
|---|---|---|
| `command` | every CLI command finishes | `command`, `result`, `ms` |
| `report_shown` / `report_withheld` | the output gate decides | `command`, `scope` |
| `consent_granted` / `consent_declined` / `consent_revoked` | consent changes | `scope` |
| `telemetry_ask` | the start screen asked whether to share usage counts | `result` (yes, no, skipped) |
| `user_created` | `users add` / `users adopt` | `user_kind` |
| `section_built` | each strategy section runs | `section` |
| `action_planned` | `bin/cashcast action` | `service`, `action`, `risk` |
| `balances_recorded` | `api record` saved balances read in the browser | `service`, `count` (no amounts) |
| `api_download` / `market_fetch` / `cloud_sync` | a consented network call | `service`, `result` |
| `reminders_sync` | Reminders written | `result` (`ok`, `failed`, `timeout` after 90 s), `created`, `updated`, `reopened`, `closed`, `removed`, `held` (removals held back), `completed` (ticked in Reminders) |
| `notion_sync` | Notion task pages written | `result`, `created`, `updated`, `closed`, `removed` (archived), `held`, `completed` (ticked in Notion) |
| `fetch_plan` | `fetch plan` listed services | `services`, `stale` |
| `fetch_collect` | Downloads moved into the inbox, unopened | `files` |
| `sync` | One `sync all` pass over the targets that are on | `result`, `targets`, `done` (tasks ticked elsewhere) |
| `sync_install` | The hourly LaunchAgent loaded or removed | `result` (`ok`, `failed`, `removed`) |
| `calendar_sync` | Calendar events written | `result` (`ok`, `failed`, `timeout` after 90 s), `created`, `updated`, `removed` |
| `change_done` / `change_reopened` | `changes done ID` / `changes undo ID` | none |
| `tasks_resolved` | `tasks resolve` ran | `resolved`, `blocked` |
| `weekly` | `bin/cashcast weekly` finished | `overlaps`, `failed_steps` |
| `task_verdict` | `tasks verdict` recorded an agent check | `result` (`pass`, `fail`), `source` (`local`, `browser`) |
| `optimize` | `bin/cashcast optimize` ran | `trades`, `sells` |
| `invest_plan` | `bin/cashcast invest-plan` ran | `moves`, `do_nothing_wins` |
| `start` | `bin/cashcast` (no command) ran | `overdue`, `changes` |
| `web_started` | `bin/cashcast web` starts | none |
| `notion_import` | `notion import` copies a Notion export into `docs/` | `pages`, `tables`, `files`, `skipped` |
| `moneywiz_sync` | `moneywiz sync` hands rows to MoneyWiz | `result`, `sent` |
| `moneywiz_coverage` | `moneywiz coverage` measures how much of MoneyWiz the exports would add | `covered`, `rows`, `covered_recent`, `rows_recent` (last 12 months) |
| `forecast_scored` | `review` scores past spending predictions (running totals; `proof field` reads the latest) | `checked`, `misses` |
| `cashback_scored` | a strategy run plans cash back (net of the plan / net of the best single card held) | `ratio`, `cards` |
| `moneywiz_edit` | `moneywiz edit` (or a sync with `fixes = "edit"`) runs the Core Data writer | `result` (`ok`, `failed`, `needs_checksum`), `changed` |
| `moneywiz_migrate` | `moneywiz categories migrate` or `rollback` runs a category migration through the Core Data writer | `result` (`ok`, `failed`, `needs_checksum`), `changed`, `migration` (file name), `rollback` |
| `moneywiz_probe` | `moneywiz probe` checks the ways in for edits | `applescript`, `shortcuts`, `core_data`, `window` |
| `check.step` | each step of `scripts/check.sh` | `name`, `seconds`, `ok`, `cached` |

## Field proof

`engine/target/release/cashcast-engine proof field` turns this machine's app events into
totals for the README Proof section: runs, ok runs, errors, median run time,
tasks closed by `sync all`, rows added by `moneywiz sync`. Counts only. The
machine shows as `m-` plus a hash of a random salt in `private/proof/salt`,
never a host or user name. The totals go into `docs/proof/metrics.json` on
this machine only; nothing is pushed or uploaded. How a second machine's
totals reach the repo is not decided yet.

Opt-in telemetry (`engine/src/core/telemetry.rs`) can send allowlisted fields of
this log to PostHog EU, only with consent `net:usage-counts` and a key the
owner stores. A person at a real terminal is asked once on the start screen
(y/n, at most 3 times). owner stores. `bin/cashcast telemetry` shows its status; see the exits table in
[privacy.md](privacy.md#exits).

## Errors

- Every caught error writes `logs/errors.jsonl`: `command`, the `story` it
  breaks (map in `events.STORIES`), and `error_type`. The message is never
  logged, since it could quote user content.
- `bin/cashcast doctor` lists errors from the last 30 days by story. A story
  with errors is broken: fix it before new work.
- No external error reporter. Financial data rules this out (decision 0004).

## Metrics

| Metric | Goal | Pivot | Command |
|---|---|---|---|
| Net yearly gain found (VISION success) | ≥ $800 for the owner | < $200 for 3 reviews in a row | `bin/cashcast strategy` → "Net:" line + alternatives (owner reads it) |
| Reports shown to an agent without consent | 0 | any | `scripts/e2e.sh us-06` |
| Errors per day, by story | 0 broken stories | a story failing 2 days running | `bin/cashcast doctor` (reads `logs/errors.jsonl`) |
| Import failure rate | < 5% of imports | > 20% | `jq -s '[.[]|select(.event=="command" and .command=="import")]|(map(select(.result=="failed"))|length)/(length|if .==0 then 1 else . end)' private/users/<name>/logs/events.jsonl` |
| Strategy run time (p50) | < 2 s | > 10 s | `jq -s '[.[]|select(.event=="command" and .command=="strategy")|.ms]|sort|.[length/2|floor]' private/users/<name>/logs/events.jsonl` |
| Gate time (`scripts/check.sh fast`) | < 60 s warm | > 120 s | `scripts/metrics.sh` |
| Cost per run | $0 (no paid services) | any paid call | none: the app calls no paid API |
| Eval judge agrees with owner labels | ≥ 5 of 6 labelled cases | < 4 of 6 | `evals/run.sh --agreement` (local Ollama; labels in `evals/labels.yaml`) |

## Health check

`bin/cashcast doctor` checks Rust, the engine build,
Apple Numbers, users and profiles, recent errors by story, the
knowledge base and the connector specs. Each failing line says how to fix it.
`scripts/doctor.sh` is the repo-level check (tools, `.metrics/`, `private/`,
`config/models.yaml`) and then runs the app checks.

## Checkup and caching

`scripts/checkup.sh` is the one repo-wide checkup. `scripts/check.sh` skips a
step that passed while the repo's inputs are unchanged (a hash of tracked,
staged, unstaged and untracked files). Any change re-runs every step;
`NO_CACHE=1` forces a full run.

## Speed review

Monthly (`RECURRING.md`), and after any change to the gate: run
`scripts/metrics.sh`. If a step got slower or the gate is over budget, file a task.

- **Gate budget**: `scripts/check.sh` finishes in under 120 seconds warm (the engine build is cached by cargo).
