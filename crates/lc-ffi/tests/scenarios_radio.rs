//! Radio link and radio settings (docs/testing.md §2, RL-1 to RL-7). RL-8 and RL-9 are
//! covered in lc-core (`tests/radio.rs`, the contact list's unit tests).

mod common;

use common::{Sim, contact_chat, copies, find_contact};
use lc_ffi::{
    ChannelKind, ChatRef, FailReason, LichenError, LichenEvent, LinkKind, MessageStatus, Telemetry,
    WaitReason,
};

fn connected(sim: &mut Sim) -> (String, lc_ffi::Radio) {
    sim.until_event("Connected", 10, |e| match e {
        LichenEvent::Connected { name, radio, .. } => Some((name.clone(), radio.clone())),
        _ => None,
    })
}

/// RL-1: the demo radio connects under its name; its contacts and channels arrive.
#[test]
fn rl_1_demo_radio_connects() {
    let mut sim = Sim::new(71);
    let (name, radio) = connected(&mut sim);
    assert_eq!(name, "ThinkNode M5");
    assert_eq!((radio.sf, radio.bw_hz, radio.cr), (8, 62_500, 5));
    assert!(radio.freq_khz > 0);
    assert_eq!(sim.core.radio(), radio);
    assert!(sim.core.node_key().is_some());
    sim.until("the radio's contacts", 10, |c| {
        ["Camille", "Julien", "Marc", "hub", "crest", "Refuge board"]
            .iter()
            .all(|n| find_contact(c, n).is_some())
    });
    assert!(
        sim.events
            .iter()
            .any(|(_, e)| matches!(e, LichenEvent::ContactsUpdated))
    );
    sim.until("the radio's channels", 10, |c| {
        c.channels()
            .iter()
            .any(|ch| ch.name == "Public" && ch.kind == ChannelKind::PublicChannel)
    });
    assert!(
        sim.events
            .iter()
            .any(|(_, e)| matches!(e, LichenEvent::ChannelsUpdated))
    );
}

