//! Location body (§12.1): a 14-byte position, in integer units.
//!
//! Degrees, metres and grids are the host's business (`lc-core`); this module only packs and
//! unpacks the bytes and rebuilds the full time from its low 16 bits.

/// Body length (after the content type byte).
pub const LEN: usize = 14;

/// `lat` units per 90° and `lon` units per 180°: 2^23.
pub const UNITS: i32 = 1 << 23;
/// Smallest and largest i24 values.
pub const I24_MIN: i32 = -(1 << 23);
pub const I24_MAX: i32 = (1 << 23) - 1;

pub const ALT_UNKNOWN: i16 = i16::MIN;
pub const HEADING_UNKNOWN: u16 = 511;
pub const SPEED_UNKNOWN: u8 = 127;

/// The position is an SOS (§12.2).
pub const FLAG_SOS: u8 = 1;
/// Last position of a share: the receiver removes the marker.
pub const FLAG_END: u8 = 2;

/// How far back a 16-bit time may point: 18 hours.
pub const PAST_S: u64 = 18 * 3600;
/// How far ahead (clock skew): 10 minutes.
pub const FUTURE_S: u64 = 600;

/// A position as carried on the air.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// Latitude, `UNITS` per 90°, within the i24 range.
    pub lat: i32,
    /// Longitude, `UNITS` per 180°, within the i24 range.
    pub lon: i32,
    /// Altitude in metres, or [`ALT_UNKNOWN`].
    pub alt: i16,
    /// Sender's Unix seconds, mod 65,536.
    pub t16: u16,
    /// Heading in degrees (0..=359), or [`HEADING_UNKNOWN`].
    pub heading: u16,
    /// Speed in 0.5 m/s (0..=126), or [`SPEED_UNKNOWN`].
    pub speed: u8,
    /// Radius of uncertainty, `2^(prec/16)` metres.
    pub prec: u8,
    /// [`FLAG_SOS`], [`FLAG_END`]; unknown bits are kept and ignored.
    pub flags: u8,
}

impl Position {
    pub const fn sos(&self) -> bool {
        self.flags & FLAG_SOS != 0
    }

    pub const fn end(&self) -> bool {
        self.flags & FLAG_END != 0
    }

    /// The 14 bytes. Out-of-range fields are clamped (coordinates) or sent as unknown.
    pub fn encode(&self) -> [u8; LEN] {
        let lat = self.lat.clamp(I24_MIN, I24_MAX).to_le_bytes();
        let lon = self.lon.clamp(I24_MIN, I24_MAX).to_le_bytes();
        let heading = if self.heading < 360 {
            self.heading
        } else {
            HEADING_UNKNOWN
        };
        let speed = u16::from(self.speed.min(SPEED_UNKNOWN));
        let hs = (heading | (speed << 9)).to_le_bytes();
        let alt = self.alt.to_le_bytes();
        let t = self.t16.to_le_bytes();
        [
            lat[0], lat[1], lat[2], lon[0], lon[1], lon[2], alt[0], alt[1], t[0], t[1], hs[0],
            hs[1], self.prec, self.flags,
        ]
    }

    /// Parses exactly [`LEN`] bytes. A heading of 360..=510 reads as unknown.
    pub fn decode(b: &[u8]) -> Option<Self> {
        let b: &[u8; LEN] = b.try_into().ok()?;
        let [
            la0,
            la1,
            la2,
            lo0,
            lo1,
            lo2,
            a0,
            a1,
            t0,
            t1,
            h0,
            h1,
            prec,
            flags,
        ] = *b;
        let hs = u16::from_le_bytes([h0, h1]);
        let heading = hs & 0x1FF;
        Some(Self {
            lat: i24(la0, la1, la2),
            lon: i24(lo0, lo1, lo2),
            alt: i16::from_le_bytes([a0, a1]),
            t16: u16::from_le_bytes([t0, t1]),
            heading: if heading < 360 {
                heading
            } else {
                HEADING_UNKNOWN
            },
            speed: (hs >> 9) as u8,
            prec,
            flags,
        })
    }
}

/// Sign-extends three little-endian bytes.
const fn i24(b0: u8, b1: u8, b2: u8) -> i32 {
    i32::from_le_bytes([0, b0, b1, b2]) >> 8
}

