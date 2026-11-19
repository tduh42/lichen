//! Location, map, Locate and SOS (docs/testing.md §8: MP-1, MP-2, MP-3, MP-5, MP-6). MP-4
//! (offline maps) is the apps' own: nothing of it goes through the core.

mod common;

use common::{Sim, contact_chat, find_contact};
use lc_ffi::{ChatRef, DemoNetwork, LichenError, LichenEvent, NodeKind, SharePrecision};

/// Metres between two points (an equirectangular approximation, enough at these distances).
fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let k = 111_320.0;
    let dy = (a.0 - b.0) * k;
    let dx = (a.1 - b.1) * k * a.0.to_radians().cos();
    dx.hypot(dy)
}

/// Where Camille waits in the demo.
const REFUGE: (f64, f64) = (44.8960, 5.4880);
/// Where Julien, hurt, waits without GPS in the SOS scenario.
const JULIEN_HURT: (f64, f64) = (44.9055, 5.4750);

/// MP-1: sharing our position with Camille: the precision chosen, replaced by another, ended
/// by Stop and on its own at its end; nothing goes without a fix.
#[test]
fn mp_1_share_my_position_with_a_contact() {
    let mut sim = Sim::new(151);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let in_an_hour = u64::from(sim.unix_s()) + 3_600;
    sim.core
        .start_share(chat.clone(), SharePrecision::M100, in_an_hour, sim.now)
        .unwrap();
    let s = sim.core.shares();
    assert_eq!(s.len(), 1);
    assert_eq!(
        (s[0].chat.clone(), s[0].precision, s[0].until_s),
        (chat.clone(), SharePrecision::M100, in_an_hour)
    );
    // Without a fix nothing goes; with one, its positions spend airtime.
    sim.run_for(20_000);
    let before = sim.core.budget(sim.now).used_ms;
    sim.gps = true;
    sim.run_for(60_000);
    assert!(sim.core.budget(sim.now).used_ms > before, "positions went");
    // Another precision: the same share, replaced.
    sim.core
        .start_share(chat.clone(), SharePrecision::Exact, in_an_hour, sim.now)
        .unwrap();
    let s = sim.core.shares();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].precision, SharePrecision::Exact);
    // Stop.
    sim.core.stop_share(chat.clone(), sim.now).unwrap();
    assert!(sim.core.shares().is_empty());
    // A share that ends on its own, a minute from now.
    let soon = u64::from(sim.unix_s()) + 60;
    sim.core
        .start_share(chat.clone(), SharePrecision::Km1, soon, sim.now)
        .unwrap();
    sim.until_event("the share's end", 120, |e| {
        matches!(e, LichenEvent::SharesChanged).then_some(())
    });
    assert!(sim.core.shares().is_empty());
    // An end already past: over at once.
    sim.core
        .start_share(
            chat,
            SharePrecision::Exact,
            u64::from(sim.unix_s()) - 10,
            sim.now,
        )
        .unwrap();
    sim.run_for(1_000);
    assert!(sim.core.shares().is_empty());
    // An unknown chat.
    assert!(
        sim.core
            .start_share(
                ChatRef::Contact { key: vec![3; 32] },
                SharePrecision::Exact,
                in_an_hour,
                sim.now
            )
            .is_err()
    );
}

/// MP-2: Camille's exact position, directly; Julien's through the group, a 1 km cell's centre
/// that does not move with him inside it; one marker each.
#[test]
fn mp_2_positions_received_on_the_map() {
    let mut sim = Sim::new(152);
    sim.gps = true;
    let camille = sim.camille();
    let julien = sim.contact("Julien");
    let from = sim.events.len();
    sim.until("both on the map", 900, |c| {
        let p = c.positions(0);
        p.iter().any(|x| x.from == camille.key) && p.iter().any(|x| x.from == julien.key)
    });
    sim.run_for(5 * 60_000);
    let map = sim.core.positions(sim.now);
    assert_eq!(map.len(), 2, "{map:?}");
    let c = map.iter().find(|p| p.from == camille.key).unwrap();
    assert!(c.group.is_none());
    assert!(c.radius_m < 10.0, "{c:?}");
    assert!(distance_m((c.lat, c.lon), REFUGE) < 10.0, "{c:?}");
    let j = map.iter().find(|p| p.from == julien.key).unwrap();
    assert!(j.group.is_some());
    assert!(j.radius_m > 700.0, "{j:?}");
    assert!(!j.extrapolated, "a cell is not moved along his way: {j:?}");
    // A coarse position says nothing of his heading, speed or height (§12.2).
    assert!(
        j.heading_deg.is_none() && j.speed_mps.is_none() && j.alt_m.is_none(),
        "{j:?}"
    );
    // He walks on: each position is a cell's centre, the same until the cell changes, never
    // noise around his track.
    sim.run_for(20 * 60_000);
    let his: Vec<(f64, f64)> = sim
        .since(from)
        .filter_map(|e| match e {
            LichenEvent::Location {
                position,
                end: false,
            } if position.from == julien.key && !position.sos => Some((position.lat, position.lon)),
            _ => None,
        })
        .collect();
    assert!(his.len() >= 2, "{his:?}");
    for w in his.windows(2) {
        let d = distance_m(w[0], w[1]);
        assert!(
            d == 0.0 || d > 500.0,
            "{d} m between two of his positions: {his:?}"
        );
    }
}

