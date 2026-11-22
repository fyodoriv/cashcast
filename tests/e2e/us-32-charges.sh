#!/usr/bin/env bash
# US-32: find charges I did not expect on every account: duplicate
# subscriptions, new recurring charges, price rises, charges after a pause.
# `charges` and `charges find WORD`.
source "$(dirname "$0")/lib.sh"

# monthly TEXT FIRST N AMOUNT: N charges 30 days apart, oldest first.
monthly() {
  local k
  echo "Date,Description,Amount"
  for ((k = 0; k < $3; k++)); do
    echo "$(date -u -d "$2 + $((30 * k)) days" +%F 2> /dev/null || date -u -j -v+"$((30 * k))"d -f %F "$2" +%F),$1,-$4"
  done
}

person() {
  ok users add alex --province ON
  monthly "ANTHROPIC PBC CANARY91" 2025-11-05 12 28.00 > "$(home alex)/inbox/visa-a.csv"
  monthly "CLAUDE.AI SUBSCRIPTION CANARY92" 2026-07-03 4 140.00 > "$(home alex)/inbox/visa-b.csv"
  grant alex "$@"
}

us32_happy_finds_a_duplicate_subscription_and_lists_it_per_account() {
  person agent:strategy
  ok charges --today 2026-10-20
  has "## Duplicate subscriptions" "- Anthropic: 2 series"
  ok charges find anthropic --today 2026-10-20
  has "2 account(s), 2 active recurring series." "monthly 140.00" "monthly 28.00"
  no_canary_on_disk alex
}

us32_error_withheld_without_consent_and_find_needs_a_word() {
  person
  ok charges find anthropic
  has "Report saved for the owner only"; lacks "Anthropic"
  try charges find
  has "charges find needs a word"
}

run_test us32_happy_finds_a_duplicate_subscription_and_lists_it_per_account
run_test us32_error_withheld_without_consent_and_find_needs_a_word
