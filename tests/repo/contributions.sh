#!/usr/bin/env bash
# Contribution rules, enforced: every strategy section and connector is named in
# a test, and the contributor guide covers each kind of contribution.
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }
tests="$(cat engine/tests/*.rs engine/tests/app/*.rs tests/e2e/*.sh tests/repo/*.sh)"

ids="$(sed -n '/^pub const PORTED/,/^];/p' engine/src/strategy/mod.rs | grep -oE '^\s+\("[a-z_]+"' | grep -oE '[a-z_]+')"
[ "$(wc -l <<< "$ids")" -ge 15 ] || fail "read too few strategy sections: $ids"
[ "$(sort <<< "$ids" | uniq -d)" = "" ] || fail "duplicate section ids"
for id in $ids; do
  grep -qF "\"$id\"" <<< "$tests" || fail "no test names strategy section $id"
done
echo "ok every strategy section is named in a test"

for f in connectors/*.toml; do
  sid="$(basename "$f" .toml)"
  grep -qF "\"$sid\"" <<< "$tests" || fail "connector $sid has no test"
done
echo "ok every connector is named in a test"

guide="$(tr '[:upper:]' '[:lower:]' < CONTRIBUTING.md)"
for topic in "strategy section" connector knowledge privacy; do
  grep -qF "$topic" <<< "$guide" || fail "CONTRIBUTING.md does not cover: $topic"
done
echo "ok the contributor guide covers sections, connectors, knowledge and privacy"
