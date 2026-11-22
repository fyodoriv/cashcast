#!/usr/bin/env bash
# Run the regression evals. CHECK_EVALS=1 scripts/check.sh calls this file.
# `grader: cli` cases run bin/cashcast on a synthetic user and check the exit
# code and output. `grader: judge` cases send their synthetic `response` to the
# judge route in config/models.yaml, which must be a local Ollama model: one
# criterion per call, temperature 0, a JSON verdict {"pass": bool, "reason": str}.
# Only text from evals/ goes to the model, never anything from private/. When
# Ollama or the model is missing, the judge cases are skipped with a one-line note.
#
#   evals/run.sh              run every case
#   evals/run.sh --agreement  judge verdicts vs the owner's labels in evals/labels.yaml
#   evals/run.sh --parse-verdict   read a judge reply on stdin, print "PASS|FAIL<TAB>reason"
#   evals/run.sh --route           print the judge route as JSON (refuses a non-local one)
#   evals/run.sh --cases           print every case as one JSON line
# Tests override: EVALS_HTTP (a command: URL [BODY] -> JSON on stdout; non-zero = no answer),
# EVALS_CONFIG, EVALS_CASES, EVALS_LABELS, EVALS_SKIP_CLI=1.
# A cli case with `no_python: true` runs with every python* hidden from PATH.
# Needs bash, jq, yq (mikefarah v4) and curl.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
CONFIG="${EVALS_CONFIG:-$ROOT/config/models.yaml}"
CASES="${EVALS_CASES:-$ROOT/evals/cases}"
LABELS="${EVALS_LABELS:-$ROOT/evals/labels.yaml}"
RUBRIC="$ROOT/evals/rubric.md"
PLACEHOLDER="owner: fill in"
JUDGE_SYSTEM='You grade one answer from a personal-finance assistant against one criterion. Decide only whether the answer meets that criterion, from what the answer says. If you cannot tell, it does not pass. Reply with JSON only: {"pass": true or false, "reason": "one sentence of evidence from the answer"}.'

# http_json URL [BODY] [TIMEOUT]: the JSON reply; exit 1 when nothing answers.
# No proxy: the request goes straight to the local Ollama.
http_json() {
  if [ -n "${EVALS_HTTP:-}" ]; then
    $EVALS_HTTP "$1" "${2:-}"
    return
  fi
  if [ -n "${2:-}" ]; then
    curl -sS --fail --noproxy '*' --max-time "${3:-10}" -H 'Content-Type: application/json' -d "$2" "$1"
  else
    curl -sS --fail --noproxy '*' --max-time "${3:-10}" "$1"
  fi
}

