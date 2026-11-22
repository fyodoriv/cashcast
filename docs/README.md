# Docs map

One row per doc: the question it answers and who reads it.

| Doc | Answers | Audience |
|---|---|---|
| [VISION.md](VISION.md) | Why does this exist, what is out of scope, when do we stop? | both |
| [user-stories.md](user-stories.md) | What can a user do, end to end? | both |
| [decisions/](decisions/) | Why was a hard-to-undo choice made? | both |
| [plans/](plans/) | How will a multi-step change land? | agent |
| [observability.md](observability.md) | What do the logs, errors, metrics and health check show? | both |
| [learnings.md](learnings.md) | What did past changes teach us? | both |
| [decisions/0005](decisions/0005-typescript-web-ui-no-notion.md) | Why is the UI a TypeScript web app, and why no Notion? | both |
| [decisions/0006](decisions/0006-moneywiz-url-scheme-not-sql.md) | Why does cashcast add MoneyWiz rows through links instead of writing its database? | both |
| [decisions/0007](decisions/0007-moneywiz-edits.md) | How does cashcast edit MoneyWiz rows and categories in place? | both |
| [decisions/0008](decisions/0008-notion-export-import.md) | Why do Notion pages come in from an export file, offline? | both |
| [decisions/0009](decisions/0009-notion-tasks-database.md) | Why may scrubbed task titles go to a Notion database, and nothing else? | both |
| [decisions/0012](decisions/0012-moneywiz-category-migrations.md) | Why do MoneyWiz categories change through numbered migrations with a computed rollback? | both |
| [decisions/0010](decisions/0010-rust-except-ui.md) | Why does the code move to Rust, and how is each step proven? | both |
| [decisions/0011](decisions/0011-moneywiz-sync-never-guesses.md) | How does the MoneyWiz sync avoid duplicates and deleted rows coming back? | both |
| [decisions/0012](decisions/0012-forecast-backtest.md) | When may the forecast model change, and what was tried? | both |
| [decisions/0013](decisions/0013-honest-investment-recommender.md) | What makes the investment recommender honest, and how can I override it? | both |
| [decisions/0014](decisions/0014-net-total-north-star.md) | Why is the net total the main number, and how do boundaries refuse options? | both |
| [decisions/0015](decisions/0015-reuse-expert-sources-live-facts.md) | Which expert sources does investment advice reuse, and how do live rates stay current? | both |
| [decisions/0016](decisions/0016-pdf-statements.md) | How are bank PDF statements read, and why can a misread never reach MoneyWiz? | both |
| [decisions/0017](decisions/0017-paperwork-local-encrypted.md) | Why does paperwork live in cashcast, sealed, with no web route? | both |
| [paperwork-threat-model.md](paperwork-threat-model.md) | What does paperwork protect, from whom, and what not? | both |
| [optimize.md](optimize.md) | How does the stock optimizer decide where new money goes? | both |
| [design.md](design.md) | Which apps does the web UI learn from, and what rules does it follow? | both |
| [privacy.md](privacy.md) | Where does personal data live, and what may leave the machine? | both |
| [../README.md](../README.md) | How do I install it and which commands exist? (tutorial + reference) | human |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | How do I add a strategy, connector or fact? (how-to) | both |
| [../connectors/README.md](../connectors/README.md) | Which services are supported, and how? (reference) | both |
| [../knowledge/README.md](../knowledge/README.md) | Which facts does the assistant use, and how fresh are they? (reference) | both |
