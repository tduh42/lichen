//! PROBE and PATH_REPLY decoders on arbitrary bytes: never panic, never accept a forgery.
#![no_main]
use lc_proto::discovery::{open_path_reply, parse_probe, verify_probe};
use libfuzzer_sys::fuzz_target;

const S: [u8; 32] = [7; 32];
const ME: [u8; 32] = [0xB2; 32];
const PEER: [u8; 32] = [0xA1; 32];

fuzz_target!(|data: &[u8]| {
    if let Some(p) = parse_probe(data) {
        assert_eq!(data.len(), 20);
        let _ = (p.dst, p.src, p.id);
    }
    assert!(!verify_probe(&S, &PEER, &ME, data));
    let mut frame = data.to_vec();
    assert!(open_path_reply(&S, &PEER, &ME, &mut frame).is_err());
});
