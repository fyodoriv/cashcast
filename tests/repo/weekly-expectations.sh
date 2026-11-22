#!/usr/bin/env bash
# The Sunday weekly does what the owner asked for on 2026-11-22: 7 results in
# order, MoneyWiz written without asking, nothing in Reminders or Calendar
# before the owner confirms, and the results page before any improvements.
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }
has() { grep -qF -- "$2" <<< "$1" || fail "$3: missing: $2"; }

cmd="$(cat commands/cashcast-weekly.md)"
n=0
for want in "Real data from every bank today" "Write it, do not ask" \
  "The fewest transfers, with easy-to-type amounts" "Charges on the wrong card" \
  "Forecast and what to do next" "replace typing into MoneyWiz, except cash" "One results page"; do
  n=$((n + 1))
  line="$(grep -n -F -- "$want" <<< "$cmd" | head -1 | cut -d: -f1)"
  [ -n "$line" ] || fail "commands/cashcast-weekly.md: result $n missing: $want"
  [ "$(sed -n "${line}p" <<< "$cmd" | grep -oE '^[0-9]+\.')" = "$n." ] \
    || fail "commands/cashcast-weekly.md: \"$want\" is not result $n"
done
has "$cmd" "Put nothing
   into Reminders or Calendar until the owner confirms" "commands/cashcast-weekly.md"
has "$cmd" "bin/cashcast cards missed" "commands/cashcast-weekly.md"
has "$cmd" "Results first,
improvements second" "commands/cashcast-weekly.md"
echo "ok /cashcast-weekly lists the owner's 7 results in order"

skill="$(awk '/^## Weekly session/{on=1; next} on && /^## /{exit} on' .claude/skills/cashcast/SKILL.md)"
has "$skill" '`commands/cashcast-weekly.md`' "skill Weekly session"
has "$skill" "never pass it before the owner confirms" "skill Weekly session"
has "$skill" '"write away"' "skill Weekly session"
has "$skill" "Results page first" "skill Weekly session"
has "$skill" "cards missed" "skill Weekly session"
grep -qF -- "bin/cashcast sync all\` so the closed checks leave Reminders" <<< "$skill" \
  && fail "skill Weekly session still syncs Reminders before the owner confirms"
echo "ok the skill's Weekly session follows the command and waits for the owner"

grep -qF '/cashcast-weekly' engine/src/tasks/weekly.rs \
  || fail "the weekly reminder does not point at /cashcast-weekly"
grep -qF 'fn weekly_sync_step' engine/src/core/cli.rs \
  || fail "weekly has no read-only task sync step"
echo "ok the Sunday reminder points at /cashcast-weekly and weekly reads the task sync only"
