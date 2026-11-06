//! Device → app frames (`oxymesh/docs/spec/companion.md` §3 and §4), decode side only.
//! Codes lichen does not use come back as [`Response::Other`].

use crate::bytes::Reader;
use crate::codes::resp;
use crate::path::{PATH_UNKNOWN, Path, path_bytes};
use crate::{DecodeError, FRAME_MAX};

/// A companion contact (`CONTACT` and `NEW_ADVERT`, 148 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contact<'a> {
    pub key: &'a [u8; 32],
    /// 0 none, 1 chat, 2 repeater, 3 room, 4 sensor.
    pub kind: u8,
    pub flags: u8,
    /// `None` when the companion does not know a path (`out_path_len = 0xFF`).
    pub out_path: Option<Path<'a>>,
    /// Name without trailing NULs (raw bytes, normally UTF-8).
    pub name: &'a [u8],
    pub last_advert: u32,
    pub lat_e6: i32,
    pub lon_e6: i32,
    pub lastmod: u32,
}

/// `SELF_INFO`: the node's identity and radio settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfInfo<'a> {
    pub kind: u8,
    pub tx_power_dbm: i8,
    pub max_tx_power_dbm: i8,
    pub key: &'a [u8; 32],
    pub lat_e6: i32,
    pub lon_e6: i32,
    pub freq_khz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    /// Coding rate denominator, 5..8 (4/5..4/8).
    pub cr: u8,
    pub name: &'a [u8],
}

/// `DEVICE_INFO`: firmware version, and the descriptive fields when present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo<'a> {
    pub fw_ver: u8,
    /// Build date, manufacturer, version (NUL-trimmed), when the frame carries them.
    pub build_date: &'a [u8],
    pub manufacturer: &'a [u8],
    pub version: &'a [u8],
    pub repeat: Option<bool>,
    pub path_hash_mode: Option<u8>,
}

/// A frame from the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response<'a> {
    Ok,
    Err(u8),
    ContactsStart {
        count: u32,
    },
    Contact(Contact<'a>),
    EndOfContacts {
        lastmod: u32,
    },
    SelfInfo(SelfInfo<'a>),
    Sent {
        flood: bool,
        tag: u32,
        timeout_ms: u32,
    },
    CurrTime(u32),
    NoMoreMessages,
    BattAndStorage {
        millivolts: u16,
        used_kb: Option<u32>,
        total_kb: Option<u32>,
    },
    DeviceInfo(DeviceInfo<'a>),
    PrivateKey(&'a [u8; 64]),
    Disabled,
    /// The companion's default flood scope: region name (NUL-trimmed) and key, if any.
    DefaultFloodScope {
        name: &'a [u8],
        key: Option<&'a [u8; 16]>,
    },
    Advert {
        key: &'a [u8; 32],
    },
    PathUpdated {
        key: &'a [u8; 32],
    },
    SendConfirmed {
        ack: u32,
        rtt_ms: u32,
    },
    MsgWaiting,
    /// A `RAW_CUSTOM` received at the end of its direct path.
    RawData {
        snr4: i8,
        rssi: i8,
        payload: &'a [u8],
    },
    /// Any frame heard on the air, raw.
    LogRxData {
        snr4: i8,
        rssi: i8,
        raw: &'a [u8],
    },
    NewAdvert(Contact<'a>),
    PathDiscoveryResponse {
        prefix: &'a [u8; 6],
        out_path: Path<'a>,
        in_path: Path<'a>,
    },
    /// A code lichen does not interpret.
    Other {
        code: u8,
        body: &'a [u8],
    },
}

fn trim_nul(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b.get(..end).unwrap_or(b)
}

fn read_path<'a>(r: &mut Reader<'a>) -> Result<Path<'a>, DecodeError> {
    let len_byte = r.u8().ok_or(DecodeError::Truncated)?;
    let n = path_bytes(len_byte).ok_or(DecodeError::Invalid)?;
    let bytes = r.bytes(n).ok_or(DecodeError::Truncated)?;
    Path::new(len_byte, bytes)
}

fn contact<'a>(r: &mut Reader<'a>) -> Result<Contact<'a>, DecodeError> {
    let t = DecodeError::Truncated;
    let key = r.array::<32>().ok_or(t)?;
    let kind = r.u8().ok_or(t)?;
    let flags = r.u8().ok_or(t)?;
    let len_byte = r.u8().ok_or(t)?;
    let raw_path = r.array::<64>().ok_or(t)?;
    let out_path = if len_byte == PATH_UNKNOWN {
        None
    } else {
        let n = path_bytes(len_byte).ok_or(DecodeError::Invalid)?;
        let bytes = raw_path.get(..n).ok_or(DecodeError::Invalid)?;
        Some(Path::new(len_byte, bytes)?)
    };
    let name = trim_nul(r.array::<32>().ok_or(t)?);
    let last_advert = r.u32().ok_or(t)?;
    let lat_e6 = r.i32().ok_or(t)?;
    let lon_e6 = r.i32().ok_or(t)?;
    let lastmod = r.u32().ok_or(t)?;
    Ok(Contact {
        key,
        kind,
        flags,
        out_path,
        name,
        last_advert,
        lat_e6,
        lon_e6,
        lastmod,
    })
}

