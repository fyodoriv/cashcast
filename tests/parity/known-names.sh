#!/usr/bin/env bash
# known-names.sh DIR: the positive leak check for MoneyWiz text.
# The parity run has only synthetic data, so every word in every cell of a
# MoneyWiz table must come from the fixture and command list (tests/parity)
# or from the engine's own text (engine/src). A MoneyWiz table is any table in
# a *moneywiz* output, in a snapshot section of a moneywiz file (for example
# users/alex/out/last-moneywiz.md copied into later outputs), or under a
# heading that names MoneyWiz. Prints the files with an unknown word (never
# the word) and exits 3.
set -euo pipefail
dir="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
known="$(mktemp "${TMPDIR:-/tmp}/known-names.XXXXXX")"
trap 'rm -f "$known"' EXIT
{ cat "$root"/tests/parity/fixture/*.* "$root"/tests/parity/commands.txt 2>/dev/null; find "$root/engine/src" -name '*.rs' -exec cat {} +; } |
  grep -aoE '[A-Za-z][A-Za-z0-9]+' | tr 'A-Z' 'a-z' | LC_ALL=C sort -u > "$known"
# The words in MoneyWiz table cells of one file, one per line.
cells() {
  awk -v whole="$([[ "${1##*/}" == *moneywiz* ]] && echo 1 || echo 0)" '
    NR == 1 { inside = whole }
    /^== / { inside = whole || tolower($0) ~ /moneywiz/; next }
    /^#/ { if (tolower($0) ~ /moneywiz/) inside = 1 }
    inside && /^[ \t]*\|/ { n = split($0, c, "|"); for (i = 2; i < n; i++) print c[i] }
  ' "$1" | grep -aoE '[A-Za-z][A-Za-z0-9]+' | tr 'A-Z' 'a-z' | LC_ALL=C sort -u || true
}
bad=()
for f in "$dir"/*.out; do
  [ -e "$f" ] || continue
  words="$(cells "$f")"
  [ -z "$words" ] && continue
  if [ -n "$(LC_ALL=C comm -23 <(printf '%s\n' "$words") "$known")" ]; then bad+=("${f##*/}"); fi
done
[ "${#bad[@]}" -eq 0 ] && exit 0
echo "rust-parity: MoneyWiz table words that are not fixture or engine words in (not shown):" >&2
printf '  %s\n' "${bad[@]}" >&2
exit 3
