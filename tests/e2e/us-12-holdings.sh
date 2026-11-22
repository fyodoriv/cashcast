#!/usr/bin/env bash
# US-12: investments and crypto from broker and wallet exports: positions, ACB, gains by year.
source "$(dirname "$0")/lib.sh"

setup_ledgers() {
  local name
  ok users add alex --province ON
  for name in "$@"; do cp "$FIXTURES/$name" "$(home alex)/inbox/$name"; done
  grant alex agent:aggregates agent:strategy
}

us12_happy_holdings_report_and_json() {
  setup_ledgers ledger-kraken.csv ledger-wealthsimple-activity.csv
  ok holdings; has "Holdings report" "| BTC |" "Realized gains"
  local data; data="$(home alex)/data/holdings.json"
  eq "$(mode "$data")" 600
  jq -e 'has("assets") and has("realized_by_year") and has("superficial_loss_flags") and has("needs_cost")' "$data" > /dev/null ||
    fail "holdings.json misses a key"
  no_canary_on_disk alex
  ok tax --today 2026-10-18; has "Tax: rates and moves"
}

us12_error_crypto_without_a_cost_is_flagged_not_guessed() {
  setup_ledgers ledger-casa.csv ledger-kraken.csv
  ok holdings; has "cost unknown" "Needs a cost"
}

us12_happy_rrsp_check_reads_sun_life_contributions() {
  setup_ledgers ledger-sunlife-transactions.csv
  mv "$(home alex)/inbox/ledger-sunlife-transactions.csv" "$(home alex)/inbox/sunlife-2025.csv"
  local prof; prof="$(home alex)/profile.toml"
  awk '!a && /^rrsp = / { print "rrsp = 500"; a = 1; next }
       !b && /^rrsp_limit_year = / { print "rrsp_limit_year = 2024"; b = 1; next } { print }' "$prof" > "$prof.new"
  mv "$prof.new" "$prof"
  ok rrsp --today 2025-02-20
  has "RRSP contributions for 2024" "Over the limit" "buffer"
  no_canary_on_disk alex
}

us12_error_rrsp_check_without_a_limit_asks_for_the_noa() {
  setup_ledgers
  ok rrsp --today 2026-10-25
  has "deduction limit for 2026 is missing" "No RRSP contributions found"
}

run_test us12_happy_holdings_report_and_json
run_test us12_error_crypto_without_a_cost_is_flagged_not_guessed
run_test us12_happy_rrsp_check_reads_sun_life_contributions
run_test us12_error_rrsp_check_without_a_limit_asks_for_the_noa
