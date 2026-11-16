//! The search solver on what group members can send (review of 2026-11-15, SR-37): positions
//! anywhere on Earth (the i24 wire range), any RSSI and TX power, bearings, fixes of any
//! precision, at any times. Nothing panics, and an estimate is always a place on a map: finite,
//! latitude within ±90°, longitude within ±180°, ellipses and heat grid well formed.
#![no_main]
use lc_core::solver::{Kind, Observation, Params, PathLoss, calibrate, solve};
use libfuzzer_sys::fuzz_target;

/// Units per 90° of latitude (and per 180° of longitude) on the wire (§12.1).
const UNITS: f64 = (1 << 23) as f64;
/// Measurements and fixes a track keeps at most (`search.rs`).
const OBS_MAX: usize = 500;
/// Bearing classes' spreads (`search::bearing_sigma`).
const BEARING_SIGMA: [f64; 3] = [15.0, 40.0, 60.0];

fn i24(b: [u8; 3]) -> f64 {
    f64::from(i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 0x80 != 0 { 0xFF } else { 0 }]))
}

fuzz_target!(|data: &[u8]| {
    let Some((&head, rest)) = data.split_first() else {
        return;
    };
    let mut obs = Vec::new();
    let mut pairs = Vec::new();
    let mut t = 1_793_000_000.0;
    // 12 bytes per observation: kind, lat (3), lon (3), Δt (2), then 3 bytes by kind.
    for c in rest.chunks_exact(12).take(OBS_MAX) {
        let lat = i24([c[1], c[2], c[3]]) * 90.0 / UNITS;
        let lon = i24([c[4], c[5], c[6]]) * 180.0 / UNITS;
        t += f64::from(u16::from_le_bytes([c[7], c[8]]));
        let kind = match c[0] % 3 {
            0 => {
                let (rssi, tx) = (c[9] as i8, c[10] as i8);
                let estimated = if c[11] & 1 != 0 { 3.0 } else { 0.0 };
                let unknown_tx = if c[11] & 2 != 0 { 4.0 } else { 0.0 };
                pairs.push((f64::from(u16::from_le_bytes([c[9], c[10]])), f64::from(rssi) - f64::from(tx)));
                Kind::Rssi {
                    rssi: f64::from(rssi),
                    tx_dbm: f64::from(tx),
                    extra_sigma_db: f64::hypot(estimated, unknown_tx),
                    lower_bound: c[11] & 4 != 0,
                }
            }
            1 => Kind::Bearing {
                deg: f64::from(u16::from_le_bytes([c[9], c[10]]) % 360),
                sigma_deg: BEARING_SIGMA[usize::from(c[11] % 3)],
            },
            _ => Kind::Fix {
                radius_m: (f64::from(c[9]) / 16.0).exp2(),
            },
        };
        obs.push(Observation { t, lat, lon, kind });
    }
    // The path loss fitted from (distance, RSSI − TX) pairs, as the track does from fixes.
    let model = calibrate(&pairs, PathLoss::DEFAULT).unwrap_or(PathLoss::DEFAULT);
    for v in [model.a_db, model.n, model.sigma_db] {
        assert!(v.is_finite(), "{model:?}");
    }
    let mut p = Params::new(model, u64::from(head));
    p.particles = 64 + usize::from(head % 4) * 64;
    let Some(e) = solve(&obs, &p) else {
        assert!(obs.is_empty());
        return;
    };
    assert!(e.lat.is_finite() && (-90.0..=90.0).contains(&e.lat), "{e:?}");
    assert!(e.lon.is_finite() && (-180.0..=180.0).contains(&e.lon), "{e:?}");
    for a in [e.major68_m, e.minor68_m, e.major95_m, e.minor95_m] {
        assert!(a.is_finite() && a >= 0.0, "{e:?}");
    }
    assert!(e.major68_m >= e.minor68_m && e.major95_m >= e.minor95_m, "{e:?}");
    assert!(e.azimuth_deg.is_finite() && (0.0..180.0).contains(&e.azimuth_deg), "{e:?}");
    let h = &e.heat;
    assert_eq!(h.cells.len(), usize::from(h.cols) * usize::from(h.rows));
    for v in [h.north, h.south, h.west, h.east] {
        assert!(v.is_finite(), "{h:?}");
    }
    assert!(h.south <= h.north && (-90.0..=90.0).contains(&h.south) && (-90.0..=90.0).contains(&h.north), "{h:?}");
});
