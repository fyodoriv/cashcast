#!/usr/bin/env bash
# US-05: several Canadian users on one computer, each fully separate.
source "$(dirname "$0")/lib.sh"

us05_happy_two_users_stay_separate() {
  ok users add alex --province ON
  ok users add sam --province QC
  ok users list; has $'- alex\n- sam'
  chequing_csv > "$(home alex)/inbox/bank.csv"
  grant alex agent:aggregates
  ok --user alex import --today 2026-10-18
  [[ "$OUT" == *"Files read"* || "${OUT,,}" == *files* ]] || fail "no files line"
  [ -f "$(home alex)/data/summary.json" ] || fail "no summary for alex"
  [ ! -e "$(home sam)/data/summary.json" ] || fail "sam got alex's summary"
  eq "$(mode "$(home alex)")" 700
  no_canary_on_disk alex
  event_any alex '.event == "command" and .command == "import"'
}

us05_error_non_canadian_and_ambiguous_user() {
  try users add tex --province TX; code_not0; has "province must be one of"
  ok users add alex --province ON
  ok users add sam --province BC
  try strategy; has "pass --user NAME"
}

run_test us05_happy_two_users_stay_separate
run_test us05_error_non_canadian_and_ambiguous_user
