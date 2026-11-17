//! The store encrypted at rest (ADR-0036). Without SQLCipher built in, a key is refused, never
//! ignored. With it (`cargo test -p lc-core --features sqlcipher --test sqlcipher`; on Apple
//! platforms it uses CommonCrypto): a plain store is encrypted in place, another key does not
//! open it, and a backup of it restores.

use std::path::PathBuf;

use lc_core::cipher;
use lc_core::store::Store;

const KEY: [u8; 32] = [0x6B; 32];

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lichen-sqlcipher-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(not(feature = "sqlcipher"))]
#[test]
fn a_key_without_sqlcipher_is_refused() {
    let d = tmp("refused");
    let db = d.join("store.sqlite");
    const { assert!(!cipher::AVAILABLE) };
    assert!(Store::open_encrypted(&db, &KEY).is_err());
    assert!(!db.exists(), "nothing written in clear");
    drop(Store::open(&db).unwrap());
    assert!(Store::open_encrypted(&db, &KEY).is_err());
    assert!(cipher::is_plain(&db).unwrap(), "left as it was");
    let _ = std::fs::remove_dir_all(d);
}

#[cfg(feature = "sqlcipher")]
mod with_sqlcipher {
    use std::path::Path;

    use lc_core::content::Content;
    use lc_core::store::{Chat, Message, Status};

    use super::*;

    fn text(t: &str) -> Message {
        Message {
            id: 0,
            chat: Chat::Contact([0xCA; 32]),
            outgoing: false,
            author: None,
            content: Content::Text {
                sent_at: 1_793_000_000,
                text: t.into(),
            },
            at_ms: 1_793_000_000_000,
            status: Status::Received,
            fragments: 1,
            airtime_ms: 0,
            snr4: None,
            read: true,
            msg_ref: None,
        }
    }

