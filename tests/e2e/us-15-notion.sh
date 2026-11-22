#!/usr/bin/env bash
# US-15: move my Notion finance pages into cashcast, privately, and read them in the web app.
source "$(dirname "$0")/lib.sh"
need zip
need curl

ID=0123456789abcdef0123456789abcdef
ID2=fedcba9876543210fedcba9876543210

# export_zip PATH: a synthetic Notion "Markdown & CSV" export, shaped like the real one.
export_zip() {
  local d="$T/notion-src" f="Finances $ID"
  mkdir -p "$d/$f/Taxes $ID2"
  printf '# Finances\n\n[Taxes](Finances%%20%s/Taxes%%20%s.md)\n\n[Budget](Finances%%20%s/Budget%%20%s.csv)\n' "$ID" "$ID2" "$ID" "$ID2" > "$d/$f.md"
  printf '# Taxes\n\nNote CANARY_NOTION_1 about tax.\n\n- [ ] File the return\n- [x] Ask the accountant\n\n![chart](Taxes%%20%s/chart.png)\n\n[outside](https://example.com/CANARY_NOTION_URL)\n\n[escape](../../../etc/passwd)\n' "$ID2" \
    > "$d/$f/Taxes $ID2.md"
  { printf '\211PNG\r\n\032\n'; head -c 16 /dev/zero; } > "$d/$f/Taxes $ID2/chart.png"
  printf 'Month,Amount\nJan,10\n' > "$d/$f/Budget $ID2.csv"
  printf 'Month,Amount\nJan,10\nFeb,CANARY_NOTION_2\n' > "$d/$f/Budget ${ID2}_all.csv"
  printf 'binary' > "$d/$f/notes $ID2.docx"
  (cd "$d" && zip -q -D -X "$1" "$f.md" "$f/Taxes $ID2.md" "$f/Taxes $ID2/chart.png" "$f/Budget $ID2.csv" \
    "$f/Budget ${ID2}_all.csv" "$f/notes $ID2.docx")
}

us15_happy_import_offline_then_read_in_the_web_app() {
  ok users add alex --province ON
  local src="$T/Export.zip"
  export_zip "$src"
  ok notion import "$src" --delete-source
  has "Imported 2 pages, 1 tables and 1 files" "Deleted the export"
  lacks Taxes Budget  # the report names no page
  [ ! -e "$src" ] || fail "the export is still there"
  eq "$(mode "$(home alex)/docs/notion")" 700
  event_any alex '.event == "notion_import" and .pages == 2'
  no_canary_on_disk alex

  web_start || { [ $? -eq 2 ] && skip "this sandbox does not allow a local port"; }
  web_get "/api/docs?user=alex" "$TOKEN"
  eq "$STATUS" 200
  eq "$(jq -c '[.[].title] | unique' <<< "$BODY")" '["Budget","Finances","Taxes"]'
  web_get "/api/doc?user=alex&path=notion/Finances/Taxes.md" "$TOKEN"
  [[ "$(jq -r .markdown <<< "$BODY")" == *CANARY_NOTION_1* ]] || fail "the page text is not served to the owner"
  stop_pid "$WEB_PID"
}

us15_error_not_an_export_changes_nothing() {
  ok users add alex --province ON
  local bad="$T/notes.txt"
  printf 'CANARY_NOT_AN_EXPORT' > "$bad"
  try notion import "$bad"; code_not0; has "not a Notion export"
  [ -e "$bad" ] || fail "the file was removed"
  [ ! -e "$(home alex)/docs/notion" ] || fail "docs/notion was made"
}

run_test us15_happy_import_offline_then_read_in_the_web_app
run_test us15_error_not_an_export_changes_nothing
