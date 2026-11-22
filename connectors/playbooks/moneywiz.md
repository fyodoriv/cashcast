# MoneyWiz playbook

MoneyWiz is a desktop and phone app, not a website. cashcast connects to it on this Mac:

- **Read:** `bin/cashcast import` and `bin/cashcast moneywiz status` read a copy of the MoneyWiz database. Reading never writes the original file.
- **Write:** `bin/cashcast moneywiz sync` adds rows through MoneyWiz's own `moneywiz://` links. MoneyWiz saves them and syncs them like rows you type.
- **Edit:** `bin/cashcast moneywiz edit` changes existing rows and categories through MoneyWiz's own Core Data model (see `edit-in-place`).
- **Categories:** `bin/cashcast moneywiz categories` keeps your category tree tidy through numbered migrations you can roll back (see `manage-categories`).

## The flow

1. You record only cash (and anything no bank sees) in MoneyWiz.
2. Bank and card exports land in `private/users/<you>/inbox/`. Download them, or let `api download` fetch them.
3. You run `bin/cashcast moneywiz sync`.
4. It matches every bank and card row with the MoneyWiz account that follows that export.
5. MoneyWiz gets the rows it is missing. A card payment seen from both sides becomes one transfer.
6. A row whose amount differs from the bank gets a correction row for the difference (memo `cashcast fix #…`). If the balance still differs, for example because of a wrong opening balance, one balance correction makes it match.
7. Rows that are in MoneyWiz but not at the bank go to `out/moneywiz-fixes.md` for you: a duplicate, or a charge not posted yet. With such rows open, the balance is not corrected.

## Before you start

- MoneyWiz runs on this Mac, and you are signed in.
- Each MoneyWiz account that follows a bank export has the same name as the export's account, or a line under `[moneywiz.accounts]` in `cashcast.toml` (`moneywiz coverage` suggests these lines). A cash account follows an export only through such a line:

  ```toml
  [moneywiz]
  # db = "~/Library/Containers/.../MoneyWiz_iCloud.sqlite"   # default: found on this Mac
  since = "2026-01-01"      # never touch older rows
  # match_days = 4          # a row matches a bank row up to 4 days away
  # match = "loose"         # rows typed by hand: pair by direction, amount within 25 % and date; the bank wins. Default "strict"
  # fixes = "list"          # only list wrong amounts; default "adjust" corrects them; "edit" edits them in place
  # import = true           # the forecast reads your cash accounts from MoneyWiz
  # writer = "core-data"    # lets `moneywiz edit` change rows and categories in place
  # writer_checksum = "…"   # the MoneyWiz model you allowed for live edits (the first `moneywiz edit` prints it)

  [moneywiz.accounts]
  "Scotia Chequing" = "Scotiabank Chequing"   # MoneyWiz name = export account name
  "Group RRSP" = "Sun Life RRSP"              # an investment account follows its ledger's cash rows

  [moneywiz.connectors]
  "Work card" = "wealthsimple"                # an account whose name does not say its bank

  [moneywiz.out_of_scope]
  "Old bank" = "closed in 2023, no exports"   # coverage shows this reason instead of a gap

  [moneywiz.accepted]
  "Chequing" = "2023-10-30: no statement that old"  # rows to this day: MoneyWiz is the record
  ```

- You grant the write once, in Terminal: `bin/cashcast consent grant local:moneywiz`.
- If you turned off MoneyWiz bank sync for the accounts cashcast feeds, rows will not arrive twice.

## Actions

### check-coverage

1. Run `bin/cashcast moneywiz coverage`. It reads a copy of MoneyWiz and the inbox and prints counts only.
2. The first lines give the share of your MoneyWiz rows (all accounts but cash) that the exports in the inbox match, all time and for the last 12 months.
   With `[moneywiz] since` set, the next lines hold the goal from that day on: every MoneyWiz row matched and 0 bank rows missing ("Coverage since …"). "History gaps since …" names each account still open and what closes it: the days to download ("download 2023-09-30 to 2024-11-03"), bank rows to add (`moneywiz sync`), or MoneyWiz rows no bank row matches (`moneywiz edit --fixes`). Accounts in `[moneywiz.out_of_scope]` are left out of the goal and listed as "Left out on purpose". The Sunday run lists the gaps under "Not up to date" until none are left (test `coverage_since_names_every_gap_from_the_start_day`). "Open MoneyWiz rows since …" lists each row still open (account, day, amount, kind, the other account of a transfer; never the payee), up to 30, so the owner can decide each one. A 0.00 deposit, withdrawal, refund or transfer changes no balance and no bank file lists it: it counts as covered (owner, 2026-11-16).
