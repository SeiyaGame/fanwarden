#!/usr/bin/env bash
# Runs every quality check: the same script is used by the pre-commit hook and the CI.
#   scripts/check.sh          all checks
#   scripts/check.sh rust     Rust only
#   scripts/check.sh web      web UI only
set -euo pipefail
cd "$(dirname "$0")/.."
part="${1:-all}"

run_rust() {
  # On Windows, cargo usually lives in WSL: run the Rust checks there.
  if ! command -v cargo >/dev/null 2>&1; then
    if command -v wsl.exe >/dev/null 2>&1; then
      echo "==> cargo not found, running the Rust checks in WSL"
      local dir
      dir="$(wsl.exe wslpath -a "$(pwd -W 2>/dev/null || pwd)" | tr -d '\r')"
      wsl.exe bash -lc "cd '$dir' && CARGO_TARGET_DIR=\${CARGO_TARGET_DIR:-\$HOME/.cache/fan-control-target} scripts/check.sh rust"
      return
    fi
    echo "cargo not found: install Rust (https://rustup.rs)" >&2
    exit 1
  fi
  echo "==> cargo fmt"
  cargo fmt --all --check
  echo "==> cargo clippy"
  cargo clippy --all-targets --locked -- -D warnings
  echo "==> cargo test"
  cargo test --locked --quiet
}

run_web() {
  cd web
  [ -d node_modules ] || npm ci
  echo "==> eslint + prettier"
  npm run --silent lint
  echo "==> svelte-check"
  npm run --silent check
  echo "==> vitest"
  npm run --silent test
  cd ..
}

case "$part" in
  rust) run_rust ;;
  web) run_web ;;
  all) run_web && run_rust ;;
  *) echo "usage: $0 [all|rust|web]" >&2; exit 2 ;;
esac
echo "==> all checks passed"