/// RL-2: the radio drops with a text waiting for Camille and one for the group; both wait
/// for the radio, and go once when it is back: Camille answers once, the group post floods.
#[test]
fn rl_2_radio_drops_and_comes_back() {
    let mut sim = Sim::new(72);
    let camille = sim.camille();
    // Our first text lets Camille know us: she makes the group (§10.1).
    sim.core
        .send_text(camille.key.clone(), "Hello".into(), sim.unix_s(), sim.now)
        .unwrap();
    let group = sim.rando();
    sim.run_for(20_000);
    sim.set_radio(false);
    let dm = sim
        .core
        .send_text(
            camille.key.clone(),
            "Written while my radio is off".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let post = sim
        .core
        .send_group_text(
            group.gid.clone(),
            "On my way, radio off for now".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let radio_wait = |s: &MessageStatus| {
        matches!(
            s,
            MessageStatus::Waiting {
                reason: WaitReason::Radio,
                started: false,
                ..
            }
        )
    };
    assert!(radio_wait(&dm.status), "{:?}", dm.status);
    assert!(radio_wait(&post.status), "{:?}", post.status);
    let gchat = ChatRef::Group {
        gid: group.gid.clone(),
    };
    let cchat = contact_chat(&camille);
    // Off for a minute and a half: still waiting.
    sim.run_for(90_000);
    assert!(radio_wait(&sim.message(&cchat, dm.id).status));
    assert!(radio_wait(&sim.message(&gchat, post.id).status));
    sim.set_radio(true);
    sim.until_status(&cchat, dm.id, 120, |s| *s == MessageStatus::Delivered);
    sim.until_status(&gchat, post.id, 60, |s| *s == MessageStatus::Flooded);
    let echo = "Copy: \"Written while my radio is off\". I'm at the refuge, one hop away.";
    sim.until("Camille's answer", 60, |c| {
        copies(&c.history(cchat.clone(), None, 500).unwrap(), echo) > 0
    });
    sim.run_for(30_000);
    let h = sim.history(&cchat);
    assert_eq!(copies(&h, "Written while my radio is off"), 1);
    assert_eq!(copies(&h, echo), 1, "she got it once");
    assert_eq!(
        copies(&sim.history(&gchat), "On my way, radio off for now"),
        1
    );
}

/// RL-3: with texts expiring after 30 s (`-expiry 30`), a text written while the radio is off
/// fails as expired; sent again, it waits, then goes once the radio is back.
#[test]
fn rl_3_a_text_expires_while_offline() {
    let mut sim = Sim::new(73);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    sim.core.set_outbox_expiry(30_000);
    sim.set_radio(false);
    let m = sim
        .core
        .send_text(
            camille.key.clone(),
            "Too late".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let expires = match m.status {
        MessageStatus::Waiting { expires_at_ms, .. } => expires_at_ms,
        ref s => panic!("{s:?}"),
    };
    assert_eq!(expires - m.at_ms, 30_000);
    // Not yet at 20 s.
    sim.run_for(20_000);
    assert!(matches!(
        sim.message(&chat, m.id).status,
        MessageStatus::Waiting { .. }
    ));
    sim.until_event("the text's expiry", 30, |e| match e {
        LichenEvent::Failed { reason, .. } => Some(*reason),
        _ => None,
    });
    assert_eq!(
        sim.message(&chat, m.id).status,
        MessageStatus::Failed {
            reason: FailReason::Expired
        }
    );
    // Sent again: the same message, waiting.
    sim.core.set_outbox_expiry(24 * 3_600_000);
    let again = sim.core.retry_message(m.id, sim.now).unwrap();
    assert_eq!(again.id, m.id);
    assert!(matches!(again.status, MessageStatus::Waiting { .. }));
    sim.set_radio(true);
    sim.until_status(&chat, m.id, 120, |s| *s == MessageStatus::Delivered);
    let echo = "Copy: \"Too late\". I'm at the refuge, one hop away.";
    sim.until("Camille's answer", 120, |c| {
        copies(&c.history(chat.clone(), None, 50).unwrap(), echo) > 0
    });
    sim.run_for(30_000);
    let h = sim.history(&chat);
    assert_eq!(copies(&h, "Too late"), 1);
    assert_eq!(copies(&h, echo), 1, "she got it once");
}

/// RL-4: the saved radios list what was paired; the one connected is current; forgetting
/// removes it; the list survives a restart, and the radio connects again.
#[test]
fn rl_4_saved_radios() {
    let mut sim = Sim::persistent(74, "rl4");
    connected(&mut sim);
    sim.core
        .remember_radio("BLE-M5".into(), LinkKind::Bluetooth, "M5".into())
        .unwrap();
    let r = sim.core.saved_radios();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].address, "BLE-M5");
    assert_eq!(r[0].name, "ThinkNode M5", "the radio's own name");
    assert_eq!(r[0].node, sim.core.node_key());
    assert!(r[0].current);
    let first_use = r[0].last_used_ms;
    // Saved again: still one.
    sim.core
        .remember_radio("BLE-M5".into(), LinkKind::Bluetooth, "M5".into())
        .unwrap();
    assert_eq!(sim.core.saved_radios().len(), 1);
    // A second radio, over TCP (lichen bridge): the latest first.
    sim.run_for(2_000);
    sim.core
        .remember_radio("127.0.0.1:5000".into(), LinkKind::Tcp, "bridge".into())
        .unwrap();
    let r = sim.core.saved_radios();
    assert_eq!(r.len(), 2);
    assert!(
        r[0].last_used_ms >= r[1].last_used_ms,
        "the most recently used first: {r:?}"
    );
    sim.set_radio(false);
    assert!(sim.core.saved_radios().iter().all(|r| !r.current));
    // Kept across a restart; connected again, current again.
    sim.restart(1_000);
    assert_eq!(sim.core.saved_radios().len(), 2);
    connected(&mut sim);
    sim.core
        .remember_radio("BLE-M5".into(), LinkKind::Bluetooth, "M5".into())
        .unwrap();
    let r = sim.core.saved_radios();
    let m5 = r.iter().find(|r| r.address == "BLE-M5").unwrap();
    assert!(m5.current);
    assert!(m5.last_used_ms >= first_use);
    assert_eq!(r[0].address, "BLE-M5", "the latest first");
    // Forgotten: gone.
    sim.core.forget_radio("127.0.0.1:5000".into()).unwrap();
    let r = sim.core.saved_radios();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].address, "BLE-M5");
    sim.core.forget_radio("BLE-M5".into()).unwrap();
    assert!(sim.core.saved_radios().is_empty());
}

