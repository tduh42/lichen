//! lichen over oxymesh's MeshCore node logic: real advert flooding and auto-add, real path
//! discovery, real direct forwarding by a repeating companion.

use lc_core::{Content, Event};
use lichen_sim::{App, pump, run};
use mc_sim::World;

/// alice — relay (companion with repeat on) — bob; adverts flooded so that the companions
/// add each other; then both apps connect.
fn setup(seed: u64, loss_milli: u32) -> (World, Vec<App>) {
    let mut w = World::new(seed);
    let a = w.add("alice", |_| {});
    let r = w.add("relay", |p| p.repeat = true);
    let b = w.add("bob", |_| {});
    w.link(a, r);
    w.link(r, b);
    w.loss_milli = loss_milli;
    for (i, delay) in [(a, 0u64), (b, 4_000)] {
        w.run(mc_core::Duration::from_millis(delay));
        w.app(i, &[7, 1]); // SEND_SELF_ADVERT, flood
    }
    w.run(mc_core::Duration::from_millis(15_000));
    let mut apps = vec![App::new(a, 1), App::new(b, 2)];
    let now = lichen_sim::ms(w.now);
    for app in &mut apps {
        app.core.connect(now);
    }
    assert!(run(&mut w, &mut apps, 10_000, |apps| apps
        .iter()
        .all(|a| a.events.contains(&Event::ContactsUpdated)
            && a.events.iter().any(|e| matches!(e, Event::Connected { .. })))));
    for app in &apps {
        assert_eq!(
            app.core.contacts().count(),
            1,
            "the advert flood added the other user"
        );
    }
    (w, apps)
}

fn key_of(apps: &[App], who: &str) -> [u8; 32] {
    apps[0]
        .core
        .contacts()
        .chain(apps[1].core.contacts())
        .find(|c| c.name == who)
        .unwrap()
        .key
}

#[test]
fn text_with_real_path_discovery() {
    let (mut w, mut apps) = setup(1, 0);
    let bob = key_of(&apps, "bob");
    let now = lichen_sim::ms(w.now);
    let id = apps[0]
        .core
        .send_text(&bob, "hello over oxymesh", 1_793_900_000, now)
        .unwrap();
    pump(&mut w, &mut apps);
    assert!(run(&mut w, &mut apps, 5 * 60_000, |apps| apps[0]
        .events
        .contains(&Event::Delivered { id })));
    let got: Vec<&Content> = apps[1]
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Received { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(
        got,
        vec![&Content::Text {
            sent_at: 1_793_900_000,
            text: "hello over oxymesh".into()
        }]
    );
    // The path came from lichen's probe: flooded by alice (node 0), re-flooded by the relay
    // (node 1) with its hash appended, then a PATH_REPLY and the session in direct mode.
    let probes: Vec<(usize, &Vec<u8>)> = w
        .log
        .iter()
        .filter(|(_, _, raw)| {
            let hops = usize::from(raw.get(1).copied().unwrap_or(0) & 0x3F);
            raw.first() == Some(&0x19) && raw.get(2 + hops) == Some(&0x7C)
        })
        .map(|(_, from, raw)| (*from, raw))
        .collect();
    assert!(
        probes.iter().any(|(from, raw)| *from == 0 && raw[1] == 0),
        "{probes:?}"
    );
    assert!(
        probes.iter().any(|(from, raw)| *from == 1 && raw[1] == 1),
        "{probes:?}"
    );
    let direct = w
        .log
        .iter()
        .filter(|(_, _, raw)| raw.first() == Some(&0x3E))
        .count();
    eprintln!(
        "delivered at {} s of simulated time, {} probe transmission(s), {direct} direct RAW_CUSTOM",
        lichen_sim::ms(w.now) / 1000,
        probes.len()
    );
}

#[test]
fn three_kilobytes_with_loss() {
    let (mut w, mut apps) = setup(2, 50);
    let bob = key_of(&apps, "bob");
    let mut data = vec![0x02];
    data.extend((0..2_999).map(|i| (i % 251) as u8));
    let now = lichen_sim::ms(w.now);
    let id = apps[0].core.send_datagram(&bob, data.clone(), now).unwrap();
    assert!(
        run(&mut w, &mut apps, 30 * 60_000, |apps| {
            apps[0]
            .events
            .iter()
            .any(|e| matches!(e, Event::Delivered { id: i } | Event::Failed { id: i, .. } if *i == id))
        }),
        "no outcome"
    );
    assert!(
        apps[0].events.contains(&Event::Delivered { id }),
        "{:?}",
        apps[0].events.iter().rev().take(3).collect::<Vec<_>>()
    );
    let received: Vec<Vec<u8>> = apps[1]
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Received { content, .. } => Some(content.encode()),
            _ => None,
        })
        .collect();
    assert_eq!(received, vec![data]);
}
