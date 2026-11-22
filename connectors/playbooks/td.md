# TD Canada Trust playbook

## Before you start

- You need an EasyWeb sign-in. Use a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money.

## Export your data

1. Sign in at https://easyweb.td.com/.
2. Select the account.
3. On **Account Activity**, pick 30, 60, 90 or 120 days, or one month under **Search by Month**.
4. Click **Download**. Pick **Quicken (QFX)**. Do not pick the CSV: it names no
   account, so cashcast names the account after the file (`accountactivity`) and
   its rows stay out of the MoneyWiz sync ("Downloads that name no account").
   The first QFX download of a card asks for the account type once ("Setup
   Download to Quicken"): pick **Credit Card**, then **OK**.
5. Save the file into `private/users/<you>/inbox/`. Rename each
   `accountactivity.qfx` in `~/Downloads` before the next download, so Chrome
   does not add " (1)".
6. Repeat for each account. Faster: the **Download** icon on **Accounts
   Overview** gives one QFX for every account (current statement period).

## Actions

### download-transactions

First setup: go back as far as EasyWeb lets you (18 months, checked 2026-11-17). Account Activity gives 120 days at a time, so download the newest 120 days, then each older month under **Custom** → **By month**, one file per month. Older history comes only from statements (`download-statements`). Overlapping files are fine: the import drops duplicate rows.

1. Follow "Export your data".
2. Verify: the file is `accountactivity.qfx` (or `.ofx`) and the period on screen matches.
3. For older months, repeat with **Search by Month**.

### download-statements

For history older than Account Activity reaches (about 120 days). `bin/cashcast moneywiz coverage` names the months in its "History gaps since" line.

1. Sign in at https://easyweb.td.com/ (the person signs in).
2. Select the account, then open **Statements** (eStatements).
3. For each month the gap names, open the statement and click **Download** (or the PDF icon). Save the PDF as it is into `private/users/<you>/inbox/`. Do not open or rename it. Many at once: the page's own `/waw/api/edelivery/estmt/documentlist?accountKey=…&period=<year>&documentType=ESTMT` lists them, and `documentdetail?documentKey=<documentId>` returns each PDF as base64. Chrome then asks once to allow easyweb.td.com to download multiple files; until the person allows it, every later download fails silently.
4. Run `bin/cashcast fetch collect` if the files went to Downloads: it moves bank statements only and leaves other PDFs.
5. Run `bin/cashcast columns`. Each statement line says `td-credit-pdf` or `td-chequing-pdf`, its row count, "balances checked on N" and the account. The account must be the one `[moneywiz.accounts]` links (`Credit card …NNNN`, `Checking …NNNN`); if it is the file name, add an alias in `[moneywiz.export_aliases]`.
6. Run `bin/cashcast moneywiz coverage`: the gap closes for those months.

Known gaps (2026-11-16): EasyWeb has no card statement for May and June 2024, and no chequing e-statement before Oct 31, 2023 or for Dec 31, 2024 to Jan 31, 2025. Account Activity no longer reaches January 2025 either (2026-11-17), so MoneyWiz's rows stand in for it, with one statement gap correction (see the `moneywiz` playbook).

If a statement says "the balances do not add up", the reader does not know that layout yet and reads no row from it. Keep the file. Paste the `columns` lines (masked shapes, no values) into a task, and note the months as a known gap here until the reader is fixed. Rows on days a CSV or QFX download covers come from the download, not the PDF.

### check-statement-close-date

1. Select the credit card.
2. Open **Statements**. The newest statement shows its date (the close date).
3. Read the **payment due date** and **statement balance**.
4. Report the dates. Do not report the card number.

### change-chequing-package

1. Select the chequing account. Look for a "change account" or "switch plan" option.
2. If there is none, stop. Tell the user to call TD or visit a branch.
3. If there is one: read the new monthly fee, the waiver balance and the transaction limit on screen.
4. Ask the user to confirm this exact change before clicking the final button.

### set-card-autopay-full-balance

1. In the TD app: select the card > **Manage** > **Automatic Payments to Pay Card**. In EasyWeb, look for the same option or TD's online PAD form.
2. Choose **full statement balance** (paid on the due date), not the minimum.
3. Check the paying account on screen.
4. Ask the user to confirm this exact change before clicking the final button.

### redeem-cash-back-statement-credit

1. Select the cash back card. Open the rewards or cash back section.
2. Read the available cash back amount.
3. Pick **statement credit** as the redemption.
4. Ask the user to confirm this exact change before clicking the final button.

### product-change-card

Usually done by phone. The agent lists the target card, its annual fee and the questions to ask (same account number? prorated fee refund? effect on credit history?). The user calls TD.

### send-etransfer

The agent does not do this. It creates a reminder for you with the amount and date.

### read-balances

1. Sign in at https://easyweb.td.com/ (the person signs in).
2. **My Accounts** lists every account and card with its balance.
3. Read the chequing balance and each TD credit card's current balance.
4. Report each as `--balance ID=AMOUNT` with the profile ids (for example `td-all-inclusive`, `td-cash-back-visa-infinite`). Do not report account or card numbers.

## API

TD has no public API for personal accounts. Use the file download.

## Sources

- https://www.td.com/ca/en/personal-banking/how-to/easy-web/access-online-statement
- https://www.td.com/ca/en/personal-banking/products/credit-cards/pre-authorized-debit
- https://www.td.com/ca/en/personal-banking/how-to/manage-my-credit-card/pay-credit-card-bill-online
- https://beancount.io/tools/csv-to-beancount/td-canada-trust
- https://invoicedataextraction.com/blog/td-bank-statement-to-excel

Last checked: 2026-10-18