/// RL-5: the settings shown are the radio's; each change is applied by the radio and read
/// back.
#[test]
fn rl_5_radio_settings_read_change_read_back() {
    let mut sim = Sim::new(75);
    let (_, radio) = connected(&mut sim);
    sim.until("the radio's settings", 10, |c| {
        let s = c.radio_settings();
        s.battery_mv.is_some() && s.autoadd_mask.is_some() && s.gps.is_some()
    });
    let s = sim.core.radio_settings();
    assert_eq!(s.name, "ThinkNode M5");
    assert_eq!(
        (s.freq_khz, s.bw_hz, s.sf, s.cr),
        (radio.freq_khz, radio.bw_hz, radio.sf, radio.cr)
    );
    assert_eq!(s.gps, Some(false), "a GPS, off as shipped");
    assert_eq!(s.autoadd_mask, Some(0x03));
    assert!(s.max_tx_dbm >= s.tx_dbm);
    assert!(!s.position_in_adverts);
    let from = sim.events.len();

    sim.core.set_tx_power(s.max_tx_dbm - 2).unwrap();
    sim.core.set_gps(true).unwrap();
    sim.core.set_autoadd(0x02 | 0x04, 2).unwrap();
    sim.core
        .set_radio_preferences(
            true,
            Telemetry {
                base: 1,
                location: 0,
                environment: 2,
            },
            false,
            1,
        )
        .unwrap();
    sim.core.set_advert_position(44_888_500, 5_498_500).unwrap();
    sim.core.set_radio_params(869_525, 125_000, 9, 6).unwrap();
    sim.until("the changes read back", 20, |c| {
        let n = c.radio_settings();
        n.gps == Some(true)
            && n.tx_dbm == s.max_tx_dbm - 2
            && n.autoadd_mask == Some(0x06)
            && n.manual_add
            && n.sf == 9
    });
    let n = sim.core.radio_settings();
    assert_eq!(n.autoadd_max_hops, Some(2));
    assert_eq!(
        n.telemetry,
        Telemetry {
            base: 1,
            location: 0,
            environment: 2
        }
    );
    assert_eq!(n.multi_acks, 1);
    assert_eq!((n.lat_e6, n.lon_e6), (44_888_500, 5_498_500));
    assert!(!n.position_in_adverts, "the position stays out of adverts");
    assert_eq!((n.freq_khz, n.bw_hz, n.sf, n.cr), (869_525, 125_000, 9, 6));
    assert!(
        sim.since(from)
            .any(|e| matches!(e, LichenEvent::RadioSettings)),
        "the app is told"
    );
    // The name MeshCore users see: shown at once, the radio's own at the next connection.
    sim.core.set_mesh_name("Trail M5".into(), sim.now).unwrap();
    assert_eq!(sim.core.mesh_name(), "Trail M5");
    assert_eq!(
        sim.core.set_mesh_name("x".repeat(32), sim.now),
        Err(LichenError::TooLarge)
    );
    sim.run_for(5_000);
    sim.set_radio(false);
    sim.run_for(1_000);
    sim.set_radio(true);
    sim.until("the radio's name", 20, |c| {
        c.radio_settings().name == "Trail M5"
    });
    assert_eq!(sim.core.mesh_name(), "Trail M5");
}

