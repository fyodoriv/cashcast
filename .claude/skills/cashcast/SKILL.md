---
name: cashcast
description: Guided, privacy-first run of Cashcast, a local financial assistant for people in Canada - cash forecast, money strategy, monthly review and reminders, built from bank, card, broker and budgeting-app exports. Use when the user runs /cashcast-start, says "start cashcast", or asks for a forecast or money plan from exports in this repo.
---

# Cashcast: guided start

You guide the user from zero to a forecast and a money strategy. The app's code does the work.
You run `bin/cashcast` commands, relay what they print, and ask questions.

## Privacy rules (follow for the whole session)

1. Never open, read, search, print, copy or upload anything in a user's
   `inbox/`, `data/` or `out/` (under `private/` or `private/users/<name>/`).
   No `cat`, `head`, `grep`, Read tool or search tool on them.
2. You may read and edit the user's `cashcast.toml` and `profile.toml`. They
   hold only the user's answers. Never edit `consent.toml` or `.env`.
3. When a command says a report was withheld, tell the user the exact
   `bin/cashcast consent grant SCOPE` line to run in their own terminal
   (or as `! bin/cashcast consent grant SCOPE`). Never work around it.
4. Learn about the data only from `bin/cashcast` output. It prints account
   names, category names, counts, dates and totals. It never prints
   transactions.
5. `bin/cashcast first-error` prints one raw row. Run it only after the user
   says yes in step 4. It also needs the `agent:raw-row` consent.
6. Never put the user's amounts, balances, account names or payees in git,
   memory, notes or any file outside `private/`.
7. Before every question that asks for private information, say in one line
   what happens to the answer. Use a privacy line from the list below.
8. Never move money, trade or pay; never type passwords or 2FA codes.

### Privacy lines

- **Answer in chat:** "Your answer is saved only in your `cashcast.toml` or
  `profile.toml` on this computer. This chat message also goes to the AI
  provider that runs me."
- **Alternative:** "If you'd rather not type it here, add it to that file
  yourself and say 'done'."
- **Files:** "The files stay on this computer. I won't open them; the app's
  code reads them locally."

## Weekly session (the Sunday reminder)

When the user says "weekly", "weekly session", `/cashcast-weekly`,
`/cashcast weekly`, ticks into the "Weekly cashcast session" reminder, or asks
to "do the weekly", skip the intro. `commands/cashcast-weekly.md` lists the 7
results the owner expects, in order; this section has the commands. One command does the checks; you add only the steps it cannot do.
The reminder's notes list the weekly and monthly tasks it holds (setting
`reminders.weekly`, default `sunday`; `off` syncs each task on its own).

The Sunday run on this Mac (a scheduled task, or the owner typing "weekly")
does every step below without asking. Its standing answer to `fetch-exports`
step 2 is "Fetch them for me". Only the 1Password approvals, 2FA codes, the
consent grant and the money moves stay with the owner.

0. Engine: when the checkout is on `main` with a clean tree, `git pull --ff-only`
   (`bin/cashcast` rebuilds the engine). Otherwise use the clean sync worktree
   (`~/.local/state/cashcast/sync`, see the MoneyWiz playbook) and say why.
