//! Lifecycle (docs/testing.md §10, LC-1 to LC-6): notifications, mute, archive, pin, deleting a
//! conversation, restarts, the app in the background, every conversation listed.

mod common;

use common::{Sim, contact_chat, copies, find_contact, text, texts};
use lc_ffi::{ChatRef, LichenEvent, LinkKind, MessageStatus, ServerLogin, SharePrecision};

/// The conversation an event opens, as the apps' notifications do.
fn chat_of(e: &LichenEvent) -> Option<ChatRef> {
    match e {
        LichenEvent::Received { from, .. } => Some(ChatRef::Contact { key: from.clone() }),
        LichenEvent::PlainReceived { chat, .. } => Some(chat.clone()),
        LichenEvent::GroupMessage { gid, .. } => Some(ChatRef::Group { gid: gid.clone() }),
        _ => None,
    }
}

/// LC-1: each kind's notification names the conversation that holds its message: lichen,
/// MeshCore, channel, room and group.
#[test]
fn lc_1_a_notification_opens_the_right_chat() {
    let mut sim = Sim::new(171);
    let board = sim.contact("Refuge board").key;
    sim.core
        .server_login(board.clone(), "hello".into(), true, sim.now)
        .unwrap();
    sim.rando();
    sim.run_to(400_000);
    let mut kinds = std::collections::BTreeSet::new();
    for (_, e) in sim.events.clone() {
        let Some(chat) = chat_of(&e) else {
            continue;
        };
        let kind = match &chat {
            ChatRef::Contact { .. } => "lichen",
            ChatRef::Mesh { .. } => "MeshCore",
            ChatRef::Channel { .. } => "channel",
            ChatRef::Room { .. } => "room",
            ChatRef::Group { .. } => "group",
        };
        kinds.insert(kind);
        let row = sim
            .conversation(&chat)
            .unwrap_or_else(|| panic!("no row for {kind}: {e:?}"));
        assert!(!row.name.is_empty(), "{kind}");
        // The text notified is in that conversation.
        let said = match &e {
            LichenEvent::Received { message, .. } | LichenEvent::GroupMessage { message, .. } => {
                text(message).map(str::to_owned)
            }
            LichenEvent::PlainReceived { text, .. } => Some(text.clone()),
            _ => None,
        };
        if let Some(said) = said {
            assert!(
                texts(&sim.history(&chat))
                    .iter()
                    .any(|t| t.ends_with(&said)),
                "{kind}: {said:?} not in its chat"
            );
        }
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["MeshCore", "channel", "group", "lichen", "room"]
    );
}

