//! Lifecycle: the store, the radio link, time, events, the outbox's cancel and retry, adverts.

use super::*;

#[uniffi::export]
impl LichenCore {
    /// `rng_seed`: 32 bytes from the OS random generator. `identity_seed`: the phone
    /// identity's 32-byte seed (ADR-0013), drawn once by the app and kept in the Keychain /
    /// Keystore. `data_dir`: where the identity's database lives; without it, history is kept
    /// in memory only (demo).
    #[uniffi::constructor]
    pub fn new(
        app_name: String,
        rng_seed: Vec<u8>,
        identity_seed: Vec<u8>,
        data_dir: Option<String>,
    ) -> Result<Arc<Self>, LichenError> {
        let rng: [u8; 32] = rng_seed
            .as_slice()
            .try_into()
            .map_err(|_| LichenError::InvalidKey)?;
        let seed: [u8; 32] = identity_seed
            .as_slice()
            .try_into()
            .map_err(|_| LichenError::InvalidKey)?;
        let mut inner = Inner {
            core: Core::new(Config::new(&app_name, seed, rng)),
            seed,
            store: None,
            data_dir: data_dir.map(PathBuf::from),
            inflight: HashMap::new(),
            nicknames: HashMap::new(),
            extra: VecDeque::new(),
            last_now: 0,
            outbox: HashMap::new(),
            outbox_expiry_ms: OUTBOX_EXPIRY_MS,
            clock_offset: None,
            reaction_inflight: HashMap::new(),
            amend_inflight: HashMap::new(),
        };
        // The history is available before the node connects.
        if let Some(id) = inner.core.key().copied() {
            inner.open_store(&id);
        }
        Ok(Arc::new(Self {
            inner: Mutex::new(inner),
        }))
    }

    /// False when the phone's message database could not be opened (recheck R02): nothing can
    /// be sent or kept, and the app says so instead of running without it.
    pub fn storage_ok(&self) -> bool {
        self.lock().store.is_some()
    }

    pub fn connect(&self, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.connect(now_ms);
        i.persist();
    }

    /// The companion is gone (Bluetooth or USB lost): nothing goes out, texts wait and a
    /// transfer in progress pauses until `connect` (§6.1).
    pub fn disconnect(&self, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.disconnect(now_ms);
        i.persist();
    }

    /// Cancels a text that is still waiting and none of whose frames went out; it is deleted
    /// (§6.1).
    pub fn cancel_message(&self, message_id: i64) -> Result<(), LichenError> {
        let mut i = self.lock();
        let p = i
            .outbox
            .get(&message_id)
            .cloned()
            .filter(|p| p.stored == Status::Queued)
            .ok_or(LichenError::NotCancellable)?;
        i.core.cancel(p.core_id)?;
        i.outbox.remove(&message_id);
        i.inflight.remove(&p.core_id);
        if let Some(st) = &i.store {
            st.delete_message(message_id)
                .map_err(|_| LichenError::Storage)?;
        }
        Ok(())
    }

