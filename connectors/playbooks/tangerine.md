# Tangerine playbook

## Before you start

- You need an online banking sign-in. Use a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money.

## Export your data

1. Sign in to Tangerine in a web browser.
2. Open the account (chequing, savings or credit card), then **Download transactions**.
3. Pick the date range and **CSV** (or OFX/QFX).
4. Save the file into `private/users/<you>/inbox/`. Name it after the account, for example `tangerine-mastercard.csv`, so cashcast guesses its type.
5. Repeat for each account.

## Actions

### download-transactions

First setup: go back as far as the site lets you, one file per account. Overlapping files are fine: the import drops duplicate rows.

1. Follow "Export your data".
2. Verify: the file ends in `.csv` (or `.qfx`/`.ofx`) and the period on screen matches.
3. Run `bin/cashcast fetch collect` if the file went to Downloads, then `bin/cashcast import`. The import report names the file and its row count, never the rows.

### read-balances

1. Sign in at https://www.tangerine.ca/app/#/login/login-id (the person signs in).
2. Read the current balance of each chequing, savings and credit card account on the accounts overview. Change nothing.
3. Pass them to `bin/cashcast weekly --balance ID=AMOUNT ...` (ids from `[[banking.accounts]]` and `[[wallet.cards]]` in profile.toml).

### send-etransfer

Money moves are the person's: cashcast turns this into a reminder. The agent never sends money.
