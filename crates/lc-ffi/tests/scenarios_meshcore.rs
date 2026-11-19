//! MeshCore chats and channels (docs/testing.md §5, MC-1 to MC-4): Marc and Lucie have the
//! stock MeshCore app.

mod common;

use common::{Sim, TempDir, find_contact, text, texts};
use lc_ffi::{ChannelKind, ChatRef, LichenError, LichenEvent, MessageStatus};

/// The public channel's conversation.
fn public(sim: &mut Sim) -> ChatRef {
    let id = sim.until_some("the public channel", 10, |c| {
        c.channels()
            .into_iter()
            .find(|ch| ch.kind == ChannelKind::PublicChannel)
            .map(|ch| ch.id)
    });
    ChatRef::Channel { id }
}

/// Sends a plain text and waits for its end, retried once as the app offers: a direct plain
/// text has three tries in the radio, and the demo's hub is busy.
fn send_plain_until_delivered(sim: &mut Sim, chat: &ChatRef, s: &str) -> lc_ffi::StoredMessage {
    let m = sim
        .core
        .send_plain(chat.clone(), s.into(), sim.unix_s(), sim.now)
        .unwrap();
    let m = sim.until_status(chat, m.id, 120, |s| {
        matches!(s, MessageStatus::Delivered | MessageStatus::Failed { .. })
    });
    if m.status == MessageStatus::Delivered {
        return m;
    }
    sim.core.retry_message(m.id, sim.now).unwrap();
    sim.until_status(chat, m.id, 120, |s| *s == MessageStatus::Delivered)
}

/// MC-1: Marc's plain message arrives in a MeshCore chat under his name; the answer reaches
/// him, and he answers once. What only lichen does is refused there.
#[test]
fn mc_1_plain_chat_with_marc() {
    let mut sim = Sim::new(121);
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh {
        key: marc.key.clone(),
    };
    let (label, said) = sim.until_event("Marc's message", 240, |e| match e {
        LichenEvent::PlainReceived { chat, label, text } if *chat == mesh => {
            Some((label.clone(), text.clone()))
        }
        _ => None,
    });
    assert_eq!(label.as_deref(), Some("Marc"));
    assert!(said.starts_with("Hi! Marc from the refuge."));
    let row = sim.conversation(&mesh).unwrap();
    assert_eq!((row.name.as_str(), row.unread), ("Marc", 1));
    assert!(!row.last.referable, "no replies or reactions in plain");
    assert!(
        sim.conversation(&ChatRef::Contact {
            key: marc.key.clone()
        })
        .is_none()
    );
    // Its limits: 160 bytes, plain text only.
    assert_eq!(sim.core.plain_text_max(mesh.clone()).unwrap(), 160);
    assert!(
        sim.core
            .estimate_plain(mesh.clone(), 40)
            .unwrap()
            .airtime_ms
            > 0
    );
    assert!(
        sim.core
            .send_plain(mesh.clone(), "x".repeat(161), sim.unix_s(), sim.now)
            .is_err()
    );
    assert!(
        sim.core
            .send_sticker(mesh.clone(), "🥳".into(), sim.unix_s(), sim.now)
            .is_err()
    );
    assert_eq!(
        sim.core
            .send_plain(
                ChatRef::Contact { key: marc.key },
                "x".into(),
                sim.unix_s(),
                sim.now
            )
            .err(),
        Some(LichenError::NotAllowed)
    );
    // The answer, once the hub is quiet (its first minutes are busy).
    sim.run_to(330_000);
    send_plain_until_delivered(&mut sim, &mesh, "Yes! Write to me here.");
    let reply = "It works! See you tonight, I'll keep the soup warm.";
    sim.until("Marc's answer", 120, |c| {
        texts(&c.history(mesh.clone(), None, 50).unwrap()).contains(&reply.to_owned())
    });
    sim.run_for(60_000);
    let h = sim.history(&mesh);
    assert_eq!(common::copies(&h, reply), 1);
    assert_eq!(common::copies(&h, "Yes! Write to me here."), 1);
    assert!(h.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
}

/// MC-2: the public channel: posts with their authors, in order; ours floods once, within the
/// channel's shorter limit.
#[test]
fn mc_2_public_channel_read_and_post() {
    let mut sim = Sim::new(122);
    let chat = public(&mut sim);
    let label = sim.until_event("Lucie's post", 60, |e| match e {
        LichenEvent::PlainReceived { chat: c, label, .. } if *c == chat => Some(label.clone()),
        _ => None,
    });
    assert_eq!(label.as_deref(), Some("Lucie"));
    let first = sim.history(&chat);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].author_name.as_deref(), Some("Lucie"));
    assert_eq!(
        text(&first[0].message),
        Some("Fog on the Pas de la Ville this morning, take care on the ridge.")
    );
    assert_eq!(sim.conversation(&chat).unwrap().name, "Public");
    // Our name goes in front of the post: the limit is shorter.
    let max = sim.core.plain_text_max(chat.clone()).unwrap();
    assert_eq!(max as usize, 160 - sim.core.mesh_name().len() - 2);
    assert!(
        sim.core
            .send_plain(
                chat.clone(),
                "x".repeat(max as usize + 1),
                sim.unix_s(),
                sim.now
            )
            .is_err()
    );
    let m = sim
        .core
        .send_plain(
            chat.clone(),
            "Heading up to the refuge, see you there.".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let m = sim.until_status(&chat, m.id, 60, |s| {
        matches!(s, MessageStatus::Flooded | MessageStatus::Failed { .. })
    });
    assert_eq!(m.status, MessageStatus::Flooded);
    assert!(m.outgoing && m.author_name.is_none());
    assert_eq!(
        text(&m.message),
        Some("Heading up to the refuge, see you there."),
        "shown without our name"
    );
    // Later posts, Lucie's and Marc's, in order.
    sim.until("Marc's post", 400, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .iter()
            .any(|m| m.author_name.as_deref() == Some("Marc"))
    });
    let h = sim.history(&chat);
    let authors: Vec<Option<&str>> = h.iter().map(|m| m.author_name.as_deref()).collect();
    assert!(authors.len() >= 4, "{authors:?}");
    assert_eq!(authors.first(), Some(&Some("Lucie")));
    assert_eq!(authors.last(), Some(&Some("Marc")));
    assert!(h.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    assert_eq!(
        common::copies(&h, "Heading up to the refuge, see you there."),
        1
    );
}

