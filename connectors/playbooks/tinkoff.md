# T-Bank (Tinkoff) playbook

## Before you start

- You need a T-Bank sign-in. Use a web browser. Sign-in usually asks for an SMS code.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never moves money.

## Export your data

1. Sign in at https://www.tbank.ru/login/.
2. Open the account or card, then **Операции** (Operations).
3. Pick the longest period (two years for a first setup).
4. Click **Выгрузить** (Export) and pick **CSV**.
5. Save the file into `private/users/<you>/inbox/`.

## Actions

### download-operations

1. Follow "Export your data".
2. Verify: the file ends in `.csv`. `bin/cashcast columns` shows the format `tinkoff-csv`.
   It reads the payment amount and currency (the account's currency), keeps only rows with status OK,
   and never reads the card number.

### read-balances

1. Sign in at https://www.tbank.ru/login/ (the person signs in).
2. The home page lists each account and card with its balance.
3. Report each as `--balance ID=AMOUNT` with the profile ids. Do not report account or card numbers.

### send-transfer

Human only. The agent never moves money: it becomes a reminder for the person.
