#!/usr/bin/env bash
# US-27: balances read in the browser are recorded and reach the strategy, from real CLI runs.
source "$(dirname "$0")/lib.sh"

us27_happy_recorded_wealthsimple_and_casa_reach_the_strategy() {
  setup_person
  ok api record wealthsimple --set TFSA=1000,RRSP=2000
  has "Recorded 2 balance(s) for wealthsimple"; lacks 1000
  ok api record casa --set BTC=0.1
  ok strategy --today 2026-10-18
  has "Market value of your accounts" '| Wealthsimple TFSA | TFSA | $1,000.00 |'
  has "Casa BTC" "no price"  # no Kraken price saved yet
  no_canary_on_disk alex
}

us27_error_an_account_number_is_refused() {
  setup_person
  try api record wealthsimple --set "TFSA 12345678=10"; has "4 or more digits"
}

run_test us27_happy_recorded_wealthsimple_and_casa_reach_the_strategy
run_test us27_error_an_account_number_is_refused
