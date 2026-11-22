#!/usr/bin/env bash
# Repo checks: the scripts, docs and agent entry points around the app.
# Each tests/repo/*.sh prints "ok ..." lines and exits non-zero on a failure.
# Arguments pick files by a part of the name: tests/repo/run.sh evals
set -euo pipefail
cd "$(dirname "$0")/../.."
n=0
for t in tests/repo/*.sh; do
  [ "$(basename "$t")" = run.sh ] && continue
  if [ "$#" -gt 0 ]; then
    hit=""
    for want in "$@"; do case "$(basename "$t")" in *"$want"*) hit=1 ;; esac; done
    [ -n "$hit" ] || continue
  fi
  bash "$t"
  n=$((n + 1))
done
[ "$n" -gt 0 ] || { echo "repo tests: none match: $*" >&2; exit 1; }
echo "repo tests: $n file(s) passed"
