# shellcheck shell=bash disable=SC2034  # globals (OUT, BODY, TOKEN...) are read by the stories
# Shared helpers for the e2e stories (tests/e2e/us-*.sh). Source it; do not run it.
# Each story runs bin/cashcast the way a person (or their agent) does, on
# synthetic data with CANARY strings, and checks the output, files and log events.
# Needs bash, jq, and for some stories sqlite3, curl and zip/unzip.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CLI="$ROOT/bin/cashcast"
ENGINE="$ROOT/engine/target/release/cashcast-engine"
FIXTURES="$ROOT/engine/tests/fixtures"
# The real path (macOS /var is /private/var), as the app prints it.
E2E_TMP="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/cashcast-e2e.XXXXXX")" && pwd -P)"
E2E_PIDS=()
cleanup() {
  local p
  for p in "${E2E_PIDS[@]+"${E2E_PIDS[@]}"}"; do stop_pid "$p"; done
  rm -rf "$E2E_TMP"
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; [ -n "${OUT:-}" ] && printf -- '--- last output ---\n%s\n' "$OUT" >&2; exit 1; }

# fresh: a new scratch private/ root and a clean environment for one test.
# Sets T (the scratch dir) and PRIVATE.
fresh() {
  T="$(mktemp -d "$E2E_TMP/t.XXXXXX")"
  PRIVATE="$T/private"
  unset CASHCAST_HOME CASHCAST_USER
  export CASHCAST_PRIVATE="$PRIVATE" CASHCAST_LOG=on CASHCAST_AUTO_UPDATE=0 CASHCAST_POSTHOG_KEY="" CASHCAST_AGENT=1 CASHCAST_LIVE=off
  # A scratch Downloads folder: `moneywiz sync` collects from it, never from the real one.
  mkdir -p "$T/Downloads" "$PRIVATE"
  printf '[fetch]\ndownloads = "%s"\n' "$T/Downloads" > "$PRIVATE/settings.toml"
}

# run_test NAME: runs the function NAME in a subshell with a fresh scratch root.
# Not `( ... ) || ...`: that would turn off set -e inside the test.
run_test() {
  local rc
  set +e
  ( set -e; fresh; "$1" )
  rc=$?
  set -e
  [ "$rc" -eq 0 ] || { echo "FAIL in $1" >&2; exit 1; }
  echo "ok $1"
}
# skip WHY: ends the current test as skipped.
skip() { echo "skip: $*"; exit 0; }

canary_check() { case "$OUT" in *CANARY*) fail "free text leaked from: $*" ;; esac; }

# ok ARGS...: runs bin/cashcast, needs exit 0. try ARGS...: any exit code.
# Both set OUT (stdout then stderr) and CODE, and fail if CANARY is printed.
_run() {
  local o e
  o="$(mktemp "$E2E_TMP/o.XXXXXX")"; e="$(mktemp "$E2E_TMP/e.XXXXXX")"
  CODE=0; "$CLI" "$@" < /dev/null > "$o" 2> "$e" || CODE=$?
  OUT="$(cat "$o" "$e")"
  rm -f "$o" "$e"
  canary_check "$@"
}
ok() { _run "$@"; [ "$CODE" -eq 0 ] || fail "exit $CODE from: $*"; }
try() { _run "$@"; }

# has NEEDLE...: every needle is in OUT. lacks NEEDLE...: none is.
has() { local n; for n in "$@"; do [[ "$OUT" == *"$n"* ]] || fail "missing: $n"; done; }
lacks() { local n; for n in "$@"; do [[ "$OUT" != *"$n"* ]] || fail "unexpected: $n"; done; }
# inside TEXT NEEDLE: NEEDLE is in TEXT.
inside() { [[ "$1" == *"$2"* ]] || fail "missing: $2"; }
code_is() { [ "$CODE" -eq "$1" ] || fail "exit $CODE, expected $1"; }
code_not0() { [ "$CODE" -ne 0 ] || fail "exit 0, expected a failure"; }
eq() { [ "$1" == "$2" ] || fail "expected [$2], got [$1]${3:+ ($3)}"; }

home() { echo "$PRIVATE/users/$1"; }

