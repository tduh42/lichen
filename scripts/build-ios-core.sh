#!/usr/bin/env bash
# Builds the lichen core for iOS: static libraries for iPhone and simulator (arm64), the Swift
# bindings (apps/ios/Generated) and apps/ios/LichenCore.xcframework.
# The database is SQLCipher on CommonCrypto (ADR-0036).
# Usage: scripts/build-ios-core.sh [--sim]
#   --sim: the simulator's slice only (UI tests, CI): the app then builds for the simulator only.
set -euo pipefail
sim_only=0
case "${1:-}" in
  --sim) sim_only=1 ;;
  "") ;;
  *) echo "usage: $0 [--sim]" >&2; exit 2 ;;
esac
cd "$(dirname "$0")/.."
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"

if [ "$sim_only" = 0 ]; then
  cargo build -q -p lc-ffi --release --features sqlcipher --target aarch64-apple-ios
fi
cargo build -q -p lc-ffi --release --features sqlcipher --target aarch64-apple-ios-sim
# Host library: the bindings are generated from its metadata.
cargo build -q -p lc-ffi --release --features sqlcipher

out=apps/ios/Generated
rm -rf "$out" && mkdir -p "$out"
cargo run -q -p lc-ffi --release --features cli,sqlcipher --bin uniffi-bindgen -- \
  generate --library target/release/liblc_ffi.dylib --language swift --out-dir "$out"

headers=target/ios-headers
rm -rf "$headers" && mkdir -p "$headers"
cp "$out/lc_ffiFFI.h" "$headers/"
cp "$out/lc_ffiFFI.modulemap" "$headers/module.modulemap"

slices=()
if [ "$sim_only" = 0 ]; then
  slices+=(-library target/aarch64-apple-ios/release/liblc_ffi.a -headers "$headers")
fi
slices+=(-library target/aarch64-apple-ios-sim/release/liblc_ffi.a -headers "$headers")
rm -rf apps/ios/LichenCore.xcframework
xcodebuild -quiet -create-xcframework "${slices[@]}" -output apps/ios/LichenCore.xcframework
echo "apps/ios/LichenCore.xcframework and apps/ios/Generated/lc_ffi.swift are ready"
