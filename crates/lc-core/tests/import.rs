//! History from the MeshCore app's exported database (ADR-0033), on a made-up export with the
//! app's own tables (schema 29).

use lc_core::Content;
use lc_core::core::FailReason;
use lc_core::import::{ImportError, meshcore_app};
use lc_core::plain::channel_id;
use lc_core::store::{Chat, Status};
use rusqlite::{Connection, params};

const SCHEMA: &str = r#"
CREATE TABLE "contact_messages" ("id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT, "attempt" INTEGER NULL, "status" TEXT NOT NULL, "to" TEXT NOT NULL, "from" TEXT NOT NULL, "path_len" INTEGER NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "expected_ack_crc" INTEGER NULL, "send_result" INTEGER NULL, "rtt" INTEGER NULL, "error" TEXT NULL, "timestamp" INTEGER NOT NULL, "room_post_author_pub_key_prefix" TEXT NULL, "snr" REAL NULL, "max_attempts" INTEGER NULL);
CREATE TABLE "channel_messages" ("id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT, "channel_secret" BLOB NULL, "from" TEXT NULL, "path_len" INTEGER NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL, "repeats_heard_count" INTEGER NOT NULL DEFAULT 0, "region_scope_name" TEXT NULL, "region_scope_key" TEXT NULL, "transport_code1" INTEGER NULL, "transport_code2" INTEGER NULL);
CREATE TABLE "contacts" ("id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT, "public_key" BLOB NOT NULL UNIQUE, "type" INTEGER NOT NULL, "flags" INTEGER NOT NULL, "out_path_len" INTEGER NOT NULL, "out_path" BLOB NOT NULL, "adv_name" TEXT NOT NULL, "last_advert" INTEGER NOT NULL, "adv_lat" INTEGER NOT NULL, "adv_lon" INTEGER NOT NULL, "last_mod" INTEGER NOT NULL, "repeater_admin_password" TEXT NULL, "room_password" TEXT NULL, "last_message_sent_or_received_at" INTEGER NULL, "custom_name" TEXT NULL, "draft_message_text" TEXT NULL, "remote_management_is_admin" INTEGER NULL, "remote_management_last_firmware_ver_level" INTEGER NULL);
PRAGMA user_version = 29;
"#;

const ME: [u8; 32] = [0xAA; 32];
const MARC: [u8; 32] = [0x4D; 32];
const LUCIE: [u8; 32] = [0x4C; 32];
const PUBLIC: [u8; 16] = [0x8B; 16];

fn hex(k: &[u8]) -> String {
    k.iter().map(|b| format!("{b:02x}")).collect()
}

fn export(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("lichen-import-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Connection::open(&path).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    let dm = |status: &str, to: &[u8], from: &[u8], kind: i64, ts: i64, text: &str| {
        db.execute(
            r#"INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp, snr)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 6.5)"#,
            params![status, hex(to), hex(from), kind, ts, text, ts * 1000 + 250],
        )
        .unwrap();
    };
    dm("received", &ME, &MARC, 0, 1_793_000_000, "At the refuge?");
    dm(
        "delivered",
        &MARC,
        &ME,
        0,
        1_793_000_060,
        "Yes, see you at 6",
    );
    dm("failed", &LUCIE, &ME, 0, 1_793_000_120, "Lost on the way");
    dm("sending", &LUCIE, &ME, 0, 1_793_000_180, "Never left");
    // A repeater's command line, and a row whose key is not one: left out.
    dm("received", &ME, &MARC, 1, 1_793_000_200, "clock sync");
    db.execute(
        r#"INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp)
           VALUES ('received', ?1, 'not-a-key', 0, 1793000300, 'x', 1793000300000)"#,
        params![hex(&ME)],
    )
    .unwrap();
    let post = |from: Option<String>, ts: i64, text: &str| {
        db.execute(
            r#"INSERT INTO channel_messages (channel_secret, "from", txt_type, sender_timestamp, text, timestamp)
               VALUES (?1, ?2, 0, ?3, ?4, ?5)"#,
            params![PUBLIC.as_slice(), from, ts, text, ts * 1000],
        )
        .unwrap();
    };
    post(None, 1_793_000_010, "Lucie: Fog on the ridge");
    post(
        Some(hex(&ME)),
        1_793_000_020,
        "Thanks, taking the lower path",
    );
    let contact = |k: &[u8], name: &str, custom: Option<&str>| {
        db.execute(
            "INSERT INTO contacts (public_key, type, flags, out_path_len, out_path, adv_name, last_advert, adv_lat, adv_lon, last_mod, custom_name)
             VALUES (?1, 1, 0, 0, x'', ?2, 0, 0, 0, 0, ?3)",
            params![k, name, custom],
        )
        .unwrap();
    };
    contact(&MARC, "Marc refuge", None);
    contact(&LUCIE, "LUC-01", Some("Lucie"));
    path
}

#[test]
fn direct_messages_and_channel_posts_come_in_as_lichen_keeps_them() {
    let path = export("full");
    let got = meshcore_app(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(got.messages.len(), 6);
    assert_eq!(got.skipped, 2);
    let first = &got.messages[0];
    assert_eq!(first.chat, Chat::Mesh(MARC));
    assert!(!first.outgoing);
    assert_eq!(first.author, Some(MARC));
    assert_eq!(first.status, Status::Received);
    assert_eq!(first.at_ms, 1_793_000_000_250);
    assert_eq!(first.snr4, Some(26));
    assert_eq!(
        first.content,
        Content::Text {
            sent_at: 1_793_000_000,
            text: "At the refuge?".into()
        }
    );
    let statuses: Vec<(Chat, bool, Status)> = got
        .messages
        .iter()
        .map(|m| (m.chat, m.outgoing, m.status))
        .collect();
    let public = Chat::Channel(channel_id(&PUBLIC));
    assert_eq!(
        statuses,
        vec![
            (Chat::Mesh(MARC), false, Status::Received),
            (public, false, Status::Received),
            (public, true, Status::Flooded),
            (Chat::Mesh(MARC), true, Status::Delivered),
            (Chat::Mesh(LUCIE), true, Status::Failed(FailReason::Timeout)),
            (
                Chat::Mesh(LUCIE),
                true,
                Status::Failed(FailReason::Interrupted)
            ),
        ]
    );
    assert!(got.messages.iter().all(|m| m.read));
    let mut people = got.people.clone();
    people.sort();
    assert_eq!(
        people,
        vec![
            (LUCIE, "LUC-01".into(), Some("Lucie".into())),
            (MARC, "Marc refuge".into(), None),
        ]
    );
}

#[test]
fn a_file_that_is_not_an_export_is_refused() {
    let path = std::env::temp_dir().join(format!("lichen-import-not-{}.db", std::process::id()));
    std::fs::write(&path, b"not a database at all").unwrap();
    assert_eq!(meshcore_app(&path).unwrap_err(), ImportError::NotMeshCore);
    // A database, but not the MeshCore app's.
    let _ = std::fs::remove_file(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE notes (id INTEGER, text TEXT);")
        .unwrap();
    assert_eq!(meshcore_app(&path).unwrap_err(), ImportError::NotMeshCore);
    let _ = std::fs::remove_file(&path);
}

/// A real export, read locally (`LICHEN_IMPORT=path`): counts only, no text printed.
#[test]
#[ignore = "needs an export"]
fn probe_a_real_export() {
    let Ok(path) = std::env::var("LICHEN_IMPORT") else {
        return;
    };
    let got = meshcore_app(std::path::Path::new(&path)).unwrap();
    let direct = got
        .messages
        .iter()
        .filter(|m| matches!(m.chat, Chat::Mesh(_)))
        .count();
    let channel = got.messages.len() - direct;
    println!(
        "direct {direct}, channel {channel}, skipped {}, people {}",
        got.skipped,
        got.people.len()
    );
}
