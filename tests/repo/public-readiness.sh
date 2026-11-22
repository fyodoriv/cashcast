#!/usr/bin/env bash
# scripts/public-readiness.sh passes a clean synthetic repo and fails each kind
# of leak it guards (SECURITY.md, "What may be public"). It never prints a value.
set -euo pipefail
cd "$(dirname "$0")/../.."
src="$PWD"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/readiness-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
# The owner's pattern file must not change the result here.
export OSS_READINESS_ENV="$tmp/no-pattern-file"

clean_repo() {
  local r="$1"
  mkdir -p "$r/scripts" "$r/config" "$r/.github/workflows"
  cp scripts/public-readiness.sh scripts/python-lines.sh "$r/scripts/"
  printf 'licence\n' > "$r/LICENSE"
  printf 'rules\n' > "$r/SECURITY.md"
  printf '# Contributing\n\n## Report problems and ideas\n\nyes\n' > "$r/CONTRIBUTING.md"
  printf 'canary CANARY-RD01@example.com\n' > "$r/README.md"
  cat > "$r/.github/workflows/ci.yml" <<'EOF'
on: [pull_request]
permissions:
  contents: read
jobs:
  a:
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4
EOF
  git -C "$r" init -q
  git -C "$r" add -A
  git -C "$r" -c core.hooksPath=/dev/null -c user.name=t -c user.email=t@users.noreply.github.com commit -qm "test: init"
}

expect() { # expect <case> <want-exit> <check-name>
  local r="$tmp/$1" got=0 out
  git -C "$r" add -A
  out="$(bash "$r/scripts/public-readiness.sh" 2>&1)" || got=$?
  if [ "$2" = 0 ]; then
    [ "$got" = 0 ] || { echo "FAIL readiness $1: want ready, got:"; echo "$out"; exit 1; }
  else
    [ "$got" != 0 ] && grep -q "^FAIL  $3 " <<<"$out" || { echo "FAIL readiness $1: want FAIL $3, got:"; echo "$out"; exit 1; }
    grep -q 'CANARY-SECRET' <<<"$out" && { echo "FAIL readiness $1: printed a matched value"; exit 1; }
  fi
  echo "ok readiness $1"
}

clean_repo "$tmp/clean"
expect clean 0 -

clean_repo "$tmp/private"
mkdir -p "$tmp/private/private/users/a"; echo x > "$tmp/private/private/users/a/profile.toml"
git -C "$tmp/private" add -f private
expect private 1 private-paths

# Synthetic values are built at runtime so this file holds none (the readiness
# scan reads this file too).
four="4111"; one="1111"; card="$four$one$one$one"
sin="046 454 2""86"; mail="person.CANARY-SECRET@""gmail.com"; phone="(416) 555-""0199"

clean_repo "$tmp/card"
# A Luhn-valid synthetic number (the Visa test card), not allowed.
echo "card $four $one $one $one CANARY-SECRET" > "$tmp/card/notes.md"
expect card 1 card-numbers

clean_repo "$tmp/card-allowed"
echo "card $card" > "$tmp/card-allowed/notes.md"
echo "card notes.md $card" > "$tmp/card-allowed/config/public-readiness-allow.txt"
expect card-allowed 0 -

clean_repo "$tmp/sin"
echo "sin $sin CANARY-SECRET" > "$tmp/sin/notes.md"
expect sin 1 sin-numbers

clean_repo "$tmp/email"
echo "mail $mail" > "$tmp/email/notes.md"
expect email 1 emails

clean_repo "$tmp/phone"
echo "call $phone CANARY-SECRET" > "$tmp/phone/notes.md"
expect phone 1 phones

clean_repo "$tmp/workflow"
sed -i.bak 's/@11bd71901bbe5b1630ceea73d27597364c9af683 # v4/@v4/' "$tmp/workflow/.github/workflows/ci.yml" && rm "$tmp/workflow/.github/workflows/ci.yml.bak"
echo "  b: { if: \${{ secrets.X }} }" >> "$tmp/workflow/.github/workflows/ci.yml"
expect workflow 1 workflows

clean_repo "$tmp/licence"
rm "$tmp/licence/LICENSE"; git -C "$tmp/licence" rm -q --cached LICENSE
expect licence 1 public-files

clean_repo "$tmp/python"
mkdir -p "$tmp/python/src"; echo "print('x')" > "$tmp/python/src/a.py"
expect python 1 rust-only

if command -v gitleaks >/dev/null 2>&1; then
  clean_repo "$tmp/secret"
  # Built at run time, so this file holds no secret-shaped literal.
  p="gh"; printf 'token = "%sp_%s"\n' "$p" "Zq8Lm3Rt7Wx2Yv5Nb9Kc4Hd6Jf1Gs0Ae3Pu8" > "$tmp/secret/creds.ini"
  expect secret 1 secrets-tree
fi
