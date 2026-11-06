//! MeshCore on-air frames (`oxymesh/docs/spec/air.md` §1): `header ‖ [tc0 u16 ‖ tc1 u16]
//! (routes 0 and 3) ‖ path_len ‖ path ‖ payload`. lichen builds `GRP_DATA` frames for
//! `SEND_RAW_PACKET` and reads frames from `LOG_RX_DATA`.

use crate::bytes::{Reader, Writer};
use crate::path::{Path, path_bytes};
use crate::{DecodeError, EncodeError};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// Largest on-air frame (`MAX_TRANS_UNIT`).
pub const MTU: usize = 255;
/// Largest payload (`MAX_PACKET_PAYLOAD`).
pub const PAYLOAD_MAX: usize = 184;

/// Route type (header bits 0-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    TransportFlood = 0,
    Flood = 1,
    Direct = 2,
    TransportDirect = 3,
}

impl Route {
    const fn from_bits(b: u8) -> Self {
        match b & 3 {
            0 => Route::TransportFlood,
            1 => Route::Flood,
            2 => Route::Direct,
            _ => Route::TransportDirect,
        }
    }

    /// Routes 0 and 3 carry two transport codes.
    pub const fn has_transport(self) -> bool {
        matches!(self, Route::TransportFlood | Route::TransportDirect)
    }
}

/// Payload types (header bits 2-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PayloadType {
    Req = 0x0,
    Response = 0x1,
    TxtMsg = 0x2,
    Ack = 0x3,
    Advert = 0x4,
    GrpTxt = 0x5,
    GrpData = 0x6,
    AnonReq = 0x7,
    Path = 0x8,
    Trace = 0x9,
    Multipart = 0xA,
    Control = 0xB,
    Reserved0C = 0xC,
    Reserved0D = 0xD,
    Reserved0E = 0xE,
    RawCustom = 0xF,
}

impl PayloadType {
    const fn from_bits(b: u8) -> Self {
        match b & 0xF {
            0x0 => PayloadType::Req,
            0x1 => PayloadType::Response,
            0x2 => PayloadType::TxtMsg,
            0x3 => PayloadType::Ack,
            0x4 => PayloadType::Advert,
            0x5 => PayloadType::GrpTxt,
            0x6 => PayloadType::GrpData,
            0x7 => PayloadType::AnonReq,
            0x8 => PayloadType::Path,
            0x9 => PayloadType::Trace,
            0xA => PayloadType::Multipart,
            0xB => PayloadType::Control,
            0xC => PayloadType::Reserved0C,
            0xD => PayloadType::Reserved0D,
            0xE => PayloadType::Reserved0E,
            _ => PayloadType::RawCustom,
        }
    }
}

/// A decoded on-air frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub route: Route,
    pub ptype: PayloadType,
    /// `tc0`, `tc1` for routes 0 and 3.
    pub transport: Option<[u16; 2]>,
    pub path: Path<'a>,
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    /// Decodes a raw frame with the receive rules of `Dispatcher::tryParsePacket`.
    pub fn decode(raw: &'a [u8]) -> Result<Self, DecodeError> {
        if raw.len() > MTU {
            return Err(DecodeError::Invalid);
        }
        let mut r = Reader::new(raw);
        let header = r.u8().ok_or(DecodeError::Empty)?;
        if header >> 6 != 0 {
            return Err(DecodeError::Invalid);
        }
        let route = Route::from_bits(header);
        let ptype = PayloadType::from_bits(header >> 2);
        let transport = if route.has_transport() {
            Some([
                r.u16().ok_or(DecodeError::Truncated)?,
                r.u16().ok_or(DecodeError::Truncated)?,
            ])
        } else {
            None
        };
        let len_byte = r.u8().ok_or(DecodeError::Truncated)?;
        let n = path_bytes(len_byte).ok_or(DecodeError::Invalid)?;
        let path = Path::new(len_byte, r.bytes(n).ok_or(DecodeError::Truncated)?)?;
        let payload = r.rest();
        if payload.len() > PAYLOAD_MAX {
            return Err(DecodeError::Invalid);
        }
        Ok(Self {
            route,
            ptype,
            transport,
            path,
            payload,
        })
    }

    /// The header byte (version 0).
    pub const fn header(&self) -> u8 {
        ((self.ptype as u8) << 2) | self.route as u8
    }

    /// Writes the frame into `out`; returns its length.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        if self.payload.len() > PAYLOAD_MAX
            || self.route.has_transport() != self.transport.is_some()
        {
            return Err(EncodeError::Invalid);
        }
        let mut w = Writer::new(out);
        w.u8(self.header())?;
        if let Some([tc0, tc1]) = self.transport {
            w.u16(tc0)?;
            w.u16(tc1)?;
        }
        w.u8(self.path.len_byte())?;
        w.put(self.path.bytes())?;
        w.put(self.payload)?;
        if w.len() > MTU {
            return Err(EncodeError::Overflow);
        }
        Ok(w.len())
    }

    /// Duplicate-filter hash: `SHA256([type] ‖ (TRACE: path_len u16) ‖ payload)[0..8]`
    /// (`src/Packet.cpp:41-50`). Route, path and transport codes are not covered.
    pub fn dedup_hash(&self) -> [u8; 8] {
        dedup_hash(self.ptype, self.path.len_byte(), self.payload)
    }
}

/// Key of a region (flood scope) from its name: `SHA256("#" ‖ name)[0..16]`; "fr" and "#fr"
/// give the same key (`oxymesh/docs/spec/air.md` §1.3).
pub fn region_key(name: &str) -> [u8; 16] {
    let name = name.strip_prefix('#').unwrap_or(name);
    let mut h = Sha256::new();
    h.update(b"#");
    h.update(name.as_bytes());
    let d = h.finalize();
    let mut k = [0u8; 16];
    for (o, b) in k.iter_mut().zip(d.iter()) {
        *o = *b;
    }
    k
}

/// Transport code of a scoped flood: `u16le(HMAC-SHA256(key, [type] ‖ payload)[0..2])`, with
/// 0x0000 → 0x0001 and 0xFFFF → 0xFFFE.
pub fn transport_code(key: &[u8; 16], ptype: PayloadType, payload: &[u8]) -> u16 {
    let Ok(mut mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else {
        return 1;
    };
    mac.update(&[ptype as u8]);
    mac.update(payload);
    let tag = mac.finalize().into_bytes();
    let code = u16::from_le_bytes([
        tag.first().copied().unwrap_or(0),
        tag.get(1).copied().unwrap_or(0),
    ]);
    match code {
        0 => 1,
        0xFFFF => 0xFFFE,
        c => c,
    }
}

/// See [`Packet::dedup_hash`].
pub fn dedup_hash(ptype: PayloadType, path_len_byte: u8, payload: &[u8]) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update([ptype as u8]);
    if ptype == PayloadType::Trace {
        h.update(u16::from(path_len_byte).to_le_bytes());
    }
    h.update(payload);
    let d = h.finalize();
    let mut out = [0; 8];
    for (o, b) in out.iter_mut().zip(d.iter()) {
        *o = *b;
    }
    out
}
