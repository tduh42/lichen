//! MeshCore chats (protocol.md §15): plain texts and channels.

use super::*;

#[uniffi::export]
impl LichenCore {
    // ----- MeshCore chats (§15, ADR-0026) ---------------------------------------------

    /// Sends a plain MeshCore text to `chat`, a node (`Mesh`) or a channel (§15.2, §15.3).
    /// A direct one waits for the radio like a lichen text (§6.1); a channel post needs the
    /// radio now.
    pub fn send_plain(
        &self,
        chat: ChatRef,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let content = Content::Text {
            sent_at,
            text: text.clone(),
        };
        match c {
            Chat::Mesh(k) => {
                let airtime = i.core.estimate_plain(&PlainChat::Direct(k), text.len());
                let id = i.core.send_plain(&k, &text, sent_at, now_ms)?;
                i.stored_outgoing(c, content, id, (1, airtime), true, None)
            }
            Chat::Channel(ch) => {
                let airtime = i.core.estimate_plain(&PlainChat::Channel(ch), text.len());
                let id = i.core.send_channel(&ch, &text, sent_at, now_ms)?;
                i.stored_outgoing(c, content, id, (1, airtime), false, None)
            }
            // A post to a room server is a direct message to it (§15.8).
            Chat::Room(k) => {
                let airtime = i.core.estimate_plain(&PlainChat::Room(k), text.len());
                let id = i.core.send_plain(&k, &text, sent_at, now_ms)?;
                i.stored_outgoing(c, content, id, (1, airtime), true, None)
            }
            Chat::Contact(_) | Chat::Group(_) => Err(LichenError::NotAllowed),
        }
    }

