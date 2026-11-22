#!/usr/bin/env bash
# US-29: which card to use right now, from actual numbers: cap used since each
# card's reset, cap left, rent charges counted, credit left. `cards now`.
source "$(dirname "$0")/lib.sh"

RENT='[rent]
monthly = 1000
fee_rate = 0.02
codes_as = "recurring"
'

PROFILE='
[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Visa"]
cap_reset = "10-26"
rent_max = 2
# Scotia before Oct 22, 2026: no rent rate, so rent codes as recurring at 4%.
earn = { groceries = 0.04, recurring = 0.04, streaming = 0.04, gas = 0.02, ev_charging = 0.02, transit = 0.02, rideshare = 0.02, delivery = 0.02, other = 0.01 }
limit = 9000
target = 150
statement_close_day = 25

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["Beta Visa"]
cap_reset = "02-01"
rent_max = 4
limit = 9000

[[wallet.cards]]
id = "cibc-dividend-visa-infinite"
accounts = ["Gamma Visa"]
cap_reset = "01-01"

[[tasks]]
id = "downgrade-cibc-dividend-visa-infinite"
title = "Product-change the CIBC card"
due = 2026-10-26
'

HEADER='"Name","Current balance","Account","Transfers","Description","Payee","Category","Date","Time","Memo","Amount","Currency","Check #","Tags"'

# row ACCOUNT N CATEGORY DAY AMOUNT: one MoneyWiz export row with canary text.
row() { printf '"","","%s","","CANARY_D%s","CANARY_P%s","%s","%s","09:00","CANARY_M%s","%s","CAD","",""\n' "$1" "$2" "$2" "$3" "$4" "$2" "$5"; }

export_csv() {
  printf 'sep=,\n%s\n' "$HEADER"
  row "Alpha Visa" 1 "Housing ► Rent" 2026-10-27 "-1,020.00"
  row "Alpha Visa" 2 "Housing ► Rent" 2026-10-29 "-1,020.00"
  row "Alpha Visa" 3 "Food & Dining ► Groceries" 2026-10-28 "-24,000.00"
  row "Alpha Visa" 4 "Food & Dining ► Groceries" 2026-10-30 "40.00"
  row "Alpha Visa" 5 "Food & Dining ► Groceries" 2026-09-01 "-500.00"
  row "Beta Visa" 6 "Food & Dining ► Groceries" 2026-03-02 "-200.00"
  row "Gamma Visa" 7 "Clothing ► Clothes" 2026-10-02 "-90.00"
}

person() {
  ok users add alex --province ON
  local profile; profile="$(home alex)/profile.toml"
  replace_first "$profile" $'[rent]\n' "$RENT"
  printf '%s' "$PROFILE" >> "$profile"
  export_csv > "$(home alex)/inbox/cards.csv"
  grant alex "$@"
}

us29_happy_routes_each_category_by_cap_left_rent_count_and_credit() {
  person agent:strategy
  # A balance recorded earlier under the card's id is used when none is given.
  mkdir -p "$(home alex)/data"
  echo '{"td": {"balances": {"td-cash-back-visa-infinite": 8950}}}' > "$(home alex)/data/api-balances.json"
  ok cards now --balance scotia-momentum-visa-infinite=400 --today 2026-11-01
  # Scotia's 4 % group: rent (coded as recurring) + groceries − the refund since Oct 26.
  has "| Scotia Momentum Visa Infinite | groceries, recurring, streaming | 26,000.00 of 25,000.00 | 0.00 | 2026-10-26 |"
  # Cap used up and the rent rule's 2 charges used: rent goes on to TD, then Wealthsimple is not held.
  has "- Scotia Momentum Visa Infinite: 2 of 2 charges since 2026-10-26 · 0 left"
  has "| groceries | **CIBC Dividend Visa Infinite** | 4% |"
  # TD has 50.00 of credit left: too little for a rent charge.
  has "TD Cash Back Visa Infinite: credit left 50.00 is too little"
  has "| Scotia Momentum Visa Infinite | 400.00 | 9,000.00 | 8,600.00 | 150.00 | Wed Nov 25 |"
  has "above its target of 150.00"
  has 'product change is pending (task `downgrade-cibc-dividend-visa-infinite`)'
  no_canary_on_disk alex
}

us29_error_unknown_card_and_withheld_without_consent() {
  person agent:strategy
  try cards now --balance nope=1; has "nope is not a card in [[wallet.cards]]"
  grant alex
  ok cards now --today 2026-11-01
  has "Report saved for the owner only"; lacks "Scotia Momentum"
}

SHARED_CAP='
[rent]
monthly = 3500
service = "chexy"
fee_rate = 0.0175
codes_as = "recurring"

[spending]
groceries = 700
recurring = 100
streaming = 20

[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Visa"]
cap_reset = "11-01"
rent_max = 7
# Scotia before Oct 22, 2026: no rent rate, so rent codes as recurring at 4%.
earn = { groceries = 0.04, recurring = 0.04, streaming = 0.04, gas = 0.02, ev_charging = 0.02, transit = 0.02, rideshare = 0.02, delivery = 0.02, other = 0.01 }

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["Beta Visa"]
cap_reset = "02-01"
rent_max = 4

[[wallet.cards]]
id = "wealthsimple-visa-infinite-plus"
'

us29_edge_cards_now_and_the_strategy_save_a_shared_cap_for_rent() {
  ok users add alex --province ON
  local profile cat now plan l
  profile="$(home alex)/profile.toml"
  replace_first "$profile" $'[rent]\n' $'[old_rent]\n'
  printf '%s' "$SHARED_CAP" >> "$profile"
  { printf 'sep=,\n%s\n' "$HEADER"; row "Beta Visa" 1 "Food & Dining ► Groceries" 2026-10-02 "-30.00"; } > "$(home alex)/inbox/cards.csv"
  grant alex agent:aggregates agent:strategy
  ok cards now --today 2026-11-01; now="$OUT"
  ok strategy --today 2026-11-01; plan="$OUT"
  for cat in groceries recurring streaming; do
    l="$(line_starting "$now" "| $cat |")"
    inside "$l" "**TD Cash Back Visa Infinite**"
    inside "$l" "cap saved for rent (7 charges left)"
    l="$(line_starting "$plan" "| $cat |")"
    inside "$l" "TD Cash Back Visa Infinite"
    [[ "$l" != *Scotia* ]] || fail "$cat goes on Scotia in the strategy"
  done
  inside "$(line_starting "$now" "| rent |")" "**Scotia Momentum Visa Infinite**"
  inside "$plan" "Scotia Momentum Visa Infinite × 7 → TD Cash Back Visa Infinite × 4"
}

RATE_DROP='
[rent]
monthly = 1000
fee_rate = 0.015
codes_as = "recurring"

[[wallet.cards]]
id = "scotia-momentum-visa-infinite"
accounts = ["Alpha Visa"]
cap_reset = "11-27"

[[wallet.cards]]
id = "td-cash-back-visa-infinite"
accounts = ["Beta Visa"]
cap_reset = "02-01"
'

# Scotia's catalog rent rate is 1% from 2026-10-22 (`rent_rate_from`): rent
# posted before that date still filled its 4% cap; rent now goes to TD.
us29_happy_rent_before_a_rent_rate_date_still_uses_the_cap() {
  ok users add alex --province ON
  local profile; profile="$(home alex)/profile.toml"
  replace_first "$profile" $'[rent]\n' $'[old_rent]\n'
  printf '%s' "$RATE_DROP" >> "$profile"
  { printf 'sep=,\n%s\n' "$HEADER"
    row "Alpha Visa" 1 "Housing ► Rent" 2026-09-25 "-24,900.00"
    row "Alpha Visa" 2 "Housing ► Rent" 2026-10-30 "-1,000.00"; } > "$(home alex)/inbox/cards.csv"
  grant alex agent:strategy
  ok cards now --today 2026-11-15
  has "| Scotia Momentum Visa Infinite | groceries, recurring, streaming | 24,900.00 of 25,000.00 | 100.00 | 2025-11-27 |"
  inside "$(line_starting "$OUT" "| rent |")" "**TD Cash Back Visa Infinite**"
  no_canary_on_disk alex
}

run_test us29_happy_routes_each_category_by_cap_left_rent_count_and_credit
run_test us29_happy_rent_before_a_rent_rate_date_still_uses_the_cap
run_test us29_error_unknown_card_and_withheld_without_consent
run_test us29_edge_cards_now_and_the_strategy_save_a_shared_cap_for_rent
