# ADR-0012 — Media over LoRa: Codec2 voice, AVIF thumbnails with a preview

- Status: accepted; voice in 700C since the vendored C library (same day); 1600 by default since the first real tests (2026-11-08)
- Date: 2026-11-06

## Context

The design allows two media over LoRa, direct only: voice messages in Codec2 700C (about 875 bytes for 10 s) and thumbnails of 2 to 4 KB, sent progressively (a preview of 3 or 4 fragments first, the rest on request), with a send refused when it would exceed the duty cycle. It asks which Rust binding or port supports 700C, and suggests link adaptation (a lower SF when the SNR allows).

## Decision

- **Voice.** No Rust port implements 700C: the `codec2` crate (pure Rust, LGPL-2.1) stops at 1200 bit/s. 700C comes from the reference C library (Codec2 1.2.0, LGPL-2.1), vendored in `crates/lc-media/codec2/` (the speech codec only, 15 files) and built by `build.rs`; its codebooks are generated the way Codec2 does, value for value. A five-function FFI wrapper is the repository's only `unsafe` code; the decoder is fuzzed with AddressSanitizer in the C code. New messages are 700C (10 s = 875 bytes, 6 fragments); 1200 is still decoded (the mode is in the body, §13.1).
- **Images.** AVIF, encoded in the Rust core with `ravif` (pure Rust, rav1e without assembly) so iOS and Android produce the same bytes; decoded by the system (iOS 16+, Android 12+). Fit in 160 × 120. A preview of at most 450 bytes is sent; the full thumbnail (at most 4 KB) only on request.
- **Groups.** No media: it would have to be flooded. It waits for the Internet bridge (L7), as the design says.
- **Budget.** A medium is refused, not queued, when its first round does not fit in the airtime left in the sliding hour; the app shows when it will fit.
- **Link adaptation: rejected.** A stock MeshCore mesh shares one radio preset; a companion that changed its SF would no longer hear its repeaters.

## Consequences

- **Voice quality, after the first real tests (2026-11-08).** 700C over a phone's microphone was judged very bad: robotic and hard to follow. The apps now encode in 1600 by default (10 s = 2,000 bytes, 13 fragments, about 25 s of airtime on one hop at SF8 / 62.5 kHz), with 700C (Compact) and 2400 (Clear, 20 fragments) in Settings; the speech is high-passed and levelled before encoding, and 700C uses its equaliser. 1600 and 2400 come from the pure-Rust `codec2` port, as 1200 does; the cost is shown before sending, as for every radio send.
- Both Codec2 options are LGPL-2.1: distributing the app (App Store) will need the relinking terms of the LGPL to be met (object files of the app available, or the codec in a separate dynamic framework).
- rav1e adds a few megabytes to the app and to build times.
- A receiver that never asks for the full image costs the network only the preview.
