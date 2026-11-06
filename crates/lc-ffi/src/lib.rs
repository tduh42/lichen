//! UniFFI bindings of the lichen core (`lc-core`) for the iOS and Android apps.
//!
//! The app is the runner: it passes companion frames and the time in, writes the frames
//! `poll_companion` returns to the companion (BLE or USB), calls `tick` at `next_wakeup`, and
//! reads events. Keys and ids cross the boundary as byte arrays.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use lc_core::store::{self, Chat, Status, Store};
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
    /// The app stopped before delivery.
    Interrupted,
}

impl From<CoreFail> for FailReason {
    fn from(r: CoreFail) -> Self {
        match r {
            CoreFail::NoPath => Self::NoPath,
            CoreFail::Unreachable => Self::Unreachable,
            CoreFail::Timeout => Self::Timeout,
            CoreFail::Refused => Self::Refused,
            CoreFail::TooLarge => Self::TooLarge,
            CoreFail::Interrupted => Self::Interrupted,
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
    /// A message was added to the history, or its status changed.
    MessageChanged {
        message: StoredMessage,
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

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// A conversation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChatRef {
    Contact { key: Vec<u8> },
    Group { gid: Vec<u8> },
}

impl ChatRef {
    fn to_store(&self) -> Result<Chat, LichenError> {
        Ok(match self {
            Self::Contact { key: k } => Chat::Contact(key(k)?),
            Self::Group { gid: g } => Chat::Group(gid(g)?),
        })
    }

    fn from_store(c: &Chat) -> Self {
        match c {
            Chat::Contact(k) => Self::Contact { key: k.to_vec() },
            Chat::Group(g) => Self::Group { gid: g.to_vec() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MessageStatus {
    Received,
    Sending {
        sent: u32,
        total: u8,
    },
    Delivered,
    Failed {
        reason: FailReason,
    },
    /// Flooded to a group, without acknowledgement.
    Flooded,
}

/// A message from the history.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StoredMessage {
    pub id: i64,
    pub chat: ChatRef,
    pub outgoing: bool,
    pub author: Option<Vec<u8>>,
    pub message: Message,
    /// Unix milliseconds.
    pub at_ms: i64,
    pub status: MessageStatus,
    pub fragments: u8,
    pub airtime_ms: u64,
    pub snr_db: Option<f32>,
}

impl From<store::Message> for StoredMessage {
    fn from(m: store::Message) -> Self {
        Self {
            id: m.id,
            chat: ChatRef::from_store(&m.chat),
            outgoing: m.outgoing,
            author: m.author.map(|a| a.to_vec()),
            message: m.content.into(),
            at_ms: m.at_ms,
            status: match m.status {
                Status::Received => MessageStatus::Received,
                Status::Sending { sent, total } => MessageStatus::Sending { sent, total },
                Status::Delivered => MessageStatus::Delivered,
                Status::Failed(r) => MessageStatus::Failed { reason: r.into() },
                Status::Flooded => MessageStatus::Flooded,
            },
            fragments: m.fragments,
            airtime_ms: m.airtime_ms,
            snr_db: m.snr4.map(snr),
        }
    }
}

/// A conversation with its last message.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Conversation {
    pub chat: ChatRef,
    pub last: StoredMessage,
    pub unread: u32,
}

struct Inner {
    core: Core,
    store: Option<Store>,
    data_dir: Option<PathBuf>,
    /// Core message id → row id, for the messages sent by this run.
    inflight: HashMap<u64, i64>,
    /// Events to hand out after the current one (`MessageChanged`).
    extra: VecDeque<LichenEvent>,
}

impl Inner {
    /// Opens the database of node `key` (or one in memory) and restores contacts and groups.
    fn open_store(&mut self, node: &PubKey) {
        let store = match &self.data_dir {
            Some(dir) => {
                let _ = std::fs::create_dir_all(dir);
                let hex: String = node.iter().take(8).map(|b| format!("{b:02x}")).collect();
                Store::open(&dir.join(format!("node-{hex}.sqlite")))
            }
            None => Store::in_memory(),
        };
        let Ok(store) = store else {
            return;
        };
        let _ = store.fail_unfinished();
        for c in store.contacts().unwrap_or_default() {
            if !self.core.contacts().any(|k| k.key == c.key) {
                self.core.add_contact(c.key, &c.name, c.out_path.clone());
            }
        }
        for snap in store.groups().unwrap_or_default() {
            self.core.restore_group(&snap);
        }
        self.store = Some(store);
    }

    fn save_contacts(&self) {
        if let Some(st) = &self.store {
            for c in self.core.contacts() {
                let _ = st.put_contact(c);
            }
        }
    }

    /// Saves the groups that changed.
    fn persist(&mut self) {
        let dirty = self.core.take_dirty_groups();
        let Some(st) = &self.store else {
            return;
        };
        for g in dirty {
            let _ = match self.core.group_snapshot(&g) {
                Some(snap) => st.put_group(&g, &snap),
                None => st.delete_group(&g),
            };
        }
    }

    fn record(&mut self, m: store::Message) -> Option<StoredMessage> {
        let row = self.store.as_ref()?.insert(&m).ok()?;
        let out = StoredMessage::from(row);
        self.extra.push_back(LichenEvent::MessageChanged {
            message: out.clone(),
        });
        Some(out)
    }

    fn update(&mut self, core_id: u64, status: Status, done: bool) {
        let Some(&row) = self.inflight.get(&core_id) else {
            return;
        };
        if done {
            self.inflight.remove(&core_id);
        }
        if let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.set_status(row, status).ok().flatten())
        {
            self.extra
                .push_back(LichenEvent::MessageChanged { message: m.into() });
        }
    }

    /// Records what an event means for the history.
    fn on_event(&mut self, e: &Event) {
        match e {
            Event::Connected { key, .. } => {
                if self.store.is_none() {
                    self.open_store(key);
                    self.save_contacts();
                }
            }
            Event::ContactsUpdated | Event::GroupJoined { .. } | Event::GroupUpdated { .. } => {
                self.save_contacts();
            }
            Event::Received {
                from,
                content,
                snr4,
                ..
            } => {
                self.record(store::Message {
                    id: 0,
                    chat: Chat::Contact(*from),
                    outgoing: false,
                    author: None,
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                });
            }
            Event::GroupMessage {
                gid,
                from,
                content,
                snr4,
                ..
            } => {
                self.record(store::Message {
                    id: 0,
                    chat: Chat::Group(*gid),
                    outgoing: false,
                    author: Some(*from),
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                });
            }
            Event::Progress { id, sent, total } => {
                self.update(
                    *id,
                    Status::Sending {
                        sent: *sent,
                        total: *total,
                    },
                    false,
                );
            }
            Event::Delivered { id } => self.update(*id, Status::Delivered, true),
            Event::Failed { id, reason } => self.update(*id, Status::Failed(*reason), true),
            _ => {}
        }
    }
}

/// The lichen core with its storage, shared between threads.
#[derive(uniffi::Object)]
pub struct LichenCore {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for LichenCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LichenCore")
    }
}

impl LichenCore {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[uniffi::export]
impl LichenCore {
    /// `rng_seed`: 32 bytes from the OS random generator. `identity`: the node's 64-byte
    /// private key if the app stored it (Keychain), otherwise it is exported from the
    /// companion. `data_dir`: where the per-node databases live; without it, history is kept
    /// in memory only (demo).
    #[uniffi::constructor]
    pub fn new(
        app_name: String,
        rng_seed: Vec<u8>,
        identity: Option<Vec<u8>>,
        data_dir: Option<String>,
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
        let mut inner = Inner {
            core: Core::new(config),
            store: None,
            data_dir: data_dir.map(PathBuf::from),
            inflight: HashMap::new(),
            extra: VecDeque::new(),
        };
        // With a known identity the history is available before the node connects.
        if let Some(node) = inner.core.key().copied() {
            inner.open_store(&node);
        }
        Ok(Arc::new(Self {
            inner: Mutex::new(inner),
        }))
    }

    pub fn connect(&self, now_ms: u64) {
        let mut i = self.lock();
        i.core.connect(now_ms);
        i.persist();
    }

    pub fn on_companion_frame(&self, frame: Vec<u8>, now_ms: u64) {
        let mut i = self.lock();
        i.core.on_companion_frame(&frame, now_ms);
        i.persist();
    }

    /// The next frame to write to the companion, if one is due.
    pub fn poll_companion(&self, now_ms: u64) -> Option<Vec<u8>> {
        self.lock().core.poll_companion(now_ms)
    }

    pub fn tick(&self, now_ms: u64) {
        let mut i = self.lock();
        i.core.tick(now_ms);
        i.persist();
    }

    /// When to call `tick` (or `poll_companion`) next, in the same clock as `now_ms`.
    pub fn next_wakeup(&self) -> Option<u64> {
        self.lock().core.next_wakeup()
    }

    /// The next event. History changes come as `MessageChanged` right after the event that
    /// caused them.
    pub fn poll_event(&self) -> Option<LichenEvent> {
        let mut i = self.lock();
        if let Some(e) = i.extra.pop_front() {
            return Some(e);
        }
        let e = i.core.poll_event()?;
        i.on_event(&e);
        i.persist();
        let f = i.core.freq_khz();
        Some(LichenEvent::from_core(e, f))
    }

    pub fn key(&self) -> Option<Vec<u8>> {
        self.lock().core.key().map(|k| k.to_vec())
    }

    /// The node's private key, for the app to keep in the Keychain / Keystore.
    pub fn private_key(&self) -> Option<Vec<u8>> {
        self.lock().core.private_key().map(|k| k.to_vec())
    }

    pub fn radio(&self) -> Radio {
        let i = self.lock();
        let r = i.core.radio();
        Radio {
            freq_khz: i.core.freq_khz(),
            bw_hz: r.bw_hz,
            sf: r.sf,
            cr: r.cr,
        }
    }

    pub fn budget(&self, now_ms: u64) -> Budget {
        let (used_ms, limit_ms) = self.lock().core.budget(now_ms);
        Budget { used_ms, limit_ms }
    }

    pub fn contacts(&self) -> Vec<ContactInfo> {
        let mut v: Vec<ContactInfo> = self
            .lock()
            .core
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
        let i = self.lock();
        let mut v: Vec<GroupInfo> = i
            .core
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
        let (fragments, airtime_ms) = self.lock().core.estimate_send(&k, 5 + text_len as usize);
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    /// Sends a text and returns it as stored (status `Sending`).
    pub fn send_text(
        &self,
        to: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        let mut i = self.lock();
        let (fragments, airtime_ms) = i.core.estimate_send(&k, 5 + text.len());
        let id = i.core.send_text(&k, &text, sent_at, now_ms)?;
        let row = store::Message {
            id: 0,
            chat: Chat::Contact(k),
            outgoing: true,
            author: None,
            content: Content::Text { sent_at, text },
            at_ms: unix_ms(),
            status: Status::Sending {
                sent: 0,
                total: fragments,
            },
            fragments,
            airtime_ms,
            snr4: None,
            read: true,
        };
        let stored = i
            .store
            .as_ref()
            .and_then(|s| s.insert(&row).ok())
            .unwrap_or(row);
        if stored.id != 0 {
            i.inflight.insert(id, stored.id);
        }
        i.persist();
        Ok(stored.into())
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
        let mut i = self.lock();
        let g = i.core.create_group(&name, &keys, now_ms)?;
        i.persist();
        Ok(g.to_vec())
    }

    /// Floods a text to the group and returns it as stored (status `Flooded`).
    pub fn send_group_text(
        &self,
        gid: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.core.send_group_text(&g, &text, sent_at, now_ms)?;
        let row = store::Message {
            id: 0,
            chat: Chat::Group(g),
            outgoing: true,
            author: None,
            content: Content::Text { sent_at, text },
            at_ms: unix_ms(),
            status: Status::Flooded,
            fragments: 1,
            airtime_ms: 0,
            snr4: None,
            read: true,
        };
        let stored = i
            .store
            .as_ref()
            .and_then(|s| s.insert(&row).ok())
            .unwrap_or(row);
        i.persist();
        Ok(stored.into())
    }

    pub fn remove_member(
        &self,
        gid: Vec<u8>,
        member: Vec<u8>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let m = key(&member)?;
        let mut i = self.lock();
        i.core.remove_member(&g, &m, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Up to `limit` messages of `chat` before message `before_id`, oldest first.
    pub fn history(
        &self,
        chat: ChatRef,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        Ok(i.store
            .as_ref()
            .and_then(|s| s.history(&c, before_id, limit).ok())
            .unwrap_or_default()
            .into_iter()
            .map(StoredMessage::from)
            .collect())
    }

    /// Conversations that have messages, most recent first.
    pub fn conversations(&self) -> Vec<Conversation> {
        let i = self.lock();
        i.store
            .as_ref()
            .and_then(|s| s.conversations().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(m, unread)| Conversation {
                chat: ChatRef::from_store(&m.chat),
                last: m.into(),
                unread,
            })
            .collect()
    }

    pub fn mark_read(&self, chat: ChatRef) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        if let Some(s) = self.lock().store.as_ref() {
            let _ = s.mark_read(&c);
        }
        Ok(())
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
        let core = LichenCore::new("test".into(), vec![1; 32], None, None).unwrap();
        core.connect(0);
        let first = core.poll_companion(0).unwrap();
        assert_eq!(first[0], 1, "APP_START comes first");
        assert!(LichenCore::new("x".into(), vec![1; 31], None, None).is_err());
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
