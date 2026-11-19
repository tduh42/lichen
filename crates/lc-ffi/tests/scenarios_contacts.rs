//! Contacts, nodes, rooms, repeaters, requests and blocks (docs/testing.md §3, CT-1 to CT-11).

mod common;

use common::{Sim, contact_chat, find_contact, texts};
use lc_ffi::{
    ChatRef, DemoNetwork, LichenError, LichenEvent, MessageStatus, NodeKind, ServerLogin,
    TraceState,
};

fn node(sim: &Sim, name: &str) -> lc_ffi::NodeInfo {
    sim.core
        .nodes()
        .into_iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node {name}"))
}

/// CT-1: Contacts lists people only, sorted by name as a person reads it (accents and case
/// aside), each with its hops.
#[test]
fn ct_1_contacts_list() {
    let mut sim = Sim::new(81);
    sim.until("the radio's contacts", 10, |c| {
        ["Camille", "Julien", "Marc", "hub"]
            .iter()
            .all(|n| find_contact(c, n).is_some())
    });
    let all = sim.core.contacts();
    let people: Vec<&str> = all
        .iter()
        .filter(|c| c.person)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(people, ["Camille", "Julien", "Marc"]);
    for not_people in ["hub", "crest", "Refuge board"] {
        assert!(!find_contact(&sim.core, not_people).unwrap().person);
    }
    // Hops as the radio knows its paths: Camille and Marc behind the hub, Julien behind the hub
    // and the crest.
    assert_eq!(find_contact(&sim.core, "Camille").unwrap().hops, Some(1));
    assert_eq!(find_contact(&sim.core, "Marc").unwrap().hops, Some(1));
    assert_eq!(find_contact(&sim.core, "Julien").unwrap().hops, Some(2));
    // Sorted as people read names: an accent or a capital does not send a name to the end.
    let julien = find_contact(&sim.core, "Julien").unwrap();
    sim.core
        .rename_contact(julien.key.clone(), Some("Élodie".into()))
        .unwrap();
    let camille = find_contact(&sim.core, "Camille").unwrap();
    sim.core
        .rename_contact(camille.key.clone(), Some("camille".into()))
        .unwrap();
    let people: Vec<String> = sim
        .core
        .contacts()
        .into_iter()
        .filter(|c| c.person)
        .map(|c| c.name)
        .collect();
    assert_eq!(people, ["camille", "Élodie", "Marc"]);
}

/// CT-2: a contact renamed shows under that name everywhere (contacts, Messages, a MeshCore
/// message's notification, requests); a reset brings the node's name back.
#[test]
fn ct_2_rename_a_contact() {
    let mut sim = Sim::new(82);
    let camille = sim.camille();
    let marc = sim.contact("Marc");
    sim.core
        .rename_contact(camille.key.clone(), Some("  Cam  ".into()))
        .unwrap();
    sim.core
        .rename_contact(marc.key.clone(), Some("Marc (refuge)".into()))
        .unwrap();
    assert!(
        sim.drain()
            .iter()
            .any(|e| matches!(e, LichenEvent::ContactsUpdated))
    );
    let c = find_contact(&sim.core, "Cam").unwrap();
    assert_eq!((c.name.as_str(), c.node_name.as_str()), ("Cam", "Camille"));
    sim.until("Camille's conversation", 60, |core| {
        core.conversations()
            .iter()
            .any(|v| v.chat == contact_chat(&camille))
    });
    assert_eq!(
        sim.conversation(&contact_chat(&camille)).unwrap().name,
        "Cam"
    );
    // Marc's plain message at 45 s: its notification names him as renamed.
    let label = sim.until_event("Marc's message", 60, |e| match e {
        LichenEvent::PlainReceived { label, .. } => Some(label.clone()),
        _ => None,
    });
    assert_eq!(label.as_deref(), Some("Marc (refuge)"));
    let mesh = ChatRef::Mesh {
        key: marc.key.clone(),
    };
    assert_eq!(sim.conversation(&mesh).unwrap().name, "Marc (refuge)");
    // Group members show the nickname too.
    let group = sim.until_some("the group", 300, |c| c.groups().into_iter().next());
    assert!(group.members.iter().any(|m| m.name == "Cam"), "{group:?}");
    // Reset (blank or none): the node's name again.
    sim.core
        .rename_contact(camille.key.clone(), Some("   ".into()))
        .unwrap();
    assert_eq!(
        sim.conversation(&contact_chat(&camille)).unwrap().name,
        "Camille"
    );
    sim.core.rename_contact(marc.key.clone(), None).unwrap();
    assert_eq!(sim.conversation(&mesh).unwrap().name, "Marc");
    assert_eq!(
        sim.core.rename_contact(vec![0x42; 32], Some("x".into())),
        Err(LichenError::UnknownContact)
    );
}

