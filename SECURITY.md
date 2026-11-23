# Security and privacy rules

cashcast handles bank exports, balances and tax facts. The code is public. Your
money data never is. This file lists the rules that keep it that way and the
check that enforces each one.

## Report a vulnerability

Open a private security advisory:
https://github.com/fyodoriv/cashcast/security/advisories/new

Do not open a public issue for a vulnerability. Do not attach real exports.

## What may be public

Only code, docs, shared knowledge and synthetic test data. `.gitignore` is an
allowlist, and `private/` is never tracked.

| Rule | Check | Where it runs |
|---|---|---|
| No private paths (`private/`, `.env`, databases, keys) are tracked | `scripts/public-readiness.sh` `private-paths` | gate step `readiness`, CI |
| No secrets in the tree or the last 20 commits | gitleaks (`readiness` `secrets-tree`, gate step `secrets`, pre-commit hook) | gate, CI, pre-commit |
| No personal emails, Luhn-valid card numbers, SIN-shaped numbers or phone numbers | `readiness` `emails`, `card-numbers`, `sin-numbers`, `phones` | gate, CI |
| Test data is synthetic and carries `CANARY` strings; no canary reaches any output | the leak tests in `engine/tests/` | gate, CI |
| No employer or internal names | `readiness` `owner-pattern` (the pattern file stays on the owner's machine) | owner's gate and pre-push hook only |
| CI has a read-only token, no secrets, no `pull_request_target`, actions pinned to a full SHA | `readiness` `workflows` | gate, CI |
| The backend is Rust; Python never comes back | `readiness` `rust-only` (`scripts/python-lines.sh` = 0) | gate, CI |
| Data leaves the computer only with the owner's consent | `engine/src/core/consent.rs`, `engine/src/core/gate.rs`, [docs/privacy.md](docs/privacy.md) exits table | engine tests |
| Agents never move money, type passwords or 2FA codes | [AGENTS.md](AGENTS.md) rule 6 | agent rules |

## Repository settings

These GitHub settings protect the public repo. They live on GitHub, not in
the code; read them with `gh api repos/fyodoriv/cashcast` and
`gh api repos/fyodoriv/cashcast/rulesets`.

| Setting | Value | Why |
|---|---|---|
| Ruleset `main` | changes only through a pull request with green `gate` and `fresh-mac` checks; no force-push, no deletion; nobody bypasses it | a broken or rewritten `main` would reach every install through auto-update |
| Secret scanning and push protection | on | a token pushed by mistake is blocked or reported |
| Dependabot security updates | on, plus `.github/dependabot.yml` (cargo, npm, GitHub Actions, weekly) | known-vulnerable packages get a fix PR |
| Private vulnerability reporting | on | the advisory link above works |
| Actions must be pinned to a full SHA | on | a moved tag cannot change what CI runs |
| Fork pull requests need approval to run CI | all outside contributors | a stranger's PR cannot use the runners unseen |

## The web app stays on this computer

`bin/cashcast web` listens on `127.0.0.1` only, never on the network. Every
API call needs the token printed in the link, and the server answers only to
the `127.0.0.1` or `localhost` Host header, which blocks DNS-rebinding pages.
Tests: `engine/src/web.rs` and `engine/tests/app_web.rs`. There is no hosted
version: a server would hold people's money data.

`scripts/public-readiness.sh --history` also checks every commit: gitleaks on
the whole history, the private pattern in every diff and commit message, and
noreply author emails. The public history starts from one fresh root commit
(2026-11-22). The old history stays in a private archive.

## Allowed exceptions

A synthetic value that looks real (a test card number, for example) goes in
`config/public-readiness-allow.txt`, one line per value:
`card <file> <digits>`, `sin <file> <digits>`, `email <file> <address>` or
`phone <file>`. Never allow a real value.

## Your own copy

- Your data lives in `private/users/<name>/`, mode 700, never tracked.
- Tokens live in `private/.env` or the macOS Keychain, never in the repo.
- Telemetry is off until you say yes. It sends counts only: see
  [docs/privacy.md](docs/privacy.md).
