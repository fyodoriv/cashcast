# 12. MoneyWiz categories change through numbered, reversible migrations

- **Status**: accepted; not yet run on a real MoneyWiz store (task `moneywiz-categories-real-store`)
- **Date**: 2026-11-01

## Context

The owner wants their MoneyWiz income and expense categories kept tidy:
in sync with the live list, with suggested changes, applied "safely and
reliably with rollbacks, like migrations". Decision 0007 already edits
categories in place (add, rename, delete with `move_to`) through the Core
Data writer, but one edit file has no history, no drift check and no undo
other than restoring a backup.

Options looked at on 2026-11-01:

| Option | History | Undo | Drift check | Verdict |
|---|---|---|---|---|
| Keep `inbox/moneywiz-edits.toml` | none | backup restore only | none | too weak for tree-wide changes |
| Snapshot the tree, diff, write the target tree | the snapshots | write the old tree back | yes | a diff cannot tell a rename from a delete plus an add, so rows would move by guess |
| **Numbered migration files with a computed inverse** | the applied record | the inverse, then the backup | yes: a tree fingerprint | **chosen**: each step says what it means, like a database migration |

## Decision

- Migrations are `inbox/moneywiz-migrations/NNNN-name.toml`, applied in
  number order. Each holds `expect_tree` (a fingerprint of category paths and
  types, no rows) and `[[op]]` steps: `add` (top level with `kind`),
  `rename` … `to`, `move` … `to` (optionally `only` income or expense rows,
  or `rows` by GID), and `delete` with `move_to`.
- `moneywiz categories suggest` writes proposals as the next file. It never
  applies them. The owner reviews, edits or deletes steps first.
- The engine refuses a migration when the live tree's fingerprint is not
  `expect_tree`, or when the writer's fixed order (adds, renames, rows,
  deletes) would change its meaning (a name removed and created again in one
  migration).
- Before a write, the engine computes the inverse and checks it: run on the
  tree after the migration, it must give back the same fingerprint and put
  every row back in its category. A migration without such an inverse is
  refused.
- The write is decision 0007's flow, unchanged: a test on a copy, the
  `writer_checksum`, MoneyWiz quit, a backup in `data/moneywiz-backups/`,
  the Core Data writer, a check (now including the whole tree), then MoneyWiz
  reopens.
- `data/moneywiz-migrations.json` (mode 600) records each applied migration:
  its file, fingerprints before and after, the backup folder and the inverse.
  The inverse names rows by Core Data GID only, no amounts or payees.
- `moneywiz categories rollback` applies the inverse of the last applied
  migration through the same flow. If the tree drifted since, it stops and
  names the backup to restore by hand as the last resort (playbook).

## Addendum 2026-11-02: the Categories view

- The web app's Categories view reads `moneywiz categories json`: each
  MoneyWiz category beside the cashcast category (`[budget]`,
  `[strategy.categories]`) it matches, the ones MoneyWiz lacks, and the
  suggestions. Names and counts only.
- Changes picked there become one migration file (`NNNN-web.toml`, through
  `moneywiz categories new --ops`), previewed with `migrate --dry-run` and
  applied by the same `migrate` flow: a test on a copy, a backup, the live
  write. Undo is the same `rollback`.
- A live `migrate` or `rollback` now needs `--yes` in the CLI. In the web app,
  any action run with `yes` needs the confirm step.

## Consequences

- Category ids in MoneyWiz change on a rollback (a deleted category comes
  back as a new one with the same name and type). Rows are moved back by
  GID, so reports by name are unchanged.
- Changing a category's type in place is not offered: a suggestion moves the
  rows into a category of the right type instead.
- Splitting rows (more than one category) cannot move by row; the writer
  refuses them, and the test on a copy fails before any live write.
- The format is hard to change once migrations are applied: a new field must
  stay optional, and the applied record must keep reading.
- The category type mapping (1 expense, 2 income) follows ileodo/moneywiz-api
  and is unverified on the owner's store until `moneywiz categories` shows the
  right types there.
