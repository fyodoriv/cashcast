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
