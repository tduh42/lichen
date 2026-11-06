//! A session facing a peer that holds the keys (a malicious or buggy client): the input is a
//! script of authenticated frames with arbitrary header fields and plaintexts, interleaved with
//! clock jumps. Never panic, never emit an oversized frame.
#![no_main]
use lc_proto::sender::Timing;
use lc_proto::{FRAME_MAX, Header, Kind, Session, SessionKeys, aead};
use libfuzzer_sys::fuzz_target;

const A: u8 = 0xA1;
const B: u8 = 0xB2;
const K_AB: [u8; 32] = [1; 32];
const K_BA: [u8; 32] = [2; 32];

fuzz_target!(|data: &[u8]| {
    let mut b = Session::new(SessionKeys { send: K_BA, recv: K_AB }, B, A, Timing::default());
    let outgoing: Vec<u8> = (0..900u32).map(|i| i as u8).collect();
    let _ = b.send(&outgoing, 157, 0);
    let mut now = 0u64;
    let mut out = [0u8; 256];
    let mut input = data;
    while let [op, rest @ ..] = input {
        input = rest;
        if op & 0x80 != 0 {
            now += u64::from(op & 0x7F) * 500;
        } else {
            let [ki, t0, t1, total, attempt, len, rest @ ..] = input else { break };
            let len = usize::from(*len) % 170;
            let Some((plain, rest)) = rest.split_at_checked(len) else { break };
            input = rest;
            let kind = match ki >> 6 {
                0 => Kind::Data,
                1 => Kind::Ack,
                2 => Kind::Abort,
                _ => Kind::Control,
            };
            let h = Header {
                dst: B,
                src: A,
                tag: u16::from_le_bytes([*t0, *t1]),
                kind,
                index: ki & 0x3F,
                total: *total,
                attempt: *attempt,
            };
            let mut frame = Vec::with_capacity(7 + len + 8);
            frame.extend_from_slice(&h.encode());
            frame.extend_from_slice(plain);
            let tag = aead::seal(&K_AB, &h.nonce(), &h.encode(), &mut frame[7..]);
            frame.extend_from_slice(&tag);
            let _ = b.handle(&mut frame, now);
        }
        while let Some(n) = b.poll_transmit(now, &mut out) {
            assert!(n <= FRAME_MAX);
        }
        while b.take_event().is_some() {}
        if let Some(t) = b.next_wakeup() {
            assert!(t <= now + 10_000_000);
        }
    }
});
