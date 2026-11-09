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
