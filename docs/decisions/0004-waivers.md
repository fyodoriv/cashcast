# 4. Requirements adapted for a private finance app

- **Status**: accepted
- **Date**: 2026-10-18

## Context

The `new-project` requirements fit most repos. A few need a stricter or
different form here because the data is a person's finances.

## Decision

- **No external error reporter.** Errors stay in each user's `logs/` as
  content-free records (story, command, error type).
- **Logs stricter than content-free.** No amounts, balances or dates of single
  transactions, even as numbers; only counts and durations.
- **The `.gitignore` stays an allowlist**, unlike the template's denylist.
- **Speed review monthly**, inside the checkup, not weekly: the gate changes rarely.
- **Evals for the agent skills run by hand** (`evals/run.sh` prints the cases
  and rubric for a judge): they need a model, and personal data must not
  reach a paid judge.

Update 2026-10-21: judge cases now run on the local Ollama model only
(`config/models.yaml` route `local`), on synthetic responses, and are skipped
when it is not running (`evals/README.md`). Still no paid judge.

## Consequences

The standards checker passes with these forms. The monthly audit re-reads this ADR.
