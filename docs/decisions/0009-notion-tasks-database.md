# 9. Finance tasks go to the owner's Notion tasks database, scrubbed

- **Status**: accepted; amends 0005
- **Date**: 2026-10-25

## Context

Decision 0005 removed Notion completely, and 0008 brought Notion pages in
only from an offline export. The owner still plans in Notion next to Apple
Reminders and Calendar, and asked on 2026-10-25 that all three always match.
Asked how far that should go, the owner picked "push tasks only" over
"keep Notion offline" and "full two-way pages".

## Decision

- `bin/cashcast sync all` writes the open tasks to one Notion database the
  owner creates and shares with their own integration. It reads back the
  pages ticked done, and cashcast records them like `changes done`.
- What leaves: task titles with every amount removed (`notion_tasks.scrub`),
  due dates, the task kind and the done tick. Never notes, balances,
  documents or the plan. A test fails when the client sends notes or an
  unscrubbed title.
- It needs the `cloud:notion` consent (365 days), the token
  `CASHCAST_NOTION_TOKEN` in the user's `.env`, and `[notion] tasks_database`
  in `cashcast.toml`. Without all three, the Notion target stays off.
- `src/cashcast/api/notion_tasks.py` is the only Notion API client. Pages
  without a cashcast id are never touched. A page is archived only when its
  task is no longer open at all, and never more than 10 in one run (a plan
  that suddenly shrinks may be incomplete; `sync status` shows the held
  ones, and `sync all --allow-removals` releases them). A ticked page stays
  as the record.
- A tick counts only when it was made after the last sync, on a task that is
  open. An older tick on a task that is open again (next year's deadline, or
  `changes undo`) is cleared, in Notion and in Reminders alike. A tick holds
  until 30 days after the later of the task's due date and the tick. Items the sync itself closed are never
  read back as your tick.
- Documents still come in only from the offline export (0008). The Notion
  connector is still not used for pages.

## Consequences

- The database needs the properties `Name` (title), `Due` (date), `Done`
  (checkbox), `Kind` (select) and `cashcast id` (text). The sync refuses a
  database without them and names the missing one.
- `sync install` runs the sync every hour, so a tick anywhere reaches the
  other two within the hour. `sync status` shows any drift.
- Task titles are written for people, so a new title that names an amount
  is scrubbed, not blocked. Titles can still name a service or a card, and
  Notion's cloud sees those.
