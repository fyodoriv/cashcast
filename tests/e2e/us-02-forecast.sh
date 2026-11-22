#!/usr/bin/env bash
# US-02 import, US-03 questions, US-04 forecast and workbook: synthetic exports
# through bin/cashcast init, import, status, forecast and backtest. Every
# description carries a canary that must never be printed.
source "$(dirname "$0")/lib.sh"
need unzip

us02_csv_exports_become_a_verified_forecast() {
  # The single-user layout: CASHCAST_HOME is the data folder itself.
  unset CASHCAST_PRIVATE
  export CASHCAST_HOME="$T/home"
  local h="$CASHCAST_HOME"
  ok init
  eq "$(mode "$h")" 700
  # The owner lets the agent read reports, by hand (see docs/privacy.md); without it they are withheld.
  printf '[[grants]]\nscope = "agent:aggregates"\ngranted_on = 2026-10-02\nexpires_on = 2099-01-01\nvia = "hand"\n' > "$h/consent.toml"
  forecast_chequing_csv > "$h/inbox/chequing.csv"
  forecast_visa_csv > "$h/inbox/visa.csv"

  ok import --today 2026-10-02
  has "Schedule: biweekly" "signs flipped" "chequing | cash (guess)"

  ok status --today 2026-10-02
  has "Ready: the forecast can run now" "floor:" "accounts: confirm account types"

  # Answer like the skill does: add keys under the existing headers.
  replace_first "$h/cashcast.toml" $'[accounts]\n' $'[accounts]\n"chequing" = "cash"\n"visa" = "card"\n'
  replace_first "$h/cashcast.toml" $'[forecast]\n' $'[forecast]\nfloor = 5000\n'
  ok status --today 2026-10-02
  lacks "accounts: confirm" "floor:"  # answers count without a manual re-import

  ok forecast --today 2026-10-02
  has "Lowest month-end cash" "Cash floor: 5,000.00" "formulas match the Python model to the cent"
  local result="$OUT" xlsx="$h/out/Cashcast.xlsx"
  eq "$(mode "$xlsx")" 600
  # Same inputs, same bytes: the workbook carries no build time (the parity gate compares sizes).
  [[ "$(unzip -p "$xlsx" docProps/core.xml)" != *"$(date +%Y-%m-%d)T"* ]] || fail "the workbook holds today's time"
  eq "$(mode "$h/data/review.md")" 600
  # Each forecast run saves its monthly lines; `backtest` scores them once later months are imported.
  eq "$(mode "$h/data/forecasts/2026-10-02.json")" 600
  inside "$result" "Saved this forecast for later scoring"
  ok backtest --today 2026-10-02
  has "No saved forecast covers an imported month yet" "## Backtest: 24 origins" "Mean miss by calendar month"
}

run_test us02_csv_exports_become_a_verified_forecast
