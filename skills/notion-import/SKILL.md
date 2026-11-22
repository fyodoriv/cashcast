---
name: notion-import
description: Moves the person's Notion finance pages (Finances: cards and banking plan, banks, taxes, crypto, investments, budgets, archive) into cashcast's private Documents, offline, from a Notion "Markdown & CSV" export. Use when the user says "move my Notion", "import my Notion finance pages", "migrate from Notion", or wants to refresh the copy after editing in Notion. Don't use to read or quote page content, to fetch pages through a Notion connector, or to write anything to Notion.
---

# Notion import

The most private route: the agent never reads the pages. Notion's own export
goes from the person's Downloads folder straight into their private `docs/`,
and every report lists counts only.

1. Ask the person to export from Notion themselves:
   - open the **Finances** page, then **••• → Export**;
   - format **Markdown & CSV**, **Include subpages**, **Create folders for subpages**;
   - save the .zip, usually to `~/Downloads`.
2. Run `bin/cashcast notion import ~/Downloads/<file>.zip --delete-source`. It
   works offline and needs no Notion token. It replaces the previous Notion copy
   and keeps the person's own notes in `docs/`.
3. Report the printed counts: pages, tables, files and skipped. Never open
   `private/users/*/docs/`, and never quote titles or text.
4. Tell the person to open `bin/cashcast web` → **Documents**, check a few pages,
   then empty the Trash (the deleted export goes there).
5. The features of those pages already live in cashcast: card routing, credit,
   reserve, tax, investing, budgets and the monthly review
   (`engine/tests/app_strategy_sections.rs`). Offer `bin/cashcast strategy` for the
   live numbers. The documents are the record; the app holds the plan.
6. The person may delete or archive the Notion pages once the copy is checked.
   That is their call: never suggest doing it for them, and never touch Notion.

Never use the Notion connector to fetch the pages for this. Page text would
pass through the agent's model provider, which "super private" rules out.
