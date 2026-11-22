# Casa playbook

## Before you start

- You need a Casa account and the web app at https://app.keys.casa.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never signs with a key and never sends bitcoin.

## Export your data

1. Sign in at https://app.keys.casa.
2. In the left menu, select **Reporting & Tax**.
3. Click **Accounting CSV**.
4. Choose the time frame. Click **OK**.
5. Save the file into `private/users/<you>/inbox/`.

## Actions

### export-transaction-history

1. Follow "Export your data".
2. Verify: the download is a `.csv` file.

### check-key-health

1. Open the vault and view its keys.
2. Read the last health check date for each key.
3. Flag any key not checked in the last 6 months (Casa asks for at least twice a year).
4. The health check itself needs your hardware key or phone. You do it; the agent only reminds you.

### check-vault-balance

1. Open the vault. Read the balance in BTC.
2. Record it: `bin/cashcast api record casa --set BTC=…`. The strategy prices it with the BTC price saved by `bin/cashcast api balances kraken`. Do not report addresses or keys.
3. Casa has no API, and cashcast does not look the vault up on a block explorer: that would show your addresses to a third party (owner's choice, 2026-11-01).

### send-bitcoin

The agent does not do this. It creates a reminder for you with the amount and date.

## API

Casa has no public API. Use the CSV export.

## Sources

- https://support.casa.io/knowledge/download-transaction-history-.csv
- https://support.casa.io/knowledge/health-checks-in-the-casa-app
- https://support.casa.io/knowledge/performing-a-health-check-with-keystone

Last checked: 2026-10-18
