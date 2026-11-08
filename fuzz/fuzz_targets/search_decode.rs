//! SEARCH bodies (§11.1) from the network, after the group signature: never panic, and what
//! decodes encodes back to something that decodes the same.
#![no_main]
use lc_proto::search::{MAX_LEN, Search};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = Search::decode(data) {
        let mut out = [0u8; MAX_LEN];
        let n = m.encode(&mut out);
        assert_eq!(Search::decode(&out[..n]), Some(m));
    }
});