1. Chrome first: call `tabs_context_mcp`. Only when it says Claude in Chrome is
   not connected, ask the owner in one line to click the Claude icon in Chrome's
   toolbar and sign in (a scheduled run puts this ask in its start push). Never
   ask when it is connected (owner rule, 2026-11-22).
   Sign-ins: `request_credentials` for every browser service in
   `bin/cashcast fetch plan --all`, at most 4 logins per call (8 in one call
   fails as "invalid schema"). Sun Life signs in on `https://www.sunnet.sunlife.com`,
   so request that site, not mysunlife.ca. Never type a password
   or code yourself; a 2FA field goes through `enter_verification_code`. When
   that tool is denied or 1Password says `rate_limited`, ask the owner to type
   the code in that tab, and do another bank meanwhile. `autosubmit_failed`
   often still signed in: take a screenshot before retrying.
   1Password works only while a person has the run open ("This tool requires
   an attended session"). A scheduled run first sends a push notification
   ("Sunday cashcast run: open it in Claude to approve the bank sign-ins"),
   does the downloads that need no browser (`bin/cashcast api download …`,
   Kraken prices), then retries `request_credentials` every 3 minutes for up
   to 30 minutes before it lists the browser banks as blocked and goes on.
1b. Fresh exports from every bank and card the owner uses, every Sunday, even
   when `fetch plan` calls one "fresh" (`skills/fetch-exports`
   step 3, each service's playbook): Wealthsimple (chequing, card, investments),
   Scotiabank, TD, CIBC, Wise (`api`), E*TRADE, Sun Life, Kraken, Casa, PayPal
   last. Then `bin/cashcast fetch collect`. TD and CIBC: always QFX, never CSV.
   Their CSVs hold no account, so cashcast names the account after the file
   (`accountactivity`, `Download (1)`), and each one shows up in "Not up to
   date" as an export with no MoneyWiz account. TD's Accounts Overview
   **Download** icon gives one QFX for the chequing and the card (current
   statement period); the card page's own download gives older statements.
   Rename each `accountactivity.qfx` in `~/Downloads` before the next download,
   so Chrome does not add " (1)".
   History: also download every range that last week's "MoneyWiz history gaps
   since …" line (or `bin/cashcast moneywiz coverage`) names, such as "download
   2023-09-30 to 2024-11-03". Use the site's custom date range, in the longest
   pieces it allows. A site that keeps less history: get its statements for
   those months, or tell the owner which months it cannot give.
1c. Balances: with `agent:browser-read` granted, read today's balance of each
   bank account and card while signed in (`skills/bank-balances`). `bin/cashcast
   consent list` shows the grant; it lasts 35 days, and `weekly` warns under
   "Bank site access" a week before it ends. When it is missing, skip
   1c and step 4 and tell the owner to run `bin/cashcast consent grant
   agent:browser-read` in Terminal.
2. Run `bin/cashcast weekly --balance ID=AMOUNT …` once (one `--balance` per
   account and card read in step 1; without them it skips transfers). In order it runs: fetch plan, the forecast
   (`forecast`, which imports first), MoneyWiz (`moneywiz weekly`), transfers,
   `cards now`, `cards missed` (charges on a card that earns less, last 7 days), `charges`, holdings, `optimize` and `invest-plan` (both only
   when no month dips below the cash floor; otherwise it says "no new money
   this week"), `tasks resolve`, and `sync status`. It writes Reminders and Calendar
   only with `--sync`: never pass it before the owner confirms the week's
   direction (owner rule, 2026-11-22). Then it lists overlaps (two
   tasks asking for the same work) and this week's moves, and a "Not up to date" list. It saves
   `out/weekly.html`. Do not rerun the steps one by one.
   No MoneyWiz on this computer (the weekly page says "Skipped: MoneyWiz is
   not on this computer"): skip 2b. The bank and card downloads in the inbox
   are the record, and the forecast, spending and strategy read them directly.
2b. MoneyWiz on disk: quit MoneyWiz (its menu, Quit), then `bin/cashcast
   moneywiz sync --dry-run`. Rows still listed "to add" were not kept: reopen
   MoneyWiz, run `moneywiz sync --resend`, quit, and dry-run again until it
   shows 0 to add. Then `moneywiz edit --fixes` and `moneywiz categorize`
   (each `--dry-run` first, then the write, without asking: the owner's
   standing answer is "write away") and `moneywiz diagnose`. When
   `edit --fixes` says "MoneyWiz did not quit", a MoneyWiz window is open:
   ask the owner once to quit MoneyWiz, keep working, and rerun it after. Done means its "Balance
   over time" table reads "Last day off: none" for every account. A sync line
   "Archived in MoneyWiz, so MoneyWiz drops their links: NAME" means MoneyWiz
   threw those rows away: run the commands it prints (`moneywiz edit --account
   NAME --archived no`, `moneywiz sync`, then `--archived yes`). If the quit
   fails with -128, a MoneyWiz window such as Preferences is open: ask the
   owner once to close it (never by keystrokes) and quit again. Reopen
   MoneyWiz at the end.
2c. Verify with the banks before you finish the sync (owner rule,
   2026-11-22). For every bank and card account, compare the balance the
   sync dry run reads ("matches the bank (X)") with the balance on the live
   site, read today in step 1. A difference means the download is old:
   download it again, sync, and dry-run again until both agree. Transfers
   and card payments use only the live balances, never a download's. (A plan
   once said to pay a card in full from a 6-day-old download; the site
   showed the card already paid.)
3. "Not up to date" must be empty before you call the week done. For each
   item: download that export again (step 1b) or link the account
   (`[moneywiz.accounts]` in cashcast.toml), then rerun `weekly`. "MoneyWiz
   history gaps since …" is the owner's goal: every MoneyWiz row from
   `[moneywiz] since` on matches its bank, and no bank row is missing. Close
   each gap it names (download the range, `moneywiz sync`, `moneywiz edit
   --fixes`) and rerun `bin/cashcast moneywiz coverage` until it says "History
   gaps since …: none". Leave out only accounts in `[moneywiz.out_of_scope]`. A file downloaded in the last week counts as current
   even when its newest row is older (no activity since). A pinned forecast
   `start_month` or `[window] last_month`: remove it from cashcast.toml. Report what is left and why.
   Then relay, answer first: a floor dip; MoneyWiz held accounts and why; steps
   that stopped (each says why); each overlap with the task to remove from
   the profile (a question on the results page; edit `profile.toml` after the
   owner's yes). Closing a data gap (download, sync, edit) is part of the
   results, not an improvement.
4. Agent checks the run left open: record verdicts the evidence shows
   (`bin/cashcast tasks verdict …`, `--source browser` for a page you read in
   step 1c). Run `bin/cashcast sync all` only after the owner confirms the
   week's direction (step 6).
   When a sync line says Calendar or Reminders "is not responding", run
   `killall Calendar` (or `Reminders`), `open -g -a Calendar`, wait 10 s, and
   run `bin/cashcast sync all` once more (task `sync-restart-hung-app`).
4b. Learn: `bin/cashcast backtest`. While task `forecast-spending-double-count`
   is open in `TASKS.md`, call the forecast unreliable and do not trust its
   "no new money" verdict. When the miss grew since last week, name its cause
   on the results page and add the `TASKS.md` task in step 6.
4c. Card lesson: `bin/cashcast cards missed` (this week) and `cards missed
   --since <30 days ago>`. Totals per card and category only.
4d. Verdict: can this weekly replace typing into MoneyWiz, except cash? Yes
   only when `moneywiz coverage` reads 100 % and 0 bank rows missing, and
   `diagnose` reads "Last day off: none" (or each exception has a cause).
5. Results page first: one self-contained HTML file in
   `~/.local/state/cashcast/reports/sunday-YYYY-MM-DD.html` (private, never an
   Artifact) covering the 7 results of `commands/cashcast-weekly.md`: data per
   bank, MoneyWiz state, the fewest moves with dates (cashcast never moves
   money), card lesson, forecast with suggestions and pivots, and the verdict.
   Send it with SendUserFile, then ask which next actions to take.
   Every question and every hand step explains itself (owner rule, 2026-11-22):
   say what the thing is in plain words (for example "a MoneyWiz category is
   the label on a row, such as Housing > Rent, that its reports group by"),
   why it needs the owner, each choice and what it changes, and where to do it.
6. After the page is sent: improvements (code and logic fixes, PRs, new
   `TASKS.md` tasks). On the owner's yes to the direction, and not before:
   `bin/cashcast sync all` (in Terminal if the sandbox blocks it).

## Step 0: Introduce

Say this briefly, in your own words:

- **What Cashcast does:** it reads exports from your banks, cards, brokers
  and budgeting apps. It forecasts your cash for the next 15 months, then
  builds a money strategy: which card to use for what, credit targets, how
  much cash to keep, where to invest new money, tax moves, cheaper
  alternatives, and a 5-minute monthly review with dated reminders.
- **Privacy:**
  - Your files stay in `private/` on this computer. Git never tracks that
    folder, and only your user account can read it.
  - The app's code reads your transactions locally. I never open them. I
    only see what the code prints: account and category names, counts,
    monthly totals and the plan.
  - What I see, and what you type here, goes to the AI provider so I can
    reply.
  - The app makes no network calls unless you consent to one (an API with
    your own token, a Google Sheets sync, market data by ticker).
- **What happens next:** check tools → create your user → you add exports →
  import → a few questions → forecast → money strategy. About 15 minutes.
- **Consent:** before I can read any report, you type one command in your
  own terminal. I cannot do that step for you, on purpose.

Ask: "Ready to start?"

## Step 1: Check tools and pick the user

1. Run `bin/cashcast doctor`. The first run builds the engine and takes a
   few minutes; say so before you start it.
2. If a tool is missing, show the fix line it printed. Ask before you
   install anything. Say that setup downloads Rust crates and a few Python
   packages once.
3. Run `bin/cashcast users list`. If there is no user, ask for a short name
   and the province or territory (Canadian residents only), then run
   `bin/cashcast users add NAME --province XX`. If several users exist, ask
   which one and pass `--user NAME` to every later command. The original
   single-user folder is the user `default`; give it a profile with
   `bin/cashcast users adopt --province XX`.
4. Run `bin/cashcast consent list`. Explain `agent:aggregates` (reports) and
   `agent:strategy` (the plan). Ask the user to grant them in their terminal.

## Step 2: Get the exports

Tell the user:

- Put export files in their inbox: `private/users/NAME/inbox/` (or
  `private/inbox/` for the user `default`). Give the full path.
- Wealthsimple exports: every one goes straight into `private/inbox/` (or
  `private/users/<you>/inbox/`), then run `bin/cashcast import`. Move any
  left in `~/Downloads` there first.
- `bin/cashcast connectors show SERVICE` prints how to export from each
  supported service (Scotiabank, TD, RBC, BMO, Tangerine, CIBC, Wise,
  Wealthsimple, Casa, Kraken, PayPal, E*TRADE, MoneyWiz, Google Sheets). Chexy
  and Paysend have no export: their charges show on the card.
- No MoneyWiz? Skip every MoneyWiz line below. cashcast works from the bank and
  card downloads alone: the import, spending, forecast and strategy read them,
  and `weekly` skips its MoneyWiz step. RBC downloads leave out today's
  balance: ask for it and save it under `[balances]` in `cashcast.toml`.
- MoneyWiz is a mirror of the banks and brokers: every MoneyWiz row may be
  changed, moved or deleted to match them, and hand-typed rows are never
  trusted. Fix the real row; do not keep correction rows.
- MoneyWiz users record only cash by hand. `bin/cashcast moneywiz sync --dry-run`
  previews, and `bin/cashcast moneywiz sync` adds the bank and card rows
  (the owner grants `local:moneywiz` once). A sync first collects exports from
  Downloads and downloads fresh Wise statements when the token and `actions:api` consent are set up
  (`--no-download` skips both). After a sync, re-run `--dry-run`:
  it must show 0 rows to add. Never open the MoneyWiz database
  or `out/moneywiz-fixes.md`. Playbook: `connectors/playbooks/moneywiz.md`.
- "Sync my banks to MoneyWiz", "do the weekly MoneyWiz check": run
  `bin/cashcast moneywiz weekly`. Held accounts are not sent; read their reason,
  then `bin/cashcast moneywiz diagnose` (counts only) to find the cause.
  "Check these rows again": `bin/cashcast moneywiz resync --account NAME --from
  YYYY-MM-DD --to YYYY-MM-DD`, then `moneywiz sync`.
- "Categorize my transactions" / "rows have no category": run
  `bin/cashcast moneywiz categorize --dry-run`, then without `--dry-run`, then
  `bin/cashcast moneywiz edit --dry-run` and `bin/cashcast moneywiz edit`.
- Other formats: CSV from any bank or budgeting app (YNAB, Mint, Monarch,
  Copilot and others), and OFX or QFX downloads ("Quicken" or "Money").
- More history is better: at least 12 months, ideally 24 to 36.
- Export every account you use: chequing, savings and credit cards.
  Broker and crypto exports feed `bin/cashcast holdings`.
- Add the files privacy line.

Always offer both ways: the user exports the files, or approves and you fetch
them. For the second, follow the `fetch-exports` skill in `skills/`:

- `bin/cashcast fetch plan --all` lists each service, its route (API, browser,
  or the user) and its login URL.
- Use Claude in Chrome with the password manager (`request_credentials`, then
  `autofill_credential`). Without it, open the site in the built-in browser
  and ask the user to sign in there. Never type a password or a code.
- Follow the service's playbook `download-*` action. Report only "downloaded"
  or the step that failed.
- `bin/cashcast fetch collect` moves the downloads from `~/Downloads` into the
  inbox without opening them, then run the import or `holdings`.
- The Notion tasks database (decision 0009) holds no finance data, so you may
  create it with the Notion connector under the user's Finances page. Put its
  id in `cashcast.toml` `[notion] tasks_database`. The user still stores the
  integration token in `.env` and grants `cloud:notion`.

Ask the user to say "done" when the files are in place.

## Step 3: Import

Run `bin/cashcast import`. Summarize the report in a few bullets:

- Files read, transactions, date range, unreadable rows, files not read.
- Accounts and their guessed types.
- Pay found, or not found.
- Recurring extra income found.

If the command fails, map the error to one question:

| Error says | Ask | Write to the user's `cashcast.toml` |
|---|---|---|
| `no rate for USD` | The exchange rate to the main currency | `[currencies]` `base` and `rates` |
| `dates match several formats equally` | "Is 03/04/2026 March 4 or April 3?" for that file | `[files."NAME"]` `date_format = "%d/%m/%Y"` or `"%m/%d/%Y"` |
| `need a date column and an amount` | Which column number holds the date, amount and description. The user opens the file, not you. | `[files."NAME"]` `columns = { date = 1, amount = 3, description = 2 }` |
| `no export files` | Check that the files are in the inbox | nothing |
| a note says `most amounts are positive` | "In that file, do purchases show as positive amounts?" | `[files."NAME"]` `invert = true` if yes |

Re-run the import after each fix.

## Step 4: Unreadable rows (only when the report shows some)

Explain: the app can show the first unreadable row so we can find the
problem. That row's text would come to me and to the AI provider. Offer
three options:

1. **Show it to me:** one row only. The user first runs
   `bin/cashcast consent grant agent:raw-row` in their terminal.
2. **You look:** you open the file and tell me the format, for example
   "dates look like 30.09.2026".
3. **Skip:** those rows stay out of the forecast.

Before asking, run `bin/cashcast columns` (needs `agent:aggregates`). It
prints header names and masked cell shapes, never values, and often is
enough to add a parser with a synthetic fixture.

On option 1, run `bin/cashcast first-error`. Then fix it through
`[files."NAME"]` settings: `date_format`, `columns`, `invert` or
`account`. If the fix needs a code change, say so. Never paste the row
into code, tests, issues or notes. Re-run the import.

## Step 5: Questions

Run `bin/cashcast status`. Ask its "Questions to ask" one at a time, in
order. Before each private question, add a privacy line. After each
answer, edit the user's `cashcast.toml` (keep it valid TOML), then re-run
`bin/cashcast status`.

- **accounts:** show the guessed list. Explain the four types:
  - `cash`: bank accounts whose balance is your cash.
  - `card`: credit cards that you pay in full.
  - `other`: wallets and foreign accounts. Their spending counts; their
    balance is not part of your cash.
  - `excluded`: investments. Money moved there counts as invested.

  Write every confirmed type to `[accounts]`, changed or not, so it stops
  being a guess.
- **balances:** the current balance of each cash account listed. Write
  `[balances]`.
- **pay:** only when none was found. Ask for the schedule, a recent
  payday, and net pay per cheque. Write `[pay]`.
- **extra_income:** list the suggestions from the import report. Names
  like "Deposits #2" are hidden on purpose. The user can open their
  `data/review.md` to see which payee each one is; you never open it. Then either:
  - set `use_suggestions = true` under `[forecast]` to keep them all, or
  - write the kept ones as `[extra_income."NAME"]` with `months` and `amount`.
- **floor:** cash to keep untouched. The default is 3 months of spending.
  Write `[forecast]` `floor`.
- **rent (optional):** a fixed rent or mortgage, if it will differ from
  history. Write `[rent]` `category` and `charged`.
- **spending (optional):** the user's own monthly estimate and the method.
  Write `[forecast]` `baseline` and `method`.

## Step 6: Forecast

Run `bin/cashcast forecast`. Relay in short bullets:

- Total income, expenses and money to invest over the horizon.
- The lowest month-end cash, and its month.
- The three biggest sensitivities.
- The defaults that were used.
- The formula check result.

Then say where the workbook is: the user's `out/Cashcast.xlsx`. The
light-yellow cells on its Inputs sheet are editable.

If it stops with "The forecast needs these answers first", ask those
questions (step 5) and run it again.

## Step 7: Apple Numbers (optional, macOS)

Offer `bin/cashcast numbers`. It converts the workbook and checks the
values Numbers computes against the model to the cent. If AppleScript is
blocked in your environment (errors such as -1743 or -600), give the user
this command to run themselves: `! bin/cashcast numbers` in Claude Code,
or `bin/cashcast numbers` in Terminal.

## Step 8: Money strategy

1. Fill the profile with the user, one question at a time, with a privacy
   line each: income, RRSP/TFSA/FHSA room, goals, the cards they hold (ids
   from `knowledge/data/credit_cards.toml`), limits and statement close days,
   bank packages, rent and the service used to pay it.
2. Run `bin/cashcast strategy`. Relay: the net yearly cash back, the "where to
   pay" table, the rent rule, the reserve and invest order, the top
   alternative, and that this is not licensed advice.
3. Run `bin/cashcast reminders preview`. Ask the user to run, in Terminal,
   `bin/cashcast consent grant local:reminders` once (task titles hold
   amounts and may sync to iCloud), then `bin/cashcast reminders sync`. It
   writes the dated tasks to their own Reminders list and touches no other list.
   For the Calendar too, follow the `apple-sync` skill in `skills/`
   (`bin/cashcast calendar preview`, then the owner runs `calendar sync`).
4. Monthly: `bin/cashcast review` (the `monthly-review` skill in `skills/`).
   Bank and card setting changes: `bin/cashcast changes` and the `bank-change`
   skill. Where new money goes: `bin/cashcast optimize` and the
   `optimize-stocks` skill. Ranked investment moves with evidence:
   `bin/cashcast invest-plan`; to question or override one, the
   `invest-challenge` skill. A whole-portfolio question ("best long-term
   strategy", "how much crypto"): read `invest-plan`, `holdings` and
   `strategy --section tax`, then compare a few options that differ in
   crypto cap and stock share. Price a sale's capital gain together with a
   same-year RRSP deduction, because the deduction can keep the gain in a
   lower bracket. For a newcomer, the cost base is the market value on the
   residency date. Ask before setting `[invest] crypto_max` or
   `target_equity` (it needs a `target_reason`). An Amazon item that never arrived ("get my
   refund", "item missing"): the `refund` skill (a free replacement or a
   refund, never a second order). Today's rates and inflation from the Bank of
   Canada, checked against the plan's assumptions ("are your numbers still
   current?"): `bin/cashcast knowledge refresh`. Charges the user did not expect (duplicate
   subscriptions, new or pricier charges): `bin/cashcast charges`; one
   merchant on every account: `bin/cashcast charges find WORD`. Groceries
   ("order groceries", "where is X cheapest delivered", "how much did I spend
   on groceries"): the `grocery-cart` skill (`bin/cashcast grocery compare
   --items a,b`, `grocery orders`; carts in the user's Chrome, they check
   out). Buying anything else ("find the best X", "what size should I get"):
   the `purchase` skill (3-5 options, one options page, the card to pay with
   from `cards now`).
5. Notion: if the user kept finance pages in Notion, follow the `notion-import`
   skill in `skills/`. They export the pages themselves and run
   `bin/cashcast notion import`, offline. Never fetch the pages through a
   connector, and never open their `docs/` folder.

## Step 9: Wrap up

- **The main view:** ask the user to run `bin/cashcast` in Terminal (it
  starts the web app in the background) and open the printed link. Plan
  ahead, the forecast, every strategy view and the Actions view live there,
  on this computer only.
- **Every action:** `bin/cashcast actions` lists each command with its
  arguments (`--json` for agents); the web app's Actions view runs the same
  list. Settings: `bin/cashcast config`, `bin/cashcast config set KEY VALUE`.
- **Where things are:** settings in the user's `cashcast.toml` and
  `profile.toml`, outputs in their `out/` folder (`strategy.md`, the workbook).
- **To refresh later:** `bin/cashcast fetch plan` shows what is stale; add new
  exports to the inbox (or let the agent fetch them), then run
  `/cashcast-start` again, or `bin/cashcast import && bin/cashcast review`.
- **To delete one person's data:** remove their folder
  (`rm -rf private/users/NAME`); everything: `rm -rf private`.

## If something breaks

Quote the exact error line from the command. Do not guess. Do not read data
files to debug. If the fix needs code, describe the problem without any of
the user's values.
