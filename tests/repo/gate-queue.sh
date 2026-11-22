#!/usr/bin/env bash
# gate-self-queue: gates run side by side by several agents froze the Mac (2026-11-01),
# so the full gate queues itself behind heavy-run unless it already holds a slot.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT="$PWD"
fail() { echo "FAIL: $*" >&2; exit 1; }
T="$(mktemp -d "${TMPDIR:-/tmp}/gate-queue.XXXXXX")"
trap 'rm -rf "$T"' EXIT
printf '#!/bin/sh\necho "$@" > "%s/calls"\n' "$T" > "$T/heavy-run"
chmod +x "$T/heavy-run"
# BASH_ENV (the shell profile) would put the real heavy-run first on PATH.
env -u HEAVY_RUN_SLOT -u BASH_ENV PATH="$T:$PATH" bash scripts/check.sh
[ "$(cat "$T/calls")" = "$ROOT/scripts/check.sh" ] || fail "the full gate did not rerun through heavy-run: $(cat "$T/calls")"
grep -qE '\[ "\$mode" != fast \] && \[ "\$mode" != changed \] && \[ -z "\$\{HEAVY_RUN_SLOT:-\}" \]' scripts/check.sh \
  || fail "fast and changed runs, and runs inside a slot, must skip the queue"
echo "ok the full gate queues itself behind heavy-run"