    fn texts(s: &Store) -> Vec<String> {
        s.history(&Chat::Contact([0xCA; 32]), None, 100)
            .unwrap()
            .into_iter()
            .filter_map(|m| match m.content {
                Content::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    fn contains(path: &Path, needle: &[u8]) -> bool {
        std::fs::read(path)
            .unwrap()
            .windows(needle.len())
            .any(|w| w == needle)
    }

    #[test]
    fn a_plain_store_is_encrypted_in_place() {
        const { assert!(cipher::AVAILABLE) };
        let d = tmp("migrate");
        let db = d.join("store.sqlite");
        {
            let s = Store::open(&db).unwrap();
            s.insert(&text("At the refuge?")).unwrap();
            s.insert(&text("Yes, by the north path")).unwrap();
        }
        assert!(cipher::is_plain(&db).unwrap());
        assert!(contains(&db, b"north path"));
        // An interrupted migration left its copy behind: it starts again from the plain file.
        std::fs::write(d.join("store.sqlite-encrypting"), b"half written").unwrap();

        let s = Store::open_encrypted(&db, &KEY).unwrap();
        assert_eq!(texts(&s), ["At the refuge?", "Yes, by the north path"]);
        s.insert(&text("written encrypted")).unwrap();
        drop(s);
        assert!(!cipher::is_plain(&db).unwrap());
        assert!(!contains(&db, b"north path") && !contains(&db, b"written encrypted"));
        assert!(!d.join("store.sqlite-encrypting").exists());

        // Opened again with its key; not with another, nor in clear.
        let s = Store::open_encrypted(&db, &KEY).unwrap();
        assert_eq!(texts(&s).len(), 3);
        drop(s);
        assert!(Store::open_encrypted(&db, &[0x6C; 32]).is_err());
        assert!(Store::open(&db).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_new_store_is_encrypted_from_the_start() {
        let d = tmp("new");
        let db = d.join("store.sqlite");
        let s = Store::open_encrypted(&db, &KEY).unwrap();
        s.insert(&text("first words")).unwrap();
        drop(s);
        assert!(!cipher::is_plain(&db).unwrap());
        assert!(!contains(&db, b"first words"));
        let _ = std::fs::remove_dir_all(d);
    }

    /// ADR-0037 with ADR-0036: a backup of an encrypted store holds a plain copy (the backup
    /// encrypts it), and its restore is encrypted again at the next start.
    #[test]
    fn backups_of_an_encrypted_store() {
        use lc_core::backup::{self, Sources};
        let d = tmp("backup");
        let db = d.join("id.sqlite");
        let s = Store::open_encrypted(&db, &KEY).unwrap();
        s.insert(&text("kept in the backup")).unwrap();
        // The app's own connection stays open meanwhile.
        let file = d.join("lichen.backup");
        let made = backup::export(
            &Sources {
                database: &db,
                key: Some(&KEY),
                downloads: None,
                settings: b"",
                drop_settings: &[],
                work: &d.join("work"),
            },
            &file,
            &[1; 32],
            [2; 32],
        )
        .unwrap();
        assert_eq!(made.messages, 1);
        s.insert(&text("after the backup")).unwrap();
        drop(s);

        let other = d.join("other");
        let restored = other.join("id.sqlite");
        backup::unpack(&file, &[1; 32], &other.join("restore"))
            .unwrap()
            .install(&restored, &other.join("downloads"))
            .unwrap();
        assert!(
            cipher::is_plain(&restored).unwrap(),
            "installed as the backup held it"
        );
        let s = Store::open_encrypted(&restored, &[0x7A; 32]).unwrap();
        assert_eq!(texts(&s), ["kept in the backup"]);
        drop(s);
        assert!(!cipher::is_plain(&restored).unwrap());
        let _ = std::fs::remove_dir_all(d);
    }

    /// The first start after the update on a large history (ADR-0036's figure):
    /// `cargo test -p lc-core --features sqlcipher --release --test sqlcipher -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn migration_time() {
        use std::time::Instant;
        let d = tmp("timing");
        let db = d.join("store.sqlite");
        drop(Store::open(&db).unwrap());
        {
            let mut c = rusqlite::Connection::open(&db).unwrap();
            let tx = c.transaction().unwrap();
            {
                let mut msg = tx
                    .prepare(
                        "INSERT INTO messages (chat_kind, chat, outgoing, author, content, at_ms,
                         status, status_a, status_b, fragments, airtime_ms, snr4, read, ref)
                         VALUES (0, ?1, ?2, NULL, ?3, ?4, 2, 0, 0, 1, 420, 20, 1, ?5)",
                    )
                    .unwrap();
                for i in 0..100_000u32 {
                    let chat = [(i % 40) as u8; 32];
                    let content = Content::Text {
                        sent_at: 1_793_000_000 + i,
                        text: format!(
                            "Message {i}: on se retrouve au refuge avant la nuit, par le col nord ?"
                        ),
                    }
                    .encode();
                    let r = i.to_le_bytes().repeat(2);
                    msg.execute(rusqlite::params![
                        chat.as_slice(),
                        i % 2 == 0,
                        content,
                        1_793_000_000_000i64 + i64::from(i) * 1000,
                        r
                    ])
                    .unwrap();
                }
                let mut media = tx
                    .prepare(
                        "INSERT INTO media (id, chat_kind, chat, datagram) VALUES (?1, 0, ?2, ?3)",
                    )
                    .unwrap();
                for i in 0..300u64 {
                    let datagram: Vec<u8> =
                        (0..9_000u32).map(|b| (b * 7 + i as u32) as u8).collect();
                    media
                        .execute(rusqlite::params![
                            i.to_le_bytes().as_slice(),
                            [1u8; 32].as_slice(),
                            datagram
                        ])
                        .unwrap();
                }
            }
            tx.commit().unwrap();
        }
        let plain = std::fs::metadata(&db).unwrap().len();
        let t = Instant::now();
        let s = Store::open_encrypted(&db, &KEY).unwrap();
        let migrated = t.elapsed();
        assert_eq!(s.message_count().unwrap(), 100_000);
        drop(s);
        let t = Instant::now();
        let s = Store::open_encrypted(&db, &KEY).unwrap();
        let reopened = t.elapsed();
        let t = Instant::now();
        let page = s.history(&Chat::Contact([7; 32]), None, 100).unwrap();
        let read = t.elapsed();
        assert_eq!(page.len(), 100);
        println!(
            "100,000 messages and 300 media, {:.1} MB in clear: encrypted in place in {:.2} s; \
             opened again in {:.0} ms; a page of 100 messages read in {:.1} ms",
            plain as f64 / 1e6,
            migrated.as_secs_f64(),
            reopened.as_secs_f64() * 1e3,
            read.as_secs_f64() * 1e3
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
