# Wealthsimple playbook

## Before you start

- You need a Wealthsimple sign-in and a computer. CSV downloads work on the web, not in the app.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money, buys or sells.

## Export your data

**Rule:** every Wealthsimple export (CSV or ZIP) goes straight into
`private/inbox/` (or `private/users/<you>/inbox/`). Unzip ZIPs there and keep
the file names. Then run `bin/cashcast import`. Never leave exports in
`~/Downloads`.

Monthly statements (best for chequing and the credit card):

1. Sign in at https://my.wealthsimple.com/app/login.
2. Go to **Profile > Documents**.
3. Under **Filters**, pick the account and the monthly statement type.
4. Click **Download CSV** next to each month. For many months, use **Generate document** to get a ZIP.
5. Save the CSVs into `private/users/<you>/inbox/`.

Activity export (all transactions in one file):

1. Open the **Activity** tab.
2. Filter by account and set a custom date range.
3. Click **Download CSV**.
4. Save it into `private/users/<you>/inbox/`.

## Actions

### download-monthly-statement-csv

First setup: download every month back to 24 months ago (or the account's first month) for each account: chequing, the credit card, TFSA, FHSA and each non-registered account. **Generate document** gives them as one ZIP; save the ZIP's CSV files into the inbox. Overlapping files are fine: the import drops duplicate rows.

1. Follow "Monthly statements" above.
2. Verify: the file name shows the account type and the month. Statements arrive by the 8th business day of the next month.

### download-activity-export

1. Follow "Activity export" above. Or use **Profile > Documents > Generate document > custom statement > CSV**.
2. Verify: the date range on screen matches. This file has no running cash balance.

### set-card-autopay

1. Open the credit card > payments > recurring (automatic) payment.
2. Pick **Statement balance** (pays the last statement in full) or **Total balance** (also pays newer charges).
3. The paying account must be a Wealthsimple chequing account. Check that it has enough money on the due date.
4. Ask the user to confirm this exact change before clicking the final button.

### check-tfsa-rrsp-contributions

1. Select the TFSA. Scroll to **TFSA contributions**. Read the year-to-date total.
2. Do the same for the RRSP.
3. Tell the user: Wealthsimple counts only its own accounts and does not track room. For room, check CRA My Account (it can lag until spring).

### buy-etf

The agent does not do this. It creates a reminder for you with the amount and date.

### sweep-cash

The agent does not do this. It creates a reminder for you with the amount and date.

### read-investment-balances

1. Sign in at https://my.wealthsimple.com/ (the person signs in).
2. The home page lists each account with its value. Read every investment account: TFSA, RRSP, FHSA, non-registered (Personal), and Crypto.
3. Record the CAD accounts in one line: `bin/cashcast api record wealthsimple --set "TFSA=…,RRSP=…,FHSA=…,Personal=…,Crypto=…"`. Names with TFSA, RRSP, FHSA, RESP or Crypto in them land in that bucket; anything else is non-registered.
4. A USD account (for example a USD RRSP): record it in its own call with `--currency USD`, for example `--set "RRSP USD=…"`.
5. Never record an account number: a name with 4 or more digits in a row is refused.

### read-balances

1. Sign in at https://my.wealthsimple.com/ (the person signs in).
2. The home page lists each account. Open **Chequing** and read its available balance; open the credit card and read its current balance.
3. Report each as `--balance ID=AMOUNT` with the profile ids (for example `wealthsimple-chequing-premium`, `wealthsimple-visa-infinite-plus`). Do not report account numbers.

## API

Wealthsimple has no public API for clients. Use the CSV downloads.

## Sources

- https://help.wealthsimple.com/hc/en-ca/articles/360056580894-Find-your-monthly-statements
- https://help.wealthsimple.com/hc/en-ca/articles/35654428540571-Request-a-custom-statement
- https://help.wealthsimple.com/hc/en-ca/articles/39449252453019-View-your-account-activity-and-running-balance
- https://help.wealthsimple.com/hc/en-ca/articles/37750166795035-Review-and-download-your-credit-card-activities
- https://help.wealthsimple.com/hc/en-ca/articles/32840779706779
- https://help.wealthsimple.com/hc/en-ca/articles/32167106170523-View-your-TFSA-contributions

Last checked: 2026-10-18
