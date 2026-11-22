#!/usr/bin/env bash
# US-26: the strategy values Kraken crypto in CAD and counts Kraken cash, from real CLI runs.
source "$(dirname "$0")/lib.sh"

us26_happy_kraken_crypto_at_market_value() {
  setup_person
  echo '{"kraken": {"balances": {"BTC": 0.5, "CAD": 30.0}, "prices": {"BTC": {"price": 100000, "currency": "CAD"}}, "fetched_on": "2026-10-17"}}' \
    > "$(home alex)/data/api-balances.json"
  ok strategy --today 2026-10-18
  has "Crypto: market value from your exchanges" 'Crypto at market: **$50,000.00**' 'Includes Kraken CAD: $30.00.'
  no_canary_on_disk alex
}

us26_edge_no_kraken_balances_says_how_to_read_them() {
  setup_person
  ok strategy --today 2026-10-18; has "bin/cashcast api balances kraken"
}

run_test us26_happy_kraken_crypto_at_market_value
run_test us26_edge_no_kraken_balances_says_how_to_read_them
