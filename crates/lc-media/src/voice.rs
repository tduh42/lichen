//! Voice messages (protocol.md §13.1): Codec2 frames packed back to back, 8 kHz mono.

use codec2::{Codec2, Codec2Mode};

use crate::codec2_c::Codec700;

/// Sample rate of the audio Codec2 takes and gives.
pub const SAMPLE_RATE: u32 = 8_000;
/// Samples in one 40 ms frame (700C and 1200).
pub const FRAME_SAMPLES: usize = 320;
/// Frame duration, milliseconds.
pub const FRAME_MS: u32 = 40;
/// At most 15 seconds.
pub const MAX_FRAMES: u16 = 375;

/// Codec2 mode, as carried in the VOICE body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// 700 bit/s, 28 bits per frame (the vendored reference C library, ADR-0012).
    C700,
    /// 1200 bit/s, 48 bits per frame.
    C1200,
}

impl Mode {
    pub const fn code(self) -> u8 {
        match self {
            Mode::C700 => 0,
            Mode::C1200 => 1,
        }
    }

    pub const fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Mode::C700),
            1 => Some(Mode::C1200),
            _ => None,
        }
    }

    pub const fn bits_per_frame(self) -> usize {
        match self {
            Mode::C700 => 28,
            Mode::C1200 => 48,
        }
    }

    /// This build can decode (and encode) it.
    pub const fn supported(self) -> bool {
        true
    }

    fn codec(self) -> Option<Codec> {
        match self {
            Mode::C700 => Codec700::new().map(Codec::C700),
            Mode::C1200 => Some(Codec::C1200(Box::new(Codec2::new(Codec2Mode::MODE_1200)))),
        }
    }
}

/// The mode new messages are encoded with.
pub const ENCODE_MODE: Mode = Mode::C700;

/// One codec state, either implementation.
enum Codec {
    C700(Codec700),
    C1200(Box<Codec2>),
}

impl Codec {
    /// Encodes a frame into `bits` (the frame's bytes, MSB first).
    fn encode(&mut self, bits: &mut [u8], frame: &[i16; FRAME_SAMPLES]) {
        match self {
            Codec::C700(c) => {
                let out = c.encode(frame);
                for (d, s) in bits.iter_mut().zip(out) {
                    *d = s;
                }
            }
            Codec::C1200(c) => c.encode(bits, frame),
        }
    }

    fn decode(&mut self, frame: &mut [i16; FRAME_SAMPLES], bits: &[u8]) {
        match self {
            Codec::C700(c) => {
                let mut b = [0u8; 4];
                for (d, s) in b.iter_mut().zip(bits) {
                    *d = *s;
                }
                *frame = c.decode(&b);
            }
            Codec::C1200(c) => c.decode(frame, bits),
        }
    }
}

/// Bytes of `frames` frames in `mode`.
pub const fn data_len(mode: Mode, frames: u16) -> usize {
    (frames as usize * mode.bits_per_frame()).div_ceil(8)
}

/// Duration of `frames` frames, milliseconds.
pub const fn duration_ms(frames: u16) -> u32 {
    frames as u32 * FRAME_MS
}

/// An encoded voice message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub mode: Mode,
    pub frames: u16,
    pub data: Vec<u8>,
}

/// Encodes 8 kHz mono samples, cut to 15 s; the last frame is padded with silence.
pub fn encode(pcm: &[i16]) -> Encoded {
    let mode = ENCODE_MODE;
    let frames = pcm
        .len()
        .div_ceil(FRAME_SAMPLES)
        .min(usize::from(MAX_FRAMES));
    let mut out = BitWriter::default();
    let Some(mut codec) = mode.codec() else {
        return Encoded {
            mode,
            frames: 0,
            data: Vec::new(),
        };
    };
    let bits = mode.bits_per_frame();
    let mut frame = [0i16; FRAME_SAMPLES];
    let mut packed = [0u8; 8];
    for f in 0..frames {
        frame.fill(0);
        let chunk = pcm.get(f * FRAME_SAMPLES..).unwrap_or(&[]);
        for (d, s) in frame.iter_mut().zip(chunk) {
            *d = *s;
        }
        let n = bits.div_ceil(8);
        if let Some(buf) = packed.get_mut(..n) {
            codec.encode(buf, &frame);
            out.push(buf, bits);
        }
    }
    Encoded {
        mode,
        frames: frames as u16,
        data: out.bytes,
    }
}

/// Decodes a voice body; `None` when the mode is unknown or unsupported, or the length does
/// not match `frames`.
pub fn decode(mode: u8, frames: u16, data: &[u8]) -> Option<Vec<i16>> {
    let mode = Mode::from_code(mode)?;
    if frames > MAX_FRAMES || data.len() != data_len(mode, frames) {
        return None;
    }
    let mut codec = mode.codec()?;
    let bits = mode.bits_per_frame();
    let mut reader = BitReader { data, pos: 0 };
    let mut pcm = vec![0i16; usize::from(frames) * FRAME_SAMPLES];
    let mut packed = [0u8; 8];
    for frame in pcm.as_chunks_mut::<FRAME_SAMPLES>().0 {
        let buf = packed.get_mut(..bits.div_ceil(8))?;
        reader.read(buf, bits);
        codec.decode(frame, buf);
    }
    Some(pcm)
}

