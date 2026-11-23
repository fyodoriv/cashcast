#!/usr/bin/env bash
# scripts/install-tools.sh on a new Mac: it installs every missing tool
# (Homebrew formulas, Rust with rustup, pnpm), prints the one Homebrew line
# when Homebrew itself is missing, and installs nothing when all are there.
# Fake brew, curl, npm and tools on a bare PATH; nothing is installed for real.
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
BASH_BIN="$(command -v bash)"

# root DIR: a copy of the script with a web/package.json and a bare PATH.
root() {
  local r="$1"
  mkdir -p "$r/scripts" "$r/web" "$r/bin" "$r/home"
  cp scripts/install-tools.sh "$r/scripts/"
  echo '{ "packageManager": "pnpm@9.12.0" }' > "$r/web/package.json"
  for t in sed dirname sh mkdir chmod cat; do ln -s "$(command -v "$t")" "$r/bin/$t"; done
}
fake() { # fake DIR NAME BODY: a tool that logs its arguments, then runs BODY
  printf '#!/bin/sh\necho "%s $*" >> "%s/log"\n%s\n' "$2" "$1" "${3:-}" > "$1/bin/$2"
  chmod +x "$1/bin/$2"
}
# env -i: no BASH_ENV or profile can put real tools back on the PATH.
# CASHCAST_BREW_DIRS points at an empty folder, so the real Homebrew in
# /opt/homebrew is never found or run (it once reinstalled a real formula).
run() { (cd "$1" && /usr/bin/env -i HOME="$1/home" PATH="$1/bin" CASHCAST_BREW_DIRS="$1/no-homebrew" "$BASH_BIN" scripts/install-tools.sh) > "$1/out" 2>&1; }

# 1. A new Mac with Homebrew: everything else is missing.
a="$tmp/a"; root "$a"
fake "$a" brew
fake "$a" npm "printf '#!/bin/sh\n' > '$a/bin/pnpm'; chmod +x '$a/bin/pnpm'"
# rustup's installer, as curl would fetch it: makes ~/.cargo/bin/cargo.
fake "$a" curl "printf 'mkdir -p \"\$HOME/.cargo/bin\"; printf \"#!/bin/sh\\\\necho cargo 1.0\\\\n\" > \"\$HOME/.cargo/bin/cargo\"; chmod +x \"\$HOME/.cargo/bin/cargo\"\\n'"
run "$a" || { echo "FAIL install-tools: failed on a new Mac"; cat "$a/out"; exit 1; }
grep -qx "brew install --quiet jq yq sqlite node" "$a/log" || { echo "FAIL install-tools: brew did not install the missing formulas"; cat "$a/log"; exit 1; }
grep -q "brew install --quiet poppler" "$a/log" || { echo "FAIL install-tools: poppler not installed"; exit 1; }
grep -q "^curl .*sh.rustup.rs" "$a/log" || { echo "FAIL install-tools: Rust not installed"; exit 1; }
[ -x "$a/home/.cargo/bin/cargo" ] || { echo "FAIL install-tools: no cargo after the Rust install"; exit 1; }
grep -qx "npm install --global --silent pnpm@9.12.0" "$a/log" || { echo "FAIL install-tools: pnpm not installed at the pinned version"; cat "$a/log"; exit 1; }
echo "ok install-tools installs every missing tool on a new Mac"

# 2. No Homebrew: one line to install it, and a non-zero exit.
b="$tmp/b"; root "$b"
if run "$b"; then echo "FAIL install-tools: passed without Homebrew"; exit 1; fi
grep -q "Homebrew/install/HEAD/install.sh" "$b/out" || { echo "FAIL install-tools: no Homebrew install line"; cat "$b/out"; exit 1; }
grep -q "Missing: jq yq sqlite node" "$b/out" || { echo "FAIL install-tools: did not name the missing tools"; exit 1; }
echo "ok install-tools prints the Homebrew line when Homebrew is missing"

# 3. Everything present: nothing is installed.
c="$tmp/c"; root "$c"
for t in jq yq sqlite3 node pdftotext cargo pnpm; do fake "$c" "$t"; done
fake "$c" brew
run "$c" || { echo "FAIL install-tools: failed with every tool present"; cat "$c/out"; exit 1; }
if grep -q "^brew install" "$c/log" 2>/dev/null; then echo "FAIL install-tools: installed a tool that was present"; exit 1; fi
echo "ok install-tools installs nothing when every tool is there"

# 4. Homebrew just installed but not on PATH yet (its installer only prints
# the `brew shellenv` step): found in its standard folder and used.
d="$tmp/d"; root "$d"
mkdir -p "$d/homebrew/bin"
printf '#!/bin/sh\necho "brew $*" >> "%s/log"\n[ "$1" = shellenv ] && echo "export PATH=\\"%s/homebrew/bin:\\$PATH\\""\nexit 0\n' "$d" "$d" > "$d/homebrew/bin/brew"
chmod +x "$d/homebrew/bin/brew"
for t in cargo pnpm pdftotext; do fake "$d" "$t"; done
(cd "$d" && /usr/bin/env -i HOME="$d/home" PATH="$d/bin" CASHCAST_BREW_DIRS="$d/homebrew/bin" "$BASH_BIN" scripts/install-tools.sh) > "$d/out" 2>&1 \
  || { echo "FAIL install-tools: did not find Homebrew off the PATH"; cat "$d/out"; exit 1; }
grep -qx "brew install --quiet jq yq sqlite node" "$d/log" || { echo "FAIL install-tools: did not use Homebrew from its folder"; cat "$d/log"; exit 1; }
echo "ok install-tools finds Homebrew that is not on the PATH yet"
