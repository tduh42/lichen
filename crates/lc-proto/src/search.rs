//! Search messages (§11.1), the body of a SEARCH datagram (type 0x07, group envelopes only):
//! PING, BEACON, MEASURE, POWER. Integer units only; the solver is the host's (`lc-core`).

use crate::location::{self, Position};

/// `kind` byte values.
pub const PING: u8 = 0;
pub const BEACON: u8 = 1;
pub const MEASURE: u8 = 2;
pub const POWER: u8 = 3;

/// Longest body (a MEASURE with a bearing, or a BEACON with a position), `kind` included.
pub const MAX_LEN: usize = 20;
/// Battery level not known.
pub const BATTERY_UNKNOWN: u8 = 255;
/// Noise floor not known.
pub const NOISE_UNKNOWN: i8 = i8::MIN;
/// POWER lasts 1 to 30 minutes.
pub const POWER_MINUTES_MAX: u8 = 30;

const BEACON_POSITION: u8 = 1;
const BEACON_SOS: u8 = 2;
const MEASURE_ESTIMATED: u8 = 1;
const MEASURE_SATURATED: u8 = 2;
const MEASURE_BEARING: u8 = 4;

/// How a bearing was taken, and so how much it is trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BearingClass {
    /// A directional antenna: ±15°.
    Yagi,
    /// The signal's minimum behind the body, turned around: ±40°.
    Body,
    /// Anything else: ±60°.
    Other,
}

impl BearingClass {
    const fn code(self) -> u16 {
        match self {
            BearingClass::Yagi => 0,
            BearingClass::Body => 1,
            BearingClass::Other => 2,
        }
    }

    const fn from_code(c: u16) -> Option<Self> {
        match c {
            0 => Some(BearingClass::Yagi),
            1 => Some(BearingClass::Body),
            2 => Some(BearingClass::Other),
            _ => None,
        }
    }
}

/// A bearing to the target from the searcher, degrees from true north.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bearing {
    /// 0..=359.
    pub deg: u16,
    pub class: BearingClass,
}

/// What a searcher heard of the target, where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measure {
    /// First 4 bytes of the target's identity key.
    pub target: [u8; 4],
    /// Time of reception, Unix seconds mod 65,536 (§12.1).
    pub t16: u16,
    /// The searcher's position, §12.1 units.
    pub lat: i32,
    pub lon: i32,
    pub prec: u8,
    /// dBm.
    pub rssi: i8,
    /// SNR × 4.
    pub snr4: i8,
    /// Noise floor, dBm, or [`NOISE_UNKNOWN`].
    pub noise: i8,
    /// RSSI estimated as noise + SNR (the radio reported none).
    pub estimated: bool,
    /// Estimated at an SNR where LoRa saturates: the true RSSI is higher.
    pub saturated: bool,
    pub bearing: Option<Bearing>,
}

/// A SEARCH datagram body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Search {
    /// Asks `target` to transmit now.
    Ping {
        target: [u8; 4],
        id: u16,
    },
    /// The target's answer (`id` 0 when unsolicited), with its TX power for ranging.
    Beacon {
        id: u16,
        tx_dbm: i8,
        /// 0..=100 %, or [`BATTERY_UNKNOWN`].
        battery: u8,
        /// The target is in SOS (with or without a position).
        sos: bool,
        position: Option<Position>,
    },
    Measure(Measure),
    /// Asks `target` to lower its TX power for `minutes` (close range).
    Power {
        target: [u8; 4],
        dbm: i8,
        minutes: u8,
    },
}