/// CT-3: the nodes: repeaters and the room server with their kind and advertised position,
/// people, and Lucie heard but not added; each with when it was last heard.
#[test]
fn ct_3_nodes() {
    let mut sim = Sim::new(83);
    sim.until("Lucie's advert heard", 10, |c| {
        c.nodes().iter().any(|n| n.name == "Lucie")
    });
    let hub = node(&sim, "hub");
    assert_eq!(hub.kind, NodeKind::Repeater);
    assert!(hub.on_radio && !hub.lichen);
    assert!((hub.lat.unwrap() - 44.8931).abs() < 1e-6);
    assert!((hub.lon.unwrap() - 5.4925).abs() < 1e-6);
    assert_eq!(hub.hops, Some(0));
    let crest = node(&sim, "crest");
    assert_eq!(crest.kind, NodeKind::Repeater);
    assert!(crest.lat.is_some());
    let board = node(&sim, "Refuge board");
    assert_eq!(board.kind, NodeKind::RoomServer);
    assert!((board.lat.unwrap() - 44.8962).abs() < 1e-6);
    let lucie = node(&sim, "Lucie");
    assert_eq!(lucie.kind, NodeKind::Companion);
    assert!(!lucie.on_radio, "heard, not added");
    assert!(lucie.last_heard_s.is_some());
    for n in sim.core.nodes() {
        if n.on_radio {
            assert!(n.last_heard_s.is_some(), "{n:?}");
        }
    }
    // Camille becomes a lichen user once her identity is known; Marc stays MeshCore.
    sim.camille();
    assert!(node(&sim, "Camille").lichen);
    assert!(!node(&sim, "Marc").lichen);
    assert_eq!(node(&sim, "Camille").kind, NodeKind::Companion);
}

