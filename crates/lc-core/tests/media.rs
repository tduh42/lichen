//! Media over the fake network (protocol.md §13): voice and image datagrams, the request for a
//! full image, and the airtime budget.

use lc_core::content::Content;
use lc_core::fakenet::Net;
use lc_core::{Error, Event};
use lc_meshcore::airtime::LoRaParams;

const S: u64 = 1_000;

fn pair(seed: u64) -> (Net, usize, usize) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, true);
    net.know(b, a, true);
    net.connect(a);
    net.connect(b);
    net.run_until(20 * S, |_| false);
    (net, a, b)
}

fn voice(seconds: u16) -> Content {
    let frames = seconds * 25;
    Content::Voice {
        sent_at: 1,
        mode: 1,
        frames,
        data: vec![0x5A; usize::from(frames) * 6],
    }
}

/// Ten seconds of 1200 voice over one hop: 11 fragments of 149 bytes, delivered whole.
#[test]
fn voice_message_is_delivered() {
    let (mut net, a, b) = pair(1);
    let bob = net.key(b);
    let v = voice(10);
    let (fragments, airtime) = net
        .with_core(a, |c, _| c.estimate_send(&bob, v.encode().len()))
        .unwrap();
    eprintln!("10 s of 1200 voice: {fragments} fragments, {airtime} ms of airtime per hop");
    assert_eq!(fragments, 11);
    net.with_core(a, |c, now| c.send_media(&bob, v.encode(), now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 120 * S, |_| false);
    let got: Vec<Content> = net
        .take_events(b)
        .into_iter()
        .filter_map(|e| match e {
            Event::Received { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![v]);
}

/// A request for a full image reaches the sender as an event, not as a message.
#[test]
fn media_request_is_an_event() {
    let (mut net, a, b) = pair(2);
    let alice = net.key(a);
    let req = Content::MediaRequest {
        media_id: [9; 8],
        full: true,
    };
    net.with_core(b, |c, now| c.send_datagram(&alice, req.encode(), now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 60 * S, |_| false);
    let events = net.take_events(a);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::MediaRequested { media_id, full: true, .. } if *media_id == [9; 8]
    )));
    assert!(!events.iter().any(|e| matches!(e, Event::Received { .. })));
}

/// Media beyond the hour's airtime are refused at once, with the time they would fit; text is
/// not refused.
#[test]
fn media_over_budget_are_refused() {
    let (mut net, a, b) = pair(3);
    let bob = net.key(b);
    let v = voice(15).encode();
    let mut accepted = 0;
    let refusal = loop {
        match net
            .with_core(a, |c, now| c.send_media(&bob, v.clone(), now))
            .unwrap()
        {
            Ok(_) => accepted += 1,
            Err(e) => break e,
        }
        assert!(accepted < 100, "never refused");
    };
    let (_, per) = net
        .with_core(a, |c, _| c.estimate_send(&bob, v.len()))
        .unwrap();
    eprintln!("15 s voice messages: {accepted} accepted ({per} ms each), then {refusal:?}");
    // 360 s an hour: about 360 / per.
    let fit = 360_000 / per;
    assert!(
        accepted as u64 >= fit - 1 && accepted as u64 <= fit,
        "{accepted} vs {fit}"
    );
    // Nothing has gone out yet: what is queued frees up an hour after it is sent.
    assert!(matches!(refusal, Error::OverBudget { wait_ms } if wait_ms == 3_600_000));
    // Once the queue has drained, the first transmissions leave the window first.
    net.run_until(net.now + 30 * 60 * S, |_| false);
    let first_sent = 20 * S;
    match net
        .with_core(a, |c, now| c.send_media(&bob, v.clone(), now))
        .unwrap()
    {
        Err(Error::OverBudget { wait_ms }) => {
            let at = net.now + wait_ms;
            assert!(
                at > first_sent + 3_600 * S && at < first_sent + 3_700 * S,
                "{at}"
            );
        }
        r => panic!("{r:?}"),
    }
    assert!(
        net.with_core(a, |c, now| c.send_text(&bob, "still fine", 1, now))
            .unwrap()
            .is_ok()
    );
}
