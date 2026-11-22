#!/usr/bin/env bash
# Public readiness: can this tree (and, with --history, its history) be public?
#   scripts/public-readiness.sh            check the tracked files (gate step `readiness`)
#   scripts/public-readiness.sh --history  also check every commit reachable from HEAD
# It prints check names, counts and file names, never a matched value.
# Exit 0 only when every check passes. Rules: SECURITY.md, "What may be public".
#
# The owner's private-pattern check runs only where its pattern file exists
# (~/.config/oss-readiness/oss-readiness.env, never in the repo). In CI it
# prints "owner-only: skipped"; the owner's pre-push hook stays the authority.
set -euo pipefail
cd "$(dirname "$0")/.."
history=0
[ "${1:-}" = --history ] && history=1
allow=config/public-readiness-allow.txt
fail=0
pass() { printf 'ok    %-18s %s\n' "$1" "$2"; }
bad() { printf 'FAIL  %-18s %s\n' "$1" "$2"; fail=1; }

# Tracked files, minus lockfiles (hashes and versions only).
files=()
while IFS= read -r f; do
  [ -f "$f" ] || continue
  case "$f" in *pnpm-lock.yaml | *Cargo.lock | "$allow") continue ;; esac
  files+=("$f")
done < <(git ls-files)

# 1. No private paths: owner data, secrets, MoneyWiz databases.
n=0
for f in "${files[@]}"; do
  case "$f" in
    private/* | */private/* | .env | .env.* | *.sqlite | *.sqlite3 | *.db | *.moneywiz* | *.p12 | *.pem | *.key | .metrics/*)
      [ "$f" = .env.example ] && continue
      echo "      private path tracked: $f"; n=$((n + 1)) ;;
  esac
done
[ "$n" = 0 ] && pass private-paths "0 private paths tracked" || bad private-paths "$n private path(s) tracked"

# 2. gitleaks on the tracked tree (and history with --history).
if command -v gitleaks >/dev/null 2>&1; then
  tree="$(mktemp -d "${TMPDIR:-/tmp}/readiness.XXXXXX")"
  trap 'rm -rf "$tree"' EXIT
  git ls-files -z | while IFS= read -r -d "" f; do [ -f "$f" ] && printf "%s\0" "$f"; done | tar --null -T - -cf - | tar -xf - -C "$tree"
  if gitleaks dir --no-banner --redact -l error "$tree" >/dev/null 2>&1; then pass secrets-tree "gitleaks: no leaks"; else bad secrets-tree "gitleaks found secrets (run: gitleaks dir --redact .)"; fi
  if [ "$history" = 1 ]; then
    if gitleaks git --no-banner --redact -l error . >/dev/null 2>&1; then pass secrets-history "gitleaks: no leaks in $(git rev-list --count HEAD) commit(s)"; else bad secrets-history "gitleaks found secrets in history"; fi
  fi
else
  bad secrets-tree "gitleaks is not installed (brew install gitleaks)"
fi

# 3. Personal identifiers: emails, card numbers, SIN-shaped numbers, phone numbers.
#    Synthetic values live in fixtures as canaries; real-looking ones need an allow line.
allowed() { [ -f "$allow" ] && grep -qxF -- "$1" "$allow"; }
n=0
while IFS= read -r hit; do
  f="${hit%%:*}"; v="${hit#*:}"
  case "$v" in *@example.com | *@example.org | *@users.noreply.github.com | noreply@anthropic.com | git@github.com) continue ;; esac
  allowed "email $f $v" && continue
  echo "      email in $f"; n=$((n + 1))
done < <(git grep -IoE -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' -- "${files[@]}" 2>/dev/null | sort -u || true)
[ "$n" = 0 ] && pass emails "0 personal emails" || bad emails "$n email(s) outside example.com/noreply"

luhn() { # luhn DIGITS -> exit 0 when the Luhn check passes
  local s=0 i d alt=0 x="$1"
  for ((i = ${#x} - 1; i >= 0; i--)); do
    d=${x:i:1}
    if [ "$alt" = 1 ]; then d=$((d * 2)); [ "$d" -gt 9 ] && d=$((d - 9)); fi
    s=$((s + d)); alt=$((1 - alt))
  done
  [ $((s % 10)) = 0 ]
}
n=0
while IFS=: read -r f v; do
  [ -n "$v" ] || continue
  d="${v//[ -]/}"
  [ -z "${d//"${d:0:1}"/}" ] && continue # one repeated digit: a placeholder
  luhn "$d" || continue
  allowed "card $f $d" && continue
  echo "      card-shaped number in $f"; n=$((n + 1))
done < <(git grep -IoE '(^|[^0-9/A-Za-z_.])[3-6][0-9]{3}[ -]?[0-9]{4}[ -]?[0-9]{4}[ -]?[0-9]{3,4}([^0-9]|$)' -- "${files[@]}" 2>/dev/null \
  | sed -E 's/^([^:]+):[^0-9]*([0-9][0-9 -]*[0-9]).*/\1:\2/' | sort -u || true)
[ "$n" = 0 ] && pass card-numbers "0 Luhn-valid card numbers" || bad card-numbers "$n Luhn-valid number(s); add 'card <file> <digits>' to $allow only for a synthetic test value"

