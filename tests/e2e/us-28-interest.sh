#!/usr/bin/env bash
# US-28: the strategy shows what each cash balance earns, from real CLI runs.
source "$(dirname "$0")/lib.sh"

us28_happy_wise_interest_and_bank_rates() {
  setup_person
  echo '{"wise": {"balances": {"USD": 1000.0}, "fetched_on": "2026-10-17"}}' > "$(home alex)/data/api-balances.json"
  ok strategy --today 2026-10-18
  has "Interest on cash: what each balance earns" "| Wise USD |" "Interest a year at today's rates" "line 12100"
  no_canary_on_disk alex
}

run_test us28_happy_wise_interest_and_bank_rates
