#!/usr/bin/env bash
# US-36: track my citizenship paperwork privately: the public steps for anyone,
# the private values sealed on this Mac, and nothing through the web app.
source "$(dirname "$0")/lib.sh"

key() { export CASHCAST_TEST_VAULT_KEY=0101010101010101010101010101010101010101010101010101010101010101; }

us36_happy_steps_doctor_and_the_application_status() {
  key
  ok users add alex --province ON
  ok --user alex paperwork steps
  has "Canadian citizenship (adult)" "presence:"
  ok --user alex paperwork doctor
  has "test key (CASHCAST_TEST_VAULT_KEY)" "0 encrypted files"
  ok --user alex citizenship status
  has "uci"
  ok actions --json
  has '"paperwork"' "run \`bin/cashcast paperwork\` in Terminal on this Mac"
}

us36_error_agents_cannot_set_private_values() {
  key
  ok users add alex --province ON
  try --user alex paperwork set ca-citizenship-adult pr-date
  [ "$CODE" -eq 2 ] || fail "set without a terminal exit $CODE, want 2"
  [ ! -e "$(home alex)/paperwork/state.sqv" ] || fail "a value was saved without the owner"
}

run_test us36_happy_steps_doctor_and_the_application_status
run_test us36_error_agents_cannot_set_private_values
