//! Codec2 decoding of arbitrary frames (the bits come from the network, after the AEAD tag):
//! never panic, and the output has one 320-sample block per frame.
#![no_main]
use lc_media::voice::{FRAME_SAMPLES, decode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let frames = (rest.len() * 8 / 48) as u16;
    let len = (usize::from(frames) * 48).div_ceil(8);
    if let Some(pcm) = decode(mode & 1, frames, &rest[..len]) {
        assert_eq!(pcm.len(), usize::from(frames) * FRAME_SAMPLES);
    }
});
