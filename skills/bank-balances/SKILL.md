---
name: bank-balances
description: Checks the balance of every bank account and card in the person's profile (TD, Scotiabank, CIBC, Wealthsimple, Wise, and the rest in connectors/) in the browser after they sign in, then runs `bin/cashcast transfers` to recommend the e-Transfers that bring each bank to its keep amount plus its card's paydown - at most the daily e-Transfer limit per sending account, in amounts quick to type (999, 2000, 1555). Use when the user says "check my balances", "balance my banks", "what should I transfer", or before the monthly paydowns. Don't use to send the transfers (the person sends each one), to read exports (use fetch-exports), or for investment accounts.
---

# Bank balances and transfers

You read balances; the code plans; the person sends every e-Transfer.

## 1. Before you start

1. `bin/cashcast consent list`: reading bank pages needs `agent:browser-read`
   (1 day), and the plan needs `agent:strategy`. Missing? Give the person the
   `bin/cashcast consent grant SCOPE` line for their own Terminal, then wait.
2. Read the ids from the person's `profile.toml`: `[[banking.accounts]]` (each
   with `keep`) and `[[wallet.cards]]` (each with `target`). `[transfers]` sets
   `hub` (the account that funds the others), `daily_limit` (default 3000
   per sending account) and `buffer` (a safety margin above `keep`, in
   percent: default 5, at most 20).

## 2. Read each bank

For each bank in the profile, one tab at a time:

1. Open its `login_url` (`bin/cashcast connectors show SERVICE`). The person
   signs in. Never type a password or a code.
2. Follow the playbook's `read-balances` action
   (`connectors/playbooks/<id>.md`; Wise: `check-balances`). Wise's bot
   check often blocks an embedded browser: prefer
   `bin/cashcast api balances wise` (needs `actions:api` and
   `CASHCAST_WISE_TOKEN`), and pass its CAD line as the profile's Wise account.
3. Note each balance against its profile id. A card balance is what is owed
   now. Do not copy account or card numbers anywhere.
4. A bank that will not load after two tries: skip it and say so. The plan
   lists it under "Not checked".

## 3. Plan

```bash
bin/cashcast transfers --balance ID=AMOUNT --balance ID=AMOUNT ...
```

Give every balance you read, accounts and cards. The plan:

- pays each card that owes more than its `target` in full ("current
  balance", one tap, the longest time until the next paydown), the hub's own
  card too; a card in credit is left alone (pass it as a negative balance);
- funds a bank only when its card payment would take it below `keep`, then
  to `keep` plus `buffer` %; a bank above `keep` gets no top-up;
- sends a bank's surplus straight to a short bank when it covers the whole
  need (one move, not two), else back to the hub (rounded down);
- keeps each sending account at or under `daily_limit` a day, so a big gap
  spreads over days;
- rounds to whole dollars that are easy to type (555, 500, 1500), at most
  10 % (or $50) off.

## 4. Tell the person

- The moves per day, as the plan prints them.
- Any line under "Check" (the hub would end below its keep): say what to do
  first, for example wait for pay.
- Offer a dated reminder (`[[tasks]]`, `kind = "money"`) for a move on day 2
  or later. Add it after a yes. Reminders reach Apple Reminders only after the
  person runs `bin/cashcast sync all` in Terminal.

Never send, schedule or approve an e-Transfer. Never put balances in git,
memory or notes.
