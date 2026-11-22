---
name: money-strategy
description: Builds and explains the person's full money strategy - card routing, credit targets, reserve and investing order, tax moves, payments, budget, alternatives and a dated checklist. Use when the user asks "how should I use my cards", "what should I do with my money", "make me a plan", or after their profile changes. Don't use for a month-by-month list of which card to use (use card-calendar) or to execute any transaction.
---

# Money strategy

1. Check the profile has the basics: province, income, room, wallet cards
   (with limits and statement close days), bank packages, rent. Ask only for
   what is missing; say the privacy line first ("saved only in your profile
   on this computer; this chat goes to the AI provider").
2. Run `bin/cashcast strategy` (add `--market` only if `net:market-data` is
   granted and the user wants live prices).
3. Explain in this order: the net yearly number, the "where to pay" table,
   the rent rule, the reserve and invest order, then the top alternative.
4. Mark estimates as estimates. Say once that this is not licensed advice.
5. Offer `bin/cashcast reminders preview` for the dated checklist.

## Which card right now

The "where to pay" table uses yearly averages. When the person asks which
card to use **now** (or before a big purchase or the rent charge):

1. Read each card's balance the `balances` / `bank-balances` way (API first,
   browser second; needs `agent:browser-read`), or use balances recorded
   under the card's id.
2. Run `bin/cashcast cards now --balance CARD=AMOUNT ...` (needs
   `agent:strategy`). It counts each cap from the imported card rows since
   the card's `cap_reset`, the rent charges against `rent_max`, and the
   credit left.
3. Say the card per spending kind with its rate, cap left and reset date,
   and credit left. Read out every "Check" line: an unknown reset date, stale
   rows (offer `bin/cashcast fetch plan`), a pending product change, a
   balance above target. "Cap saved for rent" means the card's bonus cap
   is kept for the rent charges still left: say so, and do not suggest
   moving that spending onto the card.
4. Never switch a payment card, change the rent service or apply for a card:
   offer a one-time reminder or `skills/bank-change` with the person's yes.