# load_cases: one JSON line per case, with its file name in "_file". A judge
# case's `bad_response` becomes its own case `<id>-bad` that the judge must
# fail (expect_pass: false), so a judge that always says PASS loses.
load_cases() {
  local f
  for f in "$CASES"/*.yaml; do
    yq -o=json -I=0 '.' "$f" | jq -c --arg file "$(basename "$f")" '
      . + {_file: $file} | (.bad_response // null) as $bad | del(.bad_response)
      | ., (if $bad then . + {id: (.id + "-bad"), response: $bad, expect_pass: false} else empty end)'
  done
}

# judge_route: the judge route as JSON. Refuses anything but Ollama on this computer.
judge_route() {
  local route url host
  route="$(yq -o=json -I=0 '.judge.route as $n | .routes[] | select(.name == $n)' "$CONFIG")"
  url="$(jq -r '.url // ""' <<< "$route")"
  host=""
  if [[ "$url" =~ ^[a-z]+://(\[([^]]+)\]|([^:/]+)) ]]; then host="${BASH_REMATCH[2]}${BASH_REMATCH[3]}"; fi
  if [ "$(jq -r '.provider // ""' <<< "$route")" != ollama ] || ! [[ "$host" =~ ^(127\.0\.0\.1|localhost|::1)$ ]]; then
    echo "evals: judge route '$(yq -r '.judge.route' "$CONFIG")' is not a local Ollama model; refusing to send cases to it" >&2
    return 1
  fi
  printf '%s\n' "$route"
}

# judge_ready ROUTE: empty when the judge model answers; otherwise why the judge cases are skipped.
judge_ready() {
  local url model tags
  url="$(jq -r .url <<< "$1")"; model="$(jq -r .model <<< "$1")"
  # A refused connection fails at once; a busy Ollama can take half a minute to list models.
  if ! tags="$(http_json "$url/api/tags" "" 60 2> /dev/null)" || ! jq -e . <<< "$tags" > /dev/null 2>&1; then
    echo "Ollama is not reachable at $url; start it with \`ollama serve\` (agent sandboxes may block local ports)"
    return
  fi
  if ! jq -e --arg m "$model" '[.models[]?.name] | index($m) or index($m + ":latest")' <<< "$tags" > /dev/null; then
    echo "model $model is not installed in Ollama (\`ollama pull $model\`)"
  fi
}

# rubric_json: rubric lines `1. **Name**: text` keyed by slug, for example `safe-with-money`.
rubric_json() {
  grep -E '^[0-9]+\. \*\*.+\*\*: .+$' "$RUBRIC" | jq -R -s -c '
    [split("\n")[] | select(length > 0) | capture("^[0-9]+\\. \\*\\*(?<name>.+?)\\*\\*: (?<text>.+)$")
     | {key: (.name | ascii_downcase | gsub(" "; "-")), value: "\(.name): \(.text)"}] | from_entries'
}

# parse_verdict: reads the judge's reply on stdin; prints "PASS|FAIL<TAB>reason".
# Anything but a boolean `pass` is an error (exit 1), never a pass.
parse_verdict() {
  jq -R -s -r '
    gsub("<think>[\\s\\S]*?</think>"; "")
    | (capture("(?<j>\\{[\\s\\S]*\\})").j // error("no JSON object in the reply"))
    | (try fromjson catch error("the reply is not JSON"))
    | if type == "object" and (.pass | type) == "boolean"
      then "\(if .pass then "PASS" else "FAIL" end)\t\(.reason // "" | tostring | gsub("^\\s+|\\s+$"; ""))"
      else error("no boolean pass in the reply") end' 2> /dev/null
}

# path_without_python DIR: fills DIR with links to every command on PATH
# except python*, so a case proves bin/cashcast needs no Python (Rust-only since PR 90).
path_without_python() {
  local dir="$1" d f name dirs
  mkdir -p "$dir"
  IFS=: read -ra dirs <<< "$PATH"
  for d in "${dirs[@]}"; do
    [ -d "$d" ] || continue
    for f in "$d"/*; do
      name="${f##*/}"
      case "$name" in python*) continue ;; esac
      if [ -x "$f" ] && [ ! -e "$dir/$name" ]; then ln -s "$f" "$dir/$name"; fi
    done
  done
}

# run_cli CASE: the problems of one code-graded case, one per line (none = pass).
run_cli() {
  local c="$1" tmp out code want n
  [ -z "${EVALS_SKIP_CLI:-}" ] || return 0
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/evals.XXXXXX")"
  (
    unset CASHCAST_HOME
    export CASHCAST_PRIVATE="$tmp/private" CASHCAST_LOG=off
    if [ "$(jq -r '.agent // false' <<< "$c")" = true ]; then export CASHCAST_AGENT=1; fi
    if [ "$(jq -r '.no_python // false' <<< "$c")" = true ]; then
      path_without_python "$tmp/bin"
      export PATH="$tmp/bin"
      # BASH_ENV runs a profile in every script, which can put python back on PATH.
      unset BASH_ENV ENV
    fi
    if ! setup="$("$ROOT/bin/cashcast" users add alex --province ON 2>&1)"; then
      echo "setup failed: bin/cashcast users add alex: $(tail -n 1 <<< "$setup")"
      exit 0
    fi
    jq -r '.grant // [] | .[] | "[[grants]]\nscope = \"\(.)\"\ngranted_on = 2026-10-18\nexpires_on = 2099-01-01\nvia = \"hand\"\n"' <<< "$c" \
      > "$tmp/grants"
    [ -s "$tmp/grants" ] && cp "$tmp/grants" "$tmp/private/users/alex/consent.toml"
    mapfile -t args < <(jq -r '.command[]' <<< "$c")
    code=0
    out="$("$ROOT/bin/cashcast" "${args[@]}" < /dev/null 2>&1)" || code=$?
    want="$(jq -r '.expect_exit // 0' <<< "$c")"
    [ "$code" = "$want" ] || echo "exit $code, expected $want"
    while IFS= read -r n; do
      [ -z "$n" ] || [[ "$out" == *"$n"* ]] || echo "missing: $n"
    done < <(jq -r '.expect_output // [] | .[]' <<< "$c")
  )
  rm -rf "$tmp"
}

