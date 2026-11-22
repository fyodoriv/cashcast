# 11. MoneyWiz sync: a durable keyed sent list, and conflicts instead of guesses

- **Status**: accepted; amends 0006
- **Date**: 2026-11-01

## Context

The owner wants to stop typing bank and card rows into MoneyWiz (only cash
stays by hand). That only works if one `moneywiz sync` never adds a row
twice and never brings back a row the owner deleted. A dry run on the
owner's real data on 2026-11-01 showed three ways it could:

- The inbox held a MoneyWiz CSV export. The sync read it as a bank export
  and would have sent 41 of MoneyWiz's own old rows back, cash rows included.
- The sent list forgot each link after one day. A row that MoneyWiz saved
  and the owner then deleted (a duplicate, a refund that never posted) came
  back on the next sync.
- When a bank row matched two MoneyWiz rows equally well, the sync picked
  one and corrected its amount.

The sent list was keyed with Rust's `DefaultHasher`, which may change
between Rust releases, and a plain hash of a date and an amount is easy to
guess.

## Decision

- MoneyWiz's own exports in the inbox are never bank rows for the sync
  (`import` still reads them as the cash source). A MoneyWiz cash account
  follows an export only when `[moneywiz.accounts]` names it.
- `data/moneywiz-sent.json` (version 2) keeps every link sent, for good, as
  an HMAC-SHA-256 of what the link stands for (the bank row, or the MoneyWiz
  row and amount it corrects), with the day sent and whether MoneyWiz was
  seen holding it. The key is 32 random bytes in `data/moneywiz-key`. Both
  files are mode 600.
- A link sent today waits. One sent earlier that MoneyWiz never held is sent
  again. One that MoneyWiz held and lacks now is listed for the owner in
  `out/moneywiz-fixes.md` and not sent again, unless the owner runs
  `moneywiz sync --resend`.
- A bank row with two equally good MoneyWiz partners is a conflict: nothing
  is added or corrected for it, and the owner picks in the fix list.
- One sync runs at a time per user (`data/moneywiz-sync.lock`).
- `moneywiz coverage` measures the goal: the share of MoneyWiz rows (not
  cash) that the inbox exports match, with the gap of each account.

## Consequences

- A sync can add fewer rows than before, never more. Every row it holds back
  is named in the owner's fix list.
- Deleting `data/moneywiz-key` forgets what was sent: rows deleted in
  MoneyWiz before that can come back once.
- Sent lists from before this change still hold back the links sent on the
  same day; after that day they are not needed.
