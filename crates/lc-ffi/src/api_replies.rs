//! Replies, stickers, GIFs, reactions, edits, deletes and forwards.

use super::*;

#[uniffi::export]
impl LichenCore {
    /// Answers message `to_message_id` of `chat` (§6.2): a text that waits in the outbox like
    /// any other (§6.1). Refused for a message without a reference.
    pub fn send_reply(
        &self,
        chat: ChatRef,
        to_message_id: i64,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let c = chat.to_store()?;
        if text.is_empty() {
            return Err(LichenError::Empty);
        }
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let target = i
            .store
            .as_ref()
            .and_then(|s| s.get(to_message_id).ok().flatten())
            .filter(|m| m.chat == c)
            .and_then(|m| m.msg_ref)
            .ok_or(LichenError::NotReferable)?;
        let content = Content::Reply {
            sent_at,
            target,
            text,
        };
        let datagram = content.encode();
        let (id, estimate) = match c {
            // Lichen content never goes into a plain MeshCore chat (§15).
            Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_) => {
                return Err(LichenError::NotAllowed);
            }
            Chat::Contact(k) => (
                i.core.send_datagram(&k, datagram.clone(), now_ms)?,
                i.core.estimate_send(&k, datagram.len()),
            ),
            Chat::Group(g) => (
                i.core.send_group_datagram(&g, &datagram, now_ms)?,
                i.core.estimate_group_send(datagram.len()),
            ),
        };
        i.stored_outgoing(c, content, id, estimate, true, None)
    }

    /// Sends a sticker (§6.3): a message like a text, through the outbox (§6.1), to a contact
    /// or a group. `emoji` names a Noto Animated Emoji the apps ship.
    pub fn send_sticker(
        &self,
        chat: ChatRef,
        emoji: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        if emoji.is_empty() || emoji.len() > lc_core::content::STICKER_MAX {
            return Err(LichenError::TooLarge);
        }
        self.send_message(
            chat,
            Content::Sticker {
                sent_at,
                set: lc_core::content::STICKER_SET_NOTO,
                emoji,
            },
            now_ms,
        )
    }

    /// Sends a GIPHY GIF by its id (§6.3): only the id, its shape and title travel. A message
    /// like a text, through the outbox (§6.1), to a contact or a group.
    #[allow(clippy::too_many_arguments)]
    pub fn send_gif(
        &self,
        chat: ChatRef,
        id: String,
        width: u16,
        height: u16,
        title: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let mut title = title;
        while title.len() > lc_core::content::GIF_TITLE_MAX {
            title.pop();
        }
        let content = Content::Gif {
            sent_at,
            provider: lc_core::content::GIF_GIPHY,
            width,
            height,
            id,
            title,
        };
        // What a receiver would refuse is not sent (an id that is not letters and digits).
        if Content::decode(&content.encode()).as_ref() != Some(&content) {
            return Err(LichenError::NotAllowed);
        }
        self.send_message(chat, content, now_ms)
    }

    /// Reacts to message `message_id` with `emoji` (empty removes our reaction), §6.2: shown
    /// at once, sent, and sent again after a restart until delivered (24 h at most). Returns
    /// the message with its reactions.
    pub fn send_reaction(
        &self,
        message_id: i64,
        emoji: String,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        if emoji.len() > REACTION_MAX {
            return Err(LichenError::TooLarge);
        }
        let mut i = self.lock();
        i.last_now = now_ms;
        let me = *i.core.key().ok_or(LichenError::NoIdentity)?;
        let m = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        let target = m.msg_ref.ok_or(LichenError::NotReferable)?;
        let chat = m.chat;
        // Each change goes after the previous one, even within the same second.
        let previous = i
            .store
            .as_ref()
            .and_then(|s| s.reaction(&chat, &target, &me).ok().flatten())
            .map_or(0, |(_, ts)| ts);
        let ts = u32::try_from(unix_ms() / 1000)
            .unwrap_or(u32::MAX)
            .max(previous.saturating_add(1));
        let expires = unix_ms() + i.outbox_expiry_ms;
        if let Some(st) = &i.store {
            st.upsert_reaction(&chat, &target, &me, &emoji, ts, true, Some(expires))
                .map_err(|_| LichenError::Storage)?;
        }
        i.submit_reaction(chat, target, &emoji, ts);
        i.persist();
        let row = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        Ok(i.present(row))
    }

    /// Changes the text of our message `message_id` (§6.4): a text or a reply, within 24
    /// hours. Applied here at once, sent like a reaction.
    pub fn send_edit(
        &self,
        message_id: i64,
        text: String,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        if text.trim().is_empty() {
            return Err(LichenError::Empty);
        }
        self.lock().amend(message_id, Some(text), now_ms)
    }

    /// Takes our message `message_id` back on every phone that receives this (§6.4), within
    /// 24 hours. Its place stays here as "message deleted".
    pub fn send_delete(&self, message_id: i64, now_ms: u64) -> Result<StoredMessage, LichenError> {
        self.lock().amend(message_id, None, now_ms)
    }

    /// Forwards message `message_id` to `to` (ADR-0027): a new message of ours with the same
    /// content, through the usual send paths and their checks. A text (an edited one as it
    /// is now, a reply as a plain text), a sticker or a GIF; a small file or a voice message to
    /// a contact. A MeshCore chat takes text only. Nothing marks it as forwarded.
    pub fn forward(
        &self,
        message_id: i64,
        to: ChatRef,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let current = {
            let i = self.lock();
            let st = i.store.as_ref().ok_or(LichenError::Storage)?;
            let m = st
                .get(message_id)
                .ok()
                .flatten()
                .ok_or(LichenError::Storage)?;
            i.present(m).message
        };
        match (current, to.to_store()?) {
            (Message::Text { text, .. }, Chat::Contact(k)) => {
                self.send_text(k.to_vec(), text, sent_at, now_ms)
            }
            (Message::Text { text, .. }, Chat::Group(g)) => {
                self.send_group_text(g.to_vec(), text, sent_at, now_ms)
            }
            (Message::Text { text, .. }, Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_)) => {
                self.send_plain(to, text, sent_at, now_ms)
            }
            (Message::Sticker { emoji, .. }, Chat::Contact(_) | Chat::Group(_)) => {
                self.send_sticker(to, emoji, sent_at, now_ms)
            }
            (
                Message::Gif {
                    id,
                    width,
                    height,
                    title,
                    ..
                },
                Chat::Contact(_) | Chat::Group(_),
            ) => self.send_gif(to, id, width, height, title, sent_at, now_ms),
            (Message::File { name, data, .. }, Chat::Contact(k)) => {
                self.send_file(k.to_vec(), name, data, sent_at, now_ms)
            }
            (
                Message::Voice {
                    mode,
                    frames,
                    duration_ms,
                    data,
                    ..
                },
                Chat::Contact(k),
            ) => self.send_voice(
                k.to_vec(),
                PreparedVoice {
                    mode,
                    frames,
                    duration_ms,
                    data,
                },
                sent_at,
                now_ms,
            ),
            _ => Err(LichenError::NotAllowed),
        }
    }

    /// Removes message `message_id` from this phone only (§6.4); nothing is sent.
    pub fn delete_for_me(&self, message_id: i64) -> Result<(), LichenError> {
        let mut i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        if st
            .get(message_id)
            .ok()
            .flatten()
            .is_some_and(|m| m.status == Status::Queued)
        {
            // A waiting message is cancelled, not just hidden.
            drop(i);
            return self.cancel_message(message_id);
        }
        let gone = st
            .delete_for_me(message_id)
            .map_err(|_| LichenError::Storage)?;
        if let Some(m) = gone {
            i.media_gone(&m.content);
        }
        i.persist();
        Ok(())
    }

    /// What a reaction costs us in `chat` (§6.2): fragments and the airtime of our own
    /// transmissions (a group's two floods); relays and acknowledgements add theirs.
    pub fn estimate_reaction(&self, chat: ChatRef) -> Result<Estimate, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        // ts, reference and a 4-byte emoji.
        let len = 1 + 4 + 8 + 4;
        let (fragments, airtime_ms) = match c {
            // Lichen content never goes into a plain MeshCore chat (§15).
            Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_) => {
                return Err(LichenError::NotAllowed);
            }
            Chat::Contact(k) => i.core.estimate_send(&k, len),
            Chat::Group(_) => i.core.estimate_group_send(len),
        };
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    /// Asks the sender of image preview `message_id` for its full variant.
    pub fn request_full_image(&self, message_id: i64, now_ms: u64) -> Result<(), LichenError> {
        let mut i = self.lock();
        let m = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::NotAPreview)?;
        let (
            Chat::Contact(from),
            Content::Image {
                media_id,
                full: false,
                full_len,
                ..
            },
        ) = (m.chat, &m.content)
        else {
            return Err(LichenError::NotAPreview);
        };
        if m.outgoing || *full_len == 0 {
            return Err(LichenError::NotAPreview);
        }
        let request = Content::MediaRequest {
            media_id: *media_id,
            full: true,
        };
        i.core.send_datagram(&from, request.encode(), now_ms)?;
        Ok(())
    }
}