# judge_case CASE ROUTE RUBRIC: one call per criterion (the case's `expect`
# lines, then the rubric criteria it names). Prints the failures, one per line.
# Exit 1 when the judge does not answer: that is a skip, not a verdict.
judge_case() {
  local c="$1" route="$2" rubric="$3" url model criterion prompt body reply verdict
  url="$(jq -r .url <<< "$route")"; model="$(jq -r .model <<< "$route")"
  while IFS= read -r criterion; do
    prompt="$(jq -r --arg crit "$criterion" '
      "Criterion: \($crit)\n\n"
      + ([(.fixture // {}) | to_entries[] | "Context: \(.key): \(.value)\n"] | join(""))
      + "User message:\n\(.input)\n\nAssistant answer:\n\(.response)"' <<< "$c")"
    body="$(jq -n -c --arg model "$model" --arg sys "$JUDGE_SYSTEM" --arg user "$prompt" '{
      model: $model, messages: [{role: "system", content: $sys}, {role: "user", content: $user}],
      stream: false, think: false,
      format: {type: "object", properties: {pass: {type: "boolean"}, reason: {type: "string"}}, required: ["pass", "reason"]},
      options: {temperature: 0, seed: 0}}')"
    reply="$(http_json "$url/api/chat" "$body" 300)" || return 1
    if ! verdict="$(jq -r '.message.content // ""' <<< "$reply" 2> /dev/null | parse_verdict)"; then
      verdict="$(printf 'FAIL\tjudge reply unreadable (no boolean pass in the reply)')"
    fi
    if [[ "$verdict" == FAIL* ]]; then
      printf '%s: %s\n' "$(cut -d: -f1 <<< "$criterion" | cut -c1-60)" "${verdict#*$'\t'}"
    fi
  done < <(jq -r --argjson rubric "$rubric" '(.expect // []) + [(.rubric // [])[] | $rubric[.]] | .[]' <<< "$c")
}

case "${1:-}" in
  --parse-verdict) parse_verdict; exit ;;
  --route) judge_route; exit ;;
  --cases) load_cases; exit ;;
esac
agree_only=""; [ "${1:-}" = --agreement ] && agree_only=1

failed=0 passed=0 judged=()
while IFS= read -r c; do
  id="$(jq -r '.id // ""' <<< "$c")"
  for key in id suite skill should grader; do
    if [ "$(jq --arg k "$key" 'has($k)' <<< "$c")" != true ]; then
      echo "FAIL $(jq -r ._file <<< "$c"): case without $key"; failed=$((failed + 1))
    fi
  done
  skill="$(jq -r '.skill // ""' <<< "$c")"
  if [ ! -f "$ROOT/skills/$skill/SKILL.md" ]; then echo "FAIL $id: no skill $skill"; failed=$((failed + 1)); fi
  grader="$(jq -r '.grader // ""' <<< "$c")"
  if [ "$grader" = cli ]; then
    [ -z "$agree_only" ] || continue
    problems="$(run_cli "$c")"
    if [ -z "$problems" ]; then
      echo "PASS $id"; passed=$((passed + 1))
    else
      echo "FAIL $id: $(paste -sd';' - <<< "$problems" | sed 's/;/; /g')"; failed=$((failed + 1))
    fi
  elif [ "$(jq '(.response // "") != "" and ((.expect // []) | length > 0)' <<< "$c")" != true ]; then
    echo "FAIL $id: judge case needs a synthetic response and expect lines"; failed=$((failed + 1))
  else
    judged+=("$c")
  fi
