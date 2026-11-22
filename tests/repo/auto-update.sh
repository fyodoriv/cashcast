#!/usr/bin/env bash
# bin/cashcast (no arguments: the start screen) fast-forwards a clean checkout
# to origin/main, and leaves alone a checkout with local changes or commits,
# CI, and CASHCAST_AUTO_UPDATE=0. `bin/cashcast update` always checks.
set -euo pipefail
cd "$(dirname "$0")/../.."
src="$PWD"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/auto-update.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
g() { git -c core.hooksPath=/dev/null -c user.name=t -c user.email=t@users.noreply.github.com "$@"; }

# An "origin" with the launcher and a fake engine that prints its commit.
mkdir -p "$tmp/origin/bin" "$tmp/origin/engine/target/release"
cp bin/cashcast "$tmp/origin/bin/cashcast"
printf '/engine/target/\n/private/\n' > "$tmp/origin/.gitignore"
echo one > "$tmp/origin/VERSION"
g -C "$tmp/origin" init -q -b main
g -C "$tmp/origin" add -A && g -C "$tmp/origin" commit -qm "feat: one"

clone() { # clone <dir>: a user's checkout with a built fake engine
  g clone -q "$tmp/origin" "$1"
  mkdir -p "$1/engine/target/release"
  printf '#!/bin/sh\ncat "%s/VERSION"\n' "$1" > "$1/engine/target/release/cashcast-engine"
  chmod +x "$1/engine/target/release/cashcast-engine"
}
run() { (cd "$1" && env -u CI CASHCAST_HOME="$1/private" PATH="/usr/bin:/bin" "${@:2}" bin/cashcast 2>"$tmp/err") || true; }
bump() { echo "$1" > "$tmp/origin/VERSION"; g -C "$tmp/origin" commit -qam "feat: $1"; }

clone "$tmp/a"
bump two
out="$(run "$tmp/a")"
[ "$out" = two ] || { echo "FAIL auto-update: clean checkout not updated (got '$out')"; cat "$tmp/err"; exit 1; }
grep -q "Updated cashcast" "$tmp/err" || { echo "FAIL auto-update: no update line"; exit 1; }
echo "ok auto-update fast-forwards a clean checkout"

bump three
out="$(run "$tmp/a")"
[ "$out" = two ] || { echo "FAIL auto-update: checked again within the hour"; exit 1; }
echo "ok auto-update checks at most once an hour"
(cd "$tmp/a" && env -u CI CASHCAST_HOME="$tmp/a/private" bin/cashcast update 2>"$tmp/err")
[ "$(cat "$tmp/a/VERSION")" = three ] || { echo "FAIL auto-update: 'update' did not update"; exit 1; }
echo "ok auto-update 'bin/cashcast update' always checks"

clone "$tmp/b"
echo local > "$tmp/b/VERSION"
bump four
out="$(run "$tmp/b")"
[ "$out" = local ] && grep -q "local changes" "$tmp/err" || { echo "FAIL auto-update: touched local changes"; exit 1; }
echo "ok auto-update leaves local changes alone"

clone "$tmp/c"
echo mine > "$tmp/c/VERSION"; g -C "$tmp/c" commit -qam "feat: mine"
bump five
out="$(run "$tmp/c")"
[ "$out" = mine ] && grep -q "commits that" "$tmp/err" || { echo "FAIL auto-update: moved a checkout with local commits"; exit 1; }
echo "ok auto-update leaves local commits alone"

clone "$tmp/d"
bump six
out="$(run "$tmp/d" CASHCAST_AUTO_UPDATE=0)"
[ "$out" = five ] || { echo "FAIL auto-update: CASHCAST_AUTO_UPDATE=0 still updated (got '$out')"; exit 1; }
out="$(cd "$tmp/d" && CI=true CASHCAST_HOME="$tmp/d/private" bin/cashcast 2>/dev/null)"
[ "$out" = five ] || { echo "FAIL auto-update: updated in CI"; exit 1; }
echo "ok auto-update off with CASHCAST_AUTO_UPDATE=0 and in CI"
