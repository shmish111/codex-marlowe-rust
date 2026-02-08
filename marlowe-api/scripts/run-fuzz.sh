#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo-fuzz >/dev/null 2>&1; then
  echo "cargo-fuzz is not installed."
  echo "Install: cargo install cargo-fuzz"
  exit 1
fi

target="${1:-parse_yaml}"

cargo-fuzz run "$target"
