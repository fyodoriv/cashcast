#!/usr/bin/env bash
# shellcheck disable=SC2034  # OUT and CODE are read by the lib.sh checks
# US-13: plan ahead in the local web app; only this computer, only with the token.
source "$(dirname "$0")/lib.sh"
need curl

start() { web_start || { [ $? -eq 2 ] && skip "this sandbox does not allow a local port"; }; }

us13_happy_web_app_serves_the_plan() {
  ok users add alex --province ON
  full_profile "$(home alex)/profile.toml"
  chequing_csv > "$(home alex)/inbox/bank.csv"
  grant alex agent:aggregates
  ok import --today 2026-10-18
  start
  web_get "/api/plan?user=alex" "$TOKEN"
  OUT="$BODY"; eq "$STATUS" 200
  jq -e '(.forecast | length > 0) and (.tasks | length > 0)' <<< "$BODY" > /dev/null || fail "no forecast or tasks in the plan"
  [[ "$BODY" != *CANARY* ]] || fail "the plan holds bank text"
  # The built app, or (in a checkout without web/dist) the 503 that says how to build it.
  web_get /
  [[ "$STATUS" == 200 && "$BODY" == *'<div id="root">'* || "$STATUS" == 503 && "$BODY" == *"web app not built"* ]] ||
    fail "no app page: $STATUS"
  stop_pid "$WEB_PID"
}

us13_error_no_token_no_data() {
  ok users add alex --province ON
  start
  web_get /api/users; eq "$STATUS" 401
  # A bad Content-Length is refused at once; it never makes the server wait for the body.
  local length
  for length in -1 abc 2000000; do
    eq "$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 -X POST -H "Content-Length: $length" \
      "http://127.0.0.1:$PORT/api/actions/review")" 400 "Content-Length $length"
  done
  stop_pid "$WEB_PID"
}

run_test us13_happy_web_app_serves_the_plan
run_test us13_error_no_token_no_data
