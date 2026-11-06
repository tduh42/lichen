//! App → device commands (`oxymesh/docs/spec/companion.md` §2), encode side only.

use crate::bytes::Writer;
use crate::codes::cmd;
use crate::path::Path;
use crate::{EncodeError, FRAME_MAX};

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
}

/// Smallest `SEND_RAW_DATA` payload accepted by the firmware.
pub const RAW_DATA_MIN: usize = 4;

/// Largest `SEND_RAW_DATA` payload for a path of `path_bytes` bytes: limited by the companion
/// frame (176) and by what the receiving companion can push back (`RAW_DATA`, 172).
pub const fn raw_payload_max(path_bytes: usize) -> usize {
    let by_frame = FRAME_MAX - 2 - path_bytes;
    if by_frame < 172 { by_frame } else { 172 }
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
