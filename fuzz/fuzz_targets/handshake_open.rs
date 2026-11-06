//! HELLO and HELLO_ACK openers on arbitrary frames: never panic, never accept a forgery.
#![no_main]
use lc_proto::handshake::{open_hello, open_hello_ack};
use libfuzzer_sys::fuzz_target;

const S: [u8; 32] = [7; 32];
const ME: [u8; 32] = [0xB2; 32];
const PEER: [u8; 32] = [0xA1; 32];

fuzz_target!(|data: &[u8]| {
    let mut a = data.to_vec();
    assert!(open_hello(&S, &PEER, &ME, &mut a).is_err());
    let mut b = data.to_vec();
    assert!(open_hello_ack(&S, &PEER, &ME, &[3; 16], &mut b).is_err());
});
