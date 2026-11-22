#!/usr/bin/env bash
# Repo-wide checkup: the whole health picture in one command, open work first.
# Fast on a quiet day (the gate is cached by input hash), full when anything changed.
# Run it before a work session and in the monthly checkup (RECURRING.md).
set -uo pipefail
cd "$(dirname "$0")/.."

rc=0
section() { printf '\n== %s\n' "$1"; }

section "Open work (your PRs, unmerged branches, uncommitted files)"
open_work="${WRAP_UP_SKILL:-$HOME/.claude/skills/wrap-up}/scripts/open-work.sh"
if [ -f "$open_work" ]; then
  if ! bash "$open_work" .; then
    rc=1
    echo "-> finish these first: conflicts, red checks, review comments, then merge (wrap-up skill, \"Continue open work\")"
  fi
else
  git status --short
  if command -v gh >/dev/null 2>&1; then gh pr list --author @me --state open || true; fi
  echo "(wrap-up skill not installed; showed git status and your open PRs)"
fi

section "Health"
scripts/doctor.sh || rc=1

section "Gate (cached by input hash; NO_CACHE=1 forces a full run)"
scripts/check.sh || rc=1

section "Broken stories (app errors by story, last 30 days, all users; content-free)"
shopt -s nullglob
logs=(private/errors.jsonl private/logs/errors.jsonl private/users/*/logs/errors.jsonl)
if [ ${#logs[@]} -gt 0 ]; then
  since=$(( $(date +%s) - 30 * 86400 ))
  broken="$(cat "${logs[@]}" | awk -v s="$since" -F'"ts": ' '{split($2,a,","); if (a[1]+0 > s) print}' \
    | grep -oE '"story": "[^"]+"' | sort | uniq -c | sort -rn || true)"
  if [ -n "$broken" ]; then echo "$broken"; echo "-> each one is a P0 task"; else echo "none"; fi
else
  echo "no error logs"
fi
if [ -f .metrics/events.jsonl ]; then
  grep '"ok":false' .metrics/events.jsonl | grep -oE '"name":"[^"]+"' | sort | uniq -c | sort -rn | head -5 || true
fi

section "Speed"
scripts/metrics.sh

section "Open P0 tasks"
if [ -f TASKS.md ]; then
  awk '/^## P0/{f=1; next} /^## /{f=0} f && /^- \[ \]/' TASKS.md | sed 's/^- \[ \] /- /'
fi

section "Monthly requirements audit"
skill="${NEW_PROJECT_SKILL:-$HOME/.claude/skills/new-project}"
if [ -x "$skill/scripts/audit-due.sh" ]; then
  if "$skill/scripts/audit-due.sh" .; then
    echo "-> run it: $skill/references/requirements-map.md, section 4"
  fi
else
  echo "new-project skill not installed; skipped"
fi

printf '\ncheckup: %s\n' "$([ "$rc" = 0 ] && echo healthy || echo "needs work (see above)")"
exit "$rc"
