//! On-air frames: never panic, and an accepted frame re-encodes to the same bytes.
#![no_main]
use lc_meshcore::Packet;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(p) = Packet::decode(data) {
        let mut out = [0u8; 255];
        let n = p.encode(&mut out).expect("accepted frame must re-encode");
        assert_eq!(&out[..n], data);
        let _ = p.dedup_hash();
    }
});
