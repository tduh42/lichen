//! lichen content vectors (`testdata/lichen/content.toml`, an independent Python oracle):
//! message references, REPLY and REACTION (protocol.md §6.2); group invitations and GROUP_JOIN
//! (§10.5).

use lc_core::content::{Content, message_ref};
use lc_core::identity::Invite;
use toml::{Table, Value};

fn load() -> Table {
    let path = format!(
        "{}/../../testdata/lichen/content.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

fn entries<'a>(t: &'a Table, kind: &str) -> &'a Vec<Value> {
    t[kind].as_array().unwrap()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap()
}

fn h(v: &Value, k: &str) -> Vec<u8> {
    hex::decode(s(v, k)).unwrap()
}

fn ts(v: &Value) -> u32 {
    u32::try_from(v["ts"].as_integer().unwrap()).unwrap()
}

#[test]
fn references() {
    let t = load();
    for v in entries(&t, "ref") {
        let author: [u8; 32] = h(v, "author").try_into().unwrap();
        assert_eq!(
            message_ref(&author, &h(v, "plaintext")).to_vec(),
            h(v, "ref"),
            "{}",
            s(v, "name")
        );
    }
}

#[test]
fn replies() {
    let t = load();
    for v in entries(&t, "reply") {
        let c = Content::Reply {
            sent_at: ts(v),
            target: h(v, "target").try_into().unwrap(),
            text: s(v, "text").into(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(Content::decode(&h(v, "datagram")), Some(c));
    }
}

#[test]
fn reactions() {
    let t = load();
    for v in entries(&t, "reaction") {
        let c = Content::Reaction {
            sent_at: ts(v),
            target: h(v, "target").try_into().unwrap(),
            emoji: s(v, "emoji").into(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(Content::decode(&h(v, "datagram")), Some(c));
    }
}

#[test]
fn edits_and_deletes() {
    let t = load();
    for v in entries(&t, "edit") {
        let c = Content::Edit {
            sent_at: ts(v),
            target: h(v, "target").try_into().unwrap(),
            text: s(v, "text").into(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(Content::decode(&h(v, "datagram")), Some(c));
    }
    let deletes = entries(&t, "delete");
    assert!(!deletes.is_empty());
    for v in deletes {
        let c = Content::Delete {
            sent_at: ts(v),
            target: h(v, "target").try_into().unwrap(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(Content::decode(&h(v, "datagram")), Some(c));
    }
}

/// TIMER (§6.5, ADR-0030).
#[test]
fn timers() {
    let t = load();
    let timers = entries(&t, "timer");
    assert_eq!(timers.len(), 4);
    for v in timers {
        let c = Content::Timer {
            sent_at: ts(v),
            seconds: u32::try_from(v["seconds"].as_integer().unwrap()).unwrap(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(Content::decode(&h(v, "datagram")), Some(c));
    }
}

#[test]
fn malformed() {
    let t = load();
    for v in entries(&t, "malformed") {
        assert_eq!(Content::decode(&h(v, "datagram")), None, "{}", s(v, "name"));
    }
}

#[test]
fn invites() {
    let t = load();
    for v in entries(&t, "invite") {
        let invite = Invite {
            admin: h(v, "admin").try_into().unwrap(),
            node: h(v, "node").try_into().unwrap(),
            token: h(v, "token").try_into().unwrap(),
            expires: u32::try_from(v["expires"].as_integer().unwrap()).unwrap(),
            admin_name: s(v, "admin_name").to_owned(),
            group_name: s(v, "group_name").to_owned(),
        };
        assert_eq!(invite.encode(), s(v, "text"), "{}", s(v, "name"));
        assert_eq!(
            Invite::decode(s(v, "text")),
            Some(invite),
            "{}",
            s(v, "name")
        );
    }
    for v in entries(&t, "bad_invite") {
        assert_eq!(Invite::decode(s(v, "text")), None, "{}", s(v, "name"));
    }
}

#[test]
fn group_join() {
    let t = load();
    for v in entries(&t, "group_join") {
        let mut datagram = vec![lc_proto::content::GROUP_JOIN];
        datagram.extend_from_slice(&h(v, "token"));
        assert_eq!(datagram, h(v, "datagram"), "{}", s(v, "name"));
    }
}

/// GROUP_SYNC (§10.7): decoded and encoded back to the same bytes.
#[test]
fn group_sync() {
    use lc_core::group::Control;
    let t = load();
    for v in entries(&t, "group_sync") {
        let d = h(v, "datagram");
        let Some(Control::Sync { gid, epoch }) = Control::decode(&d) else {
            panic!("{}", s(v, "name"));
        };
        assert_eq!(gid.to_vec(), h(v, "gid"));
        assert_eq!(i64::from(epoch), v["epoch"].as_integer().unwrap());
        assert_eq!(Control::Sync { gid, epoch }.encode(), d, "{}", s(v, "name"));
    }
}

#[test]
fn group_states() {
    use lc_core::group::Control;
    let t = load();
    for v in entries(&t, "group_state") {
        let d = h(v, "datagram");
        let Some(Control::State {
            gid,
            epoch,
            admin,
            name,
            members,
            cause,
        }) = Control::decode(&d)
        else {
            panic!("{}", s(v, "name"));
        };
        assert_eq!(gid.to_vec(), h(v, "gid"));
        assert_eq!(i64::from(epoch), v["epoch"].as_integer().unwrap());
        assert_eq!(admin.to_vec(), h(v, "admin"));
        assert_eq!(name, s(v, "group_name"));
        assert_eq!(members.len() as i64, v["members"].as_integer().unwrap());
        let want = v["cause"].as_integer().unwrap();
        assert_eq!(cause.map_or(-1, i64::from), want, "{}", s(v, "name"));
        let back = Control::State {
            gid,
            epoch,
            admin,
            name,
            members,
            cause,
        };
        assert_eq!(back.encode(), d, "{}", s(v, "name"));
    }
}

#[test]
fn sender_keys() {
    use lc_core::group::Control;
    let t = load();
    for v in entries(&t, "sender_keys") {
        let d = h(v, "datagram");
        let Some(Control::SenderKeys { gid, entries: keys }) = Control::decode(&d) else {
            panic!("{}", s(v, "name"));
        };
        assert_eq!(gid.to_vec(), h(v, "gid"));
        assert_eq!(keys.len() as i64, v["n"].as_integer().unwrap());
        let owners: Vec<u8> = keys.iter().flat_map(|k| k.0).collect();
        assert_eq!(owners, h(v, "owners"));
        assert_eq!(Control::SenderKeys { gid, entries: keys }.encode(), d);
    }
    for v in entries(&t, "bad_group_control") {
        assert_eq!(Control::decode(&h(v, "datagram")), None, "{}", s(v, "name"));
    }
}

#[test]
fn group_recovery() {
    use lc_core::group::Control;
    let t = load();
    for v in entries(&t, "group_missing") {
        let d = h(v, "datagram");
        let Some(Control::Missing {
            gid,
            author,
            epoch,
            counters,
        }) = Control::decode(&d)
        else {
            panic!("{}", s(v, "name"));
        };
        assert_eq!(gid.to_vec(), h(v, "gid"));
        assert_eq!(author.to_vec(), h(v, "author"));
        assert_eq!(i64::from(epoch), v["epoch"].as_integer().unwrap());
        let want: Vec<i64> = v["counters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_integer().unwrap())
            .collect();
        assert_eq!(
            counters.iter().map(|c| i64::from(*c)).collect::<Vec<_>>(),
            want
        );
        let back = Control::Missing {
            gid,
            author,
            epoch,
            counters,
        };
        assert_eq!(back.encode(), d);
    }
    for v in entries(&t, "group_resend") {
        let d = h(v, "datagram");
        let Some(Control::Resend { gid, payloads }) = Control::decode(&d) else {
            panic!("{}", s(v, "name"));
        };
        assert_eq!(gid.to_vec(), h(v, "gid"));
        let lengths: Vec<i64> = payloads.iter().map(|p| p.len() as i64).collect();
        let want: Vec<i64> = v["lengths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_integer().unwrap())
            .collect();
        assert_eq!(lengths, want, "{}", s(v, "name"));
        assert_eq!(Control::Resend { gid, payloads }.encode(), d);
    }
}

#[test]
fn stickers_and_gifs() {
    let t = load();
    for v in entries(&t, "sticker") {
        let c = Content::Sticker {
            sent_at: ts(v),
            set: v["set"].as_integer().unwrap() as u8,
            emoji: s(v, "emoji").to_owned(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(
            Content::decode(&h(v, "datagram")),
            Some(c),
            "{}",
            s(v, "name")
        );
    }
    for v in entries(&t, "gif") {
        let c = Content::Gif {
            sent_at: ts(v),
            provider: v["provider"].as_integer().unwrap() as u8,
            width: v["width"].as_integer().unwrap() as u16,
            height: v["height"].as_integer().unwrap() as u16,
            id: s(v, "id").to_owned(),
            title: s(v, "title").to_owned(),
        };
        assert_eq!(c.encode(), h(v, "datagram"), "{}", s(v, "name"));
        assert_eq!(
            Content::decode(&h(v, "datagram")),
            Some(c),
            "{}",
            s(v, "name")
        );
    }
    for v in entries(&t, "bad_content") {
        assert!(
            !matches!(
                Content::decode(&h(v, "datagram")),
                Some(Content::Sticker { .. } | Content::Gif { .. })
            ),
            "{}",
            s(v, "name")
        );
    }
    for v in entries(&t, "gif_link") {
        assert_eq!(
            lc_core::content::gif_link(s(v, "text")),
            Some((s(v, "id").to_owned(), s(v, "title").to_owned())),
            "{}",
            s(v, "name")
        );
    }
    for v in entries(&t, "not_gif_link") {
        assert_eq!(
            lc_core::content::gif_link(s(v, "text")),
            None,
            "{}",
            s(v, "name")
        );
    }
}

#[test]
fn small_files() {
    let t = load();
    for v in entries(&t, "file") {
        let content = Content::File {
            sent_at: ts(v),
            name: s(v, "file_name").to_owned(),
            data: h(v, "data"),
        };
        // The encoder's DEFLATE bytes are not fixed (ADR-0023): only decoding them is. Our own
        // encoding must decode to the same file, compressed when the vector is.
        let ours = content.encode();
        assert_eq!(Content::decode(&ours).as_ref(), Some(&content));
        if v.get("compressed").is_some() {
            assert_eq!(ours.get(5), Some(&3), "{}", s(v, "name"));
        } else {
            assert_eq!(ours, h(v, "datagram"), "{}", s(v, "name"));
        }
        assert_eq!(
            Content::decode(&h(v, "datagram")),
            Some(content),
            "{}",
            s(v, "name")
        );
    }
    for v in entries(&t, "bad_file") {
        assert!(
            !matches!(
                Content::decode(&h(v, "datagram")),
                Some(Content::File { .. })
            ),
            "{}",
            s(v, "name")
        );
    }
}

// ----- frames (`testdata/lichen/frames.toml`, an independent Python oracle with its own
// ChaCha20-Poly1305): session frames, group envelopes and PROBEs with 16-byte tags (ADR-0028).

mod frames {
    use super::{entries, h};
    use lc_proto::discovery::{Keying, build_probe, verify_probe};
    use lc_proto::group::{GroupHeader, open, seal};
    use lc_proto::sender::Timing;
    use lc_proto::{CHUNK_MAX, DATAGRAM_MAX, Received, Session, SessionEvent, SessionKeys};
    use toml::{Table, Value};

    const A: u8 = 0xA1;
    const B: u8 = 0xB2;

    fn load() -> Table {
        let path = format!(
            "{}/../../testdata/lichen/frames.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap().parse().unwrap()
    }

    fn int(v: &Value, k: &str) -> i64 {
        v[k].as_integer().unwrap()
    }

    fn pair() -> (Session, Session) {
        let (k1, k2) = ([1u8; 32], [2u8; 32]);
        let t = Timing::default();
        (
            Session::new(SessionKeys { send: k1, recv: k2 }, A, B, t),
            Session::new(SessionKeys { send: k2, recv: k1 }, B, A, t),
        )
    }

    /// Every frame `s` emits until it has nothing more due, the clock following its timers.
    fn drain(s: &mut Session, now: &mut u64, limit: usize) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 200];
        while out.len() < limit {
            if let Some(n) = s.poll_transmit(*now, &mut buf) {
                out.push(buf[..n].to_vec());
            } else if let Some(t) = s.next_wakeup().filter(|t| *t > *now) {
                *now = t;
            } else {
                break;
            }
        }
        out
    }

    fn delivered(s: &mut Session) -> bool {
        std::iter::from_fn(|| s.take_event()).any(|e| matches!(e, SessionEvent::Delivered { .. }))
    }

    /// A short datagram, then one of 8 fragments whose last fragment arrives first: the frames
    /// both sides emit are the oracle's, byte for byte, and each side accepts the other's.
    #[test]
    fn session_frames() {
        let t = load();
        let (data, acks) = (entries(&t, "data"), entries(&t, "ack"));
        let (mut a, mut b) = pair();
        let mut now = 0u64;

        // One fragment, and its one-byte COMPLETE ACK.
        let hello = h(&data[0], "plain");
        assert_eq!(a.send(&hello, CHUNK_MAX, now).unwrap(), 0);
        let mut sent = drain(&mut a, &mut now, 1);
        assert_eq!(sent[0], h(&data[0], "frame"));
        assert_eq!(sent[0].len(), 7 + hello.len() + 16);
        let got = match b.handle(&mut sent[0], now).unwrap() {
            Received::Datagram { data, .. } => data.to_vec(),
            other => panic!("{other:?}"),
        };
        assert_eq!(got, hello);
        let mut ack = drain(&mut b, &mut now, 1);
        assert_eq!(ack[0], h(&acks[0], "frame"));
        assert_eq!(ack[0].len(), 24);
        a.handle(&mut ack[0], now).unwrap();
        assert!(delivered(&mut a));

        // Eight fragments of 146 bytes.
        let long: Vec<u8> = data[1..9].iter().flat_map(|v| h(v, "plain")).collect();
        assert_eq!(long.len(), 8 * CHUNK_MAX);
        assert_eq!(a.send(&long, CHUNK_MAX, now).unwrap(), 1);
        let mut sent = drain(&mut a, &mut now, 8);
        for (frame, v) in sent.iter().zip(&data[1..9]) {
            assert_eq!(frame, &h(v, "frame"), "fragment {}", int(v, "index"));
            assert_eq!(frame.len(), 169);
        }
        // The last one alone closes the round: an ACK that names it, in two packed bytes.
        let mut last = sent.pop().unwrap();
        assert!(matches!(
            b.handle(&mut last, now).unwrap(),
            Received::Nothing
        ));
        let ack = drain(&mut b, &mut now, 1);
        assert_eq!(ack[0], h(&acks[1], "frame"));
        assert_eq!(ack[0].len(), 25);
        // The others: the datagram is whole, and the ACK says COMPLETE.
        let mut whole = None;
        for f in &mut sent {
            if let Received::Datagram { data, .. } = b.handle(f, now).unwrap() {
                whole = Some(data.to_vec());
            }
        }
        assert_eq!(whole.unwrap(), long);
        let mut ack = drain(&mut b, &mut now, 1);
        assert_eq!(ack[0], h(&acks[2], "frame"));
        a.handle(&mut ack[0], now).unwrap();
        assert!(delivered(&mut a));
    }

    /// The largest datagram: 64 fragments, acknowledged in nine packed bytes.
    #[test]
    fn the_longest_ack() {
        let t = load();
        let v = &entries(&t, "ack")[3];
        let (mut a, mut b) = pair();
        let mut now = 0u64;
        // Tags 0 and 1 are used up first, as in the vectors.
        for len in [10, 20] {
            a.send(&vec![9u8; len], CHUNK_MAX, now).unwrap();
            for f in &mut drain(&mut a, &mut now, 1) {
                b.handle(f, now).unwrap();
            }
            for f in &mut drain(&mut b, &mut now, 1) {
                a.handle(f, now).unwrap();
            }
            assert!(delivered(&mut a));
        }
        let big = vec![0x5Au8; DATAGRAM_MAX];
        assert_eq!(a.send(&big, CHUNK_MAX, now).unwrap(), 2);
        for f in &mut drain(&mut a, &mut now, 64) {
            b.handle(f, now).unwrap();
        }
        let mut ack = drain(&mut b, &mut now, 1);
        assert_eq!(ack[0], h(v, "frame"));
        assert_eq!(ack[0].len(), 32);
        a.handle(&mut ack[0], now).unwrap();
        assert!(delivered(&mut a));
    }

    /// An ACK with a bit set beyond its last fragment, or of the wrong length, is dropped.
    #[test]
    fn ack_shape() {
        let t = load();
        let acks = entries(&t, "ack");
        let (mut a, _) = pair();
        a.send(&[1, 2, 3], CHUNK_MAX, 0).unwrap();
        let mut now = 0u64;
        drain(&mut a, &mut now, 1);
        // The 8-fragment ACK does not fit a 1-fragment datagram (its length says so).
        let mut wrong = h(&acks[2], "frame");
        wrong[5] = 1;
        assert!(a.handle(&mut wrong, now).is_err());
        assert!(!delivered(&mut a));
        let mut good = h(&acks[0], "frame");
        a.handle(&mut good, now).unwrap();
        assert!(delivered(&mut a));
    }

    #[test]
    fn abort_frame() {
        let t = load();
        let v = &entries(&t, "abort")[0];
        let (mut a, _) = pair();
        let mut now = 0u64;
        a.send(&[7; 12], CHUNK_MAX, now).unwrap();
        drain(&mut a, &mut now, 1);
        a.cancel();
        let out = drain(&mut a, &mut now, 1);
        assert_eq!(out[0], h(v, "frame"));
        assert_eq!(out[0].len(), 24);
    }

    #[test]
    fn group_envelopes() {
        let t = load();
        for v in entries(&t, "envelope") {
            let key: [u8; 32] = h(v, "key").try_into().unwrap();
            let header = GroupHeader {
                gid_hash: u16::try_from(int(v, "gid_hash")).unwrap(),
                sender: u8::try_from(int(v, "sender")).unwrap(),
                key_epoch: u8::try_from(int(v, "key_epoch")).unwrap(),
                counter: u32::try_from(int(v, "counter")).unwrap(),
                index: u8::try_from(int(v, "index")).unwrap(),
                total: u8::try_from(int(v, "total")).unwrap(),
                copy: u8::try_from(int(v, "copy")).unwrap(),
            };
            let plain = h(v, "plain");
            let mut out = [0u8; 200];
            let n = seal(&key, &header, &plain, &mut out).unwrap();
            let mut payload = h(v, "payload");
            assert_eq!(&out[..n], &payload[..], "{} bytes", plain.len());
            assert_eq!(payload[1] == 0x03, v["padded"].as_bool().unwrap());
            // A stock node reads a 2-byte MAC after the channel hash and then wants whole
            // 16-byte blocks: the rest is never one (§1 fact 9).
            assert!(!(payload.len() - 3).is_multiple_of(16));
            let (got, opened) = open(&key, &mut payload).unwrap();
            assert_eq!((got, opened), (header, &plain[..]));
        }
    }

    #[test]
    fn probes() {
        let t = load();
        for v in entries(&t, "probe") {
            let s: [u8; 32] = h(v, "s").try_into().unwrap();
            let prober: [u8; 32] = h(v, "prober").try_into().unwrap();
            let target: [u8; 32] = h(v, "target").try_into().unwrap();
            let id: [u8; 8] = h(v, "id").try_into().unwrap();
            let k = if v["anonymous"].as_bool().unwrap() {
                Keying::anonymous(&prober, &target)
            } else {
                Keying {
                    s: &s,
                    anonymous: false,
                    prober: &prober,
                    target: &target,
                }
            };
            let payload = h(v, "payload");
            assert_eq!(build_probe(&k, prober[0], target[0], &id).to_vec(), payload);
            assert_eq!(payload.len(), 28);
            assert!(verify_probe(&k, prober[0], target[0], &payload));
            // The 20-byte probe of version 1 (8-byte MAC) is not one any more.
            assert!(!verify_probe(&k, prober[0], target[0], &payload[..20]));
        }
    }
}
