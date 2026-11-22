#!/usr/bin/env bash
# US-09 reminders, US-10 connectors and actions, US-11 knowledge.
source "$(dirname "$0")/lib.sh"

us09_happy_reminders_preview_uses_the_users_own_list() {
  ok users add alex --province ON --display Alex
  grant alex agent:strategy
  ok reminders preview --today 2026-10-18
  # the monthly review folds into the weekly session (setting reminders.weekly)
  has "list 'Cashcast · Alex'" "Weekly cashcast session"
}

us09_error_sync_without_osascript_explains() {
  ! command -v osascript > /dev/null || skip "on macOS this would write to Reminders"
  ok users add alex --province ON
  try reminders sync; has "local:reminders"  # amounts in task titles need consent first
  grant alex local:reminders
  try reminders sync; code_not0; has "Terminal"
}

us10_happy_connectors_and_safe_actions() {
  local name
  ok connectors list
  for name in MoneyWiz Scotiabank TD Wise Wealthsimple Casa Kraken Chexy PayPal Paysend "Google Sheets" "E*TRADE"; do
    [[ "${OUT,,}" == *"${name,,}"* ]] || fail "missing connector: $name"
  done
  ok users add alex --province ON
  try action kraken withdraw-cad; code_is 3; has "never does it"
  ok connectors show kraken
  [[ "${OUT,,}" == *withdraw* ]] || fail "no withdraw in kraken"
}

us10_error_api_without_consent() {
  ok users add alex --province ON
  try api balances wise; has "actions:api"
}

us11_happy_knowledge_check() {
  ok knowledge check; has "Knowledge check"; lacks "Problems"
  ok knowledge list; has "credit_cards"
}

# Both syncs run osascript with the script in engine/data/ and print the counts.
us09_happy_sync_runs_the_shared_jxa_script() {
  ok users add alex --province ON
  grant alex local:reminders local:calendar
  mkdir -p "$T/bin"
  cat > "$T/bin/osascript" <<FAKE
#!/bin/sh
# A fake osascript: records the script it got and answers like the JXA sync.
cp "\$3" "$T/\$(basename "\$3")"
case "\$(head -c 400 "\$3")" in
  *calendar*) echo '{"calendar": "Cal", "created": 2, "updated": 1, "removed": 0, "held": 0}' ;;
  *) echo '{"list": "List", "created": 3, "updated": 1, "completed_kept": 2, "completed": []}' ;;
esac
FAKE
  chmod 755 "$T/bin/osascript"
  export PATH="$T/bin:$PATH"
  ok reminders sync --today 2026-10-18
  has "Reminders list 'List': 3 created, 1 updated, 2 already completed."
  ok calendar sync --today 2026-10-18
  has "Calendar 'Cal': 2 created, 1 updated, 0 removed (no longer due)."
  cmp -s "$T/sync.js" "$ROOT/engine/data/reminders-sync.js" || cmp -s "$T/sync.js" "$ROOT/engine/data/calendar-sync.js" ||
    fail "osascript did not get the shared script"
  eq "$(events alex | jq -s -c '[.[] | select(.event == "reminders_sync" or .event == "calendar_sync") | .result]')" '["ok","ok"]'
}

run_test us09_happy_reminders_preview_uses_the_users_own_list
run_test us09_error_sync_without_osascript_explains
run_test us10_happy_connectors_and_safe_actions
run_test us10_error_api_without_consent
run_test us11_happy_knowledge_check
run_test us09_happy_sync_runs_the_shared_jxa_script
