//! Introduction messages (§4.6) on arbitrary frames: INTRO parses without panicking and
//! re-encodes to the same bytes; INTRO_ACK and INTRO_FIN never open without the keys.
#![no_main]
use lc_meshcore::ecdh::{expand_seed, public_key};
use lc_proto::intro::{Answer, open_intro, open_intro_ack, open_intro_fin, seal_intro};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((src, intro)) = open_intro(0xB2, data) {
        let mut out = [0u8; 200];
        let n = seal_intro(src, 0xB2, &intro, &mut out).expect("what parsed seals");
        assert_eq!(&out[..n], data);
    }
    let prv = expand_seed(&[1; 32]);
    let id = public_key(&prv);
    assert!(open_intro_ack(0xB2, 0xA1, &[5; 32], &[6; 16], &[3; 32], data).is_err());
    let answer = Answer {
        salt_i: [6; 16],
        e_i: [9; 32],
        salt_r: [7; 16],
        eph_secret: [8; 32],
        intro_hash: [3; 32],
    };
    assert!(open_intro_fin(0xB2, 0xA1, &prv, &id, &answer, data).is_err());
});