    /// Answers MeshCore message `message_id` (a channel post, a MeshCore chat's message, a
    /// room's post, §15.9): sends `@[<its sender's name>] ` and `text` to its conversation, as
    /// `send_plain` does, and keeps that the copy answers it (its `quote` is that message). The
    /// mention counts in the length (`plain_text_max`). `NotReferable` for a message whose
    /// sender has no name that a mention can hold; `NotAllowed` outside MeshCore chats.
    pub fn send_plain_reply(
        &self,
        message_id: i64,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let (chat, full) = {
            let i = self.lock();
            let st = i.store.as_ref().ok_or(LichenError::Storage)?;
            let m = st
                .get(message_id)
                .map_err(|_| LichenError::Storage)?
                .ok_or(LichenError::NotReferable)?;
            if !matches!(m.chat, Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_)) {
                return Err(LichenError::NotAllowed);
            }
            let name = i.plain_sender(&m).ok_or(LichenError::NotReferable)?;
            let full =
                lc_core::mention::reply_text(&name, &text).ok_or(LichenError::NotReferable)?;
            (ChatRef::from_store(&m.chat), full)
        };
        let sent = self.send_plain(chat, full, sent_at, now_ms)?;
        let i = self.lock();
        let Some(st) = i.store.as_ref() else {
            return Ok(sent);
        };
        st.set_plain_reply(sent.id, message_id)
            .map_err(|_| LichenError::Storage)?;
        Ok(st
            .get(sent.id)
            .ok()
            .flatten()
            .map_or(sent, |m| i.present(m)))
    }

    /// A MeshCore text cut into spans (§15.9): mentions (ours marked), links, places and the
    /// text between, so that the apps parse nothing.
    pub fn meshcore_spans(&self, text: String) -> Vec<TextSpan> {
        crate::tools::meshcore_spans(&text, &self.lock().our_name())
    }

    /// Airtime of a plain text of `len` bytes to `chat` (§15.5).
    pub fn estimate_plain(&self, chat: ChatRef, len: u32) -> Result<Estimate, LichenError> {
        let plain = match chat.to_store()? {
            Chat::Mesh(k) => PlainChat::Direct(k),
            Chat::Channel(c) => PlainChat::Channel(c),
            Chat::Room(k) => PlainChat::Room(k),
            _ => return Err(LichenError::NotAllowed),
        };
        Ok(Estimate {
            fragments: 1,
            airtime_ms: self.lock().core.estimate_plain(&plain, len as usize),
        })
    }

    /// The longest text for `chat`, in bytes: 160, less our name on a channel (§15.3).
    pub fn plain_text_max(&self, chat: ChatRef) -> Result<u32, LichenError> {
        let i = self.lock();
        let max = match chat.to_store()? {
            Chat::Mesh(_) | Chat::Room(_) => lc_core::plain::TEXT_MAX,
            Chat::Channel(_) => i.core.channel_text_max(),
            _ => return Err(LichenError::NotAllowed),
        };
        Ok(u32::try_from(max).unwrap_or(0))
    }

    /// The radio's channels (§15.3); the ones saved last while it is away.
    pub fn channels(&self) -> Vec<MeshChannel> {
        self.lock()
            .channel_list()
            .iter()
            .map(MeshChannel::from)
            .collect()
    }

    /// Joins MeshCore's public channel (§15.3), muted.
    pub fn join_public_channel(&self) -> Result<ChatRef, LichenError> {
        self.lock()
            .join_channel(lc_core::plain::PUBLIC_NAME, lc_core::plain::PUBLIC_SECRET)
    }

    /// Joins hashtag channel `tag` (`#hiking` or `hiking`), muted.
    pub fn join_hashtag_channel(&self, tag: String) -> Result<ChatRef, LichenError> {
        let (name, secret) = lc_core::plain::hashtag(&tag).ok_or(LichenError::BadCode)?;
        self.lock().join_channel(&name, secret)
    }

    /// Joins a private channel from its name and 32 hex digits, muted.
    pub fn join_private_channel(
        &self,
        name: String,
        secret_hex: String,
    ) -> Result<ChatRef, LichenError> {
        let secret = secret_from_hex(&secret_hex).ok_or(LichenError::BadCode)?;
        self.lock().join_channel(name.trim(), secret)
    }

    /// Leaves channel `id` (its history stays until the conversation is deleted).
    pub fn leave_channel(&self, id: Vec<u8>) -> Result<(), LichenError> {
        let id = channel(&id)?;
        let mut i = self.lock();
        i.core.leave_channel(&id)?;
        i.persist();
        Ok(())
    }

    /// Our node's name as MeshCore users see it (§15.4).
    pub fn mesh_name(&self) -> String {
        self.lock().core.node_name().to_owned()
    }

    /// Sets our node's name (§15.4), at most 31 bytes, and announces it.
    pub fn set_mesh_name(&self, name: String, now_ms: u64) -> Result<(), LichenError> {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.set_node_name(&name, now_ms)?;
        i.note_radio_name();
        i.persist();
        Ok(())
    }
}

impl Inner {
    /// The radio's channels; the ones saved last while it is away.
    pub(crate) fn channel_list(&self) -> Vec<lc_core::Channel> {
        if self.core.channels_known() {
            return self.core.channels().to_vec();
        }
        self.store
            .as_ref()
            .and_then(|s| s.channels().ok())
            .unwrap_or_default()
    }
}

/// The hashtag channel a search names (`#hiking`, `#Hiking`): its name as the radio keeps it,
/// `#hiking`, or `None` when it is not a hashtag (§15.3: letters, digits and `-`). The apps'
/// New conversation offers to join it (F-4).
#[uniffi::export]
pub fn hashtag_channel_name(tag: String) -> Option<String> {
    lc_core::plain::hashtag(&tag).map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::hashtag_channel_name;

    #[test]
    fn hashtag_names_as_the_radio_keeps_them() {
        assert_eq!(
            hashtag_channel_name("#Hiking".into()).as_deref(),
            Some("#hiking")
        );
        assert_eq!(
            hashtag_channel_name(" ridge-2 ".into()).as_deref(),
            Some("#ridge-2")
        );
        for bad in ["#", "", "#two words", "#été", "#a_b"] {
            assert_eq!(hashtag_channel_name(bad.into()), None, "{bad}");
        }
    }
}
