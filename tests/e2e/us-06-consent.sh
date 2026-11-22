#!/usr/bin/env bash
# US-06: private reports reach an agent only after the owner says yes.
source "$(dirname "$0")/lib.sh"

us06_happy_owner_grant_shows_reports() {
  ok users add alex --province ON
  chequing_csv > "$(home alex)/inbox/bank.csv"
  ok import --today 2026-10-18
  has "Report saved for the owner only" "consent grant agent:aggregates"
  [ -f "$(home alex)/out/last-import.md" ] || fail "no owner report"
  grant alex agent:aggregates
  ok import --today 2026-10-18
  lacks "Report saved for the owner only"
  event_any alex '.event == "report_withheld"'
  event_any alex '.event == "report_shown"'
}

us06_error_agent_cannot_grant_itself() {
  ok users add alex --province ON
  try consent grant agent:aggregates; code_not0; has "needs a real terminal"
  ok consent list; has "not granted"
  try first-error; has "agent:raw-row"
}

us06_telemetry_sends_nothing_without_consent_and_key() {
  ok users add alex --province ON
  ok telemetry; has "no consent"
  ok telemetry send --dry-run; has "<posthog_key>"
  grant alex net:usage-counts
  ok telemetry send; has "no key"
  ok telemetry off; has "turned off"
  ok config; has "telemetry.enabled = False"
}

run_test us06_happy_owner_grant_shows_reports
run_test us06_error_agent_cannot_grant_itself
run_test us06_telemetry_sends_nothing_without_consent_and_key
