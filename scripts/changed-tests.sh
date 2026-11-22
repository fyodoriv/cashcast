#!/usr/bin/env bash
# Prints the tests that match the given changed files, one per line, sorted.
#   engine/src/**/<name>.rs  → engine:lib (the crate's own tests) and engine:<name> if engine/tests/<name>.rs exists
#   engine/tests/<name>.rs   → engine:<name>
#   tests/e2e/us-*.sh        → e2e:<file>
#   tests/repo/*.sh          → repo:<file>
# Missing files and other paths add nothing. Used by `scripts/check.sh changed`.
set -euo pipefail
cd "$(dirname "$0")/.."
out=()
for f in "$@"; do
  [ -f "$f" ] || continue
  case "$f" in
    engine/src/*.rs)
      out+=("engine:lib")
      name="$(basename "$f" .rs)"
      if [ -f "engine/tests/$name.rs" ]; then out+=("engine:$name"); fi
      ;;
    engine/tests/*/*) ;;
    engine/tests/*.rs) out+=("engine:$(basename "$f" .rs)") ;;
    tests/e2e/us-*.sh) out+=("e2e:$f") ;;
    tests/repo/*.sh) out+=("repo:$f") ;;
  esac
done
[ ${#out[@]} -eq 0 ] || printf '%s\n' "${out[@]}" | sort -u
