# 2. Rust engine for raw rows, Python for everything else

- **Status**: accepted
- **Date**: 2026-10-18

## Context

Cashcast started as a Rust engine (reads exports, prints aggregates) plus a
Python forecast (openpyxl, pycel). The assistant adds strategy, consent,
connectors, API clients and reminders. Raw transactions are the most
sensitive data in the system.

## Decision

- The Rust engine stays the only code that parses raw rows: `export.rs` and
  `ofx.rs` for cash, plus `ledger.rs` for investment and crypto ledgers.
- Everything else is the Python package `src/cashcast/`, managed with uv,
  tested with pytest, linted with ruff.
- API clients save downloads unparsed into the inbox; the engine reads them.
- `bin/cashcast` stays the one entry point: bash for `doctor` (bootstrap),
  Python for every other command.

## Consequences

- A new bank format is a Rust change with a canary fixture.
- A new strategy or connector is a Python change.
- The privacy boundary stays small enough to review in one sitting.
