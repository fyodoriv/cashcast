#!/usr/bin/env bash
# US-38: buy groceries at the cheapest full basket: prices read on store pages
# go into a local database, `grocery compare` totals each store with its fees
# and promos, orders log the spend per month, and an older database imports.
# Store and product names are printed by design, so they hold no canary; the
# order amount is the private value the gate must withhold.
source "$(dirname "$0")/lib.sh"

rows() {
  cat > "$T/rows.json" << 'EOF'
[
  {"item": "oats", "store": "store-a", "name": "Oats 1 kg", "price": 20.0},
  {"item": "tea", "store": "store-a", "name": "Tea 100 g", "price": 15.0},
  {"item": "oats", "store": "store-b", "name": "Oats 1 kg", "price": 18.0},
  {"item": "oats", "store": "store-b", "name": "Organic Oats 1 kg", "price": 24.0},
  {"item": "tea", "store": "store-b", "name": "Tea 100 g", "price": 14.0},
  {"item": "oats", "store": "store-c", "name": "Oats 1 kg", "price": 1.0}
]
EOF
}

us36_happy_compares_full_baskets_with_fees_and_logs_orders() {
  ok users add alex --province ON
  rows
  ok grocery save --file "$T/rows.json" --today 2026-11-20
  has "saved 6 prices"
  ok grocery fee store-a --delivery 5 --free-over 30 --service 1 --today 2026-11-20
  ok grocery fee store-b --delivery 4 --pct-off 30 --max-off 5 --min-order 20 --today 2026-11-20
  ok grocery fees
  has "store-b" "promo 30% off up to 5.00 on 20.00+" "(checked 2026-11-20)"
  ok grocery best oats --today 2026-11-20
  has "best: 24.00 organic Organic Oats 1 kg" "sub:  18.00 Oats 1 kg"
  ok grocery compare --items oats,tea --organic-premium 0 --today 2026-11-20
  # Full baskets first, then the cheapest total: store-c lacks tea.
  eq "$(echo "$OUT" | grep -v '^ ' | cut -d: -f1 | tr '\n' ' ')" "store-b store-a store-c "
  has "store-b: 31.00 (items 32.00, promo -5.00, delivery 4.00, service 0.00)" "missing: tea"
  ok grocery stale --items oats,milk --today 2026-11-20
  eq "$OUT" "milk"
  # Three days later the prices are stale.
  ok grocery stale --items oats --today 2026-11-24
  eq "$OUT" "oats"
  grant alex agent:strategy
  ok grocery order store-b --total 123.45 --today 2026-11-20
  ok grocery orders
  has "## Grocery spend by month" "- 2026-11: 123.45 in 1 order(s)" "- 2026-11-20 store-b 123.45"
  event_any alex '.event == "grocery" and .action == "compare" and .rows == 3'
  eq "$(mode "$(home alex)/data/groceries.sqlite")" "600"
}

us36_error_orders_withheld_without_consent_and_usage_lines() {
  ok users add alex --province ON
  ok grocery order store-b --total 123.45
  ok grocery orders
  has "Report saved for the owner only"; lacks "123.45"
  try grocery compare
  code_is 2; has "grocery compare needs --items"
  try grocery fee store-a --delivery abc
  code_is 2; has "--delivery must be a number"
  try grocery best oats
  code_is 1; has "no fresh prices for 'oats'"
  try grocery import --from "$T/missing.sqlite"
  code_not0; has "no database at"
}

us36_happy_imports_an_older_database_unchanged() {
  command -v sqlite3 > /dev/null || skip "needs sqlite3"
  ok users add alex --province ON
  sqlite3 "$T/old.sqlite" "CREATE TABLE prices (item TEXT, store TEXT, name TEXT, price REAL, was REAL, unit TEXT, per100 REAL, organic INT, link TEXT, checked TEXT, PRIMARY KEY (item, store, name, checked));
    CREATE TABLE fees (store TEXT PRIMARY KEY, delivery REAL, service REAL, free_over REAL, note TEXT, checked TEXT);
    INSERT INTO prices VALUES ('tea','store-a','Tea 100 g',3.0,NULL,NULL,3.0,0,NULL,'2026-11-19');
    INSERT INTO fees VALUES ('store-a',4.99,1.0,35.0,'member','2026-11-19');"
  local before; before="$(shasum "$T/old.sqlite")"
  ok grocery import --from "$T/old.sqlite"
  has "imported 1 prices and 1 fee rules" "the old file is unchanged"
  eq "$(shasum "$T/old.sqlite")" "$before"
  ok grocery
  has "tea" "1 store(s), checked 2026-11-19"
}

run_test us36_happy_compares_full_baskets_with_fees_and_logs_orders
run_test us36_error_orders_withheld_without_consent_and_usage_lines
run_test us36_happy_imports_an_older_database_unchanged
