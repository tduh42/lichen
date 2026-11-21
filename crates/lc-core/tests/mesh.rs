//! Mesh tools over the fake network (protocol.md §16, ADR-0041): the nodes the radio knows.

use lc_core::contact_kind;
use lc_core::fakenet::Net;
use lc_core::{Event, Node};
use lc_meshcore::airtime::LoRaParams;

fn radio() -> LoRaParams {
    LoRaParams::meshcore(8, 62_500, 5).unwrap()
}

/// `app` – `hub` – `crest` – `bob`, the app's radio knowing the three; `hub` advertises its
/// position. `lucie`, next to the app, is not in its contacts.
struct Mesh {
    net: Net,
    app: usize,
    hub: usize,
    crest: usize,
    bob: usize,
    lucie: usize,
}

fn mesh(seed: u64) -> Mesh {
    let mut net = Net::new(radio(), 0, seed);
    let app = net.add_companion(1, "app");
    let hub = net.add_repeater(10, "hub");
    let crest = net.add_repeater(11, "crest");
    let bob = net.add_companion(2, "Bob");
    let lucie = net.add_stock_companion(6, "Lucie");
    net.chain(&[app, hub, crest, bob]);
    net.link(app, lucie);
    net.nodes[hub].advert_position = Some((45_123_456, 5_654_321));
    for n in [hub, crest, bob] {
        net.know(app, n, true);
    }
    net.connect(app);
    net.run_until(net.now + 10_000, |_| false);
    Mesh {
        net,
        app,
        hub,
        crest,
        bob,
        lucie,
    }
}

fn nodes_of(m: &Mesh) -> Vec<Node> {
    m.net.with_core_ref(m.app, |c| c.nodes()).unwrap()
}

fn find(nodes: &[Node], key: [u8; 32]) -> &Node {
    nodes.iter().find(|n| n.key == key).unwrap()
}

#[test]
fn the_radio_s_contacts_are_its_nodes() {
    let m = mesh(1);
    let nodes = nodes_of(&m);
    assert_eq!(nodes.len(), 3);
    let hub = find(&nodes, m.net.key(m.hub));
    assert_eq!(hub.kind, contact_kind::REPEATER);
    assert_eq!(hub.position, Some((45_123_456, 5_654_321)));
    assert!(hub.on_radio);
    assert!(hub.last_heard >= 1_793_000_000);
    assert_eq!(hub.out_path.as_ref().map(|p| p.hops()), Some(0));
    // A node without a position in its adverts has none.
    let crest = find(&nodes, m.net.key(m.crest));
    assert_eq!(crest.position, None);
    assert_eq!(crest.out_path.as_ref().map(|p| p.hops()), Some(1));
    let bob = find(&nodes, m.net.key(m.bob));
    assert_eq!(bob.kind, contact_kind::CHAT);
    assert_eq!(bob.out_path.as_ref().map(|p| p.hops()), Some(2));
}

/// A node the radio hears without adding it (`NEW_ADVERT`) is listed, not on the radio; once
/// the radio adds it, it is one of its contacts.
#[test]
fn a_node_heard_but_not_added() {
    let mut m = mesh(2);
    m.net.take_events(m.app);
    let (app, lucie) = (m.app, m.lucie);
    m.net.hear_advert(app, lucie);
    m.net.run_until(m.net.now + 1_000, |_| false);
    assert!(m.net.take_events(app).contains(&Event::NodesUpdated));
    let nodes = nodes_of(&m);
    assert_eq!(nodes.len(), 4);
    let heard = find(&nodes, m.net.key(lucie));
    assert!(!heard.on_radio);
    assert_eq!(heard.name, "Lucie");
    assert_eq!(heard.kind, contact_kind::CHAT);

    // The radio adds her: one entry, now on the radio.
    m.net.know(app, lucie, true);
    m.net.connect(app);
    m.net.run_until(m.net.now + 10_000, |_| false);
    let nodes = nodes_of(&m);
    assert_eq!(nodes.len(), 4);
    assert!(find(&nodes, m.net.key(lucie)).on_radio);
}