impl Search {
    /// Writes `kind ‖ body` into `out`; returns its length.
    pub fn encode(&self, out: &mut [u8; MAX_LEN]) -> usize {
        let mut w = Writer { out, len: 0 };
        match self {
            Search::Ping { target, id } => {
                w.put(&[PING]);
                w.put(target);
                w.put(&id.to_le_bytes());
            }
            Search::Beacon {
                id,
                tx_dbm,
                battery,
                sos,
                position,
            } => {
                w.put(&[BEACON]);
                w.put(&id.to_le_bytes());
                w.put(&[*tx_dbm as u8, *battery]);
                let mut flags = 0;
                if position.is_some() {
                    flags |= BEACON_POSITION;
                }
                if *sos {
                    flags |= BEACON_SOS;
                }
                w.put(&[flags]);
                if let Some(p) = position {
                    w.put(&p.encode());
                }
            }
            Search::Measure(m) => {
                w.put(&[MEASURE]);
                w.put(&m.target);
                w.put(&m.t16.to_le_bytes());
                let lat = m
                    .lat
                    .clamp(location::I24_MIN, location::I24_MAX)
                    .to_le_bytes();
                let lon = m
                    .lon
                    .clamp(location::I24_MIN, location::I24_MAX)
                    .to_le_bytes();
                w.put(&[lat[0], lat[1], lat[2], lon[0], lon[1], lon[2], m.prec]);
                w.put(&[m.rssi as u8, m.snr4 as u8, m.noise as u8]);
                let mut flags = 0;
                if m.estimated {
                    flags |= MEASURE_ESTIMATED;
                }
                if m.saturated {
                    flags |= MEASURE_SATURATED;
                }
                if m.bearing.is_some() {
                    flags |= MEASURE_BEARING;
                }
                w.put(&[flags]);
                if let Some(b) = m.bearing {
                    let v = (b.deg % 360) | (b.class.code() << 9);
                    w.put(&v.to_le_bytes());
                }
            }
            Search::Power {
                target,
                dbm,
                minutes,
            } => {
                w.put(&[POWER]);
                w.put(target);
                w.put(&[*dbm as u8, (*minutes).clamp(1, POWER_MINUTES_MAX)]);
            }
        }
        w.len
    }

    /// Parses `kind ‖ body`, exact lengths only.
    pub fn decode(b: &[u8]) -> Option<Self> {
        let (&kind, body) = b.split_first()?;
        match kind {
            PING => {
                let [t0, t1, t2, t3, i0, i1] = *<&[u8; 6]>::try_from(body).ok()?;
                Some(Search::Ping {
                    target: [t0, t1, t2, t3],
                    id: u16::from_le_bytes([i0, i1]),
                })
            }
            BEACON => {
                let (head, rest) = body.split_first_chunk::<5>()?;
                let [i0, i1, tx, battery, flags] = *head;
                let position = match (flags & BEACON_POSITION != 0, rest.is_empty()) {
                    (true, false) => Some(Position::decode(rest)?),
                    (false, true) => None,
                    _ => return None,
                };
                Some(Search::Beacon {
                    id: u16::from_le_bytes([i0, i1]),
                    tx_dbm: tx as i8,
                    battery,
                    sos: flags & BEACON_SOS != 0,
                    position,
                })
            }
            MEASURE => {
                let (head, rest) = body.split_first_chunk::<17>()?;
                let [
                    t0,
                    t1,
                    t2,
                    t3,
                    s0,
                    s1,
                    la0,
                    la1,
                    la2,
                    lo0,
                    lo1,
                    lo2,
                    prec,
                    rssi,
                    snr4,
                    noise,
                    flags,
                ] = *head;
                let bearing = match (flags & MEASURE_BEARING != 0, rest) {
                    (true, &[b0, b1]) => {
                        let v = u16::from_le_bytes([b0, b1]);
                        let deg = v & 0x1FF;
                        if deg >= 360 {
                            return None;
                        }
                        Some(Bearing {
                            deg,
                            class: BearingClass::from_code(v >> 9)?,
                        })
                    }
                    (false, &[]) => None,
                    _ => return None,
                };
                Some(Search::Measure(Measure {
                    target: [t0, t1, t2, t3],
                    t16: u16::from_le_bytes([s0, s1]),
                    lat: i32::from_le_bytes([0, la0, la1, la2]) >> 8,
                    lon: i32::from_le_bytes([0, lo0, lo1, lo2]) >> 8,
                    prec,
                    rssi: rssi as i8,
                    snr4: snr4 as i8,
                    noise: noise as i8,
                    estimated: flags & MEASURE_ESTIMATED != 0,
                    saturated: flags & MEASURE_SATURATED != 0,
                    bearing,
                }))
            }
            POWER => {
                let [t0, t1, t2, t3, dbm, minutes] = *<&[u8; 6]>::try_from(body).ok()?;
                if minutes == 0 || minutes > POWER_MINUTES_MAX {
                    return None;
                }
                Some(Search::Power {
                    target: [t0, t1, t2, t3],
                    dbm: dbm as i8,
                    minutes,
                })
            }
            _ => None,
        }
    }
}

