# Codec2 (vendored subset)

The speech codec of [Codec2](https://github.com/drowe67/codec2) **1.2.0** (tag `1.2.0`, commit `06d4c11e699b0351765f10398abb4f663a984f36`, 2023-07-20), by David Rowe and contributors, under the **GNU LGPL 2.1** (`COPYING`). lichen uses it for the 700C mode, which no Rust port implements (ADR-0012).

- `src/`: the 15 C files of the speech codec (no FreeDV modems) and the headers they include, unmodified.
- `codebook/`: the quantiser tables the 3200…700C modes use, unmodified. `build.rs` turns them into C the way Codec2's `generate_codebook.c` does (same `%g` printing), and adds `codec2/version.h`.
- `../src/codec2_c.rs` is the only `unsafe` code of the repository: a five-function FFI wrapper, fuzzed (`fuzz/fuzz_targets/voice_decode.rs`).

Licence: the apps link this code statically. lichen's own source is public under the GPL-3.0-or-later, so anyone receiving the app can rebuild it with a modified Codec2, as the LGPL asks.
