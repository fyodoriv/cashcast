---
name: bank-change
description: Makes the bank and card setting changes that the cashcast strategy asks for (switch a chequing package, product-change a card, change a default funding source or the rent card) in the person's browser, one change at a time, each with a yes in chat right before the final click. Use when the user says "make the bank changes", "do the card change", "switch my package", or `bin/cashcast changes` lists open changes. Don't use to move money, pay, transfer, trade, or sign in for the person.
---

# Bank and card changes in the browser

## Before you start

1. Run `bin/cashcast changes`. It lists each open change with its id, why,
   the service, the sign-in page, and its steps command. No changes: stop.
2. Browser actions show account pages to the agent. If the list says so, ask
   the person to run in Terminal: `bin/cashcast consent grant actions:browser`.
3. Pick one change with the person. Run `bin/cashcast changes show ID` and
   `bin/cashcast action SERVICE ACTION` for its playbook steps and risk.
   - risk **money**: stop. It became a reminder; the person does it.
   - no service ("issuer's app, or by phone"): give the person the steps; stop.

## Do one change

1. Open the sign-in page from the list in the browser (the built-in browser,
   or Chrome if the person asks). The person signs in and types any 2FA code.
   Never type passwords or codes, and never use a saved password yourself.
2. Follow the playbook steps to the page with the change. Read what is on
   screen. Report it without account numbers or other people's names.
3. Before the final button, show the exact change in chat, for example:
   "Change the package on chequing ending ••12 from All-Inclusive to
   Unlimited, effective next statement. Click Confirm?" Wait for a clear yes.
   No yes: stop and leave the page as it is.
4. After the yes, click the final button once. Read the confirmation and tell
   the person what changed.
5. Run `bin/cashcast changes done ID`. Then offer the `apple-sync` skill so the
   reminder and calendar event leave their lists.

## Never

- Move money, pay a bill, transfer, redeem to cash, or accept new credit
  (a product change is fine; a new application needs the person).
- Accept new terms or cookie banners beyond what the change needs; pick the
  most private option.
- Click a link from an email or message to reach the bank. Use the sign-in
  page from `bin/cashcast changes` only.
- Do two changes after one yes. Each change gets its own yes.
