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

/// Codec2 mode, as carried in the VOICE body (§13.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// 700 bit/s, 28 bits per 40 ms (the vendored reference C library, ADR-0012): the most
    /// compact, robotic.
    C700,
    /// 1200 bit/s, 48 bits per 40 ms.
    C1200,
    /// 1600 bit/s, 64 bits per 40 ms: clearer, the default.
    C1600,
    /// 2400 bit/s, two 20 ms frames of 48 bits per 40 ms: the clearest.
    C2400,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::C700, Mode::C1200, Mode::C1600, Mode::C2400];

    pub const fn code(self) -> u8 {
        match self {
            Mode::C700 => 0,
            Mode::C1200 => 1,
            Mode::C1600 => 2,
            Mode::C2400 => 3,
        }
    }

    pub const fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Mode::C700),
            1 => Some(Mode::C1200),
            2 => Some(Mode::C1600),
            3 => Some(Mode::C2400),
            _ => None,
        }
    }

    /// Bits per 40 ms frame of the message (two Codec2 frames in 2400).
    pub const fn bits_per_frame(self) -> usize {
        match self {
            Mode::C700 => 28,
            Mode::C1200 => 48,
            Mode::C1600 => 64,
            Mode::C2400 => 96,
        }
    }

    /// This build can decode (and encode) it.
    pub const fn supported(self) -> bool {
        true
    }

    fn codec(self, encoding: bool) -> Option<Codec> {
        let rust = |m| Codec::Rust(Box::new(Codec2::new(m)));
        match self {
            Mode::C700 if encoding => Codec700::encoder().map(Codec::C700),
            Mode::C700 => Codec700::new().map(Codec::C700),
            Mode::C1200 => Some(rust(Codec2Mode::MODE_1200)),
            Mode::C1600 => Some(rust(Codec2Mode::MODE_1600)),
            Mode::C2400 => Some(rust(Codec2Mode::MODE_2400)),
        }
    }
}

/// The mode new messages are encoded with when the app does not choose.
pub const ENCODE_MODE: Mode = Mode::C1600;

/// One codec state, either implementation.
enum Codec {
    C700(Codec700),
    /// The pure-Rust port: one or more Codec2 frames per 40 ms.
    Rust(Box<Codec2>),
}

impl Codec {
    /// Encodes 40 ms into `out`, `bits_per_frame` bits.
    fn encode(&mut self, out: &mut BitWriter, frame: &[i16; FRAME_SAMPLES]) {
        match self {
            Codec::C700(c) => out.push(&c.encode(frame), 28),
            Codec::Rust(c) => {
                let (n, bits) = (c.samples_per_frame(), c.bits_per_frame());
                let mut buf = [0u8; 8];
                for chunk in frame.chunks(n) {
                    let b = buf.get_mut(..bits.div_ceil(8)).unwrap_or(&mut []);
                    c.encode(b, chunk);
                    out.push(b, bits);
                }
            }
        }
    }

    /// Decodes 40 ms from `input`.
    fn decode(&mut self, frame: &mut [i16; FRAME_SAMPLES], input: &mut BitReader<'_>) {
        match self {
            Codec::C700(c) => {
                let mut b = [0u8; 4];
                input.read(&mut b, 28);
                *frame = c.decode(&b);
            }
            Codec::Rust(c) => {
                let (n, bits) = (c.samples_per_frame(), c.bits_per_frame());
                let mut buf = [0u8; 8];
                for chunk in frame.chunks_mut(n) {
                    let b = buf.get_mut(..bits.div_ceil(8)).unwrap_or(&mut []);
                    input.read(b, bits);
                    c.decode(chunk, b);
                }
            }
        }
    }
}