3. The table names one gap per account, most missing rows first: an export to download, a link to add under `[moneywiz.accounts]`, or a reader or connector that is missing.
4. "Suggested links" lists exports of the same bank and kind that no MoneyWiz account follows yet. Check each, then add the lines to `cashcast.toml`.
5. "Match rate per export" shows how the bank rows pair up. Rows to add are split by why they found no partner: sign flipped, the same amount further away in time, an amount within 1.00, or nothing alike.
6. Investment accounts follow the cash rows of their ledger: a Sun Life transactions file makes one account per plan ("Sun Life RRSP", "Sun Life TFSA"), an E*TRADE transactions file makes "E*TRADE" (USD). A Wealthsimple activity file makes "Wealthsimple Cash" and "Wealthsimple Credit" from its spending accounts, and one account per investment account, named by its type and the last six characters of its id ("Wealthsimple TFSA …123CAD"). Contributions, withdrawals, dividends, interest, fees, buys and sells become rows; fund switches do not. The cash import never reads these rows. An investment account in `[moneywiz.investments]` links to its broker's cash rows on its own: the one export account whose ids the mapping takes (all of them; with a CAD and a USD side, the one in the account's currency; with a second account of the type that has cash only, the one with the trades, test `an_investment_link_picks_the_account_with_the_trades`). Trades in such an account count as covered when a broker trade pairs with them (the pairing `diagnose` uses), so its coverage shows cash rows and trades together.
7. An account with no export of its own (a stock plan that only wires money out) still counts a transfer row as covered when the other side of that MoneyWiz transfer matches a bank row: the same two accounts, within 4 days, within 2 % of the amount in one currency (a wire fee, or a bank amount `edit --fixes` wrote on that side only). Its Source reads "transfers a bank row matches" (test `transfers_into_a_matched_account_cover_an_account_with_no_export`).
8. An account named for no bank: add its connector under `[moneywiz.connectors]`. An account no export will cover: add the reason under `[moneywiz.out_of_scope]`. Rows older than any file the bank still gives: once the owner decides MoneyWiz is the record there, add `"<account>" = "<last day>: <reason>"` under `[moneywiz.accepted]`. Coverage counts those rows and its Source adds "owner accepted MoneyWiz to <day>" (test `accepted_rows_count_as_covered_up_to_their_day`). A bank account you keep out of MoneyWiz (an unused savings account): add `"<export account>" = "<reason>"` under `[moneywiz.not_tracked]`, and the sync stops listing it under "Export accounts with no MoneyWiz account" (test `a_cash_account_follows_an_export_only_when_named_in_the_config`).
9. Verify: after you add exports or links, the coverage share goes up.

### sync-from-exports

In the web app, the **Bank sync** view does every step below on one page: collect exports from Downloads, preview each account's rows, then send them after one confirm. The steps below are the CLI way.

