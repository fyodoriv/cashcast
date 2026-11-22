# 6. MoneyWiz: read a copy of its database, write through its URL scheme

- **Status**: accepted
- **Date**: 2026-10-19

## Context

The owner keeps MoneyWiz as their ledger. They want to type only cash and
let one command bring in every bank and card row, with wrong amounts fixed.
MoneyWiz has no public API.

- Its data is a Core Data SQLite store. Entity ids and column suffixes
  change between versions.
- An open-source tool that wrote that store with raw SQL broke MoneyWiz's
  Core Data and iCloud sync (MoneyWiz later crashed). That tool now writes
  only through a Core Data host gated on one exact model version.
- MoneyWiz's documented URL scheme (`moneywiz://expense|income|transfer`
  with `save=true`) creates rows through the app itself. It cannot edit or
  delete.

## Decision

- The engine (`engine/src/moneywiz.rs`) reads a temporary copy of the
  database, read only. It maps entities and columns by name, never by id.
- `moneywiz sync` matches export rows with MoneyWiz rows per linked account:
  same amount within `match_days`, then same payee with another amount.
- Missing rows go to MoneyWiz as URL-scheme links. A card payment seen from
  both sides becomes one transfer.
- Amounts that differ get a correction row (memo `cashcast fix #<id>`), and
  a remaining balance gap gets one balance correction, so one command leaves
  MoneyWiz equal to the bank (the owner's flow). `fixes = "list"` only lists
  them. MoneyWiz rows the bank does not have are listed in
  `out/moneywiz-fixes.md`, never removed.
- Nothing writes the database file. The new exit has its own consent scope,
  `local:moneywiz`.

## Consequences

- MoneyWiz keeps its sync and history intact. Rows cashcast adds are
  ordinary MoneyWiz rows.
- The URL scheme cannot edit in place, so a wrong amount is a list item or a
  correction row. Decision 0007 adds in-place edits through a Core Data
  writer built against MoneyWiz's own model (`fixes = "edit"`).
- The reader is tested on synthetic stores only, until the owner runs it on a
  real one (task `moneywiz-real-store`).