/// Nothing is known of the nodes before the radio reports them, nor kept from another radio.
#[test]
fn nodes_follow_the_radio() {
    let mut m = mesh(3);
    assert_eq!(nodes_of(&m).len(), 3);
    // The phone moves to Bob's radio, which knows nobody.
    let (app, bob) = (m.app, m.bob);
    m.net.move_app(app, bob);
    m.net.connect(bob);
    m.net.run_until(m.net.now + 10_000, |_| false);
    assert!(m.net.with_core_ref(bob, |c| c.nodes()).unwrap().is_empty());
}

/// A contact's path names its repeaters, when the radio knows them (§16.2).
#[test]
fn a_path_names_its_repeaters() {
    let m = mesh(4);
    let (bob, hub, crest) = (m.net.key(m.bob), m.net.key(m.hub), m.net.key(m.crest));
    let hops = m
        .net
        .with_core_ref(m.app, |c| c.path_hops(&bob))
        .unwrap()
        .unwrap();
    assert_eq!(hops.len(), 2);
    assert_eq!(hops[0].hash, vec![hub[0]]);
    assert_eq!(hops[0].names, vec!["hub".to_owned()]);
    assert_eq!(hops[1].hash, vec![crest[0]]);
    assert_eq!(hops[1].names, vec!["crest".to_owned()]);
    // The hub is heard directly: an empty path.
    let direct = m.net.with_core_ref(m.app, |c| c.path_hops(&hub)).unwrap();
    assert_eq!(direct, Some(vec![]));
    // A hash no known repeater has: no name.
    let unknown = m
        .net
        .with_core_ref(m.app, |c| c.hop_names(&[bob[0]]))
        .unwrap();
    assert!(unknown.is_empty());
}

