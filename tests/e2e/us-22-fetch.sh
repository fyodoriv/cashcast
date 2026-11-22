#!/usr/bin/env bash
# US-22: get fresh exports either way, by hand or fetched by an agent after one
# approval. The browser part runs in the agent (skills/fetch-exports); here the
# CLI paths it relies on: `fetch plan` and `fetch collect`.
source "$(dirname "$0")/lib.sh"

person() {
  ok users add alex --province ON
  grant alex agent:aggregates
}

us22_happy_downloads_reach_the_inbox_unopened_and_import() {
  person
  local downloads="$T/Downloads"
  mkdir -p "$downloads"
  ok config set fetch.downloads "$downloads"
  chequing_csv > "$downloads/acct-9876.csv"
  ok fetch collect
  has "Moved 1 file(s) (.csv ×1) into your inbox, unopened"; lacks 9876
  [ -z "$(ls -A "$downloads")" ] || fail "Downloads is not empty"
  ok import
  ok fetch plan --today 2026-10-25
  has "1 inbox file(s) use a format several services share" "fetch plan --all"
  event_any alex '.event == "fetch_collect" and .files == 1'
  no_canary_on_disk alex
}

us22_error_nothing_to_collect_says_so_and_plan_lists_every_service() {
  person
  ok config set fetch.downloads "$T/empty"
  ok fetch collect; has "No new export files in Downloads"
  ok fetch plan; has "No exports in the inbox yet"
  ok fetch plan --all --today 2026-10-25
  has "td (TD Canada Trust): stale, no export yet. browser: https://easyweb.td.com/"
  has "wise (Wise): stale, no export yet. API: bin/cashcast api download wise"
}

qfx() {
  printf 'OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>\n<BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>CAD\n'
  printf '<BANKACCTFROM><BANKID>9<ACCTID>00011122%s<ACCTTYPE>CHECKING</BANKACCTFROM>\n<BANKTRANLIST>\n' "$1"
  printf '<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>%s<TRNAMT>-42.10<FITID>%s<NAME>GROCER CANARY31</STMTTRN>\n' "$2" "$3"
  printf '</BANKTRANLIST><LEDGERBAL><BALAMT>100.00<DTASOF>%s</LEDGERBAL></STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n' "$2"
}

us22_happy_bank_named_qfx_and_ledger_files_count_for_their_service() {
  person
  local downloads="$T/Downloads"
  mkdir -p "$downloads"
  ok config set fetch.downloads "$downloads"
  qfx 9876 20261020 A1 > "$downloads/cibc.qfx"
  qfx 5432 20261021 B1 > "$downloads/accountactivity.qfx"
  cp "$FIXTURES/ledger-wealthsimple-activity.csv" "$downloads/activities-export-2026-10-24.csv"
  ok fetch collect
  ok import
  ok fetch plan --all --today 2026-10-25
  has "cibc (CIBC): fresh, newest 2026-10-20" "td (TD Canada Trust): fresh, newest 2026-10-21" "wealthsimple (Wealthsimple): fresh"
  lacks share
  no_canary_on_disk alex
}

run_test us22_happy_downloads_reach_the_inbox_unopened_and_import
run_test us22_error_nothing_to_collect_says_so_and_plan_lists_every_service
run_test us22_happy_bank_named_qfx_and_ledger_files_count_for_their_service
