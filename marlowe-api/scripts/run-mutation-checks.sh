#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo-mutants >/dev/null 2>&1; then
  echo "cargo-mutants is not installed."
  echo "Install: cargo install cargo-mutants"
  exit 1
fi

cargo-mutants --file mutants.toml
