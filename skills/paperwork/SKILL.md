---
name: paperwork
description: Walks the owner through a hard document process with cashcast's private Paperwork tracker (US-36). First process - Canadian citizenship for adults - steps, document checklist, fees, test, deadlines and the physical-presence day count, each requirement cited from an official IRCC page with the date it was checked. Use when the user says "help with my citizenship application", "what's left for citizenship", "how many days in Canada do I have", "are the citizenship rules still current", or wants another government process added. Don't use to fill official forms, sign in, pay fees or submit (the owner does those), and don't use to read the owner's documents (only the local model route may).
---

# Paperwork: hard document processes, privately

Threat model: [docs/paperwork-threat-model.md](../../docs/paperwork-threat-model.md).
Read it before you change anything here.

To guide the owner through the IRCC online form itself (Chrome, screen by screen, from their
answers file), use the `citizenship-application` skill. This skill is the private tracker.

## Privacy (hard rules)

- Paperwork data is local and encrypted (`<home>/home/paperwork/`, AES-256-GCM, key in
  the macOS Keychain). Never copy it into the shared store, the site, a PR, a test, an
  eval, a log line or a message.
- Use only the redacted agent view. Agents and pipes get statuses, counts and the public
  checklist text, never dates, notes, places or file names:
  `bin/cashcast paperwork`, `paperwork show <id>`, `paperwork presence <id>`,
  `paperwork files <id>`, `paperwork doctor`, `paperwork sources`.
- Never run `--reveal`, `attach` or `export-file`, and never open a `.sqv` file. Those
  are the owner's, in their own terminal.
- Paperwork has no web view and no web route: the Actions view lists it as Terminal only.
  Never open paperwork in any browser tool (the built-in browser, Chrome, Playwright,
  computer use).
- Never read paperwork output in the owner's terminal (a tool that reads their screen
  or terminal tabs), and never copy the vault folder or real data anywhere, not even a
  temp folder.
- Documents go to no hosted model. cashcast calls no model on them.
- Never type passport, PR card or other ID numbers. Never sign in, pay, sign or submit.
  Show the owner the step's exact "how" text and its official source instead.
- If the owner pastes an ID number in chat, do not repeat it and do not save it.
  Paperwork never stores ID numbers.

## Steps

1. Run `bin/cashcast paperwork show citizenship`. Tell the owner the next action and
   the first open items, each with its official source link.
2. Dates and trips are private. Ask the owner to enter them in their own terminal: they
   run `paperwork set <id> <field>` or `paperwork trip <id>` and type the values at its
   prompts (never after the command, where
   `ps` and the shell history would show them). That way the dates never reach a chat.
   Never enter dates, trips or notes yourself: `paperwork set`, `trip`, `untrip`, `temp`,
   `untemp`, `open`, `presence --sign` and `mark --add-note` refuse anything that is not
   a terminal. Never fake a terminal to get around it. If the owner typed a date in chat,
   do not echo it back; point them to the prompt in their own terminal.
3. When the owner says an item is done or does not apply, run
   `paperwork mark <id> <item> done` (or `na`). `na` is refused for a step with no rule
   (everyone does it) and for an item whose rule applies to the owner now (it would come
   straight back): say so, and keep it on the list. The day count marks itself from the
   dates: never mark it. The police certificate is the exception: only the owner knows whether a long trip
   stayed in one country, so `na` works, and a new long trip brings it back.
4. `paperwork presence <id>` says only whether the owner has enough days, never how
   many: with today's date, a count gives away the PR date. The count and the earliest
   sign day show with `--reveal` in the owner's terminal.
5. Owner-only steps are marked `[only you]`. Give their instructions word for word and
   stop there.

## Keep the facts current

Rules and fees change. Before you advise on a fee, a day count or a document:

1. Run `bin/cashcast paperwork sources --live` outside the agent sandbox (canada.ca
   resets connections from the sandbox). Each page shows OK, CHANGED, UNREACHABLE or
   MANUAL. MANUAL is a page it cannot read as text (a PDF): open it and look for the
   listed quotes by hand.
2. On CHANGED, read the official page, update the quote, the rule and `CHECKED` in
   `src/core/paperwork-citizenship.ts`, and run its tests. Cite only `canada.ca`,
   `ircc.canada.ca` or `cic.gc.ca` pages.

## Add a process

Add a `src/core/paperwork-<name>.ts` with steps, items, rules and sources (official pages
only, each with a quote and the check date). Add it to `PROCESSES` in
`src/core/paperwork.ts`, then add tests with synthetic people only.
