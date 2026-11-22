#!/usr/bin/env bash
# Count the Python lines left in the repo: the Rust port's progress number
# (decision 0010). Counts tracked .py files only, so private/ never counts.
# Usage: scripts/python-lines.sh [--by-dir]
#   --by-dir  also print the lines per top-level folder, largest first
# Prints "python-lines: N" last. The port is done when N is 0.
set -euo pipefail
cd "$(dirname "$0")/.."
files="$(git ls-files '*.py')"
if [ -z "$files" ]; then echo "python-lines: 0"; exit 0; fi
if [ "${1:-}" = --by-dir ]; then
  # shellcheck disable=SC2086  # one word per file
  wc -l $files | awk '$2 != "total" { split($2, p, "/"); d = (p[2] == "" ? "." : p[1]); n[d] += $1 }
    END { for (d in n) printf "%-12s %6d\n", d, n[d] }' | sort -k2 -nr
fi
# shellcheck disable=SC2086
total="$(cat $files | wc -l | tr -d ' ')"
mkdir -p .metrics
printf '{"ts":"%s","event":"python_lines","lines":%s}\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$total" >> .metrics/parity.jsonl
echo "python-lines: $total"
