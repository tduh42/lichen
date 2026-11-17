#!/usr/bin/env bash
# Local checks. Usage: scripts/check.sh
# CHECK_SKIP_FUZZ=1 skips the fuzz smoke run; FUZZ_SECS sets its duration per target (30 s).
# CHECK_OFFLINE=1 checks advisories against the database already on disk.
set -euo pipefail
cd "$(dirname "$0")/.."

# no_std crates: no allocator, no floats.
NOSTD=(lc-meshcore lc-proto)
step() { printf '\n== %s\n' "$*"; }

step "one version (VERSION)"
version="$(tr -d '[:space:]' < VERSION)"
cargo_version="$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
ios_version="$(sed -n 's/^ *MARKETING_VERSION: "\(.*\)"/\1/p' apps/ios/project.yml)"
if [ "$cargo_version" != "$version" ] || [ "$ios_version" != "$version" ]; then
  echo "VERSION is $version, Cargo.toml has $cargo_version, apps/ios/project.yml has $ios_version" >&2
  exit 1
fi
echo "$version (Android reads the file)"

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

step "advisories and licences (deny.toml)"
if cargo deny --version >/dev/null 2>&1; then
  # CHECK_OFFLINE=1 uses the advisory database already on disk.
  cargo deny ${CHECK_OFFLINE:+--offline} check
else
  echo "skipped: install cargo-deny (brew install cargo-deny, or cargo install cargo-deny)"
fi

step "licences the apps show (assets/licences/licences.json)"
python3 -I scripts/licences.py --check

if [ "${CHECK_SKIP_FUZZ:-0}" != "1" ] && [ -f fuzz/Cargo.toml ]; then
  step "fuzz smoke run (${FUZZ_SECS:-30} s per target)"
  scripts/fuzz.sh "${FUZZ_SECS:-30}"
fi

printf '\nOK\n'