    /// Sends a failed text or voice message again: the same message (row and plaintext), so
    /// that a receiver who already has it drops the copy (§6.1). A text waits again for 24 h;
    /// a voice message is refused when over budget.
    pub fn retry_message(
        &self,
        message_id: i64,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let mut i = self.lock();
        i.last_now = now_ms;
        let m = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        if !m.outgoing || !matches!(m.status, Status::Failed(_)) {
            return Err(LichenError::NotAllowed);
        }
        let datagram = m.content.encode();
        let text = matches!(
            m.content,
            Content::Text { .. }
                | Content::Reply { .. }
                | Content::Sticker { .. }
                | Content::Gif { .. }
        );
        let id = match (&m.chat, &m.content) {
            (Chat::Contact(k), _) if text => i.core.send_datagram(k, datagram, now_ms)?,
            (Chat::Group(g), _) if text => i.core.send_group_datagram(g, &datagram, now_ms)?,
            (Chat::Contact(k), Content::Voice { .. }) => i.core.send_media(k, datagram, now_ms)?,
            // A plain text goes again as it was (§15.2): same timestamp, so a receiver that
            // got it already sees a retry.
            (Chat::Mesh(k), Content::Text { sent_at, text }) => {
                i.core.send_plain(k, text, *sent_at, now_ms)?
            }
            (Chat::Channel(c), Content::Text { sent_at, text }) => {
                let id = i.core.send_channel(c, text, *sent_at, now_ms)?;
                if let Some(st) = &i.store {
                    st.set_status(m.id, Status::Sending { sent: 0, total: 1 })
                        .map_err(|_| LichenError::Storage)?;
                }
                i.inflight.insert(id, m.id);
                i.persist();
                let row = i
                    .store
                    .as_ref()
                    .and_then(|s| s.get(m.id).ok().flatten())
                    .ok_or(LichenError::Storage)?;
                return Ok(i.present(row));
            }
            _ => return Err(LichenError::NotAllowed),
        };
        let status = if text {
            Status::Queued
        } else {
            Status::Sending {
                sent: 0,
                total: m.fragments,
            }
        };
        let expires = text.then(|| unix_ms() + i.outbox_expiry_ms);
        if let Some(st) = &i.store {
            st.set_status(m.id, status)
                .map_err(|_| LichenError::Storage)?;
            st.set_expiry(m.id, expires)
                .map_err(|_| LichenError::Storage)?;
        }
        match expires {
            Some(e) => i.track(m.id, id, m.chat, e, status, m.fragments),
            None => {
                i.inflight.insert(id, m.id);
            }
        }
        i.persist();
        let row = i
            .store
            .as_ref()
            .and_then(|s| s.get(m.id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        Ok(i.present(row))
    }

    /// Tests: how long a text waits before it expires (default 24 h).
    pub fn set_outbox_expiry(&self, ms: u64) {
        self.lock().outbox_expiry_ms = i64::try_from(ms).unwrap_or(i64::MAX);
    }

    pub fn on_companion_frame(&self, frame: Vec<u8>, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.on_companion_frame(&frame, now_ms);
        i.persist();
    }

    /// The next frame to write to the companion, if one is due.
    pub fn poll_companion(&self, now_ms: u64) -> Option<Vec<u8>> {
        self.lock().core.poll_companion(now_ms)
    }

    pub fn tick(&self, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.tick(now_ms);
        i.persist();
    }

    /// When to call `tick` (or `poll_companion`) next, in the same clock as `now_ms`.
    pub fn next_wakeup(&self) -> Option<u64> {
        let i = self.lock();
        let core = i.core.next_wakeup();
        // The next text to expire (§6.1); one already overdue waits for its transfer to end.
        let now_unix = unix_ms();
        let offset = i.offset();
        let expiry = i
            .outbox
            .values()
            .map(|p| p.expires_ms)
            .filter(|&e| e > now_unix)
            .min()
            .map(|e| u64::try_from(e - offset).unwrap_or(0));
        match (core, expiry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The next event. History changes come as `MessageChanged` right after the event that
    /// caused them.
    pub fn poll_event(&self) -> Option<LichenEvent> {
        let mut i = self.lock();
        let e = loop {
            if let Some(e) = i.extra.pop_front() {
                return Some(e);
            }
            let e = i.core.poll_event()?;
            let keep = i.on_event(&e);
            i.persist();
            if keep {
                break e;
            }
        };
        let f = i.core.freq_khz();
        let core = &i.core;
        Some(LichenEvent::from_core(e, f, |gid, admin| {
            core.groups()
                .find(|g| g.gid == *gid)
                .and_then(|g| g.members.iter().find(|m| m.key == *admin))
                .map(|m| m.node)
                .or_else(|| {
                    core.invitations()
                        .iter()
                        .find(|i| i.gid() == gid)
                        .and_then(|i| i.admin_node())
                })
                .unwrap_or(*admin)
        }))
    }

    /// Announces our node (a MeshCore advert) so that nearby radios add it as a contact;
    /// `flood` lets repeaters relay it.
    pub fn send_advert(&self, flood: bool, now_ms: u64) {
        let mut i = self.lock();
        i.core.send_advert(flood, now_ms);
    }
}
