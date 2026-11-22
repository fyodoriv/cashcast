#!/usr/bin/env bash
# Parity harness for the Rust port (decision 0010).
#
# Runs every command in tests/parity/commands.txt on the synthetic users in
# tests/parity/fixture with the CLI under test. It compares the output, the
# exit code, the files written to data/ and out/, and the log events with the
# golden files in tests/parity/golden/, then prints the count of commands
# that differ. The goldens are the Python reference's outputs, frozen just
# before the Python code was deleted (step 9, part 11). A change that alters
# an output on purpose records new goldens (--record) and shows the diff in its PR.
#
# Usage: scripts/rust-parity.sh [--runs N] [--only WORD] [--record]
#   --runs N   time each command N times (default 1; use 5 for a speed report)
#   --only W   run only the commands whose line contains W (state-dependent
#              commands may then differ: goldens come from the full run)
#   --record   write the outputs as the new goldens instead of comparing
# PARITY_B overrides the CLI under test (a test uses this).
# PARITY_KEEP=1 keeps the work folder and prints its path.
# PARITY_CACHE_DIR=DIR remembers that the goldens passed the leak checks, so
# repeated runs (tests/parity/harness-test.sh) check unchanged goldens once.
# Exit: 0 when every command matches its golden, 1 when one differs.
set -euo pipefail
# Start from an empty environment: the owner's shell may set CASHCAST_*
# variables (an app path, a writer, a telemetry URL) that reach real data.
# Only these pass through; the rest is set on purpose below.
if [ -z "${PARITY_CLEAN_ENV:-}" ]; then
  exec env -i PARITY_CLEAN_ENV=1 CASHCAST_LIVE=off PATH="$PATH" LANG=C LC_ALL=C PYTHONUTF8=1 TERM="${TERM:-dumb}" \
    TMPDIR="${TMPDIR:-/tmp}" PARITY_REAL_HOME="$HOME" \
    UV_CACHE_DIR="${UV_CACHE_DIR:-$HOME/.cache/uv}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
    RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
    ${PARITY_B:+PARITY_B="$PARITY_B"} ${PARITY_KEEP:+PARITY_KEEP="$PARITY_KEEP"} \
    ${PARITY_CACHE_DIR:+PARITY_CACHE_DIR="$PARITY_CACHE_DIR"} \
    bash "$0" "$@"
fi
cd "$(dirname "$0")/.."
ROOT="$PWD"
RUNS=1
ONLY=""
RECORD=""
while [ $# -gt 0 ]; do
  case "$1" in
    --runs) RUNS="$2"; shift 2 ;;
    --only) ONLY="$2"; shift 2 ;;
    --record) RECORD=1; shift ;;
    *) echo "rust-parity: unknown argument $1" >&2; exit 2 ;;
  esac
done
B="${PARITY_B:-$ROOT/bin/cashcast}"
GOLDEN="$ROOT/tests/parity/golden"
TODAY=2026-10-18
NOW="$(date +%Y-%m-%d)"
FIXTURE="$ROOT/tests/parity/fixture"
ENGINE="$ROOT/engine/target/release/cashcast-engine"
# Build first: bin/cashcast rebuilds a stale engine and says so on stderr,
# which the harness keeps in each output (2>&1).
if command -v cargo >/dev/null && [ ! -L "$ROOT/engine/target" ]; then
  cargo build --release --quiet --manifest-path "$ROOT/engine/Cargo.toml" >&2
  touch "$ENGINE"
fi
SCOPES="agent:aggregates agent:strategy"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/rust-parity.XXXXXX")"
WORK="$(cd "$WORK" && pwd -P)"  # one spelling of the path (macOS: /private/var/...)
[ -n "${PARITY_KEEP:-}" ] || trap 'rm -rf "$WORK"' EXIT  # PARITY_KEEP=1 keeps the outputs
PRIV="$WORK/private"
# A scratch HOME, so nothing reaches the owner's real files (MoneyWiz finds
# its database under $HOME/Library). Tool caches keep their real places.
REAL_HOME="$PARITY_REAL_HOME"
unset PARITY_REAL_HOME
export HOME="$WORK/home" XDG_CONFIG_HOME="$WORK/home/.config" XDG_DATA_HOME="$WORK/home/.local/share" XDG_CACHE_HOME="$WORK/home/.cache"
mkdir -p "$HOME"
# No MoneyWiz app either: the probe would read the installed bundle, which CI lacks.
export CASHCAST_MONEYWIZ_APP="$WORK/no-app/MoneyWiz.app"
# Nothing may open, quit or write the real app, even with local:moneywiz
# consent: opener and writer are `false`, the model and process match nothing.
export CASHCAST_MONEYWIZ_OPEN=false CASHCAST_MONEYWIZ_WRITER=false
export CASHCAST_MONEYWIZ_PROCESS=no-such-moneywiz CASHCAST_MONEYWIZ_MODEL="$WORK/no-app/model"
if [ -n "${PARITY_KEEP:-}" ]; then
  echo "rust-parity: PARITY_KEEP keeps $WORK; it holds only synthetic data, delete it when done" >&2
