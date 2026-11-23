#!/usr/bin/env bash
# bin/cashcast rebuilds the engine when its sources are newer than the
# binary (a `git pull` before the Sunday run), and leaves a linked
# engine/target alone (a worktree that shares the main checkout's build).
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fake_root() {
  local r="$1"
  mkdir -p "$r/bin" "$r/engine/src" "$r/engine/target/release" "$r/fakebin"
  cp bin/cashcast "$r/bin/cashcast"
  : > "$r/engine/Cargo.toml"
  : > "$r/engine/Cargo.lock"
  echo "// canary" > "$r/engine/src/main.rs"
  printf '#!/bin/sh\necho old-engine\n' > "$r/engine/target/release/cashcast-engine"
  chmod +x "$r/engine/target/release/cashcast-engine"
  # A fake cargo: "builds" by writing a new engine.
  printf '#!/bin/sh\nprintf "#!/bin/sh\\necho new-engine\\n" > "%s/engine/target/release/cashcast-engine"\n' "$r" > "$r/fakebin/cargo"
  chmod +x "$r/fakebin/cargo"
}

r="$tmp/a"
fake_root "$r"
touch -t 202601010000 "$r/engine/src/main.rs" "$r/engine/Cargo.toml" "$r/engine/Cargo.lock"
out="$(PATH="$r/fakebin:$PATH" CASHCAST_HOME="$tmp/data" "$r/bin/cashcast" status 2>/dev/null)"
[ "$out" = old-engine ] || { echo "FAIL launcher: rebuilt an up-to-date engine ($out)"; exit 1; }
echo "ok launcher keeps an up-to-date engine"

touch "$r/engine/src/main.rs"
out="$(PATH="$r/fakebin:$PATH" CASHCAST_HOME="$tmp/data" "$r/bin/cashcast" status 2>/dev/null)"
[ "$out" = new-engine ] || { echo "FAIL launcher: kept a stale engine after its sources changed ($out)"; exit 1; }
echo "ok launcher rebuilds a stale engine"

# A script built into the engine (engine/data) changed: rebuild too.
mkdir -p "$r/engine/data"
printf '#!/bin/sh\necho old-engine\n' > "$r/engine/target/release/cashcast-engine"
sleep 1
echo "// canary" > "$r/engine/data/reminders-sync.js"
out="$(PATH="$r/fakebin:$PATH" CASHCAST_HOME="$tmp/data" "$r/bin/cashcast" status 2>/dev/null)"
[ "$out" = new-engine ] || { echo "FAIL launcher: kept a stale engine after engine/data changed ($out)"; exit 1; }
echo "ok launcher rebuilds when engine/data changes"

b="$tmp/b"
fake_root "$b"
mv "$b/engine/target" "$tmp/shared-target"
ln -s "$tmp/shared-target" "$b/engine/target"
touch "$b/engine/src/main.rs"
out="$(PATH="$b/fakebin:$PATH" CASHCAST_HOME="$tmp/data" "$b/bin/cashcast" status 2>/dev/null)"
[ "$out" = old-engine ] || { echo "FAIL launcher: rebuilt a linked engine/target ($out)"; exit 1; }
echo "ok launcher leaves a linked engine/target alone"

# The web app: `cashcast web` (and the bare start) rebuild web/dist when the
# web sources or lockfile are newer than the build, or it is missing. A
# failed build keeps the old one. Other commands never build the web app.
w="$tmp/w"
fake_root "$w"
mkdir -p "$w/web/src" "$w/web/dist"
echo '{}' > "$w/web/package.json"
echo "// canary" > "$w/web/src/main.tsx"
echo old-dist > "$w/web/dist/index.html"
# A fake pnpm: `pnpm -s build [--outDir D]` writes D/index.html (vite empties
# D first). PNPM_FAIL=1 fails before writing; PNPM_FAIL=half empties D, then fails.
cat > "$w/fakebin/pnpm" <<'PNPM'
#!/bin/sh
case "$*" in *build*) ;; *) exit 0 ;; esac
out=dist; prev=""
for a in "$@"; do [ "$prev" = --outDir ] && out="$a"; prev="$a"; done
[ "${PNPM_FAIL:-0}" = 1 ] && exit 1
rm -rf "$out"; mkdir -p "$out"
[ "${PNPM_FAIL:-0}" = half ] && exit 1
echo new-dist > "$out/index.html"
PNPM
chmod +x "$w/fakebin/pnpm"
touch -t 202601010000 "$w/web/src/main.tsx" "$w/web/package.json"
run_w() { PATH="$w/fakebin:$PATH" CASHCAST_HOME="$tmp/data" CASHCAST_AUTO_UPDATE=0 "$w/bin/cashcast" "$@" >/dev/null 2>&1 || true; }
run_w web
[ "$(cat "$w/web/dist/index.html")" = old-dist ] || { echo "FAIL launcher: rebuilt an up-to-date web app"; exit 1; }
echo "ok launcher keeps an up-to-date web app"

touch "$w/web/src/main.tsx"
run_w status
[ "$(cat "$w/web/dist/index.html")" = old-dist ] || { echo "FAIL launcher: a non-web command built the web app"; exit 1; }
PNPM_FAIL=1 run_w web
[ "$(cat "$w/web/dist/index.html")" = old-dist ] || { echo "FAIL launcher: a failed web build lost the old one"; exit 1; }
PNPM_FAIL=half run_w web
[ "$(cat "$w/web/dist/index.html" 2>/dev/null)" = old-dist ] || { echo "FAIL launcher: a build that failed halfway emptied the web app"; exit 1; }
[ ! -e "$w/web/dist.next" ] || { echo "FAIL launcher: left a half-built dist.next"; exit 1; }
echo "ok launcher keeps the old web app when a build fails"
run_w --user alex web
[ "$(cat "$w/web/dist/index.html")" = new-dist ] || { echo "FAIL launcher: kept a stale web app after its sources changed (--user NAME web)"; exit 1; }
echo "ok launcher rebuilds a stale web app"

rm -rf "$w/web/dist"
run_w
[ "$(cat "$w/web/dist/index.html" 2>/dev/null)" = new-dist ] || { echo "FAIL launcher: the start did not build a missing web app"; exit 1; }
echo "ok launcher builds a missing web app on start"

# A front option with no value (the engine reports the error) must not hang.
run_w --today 2026-10-18 --user &
pid=$!
for _ in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid"; echo "FAIL launcher: hung on a trailing --user with no name"; exit 1
fi
echo "ok launcher does not hang on a trailing --user"
