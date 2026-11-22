# Privacy

The rule: a person's financial data stays on their computer. It leaves only
through an exit listed below, and only after that person says yes.

## Where data lives

| Path | What | Who reads it |
|---|---|---|
| `private/users/<name>/inbox/` | raw exports | the engine only |
| `private/users/<name>/data/` | aggregates (`summary.json`, `holdings.json`), journal, API balances and balances recorded from the browser (`api-balances.json`), market cache | the app; agents never open it |
| `private/users/<name>/out/` | workbook, `strategy.md`, withheld reports | the owner; agents never open it |
| `private/users/<name>/profile.toml`, `cashcast.toml` | the person's answers | the owner, and an agent helping them |
| `private/users/<name>/consent.toml`, `.env` | grants, API tokens | the owner only; agents never edit them |
| `private/users/<name>/logs/` | content-free events and errors | anyone debugging |
| MoneyWiz's own database (`~/Library/Containers/com.moneywiz.*`) | your MoneyWiz ledger | the engine, from a temporary copy it deletes. Written only by `moneywiz edit` through Core Data, with your consent and settings (decision 0007). Agents never open it |
| `private/users/<name>/docs/` | the owner's notes and records, for example the Notion copy in `docs/notion/` | the owner, in the web app's Documents view; agents never open it |
| `data/moneywiz-sent.json`, `data/moneywiz-key`, `out/moneywiz-fixes.md` | keyed hashes (HMAC-SHA-256) of every link sent to MoneyWiz, with the day sent, never the rows; the random key for them; your list of rows to fix and conflicts. All mode 600 | the app; the owner only |
| `inbox/moneywiz-edits.toml`, `data/moneywiz-plan.json`, `data/moneywiz-backups/` | your edits; the resolved plan; database copies from before each edit | the app; the owner only |
| `private/out/web.json`, `private/out/web.log` | the running web app's port and link (the link holds the access token); its error log, never the link | the owner (`bin/cashcast` prints the link in a terminal); agents never open it, and the web app never prints the link to an agent |
| `private/settings.toml`, `private/users/<name>/data/changes-done.json` | app setting overrides; tasks you marked or ticked done (`changes done`, Reminders, Notion) | the app and the owner |
| `private/users/<name>/data/sync-state.json`, `logs/sync.log` | each sync target's last run, result and a hash of what it was sent; the hourly run's output | the app and the owner |
| `private/users/<name>/paperwork/` | citizenship paperwork and application, sealed (`.sqv`, AES-256-GCM, key in the Keychain); `citizenship/evidence.html` for 10 minutes | the owner, in Terminal (`--reveal`); agents get only the redacted `paperwork` and `citizenship` output and never open the folder. Nothing in it ever leaves the computer; `paperwork sources --live` only reads public canada.ca pages (threat model) |
| `private/` (no `users/`) | the original single-user layout, the user "default" | as above |

Folders are mode 700, files 600. Git never tracks `private/` (the
`.gitignore` is an allowlist).

## Exits

Every way data can leave the computer, the consent scope that opens it, and
what goes out.

