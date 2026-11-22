#!/usr/bin/env bash
# shellcheck disable=SC2034  # OUT and CODE are read by the lib.sh checks
# US-14: I record cash in MoneyWiz; one command adds the bank and card rows.
# A synthetic MoneyWiz store (sqlite3), a fake app and a fake writer stand in
# for MoneyWiz (see lib.sh).
source "$(dirname "$0")/lib.sh"
need sqlite3

setup() {
  ok users add alex --province ON
  H="$(home alex)"; DB="$H/MoneyWiz.sqlite"
  moneywiz_db "$DB"
  printf '%s' "$MONEYWIZ_EXPORT" > "$H/inbox/bank.csv"
  printf '[moneywiz]\ndb = "%s"\n' "$DB" > "$H/cashcast.toml"
  CASHCAST_MONEYWIZ_OPEN="$(fake_moneywiz "$H" "$DB")"
  export CASHCAST_MONEYWIZ_OPEN CASHCAST_MONEYWIZ_DELAY_MS=0 CASHCAST_MONEYWIZ_WAIT_S=10
}
db() { sqlite3 "$DB" "$1"; }

us14_happy_one_command_adds_bank_rows_and_keeps_cash() {
  setup
  grant alex agent:aggregates local:moneywiz
  ok moneywiz sync --dry-run --today 2026-10-01
  has "5 rows to add (4 links, 1 of them transfers), 1 amounts differ" "Dry run"
  has "Left alone (you record these by hand): Cash"

  # One command: missing rows, the wrong amount and the wrong opening balance.
  ok moneywiz sync --today 2026-10-01
  has "Sent 6 links to MoneyWiz." "MoneyWiz saved 6 of 6."
  private_only "$H/out/moneywiz-fixes.md"

  ok moneywiz sync --dry-run --today 2026-10-02
  has "0 rows to add (0 links, 0 of them transfers), 0 amounts differ"
  has "matches the bank (2,575.00)" "matches the bank (470.00)"

  ok moneywiz status; has "| Cash | Cash | CAD | 1 |" "none: recorded by hand"
  ok import --today 2026-10-01
  has "MoneyWiz database"  # cash recorded in MoneyWiz feeds the forecast
  event_any alex '.event == "moneywiz_sync" and .sent == 6'
  no_canary_on_disk alex
}

us14_error_sync_needs_the_owners_consent() {
  setup
  grant alex agent:aggregates
  try moneywiz sync --today 2026-10-01; code_not0; has "consent grant local:moneywiz"
  [ ! -e "$H/data/moneywiz-sent.json" ] || fail "something was sent"
  printf '[moneywiz]\ndb = "/nowhere/MoneyWiz.sqlite"\n' > "$H/cashcast.toml"
  try moneywiz status; has "does not exist"
}

# One command: collect new exports from Downloads, send, verify, then list rows with no category.
us14_happy_sync_collects_downloads_then_lists_category_fixes() {
  setup
  grant alex agent:aggregates local:moneywiz
  mv "$H/inbox/bank.csv" "$T/Downloads/bank.csv"
  ok moneywiz sync --today 2026-10-01
  has "Collected 1 export file(s) from Downloads (.csv ×1)." "Sent 6 links to MoneyWiz." "MoneyWiz saved 6 of 6."
  [ -z "$(ls -A "$T/Downloads")" ] && [ -f "$H/inbox/bank.csv" ] || fail "the export did not move to the inbox"
  [[ "$OUT" == *"Categories to fix:"* || "$OUT" == *"Categories: every synced row has one."* ]] || fail "no category line"
  grep -q "## No category yet" "$H/out/moneywiz-fixes.md" || fail "no category section in the fixes"
  event_any alex '.event == "fetch_collect" and .files == 1'
  no_canary_on_disk alex
}

us14_happy_list_mode_only_lists_wrong_amounts() {
  setup
  grant alex agent:aggregates local:moneywiz
  echo 'fixes = "list"' >> "$H/cashcast.toml"
  ok moneywiz sync --today 2026-10-01
  has "Sent 4 links to MoneyWiz." "MoneyWiz saved 4 of 4."
  ok moneywiz sync --dry-run --today 2026-10-02
  has "0 rows to add" "1 amounts differ" "out/moneywiz-fixes.md"
  has "MoneyWiz 1,580.00 vs bank 2,575.00 after sync"  # a wrong opening balance shows up
}

