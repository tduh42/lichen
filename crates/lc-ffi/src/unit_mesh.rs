//! Mesh tools as the apps run them, on the demo network (protocol §15.8, §16; ADR-0041 to
//! ADR-0044): nodes and the map, paths and traces, saved radios, channel QR codes, the
//! refuge's room server and the hub's administration.

use crate::{
    ChatRef, DemoNetwork, LichenCore, LichenEvent, LinkKind, NodeKind, ServerLogin, TraceState,
};

fn run(demo: &DemoNetwork, core: &LichenCore, from: u64, to: u64) -> Vec<LichenEvent> {
    let mut events = Vec::new();
    let mut now = from;
    while now < to {
        for f in demo.read(now) {
            core.on_companion_frame(f, now);
        }
        core.tick(now);
        while let Some(f) = core.poll_companion(now) {
            demo.write(f, now);
        }
        while let Some(e) = core.poll_event() {
            events.push(e);
        }
        now += 100;
    }
    events
}

fn connected() -> (DemoNetwork, LichenCore) {
    let demo = std::sync::Arc::into_inner(DemoNetwork::new()).unwrap();
    let core = std::sync::Arc::into_inner(
        LichenCore::new("t".into(), vec![7; 32], vec![131; 32], None).unwrap(),
    )
    .unwrap();
    core.connect(0);
    run(&demo, &core, 0, 5_000);
    (demo, core)
}

fn node(core: &LichenCore, name: &str) -> Vec<u8> {
    core.nodes()
        .into_iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node {name}"))
        .key
}

#[test]
fn nodes_and_the_map() {
    let (demo, core) = connected();
    let nodes = core.nodes();
    let hub = nodes.iter().find(|n| n.name == "hub").unwrap();
    assert_eq!(hub.kind, NodeKind::Repeater);
    assert!(hub.on_radio);
    assert_eq!(hub.hops, Some(0));
    assert!((hub.lat.unwrap() - 44.8931).abs() < 1e-6);
    assert!(hub.last_heard_s.is_some());
    let board = nodes.iter().find(|n| n.name == "Refuge board").unwrap();
    assert_eq!(board.kind, NodeKind::RoomServer);
    let camille = nodes.iter().find(|n| n.name == "Camille").unwrap();
    assert_eq!(camille.kind, NodeKind::Companion);
    assert_eq!(camille.lat, None, "no position in her adverts");
    // People are still the contacts; the repeaters and the room are nodes only.
    assert!(core.contacts().iter().all(|c| c.name != "hub" || !c.person));
    // Lucie's advert, heard at 20 s, is not added by the radio.
    let events = run(&demo, &core, 5_000, 25_000);
    assert!(events.contains(&LichenEvent::ContactsUpdated));
    let lucie = core
        .nodes()
        .into_iter()
        .find(|n| n.name == "Lucie")
        .unwrap();
    assert!(!lucie.on_radio);
}

#[test]
fn a_path_and_its_trace() {
    let (demo, core) = connected();
    let camille = node(&core, "Camille");
    let path = core.node_path(camille.clone()).unwrap().unwrap();
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].names, vec!["hub".to_owned()]);
    core.start_trace(camille.clone(), 5_000).unwrap();
    assert_eq!(
        core.trace(camille.clone()).unwrap().state,
        TraceState::Sending
    );
    let events = run(&demo, &core, 5_000, 30_000);
    assert!(events.contains(&LichenEvent::NodeUpdated {
        node: camille.clone()
    }));
    let t = core.trace(camille.clone()).unwrap();
    assert_eq!(t.state, TraceState::Done);
    // Out to the hub, and back from it.
    assert_eq!(t.legs.len(), 2);
    assert_eq!(t.legs[0].from, None);
    assert_eq!(t.legs[0].to.as_ref().unwrap().names, vec!["hub".to_owned()]);
    assert_eq!(t.legs[1].to, None);
    assert!(t.legs.iter().all(|l| l.snr_db.is_some()));
    // The hub itself is heard directly: nothing to go through but itself.
    let hub = node(&core, "hub");
    assert_eq!(core.node_path(hub.clone()).unwrap(), Some(vec![]));
    assert_eq!(
        core.start_trace(vec![1; 3], 0),
        Err(crate::LichenError::InvalidKey)
    );
}

