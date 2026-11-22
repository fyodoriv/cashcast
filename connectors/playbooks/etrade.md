# E*TRADE playbook

## Before you start

- You need an E*TRADE sign-in (brokerage, stock plan, or both).
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never trades or sells.

## Export your data

Brokerage transactions:

If **Accounts > Activity** never loads its list (a stock-plan-only sign-in), download the stock plan statements instead (`download-statements` below). The **Orders** page also says where each sale's proceeds were wired; `moneywiz coverage` counts the MoneyWiz transfer to that bank as covered by the bank's own row.


1. Sign in at https://us.etrade.com/home.
2. Go to **Accounts > Transactions**.
3. Set a custom date range, one calendar year per file.
4. Click the download icon. Pick the spreadsheet (CSV) format.
5. Save it into `private/users/<you>/inbox/`.

Stock plan (Morgan Stanley at Work):

1. Open the stock plan account.
2. Download both **My Account > Benefit History** (vests and ESPP purchases) and
   **Gains & Losses** for each tax year with sales. Benefit History gives the buys
   (net shares after tax withholding); Gains & Losses gives the sales.
3. Click **Download** (**Download Expanded** if offered). Keep the `.xlsx` as it is; cashcast reads it. Downloading a file again is safe: rows that also appear in another file of the same format count once (`holdings` says how many).
4. Save it into `private/users/<you>/inbox/`.

## Actions

### download-transactions

Browser: follow "Brokerage transactions". Verify the date range on screen.

API: `GET /v1/accounts/list` for the `accountIdKey`, then `GET /v1/accounts/{accountIdKey}/transactions` with `startDate`/`endDate` (MMDDYYYY). Page with `marker`. History covers about two years.

### download-statements

The cash side of the stock plan: sales, wires out, fees, dividends and tax withheld.

1. Open https://us.etrade.com/etx/pxy/accountdocs#/documents and pick **Statements**. Set **Timeframe** to **Last 12 Months** (or a year) and click **Apply**: the default shows only the last 90 days. E*TRADE skips a month with no activity, so a gap in the list is not a missing statement.
2. Tick every statement since your MoneyWiz history starts. Click **Download**: you get a `ClientStatements_*.zip`.
3. Unzip the PDFs into `private/users/<you>/inbox/` and delete the zip. Reading them needs `pdftotext` (`brew install poppler`).
4. Run `bin/cashcast columns`: each statement says "each adds up to its net". A statement whose rows miss its "NET CREDITS/(DEBITS)" line is refused whole. It also says "closing cash on <day>" when its "OPENING CASH, BDP, MMFs" plus the rows give its "CLOSING CASH, BDP, MMFs": that closing is the account's balance on the period's last day, so the sync checks MoneyWiz against the newest statement, and `moneywiz diagnose` "Statement chain" checks each closing is the next opening. **Current Account Value** on the At Work overview counts vested shares only, not this cash: do not put it in `[moneywiz.balances]`.
5. `moneywiz coverage` counts every MoneyWiz row of the plan up to the newest statement whose closing cash MoneyWiz's balance equals, when the broker files pair every share move (no trade missing, none off, same shares). Its source then reads "statement cash to <day>". Hand-typed rows before the first cash flow row (a vest's setup, a reconcile) are proven this way (test `stock_plan_statement_cash_with_paired_shares_covers_the_rows_before_it`).

cashcast reads only "CASH FLOW ACTIVITY BY DATE": the row's date, activity type and amount (USD). Descriptions and comments are never read. A sale is a cash row ("sold"), not a trade leg: plan shares come from vests and payroll purchases, so the sale is the only cash a trade brings. Keep each sale's cash in MoneyWiz as a deposit in the stock plan account.

When `[moneywiz.investments]` maps the account, the sync treats the statements as its cash (test `a_stock_plan_account_offsets_share_moves_and_its_start_cash`):

- Each MoneyWiz buy and sell row gets an offset (memo `cashcast fix #<row>`): it moves shares, and the statement holds the cash. The rows stay.
- One start correction (memo `cashcast start cash fix`) on the first statement day sets the cash before it: the bank balance less the statement rows, or 0. The opening balance and early rows stay. Cash far below 0 there means a statement is missing, and the sync holds the account.
- `edit --fixes` deletes and folds nothing in this account, and keeps its opening balance (test `edit_fixes_keeps_a_stock_plan_accounts_corrections`).

### download-stockplan-gains-losses

1. Open **Gains & Losses**. Pick the tax year.
2. Click **Download** (the expanded version if offered).
3. Save it into `private/users/<you>/inbox/`. The figures are estimates; tax forms are the official record.

### download-benefit-history

1. Open **Benefit History**. Expand all grants if the page asks.
2. Click **Download**.
3. Save it into `private/users/<you>/inbox/`.

### check-upcoming-vests

1. Open the stock plan **Holdings** or **Benefit History** page.
2. Read the next vest dates and share counts.
3. Report dates and share counts. Do not report grant numbers.

### sell-shares

The agent does not do this. It creates a reminder for you with the amount and date.

## API

`bin/cashcast api download etrade` saves two years of brokerage transactions as `etrade-transactions-<id>-<page>.json`, one file per page, and replaces them on the next run. Use it **or** the transactions CSV, not both: the two hold the same rows, and holdings would count them twice. Remove `etrade*.csv` transaction exports from the inbox when you switch to the API. Stock plan files (benefit history, gains and losses) still come from the browser.

1. Request a LIVE Individual consumer key at https://developer.etrade.com/getting-started (User Intent Survey, then API Agreement). A sandbox key does not work: cashcast calls api.etrade.com.
2. Skip the Market Data Agreement; cashcast only reads balances and transactions.
3. Put the key in `private/users/<you>/.env` as `CASHCAST_ETRADE_KEY=...` and `CASHCAST_ETRADE_SECRET=...`. Never commit them.
4. Sign-in uses OAuth 1.0a: the app gets a request token, you approve it in your browser, and the app swaps the verifier for an access token. You type the verifier, not the agent.
5. Access tokens expire often; expect to approve again.

## Sources

- https://apisb.etrade.com/docs/api/account/api-transaction-v1.html
- https://developer.etrade.com/getting-started
- https://developer.etrade.com/support/frequently-asked-questions
- https://apisb.etrade.com/docs/api/authorization/authorize.html
- https://us.etrade.com/stock-plans
- https://traderfyles.helpscoutdocs.com/article/4-how-to-export-trade-history-from-e-trade

Last checked: 2026-11-16
