//! Stored conversations: the list, pages, search, preferences, read marks, deletion.

use super::*;

#[uniffi::export]
impl LichenCore {
    /// Messages of `chat` disappear `seconds` after they are stored, from now on; 0: they stay
    /// (§6.5). The other side of a lichen conversation, or the group, gets the same timer; a
    /// MeshCore chat keeps it on this phone.
    pub fn set_disappearing(&self, chat: ChatRef, seconds: u32) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        self.lock().set_timer(c, seconds)
    }

    /// `chat`'s timer, seconds (0: messages stay).
    pub fn disappearing(&self, chat: ChatRef) -> Result<u32, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        Ok(st.timer(&c).map_err(|_| LichenError::Storage)?.0)
    }

    /// Up to `limit` messages of `chat` before message `before_id`, oldest first.
    /// The messages from `from_id` (included) to `before_id` (excluded), oldest first: to show a
    /// search result or a quoted message older than what is loaded, in one read (recheck R08).
    /// 20,000 at most; empty when `from_id` is no longer there is up to the caller to notice.
    pub fn history_from(
        &self,
        chat: ChatRef,
        from_id: i64,
        before_id: i64,
    ) -> Result<Vec<StoredMessage>, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let Some(store) = i.store.as_ref() else {
            return Ok(Vec::new());
        };
        Ok(store
            .history_from(&c, from_id, before_id, 20_000)
            .map_err(|_| LichenError::Storage)?
            .into_iter()
            .map(|m| i.present(m))
            .collect())
    }

    pub fn history(
        &self,
        chat: ChatRef,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        // No database (the demo, a throwaway core): no history. A database that fails: say so,
        // not an empty conversation.
        let Some(store) = i.store.as_ref() else {
            return Ok(Vec::new());
        };
        Ok(store
            .history(&c, before_id, limit)
            .map_err(|_| LichenError::Storage)?
            .into_iter()
            .map(|m| i.present(m))
            .collect())
    }

    /// Demo and screenshots only: `count` older text messages in `chat`, alternately ours and
    /// theirs, a minute apart and before everything else, the last five of theirs unread.
    /// Refused outside the in-memory database of the demo (no data dir).
    pub fn demo_fill_history(&self, chat: ChatRef, count: u32) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        if i.data_dir.is_some() {
            return Err(LichenError::NotAllowed);
        }
        let store = i.store.as_ref().ok_or(LichenError::Storage)?;
        let start = unix_ms() - i64::from(count) * 60_000 - 3_600_000;
        for n in 0..count {
            let at_ms = start + i64::from(n) * 60_000;
            let outgoing = n % 2 == 0;
            let content = Content::Text {
                sent_at: u32::try_from(at_ms / 1000).unwrap_or(0),
                text: format!("Earlier message {}", n + 1),
            };
            let msg_ref = i.reference(&c, outgoing, None, &content.encode());
            store
                .insert(&store::Message {
                    id: 0,
                    chat: c,
                    outgoing,
                    author: None,
                    content,
                    at_ms,
                    status: if outgoing {
                        Status::Delivered
                    } else {
                        Status::Received
                    },
                    fragments: 1,
                    airtime_ms: 0,
                    snr4: None,
                    // The last five of theirs are left unread (the divider).
                    read: outgoing || n + 10 < count,
                    msg_ref,
                })
                .map_err(|_| LichenError::Storage)?;
        }
        Ok(())
    }

    /// Text messages containing `query`, case and accents aside, newest first, in every
    /// conversation (on this phone only). Under 2 characters: nothing.
    pub fn search_messages(&self, query: String, limit: u32) -> Vec<StoredMessage> {
        if query.trim().chars().count() < 2 {
            return Vec::new();
        }
        let i = self.lock();
        i.store
            .as_ref()
            .and_then(|s| s.search(&query, limit as usize).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|m| i.present(m))
            .collect()
    }

    /// Pinned, muted and archived conversations.
    pub fn chat_prefs(&self) -> Vec<ChatPrefs> {
        self.lock()
            .store
            .as_ref()
            .and_then(|s| s.chat_prefs().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|p| ChatPrefs {
                chat: ChatRef::from_store(&p.chat),
                pinned_at_ms: p.pinned_at,
                muted_until_ms: p.muted_until,
                archived: p.archived,
            })
            .collect()
    }

    /// Archives a conversation (out of the list, under "Archived") or brings it back. A
    /// message received in it brings it back, unless it is muted (ADR-0034).
    pub fn set_archived(&self, chat: ChatRef, archived: bool) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.set_archived(&c, archived)
            .map_err(|_| LichenError::Storage)
    }

    /// Pins a conversation at the top of the list (the latest pinned first), or unpins it.
    pub fn set_pinned(&self, chat: ChatRef, pinned: bool) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.set_pinned(&c, pinned.then(unix_ms))
            .map_err(|_| LichenError::Storage)
    }

    /// Mutes a conversation's notifications until `until_ms` (Unix ms, `i64::MAX`: until the
    /// user turns them back on), or unmutes it (`None`). SOS alerts are never muted.
    pub fn set_muted(&self, chat: ChatRef, until_ms: Option<i64>) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.set_muted(&c, until_ms).map_err(|_| LichenError::Storage)
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
                last: i.present(m),
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

    /// Deletes a conversation's messages (and the media kept for it). The contact or the
    /// group stays: a new message starts the conversation again.
    pub fn delete_conversation(&self, chat: ChatRef) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        // Its waiting texts stop too.
        let waiting: Vec<(i64, u64)> = i
            .outbox
            .iter()
            .filter(|(_, p)| p.chat == c)
            .map(|(row, p)| (*row, p.core_id))
            .collect();
        for (row, core_id) in waiting {
            i.outbox.remove(&row);
            i.inflight.remove(&core_id);
            if i.core.cancel(core_id).is_err() {
                i.core.abandon(core_id);
            }
        }
        let hashes = i
            .store
            .as_ref()
            .and_then(|s| s.announced_hashes(&c).ok())
            .unwrap_or_default();
        if let Some(s) = i.store.as_ref() {
            let _ = s.delete_chat(&c);
        }
        // Its announcements: offers and downloads go too (SR-28).
        for hash in hashes {
            i.extra.push_back(LichenEvent::MediaGone {
                hash: hash.to_vec(),
            });
        }
        Ok(())
    }
}