/// MC-3: a channel's QR code, as the stock app writes it; scanning one joins the channel, and
/// its own code reads back; leaving keeps its history.
#[test]
fn mc_3_channel_qr_show_and_join() {
    let mut sim = Sim::new(123);
    let chat = public(&mut sim);
    let ChatRef::Channel { id } = chat.clone() else {
        unreachable!()
    };
    let qr = sim.core.channel_qr(id.clone()).unwrap();
    assert_eq!(
        qr,
        "meshcore://channel/add?name=Public&secret=8b3387e9c5cdea6ac9e5edbaa115cd72"
    );
    let code = sim.core.parse_channel_qr(qr).unwrap();
    assert!(code.joined);
    assert_eq!(code.kind, ChannelKind::PublicChannel);

    // A private channel scanned from someone's phone.
    let scanned = "meshcore://channel/add?name=Hut%20team&secret=00112233445566778899aabbccddeeff";
    let code = sim.core.parse_channel_qr(scanned.into()).unwrap();
    assert_eq!(
        (code.name.as_str(), code.kind, code.joined),
        ("Hut team", ChannelKind::PrivateChannel, false)
    );
    let hut = sim
        .core
        .join_private_channel(code.name.clone(), code.secret_hex.clone())
        .unwrap();
    sim.until("the new channel on the radio", 20, |c| {
        c.channels().iter().any(|ch| ch.name == "Hut team")
    });
    let ChatRef::Channel { id: hut_id } = hut.clone() else {
        unreachable!()
    };
    let back = sim
        .core
        .parse_channel_qr(sim.core.channel_qr(hut_id.clone()).unwrap())
        .unwrap();
    assert!(back.joined);
    assert_eq!(back.secret_hex, code.secret_hex);
    // Joined muted (ADR-0026).
    assert!(
        sim.core
            .chat_prefs()
            .iter()
            .any(|p| p.chat == hut && p.muted_until_ms.is_some())
    );
    // A hashtag channel.
    let tag = sim.core.join_hashtag_channel("#Hiking".into()).unwrap();
    sim.until("the hashtag channel", 20, |c| {
        c.channels()
            .iter()
            .any(|ch| ch.kind == ChannelKind::HashtagChannel)
    });
    let ChatRef::Channel { id: tag_id } = tag else {
        unreachable!()
    };
    let tag_code = sim
        .core
        .parse_channel_qr(sim.core.channel_qr(tag_id).unwrap())
        .unwrap();
    assert_eq!(tag_code.kind, ChannelKind::HashtagChannel);
    // Our post there, then we leave: gone from the radio, its history stays.
    sim.core
        .send_plain(
            hut.clone(),
            "Key under the stone".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.run_for(10_000);
    sim.core.leave_channel(hut_id).unwrap();
    sim.until("the channel left", 20, |c| {
        !c.channels().iter().any(|ch| ch.name == "Hut team")
    });
    assert!(!sim.history(&hut).is_empty());
    // Not a code; a secret of zeros.
    assert_eq!(
        sim.core.parse_channel_qr("https://example.org".into()),
        Err(LichenError::BadCode)
    );
    assert!(
        sim.core
            .join_private_channel("Zero".into(), "0".repeat(32))
            .is_err()
    );
}

/// The MeshCore app's export ("export app database", its schema 29): Marc's messages both
/// ways and a public channel post, as the stock app keeps them.
fn export(dir: &TempDir, me: &[u8], marc: &[u8], at_s: i64) -> String {
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let path = dir.0.join("meshcore-export.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        r#"CREATE TABLE "contact_messages" ("id" INTEGER PRIMARY KEY, "status" TEXT NOT NULL, "to" TEXT NOT NULL, "from" TEXT NOT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL);
           CREATE TABLE "channel_messages" ("id" INTEGER PRIMARY KEY, "channel_secret" BLOB NULL, "from" TEXT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL);
           CREATE TABLE "contacts" ("id" INTEGER PRIMARY KEY, "public_key" BLOB NOT NULL, "adv_name" TEXT NOT NULL, "custom_name" TEXT NULL);"#,
    )
    .unwrap();
    let insert = r#"INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6)"#;
    db.execute(
        insert,
        rusqlite::params![
            "received",
            hex(me),
            hex(marc),
            at_s,
            "Are you coming up this weekend?",
            at_s * 1000
        ],
    )
    .unwrap();
    db.execute(
        insert,
        rusqlite::params![
            "delivered",
            hex(marc),
            hex(me),
            at_s + 60,
            "Saturday, with a friend.",
            (at_s + 60) * 1000
        ],
    )
    .unwrap();
    db.execute(
        r#"INSERT INTO channel_messages (channel_secret, "from", txt_type, sender_timestamp, text, timestamp) VALUES (?1, NULL, 0, ?2, 'Lucie: Snow above 1800 m', ?3)"#,
        rusqlite::params![
            [
                0x8Bu8, 0x33, 0x87, 0xE9, 0xC5, 0xCD, 0xEA, 0x6A, 0xC9, 0xE5, 0xED, 0xBA, 0xA1, 0x15,
                0xCD, 0x72
            ]
            .as_slice(),
            at_s + 120,
            (at_s + 120) * 1000
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO contacts (public_key, adv_name, custom_name) VALUES (?1, 'Marc', NULL)",
        rusqlite::params![marc],
    )
    .unwrap();
    path.display().to_string()
}

/// MC-4: the MeshCore app's history comes into the same chats, older than what lichen got, and
/// only once.
#[test]
fn mc_4_import_the_meshcore_app_history() {
    let mut sim = Sim::new(124);
    let chat = public(&mut sim);
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh {
        key: marc.key.clone(),
    };
    sim.until("Marc's message", 240, |c| {
        !c.history(mesh.clone(), None, 10).unwrap().is_empty()
    });
    let dir = TempDir::new("mc4");
    let a_week_ago = i64::from(sim.unix_s()) - 7 * 86_400;
    let path = export(&dir, &sim.core.node_key().unwrap(), &marc.key, a_week_ago);
    let r = sim.core.import_meshcore_app(path.clone()).unwrap();
    assert_eq!(
        (r.direct, r.channel, r.already_here, r.skipped),
        (2, 1, 0, 0)
    );
    let h = sim.history(&mesh);
    assert_eq!(
        texts(&h)[..2],
        [
            "Are you coming up this weekend?".to_owned(),
            "Saturday, with a friend.".to_owned()
        ],
        "older, first"
    );
    assert!(!h[0].outgoing && h[1].outgoing);
    assert!(texts(&h)[2].starts_with("Hi! Marc from the refuge."));
    let posts = sim.history(&chat);
    assert_eq!(posts[0].author_name.as_deref(), Some("Lucie"));
    assert_eq!(text(&posts[0].message), Some("Snow above 1800 m"));
    // The row in Messages still shows the latest.
    assert!(
        text(&sim.conversation(&mesh).unwrap().last.message)
            .is_some_and(|t| t.starts_with("Hi! Marc"))
    );
    // A second import adds nothing.
    let again = sim.core.import_meshcore_app(path).unwrap();
    assert_eq!((again.direct, again.channel, again.already_here), (0, 0, 3));
    assert_eq!(sim.history(&mesh).len(), h.len());
    assert_eq!(find_contact(&sim.core, "Marc").unwrap().name, "Marc");
    // Not an export.
    let junk = dir.0.join("junk.db");
    std::fs::write(&junk, b"nothing").unwrap();
    assert_eq!(
        sim.core.import_meshcore_app(junk.display().to_string()),
        Err(LichenError::NotAnExport)
    );
}