1. Preview the sync: `bin/cashcast moneywiz sync --dry-run`. It shows counts and sums per account and sends nothing.
2. Run it: `bin/cashcast moneywiz sync`. It first moves exports saved to Downloads in the last hour into the inbox, unopened ("Collected N export file(s)"; `--no-download` skips this). Then it opens MoneyWiz if it is closed and hands it the missing rows.
3. The report ends with "MoneyWiz saved N of N". Fewer saved means MoneyWiz is still busy: leave it open a minute, then run the dry run again.
4. Verify: in the dry run, every account shows "0 to add" and "matches the bank".
   The report also says how many synced rows have no category ("Categories to fix: N …", with cashcast's guesses). The rows are in `out/moneywiz-fixes.md` under "No category yet"; set their categories in MoneyWiz.
5. Running the command twice adds nothing twice. cashcast keeps a private list of every link it sent (`data/moneywiz-sent.json`: keyed hashes and dates, no rows; mode 600):
   - sent today: not sent again while MoneyWiz may still be saving it;
   - sent earlier, never seen in MoneyWiz: sent again;
   - seen in MoneyWiz, then gone (you deleted it there): listed in `out/moneywiz-fixes.md` and **not** sent again. To add those back, run `bin/cashcast moneywiz sync --resend`.
6. A bank row that matches two MoneyWiz rows equally well is a conflict. Nothing is added or corrected for it; pick the right row in `out/moneywiz-fixes.md`.
7. MoneyWiz CSV exports in the inbox are never treated as bank exports: they hold MoneyWiz's own rows.
8. **The ledger.** Each sync (a dry run too) saves every bank row's status in `data/moneywiz-ledger.json`: matched, fix, add, sent, conflict or held, with the days it was first seen, last checked and last changed (keyed hashes, no row text; mode 600). It also keeps the last 100 runs. The Bank sync view shows both.
8b. **Only the app's own database sends.** A sync that hands links to the real MoneyWiz app (no `CASHCAST_MONEYWIZ_OPEN`) refuses unless the file it read is the app's own database: links from a test's scratch copy would land in the owner's MoneyWiz (test `only_the_apps_own_database_may_send_to_the_real_app`; 2026-11-16).
9. **Cache.** A dry run with the same exports, MoneyWiz file and settings reuses the last report ("Cached: …") in about 0.1 s.
10. **Gate.** An account whose plan looks wrong is held, and nothing is sent for it. The report names the reason:
    - the sync would move MoneyWiz further from the bank's balance, by more than 100 or 5 % of that balance. Not held: the new rows replace an opening balance that stood in for them, so the opening-balance fix (adjust mode, or `edit --fixes` with `[moneywiz.balances]`) brings it to about 0 and MoneyWiz back to the bank (test `rows_that_replace_a_stand_in_opening_balance_are_not_held`);
      Also not held: a MoneyWiz row before the period stood in for the bank rows (a transfer in typed early, dividends never typed). The bank's own start balance, its balance less every row of the period, is not below 0 (outside a card), no MoneyWiz row of the period is left without a bank row, and MoneyWiz leads that start by what the sync adds. The balance correction the day before the period squares it (test `an_early_row_standing_in_for_bank_rows_is_not_held`);
    - a statement is missing: one PDF statement's closing balance is not the next one's opening. The report names the days to download (test `statements_that_do_not_chain_name_the_missing_month`); `coverage` lists them too. Days between two statements whose balance carries over (a weekend) are no break; nor is a month the bank never issued when MoneyWiz accounts for it: from the day before the statement ahead of the gap to the end of the one after it, MoneyWiz moves as the statements do (within max(100.00, 5 % of the closing)). The sync then leaves every row of those days alone on both sides, since a bank row near the gap would pair with a row typed for the missing month (tests `a_missing_month_moneywiz_accounts_for_holds_nothing`, `rows_around_a_month_moneywiz_stands_in_for_are_left_alone`; owner, 2026-11-16). What MoneyWiz moves across those days beyond the statements becomes one correction on the last missing day (memo "cashcast statement gap fix"), so MoneyWiz matches each statement's balance; "Balance over time" leaves those days out and counts the statements' change across them (test `a_stood_in_month_gets_one_correction_to_the_statements`). An account with statements and no bank row at all (a Scotia Smart Investor plan) gets the same kind of correction on each statement day MoneyWiz is off, and "Balance over time" compares those days; the chain's "Closings off the rows" skips it (test `a_statement_only_plan_gets_corrections_to_each_statement_day`);
      Also not held: an account in `[moneywiz.trust_bank]` ("MoneyWiz name" = "why"). The owner chose the bank there although rows typed in another form stand in for bank rows: the sync sends the bank's rows, one balance correction the day before the period squares the end, and `edit --fixes` cleans up (test `an_account_the_owner_trusts_the_bank_for_is_not_held`; owner, 2026-11-16).
      Also not held: an investment account whose new cash went into a buy that `edit --fixes` adds. The gate and the balance correction count the cash of each broker trade MoneyWiz lacks when the export holds its cash leg (a Wealthsimple "trade buy" row), so MoneyWiz reaches the bank after both steps. A Sun Life plan has no legs, so its balance stays as it is (test `a_deposit_spent_on_a_buy_the_edit_adds_is_not_held`; 2026-11-22).
    - there is no bank balance, and the sync would move MoneyWiz by more than 5,000. A balance read before the last bank row (an older file, or `[moneywiz.balances]`, while a newer download with no balance column, such as Wealthsimple's all-accounts export, adds later rows) carries forward by those rows to the last row's day, so the check still runs (test `a_balance_carries_forward_over_later_rows_of_a_file_without_one`);
    - more than a quarter of its bank rows would get a fix over 5 % (the pairs look like guesses).
    A transfer with one side in a held account waits with it, and so does the other account's balance correction; the report names those accounts ("Rows of other accounts in a transfer with a held account wait too"). A 0.00 bank row moves no money and MoneyWiz does not save one, so it is never added (test `a_zero_bank_row_is_never_added`). The balance corrections line names each account and amount. A bank balance dated before the last row still gets one: it compares on its own day, with the changes dated up to it (test `an_older_bank_balance_still_corrects_a_wrong_start`). When several files give one account's balance (a statement per period, or downloads on different days), the newest day wins; a balance with no day is current and beats any dated one (test `etrade_statement_closing_cash_is_the_balance_when_the_rows_lead_to_it`). A balance dated after the last row with a gap the rows do not explain means rows are missing after that row: download them before you sync, or the balance correction lands before the period.
11. **Reconcile rows.** A MoneyWiz reconcile row inside a synced period stood in for rows the bank lists. When the sync adds those rows, it offsets the reconcile row with a correction (memo `cashcast fix #<row>`). Nothing is deleted.
12. **Check again.** `bin/cashcast moneywiz resync [--account NAME] [--from YYYY-MM-DD] [--to YYYY-MM-DD]` asks the next sync to check that range again, and to send again rows you deleted in MoneyWiz there. The web app has a "Check this account again" button per account.
13. **One row for several purchases.** With `match = "loose"`, a MoneyWiz row left over after the other passes pairs with 2 to 6 free bank rows of the same direction within a week, when their sum is within $1 and 0.5 % of it. None of them is added again.
14. **Rows with no bank row (loose).** A MoneyWiz row in the synced period that no bank row pairs with is offset (memo `cashcast fix #<row>`; nothing is deleted) when that brings MoneyWiz closer to the bank balance, or, with no balance, when the sync then moves MoneyWiz less. Ties of the same amount pair in date order. Strict mode only lists both.
15. **Batches.** The sync sends 50 links at a time (`CASHCAST_MONEYWIZ_BATCH`) and checks the database after each batch. When a batch shows none saved yet and more wait, the run stops: MoneyWiz may show a dialog (a negative-balance alert, for example) that makes it drop links, or it may only be slow in the background (it can keep rows in memory for minutes). The batch stays recorded as sent, so a rerun never sends it twice. Close any dialog, keep MoneyWiz in front, check with `--dry-run`, and use `--resend` only for rows still missing.
16. **Why a plan looks wrong.** `bin/cashcast moneywiz diagnose` prints counts only: how the fixes differ (cents, dollars, percent), duplicate rows across export files, reconcile and investment rows the matcher skips, where the rows to add may already be in MoneyWiz, and bank rows with a sign-flipped twin. Its "Balance over time" table compares MoneyWiz with the bank at the end of every bank day: days within $1, the worst day and its gap, and the lowest balance (a debit account below zero is flagged). A fund buy the export leaves out (an RRSP lists the contribution, not the buy it pays for the same day) counts on the bank side too; a buy the export lists itself (Wealthsimple) counts once (test `balance_history_counts_a_fund_buy_the_export_also_lists_once`). "Corrections `--fixes` keeps, and why" counts the `cashcast fix #` rows a fix run leaves, per reason: the account has no bank balance, the fixed row is gone, is in another account, is an investment row, or the account is a stock plan (transfers, split and foreign rows fold since 2026-11-08; test `diagnose_says_why_fixes_keeps_a_correction`). "Gap by month" gives MoneyWiz minus the bank at each month end: a flat gap from the first month is a wrong start (a balance correction fixes it); a step names the month that differs. "Statement chain" checks the PDF statements: each closing against the next opening, and the bank rows against each closing. "Statements against MoneyWiz units" sets each Sun Life statement's units per fund beside MoneyWiz's units that day (buy and sell rows plus opening shares; a fund pairs a symbol by name or most shared words) and MoneyWiz's cash; when all agree, `coverage` counts the account's rows up to that day (test `statement_units_check_moneywiz_and_cover_the_rows_before_them`). A statement older than the broker's history turns into buys on its day (see the `sunlife` playbook, `download-statements` step 5). "Broker rows to add, by activity type" sums a broker ledger's rows to add by type (the text is the type, never a payee), and "Investment accounts, both sides summed" sets the bank's trade legs and other rows beside MoneyWiz's buys and sells, other rows and balance before the period.
17. **Categories.** Rows the sync adds get the category your own rows teach: for each payee, the category you used most, when it holds at least 60 % of that payee's rows. Words seen under 20 or more of your categories (card and point-of-sale prefixes) are skipped. Bank and engine guesses need a 75 % share, so they rarely decide. `bin/cashcast moneywiz categorize` does the same for synced rows with no category: it prints counts only and writes `inbox/moneywiz-edits.toml` (never over an existing one); `--dry-run` writes nothing. Apply it with `bin/cashcast moneywiz edit --dry-run`, then `bin/cashcast moneywiz edit`. Each row it categorizes goes into `data/moneywiz-categorized.json`: learning skips those rows, and a later run picks again when the rule now names another category. A row you move yourself is yours again. `--record-since <backup folder>` records the rows categorized since that backup (for runs made before the record existed). `bin/cashcast moneywiz categorize --check` hides every 5th row you categorized, predicts it from the rest, and prints the share of synced rows with a category and the most common misses (97 % on the owner's data, 2026-11-06; it was 86 % before stop words and the share floor). A local model (qwen3:14b) matched only 54 %, so the engine never guesses with one. A category whose own name holds a slash is matched whole; a name that fits both a slash name and a subcategory is refused, so `categorize` then writes `set_category_path = ["Food", "Dining/Eating Out"]`, the exact parts. Rows with no payee rule get a best guess: the category most used for the first payee word, else for the account and sign (99 % of synced rows on the owner's data, 2026-11-07). A path two categories share (income and expense) takes the type you used most for rows of the same sign on that path; with no clear winner the row is skipped and counted.

### weekly-sync

Run once a week: `bin/cashcast moneywiz weekly`. It does the same as `moneywiz sync`: it collects exports from Downloads, downloads fresh Wise statements, then sends what passes the gate. The report lists each export older than the `fetch.stale_days` setting ("Exports older than 6 days: Bank (ends 2026-11-03, downloaded 2026-11-08)"): its newest row and its newest download are both older than that. `fetch plan` uses the same limit; set it with `bin/cashcast config set fetch.stale_days 6`. An account whose every file comes from a crypto-only service (kinds `crypto` or `fx`: Kraken, Casa) uses `fetch.crypto_stale_days` instead (default 31: checked once a month). An account with no activity since a recent download is current. Two downloads of one account that `[moneywiz.export_aliases]` joins ("chequing-activities-2026-11-08", "…-11-16") hold each shared row once. Download those (see `bin/cashcast connectors show <bank>`) and run it again.

### fix-listed-rows

1. Open `out/moneywiz-fixes.md`. Only you read it; agents do not.
2. For each row "in MoneyWiz, not at the bank", delete the duplicate or wait for it to post.
3. For each conflict, fix or delete the MoneyWiz row that is wrong. The next sync then pairs the rest.
4. For each row "sent before, not in MoneyWiz now": nothing to do if you deleted it on purpose; otherwise run `bin/cashcast moneywiz sync --resend`.
5. For each row "no category yet", set a category in MoneyWiz (the guess column is a hint).
6. With `fixes = "list"` only: for each amount that differs, edit the MoneyWiz row to the bank amount.
7. Verify: run `bin/cashcast moneywiz sync --dry-run`. It shows 0 amounts differ, 0 rows only in MoneyWiz and 0 conflicts.

### export-csv

1. Open MoneyWiz, click the Bank (Accounts) icon, and choose **File > Export**.
2. Select all accounts and the date range. Pick **CSV**, and keep the default date format and encoding.
3. Keep the app language in English, so the headers are in English.
4. Save it into `private/users/<you>/inbox/`. With a MoneyWiz CSV in the inbox, `import` does not read the database as well.
5. Verify: the save dialog shows a `.csv` file name.

### export-report-csv

1. Open **Reports** and pick the report.
2. Click **Save report**, then **Export**, then **CSV**.
3. Save it into `private/users/<you>/inbox/`.

### refresh-bank-sync

1. Open the account list.
2. Use the app's refresh (sync) control for online-banking accounts.
3. Verify: each synced account shows today's date as the last update.
4. If an account shows a connection error, you reconnect it. The agent does not type bank passwords.

### edit-in-place

Changes existing rows (amount, category, payee, memo), opening balances, whether an account is archived, and categories (add, rename, delete) through MoneyWiz's own Core Data model ([decision 0007](../../docs/decisions/0007-moneywiz-edits.md)).

1. Write the edits into `private/users/<you>/inbox/moneywiz-edits.toml`. Name each row the way you see it in MoneyWiz; add `text` (part of the payee) when two rows share a date and amount:

   ```toml
   [[row]]
   account = "Chequing"         # the MoneyWiz account name
   date = "2026-09-02"
   amount = -40.00              # the amount MoneyWiz shows now
   set_amount = -45.00          # any of: set_amount, set_category, set_payee, set_memo
   set_category = "Food/Groceries"
   # delete = true               # instead of set_*: remove a row no bank row backs (never a transfer)

   [[category]]
   add = "Food/Coffee"
   # kind = "expense"            # or "income": needed when the parent's name is both an income and an expense category

   [[category]]
   rename = "Food/Groceries"
   to = "Grocery stores"

   [[category]]
   delete = "Old"
   move_to = "Food/Coffee"      # where its rows go; needed when rows use it
   ```

   A `set_category` edit with no `text` that matches two rows picks the one with no category yet. `set_category_kind = "expense"` (or `"income"`) picks between two categories that share a path. To keep the edits file out of the inbox (an agent may not write there), point `CASHCAST_MONEYWIZ_EDITS=<path>` at it.
2. Preview: `bin/cashcast moneywiz edit --dry-run`. It shows counts, and problems by edit number (no row matches, two rows match, a category does not exist).
3. Turn it on once: add `writer = "core-data"` under `[moneywiz]` in `cashcast.toml`. The first run builds the writer (it needs `xcode-select --install`). The engine uses the writer from its own checkout, so an engine run from the sync worktree with `CASHCAST_ROOT` set never reaches an older writer.
4. Run `bin/cashcast moneywiz edit`. It applies the edits to a copy of the database and checks the copy. The first time, it stops there and prints a `writer_checksum = "…"` line: add it under `[moneywiz]` to allow live edits for this MoneyWiz version.
5. Run it again. It quits MoneyWiz, backs the database up to `data/moneywiz-backups/`, writes the edits, checks them, and reopens MoneyWiz.
6. Verify: the report says "Verified N of N changes", and MoneyWiz sync shows **Up to Date**. The first time, also check one edited row on your phone.
7. To fix wrong amounts automatically, set `fixes = "edit"`: each `moneywiz sync` then edits them in place (and a wrong opening balance) instead of adding correction rows. `bin/cashcast moneywiz edit --fixes` does the same without a sync. It also marks every row MoneyWiz shows as pending (status 1) cleared, and the writer saves the broker trades it adds as cleared: without a status MoneyWiz lists them in a "Pending" block at the top of All transactions (owner, 2026-11-22; test `fixes_mark_pending_rows_cleared`).
8. `--fixes` also moves a matched row to the bank's day (`set_date = "YYYY-MM-DD"` does it by hand), folds every `cashcast fix #<row>` correction into the row it fixes and deletes it (a row with several corrections takes their sum in one run), deletes a row and its corrections when they sum to zero, and folds a `cashcast balance fix` row into the opening balance. A second run then has nothing to change (test `a_second_fix_run_changes_nothing`).
9. **Investments.** Map a MoneyWiz investment account to its broker files in `cashcast.toml`:

   ```toml
   [moneywiz.investments]
   "My TFSA" = "wealthsimple-activity-csv:TFSA"
   "Stock plan" = "etrade-benefit-rs:non-registered, etrade-gains-losses:non-registered"
   "Taxable" = "wealthsimple-activity-csv:non-registered:1234"   # last 4 of the account id
   ```

   `diagnose` then prints "Investments against the broker files" (shares per symbol on both sides, missing trades, last trade price) and the broker accounts nobody mapped. `moneywiz sync` links each mapped account to its broker's cash rows (deposits, dividends, fees; a Wealthsimple activity file, a Sun Life file by name) and sends them like a bank's; trade legs stay out, since the buy and sell rows carry them. Its "Broker trades" part lists, per account, the trades MoneyWiz lacks; `edit --fixes` adds them (Sunday step 2b runs it; test `wealthsimple_investment_accounts_sync_like_a_bank`). An activity file has no balance: when the gate holds the account ("no bank balance to check"), read the cash balance on the site and add it under `[moneywiz.balances]`. `edit --fixes` adds the missing buy and sell rows (symbol, shares, price, day) only when they make MoneyWiz's shares equal the broker's, every trade has a CAD amount, and no symbol's broker total is below zero (a file that starts partway through the history).
10. **Many downloads, one account.** Some banks name each download apart (TD names a CSV by its file). Join them in `cashcast.toml`; a key ending in `*` matches by prefix:

    ```toml
    [moneywiz.export_aliases]
    "td-credit-accountactivity*" = "Credit card …1234"
    ```

    Alias rows fill only the days the account's own files (the QFX) do not cover, so overlapping downloads count once. A Kraken ledger's `balance` column is the bank balance for `Kraken CAD` (and each fiat asset).

    **The bank always wins.** When your MoneyWiz entries are rough, add `bank_wins = true` under `[moneywiz]` (with `match = "loose"`). Every MoneyWiz row that no bank row pairs with in a synced period is then offset by the sync, and `edit --fixes` deletes the pair. A row you typed for several purchases (2 to 6 bank rows within a week that sum to it) also goes, and the bank's own rows are added, so every day's balance and every cent match the bank (test `bank_wins_replaces_a_lump_with_the_bank_rows_it_sums`). Without it, rows go only when the bank balance proves they stood in. A bank balance dated before the export's last row is still checked once nothing is left to change. An account that `[moneywiz.investments]` also maps leaves a ledger's trade legs to the investments fix, so trades never count twice. A stock plan account whose cash comes from E*TRADE statements keeps every correction instead (see the `etrade` playbook, `download-statements`); `edit --fixes` moves each kept correction to the day of the row it fixes (test `edit_fixes_dates_a_kept_correction_on_the_day_of_its_row`).

    **MoneyWiz is a mirror.** Every MoneyWiz row may be changed, moved or deleted to match the banks and brokers; hand-typed rows are never trusted (owner rule, 2026-11-08). To retire an old account, rename it (for example "Kraken old"), create the new one, and add `[moneywiz.merge]` with `"Kraken old" = "Kraken"`: `edit --fixes` moves every row in the new account's currency into it (both sides of a transfer stay linked) and deletes the old account's hand-typed trades when the new account shows a broker file. A forex account keeps its own currency on a holding; those rows count as cash. A coin transfer to another account goes with its other side (the broker file already has the old account's deposit or withdrawal); the other account keeps the movement as a plain row: a buy or sell with no cash in an investment or forex account, a deposit or withdrawal in a cash account. When the old account has no rows left, delete it in MoneyWiz. An account with no new home (for example an old EUR wallet) goes with `[moneywiz.retire]` `"Kraken old" = "EUR"`: every row is deleted, and each transfer leaves its other account a plain row. Broker trades write cash only in the account's own currency: coins moved in, out or swapped, and trades paid in USD or EUR, move no CAD (`trade_cash`); when a symbol's cash in MoneyWiz differs from that, the bank-wins fix replaces the symbol's trades (tests `merge_moves_rows_in_the_targets_currency_and_drops_hand_trades`, `trade_cash_moves_only_the_accounts_own_currency`).
11. **Archived accounts.** MoneyWiz drops every link sent to an archived account (Preferences, Archived Accounts), with no alert, so `moneywiz sync` warns "Archived in MoneyWiz, so MoneyWiz drops their links". Show the account first, sync, then archive it again: `bin/cashcast moneywiz edit --account NAME --archived no`, `bin/cashcast moneywiz sync`, then `--archived yes` (test `sync_warns_when_links_go_to_an_archived_account`).
12. If anything looks wrong: quit MoneyWiz, copy the newest folder in `data/moneywiz-backups/` back over the database, and open MoneyWiz. Before you do, turn off MoneyWiz sync on other devices, or they may send the edits back.

### manage-categories

Keeps your income and expense categories tidy through numbered migrations, each with a computed rollback ([decision 0012](../../docs/decisions/0012-moneywiz-category-migrations.md)). It uses the same writer and safety steps as `edit-in-place`, so do steps 3 and 4 there once first.

1. See the live tree: `bin/cashcast moneywiz categories`. It lists each category with its type (income or expense), its rows, its income rows and its subcategories. It shows counts only.
2. Get suggestions: `bin/cashcast moneywiz categories suggest`. It writes `inbox/moneywiz-migrations/NNNN-suggested.toml` and lists why each change helps:
   - a category your `[budget]` or `[strategy.categories]` uses but MoneyWiz lacks: add it;
   - an expense category that holds mostly income rows (or the reverse): move those rows to a sibling of the right type with the same name, or a new one;
   - a near-duplicate (same name, or a top-level copy of the only subcategory with that name, with no rows of the other type): merge the smaller into the bigger;
   - a rarely used top-level category whose rows look like another one: move it under that one;
   - an empty category nothing uses: delete it. When MoneyWiz still links it to rows cashcast does not read (such as scheduled transactions), it only adds a note: a rollback could not bring those links back, so `migrate` refuses to delete it.

   When two MoneyWiz categories share one name at one level (for example two top-level "Other"), cashcast cannot tell them apart. It lists them, and leaves them and their subcategories alone: rename one in MoneyWiz, then suggest again. `migrate` refuses any op on such a path.
3. Open the file in your editor. Delete the `[[op]]` blocks you do not want, or change names. Do not edit `expect_tree`. To write your own, run `bin/cashcast moneywiz categories new --name merge-food` and fill it:

   ```toml
   [[op]]
   add = "Side income"
   kind = "income"              # top level only; subcategories take their parent's type

   [[op]]
   rename = "Food/Groceries"
   to = "Grocery stores"

   [[op]]
   move = "Shopping"            # moves its rows
   to = "Side income"
   only = "income"              # optional: only rows above 0 (or "expense")

   [[op]]
   delete = "Old"
   move_to = "Food"             # where its rows go
   ```

4. Preview: `bin/cashcast moneywiz categories migrate --dry-run`. It shows the counts, and refuses a file written for another tree ("changed since this migration was written"): suggest again or run `new`.
5. Apply: `bin/cashcast moneywiz categories migrate --yes`. Each pending file, in number order, is tested on a copy, then written with a backup, checked as a whole tree, and recorded in `data/moneywiz-migrations.json`.
6. Verify: `bin/cashcast moneywiz categories` shows the new tree and "N applied, 0 pending", and MoneyWiz sync shows **Up to Date**.
   In the web app, the **Categories** view does steps 1 to 5 in one place: it shows each MoneyWiz category beside the cashcast category it matches, marks the unmatched ones and the suggestions, and lets you pick a change per row (rename, merge into another, match to a cashcast category) or add the ones MoneyWiz lacks. **Preview plan** writes `inbox/moneywiz-migrations/NNNN-web.toml` and shows the dry run. **Apply to MoneyWiz** asks you to confirm, backs up the database, applies it, and shows where the backup is.
7. Undo the last migration: `bin/cashcast moneywiz categories rollback --yes` (or **Undo last change** in the Categories view). It applies the computed inverse the same safe way: categories come back, and every moved row goes back by its id. Run it again to undo the one before.
8. If the rollback says the categories changed since, undo your own change in MoneyWiz first, or as the last resort restore the backup it names (step 8 of `edit-in-place`).

### probe-edit-paths

1. Open MoneyWiz.
2. Run `bin/cashcast moneywiz probe --ui` in Terminal. The first run may ask you to give Terminal Accessibility access in System Settings > Privacy & Security > Accessibility.
3. It reads the app bundle and the shape of the MoneyWiz window, never your ledger, and prints which ways in can edit rows and categories: AppleScript, Shortcuts, the Core Data writer from moneywiz-tools, or the window itself.
4. Verify: the report ends with a "Best way in" line. Paste the table to your agent; it holds no amounts or names.

### check-account-balances

1. Run `bin/cashcast moneywiz sync --dry-run`. Its last column compares each MoneyWiz balance with the bank's.
2. A difference with no listed row usually means a wrong opening balance. The sync adds one balance correction for it (with `fixes = "edit"` it edits the opening balance; with `fixes = "list"` it does neither).

## Limits

- The URL scheme can create rows but cannot edit or delete them. With the default `fixes = "adjust"`, a wrong amount gets a correction row next to it. `edit-in-place` above changes rows instead.
- **The in-place writer is not tested on a real MoneyWiz store yet.** It refuses a model checksum you did not allow, tests every run on a copy first, and keeps a backup. It does not change amounts of transfers, foreign-currency rows or split rows; with `fixes = "edit"`, the sync adds a correction row for those instead. Whether MoneyWiz's sync carries edits to other devices is unproven until you check.
- **Category migrations are not tested on a real MoneyWiz store yet.** The types come from the `type` column (1 expense, 2 income, as in moneywiz-api); check that `moneywiz categories` shows them right. A split row cannot move to another category; the test on a copy stops it.
- **Not tested on a real MoneyWiz store yet.** Database columns differ between MoneyWiz versions. cashcast picks them by entity name and refuses a layout it does not know.
- The transfer link's `date` and `description` parameters are not in MoneyWiz's documentation. If MoneyWiz ignores them, the transfer gets today's date.

## Sources

- https://help.wiz.money/en/articles/4525440-url-schemas
- https://help.wiz.money/en/articles/5816620-moving-data-files-between-moneywiz-versions
- https://github.com/ileodo/moneywiz-api (schema reader)
- https://github.com/marcomc/moneywiz-tools (why raw SQL writes are unsafe)
- https://github.com/ileodo/moneywiz-api/blob/main/src/moneywiz_api/model/category.py (category type: 1 expenses, 2 income)
- https://help.wiz.money/en/articles/5338397-about-manual-data-import
- https://wiz.money/

Last checked: 2026-11-01

### site-balances

Some exports carry no balance (investment cash, Sun Life, a savings account typed by hand). Read the balance on the site and save it in `private/cashcast.toml`:

```toml
[moneywiz.balances]
"MoneyWiz account name" = "AMOUNT@YYYY-MM-DD"
```

The sync then checks that account against it, and `moneywiz edit --fixes` sets its opening balance so MoneyWiz ends on that amount on that day.

### investment-rows

`moneywiz diagnose` pairs each broker trade with one MoneyWiz row. Exact pairs (same day, shares and cash) go first, then rows typed by hand (a few days or 2 % off). "Rows off" counts loose pairs; with `bank_wins`, `edit --fixes` replaces them with the broker's rows, so gains match the broker. A Kraken sale's cash is its proceeds less the fee, as the Kraken cash export holds it (test `a_kraken_sell_moves_its_proceeds_less_the_fee`). Exact pairs may each be up to 0.0001 shares apart, so a symbol whose MoneyWiz total differs from the broker's by more than 0.00001 is also rewritten from the broker's trades. Mappings that take trades from more than one broker account are listed with their id ends.

### site-holdings

A crypto vault's files can miss network fees, so its shares drift from what the app shows. Save the number you read:

```toml
[moneywiz.holdings]
"Casa" = "BTC:1.0@2026-11-08"   # SYMBOL:SHARES@YYYY-MM-DD
```

`diagnose` and `edit --fixes` add one fee trade (no cash) so the broker side, and then MoneyWiz, end at that number. The fee trade carries the price of the latest broker trade, because MoneyWiz values a holding at its newest trade's price (a CAD 0 price showed the account at CAD 0).

### holding-openings

A MoneyWiz holding can carry opening shares (`openningNumberOfShares`), which MoneyWiz adds to its buy and sell rows. `diagnose` counts them on the MoneyWiz side. When the account has a broker file, or is an old account in `[moneywiz.merge]`, `edit --fixes` sets them to 0, since the broker file holds every trade. With consent `net:market-data` it also sets each holding's price per share to the market price (in the account's currency), or to the broker's last trade when there is no quote, so MoneyWiz shows the same value as `diagnose` (test `fixes_zero_a_holdings_opening_shares`). The Sunday sync runs it.

### rows-without-category

`bin/cashcast moneywiz categorize --dry-run --list` first shows each row it would categorize (day, account, amount, payee, suggested category, where the guess came from), so an agent can ask about one row at a time. It also lists the synced rows with no category found (day, account, amount, payee; private, only on your screen) and each row's possible other side: a row in another account with the opposite amount (within 1 %) within 10 days. A row with an other side is a transfer and needs no category. To set a category on one of two identical rows, add `nth = 1` (or 2, …) to its `[[row]]` block in `moneywiz-edits.toml`.

When every pick is skipped (two rows that look the same, or a name like "Other" that several categories share), `categorize` writes no edits file and says so; set those rows in MoneyWiz.
