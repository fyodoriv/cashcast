#!/usr/bin/env bash
# The one gate. Agents run it before they say a change is done.
#   scripts/check.sh        full gate: docs, web, engine, repo tests, e2e, parity (once per PR)
#   scripts/check.sh fast   skips e2e and parity (each commit)
#   scripts/check.sh changed  only the tests of the changed files (while fixing; not a gate)
# Each step's time goes to .metrics/events.jsonl; scripts/metrics.sh reports it.
#
# Fast yet full: a step that passed is skipped while the repo's inputs are
# unchanged (hash of tracked, staged, unstaged, and untracked files, like the
# nx cache). Any change re-runs every step. NO_CACHE=1 forces a full run (do it
# after a tool upgrade). CHECK_OFFLINE=1 skips steps that need the network.
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:-full}"
# Queue behind every other heavy run on this Mac (heavy-run, from dotfiles): gates
# run side by side by several agents froze the Mac on 2026-11-01. Inside
# heavy-run HEAVY_RUN_SLOT is set, so the gate queues once. `fast` and `changed` stay unqueued.
if [ "$mode" != fast ] && [ "$mode" != changed ] && [ -z "${HEAVY_RUN_SLOT:-}" ] && command -v heavy-run >/dev/null 2>&1; then
  exec heavy-run "$PWD/scripts/check.sh" "$@"
fi
# npm (the tasks linter) needs a writable cache; agent sandboxes often block it: a root-owned or read-only ~/.npm breaks npx.
[ -w "$HOME/.npm/_cacache" ] && [ -z "$(find "$HOME/.npm/_cacache" -maxdepth 1 -user 0 2>/dev/null | head -1)" ] \
  || export npm_config_cache="${TMPDIR:-/tmp}/npm-cache"
# Same for corepack (pnpm): it caches under ~/.cache/node.
mkdir -p "${COREPACK_HOME:-$HOME/.cache/node/corepack}" 2>/dev/null && [ -w "${COREPACK_HOME:-$HOME/.cache/node/corepack}" ] \
  || export COREPACK_HOME="${TMPDIR:-/tmp}/corepack"
mkdir -p .metrics/cache

if [ "$mode" = changed ]; then
  # While fixing: only the tests that match the changed files (scripts/changed-tests.sh).
  # The full gate still runs once, right before each commit.
  start=$(date +%s)
  files=()
  while IFS= read -r f; do [ -f "$f" ] && files+=("$f"); done < <( { git diff --name-only HEAD; git ls-files --others --exclude-standard; } | sort -u)
  n=0
  while IFS= read -r t; do
    [ -n "$t" ] || continue
    n=$((n + 1))
    case "$t" in
      engine:lib) cargo test --quiet --manifest-path engine/Cargo.toml --lib ;;
      engine:*) cargo test --quiet --manifest-path engine/Cargo.toml --test "${t#engine:}" ;;
      e2e:*) bash "${t#e2e:}" ;;
      repo:*) bash "${t#repo:}" ;;
    esac
  done < <(scripts/changed-tests.sh ${files[@]+"${files[@]}"})
  [ "$n" -gt 0 ] || echo "check.sh changed: no matching tests"
  echo "check.sh changed: ${#files[@]} changed file(s), $n test target(s) in $(( $(date +%s) - start )) s; run scripts/check.sh before the commit"
  exit 0
fi

key=""
if [ "${NO_CACHE:-0}" != 1 ] && git rev-parse --git-dir >/dev/null 2>&1; then
  key="$( { echo "$mode"; git ls-files -s; git diff;
            git ls-files -z --others --exclude-standard | xargs -0 shasum 2>/dev/null || true; } | shasum | cut -c1-16)"
  # `|| true`: an untracked directory (a nested worktree) makes shasum exit 1, and pipefail ended the gate silently.
fi

step() {
  # step <name> <command...>: run, time, and log one gate step; skip it on a cache hit.
  name="$1"; shift
  if [ -n "$key" ] && [ "$(cat ".metrics/cache/$name" 2>/dev/null)" = "$key" ]; then
    printf '{"ts":"%s","event":"check.step","name":"%s","seconds":0,"ok":true,"cached":true}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$name" >> .metrics/events.jsonl
    echo "check.sh: $name cached (inputs unchanged)"
    return
  fi
  start=$(date +%s)
  if "$@"; then okv=true; else okv=false; fi
  secs=$(( $(date +%s) - start ))
  printf '{"ts":"%s","event":"check.step","name":"%s","seconds":%s,"ok":%s}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$name" "$secs" "$okv" >> .metrics/events.jsonl
  [ "$okv" = true ] || { echo "check.sh: step '$name' failed" >&2; exit 1; }
  [ -z "$key" ] || echo "$key" > ".metrics/cache/$name"
}

step docs scripts/check-docs.sh
step readiness scripts/public-readiness.sh  # SECURITY.md: may this tree be public?
if command -v gitleaks >/dev/null 2>&1 && git rev-parse -q --verify HEAD >/dev/null; then
  step secrets gitleaks git --no-banner --redact -l warn --log-opts=-20
fi
if [ -f TASKS.md ] && command -v npx >/dev/null 2>&1 && [ "${CHECK_OFFLINE:-0}" != 1 ]; then
  if npx --no-install @tasks-md/lint --help >/dev/null 2>&1 || npm view @tasks-md/lint version >/dev/null 2>&1; then
    step tasks npx -y @tasks-md/lint --require-prereg TASKS.md
  else
    echo "check.sh: tasks lint skipped (offline and not cached)"
  fi
fi

if [ -f web/package.json ] && command -v pnpm >/dev/null 2>&1; then
  [ -d web/node_modules ] || (cd web && pnpm install --frozen-lockfile)
  step web-lint sh -c 'cd web && pnpm -s lint'
  step web-types sh -c 'cd web && pnpm -s typecheck'
  step web-test sh -c 'cd web && pnpm -s test'
  step web-build sh -c 'cd web && pnpm -s build'
fi
if [ -f engine/Cargo.toml ]; then
  step engine cargo test --quiet --manifest-path engine/Cargo.toml
  step knowledge sh -c 'bin/cashcast knowledge check >/dev/null'
fi
step repo tests/repo/run.sh

if [ "$mode" != fast ]; then step e2e scripts/e2e.sh; fi
if [ "$mode" != fast ]; then
  step parity-test tests/parity/harness-test.sh
  step parity scripts/rust-parity.sh  # the app still gives the frozen Python outputs (decision 0010)
fi
if [ "${CHECK_EVALS:-0}" = 1 ]; then
  [ -x evals/run.sh ] || { echo "check.sh: CHECK_EVALS=1 but evals/run.sh is missing" >&2; exit 1; }
  # Never cached: the judge cases depend on the local Ollama model, not only on repo files.
  rm -f .metrics/cache/evals
  step evals evals/run.sh
fi
echo "check.sh: $mode gate passed"
