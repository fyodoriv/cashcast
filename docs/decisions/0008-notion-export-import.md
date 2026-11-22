# 8. Notion pages move in from an export file, offline

- **Status**: accepted
- **Date**: 2026-10-19

## Context

Decision 0005 dropped Notion support: no connector, no API client, no sync. The
owner still has years of finance notes in Notion: the cards and banking plan,
tax notes and past years, crypto records, budgets, an archive. They want all of
it in cashcast, as privately as possible.

There are two ways to get the pages:

- **The Notion connector.** An agent fetches the pages, so all page text passes
  through the agent's model provider.
- **Notion's export.** "Markdown & CSV" makes a .zip that Notion emails to the
  owner. Only the owner and Notion see it.

## Decision

- `bin/cashcast notion import EXPORT` reads the export on this computer. It makes no
  network call and needs no token or consent scope.
- Pages become Markdown, databases become Markdown tables (from the `_all`
  file, which has every row), and images and PDFs are copied. Notion's ids are
  stripped from names, and links between pages are rewritten to `doc:` links.
  Paths that would escape the folder are skipped.
- Everything lands in the user's `docs/notion/`, with folders 700 and files 600.
  A new import replaces the old copy and keeps the owner's own notes in `docs/`.
- The web app's Documents view reads them through token-protected, read-only
  endpoints. Its Content-Security-Policy allows nothing from the internet. Web
  links open only when clicked, with `noreferrer`.
- The CLI prints counts only. Agents never open `docs/`, which
  `.claude/settings.json` enforces.
- The Notion connector is not used. The `notion-import` skill tells agents
  not to use it.

## Consequences

- Decision 0005 still holds: there is no live sync and no Notion API client. To
  refresh the copy, the owner exports again.
- The features of those pages were rebuilt earlier and are checked by
  `tests/unit/test_notion_parity.py`. The documents are the record; the plan
  lives in the app.
- The web app stays read-only. The owner edits notes as files in `docs/`.