/// LC-2: muted (no notification wanted, until a time), archived (back with a message unless
/// muted), pinned (latest first).
#[test]
fn lc_2_mute_archive_pin() {
    let mut sim = Sim::new(172);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh {
        key: marc.key.clone(),
    };
    // Marc's chat archived and muted before he writes: it stays archived.
    sim.core.set_archived(mesh.clone(), true).unwrap();
    sim.core.set_muted(mesh.clone(), Some(i64::MAX)).unwrap();
    // Camille's archived after her hello, not muted: her next message brings it back.
    sim.until("her hello", 120, |c| {
        !c.history(chat.clone(), None, 5).unwrap().is_empty()
    });
    sim.core.set_archived(chat.clone(), true).unwrap();
    // A conversation back to its defaults has no preferences row.
    let prefs = |sim: &Sim, c: &ChatRef| {
        sim.core
            .chat_prefs()
            .into_iter()
            .find(|p| p.chat == *c)
            .unwrap_or(lc_ffi::ChatPrefs {
                chat: c.clone(),
                pinned_at_ms: None,
                muted_until_ms: None,
                archived: false,
            })
    };
    assert!(prefs(&sim, &chat).archived);
    sim.until("Marc's message", 240, |c| {
        !c.history(mesh.clone(), None, 5).unwrap().is_empty()
    });
    assert!(prefs(&sim, &mesh).archived, "muted: stays archived");
    assert_eq!(prefs(&sim, &mesh).muted_until_ms, Some(i64::MAX));
    let from = sim.events.len();
    sim.until("her next message", 400, |c| {
        texts(&c.history(chat.clone(), None, 50).unwrap())
            .iter()
            .any(|t| t.starts_with("The view from the refuge"))
    });
    sim.run_for(1_000);
    assert!(!prefs(&sim, &chat).archived, "back with a message");
    assert!(
        sim.since(from)
            .any(|e| matches!(e, LichenEvent::ChatPrefsChanged))
    );
    // Our own message does not bring a chat back.
    sim.core.set_archived(chat.clone(), true).unwrap();
    sim.core
        .send_text(camille.key.clone(), "Thanks".into(), sim.unix_s(), sim.now)
        .unwrap();
    assert!(prefs(&sim, &chat).archived);
    sim.core.set_archived(chat.clone(), false).unwrap();
    // Muted for an hour; unmuted.
    let in_an_hour = sim.unix_ms() + 3_600_000;
    sim.core.set_muted(chat.clone(), Some(in_an_hour)).unwrap();
    assert_eq!(prefs(&sim, &chat).muted_until_ms, Some(in_an_hour));
    sim.core.set_muted(chat.clone(), None).unwrap();
    // Pinned: the latest pinned first.
    sim.core.set_pinned(mesh.clone(), true).unwrap();
    sim.run_for(1_000);
    sim.core.set_pinned(chat.clone(), true).unwrap();
    let (a, b) = (
        prefs(&sim, &chat).pinned_at_ms.unwrap(),
        prefs(&sim, &mesh).pinned_at_ms.unwrap(),
    );
    assert!(a > b, "{a} {b}");
    sim.core.set_pinned(chat.clone(), false).unwrap();
    assert_eq!(prefs(&sim, &chat).pinned_at_ms, None);
}

/// LC-3: a conversation deleted is gone with its messages; the contact stays, and a new message
/// starts it again.
#[test]
fn lc_3_delete_a_conversation() {
    let mut sim = Sim::new(173);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    sim.until("her hello", 60, |c| {
        !c.history(chat.clone(), None, 5).unwrap().is_empty()
    });
    sim.core.delete_conversation(chat.clone()).unwrap();
    assert!(sim.history(&chat).is_empty());
    assert!(sim.conversation(&chat).is_none());
    assert!(
        sim.core
            .search_messages("mobile network".into(), 10)
            .is_empty()
    );
    let c = find_contact(&sim.core, "Camille").unwrap();
    assert_eq!(c.identity, camille.identity, "the contact stays");
    // Her photo's caption starts it again, alone.
    sim.until("a new message", 400, |c| {
        c.conversations().iter().any(|v| v.chat == chat)
    });
    let h = sim.history(&chat);
    assert!(
        !texts(&h)
            .iter()
            .any(|t| t.starts_with("Hi! I'm at the refuge")),
        "{:?}",
        texts(&h)
    );
}

