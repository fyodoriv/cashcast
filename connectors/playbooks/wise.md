# Wise playbook

## Before you start

- You need a Wise account and a web browser. CSV is also in the app, but the web has more formats.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve. Wise may ask for your password again before a statement; you type it.
- The agent never moves money.

## Export your data

1. Sign in at https://wise.com/login.
2. Click your initials > **Statements and reports** > **Statements**.
3. Click **Create a statement**.
4. Pick one currency balance (or Jar), a period of up to 365 days, **CSV**, and English.
5. Save the file into `private/users/<you>/inbox/`.
6. Repeat for each currency and each 365-day window.

## Actions

### download-statement

Browser: follow "Export your data". Verify the currency and dates on the confirmation screen.

API: call the balance statement endpoint with `.csv` for each balance and a window of at most 469 days. Save the result into `private/users/<you>/inbox/`. `bin/cashcast api download wise` saves one file per currency, `wise-<CUR>.csv`, and replaces it on the next download. It covers the last 365 days, or back to `[moneywiz] since` when that is older, asked in 400-day pieces and saved as one file (test `wise_statements_reach_back_in_pieces`). `moneywiz sync` refreshes Wise the same way, so the export account name stays the same. Link it once in `cashcast.toml`: `[moneywiz.accounts]` `"Wise CAD" = "wise-CAD"`.

### check-balances

1. On the home screen, read each currency balance.
2. Or use the API `balances` endpoint.
3. Report totals per currency.

### check-transfer-status

1. Open **Transactions** (or **Activity**) and select the transfer.
2. Read the status and the expected arrival date.
3. Do not report the recipient's name or bank details.

### convert-currency

The agent does not do this. It creates a reminder for you with the amount and date.

### send-transfer

The agent does not do this. It creates a reminder for you with the amount and date.

## API

1. Sign in on the web. Go to **Your account > Integrations and tools** (or **Connect and manage apps**) **> API tokens**.
2. Click **Add new token**. Pick a read-only token if the screen offers one.
3. Finish 2-step verification yourself. Copy the token; Wise shows it only once.
4. Put it in `private/users/<you>/.env` as `CASHCAST_WISE_TOKEN=...`. Never commit it.
5. Notes: Wise supports balance statements by personal token for Canada-based accounts. Some calls may ask for extra strong customer authentication. If the token page is missing on a personal account, use the CSV download.

## Sources

- https://wise.com/help/articles/2736049/how-do-i-download-a-statement
- https://docs.wise.com/guides/developer/auth-and-security/personal-api-token
- https://docs.wise.com/api-reference/balance-statement/balancestatementget

Last checked: 2026-10-18
