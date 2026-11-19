//! The outbox and what is written to the store.

use super::*;

impl Inner {
    /// Expires overdue texts and tells the app when an unfinished text's status changed
    /// (§6.1).
    pub(crate) fn refresh_outbox(&mut self) {
        if self.outbox.is_empty() {
            return;
        }
        let now_unix = self.wall_ms();
        let rows: Vec<i64> = self.outbox.keys().copied().collect();
        for row in rows {
            let Some(p) = self.outbox.get(&row).cloned() else {
                continue;
            };
            let live = self.core.outgoing(p.core_id, self.last_now);
            let transmitting = live.is_some_and(|o| o.stage == Stage::Transmitting);
            if p.expires_ms <= now_unix && !transmitting {
                self.outbox.remove(&row);
                // Found: its `Failed` event updates the row; otherwise do it here.
                if !self.core.abandon(p.core_id) {
                    self.inflight.remove(&p.core_id);
                    if let Some(m) = self.store.as_ref().and_then(|s| {
                        s.set_status(row, Status::Failed(CoreFail::Expired))
                            .ok()
                            .flatten()
                    }) {
                        let message = self.present(m);
                        self.extra
                            .push_back(LichenEvent::MessageChanged { message });
                    }
                }
                continue;
            }
            let status = self.live_status(&p, live);
            if status == p.presented {
                continue;
            }
            if let Some(m) = self.store.as_ref().and_then(|s| s.get(row).ok().flatten()) {
                let mut out = self.present(m);
                out.status = status.clone();
                self.extra
                    .push_back(LichenEvent::MessageChanged { message: out });
            }
            if let Some(q) = self.outbox.get_mut(&row) {
                q.presented = status;
            }
        }
    }

    /// A TEXT or VOICE already stored from the same sender in `chat`: a resend (§6.1).
    pub(crate) fn is_resend(
        &self,
        chat: &Chat,
        author: Option<&PubKey>,
        content: &Content,
    ) -> bool {
        matches!(
            content,
            Content::Text { .. }
                | Content::Reply { .. }
                | Content::Voice { .. }
                | Content::Sticker { .. }
                | Content::Gif { .. }
        ) && self
            .store
            .as_ref()
            .is_some_and(|s| s.has_duplicate(chat, author, content).unwrap_or(false))
    }

    /// The node of identity `id` (the app knows people by node): a group member or a contact.
    pub(crate) fn node_of(&self, id: &PubKey) -> Option<PubKey> {
        if Some(id) == self.core.key() {
            return self.core.node_key().copied();
        }
        self.core
            .groups()
            .flat_map(|g| g.members.iter())
            .find(|m| m.key == *id)
            .map(|m| m.node)
            .or_else(|| {
                self.core
                    .contacts()
                    .find(|c| c.identity == Some(*id))
                    .map(|c| c.key)
            })
    }

    pub(crate) fn save_contacts(&mut self) {
        self.note_renames();
        if let Some(st) = &self.store {
            for c in self.core.contacts() {
                let _ = st.put_contact(c);
            }
        }
    }

    pub(crate) fn save_shares(&self) {
        if let Some(st) = &self.store {
            let _ = st.put_shares(self.core.shares());
        }
    }

    /// Saves the groups that changed, and follows the outbox.
    pub(crate) fn persist(&mut self) {
        self.refresh_outbox();
        self.vanish();
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
        for g in self.core.take_dirty_invitations() {
            let _ = match self.core.invitation_snapshot(&g) {
                Some(snap) => st.put_invitation(&g, &snap),
                None => st.delete_invitation(&g),
            };
        }
    }

    /// Stores a message we just queued and tracks its delivery. A text (`outbox`) waits in
    /// the outbox until it is delivered, fails or expires (§6.1).
    pub(crate) fn stored_outgoing(
        &mut self,
        chat: Chat,
        content: Content,
        id: u64,
        (fragments, airtime_ms): (u8, u64),
        outbox: bool,
        wire: Option<&[u8]>,
    ) -> Result<StoredMessage, LichenError> {
        let msg_ref = self.reference(&chat, true, None, wire.unwrap_or(&content.encode()));
        let status = if outbox {
            Status::Queued
        } else {
            Status::Sending {
                sent: 0,
                total: fragments,
            }
        };
        let row = store::Message {
            id: 0,
            chat,
            outgoing: true,
            author: None,
            content,
            at_ms: self.wall_ms(),
            status,
            fragments,
            airtime_ms,
            snr4: None,
            read: true,
            msg_ref,
        };
        // Accepted means stored (recheck R02): a message the phone cannot keep is not sent.
        let Some(stored) = self.store.as_ref().and_then(|s| s.insert(&row).ok()) else {
            let _ = self.core.cancel(id);
            return Err(LichenError::Storage);
        };
        if outbox {
            let expires = stored.at_ms + self.outbox_expiry_ms;
            if let Some(st) = &self.store {
                let _ = st.set_expiry(stored.id, Some(expires));
            }
            self.track(stored.id, id, chat, expires, status, fragments);
        } else {
            self.inflight.insert(id, stored.id);
        }
        self.persist();
        Ok(self.present(stored))
    }