#[test]
fn saved_radios() {
    let dir = std::env::temp_dir().join(format!("lichen-radios-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let core = LichenCore::new(
        "t".into(),
        vec![7; 32],
        vec![132; 32],
        Some(dir.to_string_lossy().into_owned()),
    )
    .unwrap();
    let demo = DemoNetwork::new();
    core.connect(0);
    run(&demo, &core, 0, 5_000);
    core.remember_radio("BLE-1".into(), LinkKind::Bluetooth, "M5".into())
        .unwrap();
    let radios = core.saved_radios();
    assert_eq!(radios.len(), 1);
    assert_eq!(radios[0].name, "ThinkNode M5", "the radio's own name");
    assert!(radios[0].current);
    assert!(radios[0].node.is_some());
    core.disconnect(6_000);
    assert!(!core.saved_radios()[0].current);
    core.remember_radio("10.0.2.2:5000".into(), LinkKind::Tcp, "bridge".into())
        .unwrap();
    assert_eq!(core.saved_radios().len(), 2);
    core.forget_radio("BLE-1".into()).unwrap();
    let radios = core.saved_radios();
    assert_eq!(radios.len(), 1);
    assert_eq!(radios[0].link, LinkKind::Tcp);
    // The battery: read at connection, with the warning threshold.
    let s = core.radio_settings();
    assert!(s.battery_percent.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn channels_by_qr_code() {
    let (_demo, core) = connected();
    let public = core
        .channels()
        .into_iter()
        .find(|c| c.name == "Public")
        .unwrap();
    let qr = core.channel_qr(public.id.clone()).unwrap();
    assert_eq!(
        qr,
        "meshcore://channel/add?name=Public&secret=8b3387e9c5cdea6ac9e5edbaa115cd72"
    );
    let code = core.parse_channel_qr(qr).unwrap();
    assert!(code.joined);
    assert_eq!(code.kind, crate::ChannelKind::PublicChannel);
    let code = core
        .parse_channel_qr(
            "meshcore://channel/add?name=Hut%20team&secret=00112233445566778899aabbccddeeff".into(),
        )
        .unwrap();
    assert!(!code.joined);
    assert_eq!(code.kind, crate::ChannelKind::PrivateChannel);
    let chat = core
        .join_private_channel(code.name.clone(), code.secret_hex.clone())
        .unwrap();
    assert!(matches!(chat, ChatRef::Channel { .. }));
    assert_eq!(
        core.parse_channel_qr("not a code".into()),
        Err(crate::LichenError::BadCode)
    );
}

#[test]
fn the_refuge_board() {
    let (demo, core) = connected();
    let board = node(&core, "Refuge board");
    let chat = ChatRef::Room { key: board.clone() };
    // A wrong password gets no answer.
    core.server_login(board.clone(), "nope".into(), false, 5_000)
        .unwrap();
    run(&demo, &core, 5_000, 40_000);
    assert_eq!(
        core.server(board.clone()).unwrap().login,
        ServerLogin::NoAnswer
    );
    // The guest password: in, and the posts so far come.
    core.server_login(board.clone(), "hello".into(), true, 40_000)
        .unwrap();
    let events = run(&demo, &core, 40_000, 70_000);
    let info = core.server(board.clone()).unwrap();
    assert_eq!(info.login, ServerLogin::ReadWrite);
    assert!(info.remembered);
    assert!(events.iter().any(|e| matches!(e,
        LichenEvent::PlainReceived { chat: c, label: Some(l), .. } if *c == chat && l == "Marc")));
    let history = core.history(chat.clone(), None, 50).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].author_name.as_deref(), Some("Marc"));
    // Our post, acknowledged by the room.
    let m = core
        .send_plain(
            chat.clone(),
            "Arriving at 6 pm, two of us.".into(),
            7,
            70_000,
        )
        .unwrap();
    run(&demo, &core, 70_000, 100_000);
    let mine = core
        .history(chat.clone(), None, 50)
        .unwrap()
        .into_iter()
        .find(|x| x.id == m.id)
        .unwrap();
    assert_eq!(mine.status, crate::MessageStatus::Delivered);
    // Logged in again at the next connection, without asking.
    core.disconnect(100_000);
    assert_eq!(core.server(board.clone()).unwrap().login, ServerLogin::Out);
    core.connect(101_000);
    run(&demo, &core, 101_000, 130_000);
    assert_eq!(
        core.server(board.clone()).unwrap().login,
        ServerLogin::ReadWrite
    );
    // Logged out: forgotten.
    core.server_logout(board.clone()).unwrap();
    assert!(!core.server(board).unwrap().remembered);
}

#[test]
fn the_hub_s_administration() {
    let (demo, core) = connected();
    let hub = node(&core, "hub");
    // Not logged in: a status request gets no answer.
    core.request_server_stats(hub.clone(), 5_000).unwrap();
    run(&demo, &core, 5_000, 30_000);
    let info = core.server(hub.clone()).unwrap();
    assert!(info.no_answer && !info.stats_pending);
    core.server_login(hub.clone(), "password".into(), true, 30_000)
        .unwrap();
    run(&demo, &core, 30_000, 45_000);
    let info = core.server(hub.clone()).unwrap();
    assert_eq!(info.login, ServerLogin::Admin);
    assert!(!info.remembered, "a repeater's password is never kept");
    core.request_server_stats(hub.clone(), 45_000).unwrap();
    core.request_neighbours(hub.clone(), 45_000).unwrap();
    core.send_server_command(hub.clone(), "ver".into(), 9, 45_000)
        .unwrap();
    run(&demo, &core, 45_000, 80_000);
    let info = core.server(hub.clone()).unwrap();
    let stats = info.stats.unwrap();
    assert!(stats.uptime_s > 0);
    assert!(stats.rx_air_time_s.is_some(), "a repeater's status");
    let neighbours = info.neighbours.unwrap();
    assert_eq!(neighbours.len(), usize::from(info.neighbours_total));
    assert!(
        neighbours
            .iter()
            .any(|n| n.name.as_deref() == Some("Camille"))
    );
    assert_eq!(info.console.len(), 2);
    assert!(info.console[0].outgoing);
    assert_eq!(info.console[1].text, "v1.18.0 (Build: 15 Oct 2026)");
    // Nothing of the console reaches the conversations.
    assert!(
        core.history(ChatRef::Mesh { key: hub }, None, 10)
            .unwrap()
            .is_empty()
    );
}