/// RL-6: a setting out of range is refused with an error, nothing is sent, and the value
/// shown stays the radio's; with the radio away, settings wait for it to connect.
#[test]
fn rl_6_a_refused_setting() {
    let mut sim = Sim::new(76);
    connected(&mut sim);
    sim.until("the radio's settings", 10, |c| {
        c.radio_settings().autoadd_mask.is_some()
    });
    let before = sim.core.radio_settings();
    let refused = [
        sim.core.set_tx_power(before.max_tx_dbm + 1),
        sim.core.set_tx_power(-20),
        sim.core.set_radio_params(100_000, 125_000, 9, 5),
        sim.core.set_radio_params(869_525, 125_000, 13, 5),
        sim.core.set_radio_params(869_525, 125_000, 9, 9),
        sim.core.set_gps_interval(90_000),
        sim.core.set_ble_pin(12),
        sim.core.set_advert_position(91_000_000, 0),
        sim.core.set_radio_preferences(
            false,
            Telemetry {
                base: 3,
                location: 0,
                environment: 0,
            },
            false,
            0,
        ),
    ];
    for r in &refused {
        assert_eq!(*r, Err(LichenError::InvalidSetting));
    }
    assert_eq!(sim.core.poll_companion(sim.now), None, "nothing sent");
    sim.run_for(10_000);
    let after = sim.core.radio_settings();
    assert_eq!(after.tx_dbm, before.tx_dbm);
    assert_eq!(
        (after.freq_khz, after.sf, after.cr),
        (before.freq_khz, before.sf, before.cr)
    );
    assert_eq!(after.telemetry, before.telemetry);
    // The radio away: "connect your radio first".
    sim.set_radio(false);
    assert_eq!(
        sim.core.set_tx_power(before.tx_dbm),
        Err(LichenError::NoIdentity)
    );
    assert_eq!(sim.core.set_gps(true), Err(LichenError::NoIdentity));
    // The settings shown are the last read while it is away.
    assert_eq!(sim.core.radio_settings().name, "ThinkNode M5");
}

/// RL-7: the radio card's board and figures: the demo radio is a ThinkNode M5 on stock v1.18
/// (`DEVICE_INFO`), and reports its noise floor, airtime, uptime and packets (`GET_STATS`,
/// §16.4) at connection, then again when asked, at most every 60 s.
#[test]
fn rl_7_the_radio_card_s_board_and_figures() {
    let mut sim = Sim::new(78);
    connected(&mut sim);
    // The three types come one after the other: packets last.
    sim.until("the radio's figures", 10, |c| {
        c.radio_settings().packets_sent.is_some()
    });
    let s = sim.core.radio_settings();
    assert_eq!(s.manufacturer, "Elecrow ThinkNode M5", "{s:?}");
    assert_eq!(s.firmware, "v1.18.0");
    assert_eq!(s.build_date, "15 Oct 2026");
    assert_eq!(s.noise_floor_dbm, Some(-118));
    assert!(
        s.tx_airtime_s.is_some() && s.rx_airtime_s.is_some(),
        "{s:?}"
    );
    assert!(
        s.packets_received.is_some() && s.packets_sent.is_some(),
        "{s:?}"
    );
    assert_eq!(s.receive_errors, Some(0));
    let up = s.uptime_s.expect("uptime");
    assert!(
        !sim.core.refresh_radio_stats(sim.now),
        "read at connection: not again before 60 s"
    );
    sim.run_for(61_000);
    assert!(sim.core.refresh_radio_stats(sim.now));
    sim.run_for(2_000);
    let again = sim.core.radio_settings();
    assert!(again.uptime_s.is_some_and(|u| u >= up + 60), "{again:?}");
    assert!(again.packets_received >= s.packets_received);
}

/// RL-7: the battery is read at connection and again on request; with radio details on, a
/// message shows its fragments, airtime and signal.
#[test]
fn rl_7_battery_and_radio_details() {
    let mut sim = Sim::new(77);
    connected(&mut sim);
    sim.until("the battery", 10, |c| {
        c.radio_settings().battery_mv.is_some()
    });
    let s = sim.core.radio_settings();
    assert_eq!(s.battery_mv, Some(4_012));
    assert_eq!(
        s.battery_percent,
        Some(lc_core::radio::battery_percent(4_012)),
        "{s:?}"
    );
    assert!(!s.battery_low);
    assert_eq!(
        (s.storage_used_kb, s.storage_total_kb),
        (Some(96), Some(1_024))
    );
    // Read again when the radio card opens.
    let from = sim.events.len();
    sim.core.refresh_radio();
    sim.run_for(3_000);
    assert!(
        sim.since(from)
            .any(|e| matches!(e, LichenEvent::RadioSettings))
    );
    // A text both ways: what the details show.
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let m = sim
        .core
        .send_text(camille.key.clone(), "Details".into(), sim.unix_s(), sim.now)
        .unwrap();
    let sent = sim.until_status(&chat, m.id, 120, |s| *s == MessageStatus::Delivered);
    assert!(sent.fragments >= 1, "{sent:?}");
    assert!(sent.airtime_ms > 0, "{sent:?}");
    let got = sim.until_some("Camille's hello", 60, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| !m.outgoing)
    });
    assert!(got.snr_db.is_some(), "{got:?}");
    let b = sim.core.budget(sim.now);
    assert!(
        b.used_ms >= sent.airtime_ms && b.limit_ms > b.used_ms,
        "{b:?}"
    );
}