| Exit | Scope | What leaves | Where it goes |
|---|---|---|---|
| Reports printed to an AI agent | `agent:aggregates` | totals, account and category names, counts; payee groups only as numbers ("Deposits #n", "Payments #n") with their usual amount and last date | the agent's model provider |
| Strategy printed to an AI agent | `agent:strategy` | card routing, targets, balances, room, checklist | the agent's model provider |
| `columns` | `agent:aggregates` | header names, cell shapes (`date`, `num`, text masked to `a`/`9`) | the agent's model provider |
| `first-error` | `agent:raw-row` (1 day) | one raw export row | the agent's model provider |
| Browser actions | `actions:browser` | account pages on screen | the agent's model provider |
| Browser checks (`tasks verdict --source browser`) | `agent:browser-read` (35 days, so one grant a month covers the Sunday runs; `weekly` warns under "Bank site access" a week before it ends) | the finance site pages an agent reads to check a task; statements it downloads go to your private inbox and only the engine parses them; the saved verdict holds pass or fail and a reason with no amounts | the agent's model provider |
| API calls | `actions:api` | your token, to the service that issued it | that service (it already holds the data) |
| Crypto prices (`api balances kraken`) | `actions:api` | asset pair names only (for example `XBTCAD`), no key, token or amount | Kraken's public ticker |
| `sync sheets` | `cloud:sheets` | the forecast table | your Google Sheet |
| Live rates (`knowledge refresh`, daily from `cashcast` and `sync all`) | none: setting `live.refresh` (on by default) | fixed Bank of Canada series IDs only, plus your IP address and the request time (at most once a day); no amount, account, ticker or name | the Bank of Canada Valet API (decision 0015) |
| Market data | `net:market-data` | ticker symbols only | the market-data provider (Alpha Vantage); Yahoo Finance chart prices for `moneywiz diagnose` net worth |
| `moneywiz sync` | `local:moneywiz` | the export rows MoneyWiz lacks (date, amount, payee text, category), plus corrections | MoneyWiz on this Mac, and its cloud if you turned on MoneyWiz sync |
| `moneywiz edit` | `local:moneywiz`, plus `[moneywiz] writer` and `writer_checksum` | your edits to amounts, categories, payees, memos, opening balances and categories | the MoneyWiz database on this Mac, and its cloud if you turned on MoneyWiz sync |
| `reminders sync` | `local:reminders` | task titles (with amounts) and due dates | Apple Reminders on this Mac, and iCloud if you sync Reminders |
| `calendar sync` | `local:calendar` | dated task titles (with amounts) and dates | Apple Calendar on this Mac, and iCloud if you sync Calendar |
| `sync all` (Notion) | `cloud:notion`, plus `[notion] tasks_database` and your own token | task titles with every amount removed, due dates, the task kind (read, settings, money) and done ticks; never notes, balances or documents | your Notion tasks database (decision 0009) |
| `optimize` | none | nothing: arithmetic on your own targets, cash and holdings | — |
| Actions view (`POST /api/actions/NAME`) | the same scopes as the command it runs | nothing new: it runs one CLI command for the owner, with the run's token, JSON only. Terminal-only commands (consent grant, the web app) are refused | — |
| `notion import` | none | nothing: it reads a Notion export file on this computer and makes no network call | — |
| Documents view (`bin/cashcast web`) | none | nothing: the page is served to this computer only, with the run's token. Its Content-Security-Policy blocks loading anything from the internet (images, fonts, scripts) | — |
| Model calls by the app itself | none | nothing: the app calls no model | — |
| Error reporting | none | nothing: errors stay in `logs/` | — |
| README Proof field totals (`cashcast-engine proof field`) | the owner runs it and commits the result | counts only from `logs/events.jsonl` under a salted machine label: runs, errors, run time, tasks closed, MoneyWiz rows added, forecast months scored and missed, the cash-back ratio vs. one card; never amounts or names | `docs/proof/metrics.json` in the repo |
| Usage counts (`engine/src/core/telemetry.rs`, opt-in) | `net:usage-counts`, plus a PostHog key the owner stores (`CASHCAST_POSTHOG_KEY` or Keychain service `cashcast`, account `posthog_key`) | allowlisted fields of `logs/events.jsonl`: event name, time, command, result, error type, section and scope codes, durations and counts, under a random install id; never amounts, balances, payees, service or account names, paths, host or user name. Person profiles and GeoIP off. The start screen asks once, y/n, at a real terminal only (never an agent, a pipe, CI or `DO_NOT_TRACK`; at most 3 times; a no is final); a yes records this consent with `via = "telemetry-ask"`. At most once a day, after a one-line notice. `bin/cashcast telemetry off` or `DO_NOT_TRACK=1` stops it; `telemetry send --dry-run` shows the exact body | PostHog EU (`eu.i.posthog.com`) |

What you type in chat goes to your agent's model provider. Prefer editing
`profile.toml` yourself for numbers you'd rather not type.

## How consent works

- `bin/cashcast consent grant SCOPE` prints what is shared, with whom, and
  until when. You confirm by typing the scope name at a real terminal
  (`/dev/tty`). An agent's shell has no terminal it can type into, so it
  cannot grant itself access.
- Grants expire (1 to 365 days, by scope). `consent revoke` ends one at once.
- You may also edit `consent.toml` by hand.
- When a report is withheld, it is saved to `out/last-<command>.md` for you,
  and the agent sees one line telling you how to share it.
- "Probably an agent" means: an agent environment variable is set, or the
  output is not a terminal. Piping a report to a file therefore withholds
  it too; the full report is in `out/last-<command>.md`.

**Limits.** The gate stops accidental sharing by cooperating agents. It is not
a sandbox against hostile code running as your user. Add these layers too:

- `.claude/settings.json` denies agent reads of `inbox/`, `data/` and `out/`.
- `AGENTS.md` forbids opening those folders.

## Actions on your accounts

- Agents never type passwords or 2FA codes. You sign in, or your password
  manager fills them after you approve.
- `money` actions (transfers, trades, payments, conversions, and anything that
  commits future payments such as autopay or a membership renewal) are never
  done by an agent. `bin/cashcast action` turns them into reminders.
- `settings` actions (payment card, autopay, account type) need your yes in
  chat for that exact change, right before the final click.

## Tests that hold this

- Canary leak tests: every synthetic description carries `CANARY…`, and the
  engine, e2e and strategy tests fail if one reaches any output or file.
- `engine/tests/app_users.rs`, `tests/e2e/us-06-consent.sh`: the gate,
  grants, expiry, and per-user isolation.
- `engine/tests/app_tasks.rs`: every money action is human-only.
- `engine/tests/app_api.rs`: cloud writes and market data need consent; only
  ticker symbols are sent.
