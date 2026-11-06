//! Encodes an 8 kHz mono 16-bit WAV into a voice fixture: `mode u8 ‖ frames u16 ‖ data`.
//! Usage: cargo run -p lc-media --example encode_voice -- in.wav out.c2

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
    let e = lc_media::voice::encode(&pcm);
    let mut out = vec![e.mode.code()];
    out.extend_from_slice(&e.frames.to_le_bytes());
    out.extend_from_slice(&e.data);
    std::fs::write(output, &out).expect("write");
    eprintln!(
        "{} samples -> {} frames ({} ms), {} bytes",
        pcm.len(),
        e.frames,
        lc_media::voice::duration_ms(e.frames),
        e.data.len()
    );
}
