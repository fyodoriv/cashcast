# Paysend playbook

## Before you start

- You need a Paysend account.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never sends money.

## Export your data

Paysend has no CSV export. Transfers are paid by card, so they show up in that card's bank export. Import the card statement instead. The PDF statement below is for your records; do not put it in `private/users/<you>/inbox/`.

## Actions

### view-transfer-history

1. Sign in from https://paysend.com. Open **Transfers**.
2. Read each transfer's date, amount sent, fee and status.
3. Report totals. Do not report recipient names or card details.

### request-statement

1. Open the **Profile** tab and the transfer statement option.
2. Pick the period. Enter the user's own email address (ask the user which one).
3. Paysend emails a PDF. It lists outgoing transfers only.

### get-transfer-receipt

1. Open **Transfers** and select the transfer.
2. Tap **Help & More** (or the ••• menu) > **Get receipt**.
3. Enter the user's own email address. One transfer per receipt.

### send-money

The agent does not do this. It creates a reminder for you with the amount and date.

## API

Paysend has no personal API.

## Sources

- https://help.paysend.com/hc/en-us/articles/20452516351773-Transfer-history
- https://help.paysend.com/hc/en-us/articles/20452609805981-How-can-I-get-a-transfer-receipt
- https://paysend.com/en-uk/help/category/5092697897501/section/5989421778973/article/5989487615517

Last checked: 2026-10-18
