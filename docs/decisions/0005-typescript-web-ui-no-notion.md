# 5. The main interface is a TypeScript web app; Notion is dropped

- **Status**: accepted; amended by 0008 (a one-time, offline import of a Notion export into private Documents) and 0009 (scrubbed task titles to the owner's Notion tasks database)
- **Date**: 2026-10-18

## Context

A person needs to plan ahead and move between many views: the timeline,
the forecast, card routing, credit, tax, holdings, services and privacy.
A Markdown file or a Notion page is a single long view. Notion also sends
private numbers to a cloud service.

## Decision

- The main interface is a local web app in `web/` (TypeScript, React, Vite),
  served by `bin/cashcast web` on 127.0.0.1 with a per-run token.
  Plan ahead is its home view.
- **Rule for this repo and the owner's other projects: a UI with more than one
  view is built in TypeScript.** Python and Rust keep the logic; the UI only
  renders the JSON the local API returns.
- Notion support is removed completely: no connector, API client, sync
  command or consent scope. The web app holds the whole plan.

## Consequences

- The gate builds, lints, typechecks and tests the web app (`scripts/check.sh`).
- `web/src/test/fixture.json` is generated from a synthetic person
  (`scripts/web-fixture.py`); a Python test fails when the API shape drifts.
- No write endpoints: changes still go through `profile.toml` and the CLI.