# grant USER SCOPE...: what the owner writes in consent.toml by hand.
grant() {
  local user="$1" s; shift
  : > "$(home "$user")/consent.toml"
  for s in "$@"; do
    printf '[[grants]]\nscope = "%s"\ngranted_on = 2026-10-18\nexpires_on = 2099-01-01\nvia = "hand"\n\n' "$s" >> "$(home "$user")/consent.toml"
  done
}

# event_any USER JQ_FILTER: some event in the user's log matches.
event_any() {
  local f; f="$(home "$1")/logs/events.jsonl"
  [ -f "$f" ] && jq -e -s "any(.[]; $2)" "$f" > /dev/null || fail "no event matches: $2"
}
events() { local f; f="$(home "$1")/logs/events.jsonl"; if [ -f "$f" ]; then cat "$f"; fi; }

no_canary_on_disk() {
  local h sub f; h="$(home "$1")"
  for sub in data out logs; do
    [ -d "$h/$sub" ] || continue
    while IFS= read -r f; do
      case "$f" in */review.md | */moneywiz-fixes.md) continue ;; esac
      ! grep -q CANARY "$f" || fail "canary in $(basename "$f")"
    done < <(find "$h/$sub" -type f \( -name '*.json' -o -name '*.jsonl' -o -name '*.md' -o -name '*.txt' \))
  done
}

# mode PATH: the permission bits in octal (700, 600). Portable: reads ls -ld.
mode() {
  local p o="" i c v
  p="$(ls -ld "$1" | cut -c2-10)"
  for i in 0 3 6; do
    v=0
    c="${p:$i:1}"; [ "$c" = r ] && v=$((v + 4))
    c="${p:$((i + 1)):1}"; [ "$c" = w ] && v=$((v + 2))
    c="${p:$((i + 2)):1}"; case "$c" in x | s | t) v=$((v + 1)) ;; esac
    o="$o$v"
  done
  echo "$o"
}
# private_only PATH: no group or other bits.
private_only() { case "$(mode "$1")" in ?00) ;; *) fail "$1 is mode $(mode "$1")" ;; esac; }

# replace_first FILE OLD NEW: the first OLD (literal) becomes NEW.
replace_first() {
  local text; text="$(cat "$1"; printf x)"; text="${text%x}"
  [[ "$text" == *"$2"* ]] || return 0
  printf '%s' "${text/"$2"/"$3"}" > "$1"
}

# line_starting TEXT PREFIX: the first line that starts with PREFIX.
line_starting() { printf '%s\n' "$1" | awk -v p="$2" 'index($0, p) == 1 { print; exit }'; }

# --- Synthetic data --------------------------------------------------------

# Date helpers in awk: days since 1970-01-01 and back.
AWK_DATES='
function dfc(y, m, d,   era, yoe, doy, doe) {
  y -= (m <= 2); era = int((y >= 0 ? y : y - 399) / 400); yoe = y - era * 400
  doy = int((153 * (m + (m > 2 ? -3 : 9)) + 2) / 5) + d - 1
  doe = yoe * 365 + int(yoe / 4) - int(yoe / 100) + doy
  return era * 146097 + doe - 719468
}
function cfd(z,   era, doe, yoe, y, doy, mp, d, m) {
  z += 719468; era = int((z >= 0 ? z : z - 146096) / 146097); doe = z - era * 146097
  yoe = int((doe - int(doe / 1460) + int(doe / 36524) - int(doe / 146096)) / 365)
  y = yoe + era * 400; doy = doe - (365 * yoe + int(yoe / 4) - int(yoe / 100))
  mp = int((5 * doy + 2) / 153); d = doy - int((153 * mp + 2) / 5) + 1
  m = mp + (mp < 10 ? 3 : -9)
  return sprintf("%04d-%02d-%02d", y + (m <= 2), m, d)
}'

