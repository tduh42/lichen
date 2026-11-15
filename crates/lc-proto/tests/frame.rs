use lc_proto::header::{ACK_FRAME_MAX, ack_frame_len};
use lc_proto::sender::Timing;
use lc_proto::window::{ReplayWindow, Verdict};
use lc_proto::{FrameError, Header, Kind, chunk_for_path};
use proptest::prelude::*;

#[test]
fn header_layout_and_nonce() {
    let h = Header {
        dst: 0xB2,
        src: 0xA1,
        tag: 0x0102,
        kind: Kind::Ack,
        index: 0,
        total: 12,
        attempt: 3,
    };
    assert_eq!(h.encode(), [0xB2, 0xA1, 0x02, 0x01, 0x40, 12, 3]);
    assert_eq!(h.nonce(), [0x02, 0x01, 0x40, 12, 3, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn header_rules() {
    // The ACK of a 3-fragment datagram is 24 bytes: header, one packed byte, tag (§7.2).
    let mut long = [0u8; 25];
    long[..7].copy_from_slice(&[1, 2, 0, 0, 0x40, 3, 0]);
    assert_eq!(Header::decode(&long).unwrap_err(), FrameError::Malformed);
    long[5] = 12; // 12 fragments need two packed bytes
    assert!(Header::decode(&long).is_ok());
    assert_eq!(
        Header::decode(&long[..24]).unwrap_err(),
        FrameError::Malformed
    );
    let mut f = [0u8; 24];
    f[..7].copy_from_slice(&[1, 2, 0, 0, 0x40, 3, 0]);
    assert!(Header::decode(&f).is_ok());
    assert_eq!(Header::decode(&f[..23]).unwrap_err(), FrameError::Length);
    f[4] = 0x41; // ACK with index 1
    assert_eq!(Header::decode(&f).unwrap_err(), FrameError::Malformed);
    f[4] = 0x02; // DATA index 2 of total 3
    assert!(Header::decode(&f).is_ok());
    f[4] = 0x03; // DATA index 3 of total 3
    assert_eq!(Header::decode(&f).unwrap_err(), FrameError::Malformed);
    let mut c = [0u8; 40];
    c[..7].copy_from_slice(&[1, 2, 0, 0, 0xC2, 1, 0]); // CONTROL index 2 (PATH_REPLY)
    assert!(Header::decode(&c).is_ok());
    c[4] = 0xC5; // CONTROL index 5 (INTRO_FIN)
    assert!(Header::decode(&c).is_ok());
    c[4] = 0xC6; // CONTROL index 6: reserved
    assert_eq!(Header::decode(&c).unwrap_err(), FrameError::Malformed);
    f[5] = 0; // total 0
    assert_eq!(Header::decode(&f).unwrap_err(), FrameError::Malformed);
    f[5] = 65;
    assert_eq!(Header::decode(&f).unwrap_err(), FrameError::Malformed);
    assert_eq!(Header::decode(&f[..15]).unwrap_err(), FrameError::Length);
}

#[test]
fn chunk_sizes() {
    assert_eq!(chunk_for_path(0), 149);
    assert_eq!(chunk_for_path(2), 149);
    assert_eq!(chunk_for_path(5), 146);
}

/// The ACK is one packed byte up to 7 fragments: 24 bytes on the wire, as it was with the
/// 8-byte tag and its fixed 9-byte body (ADR-0028).
#[test]
fn ack_sizes() {
    for total in 1..=7 {
        assert_eq!(ack_frame_len(total), 24);
    }
    assert_eq!(ack_frame_len(8), 25);
    assert_eq!(ack_frame_len(15), 25);
    assert_eq!(ack_frame_len(16), 26);
    assert_eq!(ack_frame_len(64), 32);
    assert_eq!(ACK_FRAME_MAX, 32);
}

#[test]
fn timing_for_path() {
    let t = Timing::for_path(1_512, 260, 0);
    assert_eq!(t.pace_ms, 1_612);
    let t = Timing::for_path(1_512, 260, 3);
    assert_eq!(t.pace_ms, 10_684);
    assert_eq!(t.ack_repeat_ms, 2_020);
    assert_eq!(t.ack_guard_ms, 2_540);
    assert_eq!(t.ack_wait_ms, 4 * (3_024 + 520) + 2_000);
}

#[test]
fn replay_window() {
    let mut w = ReplayWindow::new();
    assert_eq!(w.check(5), Verdict::New);
    w.see(5);
    assert_eq!(w.check(5), Verdict::Pending);
    w.complete(5);
    assert_eq!(w.check(5), Verdict::Completed);
    assert_eq!(w.check(4), Verdict::Pending);
    w.complete(70);
    assert_eq!(w.check(70), Verdict::Completed);
    assert_eq!(w.check(7), Verdict::Pending); // 63 below
    assert_eq!(w.check(6), Verdict::TooOld); // 64 below
    assert_eq!(w.check(5), Verdict::TooOld);
    w.see(200);
    assert_eq!(w.check(70), Verdict::TooOld);
    assert_eq!(w.check(199), Verdict::Pending);
}

proptest! {
    #[test]
    fn header_decode_never_panics(frame in proptest::collection::vec(any::<u8>(), 0..200)) {
        if let Ok((h, body)) = Header::decode(&frame) {
            prop_assert_eq!(&h.encode()[..], &frame[..7]);
            prop_assert_eq!(body.len(), frame.len() - 7);
        }
    }
}
