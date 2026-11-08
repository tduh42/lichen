#!/usr/bin/env bash
# Builds the lichen core for Android: liblc_ffi.so for arm64-v8a (phones and the Apple-silicon
# emulator) in apps/android/app/src/main/jniLibs, and the Kotlin bindings in
# apps/android/app/src/main/java/com/tduhamel/lichen/core. Both are generated and git-ignored.
# Usage: scripts/build-android-core.sh
# Needs cargo-ndk, `rustup target add aarch64-linux-android` and the NDK below (CLAUDE.md).
set -euo pipefail
cd "$(dirname "$0")/.."
export ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/28.2.13676358}"
# 16 KB pages, required from Android 15 (NDK r28 aligns so already; this keeps it explicit).
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"

main=apps/android/app/src/main
# Android 12 (API 31) is the app's minimum (ADR-0012: AVIF decoding by the system).
cargo ndk -t arm64-v8a --platform 31 build -q -p lc-ffi --release
rm -rf "$main/jniLibs" && mkdir -p "$main/jniLibs/arm64-v8a"
cp target/aarch64-linux-android/release/liblc_ffi.so "$main/jniLibs/arm64-v8a/"
# Host library: the bindings are generated from its metadata, and the JVM unit tests load it.
cargo build -q -p lc-ffi --release

out="$main/java"
rm -rf "$out/com/tduhamel/lichen/core" "$main/generated"
cargo run -q -p lc-ffi --release --features cli --bin uniffi-bindgen -- \
  generate --library target/release/liblc_ffi.dylib --language kotlin --no-format --out-dir "$out"
echo "$main/jniLibs/arm64-v8a/liblc_ffi.so and $out/com/tduhamel/lichen/core are ready"