/// MP-3: the nodes layer: the repeaters and the room where they advertise; people without a
/// position in their adverts stay off it.
#[test]
fn mp_3_nodes_layer() {
    let mut sim = Sim::new(153);
    sim.until("the radio's nodes", 10, |c| c.nodes().len() >= 5);
    let placed: Vec<_> = sim
        .core
        .nodes()
        .into_iter()
        .filter(|n| n.lat.is_some() && n.lon.is_some())
        .collect();
    let mut names: Vec<&str> = placed.iter().map(|n| n.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Refuge board", "crest", "hub"]);
    for n in &placed {
        assert!(matches!(n.kind, NodeKind::Repeater | NodeKind::RoomServer));
        let (lat, lon) = (n.lat.unwrap(), n.lon.unwrap());
        assert!(distance_m((lat, lon), REFUGE) < 5_000.0, "{n:?}");
    }
    assert!(
        sim.core
            .nodes()
            .iter()
            .filter(|n| n.kind == NodeKind::Companion)
            .all(|n| n.lat.is_none())
    );
}

/// MP-5: SOS: ours goes to the group with our fix and stops; Julien's (no GPS) and Camille's
/// (with GPS, five minutes) arrive as beacons and markers, and Camille's ends.
#[test]
fn mp_5_sos_send_receive_stop() {
    let demo = DemoNetwork::new();
    demo.julien_sos();
    demo.camille_sos(240, 300);
    let mut sim = Sim::on(155, demo, false);
    sim.gps = true;
    let camille = sim.camille();
    sim.core
        .send_text(
            camille.key.clone(),
            "On my way".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let group = sim.rando();
    // Ours.
    sim.core.set_sos(true, sim.now);
    assert!(sim.core.sos());
    sim.run_for(15_000);
    let st = sim.core.sos_status(sim.now);
    assert!(st.on && st.fix_fresh && !st.beacon_only, "{st:?}");
    assert!(st.recipients >= 1, "{st:?}");
    assert!(st.last_queued_s.is_some(), "{st:?}");
    sim.core.set_sos(false, sim.now);
    assert!(!sim.core.sos() && !sim.core.sos_status(sim.now).on);
    // Julien's, without a position.
    let julien = find_contact(&sim.core, "Julien").unwrap();
    let battery = sim.until_event("Julien's SOS", 900, |e| match e {
        LichenEvent::Beacon {
            gid,
            from,
            sos: true,
            battery,
        } if *gid == group.gid && *from == julien.key => Some(*battery),
        _ => None,
    });
    assert_eq!(battery, Some(64));
    // Camille's, with her position, then its end.
    sim.until("Camille in SOS on the map", 600, |c| {
        c.positions(0)
            .iter()
            .any(|p| p.from == camille.key && p.sos)
    });
    // Her SOS ends at 540 s: her group gets the end flag, her direct share goes on as
    // before, and her marker is no longer an SOS (§12.2).
    sim.run_to(560_000);
    sim.until("Camille's SOS over on the map", 900, |c| {
        let p = c.positions(0);
        p.iter().any(|p| p.from == camille.key) && !p.iter().any(|p| p.from == camille.key && p.sos)
    });
}

/// MP-6: Julien in SOS without GPS: we search for him, ping him, Camille's measurements join
/// ours, and the estimate holds where he is.
#[test]
fn mp_6_locate_a_member_without_gps() {
    let demo = DemoNetwork::new();
    demo.julien_sos();
    let mut sim = Sim::on(156, demo, false);
    sim.gps = true;
    let camille = sim.camille();
    sim.core
        .send_text(
            camille.key.clone(),
            "On my way".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let (gid, julien) = sim.until_event("Julien's SOS", 900, |e| match e {
        LichenEvent::Beacon {
            gid,
            from,
            sos: true,
            ..
        } => Some((gid.clone(), from.clone())),
        _ => None,
    });
    assert_eq!(
        sim.core.search_ping(julien.clone(), sim.now),
        Err(LichenError::NotSearching)
    );
    sim.core
        .start_search(gid.clone(), julien.clone(), sim.now)
        .unwrap();
    let end = sim.now + 10 * 60_000;
    while sim.now < end {
        if sim.now.is_multiple_of(30_000) {
            let _ = sim.core.search_ping(julien.clone(), sim.now);
        }
        sim.step();
    }
    let ms = sim.core.search_measurements(julien.clone());
    assert!(ms.iter().any(|m| m.mine), "ours");
    assert!(ms.iter().any(|m| !m.mine), "Camille's");
    let e = sim
        .core
        .search_estimate(julien.clone())
        .expect("an estimate");
    let err = distance_m((e.lat, e.lon), JULIEN_HURT);
    assert!(
        err < e.major95_m.max(800.0),
        "{err} m off, 95 % {}",
        e.major95_m
    );
    assert_eq!(
        usize::from(e.heat_cols) * usize::from(e.heat_rows),
        e.heat.len()
    );
    let st = sim.core.searches(sim.now);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].battery, Some(64));
    sim.core.stop_search(julien.clone()).unwrap();
    assert!(sim.core.searches(sim.now).is_empty());
}