/// CT-4: the refuge board: logged in, its posts arrive with their authors, its row is in
/// Messages, a later post comes (200 s), and ours goes out.
#[test]
fn ct_4_room_log_in_read_and_post() {
    let mut sim = Sim::new(84);
    let board = sim.contact("Refuge board").key;
    let chat = ChatRef::Room { key: board.clone() };
    sim.core
        .server_login(board.clone(), "hello".into(), true, sim.now)
        .unwrap();
    assert_eq!(
        sim.core.server(board.clone()).unwrap().login,
        ServerLogin::Pending
    );
    sim.until("logged in", 30, |c| {
        c.server(board.clone()).unwrap().login == ServerLogin::ReadWrite
    });
    sim.until("the board's posts", 30, |c| {
        c.history(chat.clone(), None, 50).unwrap().len() >= 2
    });
    let h = sim.history(&chat);
    assert_eq!(h.len(), 2, "{:?}", texts(&h));
    assert_eq!(h[0].author_name.as_deref(), Some("Marc"));
    assert_eq!(h[1].author_name.as_deref(), Some("Camille"));
    assert!(h[0].at_ms < h[1].at_ms, "oldest first");
    let row = sim.conversation(&chat).expect("its row in Messages");
    assert_eq!(row.name, "Refuge board");
    assert_eq!(row.unread, 2);
    // Marc's post at 200 s.
    sim.until("Marc's new post", 240, |c| {
        texts(&c.history(chat.clone(), None, 50).unwrap())
            .iter()
            .any(|t| t.starts_with("Wind picking up"))
    });
    // Ours.
    let m = sim
        .core
        .send_plain(
            chat.clone(),
            "Two of us tonight.".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.until_status(&chat, m.id, 60, |s| *s == MessageStatus::Delivered);
    let h = sim.history(&chat);
    assert_eq!(h.last().unwrap().id, m.id);
    assert_eq!(h.iter().filter(|x| x.id == m.id).count(), 1);
}

/// CT-5: a wrong password gets no answer: the login says so, and nothing of the room comes.
#[test]
fn ct_5_room_wrong_password() {
    let mut sim = Sim::new(85);
    let board = sim.contact("Refuge board").key;
    sim.core
        .server_login(board.clone(), "wrong".into(), true, sim.now)
        .unwrap();
    sim.until("the login given up", 60, |c| {
        c.server(board.clone()).unwrap().login == ServerLogin::NoAnswer
    });
    assert!(sim.core.server(board.clone()).unwrap().no_answer);
    sim.run_for(30_000);
    let chat = ChatRef::Room { key: board.clone() };
    assert!(sim.history(&chat).is_empty());
    assert!(sim.conversation(&chat).is_none());
    // Too long a password is refused at once.
    assert!(
        sim.core
            .server_login(board, "x".repeat(16), false, sim.now)
            .is_err()
    );
}

/// CT-6: the hub's administration: admin login, its status, a command answered in the console.
#[test]
fn ct_6_repeater_admin_status_and_cli() {
    let mut sim = Sim::new(86);
    let hub = sim.contact("hub").key;
    sim.core
        .server_login(hub.clone(), "password".into(), true, sim.now)
        .unwrap();
    sim.until("admin", 30, |c| {
        c.server(hub.clone()).unwrap().login == ServerLogin::Admin
    });
    assert!(!sim.core.server(hub.clone()).unwrap().remembered);
    sim.core.request_server_stats(hub.clone(), sim.now).unwrap();
    sim.until("the hub's status", 30, |c| {
        c.server(hub.clone()).unwrap().stats.is_some()
    });
    let stats = sim.core.server(hub.clone()).unwrap().stats.unwrap();
    assert!(stats.uptime_s > 0 && stats.battery_mv > 0);
    sim.core
        .send_server_command(hub.clone(), "ver".into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until("the console's answer", 30, |c| {
        c.server(hub.clone()).unwrap().console.len() >= 2
    });
    let console = sim.core.server(hub.clone()).unwrap().console;
    assert!(console[0].outgoing && console[0].text == "ver");
    assert!(!console[1].outgoing && console[1].text.starts_with("v1.18"));
    sim.core.request_neighbours(hub.clone(), sim.now).unwrap();
    sim.until("the hub's neighbours", 30, |c| {
        c.server(hub.clone()).unwrap().neighbours.is_some()
    });
    // A guest's password: logged in as a guest, no admin console.
    let crest = sim.contact("crest").key;
    sim.core
        .server_login(crest.clone(), "hello".into(), false, sim.now)
        .unwrap();
    sim.until("guest on the crest", 30, |c| {
        c.server(crest.clone()).unwrap().login == ServerLogin::Guest
    });
}

/// CT-7: a trace to Julien, two repeaters away: every hop with its SNR, out and back.
#[test]
fn ct_7_trace_to_a_node() {
    let mut sim = Sim::new(87);
    let julien = sim.contact("Julien").key;
    let path = sim.core.node_path(julien.clone()).unwrap().unwrap();
    let names: Vec<String> = path.iter().flat_map(|h| h.names.clone()).collect();
    assert_eq!(names, ["hub", "crest"]);
    sim.core.start_trace(julien.clone(), sim.now).unwrap();
    sim.until("the trace back", 60, |c| {
        c.trace(julien.clone())
            .is_some_and(|t| t.state != TraceState::Sending)
    });
    let t = sim.core.trace(julien.clone()).unwrap();
    assert_eq!(t.state, TraceState::Done);
    assert_eq!(t.legs.len(), 4, "{t:?}");
    assert!(t.legs.iter().all(|l| l.snr_db.is_some()), "{t:?}");
    assert!(t.rtt_ms.is_some_and(|r| r > 0));
    assert!(t.airtime_ms > 0);
    assert!(sim.events.iter().any(|(_, e)| *e
        == LichenEvent::NodeUpdated {
            node: julien.clone()
        }));
    // A node the radio does not know: nothing to trace.
    assert!(sim.core.start_trace(vec![0x42; 32], sim.now).is_err());
    assert_eq!(sim.core.node_path(vec![0x42; 32]).unwrap(), None);
}

/// The demo with requests on (`-requestDemo YES`): Camille's first message becomes a request.
fn with_requests(seed: u8) -> Sim {
    Sim::on(seed, DemoNetwork::new(), true)
}

/// CT-8: Camille's first message arrives as a request, not in a conversation; accepted, she
/// becomes the contact's identity and her messages move into the chat that opens.
#[test]
fn ct_8_introduction_request_accept() {
    let mut sim = with_requests(88);
    let (node, identity) = sim.until_event("the request", 90, |e| match e {
        LichenEvent::IdentityRequested { node, identity } => Some((node.clone(), identity.clone())),
        _ => None,
    });
    sim.until("her hello in the request", 30, |c| {
        c.requests().first().is_some_and(|r| !r.messages.is_empty())
    });
    let r = sim.core.requests();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].node, node);
    assert_eq!(r[0].identity, identity);
    assert_eq!(r[0].name, "Camille");
    assert!(matches!(&r[0].messages[0].message,
        lc_ffi::Message::Text { text, .. } if text.starts_with("Hi! I'm at the refuge")));
    // Nothing of hers in a conversation, and her contact has no identity yet.
    let chat = ChatRef::Contact { key: node.clone() };
    assert!(sim.history(&chat).is_empty());
    assert!(sim.conversation(&chat).is_none());
    assert_eq!(find_contact(&sim.core, "Camille").unwrap().identity, None);
    // Her group meanwhile: not joined (she does not know us yet).
    sim.run_for(30_000);
    assert!(sim.core.groups().is_empty());
    assert!(sim.core.group_invitations().is_empty());

    assert_eq!(sim.core.accept_request(identity.clone()).unwrap(), node);
    let after = sim.drain();
    assert!(after.contains(&LichenEvent::RequestsChanged));
    assert!(after.contains(&LichenEvent::ContactsUpdated));
    assert!(sim.core.requests().is_empty());
    let c = find_contact(&sim.core, "Camille").unwrap();
    assert_eq!(c.identity, Some(identity.clone()));
    assert!(!c.verified);
    let h = sim.history(&chat);
    assert!(
        texts(&h)
            .iter()
            .any(|t| t.starts_with("Hi! I'm at the refuge")),
        "{h:?}"
    );
    assert!(sim.conversation(&chat).is_some());
    // We can write to her, and she answers in the conversation.
    let m = sim
        .core
        .send_text(node.clone(), "Hello".into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&chat, m.id, 120, |s| *s == MessageStatus::Delivered);
}

