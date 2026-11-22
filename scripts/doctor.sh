#!/usr/bin/env bash
# Health check. One line per check: "ok", or "FAIL" plus the exact fix.
# Exit 0 only when every check passes. Content-free: never prints user data.
# The app's own doctor (bin/cashcast doctor) also builds the engine.
set -uo pipefail
cd "$(dirname "$0")/.."

fails=0
ok() { echo "ok    $1"; }
fail() { echo "FAIL  $1 -> fix: $2"; fails=$((fails + 1)); }
need() { if command -v "$1" >/dev/null 2>&1; then ok "$1 installed"; else fail "$1 missing" "$2"; fi; }

need git "install git (xcode-select --install)"
need cargo "curl https://sh.rustup.rs -sSf | sh"
need node "brew install node (or: mise use node@lts)"
need pnpm "corepack enable"
need jq "brew install jq"
need yq "brew install yq"
need sqlite3 "brew install sqlite"
# Optional: without it PDF bank statements are skipped, so it is a note, not a failure.
command -v pdftotext >/dev/null 2>&1 && ok "pdftotext installed (PDF bank statements)" \
  || echo "note  pdftotext missing: PDF bank statements are not read -> fix: brew install poppler"
[ -d .metrics ] && ok ".metrics/ exists" || fail ".metrics/ missing" "scripts/setup.sh"
[ -d private ] && ok "private/ exists (ignored by git)" || fail "private/ missing" "scripts/setup.sh"
[ "$(ls -ld private 2>/dev/null | cut -c1-10)" = drwx------ ] && ok "private/ is owner-only" || fail "private/ is not mode 700" "chmod 700 private"
[ -f config/models.yaml ] && ok "config/models.yaml present" || fail "config/models.yaml missing" "restore it from git"
if git rev-parse --git-dir >/dev/null 2>&1; then
  git check-ignore -q private/x 2>/dev/null && ok "git ignores private/" || fail "git would track private/" "restore .gitignore from git"
else
  grep -qx '/private/' .gitignore && ok ".gitignore lists /private/ (no git repo here)" || fail ".gitignore misses /private/" "restore .gitignore"
fi
# App checks: users and profiles, recent errors by story, knowledge, connectors.
engine=engine/target/release/cashcast-engine
if [ ! -x "$engine" ]; then
  fail "engine not built" "cargo build --release --manifest-path engine/Cargo.toml"
elif out="$(CASHCAST_ROOT="$PWD" "$engine" health 2>&1)"; then
  echo "$out" | sed 's/^/      /'; ok "app health (users, errors, knowledge, connectors)"
else
  echo "$out" | sed 's/^/      /'; fail "app health" "follow the lines above (each says its fix)"
fi

[ "$fails" -eq 0 ] && echo "doctor: all checks passed" || echo "doctor: $fails check(s) failed"
[ "$fails" -eq 0 ]
