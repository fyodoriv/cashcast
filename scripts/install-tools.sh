#!/usr/bin/env bash
# Installs the tools cashcast needs when they are missing (scripts/setup.sh
# runs it). A new Mac has git (Xcode tools) and sqlite3, not much else:
# Homebrew installs jq, yq, Node and poppler (PDF statements, optional),
# rustup installs Rust, and corepack or npm installs pnpm. Only Homebrew
# itself asks for the Mac password, so it stays the person's one-line step.
# Safe to run again: it installs only what is missing.
set -euo pipefail
cd "$(dirname "$0")/.."

has() { command -v "$1" >/dev/null 2>&1; }

formulas=()
for pair in jq:jq yq:yq sqlite3:sqlite node:node; do
  has "${pair%%:*}" || formulas+=("${pair#*:}")
done
if [ "${#formulas[@]}" -gt 0 ]; then
  if has brew; then
    echo "Installing with Homebrew: ${formulas[*]}"
    brew install --quiet "${formulas[@]}"
  else
    echo "✗ Missing: ${formulas[*]}, and Homebrew is not installed. Install Homebrew (it asks for your Mac password):"
    # shellcheck disable=SC2016 # printed for the person to copy, not run here
    echo '  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
    echo "  Then run the setup again."
    exit 1
  fi
fi
if ! has pdftotext && has brew; then
  echo "Installing poppler (reads PDF bank statements)"
  brew install --quiet poppler || echo "note: poppler did not install; PDF statements are skipped"
fi

[ -x "$HOME/.cargo/bin/cargo" ] && PATH="$HOME/.cargo/bin:$PATH"
if ! has cargo || ! cargo --version >/dev/null 2>&1; then
  if has rustup; then
    echo "Installing the stable Rust toolchain"
    rustup default stable
  else
    echo "Installing Rust (rustup, no password needed)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    PATH="$HOME/.cargo/bin:$PATH"
  fi
fi

if ! has pnpm; then
  version="$(sed -n 's/.*"packageManager": *"pnpm@\([0-9.]*\)".*/\1/p' web/package.json)"
  echo "Installing pnpm ${version:-latest}"
  { corepack enable >/dev/null 2>&1 && has pnpm; } || npm install --global --silent "pnpm@${version:-latest}"
fi
echo "tools: ready"
