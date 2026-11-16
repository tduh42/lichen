//! App → device commands (`oxymesh/docs/spec/companion.md` §2), encode side only.

use crate::bytes::Writer;
use crate::codes::cmd;
use crate::path::Path;
use crate::{BLE_FRAME_MAX, EncodeError, FRAME_MAX};

/// The commands lichen sends to a companion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    /// `[1] [1..7] reserved (0) ‖ app name` → `SELF_INFO`.
    AppStart { name: &'a [u8] },
    /// `[22] app_target_ver` → `DEVICE_INFO`.
    DeviceQuery { app_ver: u8 },
    /// `[4] (since u32)` → `CONTACTS_START`, `CONTACT`…, `END_OF_CONTACTS`.
    GetContacts { since: Option<u32> },
    /// `[30] key` → `CONTACT` or `ERR 2`.
    GetContactByKey { key: &'a [u8; 32] },
    /// `[5]` → `CURR_TIME`.
    GetDeviceTime,
    /// `[6] time u32` → `OK`, or `ERR 6` if the clock would go back.
    SetDeviceTime { time: u32 },
    /// `[7] (1 = flood)` → `OK`.
    SendSelfAdvert { flood: bool },
    /// `[13] key` → `OK`: forget the contact's path.
    ResetPath { key: &'a [u8; 32] },
    /// `[20]` → `BATT_AND_STORAGE`.
    GetBattAndStorage,
    /// `[23]` → `PRIVATE_KEY` (64 bytes), or `DISABLED`.
    ExportPrivateKey,
    /// `[25] path_len ‖ path ‖ payload` (payload ≥ 4 bytes) → `OK`: one `RAW_CUSTOM`, direct route.
    SendRawData { path: Path<'a>, payload: &'a [u8] },
    /// `[52] 0 ‖ key` → `SENT`, then push `PATH_DISCOVERY_RESPONSE`.
    SendPathDiscoveryReq { key: &'a [u8; 32] },
    /// `[65] priority ‖ raw frame` → `OK`: the frame is transmitted as is.
    SendRawPacket { priority: u8, raw: &'a [u8] },
    /// `[64]` → `DEFAULT_FLOOD_SCOPE`.
    GetDefaultFloodScope,
    /// `[12] dBm i8` → `OK`, or `ERR 6` outside the radio's range (search, §11.1 POWER).
    SetRadioTxPower { dbm: i8 },
    /// `[56] type` (v8+) → `STATS`; type 1: noise floor, last RSSI and SNR, airtime (§11.2).
    GetStats { kind: u8 },
    /// `[2] txt_type ‖ attempt ‖ ts u32 ‖ key prefix (6) ‖ text` → `SENT`, then push
    /// `SEND_CONFIRMED` once the recipient acknowledged: a plain MeshCore direct message.
    SendTxtMsg {
        kind: u8,
        attempt: u8,
        ts: u32,
        prefix: &'a [u8; 6],
        text: &'a [u8],
    },
    /// `[3] txt_type ‖ channel ‖ ts u32 ‖ text` → `OK`: a channel post, flooded; the companion
    /// prefixes it with the node's name ("name: text").
    SendChannelTxtMsg {
        kind: u8,
        channel: u8,
        ts: u32,
        text: &'a [u8],
    },
    /// `[8] name` → `OK`: the name the node advertises.
    SetAdvertName { name: &'a [u8] },
    /// `[10]` → the next queued message (`CONTACT_MSG`, `CHANNEL_MSG`…) or `NO_MORE_MESSAGES`.
    SyncNextMessage,
    /// `[31] index` → `CHANNEL_INFO`, or `ERR 2` past the last slot.
    GetChannel { index: u8 },
    /// `[32] index ‖ name (32, NUL-padded) ‖ secret (16)` → `OK`; an empty name and a zero
    /// secret clear the slot.
    SetChannel {
        index: u8,
        name: &'a [u8],
        secret: &'a [u8; 16],
    },
}

/// Longest text of a plain MeshCore message (`MAX_TEXT_LEN`, ten AES blocks). A channel post
/// also carries "name: " within it.
pub const TEXT_MAX: usize = 160;

/// Longest channel name in `SET_CHANNEL` (32 bytes, NUL-terminated).
pub const CHANNEL_NAME_MAX: usize = 31;

/// Smallest `SEND_RAW_DATA` payload accepted by the firmware.
pub const RAW_DATA_MIN: usize = 4;

/// Largest `SEND_RAW_DATA` payload for a path of `path_bytes` bytes that reaches the other
/// app whole: our command frame (`2 + path + payload`) and the receiving companion's `RAW_DATA`
/// push (`4 + payload`) both fit a Bluetooth link ([`BLE_FRAME_MAX`], 173; ADR-0031). 169 up to
/// 2 path bytes.
pub const fn raw_payload_max(path_bytes: usize) -> usize {
    let by_frame = BLE_FRAME_MAX - 2 - path_bytes;
    let by_push = BLE_FRAME_MAX - 4;
    if by_frame < by_push {
        by_frame
    } else {
        by_push
    }
}

impl Command<'_> {
    /// Writes the frame into `out` and returns its length.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut w = Writer::new(out);
        match *self {
            Command::AppStart { name } => {
                w.put(&[cmd::APP_START, 0, 0, 0, 0, 0, 0, 0])?;
                w.put(name)?;
            }
            Command::DeviceQuery { app_ver } => w.put(&[cmd::DEVICE_QUERY, app_ver])?,
            Command::GetContacts { since } => {
                w.u8(cmd::GET_CONTACTS)?;
                if let Some(since) = since {
                    w.u32(since)?;
                }
            }
            Command::GetContactByKey { key } => {
                w.u8(cmd::GET_CONTACT_BY_KEY)?;
                w.put(key)?;
            }
            Command::GetDeviceTime => w.u8(cmd::GET_DEVICE_TIME)?,
            Command::SetDeviceTime { time } => {
                w.u8(cmd::SET_DEVICE_TIME)?;
                w.u32(time)?;
            }
            Command::SendSelfAdvert { flood } => {
                w.put(&[cmd::SEND_SELF_ADVERT, u8::from(flood)])?
            }
            Command::ResetPath { key } => {
                w.u8(cmd::RESET_PATH)?;
                w.put(key)?;
            }
            Command::GetBattAndStorage => w.u8(cmd::GET_BATT_AND_STORAGE)?,
            Command::ExportPrivateKey => w.u8(cmd::EXPORT_PRIVATE_KEY)?,
            Command::SendRawData { path, payload } => {
                if payload.len() < RAW_DATA_MIN
                    || payload.len() > raw_payload_max(path.bytes().len())
                {
                    return Err(EncodeError::Invalid);
                }
                w.put(&[cmd::SEND_RAW_DATA, path.len_byte()])?;
                w.put(path.bytes())?;
                w.put(payload)?;
            }
            Command::SendPathDiscoveryReq { key } => {
                w.put(&[cmd::SEND_PATH_DISCOVERY_REQ, 0])?;
                w.put(key)?;
            }
            Command::GetDefaultFloodScope => w.u8(cmd::GET_DEFAULT_FLOOD_SCOPE)?,
            Command::SetRadioTxPower { dbm } => w.put(&[cmd::SET_RADIO_TX_POWER, dbm as u8])?,
            Command::GetStats { kind } => w.put(&[cmd::GET_STATS, kind])?,
            Command::SendTxtMsg {
                kind,
                attempt,
                ts,
                prefix,
                text,
            } => {
                if text.len() > TEXT_MAX {
                    return Err(EncodeError::Invalid);
                }
                w.put(&[cmd::SEND_TXT_MSG, kind, attempt])?;
                w.u32(ts)?;
                w.put(prefix)?;
                w.put(text)?;
            }
            Command::SendChannelTxtMsg {
                kind,
                channel,
                ts,
                text,
            } => {
                if text.len() > TEXT_MAX {
                    return Err(EncodeError::Invalid);
                }
                w.put(&[cmd::SEND_CHANNEL_TXT_MSG, kind, channel])?;
                w.u32(ts)?;
                w.put(text)?;
            }
            Command::SetAdvertName { name } => {
                if name.len() > CHANNEL_NAME_MAX {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::SET_ADVERT_NAME)?;
                w.put(name)?;
            }
            Command::SyncNextMessage => w.u8(cmd::SYNC_NEXT_MESSAGE)?,
            Command::GetChannel { index } => w.put(&[cmd::GET_CHANNEL, index])?,
            Command::SetChannel {
                index,
                name,
                secret,
            } => {
                if name.len() > CHANNEL_NAME_MAX || name.contains(&0) {
                    return Err(EncodeError::Invalid);
                }
                let mut padded = [0u8; 32];
                for (p, &b) in padded.iter_mut().zip(name) {
                    *p = b;
                }
                w.put(&[cmd::SET_CHANNEL, index])?;
                w.put(&padded)?;
                w.put(secret)?;
            }
            Command::SendRawPacket { priority, raw } => {
                if raw.len() < 2 {
                    return Err(EncodeError::Invalid);
                }
                w.put(&[cmd::SEND_RAW_PACKET, priority])?;
                w.put(raw)?;
            }
        }
        if w.len() > FRAME_MAX {
            return Err(EncodeError::Overflow);
        }
        Ok(w.len())
    }
}
