# Recurring tasks

Work that fires on a cadence instead of waiting in `TASKS.md`. When a task
fires, do it, then bump its `**Last-fired**:` date in the same commit.

## P1

- [ ] Monthly checkup, requirements audit and speed review
  - **ID**: monthly-checkup
  - **Cadence**: monthly
  - **Last-fired**: 2026-10-18
  - **Details**: Run `scripts/checkup.sh`. It runs the doctor, the cached gate, broken stories, the
    speed review (`scripts/metrics.sh`: file a task when a step got slower or the gate is over its
    120-second budget), open P0s, and the new-project requirements audit when it is due. Follow
    section 4 of `~/.claude/skills/new-project/references/requirements-map.md` for the audit. Read
    only code and docs; never open `private/`.

- [ ] Knowledge freshness
  - **ID**: knowledge-freshness
  - **Cadence**: monthly
  - **Last-fired**: 2026-10-18
  - **Details**: Run `bin/cashcast knowledge check`. For each stale file or estimate that a strategy
    section uses, follow `skills/knowledge-update/SKILL.md`. Each January, add the new tax year
    (`tax_ca_<year>.toml`) from the CRA indexation release.
