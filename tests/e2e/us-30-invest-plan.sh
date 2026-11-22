#!/usr/bin/env bash
# US-30: ranked investment moves with evidence, do-nothing and overrides, from real CLI runs.
source "$(dirname "$0")/lib.sh"

us30_happy_ranked_moves_with_evidence() {
  setup_person
  echo '{"etrade": {"balances": {"Stock Plan (CAD)": 50000.0}, "fetched_on": "2026-10-17", "source": "browser"}}' \
    > "$(home alex)/data/api-balances.json"
  ok invest-plan --today 2026-10-18
  has "Investment moves: ranked, with evidence" "not a licensed financial advisor" "not a forecast"
  has '`do-nothing`' "concentration:EMPL"
  lacks "missing from knowledge"  # every cited fact exists
  no_canary_on_disk alex
}

us30_override_is_respected() {
  setup_person
  echo '{"etrade": {"balances": {"Stock Plan (CAD)": 50000.0}, "fetched_on": "2026-10-17"}}' > "$(home alex)/data/api-balances.json"
  printf '\n[[invest.overrides]]\nmove = "concentration:EMPL"\nreason = "vesting cliff next month"\n' >> "$(home alex)/profile.toml"
  ok invest-plan --today 2026-10-18
  has "## Your choices" "vesting cliff next month"; lacks "Risk moves"
}

us30_withheld_without_consent() {
  ok users add alex --province ON
  ok invest-plan --today 2026-10-18
  has "Report saved for the owner only" "consent grant agent:strategy"
}

run_test us30_happy_ranked_moves_with_evidence
run_test us30_override_is_respected
run_test us30_withheld_without_consent
