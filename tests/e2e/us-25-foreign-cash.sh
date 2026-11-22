#!/usr/bin/env bash
# US-25: the strategy counts Wise USD in CAD and gives advice that fits it, from real CLI runs.
source "$(dirname "$0")/lib.sh"

us25_happy_wise_usd_counts_in_the_reserve() {
  setup_person
  echo '{"wise": {"balances": {"USD": 1000.0}, "fetched_on": "2026-10-17"}}' > "$(home alex)/data/api-balances.json"
  ok strategy --today 2026-10-18
  has "Foreign cash: Wise USD and other currencies" 'Includes Wise USD: 1,000.00 USD = $'
  has "Norbert's gambit" "T1135" "line 12100"
  no_canary_on_disk alex
}

us25_edge_no_wise_balances_says_how_to_read_them() {
  setup_person
  ok strategy --today 2026-10-18; has "bin/cashcast api balances wise"
}

run_test us25_happy_wise_usd_counts_in_the_reserve
run_test us25_edge_no_wise_balances_says_how_to_read_them
