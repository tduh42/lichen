//! Group envelope decoder on arbitrary bytes: never panic, never accept a forgery, and never
//! produce a length a stock node would try to decrypt.
#![no_main]
use lc_proto::group::{GroupHeader, open, parse, seal, stock_would_try};
use libfuzzer_sys::fuzz_target;

const KEY: [u8; 32] = [9; 32];

fuzz_target!(|data: &[u8]| {
    let _ = parse(data);
    let mut p = data.to_vec();
    assert!(open(&KEY, &mut p).is_err());
    // Anything we build is never a length stock nodes try.
    if let Some((&first, plain)) = data.split_first() {
        let h = GroupHeader {
            gid_hash: 1,
            sender: first,
            key_epoch: 0,
            counter: u32::from(first),
            index: 0,
            total: 1,
            copy: first & 1,
        };
        let mut out = [0u8; 200];
        if let Ok(n) = seal(&KEY, &h, plain, &mut out) {
            assert!(!stock_would_try(n));
        }
    }
});
