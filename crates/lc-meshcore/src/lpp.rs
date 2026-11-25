//! A node's readings (protocol §16.7, ADR-0050): CayenneLPP items as MeshCore v1.18 nodes write
//! them (`electroniccats/CayenneLPP` 1.6.1): `channel ‖ type ‖ value`, the value big-endian, two's
//! complement for the signed types. Decode side only, a subset of the types; zero-copy, no
//! floats, never panics.
//!
//! Decoding stops at the first type outside the subset and at an item cut short, keeping the
//! items before it: a type's length is never guessed.

/// The channel of the node itself (`TELEM_CHANNEL_SELF`, `src/helpers/SensorManager.h` L10):
/// its voltage is its battery, its temperature its MCU's.
pub const SELF_CHANNEL: u8 = 1;

/// LPP type of a GPS position (`LPP_GPS`).
pub const GPS: u8 = 136;

/// What a scalar item measures, and the unit [`Value::Scalar`] keeps it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Type 2: hundredths.
    AnalogInput,
    /// Type 100: as sent.
    Generic,
    /// Type 101: lux.
    Illuminance,
    /// Type 103: tenths of a degree Celsius.
    Temperature,
    /// Type 104: tenths of a percent (sent in half percents).
    Humidity,
    /// Type 115: tenths of a hectopascal.
    Pressure,
    /// Type 116: millivolts (sent in hundredths of a volt).
    Voltage,
    /// Type 117: milliamps.
    Current,
    /// Type 118: hertz.
    Frequency,
    /// Type 120: percent.
    Percentage,
    /// Type 121: metres.
    Altitude,
    /// Type 128: watts.
    Power,
    /// Type 130: millimetres.
    Distance,
}

impl Kind {
    /// Its name in the vectors (`testdata/lichen/lpp.toml`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::AnalogInput => "analog_input",
            Self::Generic => "generic",
            Self::Illuminance => "illuminance",
            Self::Temperature => "temperature",
            Self::Humidity => "humidity",
            Self::Pressure => "pressure",
            Self::Voltage => "voltage",
            Self::Current => "current",
            Self::Frequency => "frequency",
            Self::Percentage => "percentage",
            Self::Altitude => "altitude",
            Self::Power => "power",
            Self::Distance => "distance",
        }
    }
}

/// A decoded value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    /// A measurement in its kind's unit ([`Kind`]).
    Scalar { kind: Kind, value: i64 },
    /// A position: 10⁻⁴ degrees, altitude in centimetres.
    Gps {
        lat_e4: i32,
        lon_e4: i32,
        alt_cm: i32,
    },
}

/// One item: its channel and value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub channel: u8,
    pub value: Value,
}

/// A scalar type of the subset: its kind, its size in bytes, whether it is signed, and the
/// factor from what is sent to the unit kept (`CayenneLPP.h` L27–L110, `getTypeSigned`).
const fn scalar(t: u8) -> Option<(Kind, usize, bool, i64)> {
    Some(match t {
        2 => (Kind::AnalogInput, 2, true, 1),
        100 => (Kind::Generic, 4, false, 1),
        101 => (Kind::Illuminance, 2, false, 1),
        103 => (Kind::Temperature, 2, true, 1),
        104 => (Kind::Humidity, 1, false, 5),
        115 => (Kind::Pressure, 2, false, 1),
        116 => (Kind::Voltage, 2, true, 10),
        117 => (Kind::Current, 2, true, 1),
        118 => (Kind::Frequency, 4, false, 1),
        120 => (Kind::Percentage, 1, false, 1),
        121 => (Kind::Altitude, 2, true, 1),
        128 => (Kind::Power, 2, false, 1),
        130 => (Kind::Distance, 4, false, 1),
        _ => return None,
    })
}

/// The items of `data`, in order, up to the first that cannot be read.
#[derive(Clone, Debug)]
pub struct Items<'a> {
    data: &'a [u8],
    pos: usize,
    done: bool,
}

/// The items of a node's readings.
pub const fn items(data: &[u8]) -> Items<'_> {
    Items {
        data,
        pos: 0,
        done: false,
    }
}

impl Items<'_> {
    /// How many bytes the items read so far take.
    pub const fn consumed(&self) -> usize {
        self.pos
    }
}

impl Iterator for Items<'_> {
    type Item = Item;

    fn next(&mut self) -> Option<Item> {
        if self.done {
            return None;
        }
        let item = self.read();
        self.done = item.is_none();
        item
    }
}

impl Items<'_> {
    /// The item at the cursor, and the cursor past it; `None` at the end, at a type outside the
    /// subset or at an item cut short.
    fn read(&mut self) -> Option<Item> {
        let rest = self.data.get(self.pos..)?;
        let (&channel, rest) = rest.split_first()?;
        let (&t, rest) = rest.split_first()?;
        let (value, size) = if t == GPS {
            let b = rest.get(..9)?;
            (
                Value::Gps {
                    lat_e4: i32::try_from(be(b.get(..3)?, true)).ok()?,
                    lon_e4: i32::try_from(be(b.get(3..6)?, true)).ok()?,
                    alt_cm: i32::try_from(be(b.get(6..9)?, true)).ok()?,
                },
                9,
            )
        } else {
            let (kind, size, signed, factor) = scalar(t)?;
            let value = be(rest.get(..size)?, signed) * factor;
            (Value::Scalar { kind, value }, size)
        };
        self.pos += 2 + size;
        Some(Item { channel, value })
    }
}

/// A big-endian integer of 1 to 4 bytes, two's complement when `signed`.
fn be(b: &[u8], signed: bool) -> i64 {
    let mut u: u64 = 0;
    for &x in b {
        u = (u << 8) | u64::from(x);
    }
    let bits = 8 * b.len() as u32;
    let v = u as i64;
    if signed && bits > 0 && (u >> (bits - 1)) & 1 == 1 {
        v - (1i64 << bits)
    } else {
        v
    }
}

/// How many bytes at the start of `data` hold items of the subset: what is worth keeping.
pub fn known_len(data: &[u8]) -> usize {
    let mut it = items(data);
    for _ in it.by_ref() {}
    it.consumed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_endian_with_sign() {
        assert_eq!(be(&[0x01, 0x88], true), 392);
        assert_eq!(be(&[0xFF, 0x83], true), -125);
        assert_eq!(be(&[0xFF, 0x83], false), 65_411);
        assert_eq!(be(&[0x80, 0x00, 0x00], true), -8_388_608);
        assert_eq!(be(&[0xFF, 0xFF, 0xFF, 0xFF], false), 4_294_967_295);
        assert_eq!(be(&[], true), 0);
    }
}