/// The full Unix time whose low 16 bits are `t16`, within `[now − PAST_S, now + FUTURE_S]`;
/// `None` when no instant of that window matches (§12.1).
pub const fn unwrap_time(t16: u16, now: u64) -> Option<u64> {
    let hi = now + FUTURE_S;
    // The latest instant ≤ hi with these low bits.
    let delta = (hi.wrapping_sub(t16 as u64)) & 0xFFFF;
    let Some(t) = hi.checked_sub(delta) else {
        return None;
    };
    if t + PAST_S < now { None } else { Some(t) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn layout() {
        let p = Position {
            lat: -2,
            lon: 0x12_3456,
            alt: 1234,
            t16: 0xBEEF,
            heading: 270,
            speed: 3,
            prec: 160,
            flags: FLAG_SOS,
        };
        let b = p.encode();
        assert_eq!(
            b,
            [
                0xFE, 0xFF, 0xFF, 0x56, 0x34, 0x12, 0xD2, 0x04, 0xEF, 0xBE, 0x0E, 0x07, 160, 1
            ]
        );
        assert_eq!(Position::decode(&b), Some(p));
        assert_eq!(Position::decode(&b[..13]), None);
        assert_eq!(Position::decode(&[0; 15]), None);
    }

    #[test]
    fn extremes() {
        let p = Position {
            lat: I24_MIN,
            lon: I24_MAX,
            alt: ALT_UNKNOWN,
            t16: 0,
            heading: HEADING_UNKNOWN,
            speed: SPEED_UNKNOWN,
            prec: 255,
            flags: FLAG_END,
        };
        assert_eq!(Position::decode(&p.encode()), Some(p));
        // Out of range on the way out: clamped, unknown.
        let q = Position {
            lat: i32::MAX,
            lon: i32::MIN,
            heading: 400,
            speed: 200,
            ..p
        };
        let r = Position::decode(&q.encode()).unwrap();
        assert_eq!((r.lat, r.lon), (I24_MAX, I24_MIN));
        assert_eq!((r.heading, r.speed), (HEADING_UNKNOWN, SPEED_UNKNOWN));
    }

    #[test]
    fn time_window() {
        let now = 1_793_000_000u64;
        let t16 = |t: u64| (t & 0xFFFF) as u16;
        assert_eq!(unwrap_time(t16(now), now), Some(now));
        assert_eq!(unwrap_time(t16(now - PAST_S), now), Some(now - PAST_S));
        assert_eq!(unwrap_time(t16(now + FUTURE_S), now), Some(now + FUTURE_S));
        // Just outside both ends: the 136-second hole.
        assert_eq!(unwrap_time(t16(now - PAST_S - 1), now), None);
        assert_eq!(unwrap_time(t16(now + FUTURE_S + 1), now), None);
        // Near the epoch, no underflow.
        assert_eq!(unwrap_time(5, 3), Some(5));
        assert_eq!(unwrap_time(60_000, 3), None);
    }

    proptest! {
        #[test]
        fn round_trip(lat in I24_MIN..=I24_MAX, lon in I24_MIN..=I24_MAX, alt: i16, t16: u16,
                      heading in 0u16..360, speed in 0u8..=127, prec: u8, flags: u8) {
            let p = Position { lat, lon, alt, t16, heading, speed, prec, flags };
            prop_assert_eq!(Position::decode(&p.encode()), Some(p));
        }

        #[test]
        fn decode_never_panics(b in proptest::collection::vec(any::<u8>(), 0..20)) {
            if let Some(p) = Position::decode(&b) {
                prop_assert!(p.heading < 360 || p.heading == HEADING_UNKNOWN);
                prop_assert!(p.speed <= SPEED_UNKNOWN);
                // A heading of 360..=510 is normalised to unknown, nothing else changes.
                let mut e = [0u8; LEN];
                e.copy_from_slice(&b);
                let hs = u16::from_le_bytes([e[10], e[11]]);
                if hs & 0x1FF >= 360 {
                    let n = (hs & !0x1FF) | HEADING_UNKNOWN;
                    e[10..12].copy_from_slice(&n.to_le_bytes());
                }
                prop_assert_eq!(p.encode(), e);
            }
        }

        #[test]
        fn time_in_window(now in PAST_S..u64::from(u32::MAX) * 4, back in 0..=PAST_S + FUTURE_S) {
            let t = now + FUTURE_S - back;
            prop_assert_eq!(unwrap_time((t & 0xFFFF) as u16, now), Some(t));
        }
    }
}
