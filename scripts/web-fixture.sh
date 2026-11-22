#!/usr/bin/env bash
# Writes web/src/test/fixture.json: the API payload for a synthetic person.
# The TypeScript tests render every view from it; tests/repo/web-fixture.sh
# fails when the API shape changes and this file was not regenerated.
# Synthetic data only. Usage: scripts/web-fixture.sh [OUT] (default: the fixture)
# Needs the built engine, jq and sqlite3.
set -euo pipefail
# shellcheck source=tests/e2e/lib.sh
source "$(dirname "$0")/../tests/e2e/lib.sh"
OUT_FILE="${1:-$ROOT/web/src/test/fixture.json}"
TODAY=2026-10-18

# get PATH [TODAY]: one GET through the engine's web handler, as JSON.
get() {
  local out status
  out="$("$ENGINE" web-get "$1" --today "${2:-$TODAY}" --token t)"
  status="${out%%$'\n'*}"
  [[ "$status" == "200 "* ]] || { echo "$1: $out" >&2; exit 1; }
  printf '%s\n' "${out#*$'\n'}"
}

# summary: the import summary for a year of synthetic spending.
summary() {
  jq -n '
    def months: [range(0; 12) | (2025 * 12 + 9 + .) as $i | "\($i / 12 | floor)-\($i % 12 + 1 | tostring | if length < 2 then "0" + . else . end)"];
    def cents: map(. * 100 | round);
    {
      "Food > Groceries": [range(12) | 500.0],
      "Food > Restaurants": ([range(9) | 300.0] + [range(3) | 450.0]),
      "Bills > Phone": [range(12) | 80.0],
      "Travel": ([range(6) | 0.0] + [range(6) | 1200.0]),
      "Fees > Overdraft": [range(12) | 5.0],
      "Income tax": [range(12) | 400.0]
    } as $spending
    | [range(12) | 0] as $zero
    | {
        generated_on: "2026-10-18", base_currency: "CAD", months: months, files: [],
        accounts: [{name: "Chequing", class: "cash", guessed: false, class_reason: "", currency: "CAD", rows: 1,
                    last_date: null, balance_cents: 4000000, balance_source: "test", sum_balance_cents: null}],
        pay: {schedule: null, source: "", anchor: null, cycles: [], half_years: [], gap_days: {},
              off_cycle_deposits: 0, deposit_accounts: {}, cycles_per_month: []},
        income_cents: {}, income_by_category_cents: {},
        spending_cents: ($spending | map_values(cents)),
        flows_cents: {salary: $zero, salary_off_cycle: $zero, income_tax: $zero, invested: $zero},
        suggested_extra_income: [], category_roles: {}, checks: []
      }'
}

HOLDINGS='{"assets": [
  {"symbol": "XEQT", "account_bucket": "TFSA", "quantity": 120, "acb_cad": 3600.0},
  {"symbol": "EMPL", "account_bucket": "non-registered", "quantity": 40, "acb_cad": 9000.0},
  {"symbol": "BTC", "account_bucket": "non-registered", "quantity": 0.05, "acb_cad": 0, "cost_unknown": true}],
 "realized_by_year": [{"year": 2026, "gains_cad": 1200.0, "losses_cad": 200.0, "net_cad": 1000.0, "sales": 3}],
 "superficial_loss_flags": [],
 "checks": ["EMPL: E*TRADE reports 40 sellable shares; the files give 40 non-registered (match)."],
 "needs_cost": [{"asset": "BTC", "account_bucket": "non-registered", "reason": "deposit with unknown cost", "quantity": 0.05}]}'

fresh
export CASHCAST_LOG=off
"$CLI" users add alex --province ON --display Alex > /dev/null
A="$(home alex)"
full_profile "$A/profile.toml"
summary > "$A/data/summary.json"
printf '%s\n' "$HOLDINGS" > "$A/data/holdings.json"
plan="$(get "/api/plan?user=alex")"

# The Categories view: a synthetic MoneyWiz database beside a budget.
moneywiz_db "$A/MoneyWiz.sqlite"
sqlite3 "$A/MoneyWiz.sqlite" "INSERT INTO ZSYNCOBJECT (Z_PK,Z_ENT,ZNAME2,ZPARENTCATEGORY,ZGID,ZTYPE2) VALUES
  (53,20,'groceries',NULL,'gcat53',1),(54,20,'Salary',NULL,'gcat54',2),(55,20,'Transport',NULL,'gcat55',1);
  INSERT INTO ZCATEGORYASSIGMENT (ZTRANSACTION,ZCATEGORY,ZAMOUNT) VALUES (101,51,-40.0);"
printf '[moneywiz]\ndb = "%s"\n' "$A/MoneyWiz.sqlite" > "$A/cashcast.toml"
replace_first "$A/profile.toml" "[budget]" $'[budget]\n"Food > Groceries" = 600\n"Food > Coffee" = 50\n"Transport" = 150'
categories="$(get "/api/moneywiz-categories?user=alex")"
users_list="$(get "/api/users")"

# The Bank sync view: a second person whose MoneyWiz lacks bank rows.
"$CLI" users add sam --province QC --display Sam > /dev/null
S="$(home sam)"
moneywiz_db "$S/MoneyWiz.sqlite"
printf '%s' "$MONEYWIZ_EXPORT" > "$S/inbox/bank.csv"
printf '[moneywiz]\ndb = "%s"\n' "$S/MoneyWiz.sqlite" > "$S/cashcast.toml"
# The engine shows bank text only with the running web app's token.
mkdir -p "$PRIVATE/out"
printf '{"pid": %s, "port": 1, "url": "http://127.0.0.1:1/#t=t"}' "$$" > "$PRIVATE/out/web.json"
sync="$(get "/api/moneywiz-sync?user=sam" 2026-10-01)"
catalog="$(get "/api/actions")"

mkdir -p "$(dirname "$OUT_FILE")"
jq -n -S --indent 1 \
  --argjson users "$users_list" --argjson plan "$plan" --argjson actions "$catalog" \
  --argjson moneywizCategories "$categories" --argjson bankSync "$sync" \
  '{users: $users, plan: $plan, actions: $actions, moneywizCategories: $moneywizCategories, bankSync: $bankSync}' > "$OUT_FILE"
echo "wrote ${OUT_FILE#"$ROOT"/}"
