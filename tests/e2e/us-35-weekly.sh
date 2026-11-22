#!/usr/bin/env bash
# US-35: the Sunday session in one command. `weekly` runs every check and the
# forecast, then lists overlaps (two tasks asking for the same work) and this
# week's moves, and saves out/weekly.html.
source "$(dirname "$0")/lib.sh"

PROFILE='
[[tasks]]
id = "pay-canary-a"
title = "Pay Canary card down to $400 (closes 25th)"
kind = "money"
due = 2026-11-23

[[tasks]]
id = "pay-canary-b-2026-11"
title = "Pay Canary Momentum Visa down to $400.00"
kind = "money"
due = 2026-11-23

[[tasks]]
id = "refresh-data"
title = "Refresh cashcast data and check the forecast"
kind = "read"
due = 2026-11-27

[[tasks]]
id = "call-bank"
title = "Call the bank about the fee"
kind = "settings"
due = 2026-11-14

[weekly]
notes = "Safe amount: chequing minus the floor."
'

person() {
  ok users add alex --province ON
  printf '%s' "$PROFILE" >> "$(home alex)/profile.toml"
  grant alex "$@"
}

us35_happy_one_run_checks_overlaps_and_moves() {
  person agent:aggregates agent:strategy
  ok weekly --dry-run --today 2026-11-17
  # Every step ran, each under its own heading, in order.
  has "# Weekly cashcast session, Tue Nov 17, 2026"
  has '## Forecast · `bin/cashcast forecast`'
  has "Skipped: transfers need today's balances"
  ok weekly --dry-run --today 2026-11-17 --balance nope=1
  has '## Transfers between your banks · `bin/cashcast transfers --balance nope=1`'
  # `cards now` gets card ids only: a bank balance must not stop that step.
  has '## Which card to use now · `bin/cashcast cards now`'
  lacks '`bin/cashcast cards now --balance nope=1`'
  lacks 'is not a card in [[wallet.cards]]'
  ok weekly --dry-run --today 2026-11-17
  has '## What to invest where'
  # optimize runs only once the forecast clears the cash floor, like invest-plan.
  lacks '## Where new money goes · `bin/cashcast optimize`'
  has '## Tasks cashcast closes itself · `bin/cashcast tasks`'
  has '## Reminders, Calendar and Notion · `bin/cashcast sync status`'
  # A week is done only when the data is current: the run names what is not.
  has '## Not up to date ('
  # The Sunday downloads need bank site access: the run says so before it lapses.
  has '## Bank site access'
  has 'Bank site access not granted: the Sunday downloads need it. Run in Terminal: bin/cashcast consent grant agent:browser-read'
  # The two pay-downs of one card are one overlap; the refresh is the weekly's own work.
  has "## Overlaps (2)"
  has '`pay-canary-a` repeats `pay-canary-b-2026-11`: the same card paid down to the same target on the same day'
  has '`refresh-data` repeats `bin/cashcast weekly`'
  has "## Money moves you make (1)"
  has '- Mon Nov 23: Pay Canary Momentum Visa down to $400.00 (`pay-canary-b-2026-11`)'
  has '- Sat Nov 14 (overdue): Call the bank about the fee (`call-bank`)'
  lacks '(`pay-canary-a`)'
  has "## Your weekly notes" "Safe amount: chequing minus the floor."
  has "Saved: " "weekly.html (2 overlap(s)"
  [ -s "$(home alex)/out/weekly.html" ] || fail "no weekly.html"
  [ "$(stat -c %a "$(home alex)/out/weekly.md" 2>/dev/null || stat -f %Lp "$(home alex)/out/weekly.md")" = 600 ] \
    || fail "weekly.md is not mode 600"
  event_any alex '.event == "weekly" and .overlaps == 2'
  no_canary_on_disk alex
}

us35_error_moves_withheld_without_consent() {
  person agent:aggregates
  ok weekly --dry-run --today 2026-11-17
  # The steps still ran; the moves name amounts, so they stay with the owner.
  has '## Forecast · `bin/cashcast forecast`'
  has "Report saved for the owner only"
  lacks "## Money moves you make"
}

us35_owner_terminal_sees_every_step() {
  # The owner runs it in a real terminal (a pty), with no agent grants: the
  # captured steps are still theirs, and the one-run token file is gone after.
  command -v script > /dev/null || skip "no script(1)"
  person
  local cmd=("$CLI" --user alex weekly --dry-run --today 2026-11-17)
  local clean=(env -u CASHCAST_AGENT -u CLAUDECODE -u CLAUDE_CODE_ENTRYPOINT -u CODEX_SANDBOX
    -u CODEX_MANAGED_BY_NPM -u CURSOR_AGENT -u CURSOR_TRACE_ID -u AIDER_MODEL -u GEMINI_CLI)
  if script --version > /dev/null 2>&1; then
    OUT="$("${clean[@]}" script -qec "${cmd[*]}" /dev/null < /dev/null)"  # util-linux
  else
    OUT="$("${clean[@]}" script -q /dev/null "${cmd[@]}" < /dev/null)"  # BSD
  fi
  canary_check weekly
  lacks "Report saved for the owner only"
  has "## Money moves you make (1)"
  [ ! -e "$PRIVATE/out/owner-run.token" ] || fail "the owner-run token file was left behind"
}

run_test us35_happy_one_run_checks_overlaps_and_moves
run_test us35_error_moves_withheld_without_consent
run_test us35_owner_terminal_sees_every_step