impl<'a> Response<'a> {
    /// Decodes one device frame. Never panics; trailing bytes are ignored as the firmware
    /// may append fields in later versions.
    pub fn decode(frame: &'a [u8]) -> Result<Self, DecodeError> {
        if frame.len() > FRAME_MAX {
            return Err(DecodeError::Invalid);
        }
        let mut r = Reader::new(frame);
        let code = r.u8().ok_or(DecodeError::Empty)?;
        let t = DecodeError::Truncated;
        Ok(match code {
            resp::OK => Response::Ok,
            resp::ERR => Response::Err(r.u8().unwrap_or(0)),
            resp::CONTACTS_START => Response::ContactsStart {
                count: r.u32().ok_or(t)?,
            },
            resp::CONTACT => Response::Contact(contact(&mut r)?),
            resp::END_OF_CONTACTS => Response::EndOfContacts {
                lastmod: r.u32().ok_or(t)?,
            },
            resp::SELF_INFO => {
                let kind = r.u8().ok_or(t)?;
                let tx_power_dbm = r.i8().ok_or(t)?;
                let max_tx_power_dbm = r.i8().ok_or(t)?;
                let key = r.array::<32>().ok_or(t)?;
                let lat_e6 = r.i32().ok_or(t)?;
                let lon_e6 = r.i32().ok_or(t)?;
                let _prefs = r.array::<4>().ok_or(t)?;
                let freq_khz = r.u32().ok_or(t)?;
                let bw_hz = r.u32().ok_or(t)?;
                let sf = r.u8().ok_or(t)?;
                let cr = r.u8().ok_or(t)?;
                Response::SelfInfo(SelfInfo {
                    kind,
                    tx_power_dbm,
                    max_tx_power_dbm,
                    key,
                    lat_e6,
                    lon_e6,
                    freq_khz,
                    bw_hz,
                    sf,
                    cr,
                    name: trim_nul(r.rest()),
                })
            }
            resp::SENT => Response::Sent {
                flood: r.u8().ok_or(t)? != 0,
                tag: r.u32().ok_or(t)?,
                timeout_ms: r.u32().ok_or(t)?,
            },
            resp::CURR_TIME => Response::CurrTime(r.u32().ok_or(t)?),
            resp::NO_MORE_MESSAGES => Response::NoMoreMessages,
            resp::BATT_AND_STORAGE => Response::BattAndStorage {
                millivolts: r.u16().ok_or(t)?,
                used_kb: r.u32(),
                total_kb: r.u32(),
            },
            resp::DEVICE_INFO => {
                let fw_ver = r.u8().ok_or(t)?;
                let mut info = DeviceInfo {
                    fw_ver,
                    build_date: &[],
                    manufacturer: &[],
                    version: &[],
                    repeat: None,
                    path_hash_mode: None,
                };
                if r.remaining() >= 78 {
                    let _limits = r.bytes(6).ok_or(t)?;
                    info.build_date = trim_nul(r.bytes(12).ok_or(t)?);
                    info.manufacturer = trim_nul(r.bytes(40).ok_or(t)?);
                    info.version = trim_nul(r.bytes(20).ok_or(t)?);
                    info.repeat = r.u8().map(|b| b != 0);
                    info.path_hash_mode = r.u8();
                }
                Response::DeviceInfo(info)
            }
            resp::PRIVATE_KEY => Response::PrivateKey(r.array::<64>().ok_or(t)?),
            resp::DISABLED => Response::Disabled,
            resp::DEFAULT_FLOOD_SCOPE => {
                if r.remaining() >= 47 {
                    let name = trim_nul(r.bytes(31).ok_or(t)?);
                    let key = r.array::<16>().ok_or(t)?;
                    let key = key.iter().any(|&b| b != 0).then_some(key);
                    Response::DefaultFloodScope { name, key }
                } else {
                    Response::DefaultFloodScope {
                        name: &[],
                        key: None,
                    }
                }
            }
            resp::ADVERT => Response::Advert {
                key: r.array::<32>().ok_or(t)?,
            },
            resp::PATH_UPDATED => Response::PathUpdated {
                key: r.array::<32>().ok_or(t)?,
            },
            resp::SEND_CONFIRMED => Response::SendConfirmed {
                ack: r.u32().ok_or(t)?,
                rtt_ms: r.u32().ok_or(t)?,
            },
            resp::MSG_WAITING => Response::MsgWaiting,
            resp::RAW_DATA => {
                let snr4 = r.i8().ok_or(t)?;
                let rssi = r.i8().ok_or(t)?;
                let _reserved = r.u8().ok_or(t)?;
                Response::RawData {
                    snr4,
                    rssi,
                    payload: r.rest(),
                }
            }
            resp::LOG_RX_DATA => {
                let snr4 = r.i8().ok_or(t)?;
                let rssi = r.i8().ok_or(t)?;
                Response::LogRxData {
                    snr4,
                    rssi,
                    raw: r.rest(),
                }
            }
            resp::NEW_ADVERT => Response::NewAdvert(contact(&mut r)?),
            resp::PATH_DISCOVERY_RESPONSE => {
                let _reserved = r.u8().ok_or(t)?;
                let prefix = r.array::<6>().ok_or(t)?;
                let out_path = read_path(&mut r)?;
                let in_path = read_path(&mut r)?;
                Response::PathDiscoveryResponse {
                    prefix,
                    out_path,
                    in_path,
                }
            }
            code => Response::Other {
                code,
                body: r.rest(),
            },
        })
    }
}
