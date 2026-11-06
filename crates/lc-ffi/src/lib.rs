//! UniFFI bindings of the lichen core (`lc-core`) for the iOS and Android apps.
//!
//! The app is the runner: it passes companion frames and the time in, writes the frames
//! `poll_companion` returns to the companion (BLE or USB), calls `tick` at `next_wakeup`, and
//! reads events. Keys and ids cross the boundary as byte arrays.

use std::sync::{Arc, Mutex, MutexGuard};

use lc_core::{Config, Content, Core, Event, FailReason as CoreFail, GroupId, PubKey};

mod demo;

pub use demo::DemoNetwork;

uniffi::setup_scaffolding!();

/// Errors returned to the app.
#[derive(Debug, PartialEq, Eq, uniffi::Error)]
pub enum LichenError {
    UnknownContact,
    UnknownGroup,
    Empty,
    TooLarge,
    NoIdentity,
    NotAllowed,
    /// A key or id does not have the expected length.
    InvalidKey,
}

impl std::fmt::Display for LichenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for LichenError {}

impl From<lc_core::Error> for LichenError {
    fn from(e: lc_core::Error) -> Self {
        match e {
            lc_core::Error::UnknownContact => Self::UnknownContact,
            lc_core::Error::UnknownGroup => Self::UnknownGroup,
            lc_core::Error::Empty => Self::Empty,
            lc_core::Error::TooLarge => Self::TooLarge,
            lc_core::Error::NoIdentity => Self::NoIdentity,
            lc_core::Error::NotAllowed => Self::NotAllowed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Radio {
    pub freq_khz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    /// Coding rate denominator (5..8).
    pub cr: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ContactInfo {
    pub key: Vec<u8>,
    pub name: String,
    /// Repeaters on the known path, if any.
    pub hops: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GroupInfo {
    pub gid: Vec<u8>,
    pub name: String,
    pub admin: Vec<u8>,
    pub members: Vec<ContactInfo>,
    /// Members whose sender key we hold (out of `members.len() - 1`).
    pub keys_known: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Budget {
    pub used_ms: u64,
    pub limit_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Estimate {
    pub fragments: u8,
    pub airtime_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Message {
    Text { sent_at: u32, text: String },
    Other { content_type: u8, body: Vec<u8> },
}

impl From<Content> for Message {
    fn from(c: Content) -> Self {
        match c {
            Content::Text { sent_at, text } => Self::Text { sent_at, text },
            Content::Other { content_type, body } => Self::Other { content_type, body },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FailReason {
    NoPath,
    Unreachable,
    Timeout,
    Refused,
    TooLarge,
}

impl From<CoreFail> for FailReason {
    fn from(r: CoreFail) -> Self {
        match r {
            CoreFail::NoPath => Self::NoPath,
            CoreFail::Unreachable => Self::Unreachable,
            CoreFail::Timeout => Self::Timeout,
            CoreFail::Refused => Self::Refused,
            CoreFail::TooLarge => Self::TooLarge,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LichenEvent {
    Connected {
        key: Vec<u8>,
        name: String,
        radio: Radio,
    },
    IdentityReady,
    KeyExportRefused,
    ContactsUpdated,
    SessionEstablished {
        peer: Vec<u8>,
    },
    Received {
        from: Vec<u8>,
        message: Message,
        snr_db: f32,
        rssi: i8,
    },
    Progress {
        id: u64,
        sent: u32,
        total: u8,
    },
    Delivered {
        id: u64,
    },
    Failed {
        id: u64,
        reason: FailReason,
    },
    GroupJoined {
        gid: Vec<u8>,
        name: String,
        admin: Vec<u8>,
    },
    GroupUpdated {
        gid: Vec<u8>,
    },
    GroupLeft {
        gid: Vec<u8>,
    },
    GroupMessage {
        gid: Vec<u8>,
        from: Vec<u8>,
        message: Message,
        snr_db: f32,
        rssi: i8,
    },
}

fn snr(snr4: i8) -> f32 {
    f32::from(snr4) / 4.0
}

impl LichenEvent {
    fn from_core(e: Event, freq_khz: u32) -> Self {
        match e {
            Event::Connected {
                key,
                name,
                freq_khz: f,
                radio,
            } => Self::Connected {
                key: key.to_vec(),
                name,
                radio: Radio {
                    freq_khz: if f == 0 { freq_khz } else { f },
                    bw_hz: radio.bw_hz,
                    sf: radio.sf,
                    cr: radio.cr,
                },
            },
            Event::IdentityReady => Self::IdentityReady,
            Event::KeyExportRefused => Self::KeyExportRefused,
            Event::ContactsUpdated => Self::ContactsUpdated,
            Event::SessionEstablished { peer } => Self::SessionEstablished {
                peer: peer.to_vec(),
            },
            Event::Received {
                from,
                content,
                snr4,
                rssi,
            } => Self::Received {
                from: from.to_vec(),
                message: content.into(),
                snr_db: snr(snr4),
                rssi,
            },
            Event::Progress { id, sent, total } => Self::Progress { id, sent, total },
            Event::Delivered { id } => Self::Delivered { id },
            Event::Failed { id, reason } => Self::Failed {
                id,
                reason: reason.into(),
            },
            Event::GroupJoined { gid, name, admin } => Self::GroupJoined {
                gid: gid.to_vec(),
                name,
                admin: admin.to_vec(),
            },
            Event::GroupUpdated { gid } => Self::GroupUpdated { gid: gid.to_vec() },
            Event::GroupLeft { gid } => Self::GroupLeft { gid: gid.to_vec() },
            Event::GroupMessage {
                gid,
                from,
                content,
                snr4,
                rssi,
            } => Self::GroupMessage {
                gid: gid.to_vec(),
                from: from.to_vec(),
                message: content.into(),
                snr_db: snr(snr4),
                rssi,
            },
        }
    }
}

fn key(v: &[u8]) -> Result<PubKey, LichenError> {
    v.try_into().map_err(|_| LichenError::InvalidKey)
}

fn gid(v: &[u8]) -> Result<GroupId, LichenError> {
    v.try_into().map_err(|_| LichenError::InvalidKey)
}

/// The lichen core, shared between threads.
#[derive(uniffi::Object)]
pub struct LichenCore {
    core: Mutex<Core>,
}

impl std::fmt::Debug for LichenCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LichenCore")
    }
}

impl LichenCore {
    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[uniffi::export]
impl LichenCore {
    /// `rng_seed`: 32 bytes from the OS random generator. `identity`: the node's 64-byte
    /// private key if the app stored it (Keychain), otherwise it is exported from the companion.
    #[uniffi::constructor]
    pub fn new(
        app_name: String,
        rng_seed: Vec<u8>,
        identity: Option<Vec<u8>>,
    ) -> Result<Arc<Self>, LichenError> {
        let seed: [u8; 32] = rng_seed
            .as_slice()
            .try_into()
            .map_err(|_| LichenError::InvalidKey)?;
        let mut config = Config::new(&app_name, seed);
        config.identity = match identity {
            Some(k) => Some(
                k.as_slice()
                    .try_into()
                    .map_err(|_| LichenError::InvalidKey)?,
            ),
            None => None,
        };
        Ok(Arc::new(Self {
            core: Mutex::new(Core::new(config)),
        }))
    }

    pub fn connect(&self, now_ms: u64) {
        self.lock().connect(now_ms);
    }

    pub fn on_companion_frame(&self, frame: Vec<u8>, now_ms: u64) {
        self.lock().on_companion_frame(&frame, now_ms);
    }

    /// The next frame to write to the companion, if one is due.
    pub fn poll_companion(&self, now_ms: u64) -> Option<Vec<u8>> {
        self.lock().poll_companion(now_ms)
    }

    pub fn tick(&self, now_ms: u64) {
        self.lock().tick(now_ms);
    }

    /// When to call `tick` (or `poll_companion`) next, in the same clock as `now_ms`.
    pub fn next_wakeup(&self) -> Option<u64> {
        self.lock().next_wakeup()
    }

    pub fn poll_event(&self) -> Option<LichenEvent> {
        let mut c = self.lock();
        let f = c.freq_khz();
        c.poll_event().map(|e| LichenEvent::from_core(e, f))
    }

    pub fn key(&self) -> Option<Vec<u8>> {
        self.lock().key().map(|k| k.to_vec())
    }

    /// The node's private key, for the app to keep in the Keychain / Keystore.
    pub fn private_key(&self) -> Option<Vec<u8>> {
        self.lock().private_key().map(|k| k.to_vec())
    }

    pub fn radio(&self) -> Radio {
        let c = self.lock();
        let r = c.radio();
        Radio {
            freq_khz: c.freq_khz(),
            bw_hz: r.bw_hz,
            sf: r.sf,
            cr: r.cr,
        }
    }

    pub fn budget(&self, now_ms: u64) -> Budget {
        let (used_ms, limit_ms) = self.lock().budget(now_ms);
        Budget { used_ms, limit_ms }
    }

    pub fn contacts(&self) -> Vec<ContactInfo> {
        let mut v: Vec<ContactInfo> = self
            .lock()
            .contacts()
            .map(|c| ContactInfo {
                key: c.key.to_vec(),
                name: c.name.clone(),
                hops: c.out_path.as_ref().map(lc_core::OwnedPath::hops),
            })
            .collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    pub fn groups(&self) -> Vec<GroupInfo> {
        let c = self.lock();
        let mut v: Vec<GroupInfo> = c
            .groups()
            .map(|g| GroupInfo {
                gid: g.gid.to_vec(),
                name: g.name.clone(),
                admin: g.admin.to_vec(),
                members: g
                    .members
                    .iter()
                    .map(|m| ContactInfo {
                        key: m.key.to_vec(),
                        name: m.name.clone(),
                        hops: None,
                    })
                    .collect(),
                keys_known: g.keys_known() as u32,
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Cost of a text of `text_len` bytes to `to`: fragments and airtime of the first round.
    pub fn estimate_text(&self, to: Vec<u8>, text_len: u32) -> Result<Estimate, LichenError> {
        let k = key(&to)?;
        let (fragments, airtime_ms) = self.lock().estimate_send(&k, 5 + text_len as usize);
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    pub fn send_text(
        &self,
        to: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<u64, LichenError> {
        let k = key(&to)?;
        Ok(self.lock().send_text(&k, &text, sent_at, now_ms)?)
    }

    pub fn create_group(
        &self,
        name: String,
        members: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<Vec<u8>, LichenError> {
        let keys = members
            .iter()
            .map(|m| key(m))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.lock().create_group(&name, &keys, now_ms)?.to_vec())
    }

    pub fn send_group_text(
        &self,
        gid: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<u64, LichenError> {
        let g = self::gid(&gid)?;
        Ok(self.lock().send_group_text(&g, &text, sent_at, now_ms)?)
    }

    pub fn remove_member(
        &self,
        gid: Vec<u8>,
        member: Vec<u8>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let m = key(&member)?;
        Ok(self.lock().remove_member(&g, &m, now_ms)?)
    }
}

/// UUIDs of the MeshCore companion's Bluetooth service (Nordic UART).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BleUuids {
    pub service: String,
    /// The app writes command frames here.
    pub rx: String,
    /// The companion notifies frames here.
    pub tx: String,
}

#[uniffi::export]
pub fn companion_ble_uuids() -> BleUuids {
    BleUuids {
        service: "6E400001-B5A3-F393-E0A9-E50E24DCCA9E".into(),
        rx: "6E400002-B5A3-F393-E0A9-E50E24DCCA9E".into(),
        tx: "6E400003-B5A3-F393-E0A9-E50E24DCCA9E".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_and_connects() {
        let core = LichenCore::new("test".into(), vec![1; 32], None).unwrap();
        core.connect(0);
        let first = core.poll_companion(0).unwrap();
        assert_eq!(first[0], 1, "APP_START comes first");
        assert!(LichenCore::new("x".into(), vec![1; 31], None).is_err());
        assert_eq!(
            core.send_text(vec![0; 3], "x".into(), 0, 0),
            Err(LichenError::InvalidKey)
        );
        assert_eq!(
            core.send_text(vec![0; 32], "x".into(), 0, 0),
            Err(LichenError::UnknownContact)
        );
    }
}
