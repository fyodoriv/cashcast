#!/usr/bin/env bash
# US-24: from the balances an agent read on the bank sites, plan the e-Transfers
# that bring each bank to its keep amount. The browser part runs in the agent
# (skills/bank-balances); here the CLI it relies on: `transfers`.
source "$(dirname "$0")/lib.sh"

PROFILE='
[transfers]
hub = "hub-chequing"
daily_limit = 3000

[[banking.accounts]]
id = "hub-chequing"
keep = 15000

[[banking.accounts]]
id = "alpha-chequing"
keep = 6500

[[wallet.cards]]
id = "alpha-visa"
target = 250
'

person() {
  ok users add alex --province ON
  printf '%s' "$PROFILE" >> "$(home alex)/profile.toml"
  grant alex "$@"
}

us24_happy_plans_quick_amounts_under_the_daily_limit() {
  person agent:strategy
  ok transfers --balance hub-chequing=40000 --balance alpha-chequing=1000 --balance alpha-visa=1530.25
  has 'Each account sends at most $3,000 a day' 'hub-chequing → alpha-chequing: **$3,000**'
  has 'Pay the full current balance of alpha-visa from alpha-chequing: $1,530.25' "## Day 2" "| alpha-chequing |"
}

us24_error_unknown_account_and_withheld_without_consent() {
  person agent:strategy
  try transfers --balance nope=1; has "nope is not an account"
  grant alex
  ok transfers --balance alpha-chequing=1000
  has "Report saved for the owner only"; lacks alpha-chequing
}

run_test us24_happy_plans_quick_amounts_under_the_daily_limit
run_test us24_error_unknown_account_and_withheld_without_consent
