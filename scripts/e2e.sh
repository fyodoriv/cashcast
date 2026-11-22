#!/usr/bin/env bash
# End-to-end tests: each story drives bin/cashcast the way a person (or their
# agent) does, with synthetic data, and checks the output, the files and the
# log events. Stories are tests/e2e/us-*.sh; shared helpers are tests/e2e/lib.sh.
# Arguments pick stories by a part of the file name: scripts/e2e.sh us29 us14
# E2E_COLD=1 also re-installs dependencies in US-01, like a stranger on a new machine.
set -euo pipefail
cd "$(dirname "$0")/.."

shopt -s nullglob
tests=()
for t in tests/e2e/us-*.sh; do
  if [ "$#" -eq 0 ]; then
    tests+=("$t")
  else
    for want in "$@"; do
      case "$(basename "$t")" in *"$want"*) tests+=("$t"); break ;; esac
    done
  fi
done
[ "${#tests[@]}" -gt 0 ] || { echo "e2e: no story matches: $*" >&2; exit 1; }
for t in "${tests[@]}"; do
  echo "e2e: $t"
  bash "$t"
done
echo "e2e: all stories passed"
