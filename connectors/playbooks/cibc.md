# CIBC playbook

## Before you start

- You need a CIBC Online Banking sign-in. Use a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money.

## Export your data

1. Sign in at https://www.cibconline.cibc.com/.
2. Open the account, then **Download transactions**.
3. Pick the date range and **Quicken (QFX)** or **Microsoft Money (OFX)**.
4. Save the file into `private/users/<you>/inbox/`.
5. Repeat for each account and card.

## Actions

### download-transactions

First setup: go back 24 months, or as far as the date picker allows (about 18 months), in one or more ranges. Overlapping files are fine: the import drops duplicate rows.

1. Follow "Export your data".
2. Verify: the file ends in `.qfx` or `.ofx` and the period on screen matches.

### read-balances

1. Sign in at https://www.cibconline.cibc.com/ (the person signs in).
2. **My Accounts** lists deposit accounts and credit cards with balances.
3. Read the chequing balance and each CIBC credit card's current balance.
4. Report each as `--balance ID=AMOUNT` with the profile ids (for example `cibc-smart-tier1`, `cibc-dividend-visa-infinite`). Do not report account or card numbers.

### product-change-card

Done by phone. The agent lists the target card, its annual fee and the questions to ask (same account number? prorated fee refund? effect on credit history?). The user calls CIBC.

### send-etransfer

The agent does not do this. It creates a reminder for you with the amount and date.
