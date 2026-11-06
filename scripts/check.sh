#!/usr/bin/env bash
# Local checks. Usage: scripts/check.sh
# CHECK_SKIP_FUZZ=1 skips the fuzz smoke run; FUZZ_SECS sets its duration per target (30 s).
set -euo pipefail
cd "$(dirname "$0")/.."

# no_std crates: no allocator, no floats.
NOSTD=(lc-meshcore lc-proto)
step() { printf '\n== %s\n' "$*"; }

step "fmt"
cargo fmt --all -- --check

step "clippy"
cargo clippy --workspace --all-targets -- -D warnings

step "test"
cargo test --workspace --quiet

step "no_std (thumbv7em-none-eabihf, no allocator)"
if rustup target list --installed | grep -q thumbv7em-none-eabihf; then
  pkgs=()
  for p in "${NOSTD[@]}"; do pkgs+=(-p "$p"); done
  cargo build --quiet --target thumbv7em-none-eabihf "${pkgs[@]}"
else
  echo "skipped: rustup target add thumbv7em-none-eabihf"
fi

step "no floats in no_std crates"
found=0
for p in "${NOSTD[@]}"; do
  if grep -rnwE 'f32|f64' --include='*.rs' "crates/$p/src" | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//'; then
    found=1
  fi
done
if [ "$found" -ne 0 ]; then echo "float found in a no_std crate" >&2; exit 1; fi

if [ "${CHECK_SKIP_FUZZ:-0}" != "1" ] && [ -f fuzz/Cargo.toml ]; then
  step "fuzz smoke run (${FUZZ_SECS:-30} s per target)"
  scripts/fuzz.sh "${FUZZ_SECS:-30}"
fi

printf '\nOK\n'
