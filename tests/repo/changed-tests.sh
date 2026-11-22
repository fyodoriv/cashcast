#!/usr/bin/env bash
# time-review-test-loop: while fixing, run only the tests that match the changed files.
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }
mapped() { scripts/changed-tests.sh "$@" | paste -sd' ' -; }
[ "$(mapped engine/src/transfers.rs)" = "engine:lib engine:transfers" ] || fail "source to its tests: $(mapped engine/src/transfers.rs)"
[ "$(mapped engine/src/web.rs)" = "engine:lib" ] || fail "source without its own test file"
[ "$(mapped engine/tests/cards.rs README.md tests/e2e/us-29-cards-now.sh tests/repo/evals.sh)" \
  = "e2e:tests/e2e/us-29-cards-now.sh engine:cards repo:tests/repo/evals.sh" ] || fail "changed tests run themselves"
[ -z "$(mapped engine/src/no_such_module.rs tests/e2e/us-99-gone.sh)" ] || fail "missing files add nothing"
echo "ok changed-tests maps changed files to their tests"