done < <(load_cases)

route="$(judge_route)"
why="$(judge_ready "$route")"
declare -A verdict=()
order=()
if [ -z "$why" ] && [ ${#judged[@]} -gt 0 ]; then
  rubric="$(rubric_json)"
  echo "judge: $(jq -r .model <<< "$route") at $(jq -r .url <<< "$route") (route $(jq -r .name <<< "$route"), temperature 0)"
  for c in "${judged[@]}"; do
    id="$(jq -r .id <<< "$c")"
    if ! problems="$(judge_case "$c" "$route" "$rubric")"; then
      why="the judge stopped answering at $id"; break
    fi
    order+=("$id")
    if [ -z "$problems" ]; then verdict[$id]=true; else verdict[$id]=false; fi
    if [ "$(jq -r '.expect_pass != false' <<< "$c")" = true ]; then
      if [ -z "$problems" ]; then echo "PASS $id"; else echo "FAIL $id: $(paste -sd';' - <<< "$problems" | sed 's/;/; /g')"; fi
    elif [ -n "$problems" ]; then
      echo "PASS $id: the judge failed the bad response"
    else
      echo "FAIL $id: the judge passed a bad response"
    fi
  done
fi
skipped=""
[ -z "$why" ] || skipped="; $(( ${#judged[@]} - ${#order[@]} )) judge cases skipped: $why"

if [ -n "$agree_only" ]; then
  # Judge vs owner labels, k of n, over the labels the owner filled in (pass or fail).
  labels="$(yq -o=json -I=0 '. // {}' "$LABELS")"
  verdicts="$(for id in "${order[@]+"${order[@]}"}"; do printf '%s\t%s\n' "$id" "${verdict[$id]}"; done |
    jq -R -s -c '[split("\n")[] | select(length > 0) | split("\t") | {key: .[0], value: (.[1] == "true")}] | from_entries')"
  jq -n -r --argjson v "$verdicts" --argjson l "$labels" --argjson order "$(printf '%s\n' "${order[@]+"${order[@]}"}" | jq -R -s -c 'split("\n") | map(select(length > 0))')" '
    ($l | with_entries(select((.value | tostring | ascii_downcase) as $x | $x == "pass" or $x == "fail"))
        | map_values((tostring | ascii_downcase) == "pass")) as $owner
    | [$order[] | . as $k | select($owner | has($k))] as $scored
    | [$order[] | . as $k | select($owner | has($k) | not)] as $open
    | "judge vs owner labels: \([$scored[] | select($v[.] == $owner[.])] | length) of \($scored | length) agree"
      + (if ($open | length) > 0 then " (\($open | length) not labelled yet: \($open | join(", ")); fill them in evals/labels.yaml)" else "" end),
      ($scored[] | select($v[.] != $owner[.]) as $k
        | "  DISAGREE \($k): judge \(if $v[$k] then "pass" else "fail" end), owner \(if $owner[$k] then "pass" else "fail" end)")'
  [ -z "$skipped" ] || echo "${skipped#; }"
  exit 0
fi

judge_passed=0
for c in "${judged[@]+"${judged[@]}"}"; do
  id="$(jq -r .id <<< "$c")"
  [ -n "${verdict[$id]:-}" ] || continue
  want="$(jq -r '.expect_pass != false' <<< "$c")"
  if [ "${verdict[$id]}" = "$want" ]; then judge_passed=$((judge_passed + 1)); else failed=$((failed + 1)); fi
done
printf '\nevals: %s code-graded passed, %s judge-graded passed, %s failed%s\n' "$passed" "$judge_passed" "$failed" "$skipped"
[ "$failed" -eq 0 ]