# A year of chequing rows with a balance: payroll, groceries, rent, phone, restaurants.
chequing_csv() {
  awk "$AWK_DATES"'
  BEGIN {
    s = dfc(2025, 10, 3); for (k = 0; k < 27; k++) print cfd(s + 14 * k) "|PAYROLL CANARY01|300000"
    for (i = 0; i < 12; i++) {
      if (i < 3) { y = 2025; m = 10 + i } else { y = 2026; m = i - 2 }
      print cfd(dfc(y, m, 2)) "|GROCER CANARY02|-45000"
      print cfd(dfc(y, m, 1)) "|RENT CANARY03|-180000"
      print cfd(dfc(y, m, 5)) "|PHONE CO CANARY04|-8000"
      print cfd(dfc(y, m, 9)) "|RESTAURANT CANARY05|-12000"
    }
  }' | LC_ALL=C sort -t'|' -k1,1 -k2,2 -k3,3n | awk -F'|' '
  BEGIN {
    print "Date,Description,Category,Amount,Balance"; bal = 2000000
    cat["PAYROLL"] = "Income > Salary"; cat["GROCER"] = "Food > Groceries"; cat["RENT"] = "Housing > Rent"
    cat["PHONE"] = "Bills > Phone"; cat["RESTAURANT"] = "Food > Restaurants"
  }
  { bal += $3; split($2, w, " "); printf "%s,%s,%s,%.2f,%.2f\n", $1, $2, cat[w[1]], $3 / 100, bal / 100 }'
}

# Three subscriptions (monthly, yearly, weekly) plus noise.
subscriptions_csv() {
  awk "$AWK_DATES"'
  BEGIN {
    for (i = 0; i < 12; i++) {
      if (i < 3) { y = 2025; m = 10 + i } else { y = 2026; m = i - 2 }
      print cfd(dfc(y, m, 12)) "|NETFLIX.COM CANARY71|-1699"
      print cfd(dfc(y, m, 25)) "|PAYROLL CANARY78|300000"
    }
    print cfd(dfc(2024, 3, 2)) "|DROPBOX ANNUAL CANARY72|-11900"
    print cfd(dfc(2025, 3, 4)) "|DROPBOX ANNUAL CANARY72|-11900"
    print cfd(dfc(2026, 3, 3)) "|DROPBOX ANNUAL CANARY72|-11900"
    s = dfc(2026, 6, 1); for (k = 0; k < 18; k++) print cfd(s + 7 * k) "|MEAL KIT CANARY73|-7900"
    split("4000 12000 7500 21000 3000", a, " ")
    s = dfc(2026, 3, 2); for (k = 0; k < 30; k++) print cfd(s + 7 * k) "|GROCER CANARY74|-" a[k % 5 + 1]
    split("1 3 1 20 3 9 3 11 7 1 9 30", c, " ")
    for (k = 1; k <= 12; k += 2) print cfd(dfc(2026, c[k], c[k + 1])) "|CAFE CANARY75|-550"
  }' | LC_ALL=C sort -t'|' -k1,1 -k2,2 -k3,3n | awk -F'|' '
  BEGIN { print "Date,Description,Amount" }
  { printf "%s,%s,%.2f\n", $1, $2, $3 / 100 }'
}

# The original forecast story's chequing and Visa exports (no final newline).
forecast_chequing_csv() {
  printf '%s' "$(awk "$AWK_DATES"'
  BEGIN {
    s = dfc(2026, 1, 9); for (k = 0; k < 19; k++) print cfd(s + 14 * k) "|ACME PAYROLL CANARY01|200000"
    for (m = 1; m < 10; m++) {
      print cfd(dfc(2026, m, 3)) "|LOBLAWS CANARY02|-40000"
      print cfd(dfc(2026, m, 1)) "|RENT CANARY03|-150000"
    }
    print "2026-09-10|VISA PAYMENT CANARY04|-12000"
  }' | LC_ALL=C sort -t'|' -k1,1 -k2,2 -k3,3n | awk -F'|' '
  BEGIN { print "Date,Description,Amount,Balance"; bal = 500000 }
  { bal += $3; printf "%s,%s,%.2f,%.2f\n", $1, $2, $3 / 100, bal / 100 }')"
}
forecast_visa_csv() {
  local d
  printf 'Transaction Date,Description,Amount'
  for d in 1 2 3 4 5 6; do printf '\n2026-09-0%s,UBER EATS CANARY05,%s.00' "$d" $((20 + d)); done
  printf '\n2026-09-11,PAYMENT THANK YOU CANARY06,-120.00'
}

# The example profile with every section filled (a synthetic person).
FULL_PROFILE_CARDS='
[[wallet.cards]]
id = "test-flat"
name = "Test Flat 2%"
earn = { other = 0.02 }
fx_fee = 0.0
limit = 25000
statement_close_day = 24
paydown_days_before = 1

