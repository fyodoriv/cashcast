#!/usr/bin/env bash
# Someone else's Mac: install cashcast the way the README says, in a fresh
# HOME, then use it. CI runs this on a clean macOS runner (job `fresh-mac`);
# you can run it on your own Mac too: it never touches your HOME or data.
#
# Steps: a fresh HOME -> the README one-line command, word for word (only the
# clone URL points at this checkout) -> add a person -> a synthetic import ->
# start the app -> the doctor. Prints one "step ok" or "step FAILED" line each.
# Synthetic data only (CANARY strings); nothing leaves the machine.
#
# FRESH_MAC_REMOVE="yq" first removes those Homebrew tools, so setup has to
# install them (CI sets it; leave it empty on your own Mac).
set -uo pipefail
src="$(cd "$(dirname "$0")/.." && pwd)"

fails=0
step() { # step NAME COMMAND...: runs it, prints the result line, keeps going
  local name="$1"; shift
  if "$@"; then echo "step ok      $name"; else echo "step FAILED  $name"; fails=$((fails + 1)); fi
}

# Keep the machine's Rust toolchain (it lives under the real HOME); every
# other path starts empty, as on a new Mac.
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
HOME="$(mktemp -d "${TMPDIR:-/tmp}/fresh-mac.XXXXXX")"
export HOME
unset CASHCAST_HOME CASHCAST_USER CASHCAST_PRIVATE CASHCAST_ROOT
export CASHCAST_POSTHOG_KEY="" CASHCAST_LIVE=off
echo "fresh HOME: $HOME"

for tool in ${FRESH_MAC_REMOVE:-}; do
  brew uninstall --ignore-dependencies --force "$tool" >/dev/null 2>&1 || true
  command -v "$tool" >/dev/null && echo "note: $tool is still on PATH ($(command -v "$tool"))"
done

# The README's one-line command, read from the README itself.
line="$(awk '/^Or set it up in one line/{f=1; next} f && /^\[ -d ~\/apps\/cashcast \]/{print; exit}' "$src/README.md")"
[ -n "$line" ] || { echo "step FAILED  find the one-line command in README.md"; exit 1; }
# A clone needs a branch to pull; CI checks out a detached commit.
git -C "$src" symbolic-ref -q HEAD >/dev/null || git -C "$src" switch -q -c fresh-mac-head
line="${line//https:\/\/github.com\/fyodoriv\/cashcast.git/file://$src}"
echo "one-liner: $line"
step "README one-line install (setup + doctor)" bash -c "$line"

app="$HOME/apps/cashcast"
cli="$app/bin/cashcast"
step "add a person" "$cli" users add alex --province ON
inbox="$app/private/users/alex/inbox"
cat > "$inbox/chequing.csv" <<'CSV'
Date,Description,Amount
2026-10-01,PAYROLL CANARY01,2400.00
2026-10-03,GROCER CANARY02,-80.00
2026-10-15,PAYROLL CANARY01,2400.00
2026-10-20,RENT CANARY03,-1600.00
CSV
import_runs() { "$cli" import >"$HOME/import.log" 2>&1 && ! grep -q CANARY "$HOME/import.log"; }
step "first import (no data printed)" import_runs
step "start the app" env CASHCAST_AGENT=1 "$cli"
"$cli" stop >/dev/null 2>&1 || true
step "the doctor passes" "$app/scripts/doctor.sh"
step "cashcast is on the PATH" test -x "$HOME/.local/bin/cashcast"

[ "$fails" -eq 0 ] && echo "fresh-mac: all steps passed" || echo "fresh-mac: $fails step(s) failed"
[ "$fails" -eq 0 ]
