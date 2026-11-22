#!/bin/bash
# Converts OUT_DIR/Cashcast.xlsx to OUT_DIR/Cashcast.numbers and writes the
# Forecast table's values to OUT_DIR/numbers_readback.tsv for verification.
# Usage: to_numbers.sh OUT_DIR
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$(cd "${1:?usage: to_numbers.sh OUT_DIR}" && pwd)"
osascript "$here/to_numbers.applescript" \
  "$out/Cashcast.xlsx" "$out/Cashcast.numbers" > "$out/numbers_readback.tsv"
chmod 600 "$out/numbers_readback.tsv"
[ -e "$out/Cashcast.numbers" ] && chmod -R go-rwx "$out/Cashcast.numbers"
echo "Saved $out/Cashcast.numbers"