[[wallet.cards]]
id = "test-grocery"
name = "Test Grocery 4%"
earn = { groceries = 0.04, recurring = 0.04, gas = 0.02, other = 0.01 }
caps = [{ categories = ["groceries", "recurring"], limit = 25000, period = "card_year", after_rate = 0.01 }]
annual_fee = 120
fee_rebated = true
fx_fee = 0.025
limit = 15000
statement_close_day = 25

[[wallet.cards]]
id = "test-costly"
name = "Test Costly"
earn = { dining = 0.021, other = 0.005 }
annual_fee = 150
fx_fee = 0.025
limit = 5000
statement_close_day = 18

[[banking.accounts]]
id = "test-package"
bank = "Test Bank"
name = "Package"
monthly_fee = 30.95
waive_min_balance = 6000
card_fee_rebate = 120
keep = 6500
source = "https://example.com"
confidence = "estimate"
'
full_profile() {
  local f="$1" nl=$'\n'
  replace_first "$f" "employment = 0$nl" "employment = 120000$nl"
  replace_first "$f" "rrsp = 0$nl" "rrsp = 20000$nl"
  replace_first "$f" "rrsp_limit_year = 0$nl" "rrsp_limit_year = 2026$nl"
  replace_first "$f" "tfsa = 0$nl" "tfsa = 7000$nl"
  replace_first "$f" "fhsa = 0$nl" "fhsa = 8000$nl"
  replace_first "$f" "first_time_home_buyer = false" "first_time_home_buyer = true${nl}mortgage_in_months = 10"
  replace_first "$f" "# monthly = 2000" "monthly = 3000"
  replace_first "$f" '# service = "chexy"' "service = \"test-rent\"${nl}fee_rate = 0.015${nl}membership_fee = 150${nl}membership_rate = 0.015${nl}codes_as = \"recurring\""
  replace_first "$f" '# "Food & Dining" = 800' '"Food" = 700'
  replace_first "$f" '# symbol = "ACME"' 'symbol = "EMPL"'
  replace_first "$f" "symbols = []" "symbols = [\"EMPL\", \"XEQT\"]${nl}metrics = { EMPL = { pe = 22.0, profit_margin = 0.2, roe = 0.3, revenue_growth = 0.05, beta = 1.1 } }"
  printf '%s' "$FULL_PROFILE_CARDS" >> "$f"
}

# setup_person: alex in ON with a full profile, a year of chequing, and an import.
setup_person() {
  ok users add alex --province ON
  full_profile "$(home alex)/profile.toml"
  chequing_csv > "$(home alex)/inbox/bank.csv"
  grant alex agent:aggregates agent:strategy
  ok import --today 2026-10-18
}

# --- The local web app -----------------------------------------------------

stop_pid() { pkill -P "$1" 2> /dev/null || true; kill "$1" 2> /dev/null || true; wait "$1" 2> /dev/null || true; }