/// CT-9: a request deleted is forgotten (her next message is a new request); a request
/// blocked is listed with the blocked, and nothing more of it comes.
#[test]
fn ct_9_request_block_and_delete() {
    let mut sim = with_requests(89);
    let identity = sim.until_event("the request", 90, |e| match e {
        LichenEvent::IdentityRequested { identity, .. } => Some(identity.clone()),
        _ => None,
    });
    // The user deletes it once its hello shows.
    sim.until("her hello in the request", 30, |c| {
        c.requests().first().is_some_and(|r| !r.messages.is_empty())
    });
    sim.core.delete_request(identity.clone()).unwrap();
    assert!(sim.drain().contains(&LichenEvent::RequestsChanged));
    assert!(sim.core.requests().is_empty());
    assert!(sim.core.blocked().is_empty());
    // Her photo at 150 s: a new request, without the deleted hello.
    sim.until("her next message, a new request", 200, |c| {
        c.requests().first().is_some_and(|r| {
            r.messages.iter().any(|l| {
                matches!(&l.message, lc_ffi::Message::Text { text, .. }
                    if text.starts_with("The view from the refuge"))
            })
        })
    });
    let r = &sim.core.requests()[0];
    assert!(
        !r.messages.iter().any(|l| matches!(&l.message,
            lc_ffi::Message::Text { text, .. } if text.starts_with("Hi! I'm at the refuge"))),
        "the deleted hello came back: {r:?}"
    );
    let camille = r.node.clone();
    sim.core.block_request(identity.clone()).unwrap();
    assert!(sim.core.requests().is_empty());
    let blocked = sim.core.blocked();
    assert_eq!(blocked.len(), 1);
    assert!(blocked[0].request);
    assert_eq!(blocked[0].name, "Camille");
    // The radio is someone else's: the contact itself is not blocked.
    assert!(!sim.core.is_blocked(camille.clone()));
    // Nothing more of her: no request, no conversation.
    sim.run_for(120_000);
    assert!(sim.core.requests().is_empty());
    assert!(
        sim.history(&ChatRef::Contact { key: camille }).is_empty(),
        "a blocked request's messages reached the chat"
    );
    // Unblocked: listed no more.
    sim.core.unblock(blocked[0].node.clone()).unwrap();
    assert!(sim.core.blocked().is_empty());
}

