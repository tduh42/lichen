//! The encrypted backup file (docs/spec/backup.md, ADR-0037): the Python oracle's vectors, a
//! store's round trip with what it leaves out, every way a file can be wrong, and a restored
//! phone that handshakes again and reads its group on the fake network.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use lc_core::backup::{self, BackupError, CHUNK, HEADER_LEN, Opener, Sealer, Sources, kind};
use lc_core::content::{Content, MessageRef};
use lc_core::core::FailReason;
use lc_core::fakenet::Net;
use lc_core::store::{Chat, Message, Status, Store};
use lc_core::{Contact, Event, GroupId, contact_kind};
use lc_meshcore::airtime::LoRaParams;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use toml::{Table, Value};

const MIN: u64 = 60_000;
const SEED: [u8; 32] = [0x5E; 32];
const PHOTO: &str = "4c69636865e6f6e652070686f746f2066726f6d20746865207269646765202020.jpg";

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lichen-backup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ----- vectors (`testdata/lichen/backup.toml`, backup.py) -------------------------------

fn load() -> Table {
    let path = format!(
        "{}/../../testdata/lichen/backup.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

fn h(v: &Value, k: &str) -> Vec<u8> {
    hex::decode(v[k].as_str().unwrap()).unwrap()
}

fn n(v: &Value, k: &str) -> usize {
    usize::try_from(v[k].as_integer().unwrap()).unwrap()
}

fn arr<const N: usize>(v: &Value, k: &str) -> [u8; N] {
    h(v, k).try_into().unwrap()
}

/// backup.py's large entries: byte i is (31 i + k) mod 251.
fn pattern(len: usize, k: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((31 * i + usize::from(k)) % 251) as u8)
        .collect()
}

fn open_all(file: &[u8], seed: &[u8; 32]) -> Result<Vec<u8>, BackupError> {
    let mut r = Opener::new(file, seed)?;
    let mut plain = Vec::new();
    r.read_to_end(&mut plain)?;
    Ok(plain)
}

#[test]
fn vectors() {
    let t = load();
    for v in t["small"].as_array().unwrap() {
        let (seed, salt) = (arr::<32>(v, "seed"), arr::<32>(v, "salt"));
        let mut out = Vec::new();
        let mut w = Sealer::new(&mut out, &seed, salt).unwrap();
        for (k, name, data) in [
            (kind::DATABASE, "", h(v, "database")),
            (kind::FILE, v["file_name"].as_str().unwrap(), h(v, "file")),
            (kind::SETTINGS, "", h(v, "settings")),
        ] {
            w.entry(k, name, data.len() as u64).unwrap();
            w.write_all(&data).unwrap();
        }
        w.finish().unwrap();
        assert_eq!(out[..HEADER_LEN], h(v, "header"));
        assert_eq!(out, h(v, "file_bytes"));
        assert_eq!(
            open_all(&h(v, "file_bytes"), &seed).unwrap(),
            h(v, "contents")
        );
    }
    for v in t["large"].as_array().unwrap() {
        let (seed, salt) = (arr::<32>(v, "seed"), arr::<32>(v, "salt"));
        let mut out = Vec::new();
        let mut w = Sealer::new(&mut out, &seed, salt).unwrap();
        let db = pattern(n(v, "database_len"), kind::DATABASE);
        w.entry(kind::DATABASE, "", db.len() as u64).unwrap();
        w.write_all(&db).unwrap();
        if n(v, "file_len") > 0 {
            let f = pattern(n(v, "file_len"), kind::FILE);
            w.entry(kind::FILE, v["file_name"].as_str().unwrap(), f.len() as u64)
                .unwrap();
            w.write_all(&f).unwrap();
        }
        w.finish().unwrap();
        let name = v["name"].as_str().unwrap();
        assert_eq!(out.len(), n(v, "file_len_total"), "{name}");
        assert_eq!(Sha256::digest(&out).to_vec(), h(v, "sha256"), "{name}");
        assert_eq!(
            open_all(&out, &seed).unwrap().len(),
            n(v, "contents_len"),
            "{name}"
        );
    }
}

// ----- a store's round trip -------------------------------------------------------------

const CAMILLE: [u8; 32] = [0xCA; 32];
const CAMILLE_ID: [u8; 32] = [0xC1; 32];
const JULIEN: [u8; 32] = [0x7E; 32];

fn msg(chat: Chat, outgoing: bool, text: &str, status: Status) -> Message {
    Message {
        id: 0,
        chat,
        outgoing,
        author: None,
        content: Content::Text {
            sent_at: 1_793_000_000,
            text: text.into(),
        },
        at_ms: 1_793_000_000_000,
        status,
        fragments: 1,
        airtime_ms: 400,
        snr4: Some(20),
        read: true,
        msg_ref: None,
    }
}

fn texts(store: &Store, chat: &Chat) -> Vec<(String, Status)> {
    store
        .history(chat, None, 1000)
        .unwrap()
        .into_iter()
        .filter_map(|m| match m.content {
            Content::Text { text, .. } => Some((text, m.status)),
            _ => None,
        })
        .collect()
}

/// A phone's store with something of every kind, kept or left out.
fn fill(path: &Path) {
    let s = Store::open(path).unwrap();
    s.put_contact(&Contact {
        key: CAMILLE,
        name: "Camille".into(),
        out_path: None,
        identity: Some(CAMILLE_ID),
        verified: true,
        kind: contact_kind::CHAT,
    })
    .unwrap();
    s.set_nickname(&CAMILLE, Some("Cam")).unwrap();
    let chat = Chat::Contact(CAMILLE);
    s.insert(&msg(chat, false, "At the refuge?", Status::Received))
        .unwrap();
    s.insert(&msg(
        chat,
        true,
        "Yes, by the north path",
        Status::Delivered,
    ))
    .unwrap();
    s.insert(&msg(chat, true, "still queued", Status::Queued))
        .unwrap();
    s.insert(&msg(
        chat,
        true,
        "half sent",
        Status::Sending { sent: 1, total: 3 },
    ))
    .unwrap();
    let target: MessageRef = [1; 8];
    s.upsert_reaction(&chat, &target, &CAMILLE, "👍", 5, false, None)
        .unwrap();
    s.upsert_reaction(&chat, &[2; 8], &[0; 32], "🔥", 5, true, Some(9))
        .unwrap();
    s.amend(
        &chat,
        &[3; 8],
        None,
        Some("our edit, not delivered"),
        6,
        true,
        Some(9),
    )
    .unwrap();
    s.set_pinned(&chat, Some(1_793_000_000_000)).unwrap();
    s.set_setting("tx_power_restore", Some(&[22])).unwrap();
    s.set_setting("kept", Some(b"yes")).unwrap();
    s.put_invitation(&[4; 16], b"\x01invitation with keys")
        .unwrap();
    s.put_group(&[6; 16], b"not a snapshot").unwrap();
    s.set_search_consent(&[6; 16], true).unwrap();
    // Julien's conversation has a timer: what is stored under it never goes in a backup.
    let timed = Chat::Contact(JULIEN);
    s.set_timer(&timed, 3600, 1, false).unwrap();
    s.insert(&msg(timed, false, "disappears", Status::Received))
        .unwrap();
    drop(s);
    let c = Connection::open(path).unwrap();
    c.execute(
        "INSERT INTO shares (chat_kind, chat, precision, until) VALUES (0, ?1, 0, 9999999999)",
        params![CAMILLE.as_slice()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO positions (sender, gid, body, at, snr4) VALUES (?1, x'', x'00', 1, 0)",
        params![CAMILLE.as_slice()],
    )
    .unwrap();
}

fn count(path: &Path, table: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

struct Phone {
    dir: PathBuf,
    db: PathBuf,
    downloads: PathBuf,
}

fn phone(name: &str) -> Phone {
    let dir = tmp(name);
    Phone {
        db: dir.join("id-0011223344556677.sqlite"),
        downloads: dir.join("downloads"),
        dir,
    }
}

/// A phone with a filled store and two downloads (one over a chunk), and its backup.
fn exported(name: &str) -> (Phone, PathBuf, backup::Summary) {
    let p = phone(name);
    fill(&p.db);
    std::fs::create_dir_all(&p.downloads).unwrap();
    std::fs::write(p.downloads.join(PHOTO), pattern(150_000, 9)).unwrap();
    std::fs::write(p.downloads.join("ab.pdf"), b"%PDF-1.7").unwrap();
    // Neither a hidden file nor a folder is a download.
    std::fs::write(p.downloads.join(".partial"), b"x").unwrap();
    std::fs::write(p.downloads.join(format!("{PHOTO}.part")), b"half a photo").unwrap();
    std::fs::create_dir_all(p.downloads.join("sub")).unwrap();
    let out = p.dir.join("lichen.backup");
    let summary = backup::export(
        &Sources {
            database: &p.db,
            key: None,
            downloads: Some(&p.downloads),
            settings: br#"{"lichen.myName":"Me"}"#,
            drop_settings: &["tx_power_restore"],
            work: &p.dir.join("work"),
        },
        &out,
        &SEED,
        [0x5A; 32],
    )
    .unwrap();
    (p, out, summary)
}

#[test]
fn round_trip() {
    let (old, file, summary) = exported("round-trip");
    assert_eq!(
        (summary.messages, summary.files),
        (4, 2),
        "the disappearing message is not in it"
    );
    assert_eq!(summary.bytes, std::fs::metadata(&file).unwrap().len());
    // The copy of the store is gone; the store itself is untouched.
    assert!(!old.dir.join("work/backup.sqlite").exists());
    assert_eq!(count(&old.db, "messages"), 5);
    assert_eq!(count(&old.db, "invitations"), 1);

    let new = phone("round-trip-new");
    let staged = backup::unpack(&file, &SEED, &new.dir.join("restore")).unwrap();
    assert_eq!(staged.settings, br#"{"lichen.myName":"Me"}"#);
    assert_eq!(staged.messages, 4);
    assert!(!new.db.exists(), "nothing installed before install");
    staged.install(&new.db, &new.downloads).unwrap();
    assert!(!new.dir.join("restore").exists());
    assert_eq!(
        std::fs::read(new.downloads.join(PHOTO)).unwrap(),
        pattern(150_000, 9)
    );
    assert_eq!(
        std::fs::read(new.downloads.join("ab.pdf")).unwrap(),
        b"%PDF-1.7"
    );
    assert!(!new.downloads.join(".partial").exists());
    assert!(
        !new.downloads.join(format!("{PHOTO}.part")).exists(),
        "still downloading"
    );

    let s = Store::open(&new.db).unwrap();
    let c = s.contacts().unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(
        (c[0].key, c[0].identity, c[0].verified),
        (CAMILLE, Some(CAMILLE_ID), true)
    );
    assert_eq!(s.nicknames().unwrap()[&CAMILLE], "Cam");
    let chat = Chat::Contact(CAMILLE);
    let interrupted = Status::Failed(FailReason::Interrupted);
    assert_eq!(
        texts(&s, &chat),
        vec![
            ("At the refuge?".to_owned(), Status::Received),
            ("Yes, by the north path".to_owned(), Status::Delivered),
            ("still queued".to_owned(), interrupted),
            ("half sent".to_owned(), interrupted),
        ]
    );
    assert!(s.resume_unfinished().unwrap().is_empty(), "no outbox");
    assert!(texts(&s, &Chat::Contact(JULIEN)).is_empty());
    assert_eq!(s.reactions_for(&chat, &[1; 8]).unwrap().len(), 1);
    assert!(s.reactions_for(&chat, &[2; 8]).unwrap().is_empty());
    assert!(s.pending_amendments().unwrap().is_empty());
    assert_eq!(s.chat_prefs().unwrap().len(), 1);
    assert_eq!(s.setting("kept").unwrap(), Some(b"yes".to_vec()));
    assert_eq!(s.setting("tx_power_restore").unwrap(), None);
    assert_eq!(s.timer(&Chat::Contact(JULIEN)).unwrap().0, 3600);
    assert!(
        s.groups().unwrap().is_empty(),
        "a snapshot that does not decode"
    );
    assert!(s.invitations().unwrap().is_empty());
    assert!(s.search_consents().unwrap().is_empty());
    assert!(s.shares().unwrap().is_empty());
    assert!(s.positions().unwrap().is_empty());
    for p in [old.dir, new.dir] {
        let _ = std::fs::remove_dir_all(p);
    }
}

/// A restore over the same identity's store replaces it, and leaves none of its journal.
#[test]
fn restore_replaces_the_store() {
    let (p, file, _) = exported("replace");
    {
        let s = Store::open(&p.db).unwrap();
        s.insert(&msg(
            Chat::Contact(CAMILLE),
            false,
            "after the backup",
            Status::Received,
        ))
        .unwrap();
    }
    std::fs::write(PathBuf::from(format!("{}-wal", p.db.display())), b"stale").unwrap();
    backup::unpack(&file, &SEED, &p.dir.join("restore"))
        .unwrap()
        .install(&p.db, &p.downloads)
        .unwrap();
    assert!(!PathBuf::from(format!("{}-wal", p.db.display())).exists());
    let s = Store::open(&p.db).unwrap();
    assert_eq!(texts(&s, &Chat::Contact(CAMILLE)).len(), 4);
    let _ = std::fs::remove_dir_all(&p.dir);
}

// ----- every way a file can be wrong ------------------------------------------------------

fn unpack_bytes(name: &str, bytes: &[u8], seed: &[u8; 32]) -> Result<(), BackupError> {
    let d = tmp(name);
    let f = d.join("file");
    std::fs::write(&f, bytes).unwrap();
    let staging = d.join("restore");
    let r = backup::unpack(&f, seed, &staging).map(|u| u.discard());
    assert!(!staging.exists(), "{name}: the staging folder is removed");
    let _ = std::fs::remove_dir_all(&d);
    r
}

fn damaged(r: Result<(), BackupError>) -> bool {
    matches!(r, Err(BackupError::Damaged))
}

#[test]
fn what_is_refused() {
    let (p, file, _) = exported("refused");
    let good = std::fs::read(&file).unwrap();
    let sealed = CHUNK + 16;
    assert!(good.len() > HEADER_LEN + 2 * sealed, "several chunks");
    assert!(unpack_bytes("good", &good, &SEED).is_ok());

    assert!(matches!(
        unpack_bytes("words", &good, &[0x11; 32]),
        Err(BackupError::WrongWords)
    ));
    assert!(matches!(
        unpack_bytes("empty", b"", &SEED),
        Err(BackupError::NotABackup)
    ));
    assert!(matches!(
        unpack_bytes("other", &[b'x'; 200], &SEED),
        Err(BackupError::NotABackup)
    ));
    let mut newer = good.clone();
    newer[13] = 2;
    assert!(matches!(
        unpack_bytes("newer", &newer, &SEED),
        Err(BackupError::Newer)
    ));
    // A changed salt changes the key: the check no longer matches.
    let mut salt = good.clone();
    salt[20] ^= 1;
    assert!(matches!(
        unpack_bytes("salt", &salt, &SEED),
        Err(BackupError::WrongWords)
    ));

    for at in [
        HEADER_LEN,
        HEADER_LEN + 1000,
        HEADER_LEN + sealed + 5,
        good.len() - 1,
    ] {
        let mut t = good.clone();
        t[at] ^= 0x40;
        assert!(damaged(unpack_bytes("tampered", &t, &SEED)), "byte {at}");
    }
    for len in [
        HEADER_LEN,
        HEADER_LEN + 10,
        HEADER_LEN + sealed,
        HEADER_LEN + 2 * sealed,
        HEADER_LEN + 2 * sealed - 1,
        good.len() - 1,
    ] {
        assert!(
            damaged(unpack_bytes("truncated", &good[..len], &SEED)),
            "cut at {len}"
        );
    }
    let mut swapped = good[..HEADER_LEN].to_vec();
    swapped.extend_from_slice(&good[HEADER_LEN + sealed..HEADER_LEN + 2 * sealed]);
    swapped.extend_from_slice(&good[HEADER_LEN..HEADER_LEN + sealed]);
    swapped.extend_from_slice(&good[HEADER_LEN + 2 * sealed..]);
    assert!(damaged(unpack_bytes("reordered", &swapped, &SEED)));
    let mut appended = good.clone();
    appended.extend_from_slice(&good[HEADER_LEN..HEADER_LEN + sealed]);
    assert!(damaged(unpack_bytes("appended", &appended, &SEED)));
    let mut extra = good.clone();
    extra.push(0);
    assert!(damaged(unpack_bytes("one more byte", &extra, &SEED)));
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// Contents that do not parse, sealed properly: refused as damaged all the same.
#[test]
fn contents_that_do_not_parse() {
    let seal = |f: &dyn Fn(&mut Sealer<&mut Vec<u8>>)| {
        let mut out = Vec::new();
        let mut w = Sealer::new(&mut out, &SEED, [1; 32]).unwrap();
        f(&mut w);
        w.finish().unwrap();
        out
    };
    let cases: [(&str, Vec<u8>); 6] = [
        ("no database", seal(&|_| {})),
        (
            "an entry longer than what follows",
            seal(&|w| {
                w.entry(kind::DATABASE, "", 100).unwrap();
                w.write_all(b"short").unwrap();
            }),
        ),
        (
            "a path for a name",
            seal(&|w| {
                w.entry(kind::FILE, "../escape", 1).unwrap();
                w.write_all(b"x").unwrap();
            }),
        ),
        ("an unknown kind", seal(&|w| w.entry(9, "", 0).unwrap())),
        (
            "two databases",
            seal(&|w| {
                for _ in 0..2 {
                    w.entry(kind::DATABASE, "", 0).unwrap();
                }
            }),
        ),
        (
            "settings too large",
            seal(&|w| {
                w.entry(kind::SETTINGS, "", 70_000).unwrap();
                w.write_all(&[b' '; 70_000]).unwrap();
            }),
        ),
    ];
    for (name, bytes) in cases {
        assert!(damaged(unpack_bytes("parse", &bytes, &SEED)), "{name}");
    }
    // A database that is not a store.
    let not_sqlite = seal(&|w| {
        w.entry(kind::DATABASE, "", 5).unwrap();
        w.write_all(b"hello").unwrap();
    });
    assert!(matches!(
        unpack_bytes("not sqlite", &not_sqlite, &SEED),
        Err(BackupError::Database(_))
    ));
}

// ----- a restored phone on the fake network ---------------------------------------------

/// Five companions around one repeater; the admin (node 0) knows everyone.
fn star(seed: u64) -> (Net, Vec<usize>) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let members: Vec<usize> = ["alice", "bob", "carol", "dave"]
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(1 + i as u8, n))
        .collect();
    let hub = net.add_repeater(50, "hub");
    for &m in &members {
        net.link(m, hub);
    }
    for &m in &members[1..] {
        net.know(members[0], m, true);
        net.know(m, members[0], true);
    }
    for &m in &members {
        net.connect(m);
    }
    assert!(net.run_until(net.now + 20_000, |n| {
        members
            .iter()
            .all(|&m| n.nodes[m].core.as_ref().is_some_and(|c| c.ready()))
    }));
    for &m in &members[1..] {
        net.introduce(members[0], m);
    }
    (net, members)
}

fn keys_complete(net: &Net, members: &[usize], gid: &GroupId) -> bool {
    members.iter().all(|&m| {
        net.nodes[m].core.as_ref().is_some_and(|c| {
            c.groups()
                .find(|g| g.gid == *gid)
                .is_some_and(|g| g.keys_known() == g.members.len() - 1)
        })
    })
}

fn settle(net: &mut Net) {
    let mut last = (net.stats.transmissions, net.now);
    net.run_until(net.now + 120 * MIN, |n| {
        if n.stats.transmissions != last.0 {
            last = (n.stats.transmissions, n.now);
        }
        n.now >= last.1 + 5 * MIN
    });
}

fn group_texts(events: &[Event]) -> Vec<(u8, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::GroupMessage {
                from,
                content: Content::Text { text, .. },
                ..
            } => Some((from[0], text.clone())),
            _ => None,
        })
        .collect()
}

fn direct_texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Received {
                content: Content::Text { text, .. },
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The identity seed the fake network gives the app of node `i` (`Net::attach_app`).
fn id_seed(net: &Net, i: usize) -> [u8; 32] {
    let mut s = net.key(i);
    s[0] ^= 0xA5;
    s
}

/// ADR-0037: Carol's phone is replaced. Its backup brings back her contacts and the group,
/// without anyone's sender key; the new phone handshakes again with Alice, sends its new
/// group key to everyone, and asks the admin for the others' keys at the first post it cannot
/// open (§10.7).
#[test]
fn a_restored_phone_talks_again() {
    let (mut net, m) = star(37);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("Refuge", &keys[1..], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m, &gid)));
    settle(&mut net);

    // Carol's phone keeps what lc-ffi keeps: contacts and the group's snapshot.
    let old = phone("fakenet-old");
    {
        let s = Store::open(&old.db).unwrap();
        net.with_core_ref(m[2], |c| {
            for k in c.contacts() {
                s.put_contact(k).unwrap();
            }
            s.put_group(&gid, &c.group_snapshot(&gid).unwrap()).unwrap();
        });
    }
    let file = old.dir.join("carol.backup");
    backup::export(
        &Sources {
            database: &old.db,
            key: None,
            downloads: None,
            settings: b"",
            drop_settings: &[],
            work: &old.dir.join("work"),
        },
        &file,
        &id_seed(&net, m[2]),
        [7; 32],
    )
    .unwrap();

    // A new phone on the same radio: a fresh app, the backup restored from her words.
    net.attach_app(m[2], 91);
    let new = phone("fakenet-new");
    backup::unpack(&file, &id_seed(&net, m[2]), &new.dir.join("restore"))
        .unwrap()
        .install(&new.db, &new.downloads)
        .unwrap();
    let s = Store::open(&new.db).unwrap();
    let (contacts, groups) = (s.contacts().unwrap(), s.groups().unwrap());
    net.with_core(m[2], |c, _| {
        for k in contacts {
            c.restore_contact(k);
        }
        for g in &groups {
            assert!(c.restore_group(g));
        }
        let g = c.groups().find(|g| g.gid == gid).unwrap();
        assert_eq!((g.members.len(), g.keys_known()), (4, 0), "no sender key");
    });
    net.connect(m[2]);
    settle(&mut net);
    for &i in &m {
        net.take_events(i);
    }

    // A new session with Alice: both ways.
    let alice = net.key(m[0]);
    let carol = net.key(m[2]);
    let say = |t: &str| {
        Content::Text {
            sent_at: 1_793_000_000,
            text: t.into(),
        }
        .encode()
    };
    net.with_core(m[2], |c, now| {
        c.send_datagram(&alice, say("back on a new phone"), now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(
        direct_texts(&net.take_events(m[0])),
        ["back on a new phone"]
    );
    net.with_core(m[0], |c, now| {
        c.send_datagram(&carol, say("welcome back"), now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(direct_texts(&net.take_events(m[2])), ["welcome back"]);

    // Bob posts: Carol holds it, asks the admin for the keys a minute on, and reads it.
    net.with_core(m[1], |c, now| c.send_group_text(&gid, "from bob", 1, now))
        .unwrap()
        .unwrap();
    settle(&mut net);
    assert_eq!(
        group_texts(&net.take_events(m[2])),
        vec![(keys[1][0], "from bob".to_owned())]
    );
    // Everyone reads Carol under her new key.
    net.with_core(m[2], |c, now| c.send_group_text(&gid, "from carol", 1, now))
        .unwrap()
        .unwrap();
    settle(&mut net);
    for &i in [m[0], m[1], m[3]].iter() {
        let got = group_texts(&net.take_events(i));
        assert!(
            got.contains(&(keys[2][0], "from carol".to_owned())),
            "{got:?}"
        );
    }
    for p in [old.dir, new.dir] {
        let _ = std::fs::remove_dir_all(p);
    }
}
