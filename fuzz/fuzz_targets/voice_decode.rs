//! Codec2 decoding of arbitrary frames in every mode (the bits come from the network, after
//! the AEAD tag): never panic, and the output has one 320-sample block per 40 ms frame.
#![no_main]
use lc_media::voice::{FRAME_SAMPLES, MAX_FRAMES, Mode, data_len, decode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&m, rest)) = data.split_first() else {
        return;
    };
    let Some(mode) = Mode::from_code(m % 4) else {
        return;
    };
    let frames = (rest.len() * 8 / mode.bits_per_frame()).min(usize::from(MAX_FRAMES)) as u16;
    let len = data_len(mode, frames);
    if let Some(pcm) = decode(mode.code(), frames, &rest[..len]) {
        assert_eq!(pcm.len(), usize::from(frames) * FRAME_SAMPLES);
    }
});