us14_happy_probe_names_the_ways_in_for_edits() {
  ok users add alex --province ON
  mkdir -p "$T/MoneyWiz.app/Contents"
  printf '%s' '<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleShortVersionString</key><string>2026.30.0</string><key>CFBundleURLTypes</key><array><dict><key>CFBundleURLSchemes</key><array><string>moneywiz</string></array></dict></array></dict></plist>' \
    > "$T/MoneyWiz.app/Contents/Info.plist"
  export CASHCAST_MONEYWIZ_APP="$T/MoneyWiz.app"
  ok moneywiz probe
  has "URL schemes: moneywiz" "| 1. AppleScript dictionary | no" "moneywiz probe --ui"
  event_any alex '.event == "moneywiz_probe" and .applescript == false'
}

us14_error_probe_without_the_app() {
  ok users add alex --province ON
  export CASHCAST_MONEYWIZ_APP="$T/missing.app"
  try moneywiz probe; code_not0; has "not in /Applications"
}

EDITS='
[[category]]
add = "Food/Coffee"

[[category]]
rename = "Food/Groceries"
to = "Grocery stores"

[[category]]
delete = "Old"
move_to = "Food/Coffee"

[[row]]
account = "Chequing"
date = "2026-09-02"
amount = -40.00
set_category = "Food/Grocery stores"
set_memo = "checked"
'

edit_setup() {
  setup
  LOG="$H/writer.log"
  CASHCAST_MONEYWIZ_WRITER="$(fake_writer "$H")"
  export CASHCAST_MONEYWIZ_WRITER CASHCAST_MONEYWIZ_MODEL="$H/Model.momd"
}

categories_setup() {
  edit_setup
  printf 'writer = "core-data"\nwriter_checksum = "sum1"\n' >> "$H/cashcast.toml"
  replace_first "$H/profile.toml" "[budget]" $'[budget]\n"Food > Coffee" = 50'
}

names() { db "SELECT ZNAME2 FROM ZSYNCOBJECT WHERE Z_ENT=20 ORDER BY ZNAME2"; }

us14_happy_categories_suggest_migrate_and_roll_back() {
  categories_setup
  grant alex agent:aggregates local:moneywiz
  ok moneywiz categories
  has "3 categories (3 expense, 0 income)" "| Old | expense | 1 | 0 | 0 |" "Migrations: 0 applied, 0 pending."

  ok moneywiz categories suggest
  has 'Your budget or card strategy uses "Food/Coffee"' '"Food/Groceries" has no rows.'
  has "Wrote inbox/moneywiz-migrations/0001-suggested.toml (2 changes)"
  [ ! -e "$LOG" ] || fail "suggestions wrote MoneyWiz"

  ok moneywiz categories migrate --dry-run
  has "Migration 0001-suggested.toml" "1 add, 0 rename, 0 move, 1 delete" "Rollback computed and checked: 2 steps."
  [ ! -e "$LOG" ] || fail "a dry run called the writer"

  ok moneywiz categories migrate --yes
  has "Test on a copy: Verified 3 of 3 changes" "Backup of the database" "Recorded 0001-suggested.toml as applied."
  eq "$(names)" $'Coffee\nFood\nOld'
  private_only "$H/data/moneywiz-migrations.json"
  ok moneywiz categories migrate --yes; has "No pending migrations."
  event_any alex '.event == "moneywiz_migrate" and .result == "ok"'

  # The owner adds a category in MoneyWiz: the rollback sees the drift and stops.
  db "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZGID,ZTYPE2) VALUES (60,20,'Gifts','g60',1)"
  try moneywiz categories rollback --yes; code_not0
  has "changed since this migration was written" "Last resort: restore the backup"
  db "DELETE FROM ZSYNCOBJECT WHERE Z_PK=60"

  ok moneywiz categories rollback --yes
  has "Rollback of 0001-suggested.toml" "Rolled back 0001-suggested.toml."; lacks CANARYMW
  eq "$(names)" $'Food\nGroceries\nOld'
  ok moneywiz categories; has "Migrations: 0 applied, 1 pending"
  no_canary_on_disk alex
}