/// Pumps the app's core against companion `app` of a small network, from `from` to `to`
/// (simulated ms), as `Sim` pumps the demo; the events polled.
fn pump(
    net: &mut lc_core::fakenet::Net,
    app: usize,
    core: &lc_ffi::LichenCore,
    wall0: u64,
    from: u64,
    to: u64,
) -> Vec<LichenEvent> {
    let mut events = Vec::new();
    let mut now = from;
    while now < to {
        if now > net.now {
            net.run_until(now, |_| false);
        }
        for f in net.external_read(app) {
            core.on_companion_frame(f, now);
        }
        core.set_clock(wall0 + now, now);
        core.tick(now);
        while let Some(f) = core.poll_companion(now) {
            net.external_write(app, &f);
        }
        while let Some(e) = core.poll_event() {
            events.push(e);
        }
        now += common::STEP_MS;
    }
    events
}

/// RL-6 (B-ios-10): a setting the core lets through but the radio refuses (an interval for a
/// GPS the board does not have): the app is told which one, once, and the value stays the
/// radio's; a setting it takes is saved (B-user-5).
#[test]
fn rl_6_a_setting_the_radio_refuses() {
    let radio = lc_meshcore::airtime::LoRaParams::meshcore(8, 62_500, 5).unwrap();
    let mut net = lc_core::fakenet::Net::new(radio, 0, 9);
    let app = net.add_external_companion(1, "Heltec V4");
    let core = common::new_core(78, 178, None);
    let wall0 = common::unix_now_ms();
    core.set_clock(wall0, 0);
    core.connect(0);
    pump(&mut net, app, &core, wall0, 0, 5_000);
    let before = core.radio_settings();
    assert_eq!(before.gps, None, "no GPS on this board");
    assert!(core.take_radio_outcomes().is_empty());
    core.set_gps_interval(300).unwrap();
    let refused = lc_ffi::RadioOutcome {
        setting: lc_ffi::RadioSetting::GpsInterval,
        outcome: lc_ffi::SettingOutcome::Refused,
    };
    let events = pump(&mut net, app, &core, wall0, 5_000, 10_000);
    assert!(events.contains(&LichenEvent::RadioOutcome { outcome: refused }));
    assert_eq!(core.take_radio_outcomes(), [refused]);
    assert!(core.take_radio_outcomes().is_empty(), "told once");
    assert_eq!(core.radio_settings().gps_interval_s, None);
    // Taken: saved.
    core.set_tx_power(before.max_tx_dbm - 1).unwrap();
    pump(&mut net, app, &core, wall0, 10_000, 15_000);
    assert_eq!(
        core.take_radio_outcomes(),
        [lc_ffi::RadioOutcome {
            setting: lc_ffi::RadioSetting::TxPower,
            outcome: lc_ffi::SettingOutcome::Saved,
        }]
    );
    assert_eq!(core.radio_settings().tx_dbm, before.max_tx_dbm - 1);
}

