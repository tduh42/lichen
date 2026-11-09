//! lichen content vectors (`testdata/lichen/content.toml`, an independent Python oracle):
//! message references, REPLY and REACTION (protocol.md §6.2).

use lc_core::content::{Content, message_ref};
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