us14_error_categories_migrate_needs_consent_and_writes_nothing() {
  categories_setup
  grant alex agent:aggregates
  ok moneywiz categories suggest
  try moneywiz categories migrate --yes; code_not0; has "consent grant local:moneywiz"
  [ ! -e "$LOG" ] && [ ! -e "$H/data/moneywiz-migrations.json" ] || fail "something was written"
  ok moneywiz categories rollback; has "No applied migrations to roll back."
}

us14_happy_edit_changes_rows_and_categories_in_place() {
  edit_setup
  grant alex agent:aggregates local:moneywiz
  printf '%s' "$EDITS" > "$H/inbox/moneywiz-edits.toml"

  ok moneywiz edit --dry-run
  has "1 row changes: 0 amounts, 1 categories, 0 payees, 1 memos" "3 category changes: 1 add, 1 rename, 1 delete"
  lacks CANARYMW
  [ ! -e "$LOG" ] || fail "a dry run called the writer"

  try moneywiz edit; code_not0; has 'writer = "core-data"'

  echo 'writer = "core-data"' >> "$H/cashcast.toml"
  try moneywiz edit
  has "Test on a copy: Verified 4 of 4 changes" 'writer_checksum = "sum1"'; lacks CANARYMW
  eq "$(db "SELECT COUNT(*) FROM ZSYNCOBJECT WHERE ZNAME2='Coffee'")" 0 "live store untouched"
  grep -q -- --allow-running "$LOG" || fail "the copy test may run with MoneyWiz open"

  echo 'writer_checksum = "sum1"' >> "$H/cashcast.toml"
  ok moneywiz edit
  has "Verified 4 of 4 changes in MoneyWiz.sqlite" "Backup of the database"; lacks CANARYMW
  local live; live="$(grep '^apply' "$LOG" | grep MoneyWiz.sqlite | tail -n1)"
  [ -n "$live" ] && [[ "$live" != *--allow-running* ]] || fail "the live write needs MoneyWiz closed"
  eq "$(names)" $'Coffee\nFood\nGrocery stores'
  eq "$(db "SELECT ZNOTES1 FROM ZSYNCOBJECT WHERE ZGID='g101'")" checked
  local backups; backups=("$H/data/moneywiz-backups"/*)
  [ "${#backups[@]}" -eq 1 ] && [ -f "${backups[0]}/MoneyWiz.sqlite" ] || fail "not one backup"
  event_any alex '.event == "moneywiz_edit" and .changed == 4'
  no_canary_on_disk alex
}

us14_happy_fixes_edit_mode_edits_wrong_amounts_in_place() {
  edit_setup
  grant alex agent:aggregates local:moneywiz
  printf 'fixes = "edit"\nwriter = "core-data"\nwriter_checksum = "sum1"\n' >> "$H/cashcast.toml"
  ok moneywiz sync --today 2026-10-01
  has "Sent 4 links to MoneyWiz."  # adds still go in as links; no correction rows
  has "1 row changes: 1 amounts" "1 opening balances" "Verified 2 of 2 changes"
  ok moneywiz sync --dry-run --today 2026-10-02
  has "0 rows to add (0 links, 0 of them transfers), 0 amounts differ" "matches the bank (2,575.00)"
  eq "$(db "SELECT COUNT(*) FROM ZSYNCOBJECT WHERE ZNOTES1 LIKE 'cashcast%'")" 0
}

us14_error_edit_names_problems_by_number_and_writes_nothing() {
  edit_setup
  grant alex agent:aggregates
  printf '[[row]]\naccount = "Chequing"\ndate = "2026-09-09"\namount = -1.0\nset_memo = "x"\n[[category]]\ndelete = "Food"\n[[category]]\nrename = "Nope"\nto = "X"\n' \
    > "$H/inbox/moneywiz-edits.toml"
  ok moneywiz edit
  has "row #1: no MoneyWiz row in Chequing on that date with that amount"
  has "category #1: delete or move its subcategories first" "category #2: no such category" "Nothing to change."
  printf '[[category]]\nadd = "Food/Tea"\n' > "$H/inbox/moneywiz-edits.toml"
  try moneywiz edit; code_not0; has "consent grant local:moneywiz"
  [ ! -e "$LOG" ] || fail "something was written"
}

us14_happy_a_row_deleted_in_moneywiz_is_not_added_back() {
  setup
  grant alex agent:aggregates local:moneywiz
  # A MoneyWiz CSV export in the inbox holds MoneyWiz's own rows: never sent back.
  printf '%s\n' 'sep=,' \
    '"Name","Current balance","Account","Transfers","Description","Payee","Category","Date","Time","Memo","Amount","Currency","Check #","Tags"' \
    '"","","Chequing","","OLD CANARYCSV1","","","2026-09-20","09:00","","-99.00","CAD","",""' > "$H/inbox/moneywiz.csv"
  ok moneywiz sync --today 2026-10-01
  has "MoneyWiz saved 6 of 6." "Not compared: 1 MoneyWiz export(s)"
  private_only "$H/data/moneywiz-sent.json"
  private_only "$H/data/moneywiz-key"

  # The owner deletes one added row in MoneyWiz.
  db "DELETE FROM ZSYNCOBJECT WHERE ZDESC2 LIKE 'PHONE CO%'"
  ok moneywiz sync --today 2026-10-02
  has "1 links were sent on an earlier day" "Sent 0 links"
  ok moneywiz sync --resend --today 2026-10-02; has "Sent 1 links to MoneyWiz."

  ok moneywiz coverage --today 2026-10-02; has "# MoneyWiz coverage" "| Chequing | BankCheque |"
  event_any alex '.event == "moneywiz_coverage" and (.rows // 0) > 0'
  no_canary_on_disk alex
}

us14_happy_weekly_sync_keeps_a_ledger_and_checks_rows_again() {
  setup
  grant alex agent:aggregates local:moneywiz
  ok moneywiz sync --dry-run --today 2026-10-01
  has "Ledger: 7 bank rows tracked"
  private_only "$H/data/moneywiz-ledger.json"
  ok moneywiz sync --dry-run --today 2026-10-01; has "Cached: the exports, MoneyWiz and the settings are unchanged"
  ok moneywiz weekly --today 2026-10-01
  has "Sent 6 links to MoneyWiz." "MoneyWiz saved 6 of 6."
  # The owner deletes one added row; resync sends it again, in that range only.
  db "DELETE FROM ZSYNCOBJECT WHERE ZDESC2 LIKE 'PHONE CO%'"
  ok moneywiz sync --today 2026-10-02; has "Sent 0 links"
  ok moneywiz resync --account Chequing --from 2026-09-01 --to 2026-09-10
  has "The next sync checks Chequing from 2026-09-01 to 2026-09-10 again"
  ok moneywiz sync --today 2026-10-03; has "Sent 1 links to MoneyWiz."
  ok moneywiz diagnose; has "# MoneyWiz diagnose" "| Chequing |"; lacks CANARY
  ok moneywiz categorize --dry-run; has "# MoneyWiz categorize" "Dry run."; lacks CANARY
  no_canary_on_disk alex
}

us14_happy_web_view_picks_changes_and_applies_only_with_yes() {
  categories_setup
  grant alex agent:aggregates local:moneywiz
  ok moneywiz categories new --name web '--ops=[{"add": "Food/Coffee"}, {"delete": "Old", "move_to": "Food"}]'
  has "0001-web.toml (2 changes)"
  try moneywiz categories migrate; code_not0; has "run it again with --yes"
  [ ! -e "$LOG" ] || fail "migrate without --yes called the writer"
  ok moneywiz categories migrate --yes
  has "Backup of the database" "Recorded 0001-web.toml as applied."; lacks CANARYMW
  eq "$(names)" $'Coffee\nFood\nGroceries'
  # The web app's action path refuses a rollback with yes unless the owner confirms.
  web_start || { [ $? -eq 2 ] && skip "this sandbox does not allow a local port"; }
  local code body="$T/action.json"
  code="$(curl -s -o "$body" -w '%{http_code}' --max-time 60 -X POST -H "X-Cashcast-Token: $TOKEN" \
    -H "Content-Type: application/json" --data '{"values": {"action": "categories", "step": "rollback", "yes": true}}' \
    "http://127.0.0.1:$PORT/api/actions/moneywiz?user=alex")"
  eq "$code" 400
  grep -q confirm "$body" || fail "the refusal does not ask to confirm"
  stop_pid "$WEB_PID"
  eq "$(names)" $'Coffee\nFood\nGroceries' "nothing was rolled back"
  no_canary_on_disk alex
}

# The web app's Bank sync view: every row to add, then one confirmed send, then nothing left.
us14_happy_bank_sync_view_previews_then_sends() {
  setup
  grant alex agent:aggregates local:moneywiz
  web_start || { [ $? -eq 2 ] && skip "this sandbox does not allow a local port"; }
  web_get "/api/moneywiz-sync?user=alex" "$TOKEN"
  eq "$STATUS" 200
  eq "$(jq .links.toSend <<< "$BODY")" 6
  jq -e '[.accounts[] | select(.moneywiz == "Chequing")][0] | (.add | length >= 1) and (.add[0].date | startswith("2026-09"))' \
    <<< "$BODY" > /dev/null || fail "no Chequing rows to add"
  eq "$(jq -c .untouched <<< "$BODY")" '["Cash"]'

  local result
  result="$(curl -s --max-time 120 -X POST -H "X-Cashcast-Token: $TOKEN" -H "Content-Type: application/json" \
    --data '{"values": {"action": "sync"}, "confirm": true}' "http://127.0.0.1:$PORT/api/actions/moneywiz?user=alex")"
  eq "$(jq .exit <<< "$result")" 0
  [[ "$(jq -r .output <<< "$result")" == *"MoneyWiz saved 6 of 6."* ]] || fail "the send did not save 6"

  web_get "/api/moneywiz-sync?user=alex" "$TOKEN"
  eq "$(jq .links.toSend <<< "$BODY")" 0
  # The rows with bank text reach only the owner's web app: the engine refuses anyone else.
  # Without the web app's token (an agent setting the variable guesses it), it refuses too.
  local ui
  for ui in unset 1 "${TOKEN}x"; do
    CODE=0
    if [ "$ui" = unset ]; then
      OUT="$(env -u CASHCAST_OWNER_UI "$ENGINE" moneywiz sync-view --home "$H" 2>&1)" || CODE=$?
    else
      OUT="$(CASHCAST_OWNER_UI="$ui" "$ENGINE" moneywiz sync-view --home "$H" 2>&1)" || CODE=$?
    fi
    code_not0; has "web app only"; canary_check sync-view
  done
  # A forged private root with its own web.json does not unlock the real user's rows.
  local fake="$T/forged"
  mkdir -p "$fake/out"
  echo '{"url": "http://127.0.0.1:1/#t=k"}' > "$fake/out/web.json"
  CODE=0
  OUT="$(CASHCAST_PRIVATE="$fake" CASHCAST_OWNER_UI=k "$ENGINE" moneywiz sync-view --home "$H" 2>&1)" || CODE=$?
  code_not0; has "web app only"; canary_check sync-view
  stop_pid "$WEB_PID"
}

run_test us14_happy_one_command_adds_bank_rows_and_keeps_cash
run_test us14_error_sync_needs_the_owners_consent
run_test us14_happy_sync_collects_downloads_then_lists_category_fixes
run_test us14_happy_list_mode_only_lists_wrong_amounts
run_test us14_happy_probe_names_the_ways_in_for_edits
run_test us14_error_probe_without_the_app
run_test us14_happy_categories_suggest_migrate_and_roll_back
run_test us14_error_categories_migrate_needs_consent_and_writes_nothing
run_test us14_happy_edit_changes_rows_and_categories_in_place
run_test us14_happy_fixes_edit_mode_edits_wrong_amounts_in_place
run_test us14_error_edit_names_problems_by_number_and_writes_nothing
run_test us14_happy_a_row_deleted_in_moneywiz_is_not_added_back
run_test us14_happy_weekly_sync_keeps_a_ledger_and_checks_rows_again
run_test us14_happy_web_view_picks_changes_and_applies_only_with_yes
run_test us14_happy_bank_sync_view_previews_then_sends
