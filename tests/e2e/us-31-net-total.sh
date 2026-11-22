#!/usr/bin/env bash
# US-31: the net total, the main number: all you have minus all you owe, its
# trend, and options checked against your boundaries. From real CLI runs.
source "$(dirname "$0")/lib.sh"

IDEAS='
[boundaries]
work_hours_week = 40
max_work_hours_week = 45

[[net_total.ideas]]
id = "weekend-gig"
title = "Weekend gig"
effect_year = 9000
work_hours_week = 10

[[net_total.ideas]]
id = "sell-old-bike"
title = "Sell the old bike"
effect_year = 300
'

with_balances() {
  echo '{"kraken": {"balances": {"BTC": 0.1}, "prices": {"BTC": {"price": 100000, "currency": "CAD"}}, "fetched_on": "2026-10-17"}}' \
    > "$(home alex)/data/api-balances.json"
}

us31_happy_net_total_ranks_options_and_refuses_burnout() {
  setup_person
  with_balances
  printf '%s' "$IDEAS" >> "$(home alex)/profile.toml"
  ok net-total --today 2026-10-18
  local stripped="${OUT#"${OUT%%[![:space:]]*}"}"
  [[ "$stripped" == '## Net total: $'* ]] || fail "the output does not open with the net total"
  has '| Crypto | $10,000 |' "Sell the old bike" "~~Weekend gig~~" "no burnout" "Trend: first point saved today"
  # A second day adds a trend point.
  ok net-total --today 2026-10-19
  eq "$(jq -c '[.[].date]' "$(home alex)/data/net-total-history.json")" '["2026-10-18","2026-10-19"]'
  no_canary_on_disk alex
}

us31_error_bad_margin_and_withheld_without_consent() {
  setup_person
  printf '\n[boundaries]\nmargin = 50\n' >> "$(home alex)/profile.toml"
  ok net-total --today 2026-10-18; has "0 to 20" "[boundaries]"
  grant alex
  ok net-total --today 2026-10-18; has "Report saved for the owner only"; lacks 'Net total: $'
}

us31_start_prints_the_net_total_before_the_tasks() {
  setup_person
  ok config set web.autostart false
  ok; has 'Net total: $' "Higher is better" "Needed from you:"
  local before_total="${OUT%%"Net total: \$"*}" before_needed="${OUT%%"Needed from you:"*}"
  [ "${#before_total}" -lt "${#before_needed}" ] || fail "the net total is not before the tasks"
  ok config set start.net_total false
  ok; lacks 'Net total: $'
}

run_test us31_happy_net_total_ranks_options_and_refuses_burnout
run_test us31_error_bad_margin_and_withheld_without_consent
run_test us31_start_prints_the_net_total_before_the_tasks
