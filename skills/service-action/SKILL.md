---
name: service-action
description: Does a task on one of the person's financial services (Scotiabank, TD, Wise, Wealthsimple, Casa, Kraken, Chexy, PayPal, Paysend, Google Sheets, E*TRADE, MoneyWiz) by browser or API, safely by risk level. Use when the user asks to download an export, check a balance, change a payment card, set autopay, or similar. Don't use to move money, trade or pay.
---

# Service action

1. Run `bin/cashcast action SERVICE ACTION` (ids: `bin/cashcast connectors show SERVICE`).
2. By the printed risk:
   - **money**: stop. Say it became a reminder; the user does it.
   - **settings**: show the exact change, ask for a yes in chat, and click
     the final button only after it.
   - **read**: do it.
3. Browser steps come from the printed playbook. The user signs in; never
   type passwords or 2FA codes. Report what is on screen without account
   numbers or other people's names.
4. For API reads, run `bin/cashcast api balances SERVICE` or
   `bin/cashcast api download SERVICE`, then `bin/cashcast import`.
