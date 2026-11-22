#!/usr/bin/env bash
# US-21: Reminders, Calendar and Notion tasks stay in step (`sync all`, `sync status`).
# The two-way behaviour runs against fake apps in the engine tests; here the
# CLI paths that need no macOS app.
source "$(dirname "$0")/lib.sh"

person() {
  ok users add alex --province ON
  grant alex agent:strategy agent:aggregates
}

us21_happy_status_lists_each_target_and_how_to_turn_it_on() {
  person
  try sync status --today 2026-10-25; code_is 1; has "out of step"
  has "consent grant local:reminders" "tasks_database" "hourly sync"
  ok sync all --today 2026-10-25; has "Nothing to sync yet"
}

us21_error_notion_without_its_consent_says_so() {
  person
  printf '[notion]\ntasks_database = "db-canary"\n' > "$(home alex)/cashcast.toml"
  try sync status --today 2026-10-25; has "consent grant cloud:notion"
  try sync sheets; has "usage: bin/cashcast sync sheets SHEET_ID"
}

run_test us21_happy_status_lists_each_target_and_how_to_turn_it_on
run_test us21_error_notion_without_its_consent_says_so
