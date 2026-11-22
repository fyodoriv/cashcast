# RBC Royal Bank playbook

## Before you start

- You need an online banking sign-in. Use a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money.

## Export your data

1. Sign in to RBC Online Banking in a web browser.
2. Open **Account Services** (or the account menu), then **Download Transactions**.
3. Pick the accounts, the file type **Comma Separated Values (.csv)** (or Quicken/Money for QFX/OFX), and the date range.
4. Click **Continue** and save the file into `private/users/<you>/inbox/`.
5. One CSV may hold several accounts; cashcast reads the CAD$ column. A US-dollar account (USD$ column) needs its own QFX/OFX download.

## Actions

### download-transactions

First setup: go back as far as the site lets you, one file per account. Overlapping files are fine: the import drops duplicate rows.

1. Follow "Export your data".
2. Verify: the file ends in `.csv` (or `.qfx`/`.ofx`) and the period on screen matches.
3. Run `bin/cashcast fetch collect` if the file went to Downloads, then `bin/cashcast import`. The import report names the file and its row count, never the rows.

### read-balances

1. Sign in at https://www1.royalbank.com/cgi-bin/rbaccess/rbunxcgi?F6=1&F7=IB&F21=IB&F22=IB&REQUEST=ClientSignin&LANGUAGE=ENGLISH (the person signs in).
2. Read the current balance of each chequing, savings and credit card account on the accounts overview. Change nothing.
3. Pass them to `bin/cashcast weekly --balance ID=AMOUNT ...` (ids from `[[banking.accounts]]` and `[[wallet.cards]]` in profile.toml).

### send-etransfer

Money moves are the person's: cashcast turns this into a reminder. The agent never sends money.