/// RL-5 (B-ios-15): a radio slow to answer (its replies held 7 s, past the command timeout):
/// until it has, the settings say they are not read yet instead of zeros; its late SELF_INFO
/// does not leave the app half connected: APP_START goes again, and the values come.
#[test]
fn rl_5_a_slow_radio_s_settings() {
    let radio = lc_meshcore::airtime::LoRaParams::meshcore(8, 62_500, 5).unwrap();
    let mut net = lc_core::fakenet::Net::new(radio, 0, 11);
    let app = net.add_external_companion(1, "ThinkNode M5");
    let core = common::new_core(79, 179, None);
    let wall0 = common::unix_now_ms();
    core.set_clock(wall0, 0);
    assert!(!core.radio_settings().read, "nothing read before a radio");
    core.connect(0);
    // The radio's answers wait 7 s: the app sends, nothing comes back.
    let mut now = 0;
    while now < 7_000 {
        net.run_until(now, |_| false);
        core.set_clock(wall0 + now, now);
        core.tick(now);
        while let Some(f) = core.poll_companion(now) {
            net.external_write(app, &f);
        }
        while core.poll_event().is_some() {}
        now += common::STEP_MS;
    }
    let s = core.radio_settings();
    assert!(!s.read, "not read yet: {s:?}");
    // They come, late, then the radio answers in time.
    let events = pump(&mut net, app, &core, wall0, now, now + 30_000);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, LichenEvent::Connected { .. })),
        "connected: {events:?}"
    );
    assert!(core.node_key().is_some());
    let s = core.radio_settings();
    assert!(s.read);
    assert_eq!((s.sf, s.bw_hz, s.cr), (8, 62_500, 5));
    assert!(s.freq_khz > 0 && s.max_tx_dbm > 0, "{s:?}");
}

/// RL-11 (B-user-5): every setting says what became of it. Pending while it waits; saved
/// once the radio reports the value asked for; not kept when the radio answers OK and keeps
/// its old value (`radioIgnores preferences`), the value shown staying the radio's; refused
/// on ERR (`radioRefuses power`). An outcome comes as an event and in `take_radio_outcomes`,
/// once.
#[test]
fn rl_11_each_setting_says_what_happened() {
    use lc_ffi::{RadioOutcome, RadioSetting, SettingOutcome};
    let demo = lc_ffi::DemoNetwork::new();
    demo.ignore_settings("preferences".into());
    demo.refuse_tx_power();
    let mut sim = Sim::on(111, demo, false);
    connected(&mut sim);
    sim.until("the radio's settings", 10, |c| {
        c.radio_settings().autoadd_mask.is_some()
    });
    let before = sim.core.radio_settings();
    let outcome = |setting, outcome| RadioOutcome { setting, outcome };
    let wait_outcome = |sim: &mut Sim, what: &str| {
        sim.until_event(what, 10, |e| match e {
            LichenEvent::RadioOutcome { outcome } => Some(*outcome),
            _ => None,
        })
    };

    // Saved: the auto-add, as asked.
    sim.core.set_autoadd(0x06, 2).unwrap();
    assert_eq!(sim.core.radio_pending(), [RadioSetting::Autoadd]);
    assert_eq!(
        wait_outcome(&mut sim, "the auto-add's outcome"),
        outcome(RadioSetting::Autoadd, SettingOutcome::Saved)
    );
    assert!(sim.core.radio_pending().is_empty());
    assert_eq!(sim.core.radio_settings().autoadd_mask, Some(0x06));

    // Not kept: telemetry on, answered OK, the radio's still off.
    sim.core
        .set_radio_preferences(
            before.manual_add,
            Telemetry {
                base: 2,
                location: 0,
                environment: 0,
            },
            before.position_in_adverts,
            before.multi_acks,
        )
        .unwrap();
    assert_eq!(
        wait_outcome(&mut sim, "the telemetry's outcome"),
        outcome(RadioSetting::Preferences, SettingOutcome::NotKept)
    );
    assert_eq!(sim.core.radio_settings().telemetry, before.telemetry);

    // Refused: the power.
    sim.core.set_tx_power(before.tx_dbm - 1).unwrap();
    assert_eq!(
        wait_outcome(&mut sim, "the power's outcome"),
        outcome(RadioSetting::TxPower, SettingOutcome::Refused)
    );
    assert_eq!(sim.core.radio_settings().tx_dbm, before.tx_dbm);
    // Each once, in order, in the list too.
    assert_eq!(
        sim.core.take_radio_outcomes(),
        [
            outcome(RadioSetting::Autoadd, SettingOutcome::Saved),
            outcome(RadioSetting::Preferences, SettingOutcome::NotKept),
            outcome(RadioSetting::TxPower, SettingOutcome::Refused),
        ]
    );
    assert!(sim.core.take_radio_outcomes().is_empty());
}
