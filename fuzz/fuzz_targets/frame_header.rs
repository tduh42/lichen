//! lichen frame header: never panic, accepted headers re-encode to the same 7 bytes.
#![no_main]
use lc_proto::Header;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((h, body)) = Header::decode(data) {
        assert_eq!(&h.encode()[..], &data[..7]);
        assert_eq!(body.len(), data.len() - 7);
        assert!(h.total >= 1 && h.total <= 64);
    }
});