/// Speech prepared for Codec2: the low rumble and any DC offset removed (first-order high-pass
/// at about 100 Hz), then the level brought to about -20 dBFS while speaking (the loud 20 ms
/// windows), at most +24 dB so that silence stays silence, peaks softly limited. Codec2 models
/// pitch and spectral envelope: a quiet or offset recording comes out muffled and robotic.
pub fn prepare(pcm: &[i16]) -> Vec<i16> {
    const A: f32 = 0.927; // RC / (RC + dt), RC = 1 / (2π · 100 Hz), dt = 1 / 8 kHz
    let mut hp = Vec::with_capacity(pcm.len());
    let (mut x1, mut y1) = (0f32, 0f32);
    for &s in pcm {
        let x = f32::from(s);
        let y = A * (y1 + x - x1);
        hp.push(y);
        (x1, y1) = (x, y);
    }
    // Speech level: the 90th percentile of the 20 ms windows' RMS.
    let mut levels: Vec<f32> = hp
        .chunks(160)
        .map(|w| (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt())
        .collect();
    levels.sort_by(f32::total_cmp);
    let speech = levels
        .get(levels.len().saturating_sub(1) * 9 / 10)
        .copied()
        .unwrap_or(0.0);
    let gain = if speech < 30.0 {
        1.0
    } else {
        (3_300.0 / speech).clamp(0.5, 16.0)
    };
    hp.iter()
        .map(|v| {
            let y = v * gain;
            // Soft knee above 24,000: peaks squeezed rather than clipped.
            let a = y.abs();
            let limited = if a > 24_000.0 {
                24_000.0 + (a - 24_000.0) / (1.0 + (a - 24_000.0) / 8_000.0)
            } else {
                a
            };
            (limited.min(32_000.0) * y.signum()) as i16
        })
        .collect()
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

/// Encodes 8 kHz mono samples in the default mode (see [`encode_mode`]).
pub fn encode(pcm: &[i16]) -> Encoded {
    encode_mode(pcm, ENCODE_MODE)
}

/// Encodes 8 kHz mono samples in `mode`, prepared first ([`prepare`]), cut to 15 s; the last
/// frame is padded with silence.
pub fn encode_mode(pcm: &[i16], mode: Mode) -> Encoded {
    let frames = pcm
        .len()
        .div_ceil(FRAME_SAMPLES)
        .min(usize::from(MAX_FRAMES));
    let Some(mut codec) = mode.codec(true) else {
        return Encoded {
            mode,
            frames: 0,
            data: Vec::new(),
        };
    };
    let pcm = prepare(pcm.get(..frames * FRAME_SAMPLES).unwrap_or(pcm));
    let mut out = BitWriter::default();
    let mut frame = [0i16; FRAME_SAMPLES];
    for f in 0..frames {
        frame.fill(0);
        let chunk = pcm.get(f * FRAME_SAMPLES..).unwrap_or(&[]);
        for (d, s) in frame.iter_mut().zip(chunk) {
            *d = *s;
        }
        codec.encode(&mut out, &frame);
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
    let mut codec = mode.codec(false)?;
    let mut reader = BitReader { data, pos: 0 };
    let mut pcm = vec![0i16; usize::from(frames) * FRAME_SAMPLES];
    for frame in pcm.as_chunks_mut::<FRAME_SAMPLES>().0 {
        codec.decode(frame, &mut reader);
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
        let e = encode_mode(&speechlike(10.0), Mode::C700);
        assert_eq!(e.mode, Mode::C700);
        assert_eq!(e.frames, 250);
        assert_eq!(e.data.len(), 875);
        assert_eq!(data_len(Mode::C1200, 250), 1_500);
        assert_eq!(encode(&speechlike(10.0)).data.len(), 2_000);
        assert_eq!(
            encode_mode(&speechlike(10.0), Mode::C2400).data.len(),
            3_000
        );
        assert_eq!(duration_ms(e.frames), 10_000);
    }

    #[test]
    fn round_trip_keeps_the_energy() {
        let pcm = speechlike(3.0);
        let rms = |v: &[i16]| {
            (v.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        for mode in Mode::ALL {
            let e = encode_mode(&pcm, mode);
            let out = decode(e.mode.code(), e.frames, &e.data).unwrap();
            assert_eq!(out.len(), usize::from(e.frames) * FRAME_SAMPLES);
            let (a, b) = (rms(&prepare(&pcm)), rms(&out));
            assert!(
                b > a * 0.3 && b < a * 3.0,
                "{mode:?}: rms {a:.0} in, {b:.0} out"
            );
        }
    }

    #[test]
    fn prepare_levels_speech_not_silence() {
        let rms = |v: &[i16]| {
            (v.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        // Quiet speech with a DC offset: brought up, centred.
        let quiet: Vec<i16> = speechlike(2.0).iter().map(|&s| s / 20 + 400).collect();
        let p = prepare(&quiet);
        assert!(
            rms(&p) > rms(&quiet) * 4.0,
            "{} -> {}",
            rms(&quiet),
            rms(&p)
        );
        let mean = p.iter().map(|&s| f64::from(s)).sum::<f64>() / p.len() as f64;
        assert!(mean.abs() < 50.0, "DC left: {mean}");
        // Loud speech: no clipping.
        let loud: Vec<i16> = speechlike(2.0)
            .iter()
            .map(|&s| s.saturating_mul(5))
            .collect();
        assert!(prepare(&loud).iter().all(|&s| s.unsigned_abs() <= 32_000));
        // Near silence stays near silence.
        let hiss: Vec<i16> = (0..16_000).map(|i| ((i * 7919) % 41) as i16 - 20).collect();
        assert!(rms(&prepare(&hiss)) < 40.0);
    }

    #[test]
    fn every_mode_decodes() {
        let pcm = speechlike(1.0);
        for mode in Mode::ALL {
            let mut codec = mode.codec(true).unwrap();
            let mut w = BitWriter::default();
            let mut frame = [0i16; FRAME_SAMPLES];
            for chunk in pcm.chunks(FRAME_SAMPLES).take(25) {
                frame[..chunk.len()].copy_from_slice(chunk);
                codec.encode(&mut w, &frame);
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
        assert_eq!(decode(3, 1, &[0; 12]).unwrap().len(), 320);
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
