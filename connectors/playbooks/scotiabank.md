# Scotiabank playbook

## Before you start

- You need a Scotia OnLine sign-in. Use a web browser; the mobile app cannot download transactions.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money.

## Export your data

1. Sign in at https://www.scotiaonline.scotiabank.com/.
2. On the Accounts page, select the account.
3. Pick the statement period from the dropdown.
4. Scroll down. Click **Load more** until the **Current statement period total** line shows.
5. Click the download icon at the top of the screen.
6. Pick **CSV** if it is offered. If not, pick **Quicken (QFX)**.
7. Save the file into `private/users/<you>/inbox/`.
8. Repeat for each account and statement period.

## Actions

### download-transactions

First setup: go back 24 months. Download each statement period in the dropdown, one file per period; older periods are PDF only: get those with download-statements. Overlapping files are fine: the import drops duplicate rows.

1. Follow "Export your data".
2. Verify: the file name ends in `.csv` or `.qfx`, and the period on screen matches the one you picked.

### download-statements

For history older than the web filter reaches (November 2024). `bin/cashcast moneywiz coverage` names the months in its "History gaps since" line.

1. Sign in at https://www.scotiaonline.scotiabank.com/ (the person signs in).
2. Select the account, then open its **Statements** tab.
3. Pick each year the gap names, then click **View PDF** on each statement and save it as it is into `private/users/<you>/inbox/`. Do not open or rename the PDFs. **Download all as PDF** is broken (2026-11-16): its zip holds one statement named `manage.statements.pdfFilename`. Many at once: the page's own `/my-accounts/api/statements?accountKey=<key from the account URL>` lists each statement's `key` and `documentDate`, and `/my-accounts/api/statements/<key>` returns its PDF. Chrome then asks once to allow secure.scotiabank.com to download multiple files.
4. Run `bin/cashcast fetch collect` if the files went to Downloads: it moves bank statements only and leaves other PDFs.
5. Run `bin/cashcast columns`. Each statement line says `scotiabank-visa-pdf` or `scotiabank-chequing-pdf`, its row count, "balances checked on N" and the account. The account must be the one `[moneywiz.accounts]` links; when it is not, add an alias in `[moneywiz.export_aliases]` (for example `"Checking …1234" = "Ultimate_Package_1234"`).
6. Run `bin/cashcast moneywiz coverage`: the gap closes for those months.

A Visa statement names the card number it was issued under, so a replaced card gives a second name (`Credit card …NNNN`): alias each one.

If a statement says "the balances do not add up", the reader does not know that layout yet and reads no row from it. Keep the file. Paste the `columns` lines (masked shapes, no values) into a task, and note the months as a known gap here until the reader is fixed. Rows on days a CSV or QFX download covers come from the download, not the PDF.

### download-smart-investor-statements

For a Scotia Smart Investor plan (a TFSA or RRSP of funds and GICs). It has no CSV download; its statements prove MoneyWiz's rows.

1. On the Accounts page, open the plan under **Investments** ("Tax-Free Savings - SSI"). Smart Investor opens.
2. Click **⋮** (more options) > **View statements**. Open each year's list.
3. Each link posts a form and opens the PDF in a new tab. Many at once: capture each link's form (`HTMLFormElement.prototype.submit`), post it with `fetch`, and save each PDF as `scotia-ssi-<plan>-statement-<YYYY-MM>.pdf`. Chrome asks once to allow scotiaonline.scotiabank.com to download multiple files; the blocked files then arrive too, as copies ("name (1).pdf"): move the copies out of the inbox.
4. Save them into `private/users/<you>/inbox/` and run `bin/cashcast columns`. The year-end statement reads as `scotiabank-ssi-pdf` with "1 statement <start> to <end>; account TFSA …NNNN; the summary adds up": opening market value plus deposits, less withdrawals, plus the change in market value gives the closing market value. The other quarters show the account value only and read as "nothing to read". No row is read: trades and interest stay out of MoneyWiz.
5. Link the account once in `[moneywiz.accounts]` (`"Scotiabank TFSA" = "TFSA …NNNN"`). The sync checks MoneyWiz against the newest closing value, and `moneywiz coverage` counts every MoneyWiz row up to that day when MoneyWiz's balance is the same ("statement cash to <day>", tests `scotia_smart_investor_*`). An emptied plan (closing 0.00, MoneyWiz 0.00) is never listed as an old export. With `[moneywiz] since` set, the sync also makes MoneyWiz hold each statement's opening market value the day before it starts and its closing on its last day: one correction per statement day that is off (memo `cashcast statement gap fix`, text "Statement balance correction"); MoneyWiz's own rows stay. "Balance over time" checks those days (test `a_statement_only_plan_gets_corrections_to_each_statement_day`).

### check-statement-close-date

1. Select the credit card account.
2. Open the statement period dropdown. The newest closed period shows its end date.
3. Read the **payment due date** and **statement balance** on the account summary.
4. Report the close date and due date. Do not report the card number.

### change-chequing-package

1. Select the chequing account. Look for an account settings or "change account" option.
2. If there is no online option, stop. Tell the user to call Scotiabank or visit a branch.
3. If there is one: pick the new package and read the monthly fee and limits on screen.
4. Ask the user to confirm this exact change before clicking the final button.

### set-card-autopay-full-balance

1. Select the credit card. Look for automatic or pre-authorized payments.
2. If there is no online option, stop. Tell the user to call Scotiabank.
3. If there is one: choose **statement balance** (full balance), not the minimum.
4. Check the paying account on screen.
5. Ask the user to confirm this exact change before clicking the final button.

### redeem-scene-points-statement-credit

1. Select the Scene+ credit card. Open **Redeem points** / **Points for Credit**. You can also use sceneplus.ca.
2. Pick an amount block (credits run from $20 to $500).
3. Warn the user: the request cannot be undone, the credit can take up to 5 business days, and the minimum payment is still due.
4. Ask the user to confirm this exact change before clicking the final button.

### product-change-card

Usually done by phone. The agent lists the target card, its annual fee and the questions to ask (keep the same account number? effect on credit history? prorated fee refund?). The user calls Scotiabank.

### send-etransfer

The agent does not do this. It creates a reminder for you with the amount and date.

### read-balances

1. Sign in at https://www.scotiabank.com/ca/en/personal.html (the person signs in).
2. **Accounts** lists Banking, Credit cards and Investments with balances.
3. Read the chequing balance and each Scotia credit card's balance ("I owe").
4. Report each as `--balance ID=AMOUNT` with the profile ids (for example `scotia-ultimate-package`, `scotia-momentum-visa-infinite`). Do not report account or card numbers.

## API

Scotiabank has no public API for personal accounts. Use the file download.

## Sources

- https://help.scotiabank.com/article/how-can-i-download-transactions-to-ms-money?origin=dotcom-help
- https://help.scotiabank.com/article/can-i-download-my-transaction-history-in-the-app?origin=neo
- https://scotiabank.com/ca/en/personal/bank-your-way/digital-banking-guide/credit-cards/redeem-points-for-credit.html
- https://www.scotiabank.com/ca/en/personal/credit-cards/visa/scene-card/welcome-kit/redeeming-sceneplus-points.html

Last checked: 2026-10-18
