#!/usr/bin/env bash
# US-37 A second person, no MoneyWiz: a fresh HOME with no MoneyWiz app or
# database, a new user, and synthetic RBC chequing and Tangerine card
# downloads. import, spending, forecast, strategy and the weekly dry run all
# work and point at the bank downloads, never at MoneyWiz.
source "$(dirname "$0")/lib.sh"

rbc_chequing_csv() {
  # RBC "Download transactions" CSV: M/D/YYYY dates, CAD$ and USD$ columns.
  printf '"Account Type","Account Number","Transaction Date","Cheque Number","Description 1","Description 2","CAD$","USD$"\n'
  awk "$AWK_DATES"'
  BEGIN {
    s = dfc(2025, 10, 10); for (k = 0; k < 28; k++) row(s + 14 * k, "PAYROLL DEPOSIT", "EMPLOYER CANARY11", "2400.00")
    for (m = 0; m < 12; m++) {
      y = 2025 + int((m + 9) / 12); mo = (m + 9) % 12 + 1
      row(dfc(y, mo, 1), "RENT", "LANDLORD CANARY12", "-1600.00")
      row(dfc(y, mo, 4), "SAVE ON FOODS", "CANARY13", "-310.00")
      row(dfc(y, mo, 20), "TANGERINE MC PAYMENT", "CANARY14", "-450.00")
    }
  }
  function row(day, d1, d2, amt,   v) { v = cfd(day); split(v, p, "-")
    printf "\"Chequing\",\"00000-0000000\",\"%d/%d/%d\",\"\",\"%s\",\"%s\",%s,\n", p[2] + 0, p[3] + 0, p[1], d1, d2, amt }'
}
tangerine_card_csv() {
  # Tangerine credit card CSV: Transaction date, Transaction, Name, Memo, Amount (purchases negative).
  printf 'Transaction date,Transaction,Name,Memo,Amount\n'
  awk "$AWK_DATES"'
  BEGIN {
    for (m = 0; m < 12; m++) {
      y = 2025 + int((m + 9) / 12); mo = (m + 9) % 12 + 1
      row(dfc(y, mo, 6), "DEBIT", "COSTCO CANARY15", "-180.00")
      row(dfc(y, mo, 12), "DEBIT", "BC HYDRO CANARY16", "-90.00")
      row(dfc(y, mo, 15), "DEBIT", "NETFLIX CANARY17", "-20.99")
      row(dfc(y, mo, 21), "CREDIT", "PAYMENT CANARY18", "450.00")
    }
  }
  function row(day, t, n, amt,   v) { v = cfd(day); split(v, p, "-")
    printf "%d/%d/%d,%s,%s,Rewards earned: 0.00,%s\n", p[2] + 0, p[3] + 0, p[1], t, n, amt }'
}

us37_new_user_without_moneywiz_gets_a_forecast_and_a_plan() {
  # A second person's Mac: an empty HOME, so no MoneyWiz app and no MoneyWiz database.
  export HOME="$T/home"; mkdir -p "$HOME"
  ok users add sam --province BC
  has "sam"
  grant sam agent:aggregates agent:strategy
  local in; in="$(home sam)/inbox"
  rbc_chequing_csv > "$in/rbc-chequing.csv"
  tangerine_card_csv > "$in/tangerine-mastercard.csv"

  ok --user sam import --today 2026-10-18
  has "rbc-chequing.csv (rbc-csv)" "tangerine-mastercard.csv (CSV)"
  lacks "NOT READ" "MoneyWiz"
  inside "$OUT" "biweekly"

  ok --user sam spending --today 2026-10-18
  # The skill asks for the balance RBC downloads leave out, then writes the answer.
  local cfg; cfg="$(home sam)/cashcast.toml"
  if grep -q "^\[balances\]" "$cfg"; then replace_first "$cfg" $'[balances]\n' $'[balances]\n"rbc-chequing" = 3200.00\n'
  else printf '\n[balances]\n"rbc-chequing" = 3200.00\n' >> "$cfg"; fi
  ok --user sam forecast --today 2026-10-18
  has "Lowest month-end cash"
  ok --user sam strategy --today 2026-10-18
  try --user sam moneywiz status
  has "no MoneyWiz database found"
  ok --user sam weekly --dry-run --today 2026-10-18
  has "MoneyWiz is not on this computer, and cashcast works without it" "no cards in profile.toml yet"
  lacks "no MoneyWiz database found" "This step stopped"
  no_canary_on_disk sam
}

run_test us37_new_user_without_moneywiz_gets_a_forecast_and_a_plan
