#!/usr/bin/env bash
# scripts/e2e.sh picks stories by a part of the file name and runs only those.
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }
if out="$(scripts/e2e.sh no-such-story 2>&1)"; then fail "an unknown story passed"; fi
[[ "$out" == *"no story matches: no-such-story"* ]] || fail "$out"
grep -qF 'case "$(basename "$t")" in *"$want"*)' scripts/e2e.sh || fail "arguments must filter stories by file name"
echo "ok e2e.sh runs only the stories its arguments name"