fi
# Real data in an output fails the run, naming files only:
# - a real database name or a path under the real HOME;
# - a positive check: the run has no MoneyWiz database or app, so no output
#   may show one ("Database:" header, an app version line).
LEAK="MoneyWiz""_iCloud|ipadMoneyWiz|$REAL_HOME/|Database: |App: .*version"
leaks() {  # leaks <dir>: exit 3 when any file there holds real data
  local hits
  hits="$(grep -lE -- "$LEAK" "$1"/*.out 2>/dev/null || true)"
  [ -z "$hits" ] && return 0
  echo "rust-parity: real data outside the scratch dirs reached these files (not shown):" >&2
  printf '%s\n' "$hits" | sed 's|.*/|  |' >&2
  exit 3
}

now() { echo "${EPOCHREALTIME/[.,]/}"; }  # microseconds

# Output with run-to-run noise removed: times of day and durations.
normalise() {
  sed -E \
    -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|[+-][0-9]{2}:?[0-9]{2})?/<TIME>/g' \
    -e 's/[0-9]+(\.[0-9]+)? ?(ms|s) (elapsed|total)/<DUR>/g' \
    -e 's/"(ms|seconds|duration_ms)": ?[0-9.]+/"\1": <DUR>/g' \
    -e 's/"distinct_id": ?"[0-9a-f]{32}"/"distinct_id": "<ID>"/g' |
  # A Python traceback compares as its message: where Python crashed,
  # Rust prints `error: MESSAGE` and exits 1 too.
  awk '/^Traceback \(most recent call last\):$/ { tb = 1; next }
       tb && /^ / { next }
       tb { tb = 0; sub(/^[A-Za-z_.]+: /, "error: ") }
       { print }'
}

# Machine paths become placeholders, so a golden matches on any machine.
depath() {
  # Real-clock stamps read as <NOW>: `balances record` writes fetched_on,
  # reports write as_of and generated_on, and tables show them in a cell.
  # Only those: a date derived from the fixed --today (a task's next due
  # date) must stay, or a golden recorded on that day fails every other day.
  # A knowledge fact's as_of is the day it was checked, not a stamp: a fact
  # prints "review_by" on the next line, so that as_of keeps its date.
  awk -v now="$NOW" '
    function flush() { if (held != "") { print held; held = "" } }
    held != "" && /^ *"review_by":/ { sub(/"as_of": "/, "\"as_of@FACT\": \"", held) }
    { flush() }
    index($0, "\"as_of\": \"" now "\"") { held = $0; next }
    { print }
    END { flush() }' |
  sed -E -e "s|$WORK|<WORK>|g" -e "s|${WORK#/private}|<WORK>|g" -e "s|$ROOT|<ROOT>|g" \
    -e "s/\"(fetched_on|as_of|generated_on)\": \"$NOW\"/\"\1\": \"<NOW>\"/g" \
    -e ":cell" -e "s/\| $NOW \|/| <NOW> |/" -e "t cell" -e 's/"as_of@FACT"/"as_of"/g'
}

# snapshot_text <file>...: each text file as "== name" and its normalised
# content. One normalise for all of them: a sed and an awk per file made up
# most of the self-test's time (about 4,000 files per run).
snapshot_text() {
  [ $# -gt 0 ] || return 0
  awk -v pre="$PRIV/" 'BEGIN {
    for (i = 1; i < ARGC; i++) {
      f = ARGV[i]; print "== " substr(f, length(pre) + 1)
      while ((getline l < f) > 0) print l
      close(f)
    }
    exit
  }' "$@" | normalise
}

