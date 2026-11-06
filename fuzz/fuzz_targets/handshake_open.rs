//! HELLO and HELLO_ACK openers on arbitrary frames: never panic, never accept a forgery.
#![no_main]
use lc_proto::handshake::{Ends, open_hello, open_hello_ack};
use libfuzzer_sys::fuzz_target;

const S: [u8; 32] = [7; 32];
const ME: [u8; 32] = [0xB2; 32];
const PEER: [u8; 32] = [0xA1; 32];

fuzz_target!(|data: &[u8]| {
    let ends = Ends {
        me: &ME,
        peer: &PEER,
        me_node: 0xB2,
        peer_node: 0xA1,
    };
    let mut a = data.to_vec();
    assert!(open_hello(&S, &ends, &mut a).is_err());
    let mut b = data.to_vec();
    assert!(open_hello_ack(&S, &ends, &[3; 16], &mut b).is_err());
});
