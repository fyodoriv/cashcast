#!/usr/bin/env bash
# US-16 calendar, US-17 bank changes, US-18 stock optimizer, US-19 one command
# starts cashcast, US-23 tasks resolve and verdicts.
source "$(dirname "$0")/lib.sh"

OPTIMIZE='
[optimize]
min_trade = 0

[optimize.cash]
tfsa = 6000
rrsp = 4000

[[optimize.targets]]
symbol = "EQCA"
weight = 0.5
listing = "CA"

[[optimize.targets]]
symbol = "EQUS"
weight = 0.3
listing = "US"

[[optimize.targets]]
symbol = "BOND"
weight = 0.2
asset_class = "bond"
'

person() {
  ok users add alex --province ON --display Alex
  full_profile "$(home alex)/profile.toml"
  chequing_csv > "$(home alex)/inbox/chequing.csv"
  grant alex agent:strategy agent:aggregates
}

us16_happy_calendar_preview_lists_only_dated_deadlines() {
  person
  ok calendar preview --today 2026-10-21
  has "Calendar 'Cashcast · Alex'" "bin/cashcast calendar sync"
  eq "$(printf '%s\n' "$OUT" | grep '^- 2' | grep -cvE '^- [0-9]{4}-[0-9]{2}-[0-9]{2}: ' || true)" 0
}

us16_error_calendar_sync_needs_consent_then_macos() {
  ! command -v osascript > /dev/null || skip "on macOS this would write to Calendar"
  person
  try calendar sync; has "local:calendar"
  grant alex local:calendar
  try calendar sync; code_not0; has "Terminal"
}

us17_happy_changes_list_and_done() {
  person
  ok changes --today 2026-10-21
  has "Bank and card changes" "consent grant actions:browser"
  local ids; ids="$(printf '%s\n' "$OUT" | sed -nE 's/^[0-9]+\. `([^`]+)`.*/\1/p')"
  eq "$ids" $'keep-active-test-costly\ndowngrade-test-costly'  # the synthetic costly card
  ok changes "done" downgrade-test-costly; has "Marked downgrade-test-costly done"
  ok changes --today 2026-10-21; lacks '`downgrade-test-costly`'
}

us17_error_unknown_change_explains() {
  person
  try changes show nope; code_is 3; has "changes list"
}

us18_happy_optimize_from_the_profile() {
  person
  printf '%s' "$OPTIMIZE" >> "$(home alex)/profile.toml"
  ok optimize
  has '| rrsp | BOND | $2,000.00 |' '| tfsa | EQCA | $5,000.00 |' "Largest gap to target" "You place the trades yourself"
  event_any alex '.event == "optimize"'
}

us18_error_optimize_without_inputs_explains() {
  person
  try optimize; code_is 3; has "[optimize]" "profile.toml"
}

us19_happy_one_command_starts_and_reuses_the_web_app() {
  person
  ok config set web.port 0
  _run
  if [ "$CODE" -ne 0 ] && [[ "$OUT" == *"Operation not permitted"* || "$OUT" == *"no permission"* ]]; then
    skip "this sandbox does not allow a local port"
  fi
  code_is 0
  has "Web app: started" "Exports: read." "Coming up for Alex" "Next:"
  lacks "#t="  # an agent never sees the token link
  local state="$PRIVATE/out/web.json"
  [ "$(jq .port "$state")" -gt 0 ] || fail "no port in web.json"
  eq "$(mode "$state")" 600
  ! grep -q '#t=' "$PRIVATE/out/web.log" || fail "the log holds the token"
  ok; has "Web app: already running"
  ok stop; has "Stopped the web app"
  [ ! -e "$state" ] || fail "web.json is still there"
}

us19_config_round_trips_a_setting_and_rejects_bad_values() {
  ok config; has "web.port = 8765  (default)"
  ok config set start.tasks 3; has "start.tasks = 3"
  ok config; OUT="${OUT//"$PRIVATE"/private}"; has "start.tasks = 3  (private/settings.toml)"
  try config set web.port nope; code_is 3; has "whole number"
}

us19_error_no_people_yet_says_what_to_do() {
  ok config set web.autostart false
  ok; has "autostart is off" "users add NAME --province XX"
}

us19_agents_read_the_action_catalog() {
  ok actions --json
  jq -e '[.[].name] as $n | ["start", "calendar", "changes", "optimize", "config"] | all(. as $x | $n | index($x))' <<< "$OUT" > /dev/null ||
    fail "an action is missing from the catalog"
  jq -e '.[] | select(.name == "web") | .terminal_only | . != null and . != false and . != ""' <<< "$OUT" > /dev/null ||
    fail "web is not terminal only"
}

us23_happy_tasks_resolve_closes_the_review_and_names_blockers() {
  person
  ok tasks --today 2026-10-30; has "# Open finance tasks" "Blocked: you do these"
  ok tasks --json --today 2026-10-30
  jq -e 'length > 0 and all(.[]; (.lane | IN("auto", "agent", "browser", "you")) and (.step | . != null and . != ""))' <<< "$OUT" > /dev/null ||
    fail "a task has no lane or step"
  ok tasks resolve --today 2026-10-30
  has "Resolved 1 of" 'Done: `monthly-review-2026-11`' "## BLOCKED ("
  ok tasks --json --today 2026-10-30
  jq -e 'all(.[]; .id != "monthly-review-2026-11")' <<< "$OUT" > /dev/null || fail "the review is still open"
}

us23_happy_an_agent_records_a_check_without_amounts() {
  person
  ok tasks --json --today 2026-10-30
  local id day
  id="$(jq -r '[.[] | select(.lane == "agent")][0].id' <<< "$OUT")"
  day="$(jq -r '[.[] | select(.lane == "agent")][0].due // "2026-10-30"' <<< "$OUT")"
  [ "$id" != null ] || fail "no agent task"
  try tasks verdict "$id" pass --reason 'paid $1,234.56' --today "$day"; code_is 1; has "no amount"
  try tasks verdict "$id" pass --reason "shown as delivered" --source browser --today "$day"
  code_is 1; has "consent grant agent:browser-read"
  ok tasks verdict "$id" pass --reason "shown as delivered" --today "$day"; has "Marked $id done"
  ok tasks --json --today "$day"
  jq -e --arg id "$id" 'all(.[]; .id != $id)' <<< "$OUT" > /dev/null || fail "the check is still open"
}

run_test us16_happy_calendar_preview_lists_only_dated_deadlines
run_test us16_error_calendar_sync_needs_consent_then_macos
run_test us17_happy_changes_list_and_done
run_test us17_error_unknown_change_explains
run_test us18_happy_optimize_from_the_profile
run_test us18_error_optimize_without_inputs_explains
run_test us19_happy_one_command_starts_and_reuses_the_web_app
run_test us19_config_round_trips_a_setting_and_rejects_bad_values
run_test us19_error_no_people_yet_says_what_to_do
run_test us19_agents_read_the_action_catalog
run_test us23_happy_tasks_resolve_closes_the_review_and_names_blockers
run_test us23_happy_an_agent_records_a_check_without_amounts
