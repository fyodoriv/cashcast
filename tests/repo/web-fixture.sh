#!/usr/bin/env bash
# The web app's test fixture (web/src/test/fixture.json) is what the engine
# serves today: regenerate it and compare.
set -euo pipefail
cd "$(dirname "$0")/../.."
T="$(mktemp -d "${TMPDIR:-/tmp}/web-fixture.XXXXXX")"
trap 'rm -rf "$T"' EXIT
scripts/web-fixture.sh "$T/fixture.json" > /dev/null
if ! diff <(jq -S . web/src/test/fixture.json) <(jq -S . "$T/fixture.json") > "$T/diff"; then
  head -40 "$T/diff" >&2
  echo "FAIL: web/src/test/fixture.json is stale; run scripts/web-fixture.sh" >&2
  exit 1
fi
echo "ok the web fixture matches what the engine serves"