    /// Nothing is sent that the phone could not keep (recheck R02): the database must be open.
    pub(crate) fn writable(&self) -> Result<(), LichenError> {
        if self.store.is_some() {
            Ok(())
        } else {
            Err(LichenError::Storage)
        }
    }

    /// Our edit (`text`) or delete (`None`) of message `message_id` (§6.4).
    pub(crate) fn amend(
        &mut self,
        message_id: i64,
        text: Option<String>,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        self.last_now = now_ms;
        let st = self.store.as_ref().ok_or(LichenError::Storage)?;
        let m = st
            .get(message_id)
            .ok()
            .flatten()
            .ok_or(LichenError::Storage)?;
        let out = self.present(m.clone());
        let allowed = if text.is_some() {
            out.editable
        } else {
            out.deletable
        };
        if !allowed {
            return Err(LichenError::NotAllowed);
        }
        let target = m.msg_ref.ok_or(LichenError::NotReferable)?;
        let st = self.store.as_ref().ok_or(LichenError::Storage)?;
        // Each change goes after the previous one, even within the same second.
        let previous = st.amendment_for(&m).ok().flatten().map_or(0, |a| a.ts);
        let ts = u32::try_from(self.wall_ms() / 1000)
            .unwrap_or(u32::MAX)
            .max(previous.saturating_add(1));
        let expires = self.wall_ms() + self.outbox_expiry_ms;
        st.amend(
            &m.chat,
            &target,
            None,
            text.as_deref(),
            ts,
            true,
            Some(expires),
        )
        .map_err(|_| LichenError::Storage)?;
        let before = st.get(message_id).ok().flatten().map(|m| m.content);
        let row = if text.is_none() {
            st.tombstone(message_id)
                .ok()
                .flatten()
                .ok_or(LichenError::Storage)?
        } else {
            st.get(message_id)
                .ok()
                .flatten()
                .ok_or(LichenError::Storage)?
        };
        let deleted = text.is_none();
        self.submit_amendment(m.chat, target, text, ts);
        if deleted && let Some(c) = before {
            self.media_gone(&c);
        }
        self.persist();
        Ok(self.present(row))
    }

    /// Joins a channel (§15.3); a new one starts muted.
    pub(crate) fn join_channel(
        &mut self,
        name: &str,
        secret: [u8; 16],
    ) -> Result<ChatRef, LichenError> {
        let known = self.core.channels().iter().any(|c| c.secret == secret);
        let id = self.core.join_channel(name, secret)?;
        let chat = Chat::Channel(id);
        if !known && let Some(st) = &self.store {
            let _ = st.set_muted(&chat, Some(i64::MAX));
        }
        self.persist();
        Ok(ChatRef::from_store(&chat))
    }

    pub(crate) fn record(&mut self, m: store::Message) -> Option<StoredMessage> {
        let st = self.store.as_ref()?;
        let mut row = st.insert(&m).ok()?;
        // Its author took it back before it arrived (§6.4).
        if st
            .amendment_for(&row)
            .ok()
            .flatten()
            .is_some_and(|a| a.deleted)
        {
            row = st.tombstone(row.id).ok().flatten()?;
        }
        // A message received in an archived conversation brings it back to the list, unless
        // it is muted (ADR-0034).
        if !m.outgoing
            && !matches!(m.content, Content::Notice(_))
            && st
                .unarchive_on_message(&m.chat, self.wall_ms())
                .unwrap_or(false)
        {
            self.extra.push_back(LichenEvent::ChatPrefsChanged);
        }
        let out = self.present(row);
        self.extra.push_back(LichenEvent::MessageChanged {
            message: out.clone(),
        });
        Some(out)
    }

    pub(crate) fn update(&mut self, core_id: u64, status: Status, done: bool) {
        let Some(&row) = self.inflight.get(&core_id) else {
            return;
        };
        if done {
            self.inflight.remove(&core_id);
            self.outbox.remove(&row);
        } else if let Some(p) = self.outbox.get_mut(&row) {
            p.stored = status;
        }
        if let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.set_status(row, status).ok().flatten())
        {
            let out = self.present(m);
            if let Some(p) = self.outbox.get_mut(&row) {
                p.presented = out.status.clone();
            }
            self.extra
                .push_back(LichenEvent::MessageChanged { message: out });
        }
    }
}