n=0
while IFS=: read -r f v; do
  d="${v//[ -]/}"
  luhn "$d" || continue
  allowed "sin $f $d" && continue
  echo "      SIN-shaped number in $f"; n=$((n + 1))
done < <(git grep -IoE '(^|[^0-9])[0-9]{3}[ -][0-9]{3}[ -][0-9]{3}([^0-9]|$)' -- "${files[@]}" 2>/dev/null \
  | sed -E 's/^([^:]+):[^0-9]*([0-9]{3}[ -][0-9]{3}[ -][0-9]{3}).*/\1:\2/' | sort -u || true)
[ "$n" = 0 ] && pass sin-numbers "0 SIN-shaped numbers" || bad sin-numbers "$n SIN-shaped number(s)"

n=0
while IFS= read -r f; do
  allowed "phone $f" && continue
  echo "      phone number in $f"; n=$((n + 1))
done < <(git grep -IlE '(^|[^0-9])\(?[2-9][0-9]{2}\)?[-. ][2-9][0-9]{2}[-. ][0-9]{4}([^0-9]|$)' -- "${files[@]}" 2>/dev/null || true)
[ "$n" = 0 ] && pass phones "0 phone numbers" || bad phones "$n file(s) with phone numbers"

# 4. The owner's private pattern (employer and internal names). Owner machine only.
env_file="${OSS_READINESS_ENV:-$HOME/.config/oss-readiness/oss-readiness.env}"
if [ -r "$env_file" ]; then
  n=$( (
    # shellcheck disable=SC1090
    . "$env_file"
    [ -n "${OSS_READINESS_INTERNAL_PATTERN:-}" ] || { echo missing; exit 0; }
    if [ "$history" = 1 ]; then git log -p --all | grep -icE -- "$OSS_READINESS_INTERNAL_PATTERN" || true
    else git grep -IicE -- "$OSS_READINESS_INTERNAL_PATTERN" -- "${files[@]}" | awk -F: '{s+=$NF} END {print s+0}'; fi
    [ "$history" = 1 ] && { git log --format="%an %ae %cn %ce %B" --all | grep -icE -- "$OSS_READINESS_INTERNAL_PATTERN" >/dev/null && echo meta; } || true
  ) | tr '\n' ' ')
  case "$n" in
    "0 ") pass owner-pattern "0 private-pattern hits$([ "$history" = 1 ] && echo ' in history')" ;;
    missing*) bad owner-pattern "pattern file has no OSS_READINESS_INTERNAL_PATTERN" ;;
    *) bad owner-pattern "private-pattern hits: ${n% }" ;;
  esac
else
  echo "skip  owner-pattern      owner-only: skipped (no pattern file here)"
fi

# 5. Commit identity (history mode): authors use a noreply address.
if [ "$history" = 1 ]; then
  n=$(git log --format='%ae%n%ce' | sort -u | grep -cvE '@users\.noreply\.github\.com$|^noreply@github\.com$|^noreply@anthropic\.com$' || true)
  [ "$n" = 0 ] && pass commit-emails "every author and committer uses a noreply address" || bad commit-emails "$n commit email(s) are not noreply"
fi

# 6. Workflow safety: read-only token, no pull_request_target, no secrets, actions pinned to a full SHA.
n=0
for w in .github/workflows/*.yml; do
  [ -f "$w" ] || continue
  grep -qE '^[^#]*pull_request_target' "$w" && { echo "      $w uses pull_request_target"; n=$((n + 1)); }
  grep -qE '^permissions:[[:space:]]*$' "$w" && grep -qE '^[[:space:]]+contents:[[:space:]]*read' "$w" || { echo "      $w lacks top-level permissions: contents: read"; n=$((n + 1)); }
  grep -qE '^[^#]*\$\{\{[[:space:]]*secrets\.' "$w" && { echo "      $w uses secrets"; n=$((n + 1)); }
  while IFS= read -r u; do
    [[ "$u" =~ @[0-9a-f]{40}$ ]] || [[ "$u" == ./* ]] || { echo "      $w: action not pinned to a SHA: $u"; n=$((n + 1)); }
  done < <(sed -nE 's/^[[:space:]-]*uses:[[:space:]]*([^[:space:]#]+).*/\1/p' "$w")
done
[ "$n" = 0 ] && pass workflows "read-only, no secrets, pinned actions" || bad workflows "$n workflow problem(s)"

# 7. Required public files and the Rust-only backend.
n=0
for f in LICENSE SECURITY.md CONTRIBUTING.md; do git ls-files --error-unmatch "$f" >/dev/null 2>&1 || { echo "      $f is not tracked"; n=$((n + 1)); }; done
grep -q '^## Report problems and ideas' CONTRIBUTING.md 2>/dev/null || { echo "      CONTRIBUTING.md lacks '## Report problems and ideas'"; n=$((n + 1)); }
[ "$n" = 0 ] && pass public-files "LICENSE, SECURITY.md, CONTRIBUTING.md" || bad public-files "$n missing"
py="$(scripts/python-lines.sh 2>/dev/null | tail -1)"
[ "$py" = "python-lines: 0" ] && pass rust-only "0 Python lines" || bad rust-only "${py:-python-lines failed}"

[ "$fail" = 0 ] && echo "public-readiness: ready" || { echo "public-readiness: NOT ready" >&2; exit 1; }
