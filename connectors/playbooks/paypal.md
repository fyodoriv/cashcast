# PayPal playbook

## Before you start

- You need a PayPal sign-in and a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never sends money.

## Export your data

1. Sign in at https://www.paypal.com/signin.
2. Go to **Activity** (or **History**).
3. Click the **Download** icon, then **Custom** (or **Statements and reports > Activity report**).
4. Pick a date range of up to 12 months and **CSV**. Click **Create report**.
5. Small reports download at once. For large ones, wait for PayPal's email.
6. Save the CSV into `private/users/<you>/inbox/`.

## Actions

### download-activity

1. Follow "Export your data".
2. Verify: the report list shows the date range and the status says ready.
3. For more than 12 months, make one report per year.
4. Request the reports first, then fetch other services: building them takes a few minutes.
5. The date fields follow the account locale: a Canadian account reads `DD/MM/YYYY`. Check the range in the report list.
6. Both CSV layouts work: the full one (`Gross`, `Net`) and the "Balance affecting" one (`Amount`, `Fees`, `Total`).

### api-transaction-search

1. Get an OAuth2 token with the client ID and secret (client-credentials grant).
2. Call `GET /v1/reporting/transactions` with `start_date` and `end_date` at most 31 days apart.
3. Loop over months, up to 3 years back. New transactions can take 3 hours to appear.
4. Save results only under `private/users/<you>/`.

### check-balance

1. Browser: read the balance on the home page.
2. API: `GET /v1/reporting/balances`.

### change-default-funding-source

1. Open **Wallet** (or **Settings > Payments**). Find the preferred way to pay.
2. Pick the new card or bank account. Read which one is set now and which one will be set.
3. Ask the user to confirm this exact change before clicking the final button.

### send-money

The agent does not do this. It creates a reminder for you with the amount and date.

## API

1. Sign in at https://developer.paypal.com. Open **Apps & Credentials**, switch to **Live**.
2. Create a REST app. In the app settings, turn on **Transaction Search** only. Save.
3. Copy the client ID and secret.
4. Put them in `private/users/<you>/.env` as `CASHCAST_PAYPAL_CLIENT_ID=...` and `CASHCAST_PAYPAL_CLIENT_SECRET=...`. Never commit them.
5. If PayPal will not create a live app for a personal account, use the CSV download.

## Sources

- https://www.paypal.com/us/cshelp/article/how-do-i-view-and-download-statements-and-reports-help145
- https://developer.paypal.com/docs/api/transaction-search/v1/
- https://developer.paypal.com/api/transaction-search/v1/search-get
- https://developer.paypal.com/docs/transaction-search
- https://developer.paypal.com/docs/reports/online-reports/activity-download/

Last checked: 2026-10-18
