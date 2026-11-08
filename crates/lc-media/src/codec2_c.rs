//! The vendored Codec2 C library (codec2/README.md), 700C mode only: the repository's only
//! `unsafe` code. One codec state per call; the C code keeps no global mutable state.
#![allow(unsafe_code)]

use std::ffi::{c_int, c_short, c_uchar, c_void};

const CODEC2_MODE_700C: c_int = 8;
/// 700C: 40 ms frames of 320 samples, 28 bits in 4 bytes.
pub const SAMPLES: usize = 320;
pub const BYTES: usize = 4;

unsafe extern "C" {
    fn codec2_create(mode: c_int) -> *mut c_void;
    fn codec2_destroy(c: *mut c_void);
    fn codec2_encode(c: *mut c_void, bytes: *mut c_uchar, speech: *const c_short);
    fn codec2_decode(c: *mut c_void, speech: *mut c_short, bytes: *const c_uchar);
    fn codec2_samples_per_frame(c: *mut c_void) -> c_int;
    fn codec2_bytes_per_frame(c: *mut c_void) -> c_int;
    fn codec2_700c_eq(c: *mut c_void, en: bool);
}

/// A 700C encoder or decoder.
#[derive(Debug)]
pub struct Codec700 {
    state: *mut c_void,
}

impl Codec700 {
    pub fn new() -> Option<Self> {
        // SAFETY: codec2_create allocates a state for a valid mode, or returns null.
        let state = unsafe { codec2_create(CODEC2_MODE_700C) };
        if state.is_null() {
            return None;
        }
        let c = Self { state };
        // SAFETY: `state` is a live codec state.
        let (n, b) = unsafe {
            (
                codec2_samples_per_frame(c.state),
                codec2_bytes_per_frame(c.state),
            )
        };
        (n as usize == SAMPLES && b as usize == BYTES).then_some(c)
    }

    /// An encoder with 700C's equaliser on: it evens out the spectral envelope of the
    /// microphone before quantisation (Codec2's `-eq` option), clearer on phone microphones.
    pub fn encoder() -> Option<Self> {
        let c = Self::new()?;
        // SAFETY: the state is live; the flag only selects the encoder's equaliser.
        unsafe { codec2_700c_eq(c.state, true) };
        Some(c)
    }

    /// Encodes one frame of 320 samples into 4 bytes (28 bits, most significant first).
    pub fn encode(&mut self, speech: &[i16; SAMPLES]) -> [u8; BYTES] {
        let mut out = [0u8; BYTES];
        // SAFETY: the state is live; `out` holds bytes_per_frame bytes and `speech`
        // samples_per_frame samples, as checked in `new`.
        unsafe { codec2_encode(self.state, out.as_mut_ptr(), speech.as_ptr()) };
        out
    }

    /// Decodes 4 bytes (28 bits) into 320 samples.
    pub fn decode(&mut self, bits: &[u8; BYTES]) -> [i16; SAMPLES] {
        let mut out = [0i16; SAMPLES];
        // SAFETY: as in `encode`; every 28-bit value is a valid frame (indices within their
        // tables, see fuzz/fuzz_targets/voice_decode.rs).
        unsafe { codec2_decode(self.state, out.as_mut_ptr(), bits.as_ptr()) };
        out
    }
}

impl Drop for Codec700 {
    fn drop(&mut self) {
        // SAFETY: created by codec2_create, destroyed once.
        unsafe { codec2_destroy(self.state) };
    }
}
