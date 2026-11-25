//! A node's readings, CayenneLPP (protocol.md §16.7, ADR-0050), against
//! `testdata/lichen/lpp.toml`, written by `lpp.py` from the library MeshCore builds with (an
//! independent oracle). Cut short, unknown types and long inputs come from the vectors too.

use lc_meshcore::lpp::{self, Item, Value};
use proptest::prelude::*;
use toml::{Table, Value as Toml};

fn load() -> Table {
    let path = format!(
        "{}/../../testdata/lichen/lpp.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

fn int(v: &Toml, k: &str) -> i64 {
    v[k].as_integer().unwrap()
}

/// An item as the vectors write it.
fn want(v: &Toml) -> (u8, String, Vec<i64>) {
    let kind = v["kind"].as_str().unwrap().to_owned();
    let values = if kind == "gps" {
        vec![int(v, "lat"), int(v, "lon"), int(v, "alt")]
    } else {
        vec![int(v, "value")]
    };
    (u8::try_from(int(v, "channel")).unwrap(), kind, values)
}

fn got(i: &Item) -> (u8, String, Vec<i64>) {
    match i.value {
        Value::Scalar { kind, value } => (i.channel, kind.name().to_owned(), vec![value]),
        Value::Gps {
            lat_e4,
            lon_e4,
            alt_cm,
        } => (
            i.channel,
            "gps".to_owned(),
            vec![lat_e4.into(), lon_e4.into(), alt_cm.into()],
        ),
    }
}

#[test]
fn readings_match_the_vectors() {
    let t = load();
    let all = t["lpp"].as_array().unwrap();
    assert!(all.len() >= 14);
    for v in all {
        let name = v["name"].as_str().unwrap();
        let data = hex::decode(v["hex"].as_str().unwrap()).unwrap();
        let mut it = lpp::items(&data);
        let items: Vec<_> = it.by_ref().map(|i| got(&i)).collect();
        let expected: Vec<_> = v["items"].as_array().unwrap().iter().map(want).collect();
        assert_eq!(items, expected, "{name}");
        let consumed = usize::try_from(int(v, "consumed")).unwrap();
        assert_eq!(it.consumed(), consumed, "{name}");
        assert_eq!(lpp::known_len(&data), consumed, "{name}");
        // Once stopped, it stays stopped.
        assert_eq!(it.next(), None, "{name}");
    }
}

/// Every prefix of every vector decodes to a prefix of its items, never past what it holds.
#[test]
fn prefixes_decode_to_prefixes() {
    let t = load();
    for v in t["lpp"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        let data = hex::decode(v["hex"].as_str().unwrap()).unwrap();
        let all: Vec<_> = lpp::items(&data).collect();
        for n in 0..=data.len() {
            let mut it = lpp::items(&data[..n]);
            let part: Vec<_> = it.by_ref().collect();
            assert!(it.consumed() <= n, "{name} at {n}");
            assert_eq!(part[..], all[..part.len()], "{name} at {n}");
        }
    }
}

proptest! {
    /// Any bytes: no panic, never past the end, and what is kept decodes to the same items.
    #[test]
    fn any_bytes(data in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut it = lpp::items(&data);
        let items: Vec<_> = it.by_ref().collect();
        let n = it.consumed();
        prop_assert!(n <= data.len());
        let again: Vec<_> = lpp::items(&data[..n]).collect();
        prop_assert_eq!(again, items);
    }
}