struct Writer<'a> {
    out: &'a mut [u8; MAX_LEN],
    len: usize,
}

impl Writer<'_> {
    fn put(&mut self, b: &[u8]) {
        if let Some(dst) = self.out.get_mut(self.len..self.len + b.len()) {
            dst.copy_from_slice(b);
            self.len += b.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn round(m: Search) -> usize {
        let mut out = [0u8; MAX_LEN];
        let n = m.encode(&mut out);
        assert_eq!(Search::decode(&out[..n]), Some(m), "{m:?}");
        n
    }

    fn position() -> Position {
        Position {
            lat: 5_418_000,
            lon: 248_000,
            alt: 1_620,
            t16: 0x1234,
            heading: location::HEADING_UNKNOWN,
            speed: location::SPEED_UNKNOWN,
            prec: 40,
            flags: location::FLAG_SOS,
        }
    }

    #[test]
    fn sizes_and_round_trips() {
        let target = [1, 2, 3, 4];
        assert_eq!(round(Search::Ping { target, id: 0xBEEF }), 7);
        let beacon = |sos, position| Search::Beacon {
            id: 7,
            tx_dbm: 22,
            battery: 81,
            sos,
            position,
        };
        assert_eq!(round(beacon(false, None)), 6);
        assert_eq!(round(beacon(true, None)), 6);
        assert_eq!(round(beacon(true, Some(position()))), 20);
        // Unknown flag bits are ignored; the SOS bit is bit 1.
        assert_eq!(
            Search::decode(&[BEACON, 7, 0, 22, 81, 0x82]),
            Some(beacon(true, None))
        );
        let m = Measure {
            target,
            t16: 0xABCD,
            lat: -1_000_000,
            lon: 4_000_000,
            prec: 30,
            rssi: -97,
            snr4: -22,
            noise: -118,
            estimated: true,
            saturated: false,
            bearing: None,
        };
        assert_eq!(round(Search::Measure(m)), 18);
        let with = Measure {
            bearing: Some(Bearing {
                deg: 317,
                class: BearingClass::Body,
            }),
            saturated: true,
            ..m
        };
        assert_eq!(round(Search::Measure(with)), 20);
        assert_eq!(
            round(Search::Power {
                target,
                dbm: -9,
                minutes: 10
            }),
            7
        );
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(Search::decode(&[]), None);
        assert_eq!(Search::decode(&[9, 0, 0]), None, "unknown kind");
        assert_eq!(Search::decode(&[PING, 1, 2, 3, 4, 5]), None, "short ping");
        assert_eq!(
            Search::decode(&[POWER, 1, 2, 3, 4, 0, 0]),
            None,
            "0 minutes"
        );
        assert_eq!(
            Search::decode(&[POWER, 1, 2, 3, 4, 0, 31]),
            None,
            "31 minutes"
        );
        // A beacon flagged with a position but without one, and the reverse.
        assert_eq!(Search::decode(&[BEACON, 0, 0, 22, 50, 1]), None);
        assert_eq!(Search::decode(&[BEACON, 0, 0, 22, 50, 0, 1]), None);
        // A measure whose bearing flag and length disagree, or whose bearing is out of range.
        let mut m = [0u8; 18];
        m[0] = MEASURE;
        assert!(Search::decode(&m).is_some());
        m[17] = MEASURE_BEARING;
        assert_eq!(Search::decode(&m), None);
        let mut b = [0u8; 20];
        b[0] = MEASURE;
        b[17] = MEASURE_BEARING;
        b[18..20].copy_from_slice(&360u16.to_le_bytes());
        assert_eq!(Search::decode(&b), None);
        b[18..20].copy_from_slice(&(359u16 | 3 << 9).to_le_bytes());
        assert_eq!(Search::decode(&b), None, "unknown bearing class");
    }

    proptest! {
        #[test]
        fn decode_never_panics(b in proptest::collection::vec(any::<u8>(), 0..40)) {
            // Unknown flag bits are dropped: decode, encode, decode is stable.
            if let Some(m) = Search::decode(&b) {
                let mut out = [0u8; MAX_LEN];
                let n = m.encode(&mut out);
                prop_assert_eq!(Search::decode(&out[..n]), Some(m));
            }
        }
    }
}
