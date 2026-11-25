//! Contacts, nodes, rooms, repeaters, requests and blocks (docs/testing.md §3, CT-1 to CT-11).

mod common;

use common::{Sim, contact_chat, find_contact, texts};
use lc_ffi::{
    ChatRef, DemoNetwork, LichenError, LichenEvent, MessageStatus, NodeKind, ReadingsState,
    ServerLogin, SignalRating, TraceState,
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
    // Heard just now, by the clock the app set on the radio (B-and-5: not 24 days ago).
    for n in sim.core.nodes() {
        let heard = n.last_heard_s.unwrap_or_else(|| panic!("{n:?}"));
        assert!(
            heard.abs_diff(u64::from(sim.unix_s())) < 60,
            "{} heard at {heard}, now {}",
            n.name,
            sim.unix_s()
        );
    }
    // Camille becomes a lichen user once her identity is known; Marc stays MeshCore.
    sim.camille();
    assert!(node(&sim, "Camille").lichen);
    assert!(!node(&sim, "Marc").lichen);
    assert_eq!(node(&sim, "Camille").kind, NodeKind::Companion);
}

/// CT-3: each node's signal (protocol §16.1, ADR-0049): the nodes the radio hears directly
/// carry their last reading, rated against the preset: the hub Good, Lucie Fair, the board Bad.
/// Crest, behind the hub, has none of its own. Each reading is kept in the history, which
/// grows, and the last one shows again after a restart.
#[test]
fn ct_3_node_signal() {
    let mut sim = Sim::persistent(93, "ct3-signal");
    sim.until("the hub, Lucie and the board heard", 30, |c| {
        let nodes = c.nodes();
        ["hub", "Lucie", "Refuge board"]
            .iter()
            .all(|n| nodes.iter().any(|x| x.name == *n && x.snr_db.is_some()))
    });
    let hub = node(&sim, "hub");
    assert_eq!(hub.signal, SignalRating::Good, "{hub:?}");
    assert!(hub.rssi_dbm.is_some(), "{hub:?}");
    let at = hub.signal_at_s.unwrap();
    assert!(at.abs_diff(u64::from(sim.unix_s())) < 60, "{at}");
    let lucie = node(&sim, "Lucie");
    assert_eq!(lucie.signal, SignalRating::Fair, "{lucie:?}");
    let board = node(&sim, "Refuge board");
    assert_eq!(board.signal, SignalRating::Bad, "{board:?}");
    // Through the hub only: no signal of its own.
    let crest = node(&sim, "crest");
    assert_eq!((crest.snr_db, crest.signal), (None, SignalRating::None));
    assert_eq!((crest.rssi_dbm, crest.signal_at_s), (None, None));
    // Its page hears of each reading.
    assert!(sim.events.iter().any(|(_, e)| *e
        == LichenEvent::NodeUpdated {
            node: hub.key.clone()
        }));

    // The history grows with each advert heard.
    let first = sim.core.node_signal_history(hub.key.clone());
    assert_eq!(first.last().map(|r| r.snr_db), hub.snr_db);
    sim.until("another reading of the hub", 30, |c| {
        c.node_signal_history(hub.key.clone()).len() > first.len()
    });
    let h = sim.core.node_signal_history(hub.key.clone());
    assert!(h.windows(2).all(|w| w[0].at_s <= w[1].at_s), "oldest first");
    assert!(h.iter().all(|r| r.signal == SignalRating::Good), "{h:?}");
    assert!(sim.core.node_signal_history(crest.key.clone()).is_empty());
    assert!(sim.core.node_signal_history(vec![1, 2]).is_empty());

    // Restarted, the app shows the last reading before the radio hears the hub again.
    sim.restart(1_000);
    sim.until("the radio's nodes again", 10, |c| {
        c.nodes().iter().any(|n| n.name == "hub")
    });
    let again = node(&sim, "hub");
    let h = sim.core.node_signal_history(hub.key.clone());
    assert!(h.len() >= 2, "{h:?}");
    assert_eq!(again.signal_at_s, h.last().map(|r| r.at_s), "{again:?}");
    assert_eq!(again.signal, SignalRating::Good);
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

/// CT-7: the route to crest (protocol §16.3, ADR-0049): our radio, the hub, crest, each with
/// the position it advertises; the hub's leg measured by its adverts, crest's by a trace once
/// asked for, never made up before. Julien, a phone beyond crest: his own leg is never measured
/// (a trace stops at the last repeater).
#[test]
fn ct_7_route_to_a_node() {
    let mut sim = Sim::new(94);
    sim.until("the hub heard", 30, |c| {
        c.nodes()
            .iter()
            .any(|n| n.name == "hub" && n.snr_db.is_some())
    });
    let hub = node(&sim, "hub");
    let crest = node(&sim, "crest").key;
    let r = sim.core.node_route(crest.clone()).unwrap();
    let names: Vec<String> = r.stops.iter().map(|s| s.names.join("/")).collect();
    assert_eq!(names, ["ThinkNode M5", "hub", "crest"]);
    assert_eq!(r.stops[1].key.as_ref(), Some(&hub.key));
    assert_eq!(r.stops[1].hash, Some(format!("{:02x}", hub.key[0])));
    assert_eq!(
        (r.stops[0].hash.clone(), r.stops[2].hash.clone()),
        (None, None)
    );
    assert_eq!(r.stops[2].key.as_ref(), Some(&crest));
    assert_eq!(r.stops[0].lat, None, "our radio: where the phone is");
    assert_eq!(r.stops[1].lat, hub.lat);
    assert!(r.stops[2].lat.is_some() && r.stops[2].lon.is_some());
    assert_eq!(r.legs.len(), 2);
    assert_eq!(r.legs[0].snr_db, hub.snr_db);
    assert_eq!(r.legs[0].signal, SignalRating::Good);
    assert!(!r.legs[0].traced && r.legs[0].rssi_dbm.is_some());
    assert_eq!(
        (r.legs[1].snr_db, r.legs[1].at_s),
        (None, None),
        "not measured yet"
    );
    assert_eq!(r.legs[1].signal, SignalRating::None);

    sim.core.start_trace(crest.clone(), sim.now).unwrap();
    sim.until("the trace back", 60, |c| {
        c.trace(crest.clone())
            .is_some_and(|t| t.state != TraceState::Sending)
    });
    assert_eq!(
        sim.core.trace(crest.clone()).unwrap().state,
        TraceState::Done
    );
    let r = sim.core.node_route(crest.clone()).unwrap();
    assert!(r.legs[1].traced, "{r:?}");
    assert_eq!(r.legs[1].snr_db, Some(-3.0), "the hub hearing crest");
    assert_eq!(r.legs[1].signal, SignalRating::Fair);
    assert!(r.legs[1].at_s.is_some());
    assert!(r.legs[0].snr_db.is_some());

    // The hub, heard directly: one leg.
    let r = sim.core.node_route(hub.key.clone()).unwrap();
    assert_eq!((r.stops.len(), r.legs.len()), (2, 1));
    // Julien: the leg from crest to his phone is never measured.
    let julien = sim.contact("Julien").key;
    sim.core.start_trace(julien.clone(), sim.now).unwrap();
    sim.until("the trace to Julien back", 60, |c| {
        c.trace(julien.clone())
            .is_some_and(|t| t.state != TraceState::Sending)
    });
    let r = sim.core.node_route(julien.clone()).unwrap();
    let names: Vec<String> = r.stops.iter().map(|s| s.names.join("/")).collect();
    assert_eq!(names, ["ThinkNode M5", "hub", "crest", "Julien"]);
    assert!(r.legs[1].snr_db.is_some());
    assert_eq!(r.legs[2].snr_db, None);
    // A node the radio does not know.
    assert!(sim.core.node_route(vec![0x42; 32]).is_none());
    assert!(sim.core.node_route(vec![1]).is_none());
}

/// CT-14: a node's readings (protocol §16.7, ADR-0050): crest, which lists us once we log in
/// as its guest, answers 3.92 V, about 69 % and 4 °C, and again when asked again; one request
/// at a time. Marc, with the stock app, never answers. Lucie, heard but not on the radio,
/// cannot be asked.
#[test]
fn ct_14_a_node_s_readings() {
    let mut sim = Sim::new(95);
    let crest = sim.contact("crest").key;
    assert_eq!(
        sim.core.node_readings_state(crest.clone()),
        ReadingsState::Idle
    );
    assert!(sim.core.node_readings(crest.clone()).is_empty());
    assert_eq!(node(&sim, "crest").readings_at_s, None);
    sim.core
        .server_login(crest.clone(), "hello".into(), false, sim.now)
        .unwrap();
    sim.until("guest on the crest", 30, |c| {
        c.server(crest.clone()).unwrap().login == ServerLogin::Guest
    });
    sim.core
        .request_node_readings(crest.clone(), sim.now)
        .unwrap();
    assert_eq!(
        sim.core.node_readings_state(crest.clone()),
        ReadingsState::Asking
    );
    assert!(matches!(
        sim.core.request_node_readings(crest.clone(), sim.now),
        Err(LichenError::TooSoon { .. })
    ));
    sim.until("crest's readings", 60, |c| {
        c.node_readings_state(crest.clone()) != ReadingsState::Asking
    });
    assert_eq!(
        sim.core.node_readings_state(crest.clone()),
        ReadingsState::Answered
    );
    let r = sim.core.node_readings(crest.clone());
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].battery_mv, Some(3_920));
    assert_eq!(r[0].battery_percent, Some(69));
    assert_eq!(r[0].temperature_c, Some(4.0));
    assert!(
        r[0].values.is_empty(),
        "a guest gets the base readings: {r:?}"
    );
    assert!(r[0].at_s.abs_diff(u64::from(sim.unix_s())) < 60);
    assert_eq!(node(&sim, "crest").readings_at_s, Some(r[0].at_s));
    assert!(sim.events.iter().any(|(_, e)| *e
        == LichenEvent::NodeUpdated {
            node: crest.clone()
        }));
    // Asked again: a second answer in its history.
    sim.core
        .request_node_readings(crest.clone(), sim.now)
        .unwrap();
    sim.until("crest's readings again", 60, |c| {
        c.node_readings(crest.clone()).len() == 2
    });

    // Marc never answers: no answer, nothing kept.
    let marc = sim.contact("Marc").key;
    sim.core
        .request_node_readings(marc.clone(), sim.now)
        .unwrap();
    sim.until("Marc's silence", 120, |c| {
        c.node_readings_state(marc.clone()) != ReadingsState::Asking
    });
    assert_eq!(
        sim.core.node_readings_state(marc.clone()),
        ReadingsState::NoAnswer
    );
    assert!(sim.core.node_readings(marc.clone()).is_empty());
    assert_eq!(node(&sim, "Marc").readings_at_s, None);

    // Lucie is not on the radio: refused at once.
    let lucie = node(&sim, "Lucie").key;
    assert!(matches!(
        sim.core.request_node_readings(lucie.clone(), sim.now),
        Err(LichenError::UnknownContact)
    ));
    assert_eq!(sim.core.node_readings_state(lucie), ReadingsState::Idle);
    assert!(sim.core.node_readings(vec![1, 2]).is_empty());
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
/// lists her; unblocked, what she sends comes again, and what she sent while blocked does not
/// (B-core-4).
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
    // Her photo's caption at 150 s comes: the hello her phone was still trying is
    // acknowledged and dropped, so nothing holds it back (D-11).
    sim.until("her next message", 240, |c| {
        texts(&c.history(chat.clone(), None, 50).unwrap())
            .iter()
            .any(|t| t.starts_with("The view from the refuge"))
    });
    // Her hello, dated inside the block, stays out even though her phone sends it again
    // (ADR-0034): give her retries time.
    sim.run_for(240_000);
    assert!(
        !texts(&sim.history(&chat))
            .iter()
            .any(|t| t.starts_with("Hi! I'm at the refuge")),
        "{:?}",
        texts(&sim.history(&chat))
    );
    let m = sim
        .core
        .send_text(camille.key.clone(), "Back".into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&chat, m.id, 120, |s| *s == MessageStatus::Delivered);
}