# The files a command left behind, as one comparable text: name and content.
# Log events keep their names and fields but lose their time and durations.
snapshot() {
  local home f text=()
  for home in "$PRIV"/users/*; do
    [ -d "$home" ] || continue
    # `|| true`: a missing folder makes find exit 1, and pipefail would end the run silently.
    while read -r f; do
      case "$f" in
        *.xlsx|*.numbers)
          snapshot_text ${text[@]+"${text[@]}"}; text=()
          echo "== ${f#"$PRIV"/}"
          # A workbook compares by its cells (values and formulas), not its bytes (decision 0010).
          case "$f" in
            *.xlsx) "$ENGINE" xlsx-cells "$f" ;;
            *) echo "(binary, $(wc -c < "$f" | tr -d ' ') bytes)" ;;
          esac ;;
        *) text+=("$f") ;;
      esac
    done < <({ find "$home/data" "$home/out" "$home/docs" -type f 2>/dev/null || true; } | sort)
    snapshot_text ${text[@]+"${text[@]}"}; text=()
    if [ -f "$home/logs/events.jsonl" ]; then
      echo "== ${home#"$PRIV"/}/logs/events.jsonl"
      sed -E -e 's/,? ?"(ts|ms|seconds|duration_ms)": ?[0-9.]+//g' -e 's/\{, ?/{/' "$home/logs/events.jsonl"
    fi
  done
}

# setup <cli>: two synthetic users made through the CLI under test.
setup() {
  local cli="$1" s
  rm -rf "$PRIV"
  $cli users add alex --province ON
  $cli users add sam --province QC
  cp "$FIXTURE/profile.toml" "$PRIV/users/alex/profile.toml"
  cp "$FIXTURE/bank.csv" "$PRIV/users/alex/inbox/bank.csv"
  for s in $SCOPES; do
    printf '[[grants]]\nscope = "%s"\ngranted_on = 2026-10-18\nexpires_on = 2099-01-01\nvia = "hand"\n\n' "$s"
  done > "$PRIV/users/alex/consent.toml"
  # A scratch Downloads folder: `moneywiz sync` collects from it, never from the real one.
  mkdir -p "$PRIV/../Downloads"
  printf '[fetch]\ndownloads = "%s"\n' "$PRIV/../Downloads" > "$PRIV/settings.toml"
  $cli --user alex --today "$TODAY" import
  # Synthetic API balances, so the foreign cash, interest, crypto and net total sections have rows.
  cp "$FIXTURE/api-balances.json" "$PRIV/users/alex/data/api-balances.json"
}

export CASHCAST_PRIVATE="$PRIV" CASHCAST_LOG=on CASHCAST_POSTHOG_KEY="" CASHCAST_AGENT=1
unset CASHCAST_HOME CASHCAST_USER

# Each command's golden is NNN-<command as a file name>. Commands change the
# data, so --only still runs every command before the last one it picks (the
# state then matches the goldens), but compares only the picked ones.
commands=() keys=() picked=()
last=-1
while IFS= read -r line; do
  case "$line" in ''|'#'*) continue ;; esac
  commands+=("$line")
  keys+=("$(printf '%03d-%s' "${#commands[@]}" "$(printf '%s' "$line" | tr -c 'A-Za-z0-9_=.,-' '_' | cut -c1-80)")")
  if [ -z "$ONLY" ] || [[ "$line" == *"$ONLY"* ]]; then
    picked+=(1); last=$((${#commands[@]} - 1))
  else
    picked+=(0)
  fi
done < tests/parity/commands.txt
commands=("${commands[@]:0:$((last + 1))}")

# Run the CLI under test: setup, then each command once for the output and
# RUNS times for the time. Writes $WORK/out/<key>.out and <key>.us.
mkdir -p "$WORK/out"
{ setup "$B" 2>&1 | normalise | depath; } > "$WORK/out/setup.out" || true
for i in "${!commands[@]}"; do
  line="${commands[$i]}" key="${keys[$i]}"
  times=()
  for ((r = 0; r < RUNS; r++)); do
    t0=$(now)
    set +e
    # shellcheck disable=SC2086  # the command line splits into words on purpose
    out="$($B --today "$TODAY" $line 2>&1 < /dev/null)"
    code=$?
    set -e
    times+=($(( $(now) - t0 )))
    if [ "$r" -eq 0 ]; then
      { printf '%s\n' "$out" | normalise; echo "exit $code"; snapshot; } | depath > "$WORK/out/$key.out"
    fi
  done
  if [ "$RUNS" -eq 1 ]; then
    echo "${times[0]}" > "$WORK/out/$key.us"
  else
    printf '%s\n' "${times[@]}" | sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}' > "$WORK/out/$key.us"
  fi
done
# Each strategy section's markdown and tasks, and every GET route of the web
# server (status, type and body), on the data the run left behind. The state
# file holds a test token, so the owner-only routes answer as they do for the owner.
extra=()
if [ -z "$ONLY" ] || [[ "strategy sections" == *"$ONLY"* ]]; then
  for u in alex sam; do
    { echo "strategy sections $u"; "$ENGINE" strategy-sections --home "$PRIV/users/$u" --today "$TODAY" 2>&1 || true; } | depath > "$WORK/out/sections-$u.out"
    extra+=("sections-$u")
  done
fi
if [ -z "$ONLY" ] || [[ "web routes" == *"$ONLY"* ]]; then
  mkdir -p "$PRIV/out"
  printf '{"pid": 0, "port": 1, "url": "http://127.0.0.1:1/#t=t"}' > "$PRIV/out/web.json"
  w=0
  for route in "/api/users" "/api/plan?user=alex" "/api/plan?user=sam" "/api/plan?user=nobody" \
    "/api/docs?user=sam" "/api/docs?user=sam&q=canary" "/api/doc?user=sam&path=nope.md" \
    "/api/moneywiz-categories?user=alex" "/api/moneywiz-sync?user=alex" "/api/nope" "/api/users?x=1"; do
    w=$((w + 1))
    { echo "web $route"; "$ENGINE" web-get "$route" --today "$TODAY" --token t 2>&1 || true; } | depath > "$WORK/out/web-$w.out"
    extra+=("web-$w")
  done
  rm -f "$PRIV/out/web.json"
fi

leaks "$WORK/out"
bash "$ROOT/tests/parity/known-names.sh" "$WORK/out" || exit 3
if [ -z "$RECORD" ]; then  # never print a diff against a leaked golden
  # The stamp names the goldens, the checker and the leak pattern: any change checks again.
  stamp=""
  if [ -n "${PARITY_CACHE_DIR:-}" ]; then
    stamp="$PARITY_CACHE_DIR/golden-ok.$({ printf '%s\n' "$LEAK"; cat "$ROOT/tests/parity/known-names.sh" "$GOLDEN"/*.out; } | cksum | tr ' ' -)"
  fi
  if [ -z "$stamp" ] || [ ! -f "$stamp" ]; then
    leaks "$GOLDEN"
    bash "$ROOT/tests/parity/known-names.sh" "$GOLDEN" || exit 3
    [ -z "$stamp" ] || touch "$stamp"
  fi
fi

n_picked=0
for i in "${!commands[@]}"; do [ "${picked[$i]}" = 0 ] || n_picked=$((n_picked + 1)); done

if [ -n "$RECORD" ]; then
  mkdir -p "$GOLDEN"
  if [ -z "$ONLY" ]; then
    rm -f "$GOLDEN"/*.out
    cp "$WORK"/out/*.out "$GOLDEN"/
  else
    for i in "${!commands[@]}"; do [ "${picked[$i]}" = 0 ] || cp "$WORK/out/${keys[$i]}.out" "$GOLDEN"/; done
    for key in ${extra[@]+"${extra[@]}"}; do cp "$WORK/out/$key.out" "$GOLDEN"/; done
  fi
  [ -z "${PARITY_KEEP:-}" ] || echo "rust-parity: outputs kept in $WORK"
  echo "rust-parity: recorded $((n_picked + ${#extra[@]})) goldens in tests/parity/golden"
  exit 0
fi

mkdir -p .metrics
mismatch=0 n=0 total=0
printf '%-40s %8s  %s\n' command ms result
if [ -z "$ONLY" ] && ! diff -u "$GOLDEN/setup.out" "$WORK/out/setup.out" > "$WORK/setup.diff"; then
  mismatch=$((mismatch + 1)); echo "setup differs:"; cat "$WORK/setup.diff"
fi
compare() {  # compare <key>: one golden against its output; sets ok
  n=$((n + 1))
  if diff -u "$GOLDEN/$1.out" "$WORK/out/$1.out" > "$WORK/$1.diff" 2>&1; then ok=same; else ok=DIFF; mismatch=$((mismatch + 1)); fi
}
for i in "${!commands[@]}"; do
  [ "${picked[$i]}" = 1 ] || continue
  line="${commands[$i]}" key="${keys[$i]}"
  compare "$key"
  us=$(cat "$WORK/out/$key.us"); total=$((total + us))
  printf '%-40s %8d  %s\n' "${line:0:40}" $((us / 1000)) "$ok"
  [ "$ok" = same ] || sed 's/^/    /' "$WORK/$key.diff"
  printf '{"ts":"%s","event":"parity","cmd":"%s","b_ms":%d,"same":%s}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$line" $((us / 1000)) "$([ "$ok" = same ] && echo true || echo false)" >> .metrics/parity.jsonl
done
for key in ${extra[@]+"${extra[@]}"}; do
  compare "$key"
  printf '%-40s %8s  %s\n' "$(head -1 "$WORK/out/$key.out" | cut -c1-40)" - "$ok"
  [ "$ok" = same ] || head -40 "$WORK/$key.diff" | sed 's/^/    /'
done
[ -z "${PARITY_KEEP:-}" ] || echo "rust-parity: outputs kept in $WORK"
echo "rust-parity: $n commands, $mismatch mismatches vs goldens; total median time $((total / 1000)) ms (runs: $RUNS)"
[ "$mismatch" -eq 0 ]