/// LC-4: a restart keeps messages, contacts and nicknames, groups and their keys, preferences,
/// timers, saved radios, shares, channels and the room's login; what comes after arrives.
#[test]
fn lc_4_restart_keeps_everything() {
    let mut sim = Sim::persistent(174, "lc4");
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let m = sim
        .core
        .send_text(
            camille.key.clone(),
            "Before the restart".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.until_status(&chat, m.id, 300, |s| *s == MessageStatus::Delivered);
    let rando = sim.rando();
    let julien = find_contact(&sim.core, "Julien").unwrap();
    sim.core
        .rename_contact(julien.key.clone(), Some("Jules".into()))
        .unwrap();
    let ours = sim
        .core
        .create_group("Sortie".into(), vec![camille.key.clone()], sim.now)
        .unwrap();
    let gchat = ChatRef::Group { gid: ours.clone() };
    sim.core.set_disappearing(gchat.clone(), 3_600).unwrap();
    sim.core.set_pinned(chat.clone(), true).unwrap();
    sim.core.set_muted(gchat.clone(), Some(i64::MAX)).unwrap();
    sim.core
        .remember_radio("BLE-M5".into(), LinkKind::Bluetooth, "M5".into())
        .unwrap();
    sim.core
        .start_share(
            chat.clone(),
            SharePrecision::M100,
            u64::from(sim.unix_s()) + 7_200,
            sim.now,
        )
        .unwrap();
    let tag = sim.core.join_hashtag_channel("#vercors".into()).unwrap();
    let board = sim.contact("Refuge board").key;
    sim.core
        .server_login(board.clone(), "hello".into(), true, sim.now)
        .unwrap();
    sim.until("logged in to the board", 60, |c| {
        c.server(board.clone()).unwrap().login == ServerLogin::ReadWrite
    });
    sim.run_for(20_000);
    let before = (
        sim.core.conversations(),
        sim.core.contacts(),
        sim.core.chat_prefs(),
        sim.history(&chat),
        sim.history(&ChatRef::Group {
            gid: rando.gid.clone(),
        }),
    );

    sim.restart(2_000);
    // Before the radio answers: everything from the store.
    assert_eq!(sim.core.conversations(), before.0);
    assert_eq!(sim.core.contacts(), before.1);
    assert_eq!(sim.core.chat_prefs(), before.2);
    assert_eq!(sim.history(&chat), before.3);
    assert_eq!(
        sim.history(&ChatRef::Group {
            gid: rando.gid.clone()
        }),
        before.4
    );
    assert_eq!(find_contact(&sim.core, "Jules").unwrap().key, julien.key);
    let groups = sim.core.groups();
    let r = groups.iter().find(|g| g.gid == rando.gid).unwrap();
    assert_eq!(r.keys_known, rando.keys_known, "the members' keys kept");
    assert!(groups.iter().any(|g| g.gid == ours));
    assert_eq!(sim.core.disappearing(gchat).unwrap(), 3_600);
    assert_eq!(sim.core.saved_radios().len(), 1);
    assert_eq!(sim.core.shares().len(), 1);
    assert!(sim.core.server(board.clone()).unwrap().remembered);
    let ChatRef::Channel { id: tag_id } = tag else {
        unreachable!()
    };
    assert!(sim.core.channels().iter().any(|c| c.id == tag_id));
    // Connected again: the room logs in by itself, and the group's posts still open.
    sim.until("the board again", 60, |c| {
        c.server(board.clone()).unwrap().login == ServerLogin::ReadWrite
    });
    let rchat = ChatRef::Group {
        gid: rando.gid.clone(),
    };
    sim.until("Julien's post after the restart", 900, |c| {
        texts(&c.history(rchat.clone(), None, 100).unwrap())
            .iter()
            .any(|t| t.starts_with("Leaving the col"))
    });
    let m = sim
        .core
        .send_text(
            camille.key.clone(),
            "After the restart".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.until_status(&chat, m.id, 300, |s| *s == MessageStatus::Delivered);
    assert_eq!(copies(&sim.history(&chat), "Before the restart"), 1);
}

/// LC-4: a request and a group invitation waiting for the user are still there after a
/// restart, and can be answered then.
#[test]
fn lc_4_restart_keeps_requests_and_invitations() {
    // A request (Camille's first message, requests on).
    let mut sim = Sim::persistent_asking(175, "lc4-requests", true, true);
    sim.until("the request and its hello", 120, |c| {
        c.requests().first().is_some_and(|r| !r.messages.is_empty())
    });
    let before = sim.core.requests();
    sim.restart(2_000);
    assert_eq!(sim.core.requests(), before);
    let node = sim.core.accept_request(before[0].identity.clone()).unwrap();
    assert!(
        texts(&sim.history(&ChatRef::Contact { key: node }))
            .iter()
            .any(|t| t.starts_with("Hi! I'm at the refuge"))
    );

    // An invitation (groups asking, requests off).
    let mut sim = Sim::persistent_asking(176, "lc4-invitation", false, true);
    let gid = sim.until_event("the invitation", 300, |e| match e {
        LichenEvent::GroupInvited { gid, .. } => Some(gid.clone()),
        _ => None,
    });
    let before = sim.core.group_invitations();
    sim.restart(2_000);
    assert_eq!(sim.core.group_invitations(), before);
    assert!(sim.core.groups().is_empty());
    sim.core.accept_group(gid.clone(), sim.now).unwrap();
    sim.until("joined after the restart", 60, |c| {
        c.groups().iter().any(|g| g.gid == gid)
    });
}

/// LC-5: the app asleep (not pumped for five minutes, the radio still on): the MeshCore
/// messages the radio kept meanwhile are there once it runs again, once each, and nothing is
/// doubled; then the same with the radio dropped too.
#[test]
fn lc_5_background_and_back() {
    let mut sim = Sim::new(177);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    // Asleep from 40 s to 340 s: nothing pumped.
    sim.run_to(40_000);
    sim.now = 340_000;
    sim.run_for(300_000);
    let marc = find_contact(&sim.core, "Marc").unwrap();
    let mesh = ChatRef::Mesh { key: marc.key };
    let public = sim
        .core
        .channels()
        .into_iter()
        .find(|c| c.name == "Public")
        .map(|c| ChatRef::Channel { id: c.id })
        .unwrap();
    // Lichen texts sent while we slept got no acknowledgement: their senders gave them up
    // (§6.1, nothing retried by itself), and they may come or not. What came, came once.
    let h = sim.history(&chat);
    assert_eq!(
        copies(
            &h,
            "Hi! I'm at the refuge, the mobile network doesn't reach up here."
        ),
        1
    );
    // Marc's stock client sends once, without a retry (the demo's): it may collide.
    assert!(
        sim.history(&mesh).len() <= 1,
        "Marc's message, once at most"
    );
    // The posts the radio kept while we slept (110 s, 280 s), each once.
    let posts = texts(&sim.history(&public));
    for t in &posts {
        assert_eq!(posts.iter().filter(|x| *x == t).count(), 1, "{posts:?}");
    }
    for kept in [
        "The fog is lifting, the ridge is clear now.",
        "Sun on the plateau, the path to the refuge is dry.",
    ] {
        assert!(posts.iter().any(|t| t == kept), "{kept:?} not in {posts:?}");
    }
    // With the radio dropped too (Bluetooth lost in the background): the same once it is back.
    sim.set_radio(false);
    sim.now += 120_000;
    sim.set_radio(true);
    sim.run_for(300_000);
    let again = sim.history(&chat);
    for m in &again {
        assert_eq!(
            again
                .iter()
                .filter(|x| x.message == m.message && x.outgoing == m.outgoing)
                .count(),
            1,
            "{:?}",
            m.message
        );
    }
}

/// LC-6: every kind of conversation with a message has its row, named.
#[test]
fn lc_6_every_conversation_listed() {
    let mut sim = Sim::new(178);
    let board = sim.contact("Refuge board").key;
    sim.core
        .server_login(board.clone(), "hello".into(), true, sim.now)
        .unwrap();
    let camille = sim.camille();
    let rando = sim.rando();
    sim.run_to(400_000);
    let marc = find_contact(&sim.core, "Marc").unwrap();
    let rows = sim.core.conversations();
    let name = |c: &ChatRef| {
        rows.iter()
            .find(|r| r.chat == *c)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| panic!("no row for {c:?}"))
    };
    assert_eq!(name(&contact_chat(&camille)), "Camille");
    assert_eq!(name(&ChatRef::Mesh { key: marc.key }), "Marc");
    assert_eq!(name(&ChatRef::Room { key: board }), "Refuge board");
    assert_eq!(name(&ChatRef::Group { gid: rando.gid }), "Rando Vercors");
    let public = rows
        .iter()
        .find(|r| matches!(r.chat, ChatRef::Channel { .. }))
        .unwrap();
    assert_eq!(public.name, "Public");
    // Most recent first.
    assert!(rows.windows(2).all(|w| w[0].last.at_ms >= w[1].last.at_ms));
}
