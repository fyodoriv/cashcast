# Kraken playbook

## Before you start

- You need a Kraken account and a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never trades or withdraws.

## Export your data

1. Sign in at https://www.kraken.com/sign-in.
2. Open the profile menu > **Documents**. (Kraken Pro: **Statements > Export statement**.)
3. In **Exports**, click **Create export**.
4. Pick **Ledgers** (not Trades). Set start and end dates. Select all fields. Pick **CSV**.
5. Submit. Kraken sends no email. Check the Documents page later; it can take minutes up to a week.
6. Download. If it is a ZIP, unzip it and keep the ledgers CSV.
7. Save the CSV into `private/users/<you>/inbox/`.

### Daily prices for coin moves

Deposits, withdrawals, rewards and swaps carry no price in the ledger. To value them
for MoneyWiz, put Kraken's daily OHLCVT files (`<PAIR>_1440.csv`, for example
`XBTUSD_1440.csv` and `USDCAD_1440.csv`) in `private/inbox/prices/`. They are public
market data from Kraken's "Downloadable historical OHLCVT data" page. The full archive
is about 10 GB in parts; only the daily files are needed, and a ranged read of the
archive gets them without the full download. Top up the latest months with the public
`OHLC` endpoint (`interval=1440`). A coin with no CAD pair uses its USD or EUR pair
times `USDCAD` or `EURCAD`. MoneyWiz takes trades only in an investment account.

## Actions

### export-ledgers

Browser: follow "Export your data". Verify the export type says Ledgers and the status says ready.

API: call `AddExport` with `report=ledgers`, poll `ExportStatus`, then `RetrieveExport`. This needs the Export Data permission. Or page through `Ledgers` directly.

### check-balances

1. Browser: read the balances on the portfolio page.
2. API: call `Balance` with the read-only key. `bin/cashcast api balances kraken` does this, prices each asset from the public ticker (CAD pair, else USD), and saves both for the strategy's Crypto section.
3. Report totals per asset.

### withdraw-cad

The agent does not do this. It creates a reminder for you with the amount and date.

### buy-or-sell-crypto

The agent does not do this. It creates a reminder for you with the amount and date.

## API

1. Sign in. Open the profile menu > **Settings > API** > **Create API key**.
2. Turn on only **Query Funds**, **Query Ledger Entries** and **Export Data**. `api download kraken` uses the export call (`AddExport`), which needs Export Data.
3. Leave **Trade**, **Deposit Funds** and **Withdraw Funds** off.
4. Set a key expiration date.
5. Click **Generate key**. Copy the key and the private key; treat them like a password.
6. Put them in `private/users/<you>/.env` as `CASHCAST_KRAKEN_KEY=...` and `CASHCAST_KRAKEN_SECRET=...`. Never commit them.

## Sources

- https://support.kraken.com/articles/208267878-how-to-export-your-account-history
- https://support.kraken.com/articles/14320306955668-how-to-export-your-account-history
- https://support.kraken.com/articles/360000919966-how-to-create-an-api-key
- https://support.kraken.com/articles/how-to-create-an-api-key-on-kraken-pro
- https://support.kraken.com/br/articles/funding-api-v1
- https://docs.kraken.com/api/docs/rest-api/get-ledgers
- https://docs.kraken.com/api/docs/rest-api/add-export

Last checked: 2026-10-18