# web_start: starts `bin/cashcast web --port 0`; sets WEB_PID, PORT, TOKEN.
# Returns 2 (the caller skips) when the sandbox allows no local port.
web_start() {
  local log="$T/web-start.log" i line url
  "$CLI" web --port 0 < /dev/null > "$log" 2>&1 &
  WEB_PID=$!; E2E_PIDS+=("$WEB_PID")
  for i in $(seq 1 600); do [ -s "$log" ] && grep -q . "$log" && break; sleep 0.1; done
  line="$(head -n1 "$log")"
  if [[ "$line" != *"running on port"* ]]; then
    stop_pid "$WEB_PID"
    if [[ "$line" == *"no permission"* || "$line" == *"Operation not permitted"* ]]; then return 2; fi
    fail "no start line printed: $line"
  fi
  [[ "$line" != *"#t="* ]] || fail "an agent never sees the token link"
  # The owner's link, from the state file in out/ (agents never open it).
  url="$(jq -r .url "$PRIVATE/out/web.json")"
  [[ "$url" =~ http://127\.0\.0\.1:([0-9]+)/#t=([A-Za-z0-9_-]+) ]] || fail "no link in web.json"
  PORT="${BASH_REMATCH[1]}"; TOKEN="${BASH_REMATCH[2]}"
}

# web_get PATH [TOKEN]: sets STATUS and BODY.
web_get() {
  local f="$T/body"
  STATUS="$(curl -s -o "$f" -w '%{http_code}' --max-time 60 ${2:+-H "X-Cashcast-Token: $2"} "http://127.0.0.1:$PORT$1")"
  BODY="$(cat "$f")"
}

# --- MoneyWiz: a synthetic Core Data store and a fake app ------------------

# mw_ts DAY: seconds from 2001-01-01 to DAY at 12:00 UTC.
mw_ts() { awk -v d="$1" "$AWK_DATES"'BEGIN { split(d, p, "-"); print dfc(p[1] + 0, p[2] + 0, p[3] + 0) * 86400 + 43200 - 978307200 }'; }

# moneywiz_db PATH: Chequing and Visa follow exports, Cash is by hand.
moneywiz_db() {
  sqlite3 "$1" <<SQL
CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER PRIMARY KEY, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);
CREATE TABLE ZSYNCOBJECT (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, ZNAME VARCHAR, ZCURRENCYNAME VARCHAR,
  ZOPENINGBALANCE FLOAT, ZNAME2 VARCHAR, ZPARENTCATEGORY INTEGER, ZNAME5 VARCHAR, ZAMOUNT1 FLOAT,
  ZDATE1 TIMESTAMP, ZDESC2 VARCHAR, ZNOTES1 VARCHAR, ZACCOUNT2 INTEGER, ZPAYEE2 INTEGER,
  ZRECIPIENTACCOUNT1 INTEGER, ZSENDERACCOUNT INTEGER, ZGID VARCHAR, ZTYPE2 INTEGER);
CREATE TABLE ZCATEGORYASSIGMENT (Z_PK INTEGER PRIMARY KEY, ZTRANSACTION INTEGER, ZCATEGORY INTEGER, ZAMOUNT FLOAT);
INSERT INTO Z_PRIMARYKEY VALUES (10,'Account',0,0),(11,'BankChequeAccount',10,0),(13,'CashAccount',10,0),
  (14,'CreditCardAccount',10,0),(20,'Category',0,0),(37,'Transaction',0,0),(38,'DepositTransaction',37,0),
  (48,'WithdrawTransaction',37,0),(46,'TransferWithdrawTransaction',37,0),(45,'TransferDepositTransaction',37,0);
INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME,ZCURRENCYNAME,ZOPENINGBALANCE,ZGID) VALUES
  (1,11,'Chequing','CAD',1000.0,'gacct1'),(2,14,'Visa','CAD',0.0,'gacct2'),(3,13,'Cash','CAD',100.0,'gacct3');
INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY,ZGID,ZTYPE2) VALUES
  (50,20,'Food',NULL,'gcat50',1),(51,20,'Groceries',50,'gcat51',1),(52,20,'Old',NULL,'gcat52',1);
INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZGID) VALUES
  (100,48,1,-1800.0,$(mw_ts 2026-09-01).0,'RENT CANARYMW0','g100'),
  (101,48,1,-40.0,$(mw_ts 2026-09-02).0,'GROCER CANARYMW1','g101'),
  (103,48,3,-20.0,$(mw_ts 2026-09-03).0,'COFFEE CANARYMW3','g103');
INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (103,52,-20.0);
SQL
}

MONEYWIZ_EXPORT='Date,Account,Description,Amount,Balance
2026-09-01,Chequing,RENT CANARY01,-1800.00,
2026-09-02,Chequing,GROCER CANARY02,-45.00,
2026-09-05,Chequing,PHONE CO CANARY03,-80.00,
2026-09-15,Chequing,VISA PAYMENT CANARY04,-500.00,
2026-09-30,Chequing,PAYROLL CANARY05,3000.00,2575.00
2026-09-08,Visa,RESTAURANT CANARY06,-30.00,
2026-09-15,Visa,PAYMENT THANK YOU CANARY07,500.00,470.00
'

