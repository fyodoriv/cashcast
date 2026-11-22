# 3. One folder per user, and consent granted only at a terminal

- **Status**: accepted
- **Date**: 2026-10-18

## Context

The assistant must serve any number of people in Canada on one computer.
Their private data must never leave it without explicit approval, including
to the AI agent that runs the assistant.

## Decision

- Each person gets `private/users/<name>/` (mode 700) with a `profile.toml`.
  The profile must say `country = "CA"` and a valid province or territory.
- The original `private/` layout keeps working as the user `default`.
- Every report goes through `gate.emit`. People at a terminal see it. An AI
  agent sees it only when the owner has granted the matching scope.
- Grants are typed at `/dev/tty`, which an agent's shell cannot type into.
  They expire and can be revoked. Scopes are listed in `consent.SCOPES`.
- Money actions are never automated; settings changes need a yes in chat.

## Consequences

- An existing user runs `bin/cashcast consent grant agent:aggregates` once
  before their agent can read reports again.
- The gate guards against mistakes, not malice; `.claude/settings.json` deny
  rules and `AGENTS.md` rules add layers. `docs/privacy.md` states the limit.
