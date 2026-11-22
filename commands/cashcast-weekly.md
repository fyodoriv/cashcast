---
description: Run the Sunday cashcast weekly in full, without questions, then show one results page before any improvements
allowed-tools: Bash(bin/cashcast:*)
---

The owner's Sunday weekly. Do every step without asking questions. Only these stay with the owner:
1Password approvals, 2FA codes, consent grants, closing a MoneyWiz window you cannot close, and every
money move (cashcast never moves money). Follow `.claude/skills/cashcast/SKILL.md` "Weekly session"
for the exact commands of each step.

The owner expects exactly these 7 results, in this order:

1. **Real data from every bank today.** Fresh downloads of all accounts, cards and investments
   (Wealthsimple, Scotiabank, TD and CIBC as QFX, Wise, E*TRADE, Sun Life, Kraken, Casa, PayPal), and
   today's balances. Investments too: Kraken daily prices and the Sun Life history (their playbooks
   in `connectors/playbooks/`). Name each one you could not get and why.
2. **MoneyWiz matches the banks from the last analyzed row on. Write it, do not ask.** Run
   `moneywiz sync`, then `moneywiz edit --fixes` (each after its dry run) and `moneywiz categorize`.
   Done when a dry run shows 0 to add and `moneywiz diagnose` reads "Last day off: none" per account,
   or each exception is named with its cause.
3. **The fewest transfers, with easy-to-type amounts**, between all accounts
   (`bin/cashcast transfers --balance …`): pay cards in full, keep each bank at its keep amount.
4. **Charges on the wrong card**: `bin/cashcast cards missed` (totals per card and category, never one
   charge), plus the 30-day view (`--since`), so the owner learns which card to use.
5. **Forecast and what to do next**: `forecast` and `backtest`, suggestions and pivots. Put nothing
   into Reminders or Calendar until the owner confirms the direction: `weekly` without `--sync` only
   reads the task sync. After the owner's yes, run `bin/cashcast sync all`.
6. **Verdict: can this weekly replace typing into MoneyWiz, except cash?** Answer yes or no with
   `moneywiz coverage` and `moneywiz diagnose` evidence, and list what still needs a hand entry.
7. **One results page** that covers 1 to 6 in full: a self-contained HTML file in
   `~/.local/state/cashcast/reports/` (private, never published), sent to the owner. Then ask which
   next actions to take.

Every question and every hand step explains itself (owner rule, 2026-11-22): say what the thing is
in plain words (for example "a MoneyWiz category is the label on a row, such as Housing > Rent,
that its reports group by"), why it needs the owner, each choice and what it changes, and where to
do it.

Closing a data gap (a download, a sync, an edit) is part of the results. Only after the owner has
the page: work on improvements (code and logic fixes, PRs, new tasks). Results first,
improvements second (owner rule, 2026-11-22).