# fake_moneywiz HOME DB: a fake MoneyWiz that saves each moneywiz:// link into
# the database, like the app. Prints its path.
fake_moneywiz() {
  local script="$1/fake-moneywiz"
  {
    echo '#!/usr/bin/env bash'
    echo 'set -euo pipefail'
    printf 'DB=%q\n' "$2"
    printf 'AWK_DATES=%q\n' "$AWK_DATES"
    cat <<'APP'
url="$1"; rest="${url#*://}"; kind="${rest%%\?*}"; query="${rest#*\?}"
decode() { local s="${1//+/ }"; printf '%b' "${s//%/\\x}"; }
sq() { printf "'%s'" "${1//\'/\'\'}"; }
declare -A q=()
IFS='&' read -r -a parts <<< "$query"
for p in "${parts[@]}"; do
  k="$(decode "${p%%=*}")"; v="$(decode "${p#*=}")"
  [ -n "$v" ] && [ -z "${q[$k]+x}" ] && q[$k]="$v"
done
when="$(awk -v d="${q[date]}" "$AWK_DATES"'BEGIN { split(d, a, /[- :T]/); print dfc(a[1] + 0, a[2] + 0, a[3] + 0) * 86400 + a[4] * 3600 + a[5] * 60 + a[6] - 978307200 }')"
acct() { sqlite3 "$DB" "SELECT Z_PK FROM ZSYNCOBJECT WHERE ZNAME IS NOT NULL AND REPLACE(ZNAME, ' ', '') = $(sq "$1") ORDER BY Z_PK DESC LIMIT 1"; }
add() {
  sqlite3 "$DB" "INSERT INTO ZSYNCOBJECT (Z_ENT,ZACCOUNT2,ZAMOUNT1,ZDATE1,ZDESC2,ZNOTES1) VALUES ($1,$2,$3,$when.0,$(sq "${q[description]:-}"),$(sq "${q[memo]:-}"))"
}
amount="${q[amount]}"
case "$kind" in
  transfer) add 46 "$(acct "${q[account]}")" "-1.0*$amount"; add 45 "$(acct "${q[toAccount]}")" "1.0*$amount" ;;
  expense) add 48 "$(acct "${q[account]}")" "-1.0*$amount" ;;
  *) add 38 "$(acct "${q[account]}")" "1.0*$amount" ;;
esac
APP
  } > "$script"
  chmod 755 "$script"
  echo "$script"
}

