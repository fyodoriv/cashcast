---
name: apple-sync
description: Sets up and updates the person's own Apple Reminders list and Apple Calendar with their cashcast tasks and dated deadlines (card payments, statement dates, tax and contribution deadlines, bank changes). Use when the user says "add this to my reminders", "put the deadlines in my calendar", "set up reminders", "update my calendar", or after a strategy or monthly review changed the tasks. Don't use to read or change any other list or calendar.
---

# Apple Reminders and Calendar

Each person gets one Reminders list and one Calendar of their own. Undated
tasks go to Reminders only; dated deadlines go to both. A re-run updates the
same items (a `[cashcast:<id>]` marker in the notes), keeps what the person
completed, and never touches another list, calendar, or unmarked event.

## Set up (once per person)

1. Optional names: ask whether the defaults ("Cashcast · NAME") are fine. To
   change them, edit the person's `profile.toml`:
   `[reminders] list = "…"`, `[calendar] name = "…"`,
   `[calendar] alert_days = 1` (0 turns the alert off).
2. Run both previews and show the person the counts and the first lines:
   `bin/cashcast reminders preview` and `bin/cashcast calendar preview`.
3. Task titles hold amounts and may sync to iCloud, so writing needs the
   person's consent. Ask them to run in Terminal:
   `bin/cashcast consent grant local:reminders` and
   `bin/cashcast consent grant local:calendar`.
4. Ask them to run in Terminal (the agent sandbox blocks `osascript`):
   `bin/cashcast reminders sync && bin/cashcast calendar sync`.
   macOS asks once to let Terminal control Reminders and Calendar: they click OK.
5. Relay the printed counts (created, updated, removed).

## Update (after a review, a strategy run, or new exports)

1. `bin/cashcast reminders preview` and `bin/cashcast calendar preview`.
2. The person runs `bin/cashcast reminders sync && bin/cashcast calendar sync`.
   Events whose deadline was done or dropped are removed from their calendar.
3. A bank change finished in the browser: `bin/cashcast changes done ID` first,
   so it leaves both.

## Rules

- Never write to Reminders or Calendar any other way (no AppleScript of your
  own, no calendar connector): only the two `sync` commands.
- Money tasks say "You do this: it moves money." Never offer to do them.
- If a sync fails with -600, -1743 or -10822, it is the sandbox or a missing
  macOS permission: give the person the Terminal command; do not retry.
