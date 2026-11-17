//! What repeaters and room servers answer (protocol §16.5): their status and a repeater's
//! neighbours, decode side only. Zero-copy, never panics.

use crate::bytes::Reader;

/// A server's statistics (`STATUS_RESPONSE`'s data): `RepeaterStats` in
/// `examples/simple_repeater/MyMesh.h` (56 bytes), `ServerStats` in
/// `examples/simple_room_server/MyMesh.cpp` (52 bytes); both start with the same 48 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerStatus {
    pub battery_mv: u16,
    pub tx_queue: u16,
    pub noise_floor: i16,
    pub last_rssi: i16,
    pub packets_recv: u32,
    pub packets_sent: u32,
    pub air_time_s: u32,
    pub uptime_s: u32,
    pub sent_flood: u32,
    pub sent_direct: u32,
    pub recv_flood: u32,
    pub recv_direct: u32,
    pub err_events: u16,
    /// SNR × 4 of the last packet it heard.
    pub last_snr4: i16,
    pub direct_dups: u16,
    pub flood_dups: u16,
    /// A repeater's receive airtime (s) and receive errors.
    pub rx_air_time_s: Option<u32>,
    pub recv_errors: Option<u32>,
    /// A room server's posts stored and pushed.
    pub posted: Option<u16>,
    pub post_pushes: Option<u16>,
}

/// Bytes of a repeater's status and of a room server's.
pub const REPEATER_STATUS_LEN: usize = 56;
pub const ROOM_STATUS_LEN: usize = 52;

impl ServerStatus {
    /// Decodes a status; `None` when it is shorter than the common 48 bytes.
    pub fn decode(data: &[u8]) -> Option<Self> {
        let mut r = Reader::new(data);
        let i16_ = |r: &mut Reader<'_>| r.u16().map(|v| v as i16);
        let mut s = Self {
            battery_mv: r.u16()?,
            tx_queue: r.u16()?,
            noise_floor: i16_(&mut r)?,
            last_rssi: i16_(&mut r)?,
            packets_recv: r.u32()?,
            packets_sent: r.u32()?,
            air_time_s: r.u32()?,
            uptime_s: r.u32()?,
            sent_flood: r.u32()?,
            sent_direct: r.u32()?,
            recv_flood: r.u32()?,
            recv_direct: r.u32()?,
            err_events: r.u16()?,
            last_snr4: i16_(&mut r)?,
            direct_dups: r.u16()?,
            flood_dups: r.u16()?,
            ..Self::default()
        };
        if data.len() >= REPEATER_STATUS_LEN {
            s.rx_air_time_s = r.u32();
            s.recv_errors = r.u32();
        } else if data.len() >= ROOM_STATUS_LEN {
            s.posted = r.u16();
            s.post_pushes = r.u16();
        }
        Some(s)
    }
}

/// One of a repeater's neighbours: its key's first bytes, how long ago it heard it, and the
/// SNR × 4 of that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Neighbour<'a> {
    pub prefix: &'a [u8],
    pub heard_ago_s: u32,
    pub snr4: i8,
}

/// A repeater's answer to `GET_NEIGHBOURS` (`examples/simple_repeater/MyMesh.cpp` L276):
/// `total u16 ‖ count u16 ‖ count × (key prefix ‖ heard_ago_s u32 ‖ snr4 i8)`, the prefix as
/// long as the request asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Neighbours<'a> {
    /// Neighbours it knows in all.
    pub total: u16,
    /// Neighbours in this answer.
    pub count: u16,
    entries: &'a [u8],
    prefix_len: usize,
}

impl<'a> Neighbours<'a> {
    /// Decodes an answer whose request asked for `prefix_len` key bytes; `None` when the
    /// entries do not add up.
    pub fn decode(data: &'a [u8], prefix_len: usize) -> Option<Self> {
        let mut r = Reader::new(data);
        let total = r.u16()?;
        let count = r.u16()?;
        let entries = r.rest();
        let size = prefix_len.checked_add(5)?;
        if entries.len() != usize::from(count).checked_mul(size)? {
            return None;
        }
        Some(Self {
            total,
            count,
            entries,
            prefix_len,
        })
    }

    /// The neighbours in this answer, in the order asked for.
    pub fn iter(&self) -> impl Iterator<Item = Neighbour<'a>> + 'a {
        let prefix_len = self.prefix_len;
        self.entries
            .chunks_exact(prefix_len + 5)
            .filter_map(move |e| {
                let mut r = Reader::new(e);
                Some(Neighbour {
                    prefix: r.bytes(prefix_len)?,
                    heard_ago_s: r.u32()?,
                    snr4: r.i8()?,
                })
            })
    }
}
