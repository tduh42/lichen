//! Plain MeshCore text over the fake network (protocol.md §15, ADR-0026): a lichen app and
//! someone with a stock client, one repeater apart.

use lc_core::fakenet::Net;
use lc_core::plain::{PUBLIC_NAME, PUBLIC_SECRET, channel_id};
use lc_core::{Event, PlainChat};
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;

fn radio() -> LoRaParams {
    LoRaParams::meshcore(8, 62_500, 5).unwrap()
}

/// `app` (lichen) – repeater – `marc` (a stock client), each in the other's contacts.
fn pair(seed: u64) -> (Net, usize, usize) {
    let mut net = Net::new(radio(), 0, seed);
    let app = net.add_companion(1, "app");
    let r = net.add_repeater(10, "rpt");
    let marc = net.add_stock_companion(3, "Marc");
    net.chain(&[app, r, marc]);
    net.know(app, marc, true);
    net.know(marc, app, true);
    net.set_channel(app, 0, PUBLIC_NAME, PUBLIC_SECRET);
    net.set_channel(marc, 0, PUBLIC_NAME, PUBLIC_SECRET);
    net.connect(app);
    net.run_until(net.now + 10_000, |_| false);
    (net, app, marc)
}

fn plain(events: &[Event]) -> Vec<lc_core::PlainMessage> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::PlainReceived(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_direct_message_both_ways() {
    let (mut net, app, marc) = pair(1);
    assert!(net.with_core_ref(app, |c| c.channels_known()).unwrap());

    // Marc writes from his stock app.
    net.send_plain_text(marc, app, "Salut, tu es sur lichen ?", 1_793_000_000);
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + MIN, |n| {
        ev.extend(n.take_events(app));
        !plain(&ev).is_empty()
    }));
    let got = plain(&ev);
    assert_eq!(got[0].chat, PlainChat::Direct(net.key(marc)));
    assert_eq!(got[0].label.as_deref(), Some("Marc"));
    assert_eq!(got[0].text, "Salut, tu es sur lichen ?");
    assert_eq!(got[0].hops, None, "it came along a known path");

    // The answer, acknowledged by Marc's radio.
    let to = net.key(marc);
    let id = net
        .with_core(app, |c, now| {
            c.send_plain(&to, "Oui ! Écris-moi ici.", 1_793_000_060, now)
        })
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + MIN, |n| {
        ev.extend(n.take_events(app));
        ev.contains(&Event::Delivered { id })
    }));
    let inbox: Vec<Vec<u8>> = net.nodes[marc].inbox.iter().cloned().collect();
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].ends_with("Oui ! Écris-moi ici.".as_bytes()));
}

/// Posts on Public both ways; a post by someone else reaches the app with its author's name.
#[test]
fn public_channel_posts() {
    let (mut net, app, marc) = pair(2);
    net.post_on_channel(marc, 0, "Quelqu'un au col ?", 1_793_000_000);
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + MIN, |n| {
        ev.extend(n.take_events(app));
        !plain(&ev).is_empty()
    }));
    let got = plain(&ev);
    assert_eq!(got[0].chat, PlainChat::Channel(channel_id(&PUBLIC_SECRET)));
    assert_eq!(
        (got[0].label.as_deref(), got[0].text.as_str()),
        (Some("Marc"), "Quelqu'un au col ?")
    );

    let ch = channel_id(&PUBLIC_SECRET);
    let id = net
        .with_core(app, |c, now| {
            c.send_channel(&ch, "Moi, j'y suis.", 1_793_000_030, now)
        })
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + MIN, |n| {
        ev.extend(n.take_events(app));
        ev.contains(&Event::GroupSent { id }) && !n.nodes[marc].inbox.is_empty()
    }));
    assert!(net.nodes[marc].inbox[0].ends_with(b"app: Moi, j'y suis."));
}

/// Messages that arrived while the phone was away wait in the companion and come at the next
/// connection (§15.1).
#[test]
fn messages_wait_in_the_companion() {
    let (mut net, app, marc) = pair(3);
    net.disconnect(app);
    net.send_plain_text(marc, app, "Tu es là ?", 1_793_000_000);
    net.post_on_channel(marc, 0, "Météo : orage vers 16 h", 1_793_000_010);
    net.run_until(net.now + MIN, |_| false);
    assert_eq!(net.nodes[app].inbox.len(), 2);
    net.take_events(app);
    net.connect(app);
    let mut ev = Vec::new();
    net.run_until(net.now + 20_000, |n| {
        ev.extend(n.take_events(app));
        plain(&ev).len() == 2
    });
    let texts: Vec<String> = plain(&ev).into_iter().map(|m| m.text).collect();
    assert_eq!(texts, ["Tu es là ?", "Météo : orage vers 16 h"]);
    assert!(net.nodes[app].inbox.is_empty());
}
