#!/usr/bin/env bash
# Traces each requirement of the owner's three founding goals to the code,
# tests and docs that meet it. If a file below moves, update this map in the
# same PR; a missing path fails the test.
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }
n=0
need() {  # need "requirement" path...
  local req="$1"; shift
  for p in "$@"; do [ -e "$p" ] || fail "$req: missing $p"; done
  n=$((n + 1))
}

# First goal: evolve cashcast into a personal financial assistant (eleven requirements).
need "do everything cashcast does" engine/src/forecast/model.rs tests/e2e/us-02-forecast.sh engine/src/export.rs bin/cashcast
need "any number of Canadian-only users, private unless approved" engine/src/core/users.rs engine/src/core/consent.rs \
  engine/src/core/gate.rs tests/e2e/us-05-users.sh tests/e2e/us-06-consent.sh docs/privacy.md
need "changes in browser or API of services" engine/src/tasks/connectors.rs engine/src/api.rs connectors/playbooks/chexy.md \
  engine/tests/app_api.rs skills/service-action/SKILL.md
need "detailed strategy: cashback, stocks, forecast, payments, alternatives" engine/src/strategy/cashback.rs \
  engine/src/strategy/invest.rs engine/src/strategy/forecast.rs engine/src/strategy/payments.rs \
  engine/src/strategy/alternatives.rs engine/tests/app_strategy_sections.rs
need "separate Apple Reminders system" engine/src/tasks/apple.rs engine/tests/app_apple.rs
services=(moneywiz scotiabank td wise wealthsimple casa kraken chexy paypal paysend google-sheets etrade)
need "13 services supported (the owner dropped Notion)" engine/tests/formats.rs engine/src/ledger.rs
for s in "${services[@]}"; do [ -f "connectors/$s.toml" ] || fail "missing connectors/$s.toml"; done
need "knowledge: credit cards, insurance, Canada taxes, corporate stocks" knowledge/data/credit_cards.toml \
  knowledge/data/insurance.toml knowledge/data/tax_ca_2026.toml knowledge/data/tax_deductions.toml \
  knowledge/data/corporate_stock.toml knowledge/credit-cards.md knowledge/canada-tax.md knowledge/insurance.md \
  knowledge/corporate-stock.md
need "pivot and iterate on knowledge" engine/src/knowledge.rs skills/knowledge-update/SKILL.md docs/learnings.md \
  tests/e2e/us-09-reminders-connectors-knowledge.sh
need "various people contributing" CONTRIBUTING.md tests/repo/contributions.sh connectors/README.md
need "follow the owner's app standards" AGENTS.md docs/VISION.md docs/user-stories.md docs/observability.md \
  scripts/check.sh scripts/setup.sh scripts/checkup.sh evals/run.sh Agentfile.yaml
need "everything the Notion finance pages do" engine/src/strategy/review.rs engine/src/docs.rs \
  skills/notion-import/SKILL.md tests/e2e/us-15-notion.sh web/src/views/Documents.tsx
[ "$n" -eq 11 ] || fail "the first goal has eleven requirements, not $n"

# MoneyWiz goal: record only cash in MoneyWiz; one command updates the rest.
need "MoneyWiz connection, as automated as possible" engine/src/moneywiz.rs connectors/playbooks/moneywiz.md \
  docs/decisions/0006-moneywiz-url-scheme-not-sql.md
need "one command fixes MoneyWiz rows from the bank's values" engine/tests/moneywiz.rs tests/e2e/us-14-moneywiz.sh

# Third goal: every unblocked task, Apple Calendar, bank changes in the browser, a stock optimizer, Sun Life.
need "judge-graded evals on the local model" evals/run.sh evals/labels.yaml tests/repo/evals.sh
need "recurring payments by hashed payee" engine/src/detect.rs engine/tests/recurring.rs engine/src/strategy/payments.rs
need "Sun Life group RRSP support" connectors/sunlife.toml connectors/playbooks/sunlife.md engine/src/ledger.rs \
  engine/tests/fixtures/ledger-sunlife-transactions.csv
need "every action through the CLI, agents and the UI" engine/src/core/commands.rs web/src/views/Actions.tsx \
  engine/tests/app_actions.rs commands/cashcast.md
need "one command starts cashcast, with one settings table" engine/src/tasks/start.rs engine/src/core/settings.rs \
  tests/e2e/us-16-19-calendar-changes-optimize-start.sh
need "set up and update Apple Reminders and Calendar" engine/src/tasks/apple.rs engine/tests/app_apple.rs skills/apple-sync/SKILL.md
need "needed bank changes in the browser" engine/src/tasks/changes.rs engine/tests/app_tasks.rs skills/bank-change/SKILL.md
need "refund a missing item without paying twice" skills/refund/SKILL.md evals/cases/refund.yaml
need "groceries and purchases: best store basket, never paying" engine/src/groceries.rs skills/grocery-cart/SKILL.md \
  skills/purchase/SKILL.md tests/e2e/us-38-groceries.sh evals/cases/grocery-cart.yaml
need "optimize stocks from the owner's inputs" engine/src/strategy/optimize.rs skills/optimize-stocks/SKILL.md docs/optimize.md
echo "ok each of $n goal requirements is backed by files"