# fake_writer HOME: stands in for engine/moneywiz-writer (Swift, macOS only):
# applies the plan with SQL to the synthetic store, and logs each call to
# HOME/writer.log. Prints its path.
fake_writer() {
  local script="$1/fake-writer"
  {
    echo '#!/usr/bin/env bash'
    echo 'set -euo pipefail'
    printf 'LOG=%q\n' "$1/writer.log"
    cat <<'WRITER'
args=("$@"); line=""
for a in "${args[@]}"; do case "$a" in /*) a="${a##*/}" ;; esac; line="${line:+$line }$a"; done
echo "$line" >> "$LOG"
declare -A opt=()
for ((i = 1; i < ${#args[@]} - 1; i++)); do case "${args[$i]}" in --*) opt[${args[$i]}]="${args[$((i + 1))]}" ;; esac; done
if [ "${args[0]}" = checksum ]; then echo '{"model": "sum1", "store": "sum1"}'; exit 0; fi
plan="${opt[--plan]}"; store="${opt[--store]}"
backup="$(mktemp)"; cp "$store" "$backup"
die() { cp "$backup" "$store"; rm -f "$backup"; echo "$1" >&2; exit 1; }
sql() { sqlite3 "$store" "$1"; }
sq() { printf "'%s'" "${1//\'/\'\'}"; }
cat_of() {  # a JSON array of names -> the category's Z_PK
  local parent=NULL name row
  while IFS= read -r name; do
    if [ "$parent" = NULL ]; then
      row="$(sql "SELECT Z_PK FROM ZSYNCOBJECT WHERE Z_ENT=20 AND ZNAME2=$(sq "$name") AND ZPARENTCATEGORY IS NULL")"
    else
      row="$(sql "SELECT Z_PK FROM ZSYNCOBJECT WHERE Z_ENT=20 AND ZNAME2=$(sq "$name") AND ZPARENTCATEGORY=$parent")"
    fi
    [ -n "$row" ] || die "error: a category in the plan does not exist"
    parent="$row"
  done < <(jq -r '.[]' <<< "$1")
  echo "$parent"
}
while IFS= read -r op; do
  case "$(jq -r .op <<< "$op")" in
    add)
      n="$(jq '.path | length' <<< "$op")"; parent=NULL
      [ "$n" -gt 1 ] && parent="$(cat_of "$(jq -c '.path[:-1]' <<< "$op")")"
      if [ "$parent" != NULL ]; then template="$parent"
      elif [ "$(jq '(.like // []) | length' <<< "$op")" -gt 0 ]; then template="$(cat_of "$(jq -c .like <<< "$op")")"
      else template=50; fi
      kind="$(sql "SELECT ZTYPE2 FROM ZSYNCOBJECT WHERE Z_PK=$template")"
      sql "INSERT INTO ZSYNCOBJECT (Z_ENT,ZNAME2,ZPARENTCATEGORY,ZGID,ZTYPE2) VALUES (20,$(sq "$(jq -r '.path[-1]' <<< "$op")"),$parent,'new',$kind)"
      ;;
    rename) pk="$(cat_of "$(jq -c .path <<< "$op")")"; sql "UPDATE ZSYNCOBJECT SET ZNAME2=$(sq "$(jq -r .to <<< "$op")") WHERE Z_PK=$pk" ;;
  esac
done < <(jq -c '.categories[]' "$plan")
while IFS= read -r e; do
  IFS='|' read -r pk amount <<< "$(sql "SELECT Z_PK, ZAMOUNT1 FROM ZSYNCOBJECT WHERE ZGID=$(sq "$(jq -r .gid <<< "$e")")")"
  awk -v a="$amount" -v b="$(jq -r .expect_amount <<< "$e")" 'BEGIN { d = a - b; exit !((d < 0 ? -d : d) > 0.005) }' &&
    die "error: a row changed since the plan was made"
  [ "$(jq -r '.amount == null' <<< "$e")" = true ] || sql "UPDATE ZSYNCOBJECT SET ZAMOUNT1=$(jq -r .amount <<< "$e") WHERE Z_PK=$pk"
  [ "$(jq -r '.memo == null' <<< "$e")" = true ] || sql "UPDATE ZSYNCOBJECT SET ZNOTES1=$(sq "$(jq -r .memo <<< "$e")") WHERE Z_PK=$pk"
  [ "$(jq -r '.account == null' <<< "$e")" = true ] || sql "UPDATE ZSYNCOBJECT SET ZACCOUNT2=(SELECT Z_PK FROM ZSYNCOBJECT WHERE ZGID=$(sq "$(jq -r .account <<< "$e")")) WHERE Z_PK=$pk"
  if [ "$(jq -r '.category == null' <<< "$e")" != true ]; then
    c="$(cat_of "$(jq -c .category <<< "$e")")"
    sql "DELETE FROM ZCATEGORYASSIGMENT WHERE ZTRANSACTION=$pk; INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES ($pk,$c,$amount)"
  fi
done < <(jq -c '.rows[]' "$plan")
while IFS= read -r a; do
  sql "UPDATE ZSYNCOBJECT SET ZOPENINGBALANCE=$(jq -r .opening <<< "$a") WHERE ZGID=$(sq "$(jq -r .gid <<< "$a")")"
done < <(jq -c '.accounts[]' "$plan")
while IFS= read -r op; do
  [ "$(jq -r .op <<< "$op")" = delete ] || continue
  old="$(cat_of "$(jq -c .path <<< "$op")")"
  if [ "$(jq '(.move_to // []) | length' <<< "$op")" -gt 0 ]; then
    to="$(cat_of "$(jq -c .move_to <<< "$op")")"
    sql "UPDATE ZCATEGORYASSIGMENT SET ZCATEGORY=$to WHERE ZCATEGORY=$old"
  fi
  sql "DELETE FROM ZSYNCOBJECT WHERE Z_PK=$old"
done < <(jq -c '.categories[]' "$plan")
rm -f "$backup"
echo "{\"rows\": $(jq '.rows | length' "$plan")}"
WRITER
  } > "$script"
  chmod 755 "$script"
  echo "$script"
}

need() { command -v "$1" > /dev/null || fail "$1 is not installed"; }
need jq