/// CT-10: Camille blocked: her messages stop, writing to her is refused, Settings › Blocked
/// lists her; unblocked, what she sends comes again.
#[test]
fn ct_10_block_and_unblock_a_contact() {
    let mut sim = Sim::new(90);
    let camille = sim.contact("Camille");
    let chat = contact_chat(&camille);
    sim.core.block(camille.key.clone(), sim.now).unwrap();
    assert!(sim.core.is_blocked(camille.key.clone()));
    let b = sim.core.blocked();
    assert_eq!(b.len(), 1);
    assert_eq!((b[0].name.as_str(), b[0].request), ("Camille", false));
    assert_eq!(
        sim.core
            .send_text(camille.key.clone(), "hi".into(), sim.unix_s(), sim.now)
            .err(),
        Some(LichenError::Blocked)
    );
    // Her hello at 30 s does not come.
    sim.run_to(100_000);
    assert!(
        sim.history(&chat).is_empty(),
        "{:?}",
        texts(&sim.history(&chat))
    );
    assert!(sim.conversation(&chat).is_none());
    sim.core.unblock(camille.key.clone()).unwrap();
    assert!(!sim.core.is_blocked(camille.key.clone()));
    assert!(sim.core.blocked().is_empty());
    // Her photo's caption at 150 s comes.
    sim.until("her next message", 240, |c| {
        texts(&c.history(chat.clone(), None, 50).unwrap())
            .iter()
            .any(|t| t.starts_with("The view from the refuge"))
    });
    // Her hello, sent while she was blocked, may come now: her phone was still trying to
    // deliver it (a text waits 24 h, §6.1). ADR-0034 calls it lost: a question for the
    // spec (QA bug B-core-4), so neither is checked here.
    let m = sim
        .core
        .send_text(camille.key.clone(), "Back".into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&chat, m.id, 120, |s| *s == MessageStatus::Delivered);
}

/// CT-11: Marc, whom the radio files as a repeater (`-marcRepeater YES`): not among the people,
/// yet his MeshCore chat stays in Messages under his name, and he can be answered.
#[test]
fn ct_11_a_person_the_radio_files_as_a_repeater() {
    let demo = DemoNetwork::new();
    demo.marc_as_repeater();
    let mut sim = Sim::on(91, demo, false);
    let marc = sim.contact("Marc");
    assert!(!marc.person);
    let chat = ChatRef::Mesh {
        key: marc.key.clone(),
    };
    sim.until("Marc's message", 90, |c| {
        c.conversations().iter().any(|v| v.chat == chat)
    });
    let row = sim.conversation(&chat).unwrap();
    assert_eq!(row.name, "Marc");
    assert_eq!(row.unread, 1);
    // The user answers once the hub is quiet: in the demo's first minutes, the group's setup
    // and Camille's photo keep it busy and acknowledgements collide there.
    sim.run_to(330_000);
    let m = sim
        .core
        .send_plain(
            chat.clone(),
            "Yes, on lichen!".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let m = sim.until_status(&chat, m.id, 90, |s| {
        matches!(s, MessageStatus::Delivered | MessageStatus::Failed { .. })
    });
    if m.status != MessageStatus::Delivered {
        // Retried, as the app offers.
        sim.core.retry_message(m.id, sim.now).unwrap();
        sim.until_status(&chat, m.id, 90, |s| *s == MessageStatus::Delivered);
    }
    sim.until("his answer", 60, |c| {
        texts(&c.history(chat.clone(), None, 50).unwrap())
            .iter()
            .any(|t| t.starts_with("It works!"))
    });
    assert_eq!(sim.conversation(&chat).unwrap().name, "Marc");
}
