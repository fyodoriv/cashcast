#!/usr/bin/env bash
# US-07/US-08: a full strategy and the monthly review, from real CLI runs.
source "$(dirname "$0")/lib.sh"

us07_happy_strategy_document() {
  setup_person
  ok strategy --today 2026-10-18
  has "Cash back: where to pay" "Credit score" "Tax: rates and moves" "Balance forecast" \
    "Payments analysis" "Alternatives finder" "Monthly review" "Dated checklist"
  has "not a licensed financial advisor"
  eq "$(mode "$(home alex)/out/strategy.md")" 600
  no_canary_on_disk alex
}

us07_happy_recurring_payments_are_numbered_never_named() {
  ok users add alex --province ON
  full_profile "$(home alex)/profile.toml"
  subscriptions_csv > "$(home alex)/inbox/bank.csv"
  grant alex agent:aggregates agent:strategy
  ok import --today 2026-10-18
  ok payments --today 2026-10-18
  has "### Recurring payments"
  eq "$(printf '%s\n' "$OUT" | grep '^| Payments #' | sed 's/ | .*//')" $'| Payments #1\n| Payments #2\n| Payments #3'
  has '$4,430.88 a year in total'
  ok alternatives --today 2026-10-18
  eq "$(printf '%s\n' "$OUT" | grep -c '^- Review Payments #')" 3
  no_canary_on_disk alex
}

us07_error_missing_profile_says_how_to_fix() {
  ok users add alex --province ON
  rm "$(home alex)/profile.toml"
  try strategy; has "users add alex --province"
}

us08_happy_review_lists_dated_steps() {
  setup_person
  ok review --today 2026-10-18
  has "Next review: **Mon, Nov 2**" "- [ ] 2026-11-02"
}

run_test us07_happy_strategy_document
run_test us07_happy_recurring_payments_are_numbered_never_named
run_test us07_error_missing_profile_says_how_to_fix
run_test us08_happy_review_lists_dated_steps