/// Bits most significant first.
#[derive(Debug, Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    /// Appends the first `n` bits of `src`.
    fn push(&mut self, src: &[u8], n: usize) {
        for i in 0..n {
            let bit = src.get(i / 8).is_some_and(|b| b & (0x80 >> (i % 8)) != 0);
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if bit && let Some(last) = self.bytes.last_mut() {
                *last |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }
}

#[derive(Debug)]
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    /// Reads `n` bits into the start of `dst` (the rest of its last byte is zero).
    fn read(&mut self, dst: &mut [u8], n: usize) {
        dst.fill(0);
        for i in 0..n {
            let p = self.pos + i;
            let bit = self
                .data
                .get(p / 8)
                .is_some_and(|b| b & (0x80 >> (p % 8)) != 0);
            if bit && let Some(d) = dst.get_mut(i / 8) {
                *d |= 0x80 >> (i % 8);
            }
        }
        self.pos += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A voiced sound: a gliding pitch with a few harmonics, amplitude-modulated like
    /// syllables.
    pub(crate) fn speechlike(seconds: f32) -> Vec<i16> {
        let n = (seconds * SAMPLE_RATE as f32) as usize;
        let mut phase = 0.0f32;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let f0 = 140.0 + 40.0 * (t * 2.1).sin();
                phase += 2.0 * std::f32::consts::PI * f0 / SAMPLE_RATE as f32;
                let env = (0.5 + 0.5 * (t * 2.0 * std::f32::consts::PI * 3.0).sin()).max(0.05);
                let s: f32 = (1..8).map(|h| (phase * h as f32).sin() / h as f32).sum();
                (s * env * 6_000.0) as i16
            })
            .collect()
    }

    #[test]
    fn ten_seconds_size() {
        let e = encode(&speechlike(10.0));
        assert_eq!(e.mode, Mode::C700);
        assert_eq!(e.frames, 250);
        assert_eq!(e.data.len(), 875);
        assert_eq!(data_len(Mode::C1200, 250), 1_500);
        assert_eq!(duration_ms(e.frames), 10_000);
    }

    #[test]
    fn round_trip_keeps_the_energy() {
        let pcm = speechlike(3.0);
        let e = encode(&pcm);
        let out = decode(e.mode.code(), e.frames, &e.data).unwrap();
        assert_eq!(out.len(), usize::from(e.frames) * FRAME_SAMPLES);
        let rms = |v: &[i16]| {
            (v.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        let (a, b) = (rms(&pcm), rms(&out));
        assert!(b > a * 0.3 && b < a * 3.0, "rms {a:.0} in, {b:.0} out");
    }

    #[test]
    fn both_modes_decode() {
        let pcm = speechlike(1.0);
        for mode in [Mode::C700, Mode::C1200] {
            let mut codec = mode.codec().unwrap();
            let mut w = BitWriter::default();
            let mut frame = [0i16; FRAME_SAMPLES];
            let mut buf = [0u8; 8];
            let n = mode.bits_per_frame().div_ceil(8);
            for chunk in pcm.chunks(FRAME_SAMPLES).take(25) {
                frame[..chunk.len()].copy_from_slice(chunk);
                codec.encode(&mut buf[..n], &frame);
                w.push(&buf[..n], mode.bits_per_frame());
            }
            assert_eq!(w.bytes.len(), data_len(mode, 25));
            let out = decode(mode.code(), 25, &w.bytes).unwrap();
            assert!(
                out.iter().any(|&s| s.unsigned_abs() > 500),
                "{mode:?} decoded silence"
            );
        }
    }

    #[test]
    fn limits() {
        assert_eq!(encode(&speechlike(20.0)).frames, MAX_FRAMES);
        assert_eq!(encode(&[]).frames, 0);
        // Wrong length, unknown mode, too many frames.
        assert!(decode(1, 2, &[0; 11]).is_none());
        assert!(decode(9, 1, &[0; 6]).is_none());
        assert!(decode(0, 2, &[0; 8]).is_none());
        assert_eq!(decode(0, 2, &[0; 7]).unwrap().len(), 640);
        assert!(
            decode(
                1,
                MAX_FRAMES + 1,
                &vec![0; data_len(Mode::C1200, MAX_FRAMES + 1)]
            )
            .is_none()
        );
        assert_eq!(decode(1, 0, &[]).unwrap().len(), 0);
    }

    #[test]
    fn bit_packing_28() {
        let mut w = BitWriter::default();
        w.push(&[0xAB, 0xCD, 0xEF, 0x1F], 28);
        w.push(&[0x12, 0x34, 0x56, 0x7F], 28);
        assert_eq!(w.bytes, vec![0xAB, 0xCD, 0xEF, 0x11, 0x23, 0x45, 0x67]);
        let mut r = BitReader {
            data: &w.bytes,
            pos: 0,
        };
        let mut b = [0u8; 4];
        r.read(&mut b, 28);
        assert_eq!(b, [0xAB, 0xCD, 0xEF, 0x10]);
        r.read(&mut b, 28);
        assert_eq!(b, [0x12, 0x34, 0x56, 0x70]);
    }
}
