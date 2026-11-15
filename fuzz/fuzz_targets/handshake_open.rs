//! HELLO and HELLO_ACK openers on arbitrary frames: never panic, never accept a forgery.
#![no_main]
use lc_meshcore::ecdh::{expand_seed, public_key};
use lc_proto::handshake::{Ends, Hello, VERSION, open_hello, open_hello_ack};
use libfuzzer_sys::fuzz_target;

const S: [u8; 32] = [7; 32];

fuzz_target!(|data: &[u8]| {
    let prv = expand_seed(&[2; 32]);
    let me = public_key(&prv);
    let peer = public_key(&expand_seed(&[1; 32]));
    let ends = Ends {
        me: &me,
        peer: &peer,
        me_node: me[0],
        peer_node: peer[0],
    };
    let mut a = data.to_vec();
    assert!(open_hello(&S, &ends, &prv, &mut a).is_err());
    let sent = Hello {
        tag: 1,
        attempt: 0,
        salt: [3; 16],
        ephemeral: [9; 32],
        version: VERSION,
        return_path_len: 0xFF,
        return_path: &[],
    };
    let mut b = data.to_vec();
    assert!(open_hello_ack(&S, &ends, &prv, &[5; 32], &sent, &mut b).is_err());
});
