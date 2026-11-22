#!/usr/bin/env bash
# US-01 (happy path): a new contributor runs the one-line setup and gets a
# healthy machine. Copies the repo to a scratch dir so the real checkout is
# never touched. Replace or extend with the first real user story.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/e2e-setup.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

tar -C "$root" --exclude=.git --exclude=node_modules --exclude=.venv --exclude=target \
  --exclude=private --exclude=.env --exclude=.metrics -cf - . | tar -C "$scratch" -xf -
# Dummy values for required env keys, so the doctor can pass in the copy.
[ -f "$scratch/.env.example" ] && sed -E 's/^([A-Z][A-Z0-9_]*)=[^#]*# required/\1=e2e-dummy # required/' \
  "$scratch/.env.example" > "$scratch/.env"
# Fast mode reuses nothing and installs nothing; E2E_COLD=1 runs the full stranger install.
skip=1; [ "${E2E_COLD:-0}" = 1 ] && skip=0
# Fast mode copies the built engine, which the doctor's app health check runs.
if [ "$skip" = 1 ]; then
  mkdir -p "$scratch/engine/target/release"
  cp "$root/engine/target/release/cashcast-engine" "$scratch/engine/target/release/"
fi
out="$(cd "$scratch" && SETUP_SKIP_DEPS=$skip SETUP_SKIP_LINK=1 scripts/setup.sh 2>&1)" || true  # the grep below reports a failure with the output
echo "$out" | grep -q "doctor: all checks passed" || { echo "$out"; echo "FAIL: setup did not end healthy"; exit 1; }
[ -d "$scratch/.metrics" ] || { echo "FAIL: setup did not create .metrics/"; exit 1; }
echo "ok US-01 setup"

# US-01 (error): a broken connector spec makes the doctor fail with a fix line.
rm "$scratch/connectors/playbooks/wise.md"
bad="$(cd "$scratch" && scripts/doctor.sh 2>&1 || true)"
echo "$bad" | grep -q "Connectors with problems: wise" || { echo "$bad"; echo "FAIL: doctor missed the broken connector"; exit 1; }
echo "$bad" | grep -q "FAIL  app health -> fix:" || { echo "$bad"; echo "FAIL: doctor printed no fix"; exit 1; }
echo "ok US-01 doctor error case"
