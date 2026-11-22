# 7. MoneyWiz edits: a Core Data writer, tested on a copy first

- **Status**: accepted; not yet run on a real MoneyWiz store (task `moneywiz-edits-real-store`)
- **Date**: 2026-10-19

## Context

Decision 0006 writes MoneyWiz rows only through its URL scheme, which can
create rows but cannot edit or delete them. The owner now needs cashcast to
change existing rows (amount, category, payee, memo) and to add, rename and
delete categories automatically. MoneyWiz has no public API, and the Mac app
is open on the owner's computer.

Options looked at on 2026-10-19:

| Option | Edits rows | Categories | Sync safe | Verdict |
|---|---|---|---|---|
| URL scheme (`moneywiz://expense…`) | no | creates one only as a side effect of a new row | yes | keep for new rows |
| AppleScript dictionary | if the app has one | if the app has one | yes: the app saves | best, if it exists. No public sign that it does |
| Shortcuts (App Intents, SiriKit) | if an edit action exists | if one exists | yes | second best. No public sign that edit actions exist |
| Core Data writer, as in marcomc/moneywiz-tools | yes | yes | yes when it saves through MoneyWiz's own model with persistent history, with the app closed; moneywiz-tools verified this for payee changes on one model | **chosen**: needs no UI knowledge, works in bulk, checks every row before it changes |
| The MoneyWiz window, through macOS Accessibility | yes | yes | yes: the app's own code saves | fallback; slower and breaks when the UI changes |
| Raw SQL on the database | yes | yes | **no**: it broke Core Data and iCloud sync for another tool | rejected (0006) |
| Edit a backup, then restore it | yes | yes | **no**: a restore replaces every row and resets sync | rejected |
| CSV import | no (adds rows) | creates categories by name | yes | no help with edits |
| The iCloud or SYNCbits sync service | yes | yes | unknown; private protocol | rejected: reverse engineering a private service |

The help site that lists MoneyWiz features (help.wiz.money) was not reachable
from the agent's sandbox, so the AppleScript and Shortcuts rows rest on a
web search that found nothing, not on proof that nothing exists.
`bin/cashcast moneywiz probe [--ui]` checks the real app for both, and for
an Accessibility-drivable window.

## Decision

- `bin/cashcast moneywiz edit` changes rows, opening balances and
  categories through `engine/moneywiz-writer` (Swift, built on first use with
  `swiftc`). It loads MoneyWiz's own Core Data model from the app, opens the
  store with persistent history, and saves once after every edit is checked;
  any failure rolls back. The open, the checksum check, the transaction author
  and the value transformers follow moneywiz-tools (MIT).
- The owner lists edits in `inbox/moneywiz-edits.toml`; `--fixes` (or
  `[moneywiz] fixes = "edit"` in every sync) adds the wrong amounts and the
  opening balance a sync finds. The engine resolves each row edit to exactly
  one MoneyWiz row and names it by its `GID`; the writer checks the expected
  amount and date again.
- Every run first applies the plan to a **copy** of the database and the
  engine checks that the copy holds every change. Live writes then need:
  `[moneywiz] writer = "core-data"`, the `local:moneywiz` consent, and
  `[moneywiz] writer_checksum` equal to the model checksum, which the owner
  sets after the first copy test passes. A MoneyWiz update changes the
  checksum and stops live writes until the owner allows the new one.
- The live write quits MoneyWiz, backs the database up to
  `data/moneywiz-backups/`, writes, checks again, and reopens MoneyWiz so it
  syncs the change.
- Moves (2026-11-08): a row may move to another account in the same
  currency (`[moneywiz.merge]`); the other side of a transfer follows. A
  coin transfer is unlinked instead: both sides go, and the other account
  keeps a plain row of the same day and amount. `[moneywiz.retire]` empties an
  account the same way, for the owner to delete.
- Owner rule (2026-11-08): MoneyWiz is a mirror of the banks and brokers;
  every row may change or go, hand rows are never trusted.
- Amounts (2026-11-08): transfers, split and foreign-currency rows take the
  bank amount too. Split parts scale with the row, the last part takes the
  rounding; a foreign row keeps its original amount and its rate follows;
  the other transfer side's sender or recipient amount follows.
- The writer still refuses: amounts of investment rows; deleting a category
  with subcategories, or with rows but no `move_to`.
- Raw SQL and backup restores stay out. If the writer fails on the real
  store, the probe's window path is the fallback.

## Consequences

- Edits are in place: no correction rows, and the fix list shrinks to rows
  only the owner can judge (duplicates, charges not posted yet).
- Whether MoneyWiz's iCloud or SYNCbits sync carries these edits to other
  devices is unproven until the owner checks "Up to Date" on a second device.
  moneywiz-tools saw iCloud export its payee changes; category and amount
  changes are not verified anywhere yet.
- A new category copies the required settings (such as income or expense)
  of its parent, or of a top-level category.
- The writer is not compiled or run in CI (CI is Linux). Its logic is
  mirrored by a fake writer in the e2e tests; the real one is proven only on
  the owner's Mac.
