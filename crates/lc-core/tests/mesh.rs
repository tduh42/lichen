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

/// The stock app's shared contact links (`mesh.toml` `[[link]]`, signed by `mesh.py` with
/// test keys): read, the signature checked, and the frames lichen sends for each (§15.3,
/// §16.6; user request F-2). A contact code reads through the same call, unsigned.
#[test]
fn contact_links() {
    use lc_core::plain::{LinkError, parse_contact_code, read_meshcore_link};
    use lc_meshcore::Command;
    let path = format!(
        "{}/../../testdata/meshcore/mesh.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let t: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let bytes = |v: &toml::Value, k: &str| hex::decode(v[k].as_str().unwrap()).unwrap();
    let (mut signed, mut codes, mut refused) = (0, 0, 0);
    for v in t["link"].as_array().unwrap() {
        let text = v["text"].as_str().unwrap();
        match v.get("refused").and_then(toml::Value::as_str) {
            Some("code") => {
                assert_eq!(read_meshcore_link(text), Err(LinkError::NotALink), "{text}");
                refused += 1;
                continue;
            }
            Some(_) => {
                assert_eq!(
                    read_meshcore_link(text),
                    Err(LinkError::BadSignature),
                    "{text}"
                );
                refused += 1;
                continue;
            }
            None => {}
        }
        let link = read_meshcore_link(text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        assert_eq!(link.name, v["contact_name"].as_str().unwrap(), "{text}");
        assert_eq!(link.key.to_vec(), bytes(v, "public_key"));
        assert_eq!(i64::from(link.kind), v["node_type"].as_integer().unwrap());
        assert_eq!(
            i64::from(link.timestamp),
            v["timestamp"].as_integer().unwrap()
        );
        assert_eq!(link.signed(), v["signed"].as_bool().unwrap());
        assert_eq!(
            i64::from(link.hops),
            v["hops"].as_integer().unwrap(),
            "{text}"
        );
        assert_eq!(link.path, bytes(v, "path"), "{text}");
        let position = v.get("lat_e6").map(|lat| {
            let lon = v["lon_e6"].as_integer().unwrap();
            (lat.as_integer().unwrap() as i32, lon as i32)
        });
        assert_eq!(link.position, position, "{text}");
        let mut buf = [0u8; lc_meshcore::FRAME_MAX];
        let (lat_e6, lon_e6) = link.position.unwrap_or((0, 0));
        let n = Command::AddUpdateContact {
            key: &link.key,
            kind: link.kind,
            name: link.name.as_bytes(),
            last_advert: link.timestamp,
            lat_e6,
            lon_e6,
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf[..n], bytes(v, "add_frame")[..], "{text}");
        match &link.advert {
            Some(packet) => {
                let n = (Command::ImportContact { packet })
                    .encode(&mut buf)
                    .unwrap();
                assert_eq!(buf[..n], bytes(v, "import_frame")[..], "{text}");
                signed += 1;
            }
            None => codes += 1,
        }
    }
    assert_eq!((signed, codes, refused), (9, 1, 22));
    // F-1's codes read the same through it.
    for v in t["qr"].as_array().unwrap() {
        let text = v["text"].as_str().unwrap();
        match parse_contact_code(text) {
            Some((name, key, kind)) => {
                let link = read_meshcore_link(text).unwrap();
                assert!(!link.signed() && link.position.is_none());
                assert_eq!((link.name, link.key, link.kind), (name, key, kind));
            }
            None => assert_eq!(read_meshcore_link(text), Err(LinkError::NotALink), "{text}"),
        }
    }
}

/// A node's shared link, signed with a test key: `meshcore://` and the hex of its advert.
fn signed_link(seed: u8, kind: u8, name: &str, position: Option<(i32, i32)>) -> (String, [u8; 32]) {
    signed_link_via(seed, kind, name, position, &[])
}

/// The same, its advert having come through `path` (1-byte hashes) to the radio that shared it.
fn signed_link_via(
    seed: u8,
    kind: u8,
    name: &str,
    position: Option<(i32, i32)>,
    path: &[u8],
) -> (String, [u8; 32]) {
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
    let mut raw = vec![0x11, u8::try_from(path.len()).unwrap()];
    raw.extend_from_slice(path);
    raw.extend_from_slice(&key);
    raw.extend_from_slice(&ts);
    raw.extend_from_slice(&sig);
    raw.extend_from_slice(&data);
    (format!("meshcore://{}", hex::encode(raw)), key)
}

fn add_link(m: &mut Mesh, text: &str) -> Result<lc_core::MeshAdd, lc_core::Error> {
    let link = lc_core::plain::read_meshcore_link(text).unwrap();
    m.net
        .with_core(m.app, |c, _| c.add_meshcore_contact(&link))
        .unwrap()
}

/// What the app's radio holds of `key` added from a link: the advert's position, and whether
/// it keeps the advert itself.
fn on_radio(m: &Mesh, key: [u8; 32]) -> Option<(Option<(i32, i32)>, bool)> {
    m.net.nodes[m.app]
        .contact_adverts
        .iter()
        .find(|c| c.key == key)
        .map(|c| (c.position, c.kept))
}

/// A signed link: imported, so the radio keeps the advert; the node comes as any contact,
/// with its type and position. Added again, it is already there (§16.6).
#[test]
fn a_shared_link_is_imported() {
    let mut m = mesh(21);
    let (text, key) = signed_link(
        40,
        contact_kind::REPEATER,
        "ridge",
        Some((12_345_678, -98_765_432)),
    );
    m.net.take_events(m.app);
    assert_eq!(add_link(&mut m, &text), Ok(lc_core::MeshAdd::Sent));
    assert!(run(&mut m, 5_000).contains(&Event::ContactsUpdated));
    assert_eq!(
        on_radio(&m, key),
        Some((Some((12_345_678, -98_765_432)), true))
    );
    let nodes = nodes_of(&m);
    let ridge = find(&nodes, key);
    assert!(ridge.on_radio);
    assert_eq!(
        (ridge.name.as_str(), ridge.kind),
        ("ridge", contact_kind::REPEATER)
    );
    assert_eq!(ridge.position, Some((12_345_678, -98_765_432)));
    assert_eq!(
        add_link(&mut m, &text),
        Ok(lc_core::MeshAdd::AlreadyOnTheRadio)
    );
    assert!(
        m.net
            .with_core(m.app, |c, _| c.take_radio_refusals())
            .unwrap()
            .is_empty()
    );
}

/// A radio in manual add does not take an imported advert (it pushes `NEW_ADVERT`): lichen
/// adds the node with `ADD_UPDATE_CONTACT` from the advert's fields (§16.6, ADR-0047).
#[test]
fn a_link_the_radio_does_not_take_is_added() {
    let mut m = mesh(22);
    let app = m.app;
    m.net.nodes[app].settings.prefs[3] = 1;
    m.net.nodes[app].settings.autoadd = (0, 0);
    let (text, key) = signed_link(41, contact_kind::CHAT, "Hut radio", None);
    assert_eq!(add_link(&mut m, &text), Ok(lc_core::MeshAdd::Sent));
    run(&mut m, 5_000);
    assert_eq!(on_radio(&m, key), Some((None, false)));
    let hut = m
        .net
        .with_core_ref(app, |c| c.contacts().find(|c| c.key == key).cloned())
        .unwrap()
        .expect("a person on the radio");
    assert_eq!(
        (hut.name.as_str(), hut.kind),
        ("Hut radio", contact_kind::CHAT)
    );
    assert!(find(&nodes_of(&m), key).on_radio);
}

/// The link's path goes to the radio with the advert (§16.6): under the auto-add hop limit the
/// radio takes it; at the limit it does not, and lichen adds the node with
/// `ADD_UPDATE_CONTACT`, with no path of its own either way.
#[test]
fn a_link_s_path_and_the_hop_limit() {
    let mut m = mesh(25);
    let app = m.app;
    m.net.nodes[app].settings.autoadd = (0, 2);
    let (near, near_key) = signed_link_via(44, contact_kind::CHAT, "near", None, &[0x5A]);
    let link = lc_core::plain::read_meshcore_link(&near).unwrap();
    assert_eq!((link.hops, link.path.as_slice()), (1, &[0x5A][..]));
    assert_eq!(link.advert.as_ref().unwrap()[1..3], [1, 0x5A]);
    let (far, far_key) = signed_link_via(45, contact_kind::CHAT, "far", None, &[0x5A, 0x77]);
    assert_eq!(add_link(&mut m, &near), Ok(lc_core::MeshAdd::Sent));
    assert_eq!(add_link(&mut m, &far), Ok(lc_core::MeshAdd::Sent));
    run(&mut m, 5_000);
    assert_eq!(on_radio(&m, near_key), Some((None, true)));
    assert_eq!(on_radio(&m, far_key), Some((None, false)));
    let nodes = nodes_of(&m);
    for key in [near_key, far_key] {
        let n = find(&nodes, key);
        assert!(n.on_radio);
        assert_eq!(n.out_path, None, "no path of its own");
    }
}

/// A full radio: the import is pushed back as heard, and `ADD_UPDATE_CONTACT` answers `ERR 3`:
/// the contact is refused like a setting (§1.1, §16.6).
#[test]
fn a_full_radio_refuses_a_link() {
    let mut m = mesh(23);
    let app = m.app;
    m.net.nodes[app].contacts_max = 3;
    let (text, key) = signed_link(42, contact_kind::CHAT, "Hut radio", None);
    assert_eq!(add_link(&mut m, &text), Ok(lc_core::MeshAdd::Sent));
    run(&mut m, 5_000);
    assert_eq!(on_radio(&m, key), None);
    assert_eq!(
        m.net
            .with_core(app, |c, _| c.take_radio_refusals())
            .unwrap(),
        vec![lc_core::radio::RadioSetting::Contact]
    );
    assert!(!find(&nodes_of(&m), key).on_radio);
}

/// A contact code is added unsigned, with `ADD_UPDATE_CONTACT`; the radio's own code is not
/// sent; without a radio nothing is.
#[test]
fn a_contact_code_and_what_is_not_sent() {
    use lc_core::plain::contact_code;
    let mut m = mesh(24);
    let key = [0x42; 32];
    let code = contact_code("hub west", &key, contact_kind::REPEATER);
    assert_eq!(add_link(&mut m, &code), Ok(lc_core::MeshAdd::Sent));
    run(&mut m, 5_000);
    assert_eq!(on_radio(&m, key), Some((None, false)));
    assert_eq!(find(&nodes_of(&m), key).kind, contact_kind::REPEATER);
    let own = contact_code("app", &m.net.key(m.app), contact_kind::CHAT);
    assert_eq!(add_link(&mut m, &own), Ok(lc_core::MeshAdd::OwnRadio));
    let app = m.app;
    m.net.disconnect(app);
    run(&mut m, 1_000);
    let (text, _) = signed_link(43, contact_kind::CHAT, "Hut radio", None);
    assert_eq!(add_link(&mut m, &text), Err(lc_core::Error::NoIdentity));
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