fn traced(m: &mut Mesh, node: [u8; 32]) -> lc_core::Trace {
    let app = m.app;
    m.net
        .with_core(app, |c, now| c.trace(&node, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    m.net.run_until(m.net.now + 60_000, |n| {
        ev.extend(n.take_events(app));
        n.with_core_ref(app, |c| c.trace_of(&node).map(|t| t.status))
            .flatten()
            .is_some_and(|s| s != lc_core::TraceStatus::Sending)
    });
    assert!(ev.contains(&Event::NodeTool { node }));
    m.net
        .with_core_ref(app, |c| c.trace_of(&node).cloned())
        .unwrap()
        .unwrap()
}

/// A trace goes along the path and back; each hop's SNR comes back (§16.3).
#[test]
fn a_trace_goes_there_and_back() {
    let mut m = mesh(5);
    let (bob, hub, crest) = (m.net.key(m.bob), m.net.key(m.hub), m.net.key(m.crest));
    let t = traced(&mut m, bob);
    assert_eq!(t.status, lc_core::TraceStatus::Done);
    let hashes: Vec<Vec<u8>> = t.hops.iter().map(|h| h.hash.clone()).collect();
    assert_eq!(hashes, [vec![hub[0]], vec![crest[0]], vec![hub[0]]]);
    assert_eq!(t.hops[1].names, vec!["crest".to_owned()]);
    assert_eq!(t.snrs, [40, 40, 40], "unplaced nodes hear at 10 dB");
    assert_eq!(t.final_snr4, Some(40));
    assert!(t.rtt_ms.is_some_and(|r| r > 0));
    assert!(t.airtime_ms > 0);

    // A repeater is traced to itself: out to the hub, and heard back.
    let t = traced(&mut m, hub);
    assert_eq!(t.status, lc_core::TraceStatus::Done);
    assert_eq!(t.hops.len(), 1);
    assert_eq!(t.snrs.len(), 1);
}

/// A hop that does not hear the trace: no answer once the companion's estimate is past.
#[test]
fn a_lost_trace_has_no_answer() {
    let mut m = mesh(6);
    let bob = m.net.key(m.bob);
    let (hub, crest) = (m.hub, m.crest);
    m.net.unlink(hub, crest);
    let t = traced(&mut m, bob);
    assert_eq!(t.status, lc_core::TraceStatus::NoAnswer);
    assert!(t.snrs.is_empty());
}

/// Nothing to trace without a repeater on the way; a second trace waits for the first.
#[test]
fn what_cannot_be_traced() {
    let mut m = mesh(7);
    let (app, lucie, bob) = (m.app, m.lucie, m.net.key(m.bob));
    m.net.hear_advert(app, lucie);
    m.net.run_until(m.net.now + 1_000, |_| false);
    let lucie = m.net.key(lucie);
    let r = m.net.with_core(app, |c, now| c.trace(&lucie, now)).unwrap();
    assert_eq!(
        r,
        Err(lc_core::Error::NoPath),
        "heard directly, not a repeater"
    );
    let r = m
        .net
        .with_core(app, |c, now| c.trace(&[9; 32], now))
        .unwrap();
    assert_eq!(r, Err(lc_core::Error::UnknownContact));
    let first = m.net.with_core(app, |c, now| c.trace(&bob, now)).unwrap();
    assert_eq!(first, Ok(()));
    let second = m.net.with_core(app, |c, now| c.trace(&bob, now)).unwrap();
    assert!(matches!(second, Err(lc_core::Error::TooSoon { .. })));
}

/// The stock app's channel QR codes (`testdata/meshcore/mesh.toml`, from MeshCore's
/// `docs/qr_codes.md`): read, and written back so that it reads them (§15.3).
#[test]
fn channel_qr_codes() {
    use lc_core::plain::{channel_qr, parse_channel_qr};
    let path = format!(
        "{}/../../testdata/meshcore/mesh.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let t: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let codes: Vec<&toml::Value> = t["qr"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v.get("channel_name").is_some())
        .collect();
    assert_eq!(codes.len(), 3);
    for v in codes {
        let (text, name) = (
            v["text"].as_str().unwrap(),
            v["channel_name"].as_str().unwrap(),
        );
        let secret: [u8; 16] = hex::decode(v["secret"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            parse_channel_qr(text),
            Some((name.to_owned(), secret)),
            "{text}"
        );
        let ours = channel_qr(name, &secret);
        assert_eq!(parse_channel_qr(&ours), Some((name.to_owned(), secret)));
        // Written as the stock app's documentation shows it, percent-encoded.
        if !text.contains('+') {
            assert_eq!(ours, text);
        }
    }
    for bad in [
        "meshcore://contact/add?name=x&public_key=00",
        "meshcore://channel/add?name=x",
        "meshcore://channel/add?name=x&secret=00112233",
        "meshcore://channel/add?name=x&secret=00000000000000000000000000000000",
        "meshcore://channel/add?name=&secret=00112233445566778899aabbccddeeff",
        "meshcore://channel/add?name=%zz&secret=00112233445566778899aabbccddeeff",
        "meshcore://channel/add?name=%C3&secret=00112233445566778899aabbccddeeff",
        "https://example.org/?name=x&secret=00112233445566778899aabbccddeeff",
    ] {
        assert_eq!(parse_channel_qr(bad), None, "{bad}");
    }
    let long = "a".repeat(32);
    assert_eq!(
        parse_channel_qr(&format!(
            "meshcore://channel/add?name={long}&secret=00112233445566778899aabbccddeeff"
        )),
        None,
        "longer than a slot takes"
    );
}

/// The stock app's contact codes (`testdata/meshcore/mesh.toml`, from MeshCore's
/// `docs/qr_codes.md`, its own example included): read, written back as lichen writes them,
/// and the refusals (§15.3; user request F-1).
#[test]
fn contact_codes() {
    use lc_core::plain::{contact_code, parse_contact_code};
    let path = format!(
        "{}/../../testdata/meshcore/mesh.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let t: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let (mut read, mut written, mut refused) = (0, 0, 0);
    for v in t["qr"].as_array().unwrap() {
        let text = v["text"].as_str().unwrap();
        if v.get("refused").and_then(toml::Value::as_bool) == Some(true) {
            assert_eq!(parse_contact_code(text), None, "{text}");
            refused += 1;
            continue;
        }
        let Some(name) = v.get("contact_name").and_then(toml::Value::as_str) else {
            // A channel's code is no contact's.
            assert_eq!(parse_contact_code(text), None, "{text}");
            continue;
        };
        let key: [u8; 32] = hex::decode(v["public_key"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let kind = u8::try_from(v["node_type"].as_integer().unwrap()).unwrap();
        assert_eq!(
            parse_contact_code(text),
            Some((name.to_owned(), key, kind)),
            "{text}"
        );
        read += 1;
        let ours = contact_code(name, &key, kind);
        assert_eq!(
            parse_contact_code(&ours),
            Some((name.to_owned(), key, kind))
        );
        // Written as lichen writes it: name, key, type, percent-encoded.
        if text.starts_with("meshcore://contact/add?name=") && !text.contains('+') {
            assert_eq!(ours, text);
            written += 1;
        }
    }
    assert_eq!((read, written, refused), (4, 2, 8));
}

/// `app` – `hub` – `room`, the radio knowing both; Bob, a contact, posted before in the room.
fn with_room(seed: u64) -> (Mesh, usize) {
    let mut m = mesh(seed);
    let room = m.net.add_room(12, "Refuge board", "keeper", "hello");
    m.net.link(m.hub, room);
    m.net.set_passwords(m.hub, "password", "hello");
    let (app, bob) = (m.app, m.bob);
    m.net.know(app, room, true);
    m.net
        .room_post(room, bob, "Spring flowing again", 1_793_000_000);
    m.net.connect(app);
    m.net.run_until(m.net.now + 10_000, |_| false);
    m.net.take_events(app);
    (m, room)
}

fn server(m: &Mesh, key: [u8; 32]) -> lc_core::Server {
    m.net
        .with_core_ref(m.app, |c| c.server(&key).cloned())
        .unwrap()
        .unwrap_or_default()
}

fn run(m: &mut Mesh, ms: u64) -> Vec<Event> {
    let app = m.app;
    let mut ev = Vec::new();
    m.net.run_until(m.net.now + ms, |n| {
        ev.extend(n.take_events(app));
        false
    });
    ev.extend(m.net.take_events(app));
    ev
}

/// A room server: a wrong password gets no answer; the right one lets us in, and the posts so
/// far come, with their authors; ours is acknowledged by the room (§15.8).
#[test]
fn a_room_server() {
    let (mut m, room) = with_room(8);
    let (app, key) = (m.app, m.net.key(room));
    m.net
        .with_core(app, |c, now| c.login(&key, "nope", now))
        .unwrap()
        .unwrap();
    assert_eq!(server(&m, key).login, lc_core::Login::Pending);
    run(&mut m, 40_000);
    let s = server(&m, key);
    assert_eq!(s.login, lc_core::Login::NoAnswer);
    assert!(s.no_answer);
    m.net
        .with_core(app, |c, now| c.login(&key, "hello", now))
        .unwrap()
        .unwrap();
    let ev = run(&mut m, 30_000);
    assert_eq!(
        server(&m, key).login,
        lc_core::Login::In(lc_core::Role::ReadWrite)
    );
    let posts: Vec<&lc_core::PlainMessage> = ev
        .iter()
        .filter_map(|e| match e {
            Event::PlainReceived(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].chat, lc_core::PlainChat::Room(key));
    assert_eq!(posts[0].label.as_deref(), Some("Bob"));
    assert_eq!(posts[0].from, Some(m.net.key(m.bob)));
    assert_eq!(posts[0].text, "Spring flowing again");
    assert_eq!(posts[0].sent_at, 1_793_000_000);
    // Our post: a direct message to the room, which keeps it and acknowledges it.
    let id = m
        .net
        .with_core(app, |c, now| {
            c.send_plain(&key, "Two of us tonight", 1_793_000_500, now)
        })
        .unwrap()
        .unwrap();
    let ev = run(&mut m, 30_000);
    assert!(ev.contains(&Event::Delivered { id }));
    assert_eq!(m.net.nodes[room].posts.len(), 2);
    // Someone else posts: it comes at once, we are logged in.
    let bob = m.bob;
    m.net.room_post(room, bob, "See you there", 1_793_000_900);
    let ev = run(&mut m, 30_000);
    assert!(ev.iter().any(|e| matches!(e,
        Event::PlainReceived(p) if p.text == "See you there")));
}

/// A room the user keeps is logged in to at each connection, once the contacts are read.
#[test]
fn a_kept_room_is_logged_in_again() {
    let (mut m, room) = with_room(9);
    let (app, key) = (m.app, m.net.key(room));
    m.net
        .with_core(app, |c, _| c.set_room_logins(vec![(key, "hello".into())]));
    m.net.disconnect(app);
    m.net.connect(app);
    let ev = run(&mut m, 30_000);
    assert_eq!(
        server(&m, key).login,
        lc_core::Login::In(lc_core::Role::ReadWrite)
    );
    assert!(ev.iter().any(|e| matches!(e, Event::PlainReceived(_))));
    // Disconnected: logged out, asked again at the next connection.
    m.net.disconnect(app);
    assert_eq!(server(&m, key).login, lc_core::Login::Out);
}

/// A repeater: status, neighbours and a command, one request at a time (§16.5).
#[test]
fn a_repeater_s_administration() {
    let (mut m, _room) = with_room(10);
    let (app, hub) = (m.app, m.net.key(m.hub));
    // A person's radio is not a server.
    let bob = m.net.key(m.bob);
    let r = m
        .net
        .with_core(app, |c, now| c.login(&bob, "x", now))
        .unwrap();
    assert_eq!(r, Err(lc_core::Error::NotAllowed));
    m.net
        .with_core(app, |c, now| c.login(&hub, "password", now))
        .unwrap()
        .unwrap();
    run(&mut m, 20_000);
    assert_eq!(
        server(&m, hub).login,
        lc_core::Login::In(lc_core::Role::Admin)
    );
    m.net.with_core(app, |c, now| {
        c.request_status(&hub, now).unwrap();
        c.request_neighbours(&hub, now).unwrap();
        // The same request twice is refused while the first is out.
        assert!(matches!(
            c.request_status(&hub, now),
            Err(lc_core::Error::TooSoon { .. })
        ));
        c.send_command(&hub, "ver", 1_793_000_000, now).unwrap();
    });
    run(&mut m, 40_000);
    let s = server(&m, hub);
    let st = s.status.unwrap();
    assert_eq!(st.battery_mv, 4_100);
    assert!(st.rx_air_time_s.is_some());
    let ns = s.neighbours.unwrap();
    assert_eq!(usize::from(s.neighbours_total), ns.len());
    // The hub hears the app, crest and the room.
    assert_eq!(ns.len(), 3);
    assert!(ns.iter().any(|n| n.name.as_deref() == Some("crest")));
    assert!(ns.iter().all(|n| n.prefix.len() == 6));
    assert_eq!(s.console.len(), 2);
    assert!(s.console[0].outgoing);
    assert_eq!(s.console[1].text, "v1.18.0 (Build: 15 Oct 2026)");
}

/// A guest does not get the console's answers; nothing comes for a signed post from a person.
#[test]
fn a_guest_has_no_console() {
    let (mut m, _room) = with_room(11);
    let (app, hub) = (m.app, m.net.key(m.hub));
    m.net
        .with_core(app, |c, now| c.login(&hub, "hello", now))
        .unwrap()
        .unwrap();
    run(&mut m, 20_000);
    assert_eq!(
        server(&m, hub).login,
        lc_core::Login::In(lc_core::Role::Guest)
    );
    m.net
        .with_core(app, |c, now| c.send_command(&hub, "ver", 1, now))
        .unwrap()
        .unwrap();
    run(&mut m, 20_000);
    assert_eq!(server(&m, hub).console.len(), 1, "only ours");
}

/// The battery is read again at most every 10 minutes, when the app asks (ADR-0042).
#[test]
fn the_battery_is_read_again_when_due() {
    let mut m = mesh(12);
    let app = m.app;
    let due = |m: &mut Mesh, at: u64| {
        m.net
            .with_core(app, |c, now| c.radio_refresh_if_due(now + at))
            .unwrap()
    };
    assert!(!due(&mut m, 0), "the first call starts the clock");
    assert!(!due(&mut m, 5 * 60_000));
    assert!(due(&mut m, 10 * 60_000));
    assert!(!due(&mut m, 11 * 60_000));
    m.net.disconnect(app);
    assert!(!due(&mut m, 30 * 60_000), "not while the radio is away");
}
