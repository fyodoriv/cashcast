#!/usr/bin/env bash
# One-line setup: install everything, then prove the machine is ready.
# Safe to run again; each step skips work that is already done.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

step "Install missing tools"
if [ "${SETUP_SKIP_TOOLS:-0}" = 1 ]; then
  echo "skipped (SETUP_SKIP_TOOLS=1)"
else
  scripts/install-tools.sh
  # Tools it just installed (Rust under ~/.cargo/bin) for the steps below.
  [ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH" && export PATH
fi

step "Install dependencies"
if [ "${SETUP_SKIP_DEPS:-0}" = 1 ]; then
  echo "skipped (SETUP_SKIP_DEPS=1)"
else
  if [ -f package.json ]; then
    if [ -f pnpm-lock.yaml ] || grep -q '"packageManager": *"pnpm' package.json; then
      corepack enable >/dev/null 2>&1 || true
      pnpm install --frozen-lockfile 2>/dev/null || pnpm install
    else
      npm ci 2>/dev/null || npm install
    fi
  fi
  if [ -f web/package.json ]; then
    echo "Installing and building the web app"
    (cd web && pnpm install --frozen-lockfile && pnpm -s build)
  fi
  if [ -f engine/Cargo.toml ]; then
    echo "Building the engine (the first build takes a few minutes)"
    cargo build --release --quiet --manifest-path engine/Cargo.toml
  fi
fi

step "Create local folders (never committed)"
mkdir -p .metrics
mkdir -p private private/users
chmod 700 private private/users

step "Install git hooks"
if [ -f lefthook.yml ] && command -v lefthook >/dev/null 2>&1; then
  lefthook install >/dev/null 2>&1 || echo "note: lefthook install skipped (a global core.hooksPath may own the hooks)"
fi

step "Put cashcast on your PATH"
link="$HOME/.local/bin/cashcast"
current="$(readlink "$link" 2>/dev/null || true)"
if [ "${SETUP_SKIP_LINK:-0}" = 1 ]; then
  echo "skipped (SETUP_SKIP_LINK=1)"
elif [ "$(git rev-parse --git-dir 2>/dev/null)" != "$(git rev-parse --git-common-dir 2>/dev/null)" ]; then
  echo "skipped: this is a git worktree; link from the main checkout (its private/ holds your data)"
elif [ -e "$link" ] && [ ! -L "$link" ]; then
  echo "skipped: $link exists and is not a link"
elif [ -n "$current" ] && [ -e "$current" ] && [ "$current" != "$PWD/bin/cashcast" ]; then
  echo "skipped: $link already points to $current"
else
  mkdir -p "$HOME/.local/bin" && ln -sf "$PWD/bin/cashcast" "$link"
  echo "linked $link -> bin/cashcast"
  case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "note: add \$HOME/.local/bin to your PATH to run \`cashcast\`" ;; esac
fi

step "Health check"
scripts/doctor.sh

printf '\nReady. Next: run `cashcast` (or bin/cashcast). In an agent: /cashcast.\n'
