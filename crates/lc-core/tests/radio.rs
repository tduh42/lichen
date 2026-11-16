//! The radio's settings over the fake network (protocol §1.1, ADR-0032): read at connection,
//! changed, read back.

use lc_core::Event;
use lc_core::core::Error;
use lc_core::fakenet::Net;
use lc_meshcore::OtherParams;
use lc_meshcore::airtime::LoRaParams;

fn connected(gps: Option<bool>) -> (Net, usize) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, 1);
    let a = net.add_companion(1, "alice");
    net.nodes[a].settings.gps = gps;
    net.nodes[a].settings.autoadd = (0x02, 3);
    net.connect(a);
    assert!(net.run_until(net.now + 10_000, |n| {
        n.nodes[a].core.as_ref().is_some_and(|c| c.ready())
    }));
    net.run_until(net.now + 2_000, |_| false);
    (net, a)
}

/// Events seen by `a` since the last call.
fn drain(net: &mut Net, a: usize) -> Vec<Event> {
    std::mem::take(&mut net.nodes[a].events)
}

#[test]
fn settings_are_read_at_connection() {
    let (net, a) = connected(Some(false));
    let r = net
        .with_core_ref(a, |c| c.radio_settings().clone())
        .unwrap();
    assert_eq!(r.name, "alice");
    assert_eq!((r.sf, r.bw_hz, r.tx_dbm, r.max_tx_dbm), (8, 62_500, 22, 22));
    assert_eq!(r.var("gps"), Some("0"));
    assert_eq!(r.autoadd, Some((0x02, 3)));
    assert_eq!(r.battery_mv, Some(4_012));
    assert_eq!(
        (r.storage_used_kb, r.storage_total_kb),
        (Some(96), Some(1_024))
    );
    assert_eq!(r.other, OtherParams::default());
}

#[test]
fn a_change_is_applied_and_read_back_without_a_new_connection() {
    let (mut net, a) = connected(Some(false));
    drain(&mut net, a);
    net.with_core(a, |c, _| c.set_radio_var("gps", "1"))
        .unwrap()
        .unwrap();
    let other = OtherParams {
        manual_add: true,
        telemetry: 2,
        advert_loc_policy: 1,
        multi_acks: 1,
    };
    net.with_core(a, |c, _| c.set_other_params(other))
        .unwrap()
        .unwrap();
    net.with_core(a, |c, _| c.set_advert_position(45_000_000, 6_000_000))
        .unwrap()
        .unwrap();
    net.with_core(a, |c, _| c.set_radio_params(869_525, 250_000, 11, 5))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5_000, |_| false);
    assert!(net.nodes[a].settings.gps == Some(true));
    let r = net
        .with_core_ref(a, |c| c.radio_settings().clone())
        .unwrap();
    assert_eq!(r.var("gps"), Some("1"));
    assert_eq!(r.other, other);
    assert_eq!((r.lat_e6, r.lon_e6), (45_000_000, 6_000_000));
    assert_eq!((r.freq_khz, r.bw_hz, r.sf, r.cr), (869_525, 250_000, 11, 5));
    let events = drain(&mut net, a);
    assert!(events.iter().any(|e| matches!(e, Event::RadioSettings)));
    assert!(
        !events.iter().any(|e| matches!(e, Event::Connected { .. })),
        "a settings read is not a new connection: {events:?}"
    );
    // The airtime model follows the radio's parameters.
    let radio = net.with_core_ref(a, |c| *c.radio()).unwrap();
    assert_eq!((radio.sf, radio.bw_hz, radio.cr), (11, 250_000, 5));
}

#[test]
fn what_the_radio_would_refuse_is_not_sent() {
    let (mut net, a) = connected(None);
    for r in [
        net.with_core(a, |c, _| c.set_radio_params(100, 62_500, 8, 5))
            .unwrap(),
        net.with_core(a, |c, _| c.set_radio_params(869_525, 62_500, 13, 5))
            .unwrap(),
        net.with_core(a, |c, _| c.set_tx_power(23)).unwrap(),
        net.with_core(a, |c, _| c.set_advert_position(91_000_000, 0))
            .unwrap(),
        net.with_core(a, |c, _| c.set_device_pin(12_345)).unwrap(),
        // A board without a GPS has no `gps` to set.
        net.with_core(a, |c, _| c.set_radio_var("gps", "1"))
            .unwrap(),
        net.with_core(a, |c, _| {
            c.set_other_params(OtherParams {
                telemetry: 3,
                ..OtherParams::default()
            })
        })
        .unwrap(),
    ] {
        assert_eq!(r, Err(Error::Invalid));
    }
    assert_eq!(
        net.with_core_ref(a, |c| c.radio_settings().var("gps").map(str::to_owned))
            .unwrap(),
        None
    );
}
