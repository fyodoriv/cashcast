#!/usr/bin/env bash
# Every test a user story names in its case table exists: a Rust `fn NAME(`
# or a shell story function run by `run_test NAME`. Stories opt in with a line
# that names this file. A test listed after a file path belongs to that file.
set -euo pipefail
cd "$(dirname "$0")/../.."
missing=0
checked=0
story=""
while IFS= read -r line; do
  case "$line" in "## US-"*) story="${line%%:*}" ;; esac
  [[ "$line" == "|"* ]] || continue
  file=""
  while read -r tok; do
    case "$tok" in
      *.rs | *.sh) file="$tok" ;;
      *_*_*)
        [ -n "$file" ] || continue
        [[ "$tok" =~ ^[a-z0-9_]+$ ]] || continue
        checked=$((checked + 1))
        if ! grep -qE "^(fn $tok\(|$tok\(\) \{|run_test $tok\$)" "$file" 2> /dev/null; then
          echo "FAIL: ${story#\#\# } names $tok in $file, which has no such test" >&2
          missing=$((missing + 1))
        fi
        ;;
    esac
  done < <(grep -oE '`[^`]+`' <<< "$line" | tr -d '`')
done < <(awk '/^## US-/{on=0; print; next} /tests\/repo\/story-tests\.sh/{on=1} on' docs/user-stories.md)
[ "$checked" -gt 0 ] || { echo "FAIL: no story names a test for this check" >&2; exit 1; }
[ "$missing" -eq 0 ] || exit 1
echo "ok each of $checked tests the opted-in user stories name exists"
