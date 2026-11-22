#!/usr/bin/env bash
# One-prompt setup, as in agentbrew, dotfiles and careerbrew: the README opens
# with one agent prompt that INSTALL.md carries out.
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }
has() { grep -qF -- "$2" <<< "$1" || fail "missing: $2"; }

# The first fenced block of the README is the agent prompt, in a `text` block.
first_lang="$(grep -m1 -oE '^```[a-z]*' README.md)"
[ "$first_lang" = '```text' ] || fail "the README must open with a text prompt, not $first_lang"
prompt="$(awk '/^```text$/{on=1; next} on && /^```/{exit} on' README.md)"
has "$prompt" "blob/HEAD/INSTALL.md"
has "$prompt" "updates an existing clone"
for step in "only use cashcast or also contribute" "scripts/doctor.sh" "fix every failing line" \
  "propose adopt or merge" "extend cashcast instead of building local copies" \
  'start cashcast for the first time with `/cashcast`' "walk me through" "offer an issue or PR"; do
  has "$prompt" "$step"
done
[ "$(grep -n -m1 '^```text' README.md | cut -d: -f1)" -lt "$(grep -n -m1 '^## What it reads' README.md | cut -d: -f1)" ] \
  || fail "the prompt must come before ## What it reads"
echo "ok the README opens with one agent prompt that installs through INSTALL.md"

line="$(awk '/^```bash$/{on=1; next} on && /^```/{exit} on' README.md)"
[ "$(wc -l <<< "$line")" -eq 1 ] || fail "the shell setup must be one line"
[[ "$line" == "[ -d ~/apps/cashcast ] || git clone "* ]] || fail "setup must be safe to rerun: $line"
[[ "$line" == *"git pull --ff-only"* && "$line" == *"scripts/setup.sh"* && "$line" == *scripts/doctor.sh ]] \
  || fail "setup must pull, run setup and end with the doctor: $line"
echo "ok the one-line setup is safe to rerun and ends with the doctor"

install="$(cat INSTALL.md)"
for part in "pull --ff-only" "Use and contribute" "### 4b. Contributor setup" "## 5. Check and fix" "0 failed" \
  "agentbrew sync" "## 6. Audit what the machine already has" "**adopt**" "**merge**" \
  "## 7. Start cashcast for the first time" "/cashcast" "bin/cashcast" "offer an issue or a PR"; do
  has "$install" "$part"
done
steps="$(grep -oE '^## [0-9]+\. ' INSTALL.md | grep -oE '[0-9]+')"
[ "$steps" = "$(seq 1 "$(wc -l <<< "$steps")")" ] || fail "INSTALL.md steps must count 1, 2, 3, ..."
for ref in $(grep -oE 'step [0-9]+' INSTALL.md | grep -oE '[0-9]+' | sort -u); do
  grep -qx "$ref" <<< "$steps" || fail "INSTALL.md names a missing step $ref"
done
[ "$(grep -n -m1 '^## 7. Start cashcast' INSTALL.md | cut -d: -f1)" -lt "$(grep -n -m1 '^## 8. Finish' INSTALL.md | cut -d: -f1)" ] \
  || fail "step 7 must come before step 8"
echo "ok INSTALL.md carries out each part of the prompt"

head="$(sed '/^## What it reads/q' README.md; cat INSTALL.md)"
for script in $(grep -oE '(scripts|bin)/[A-Za-z0-9_.-]+' <<< "$head" | sort -u); do
  [ -f "$script" ] || fail "named but missing: $script"
done
echo "ok every script the prompt and playbook name exists"
