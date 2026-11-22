#!/usr/bin/env bash
# Tests for scripts/rust-parity.sh: it must catch a changed output, and it
# must report 0 when the app matches its goldens.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT="$PWD"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/parity-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
# Check the unchanged goldens for leaks once, not in every case.
export PARITY_CACHE_DIR="$TMP"
fail() { echo "parity test FAILED: $*" >&2; exit 1; }

# 1. The app as it is: 0 mismatches, and the fixture is not withheld.
case_1() {
out="$(PARITY_KEEP=1 scripts/rust-parity.sh --only "alex strategy --section cashback" 2>&1)" || fail "same code differs: $out"
grep -q "1 commands, 0 mismatches" <<<"$out" || fail "expected 0 mismatches: $out"
work="$(sed -n 's/^rust-parity: outputs kept in //p' <<<"$out")"
grep -q "Cash back" "$work"/out/*alex_strategy_--section_cashback.out || fail "the synthetic user's strategy was withheld or empty"
rm -rf "$work"
}

# 2. A candidate that changes one word in one command: exactly 1 mismatch, exit 1.
case_2() {
cat > "$TMP/changed" <<SH
#!/usr/bin/env bash
case " \$* " in
  *" knowledge list "*) "$ROOT/bin/cashcast" "\$@" | sed 's/a/A/' ;;
  *) exec "$ROOT/bin/cashcast" "\$@" ;;
esac
SH
chmod +x "$TMP/changed"
set +e
out="$(PARITY_B="$TMP/changed" scripts/rust-parity.sh --only "knowledge list" 2>&1)"
code=$?
set -e
[ "$code" -eq 1 ] || fail "expected exit 1, got $code"
grep -q "1 commands, 1 mismatches" <<<"$out" || fail "expected 1 mismatch: $out"
grep -q "knowledge list .*DIFF" <<<"$out" || fail "the changed command is not named: $out"
}

# 3. A changed exit code alone is a mismatch too.
case_3() {
cat > "$TMP/exit" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*) exit 3 ;; esac
SH
chmod +x "$TMP/exit"
if PARITY_B="$TMP/exit" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1; then fail "a changed exit code passed"; fi
}

# 4. Real data in an output fails the run (exit 3) and is never printed.
case_4() {
cat > "$TMP/leak" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*) echo "Database: MoneyWiz""_iCloud.sqlite canary-leak" ;; esac
SH
chmod +x "$TMP/leak"
set +e
out="$(PARITY_B="$TMP/leak" scripts/rust-parity.sh --only "users list" 2>&1)"
code=$?
set -e
[ "$code" -eq 3 ] || fail "a leaked output gave exit $code, not 3"
! grep -q "canary-leak" <<<"$out" || fail "the leaked text was printed"
}

# 5. Commands run with a scratch HOME, never the real one.
case_5() {
cat > "$TMP/home" <<SH
#!/usr/bin/env bash
case " \$* " in *" users list "*) [ "\$HOME" != "$HOME" ] && case "\$HOME" in "${TMPDIR:-/none}"*|/tmp/*|/private/*|/var/*) exec "$ROOT/bin/cashcast" "\$@" ;; esac; echo "real home"; exit 9 ;; esac
exec "$ROOT/bin/cashcast" "\$@"
SH
chmod +x "$TMP/home"
PARITY_B="$TMP/home" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1 || fail "a command saw the real HOME"
}

# 6. The run has no MoneyWiz: an output that shows a database fails (exit 3).
case_6() {
cat > "$TMP/db" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*) echo "Database: synthetic.sqlite" ;; esac
SH
chmod +x "$TMP/db"
set +e
PARITY_B="$TMP/db" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1
code=$?
set -e
[ "$code" -eq 3 ] || fail "an output showing a MoneyWiz database gave exit $code, not 3"
}

# 7. The owner's CASHCAST_* settings never reach a command.
case_7() {
cat > "$TMP/env" <<SH
#!/usr/bin/env bash
case " \$* " in *" users list "*) [ "\${CASHCAST_MONEYWIZ_WRITER:-}" = false ] || exit 9; case "\${CASHCAST_MONEYWIZ_APP:-}" in /Applications/*|"") exit 9 ;; esac ;; esac
exec "$ROOT/bin/cashcast" "\$@"
SH
chmod +x "$TMP/env"
CASHCAST_MONEYWIZ_WRITER=/bin/false CASHCAST_MONEYWIZ_APP=/Applications/MoneyWiz.app PARITY_B="$TMP/env" \
  scripts/rust-parity.sh --only "users list" >/dev/null 2>&1 || fail "the owner's CASHCAST_* settings reached a command"
}

# 8. A MoneyWiz table name that is not a fixture or engine word fails (exit 3).
case_8() {
cat > "$TMP/name" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" moneywiz status "*) echo "| Qzxvbank Chequing | 1 |" ;; esac
SH
chmod +x "$TMP/name"
set +e
out="$(PARITY_B="$TMP/name" scripts/rust-parity.sh --only "alex moneywiz status" 2>&1)"
code=$?
set -e
[ "$code" -eq 3 ] || fail "an unknown MoneyWiz table name gave exit $code, not 3"
! grep -q "Qzxvbank" <<<"$out" || fail "the unknown name was printed"
}

# 9. Commands run in the C locale and can never open, quit or write MoneyWiz.
case_9() {
cat > "$TMP/app" <<SH
#!/usr/bin/env bash
case " \$* " in *" users list "*)
  [ "\${LC_ALL:-}" = C ] && [ "\${CASHCAST_MONEYWIZ_OPEN:-}" = false ] && [ "\${CASHCAST_MONEYWIZ_WRITER:-}" = false ] || exit 9 ;;
esac
exec "$ROOT/bin/cashcast" "\$@"
SH
chmod +x "$TMP/app"
PARITY_B="$TMP/app" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1 || fail "a command could reach the real MoneyWiz app or locale"
}

# 10. A MoneyWiz report copied into another command's output is checked too.
case_10() {
cat > "$TMP/section" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*) printf '== users/alex/out/last-moneywiz.md\n| Cash | Qzxvpayee |\n' ;; esac
SH
chmod +x "$TMP/section"
set +e
PARITY_B="$TMP/section" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1
code=$?
set -e
[ "$code" -eq 3 ] || fail "an unknown name in a copied MoneyWiz report gave exit $code, not 3"
}

# 11. A database line inside a table is caught too.
case_11() {
cat > "$TMP/cell" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*) echo "| x | Database: y |" ;; esac
SH
chmod +x "$TMP/cell"
set +e
PARITY_B="$TMP/cell" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1
code=$?
set -e
[ "$code" -eq 3 ] || fail "a database name inside a table gave exit $code, not 3"
}

# 12. A MoneyWiz heading on an output's first line is checked too.
case_12() {
cat > "$TMP/first" <<SH
#!/usr/bin/env bash
case " \$* " in *" users list "*) printf '# MoneyWiz accounts\n| Qzxvfirst | 1 |\n' ;; esac
exec "$ROOT/bin/cashcast" "\$@"
SH
chmod +x "$TMP/first"
set +e
PARITY_B="$TMP/first" scripts/rust-parity.sh --only "users list" >/dev/null 2>&1
code=$?
set -e
[ "$code" -eq 3 ] || fail "an unknown name under a first-line MoneyWiz heading gave exit $code, not 3"
}

# 13. Only real-clock stamps read as <NOW>; a fixed date that falls on today stays.
case_13() {
cat > "$TMP/today" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*)
  d="\$(date +%Y-%m-%d)"; printf '"fetched_on": "%s",\n| x | \$1 | %s |\n- [ ] %s: Fixed task\n' "\$d" "\$d" "\$d" ;; esac
SH
chmod +x "$TMP/today"
out="$(PARITY_KEEP=1 PARITY_B="$TMP/today" scripts/rust-parity.sh --only "users list" 2>&1)" || true
work="$(sed -n 's/^rust-parity: outputs kept in //p' <<<"$out")"
f="$(ls "$work"/out/*users_list.out | head -1)"
grep -qF '"fetched_on": "<NOW>",' "$f" || fail "a fetched_on stamp did not read as <NOW>"
grep -qF '| x | $1 | <NOW> |' "$f" || fail "a stamp in a table did not read as <NOW>"
grep -qF -- "- [ ] $(date +%Y-%m-%d): Fixed task" "$f" || fail "a fixed date that falls on today was replaced with <NOW>"
rm -rf "$work"
}

# 14. A knowledge fact checked today keeps its as_of; a balance stamp reads <NOW>.
case_14() {
cat > "$TMP/fact" <<SH
#!/usr/bin/env bash
"$ROOT/bin/cashcast" "\$@"; case " \$* " in *" users list "*)
  d="\$(date +%Y-%m-%d)"; printf '   "as_of": "%s",\n   "review_by": "2027-04-30",\n   "as_of": "%s",\n   "source": "api"\n' "\$d" "\$d" ;; esac
SH
chmod +x "$TMP/fact"
out="$(PARITY_KEEP=1 PARITY_B="$TMP/fact" scripts/rust-parity.sh --only "users list" 2>&1)" || true
work="$(sed -n 's/^rust-parity: outputs kept in //p' <<<"$out")"
f="$(ls "$work"/out/*users_list.out | head -1)"
grep -qF "\"as_of\": \"$(date +%Y-%m-%d)\"," "$f" || fail "a fact checked today lost its as_of date"
grep -qF '"as_of": "<NOW>",' "$f" || fail "a balance as_of stamp did not read as <NOW>"
grep -qF 'as_of@FACT' "$f" && fail "the fact marker leaked into the output"
rm -rf "$work"
}

# The cases are independent (each run has its own work folder and HOME), so
# they run side by side; each one's messages go to its own log.
pids=()
for n in 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
  ( "case_$n" ) > "$TMP/case-$n.log" 2>&1 &
  pids+=($!)
done
failed=0
for i in "${!pids[@]}"; do
  if ! wait "${pids[$i]}"; then failed=1; cat "$TMP/case-$((i + 1)).log" >&2; fi
done
[ "$failed" -eq 0 ] || exit 1
echo "parity test: 14 cases passed"
