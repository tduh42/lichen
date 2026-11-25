//! App → device commands (`oxymesh/docs/spec/companion.md` §2), encode side only.

use crate::bytes::Writer;
use crate::codes::cmd;
use crate::path::Path;
use crate::{BLE_FRAME_MAX, EncodeError, FRAME_MAX};

/// The preferences `SET_OTHER_PARAMS` sets and `SELF_INFO` reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OtherParams {
    /// Only the advert types of the auto-add mask become contacts by themselves; off, every
    /// advert does.
    pub manual_add: bool,
    /// Who may ask for telemetry: `env << 4 | location << 2 | base`, each 0 nobody, 1 the
    /// contacts allowed one by one, 2 everyone.
    pub telemetry: u8,
    /// 0: adverts carry no position; 1: they carry the node's.
    pub advert_loc_policy: u8,
    /// Extra transmissions of each ACK.
    pub multi_acks: u8,
}

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
    /// `[18] packet` (a node's advert, more than 97 bytes) → `OK` when it reads as an ADVERT,
    /// else `ERR 6`. The radio then handles it as if heard by flood, silently: the contact is
    /// added only when the radio would add it by itself (§16.6).
    ImportContact { packet: &'a [u8] },
    /// `[9] key ‖ type ‖ flags 0 ‖ out_path_len 0xFF (no path) ‖ out_path (64, zero) ‖ name (32,
    /// NUL-padded) ‖ last_advert u32 ‖ lat i32 ‖ lon i32` → `OK`, or `ERR 3` when the table is
    /// full; a contact with that key is updated (§16.6). The name is at most 31 bytes.
    AddUpdateContact {
        key: &'a [u8; 32],
        kind: u8,
        name: &'a [u8],
        last_advert: u32,
        lat_e6: i32,
        lon_e6: i32,
    },
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
    /// `[11] freq kHz u32 ‖ bw Hz u32 ‖ sf ‖ cr ‖ repeat` → `OK`, or `ERR 6` outside 150–2,500 MHz,
    /// 7–500 kHz, SF 5–12, CR 5–8 (or a repeat frequency not allowed). Applied at once, kept.
    SetRadioParams {
        freq_khz: u32,
        bw_hz: u32,
        sf: u8,
        cr: u8,
        repeat: bool,
    },
    /// `[14] lat i32 ‖ lon i32` (degrees × 10⁶) → `OK`, or `ERR 6` outside ±90 / ±180: the
    /// position an advert carries when the policy allows it; a GPS fix overwrites it.
    SetAdvertLatLon { lat_e6: i32, lon_e6: i32 },
    /// `[19] "reboot"` → no answer: the radio restarts.
    Reboot,
    /// `[37] pin u32` → `OK`, or `ERR 6` unless 0 (the default) or 100000–999999: the Bluetooth
    /// PIN, used from the next boot.
    SetDevicePin { pin: u32 },
    /// `[38] manual_add ‖ telemetry ‖ advert_loc_policy ‖ multi_acks` → `OK`: all four always
    /// (a shorter frame keeps the rest, but the first byte is always taken).
    SetOtherParams(OtherParams),
    /// `[40]` → `CUSTOM_VARS`: the board's settings, such as `gps`.
    GetCustomVars,
    /// `[41] "name:value"` → `OK`, or `ERR 6` for an unknown name or value (`gps:1`, `gps:0`,
    /// `gps_interval:<s>`).
    SetCustomVar { name: &'a str, value: &'a str },
    /// `[58] mask ‖ max_hops` → `OK`: which advert types become contacts by themselves when
    /// manual add is on, and from how far.
    SetAutoaddConfig { mask: u8, max_hops: u8 },
    /// `[59]` → `AUTOADD_CONFIG`.
    GetAutoaddConfig,
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
    /// `[26] key ‖ password` → `SENT` (`ack` = the key's first 4 bytes), then push
    /// `LOGIN_SUCCESS`, or `LOGIN_FAIL`, or nothing for a wrong password (§15.8, §16.5). The
    /// firmware keeps 15 bytes of the password.
    SendLogin {
        key: &'a [u8; 32],
        password: &'a [u8],
    },
    /// `[27] key` → `SENT`, then push `STATUS_RESPONSE` (§16.5).
    SendStatusReq { key: &'a [u8; 32] },
    /// `[29] key` → `OK`: the radio stops keeping the connection to that server alive.
    Logout { key: &'a [u8; 32] },
    /// `[39] reserved (3) ‖ key` → `SENT`, then push `TELEMETRY_RESPONSE` (§16.7).
    SendTelemetryReq { key: &'a [u8; 32] },
    /// `[50] key ‖ request` → `SENT` (`ack` = the tag), then push `BINARY_RESPONSE` with the
    /// same tag (§16.5).
    SendBinaryReq { key: &'a [u8; 32], data: &'a [u8] },
    /// `[36] tag u32 ‖ auth u32 ‖ flags ‖ path` → `SENT` (`ack` = `tag`), then push
    /// `TRACE_DATA` from the end of the path (§16.3). `flags & 3` is the size of each hash in
    /// `path`, `1 << (flags & 3)` bytes.
    SendTracePath {
        tag: u32,
        auth: u32,
        flags: u8,
        path: &'a [u8],
    },
}

/// Longest text of a plain MeshCore message (`MAX_TEXT_LEN`, ten AES blocks). A channel post
/// also carries "name: " within it.
pub const TEXT_MAX: usize = 160;

/// Longest channel name in `SET_CHANNEL` (32 bytes, NUL-terminated).
pub const CHANNEL_NAME_MAX: usize = 31;

/// Longest contact name in `ADD_UPDATE_CONTACT` (32 bytes, NUL-terminated).
pub const CONTACT_NAME_MAX: usize = 31;

/// Smallest packet `IMPORT_CONTACT` takes (`len > 2 + 32 + 64`, `MyMesh.cpp` L1409, the
/// command byte included).
pub const IMPORT_MIN: usize = 98;

/// Longest password a server checks (`BaseChatMesh::sendLogin` keeps 15 bytes).
pub const PASSWORD_MAX: usize = 15;

/// A `GET_NEIGHBOURS` request to a repeater (`examples/simple_repeater/MyMesh.cpp` L276):
/// version 0, how many, from which, the order (0 newest heard first, 2 strongest first), how
/// many key bytes per neighbour, and 4 random bytes that make the packet unique.
pub fn neighbours_request(
    count: u8,
    offset: u16,
    order: u8,
    prefix_len: u8,
    random: [u8; 4],
) -> [u8; 11] {
    let [o0, o1] = offset.to_le_bytes();
    let [r0, r1, r2, r3] = random;
    [
        crate::codes::req::GET_NEIGHBOURS,
        0,
        count,
        o0,
        o1,
        order,
        prefix_len,
        r0,
        r1,
        r2,
        r3,
    ]
}

/// Longest trace path in bytes the firmware takes (`len - 10 < MAX_PACKET_PAYLOAD - 5`).
pub const TRACE_PATH_MAX: usize = 178;

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
            Command::ImportContact { packet } => {
                if packet.len() < IMPORT_MIN {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::IMPORT_CONTACT)?;
                w.put(packet)?;
            }
            Command::AddUpdateContact {
                key,
                kind,
                name,
                last_advert,
                lat_e6,
                lon_e6,
            } => {
                if name.len() > CONTACT_NAME_MAX {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::ADD_UPDATE_CONTACT)?;
                w.put(key)?;
                w.put(&[kind, 0, 0xFF])?;
                w.put(&[0; crate::path::MAX_PATH_BYTES])?;
                w.put(name)?;
                for _ in name.len()..=CONTACT_NAME_MAX {
                    w.u8(0)?;
                }
                w.u32(last_advert)?;
                w.put(&lat_e6.to_le_bytes())?;
                w.put(&lon_e6.to_le_bytes())?;
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
            Command::SetRadioParams {
                freq_khz,
                bw_hz,
                sf,
                cr,
                repeat,
            } => {
                w.u8(cmd::SET_RADIO_PARAMS)?;
                w.u32(freq_khz)?;
                w.u32(bw_hz)?;
                w.put(&[sf, cr, u8::from(repeat)])?;
            }
            Command::SetAdvertLatLon { lat_e6, lon_e6 } => {
                w.u8(cmd::SET_ADVERT_LATLON)?;
                w.put(&lat_e6.to_le_bytes())?;
                w.put(&lon_e6.to_le_bytes())?;
            }
            Command::Reboot => {
                w.u8(cmd::REBOOT)?;
                w.put(b"reboot")?;
            }
            Command::SetDevicePin { pin } => {
                if pin != 0 && !(100_000..=999_999).contains(&pin) {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::SET_DEVICE_PIN)?;
                w.u32(pin)?;
            }
            Command::SetOtherParams(p) => w.put(&[
                cmd::SET_OTHER_PARAMS,
                u8::from(p.manual_add),
                p.telemetry,
                p.advert_loc_policy,
                p.multi_acks,
            ])?,
            Command::GetCustomVars => w.u8(cmd::GET_CUSTOM_VARS)?,
            Command::SetCustomVar { name, value } => {
                let bad = |s: &str| {
                    s.is_empty()
                        || !s
                            .bytes()
                            .all(|b| b.is_ascii_graphic() && b != b':' && b != b',')
                };
                if bad(name) || bad(value) {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::SET_CUSTOM_VAR)?;
                w.put(name.as_bytes())?;
                w.u8(b':')?;
                w.put(value.as_bytes())?;
            }
            Command::SetAutoaddConfig { mask, max_hops } => {
                w.put(&[cmd::SET_AUTOADD_CONFIG, mask, max_hops.min(64)])?
            }
            Command::GetAutoaddConfig => w.u8(cmd::GET_AUTOADD_CONFIG)?,
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
            Command::SendLogin { key, password } => {
                if password.len() > PASSWORD_MAX || password.contains(&0) {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::SEND_LOGIN)?;
                w.put(key)?;
                w.put(password)?;
            }
            Command::SendStatusReq { key } => {
                w.u8(cmd::SEND_STATUS_REQ)?;
                w.put(key)?;
            }
            Command::Logout { key } => {
                w.u8(cmd::LOGOUT)?;
                w.put(key)?;
            }
            Command::SendTelemetryReq { key } => {
                w.u8(cmd::SEND_TELEMETRY_REQ)?;
                w.put(&[0, 0, 0])?;
                w.put(key)?;
            }
            Command::SendBinaryReq { key, data } => {
                if data.is_empty() {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::SEND_BINARY_REQ)?;
                w.put(key)?;
                w.put(data)?;
            }
            Command::SendTracePath {
                tag,
                auth,
                flags,
                path,
            } => {
                let size = 1usize << (flags & 3);
                if path.is_empty()
                    || path.len() > TRACE_PATH_MAX
                    || path.len() % size != 0
                    || path.len() / size > crate::path::MAX_PATH_BYTES
                {
                    return Err(EncodeError::Invalid);
                }
                w.u8(cmd::SEND_TRACE_PATH)?;
                w.u32(tag)?;
                w.u32(auth)?;
                w.u8(flags)?;
                w.put(path)?;
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
