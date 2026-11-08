//! Encodes an 8 kHz mono 16-bit WAV into a voice fixture: `mode u8 ‖ frames u16 ‖ data`; with
//! a third path, also writes what the receiver will hear, as a WAV. `LICHEN_VOICE_MODE` picks
//! the mode (§13.1: 0 = 700C, 2 = 1600, the default, 3 = 2400).
//! Usage: cargo run -p lc-media --example encode_voice -- in.wav out.c2 [decoded.wav]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: encode_voice in.wav out.c2");
        std::process::exit(2);
    };
    let wav = std::fs::read(input).expect("read the WAV");
    // Walk the RIFF chunks to the samples.
    let mut i = 12;
    let pcm: Vec<i16> = loop {
        let id = &wav[i..i + 4];
        let len = u32::from_le_bytes(wav[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            break wav[i + 8..i + 8 + len]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| i16::from_le_bytes(b))
                .collect();
        }
        i += 8 + len + (len & 1);
    };
    let mode = std::env::var("LICHEN_VOICE_MODE")
        .ok()
        .and_then(|m| m.parse().ok())
        .and_then(lc_media::voice::Mode::from_code)
        .unwrap_or(lc_media::voice::ENCODE_MODE);
    let e = lc_media::voice::encode_mode(&pcm, mode);
    let mut out = vec![e.mode.code()];
    out.extend_from_slice(&e.frames.to_le_bytes());
    out.extend_from_slice(&e.data);
    std::fs::write(output, &out).expect("write");
    if let Some(heard) = args.get(3) {
        let pcm = lc_media::voice::decode(e.mode.code(), e.frames, &e.data).expect("decodes");
        let bytes = (pcm.len() * 2) as u32;
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + bytes).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        for v in [16u32, 1 | (1 << 16), 8_000, 16_000, 2 | (16 << 16)] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        w.extend_from_slice(b"data");
        w.extend_from_slice(&bytes.to_le_bytes());
        for s in pcm {
            w.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(heard, w).expect("write the WAV");
    }
    eprintln!(
        "{} samples -> {} frames ({} ms), {} bytes",
        pcm.len(),
        e.frames,
        lc_media::voice::duration_ms(e.frames),
        e.data.len()
    );
}
