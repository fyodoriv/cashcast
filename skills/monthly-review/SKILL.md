---
name: monthly-review
description: Runs the 5-minute monthly money review with the person: import, statements, bank minimums, rent card counts, credit scores, cash sweep, cash back, budget, and pivot signals. Use when the user says "monthly review", "money review", or a review reminder fires. Don't use for a single question about one card.
---

# Monthly review

1. Ask the user to put last month's exports in their inbox, then run
   `bin/cashcast import`.
2. Run `bin/cashcast review`. Walk the numbered steps one at a time; the user
   answers yes or no. Do not open statements yourself.
3. Signals: if the review says **Pivot**, run `bin/cashcast strategy` and
   explain what changed (spending method, budget, a life change).
4. Money steps (sweep, pay-downs, top-ups) are done by the user. Run
   `bin/cashcast reminders preview`, and ask the user to run
   `bin/cashcast reminders sync` in Terminal (after a one-time
   `bin/cashcast consent grant local:reminders`).
5. Finish with the next review date.