/// CT-10 (B-ios-8, B-ios-19): our text on the air when we block Camille still says delivered
/// once she has it; her answer, written while blocked, never shows, and what she says after
/// the unblock comes at once.
#[test]
fn ct_10_our_text_on_the_air_at_the_block() {
    let mut sim = Sim::new(92);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    // Once the hub is quiet.
    sim.run_to(330_000);
    let m = sim
        .core
        .send_text(
            camille.key.clone(),
            "Are you still at the refuge?".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.until("its first frame", 60, |c| {
        matches!(
            c.history(chat.clone(), None, 100)
                .unwrap()
                .into_iter()
                .find(|x| x.id == m.id)
                .map(|x| x.status),
            Some(MessageStatus::Sending { .. } | MessageStatus::Delivered)
        )
    });
    sim.core.block(camille.key.clone(), sim.now).unwrap();
    let ours = sim.until_status(&chat, m.id, 120, |s| {
        matches!(s, MessageStatus::Delivered | MessageStatus::Failed { .. })
    });
    assert_eq!(ours.status, MessageStatus::Delivered, "it reached her");
    sim.run_for(25_000);
    sim.core.unblock(camille.key.clone()).unwrap();
    let back = sim
        .core
        .send_text(
            camille.key.clone(),
            "Back again".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    // Her answer to it comes as fast as before the block: the one she wrote while blocked,
    // still on her phone, is acknowledged and dropped (D-11, from B-ios-19).
    sim.until_status(&chat, back.id, 60, |s| *s == MessageStatus::Delivered);
    sim.until("her answer to the second", 60, |c| {
        texts(&c.history(chat.clone(), None, 100).unwrap())
            .iter()
            .any(|t| t.starts_with("Copy: \"Back again\""))
    });
    sim.run_for(120_000);
    assert!(
        !texts(&sim.history(&chat))
            .iter()
            .any(|t| t.starts_with("Copy: \"Are you still")),
        "her answer written while blocked came: {:?}",
        texts(&sim.history(&chat))
    );
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

/// F-1: the stock MeshCore app's contact code: the radio's own (its name, a companion), which
/// a stock app scans to add it, read back; a repeater's; the documentation's example; and
/// what is refused (§15.3).
#[test]
fn f_1_meshcore_contact_code() {
    use lc_ffi::{LichenError, MeshCoreContact, meshcore_contact_code, read_meshcore_contact_code};
    let mut sim = Sim::new(93);
    sim.until("the radio's settings and nodes", 10, |c| {
        c.radio_settings().read && c.nodes().iter().any(|n| n.name == "hub")
    });
    let (name, key) = (sim.core.radio_settings().name, sim.core.node_key().unwrap());
    let code = meshcore_contact_code(name.clone(), key.clone(), NodeKind::Companion).unwrap();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        code,
        format!("meshcore://contact/add?name=ThinkNode%20M5&public_key={hex}&type=1")
    );
    assert_eq!(
        read_meshcore_contact_code(code).unwrap(),
        MeshCoreContact {
            name,
            key,
            kind: NodeKind::Companion
        }
    );
    let hub = node(&sim, "hub");
    let code = meshcore_contact_code(hub.name.clone(), hub.key.clone(), hub.kind).unwrap();
    assert!(code.ends_with("&type=2"), "{code}");
    assert_eq!(
        read_meshcore_contact_code(code).unwrap().kind,
        NodeKind::Repeater
    );
    // MeshCore's own example.
    let example = read_meshcore_contact_code(
        "meshcore://contact/add?name=Example+Contact&public_key=9cd8fcf22a47333b591d96a2b848b73f457b1bb1a3ea2453a885f9e5787765b1&type=1".into(),
    )
    .unwrap();
    assert_eq!(
        (example.name.as_str(), example.kind, example.key.len()),
        ("Example Contact", NodeKind::Companion, 32)
    );
    // Refused.
    for bad in [
        "meshcore://contact/add?name=x&public_key=9cd8&type=1",
        "meshcore://contact/add?name=x&public_key=9cd8fcf22a47333b591d96a2b848b73f457b1bb1a3ea2453a885f9e5787765b1&type=7",
        "meshcore://channel/add?name=Public&secret=8b3387e9c5cdea6ac9e5edbaa115cd72",
        "lichen:1:nope",
    ] {
        assert_eq!(
            read_meshcore_contact_code(bad.into()),
            Err(LichenError::BadCode),
            "{bad}"
        );
    }
    assert_eq!(
        meshcore_contact_code("x".into(), vec![1; 31], NodeKind::Companion),
        Err(LichenError::InvalidKey)
    );
    assert_eq!(
        meshcore_contact_code("x".into(), vec![1; 32], NodeKind::Unknown),
        Err(LichenError::InvalidSetting)
    );
}

/// A node's link as the stock app shares it (§15.3), signed with a test key: `meshcore://`
/// and the hex of its advert.
fn signed_link(seed: u8, kind: u8, name: &str, position: Option<(i32, i32)>) -> (String, Vec<u8>) {
    use lc_meshcore::ecdh::{expand_seed, public_key, sign};
    let prv = expand_seed(&[seed; 32]);
    let key = public_key(&prv);
    let ts = 1_793_000_000u32.to_le_bytes();
    let mut data = vec![kind | 0x80 | if position.is_some() { 0x10 } else { 0 }];
    if let Some((lat, lon)) = position {
        data.extend_from_slice(&lat.to_le_bytes());
        data.extend_from_slice(&lon.to_le_bytes());
    }
    data.extend_from_slice(name.as_bytes());
    let sig = sign(&prv, &[&key, &ts, &data]).unwrap();
    let mut raw = vec![0x11, 0];
    raw.extend_from_slice(&key);
    raw.extend_from_slice(&ts);
    raw.extend_from_slice(&sig);
    raw.extend_from_slice(&data);
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    (format!("meshcore://{hex}"), key.to_vec())
}

/// CT-12: a MeshCore contact from a link (§15.3, §16.6; user request F-2). A person's signed
/// link is read, added to the radio and comes among the people; added again, it is already
/// there. A repeater's, with a position, comes among the nodes. A changed link is refused for
/// its signature and nothing is sent; a contact code adds a node too, unsigned.
#[test]
fn ct_12_add_a_meshcore_contact_from_a_link() {
    use lc_ffi::{MeshCoreAdd, MeshCoreLink, read_meshcore_link};
    let mut sim = Sim::new(94);
    sim.until("the radio's contacts", 10, |c| {
        find_contact(c, "Camille").is_some() && c.radio_settings().read
    });
    let (text, key) = signed_link(50, 1, "Hut radio", None);
    assert_eq!(
        read_meshcore_link(text.clone()).unwrap(),
        MeshCoreLink {
            name: "Hut radio".into(),
            key: key.clone(),
            kind: NodeKind::Companion,
            lat: None,
            lon: None,
            signed: true,
            hops: 0,
            path: Vec::new(),
        }
    );
    assert_eq!(
        sim.core.add_meshcore_contact(text.clone()),
        Ok(MeshCoreAdd::Sent)
    );
    sim.until("Hut radio among the people", 10, |c| {
        find_contact(c, "Hut radio").is_some_and(|h| h.person)
    });
    assert_eq!(find_contact(&sim.core, "Hut radio").unwrap().key, key);
    assert_eq!(
        sim.core.add_meshcore_contact(text),
        Ok(MeshCoreAdd::AlreadyOnTheRadio)
    );

    // A repeater's, with the position it advertises: a node, not a person.
    let (text, key) = signed_link(51, 2, "ridge", Some((12_345_678, -98_765_432)));
    let link = read_meshcore_link(text.clone()).unwrap();
    assert_eq!(link.kind, NodeKind::Repeater);
    assert!((link.lat.unwrap() - 12.345_678).abs() < 1e-9);
    assert!((link.lon.unwrap() + 98.765_432).abs() < 1e-9);
    assert_eq!(sim.core.add_meshcore_contact(text), Ok(MeshCoreAdd::Sent));
    sim.until("ridge among the nodes", 10, |c| {
        c.nodes().iter().any(|n| n.key == key && n.on_radio)
    });
    let ridge = node(&sim, "ridge");
    assert_eq!(ridge.kind, NodeKind::Repeater);
    assert!((ridge.lat.unwrap() - 12.345_678).abs() < 1e-9);
    assert!(!find_contact(&sim.core, "ridge").unwrap().person);

    // Changed after it was signed: refused, nothing sent.
    let (text, key) = signed_link(52, 1, "Lake hut", None);
    let changed = format!(
        "{}{}",
        &text[..text.len() - 1],
        if text.ends_with('0') { '1' } else { '0' }
    );
    assert_eq!(
        read_meshcore_link(changed.clone()),
        Err(LichenError::BadSignature)
    );
    assert_eq!(
        sim.core.add_meshcore_contact(changed),
        Err(LichenError::BadSignature)
    );
    assert_eq!(
        read_meshcore_link("meshcore://11zz".into()),
        Err(LichenError::BadCode)
    );
    sim.run_for(5_000);
    assert!(!sim.core.nodes().iter().any(|n| n.key == key));

    // A contact code: unsigned, added all the same.
    let hex: String = [0x43u8; 32].iter().map(|b| format!("{b:02x}")).collect();
    let code = format!("meshcore://contact/add?name=Lake%20hut&public_key={hex}&type=1");
    assert!(!read_meshcore_link(code.clone()).unwrap().signed);
    assert_eq!(sim.core.add_meshcore_contact(code), Ok(MeshCoreAdd::Sent));
    sim.until("Lake hut among the people", 10, |c| {
        find_contact(c, "Lake hut").is_some_and(|h| h.person)
    });
    // The radio took every one: nothing refused.
    assert!(sim.core.take_radio_outcomes().is_empty());
}
