//! A lichen group on oxymesh's MeshCore node logic, next to a stock node that follows a
//! channel whose hash is 0x7C, lichen's channel hash (protocol.md §10.3, ADR-0008).

use lc_core::{Content, Event};
use lichen_sim::{App, introduce, ms, pump, run};
use mc_sim::World;
use sha2::{Digest, Sha256};

/// A 16-byte channel secret whose MeshCore hash (`SHA256(secret)[0]`) is 0x7C.
fn colliding_secret() -> [u8; 16] {
    (0u64..)
        .map(|i| {
            let mut s = [0u8; 16];
            s[..8].copy_from_slice(&i.to_le_bytes());
            s[8] = 0x5A;
            s
        })
        .find(|s| Sha256::digest(s)[0] == 0x7C)
        .unwrap()
}

#[test]
fn group_next_to_a_stock_node_on_the_same_channel_hash() {
    let mut w = World::new(5);
    let a = w.add("alice", |_| {});
    let r = w.add("relay", |p| p.repeat = true);
    let b = w.add("bob", |_| {});
    let c = w.add("carol", |_| {});
    let stock = w.add("stock", |_| {});
    for x in [a, b, c, stock] {
        w.link(x, r);
    }
    // The stock node follows a named channel with hash 0x7C.
    let mut set = vec![32u8, 1];
    let mut name = [0u8; 32];
    name[..6].copy_from_slice(b"#clash");
    set.extend_from_slice(&name);
    set.extend_from_slice(&colliding_secret());
    let ok = w.app(stock, &set);
    assert_eq!(ok.first().map(|f| f[0]), Some(0), "SET_CHANNEL accepted");
    w.nodes[stock].app.clear();

    for (i, x) in [a, b, c].into_iter().enumerate() {
        w.run(mc_core::Duration::from_millis(3_000 * i as u64));
        w.app(x, &[7, 1]);
    }
    w.run(mc_core::Duration::from_millis(20_000));
    let mut apps = vec![App::new(a, 1), App::new(b, 2), App::new(c, 3)];
    let now = ms(w.now);
    for app in &mut apps {
        app.core.connect(now);
    }
    assert!(run(&mut w, &mut apps, 10_000, |apps| apps
        .iter()
        .all(|a| a.events.contains(&Event::ContactsUpdated))));
    let members: Vec<[u8; 32]> = apps[0]
        .core
        .contacts()
        .filter(|c| c.name == "bob" || c.name == "carol")
        .map(|c| c.key)
        .collect();
    assert_eq!(members.len(), 2);
    // Alice scanned Bob's and Carol's codes (ADR-0013).
    introduce(&mut apps, 0, 1);
    introduce(&mut apps, 0, 2);
    let now = ms(w.now);
    let gid = apps[0].core.create_group("trail", &members, now).unwrap();
    pump(&mut w, &mut apps);
    assert!(
        run(&mut w, &mut apps, 40 * 60_000, |apps| apps.iter().all(
            |a| a.core.groups().any(|g| g.gid == gid && g.keys_known() == 2)
        )),
        "sender keys not distributed"
    );
    run(&mut w, &mut apps, 5 * 60_000, |_| false);
    let now = ms(w.now);
    apps[1]
        .core
        .send_group_text(
            &gid,
            "hello trail, from oxymesh's nodes",
            1_793_900_000,
            now,
        )
        .unwrap();
    assert!(run(&mut w, &mut apps, 3 * 60_000, |apps| [0, 2]
        .iter()
        .all(|&i| apps[i].events.iter().any(
            |e| matches!(e, Event::GroupMessage { content: Content::Text { text, .. }, .. }
            if text == "hello trail, from oxymesh's nodes")
        ))));

    // The stock node heard the floods and showed nothing: no channel message, no data, no
    // "message waiting".
    let flooded = w
        .log
        .iter()
        .filter(|(_, from, raw)| *from == r && raw.first() == Some(&0x19))
        .count();
    assert!(flooded > 0, "the relay re-flooded lichen envelopes");
    let shown: Vec<u8> = w.nodes[stock].app.iter().map(|f| f[0]).collect();
    assert!(
        !shown.iter().any(|c| matches!(c, 8 | 17 | 27 | 0x83)),
        "stock node pushed {shown:02x?}"
    );
}
