//! Sending texts and media: estimates, voice, images, files, announcements.

use super::*;

#[uniffi::export]
impl LichenCore {
    /// Cost of a text of `text_len` bytes to `to`: fragments and airtime of the first round.
    pub fn estimate_text(&self, to: Vec<u8>, text_len: u32) -> Result<Estimate, LichenError> {
        let k = key(&to)?;
        let (fragments, airtime_ms) = self.lock().core.estimate_send(&k, 5 + text_len as usize);
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    /// What a group text of `text_len` bytes costs us: fragments and the airtime of our own
    /// floods (both copies). Relays spend theirs, and nobody acknowledges (§10.3).
    pub fn estimate_group_text(&self, text_len: u32) -> Estimate {
        let (fragments, airtime_ms) = self.lock().core.estimate_group_send(5 + text_len as usize);
        Estimate {
            fragments,
            airtime_ms,
        }
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
        let content = Content::Text { sent_at, text };
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let datagram = content.encode();
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_text(
            &k,
            match &content {
                Content::Text { text, .. } => text,
                _ => "",
            },
            sent_at,
            now_ms,
        )?;
        i.stored_outgoing(Chat::Contact(k), content, id, estimate, true, None)
    }

    /// Fragments and airtime (first round) of a datagram of `len` bytes to `to`.
    pub fn estimate_datagram(&self, to: Vec<u8>, len: u32) -> Result<Estimate, LichenError> {
        let k = key(&to)?;
        let (fragments, airtime_ms) = self.lock().core.estimate_send(&k, len as usize);
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    /// Sends a voice message prepared by [`prepare_voice`] (refused when over budget).
    pub fn send_voice(
        &self,
        to: Vec<u8>,
        voice: PreparedVoice,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        let content = Content::Voice {
            sent_at,
            mode: voice.mode,
            frames: voice.frames,
            data: voice.data,
        };
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let datagram = content.encode();
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_media(&k, datagram, now_ms)?;
        i.stored_outgoing(Chat::Contact(k), content, id, estimate, false, None)
    }

    /// Sends the preview of an image prepared by [`prepare_image`]; its full variant goes out
    /// when the receiver asks (§13.2). Refused when over budget.
    pub fn send_image(
        &self,
        to: Vec<u8>,
        image: PreparedImage,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let media_id = i.core.random_id();
        let variant = |full: bool, data: Vec<u8>| Content::Image {
            sent_at,
            media_id,
            full,
            width: image.width,
            height: image.height,
            full_len: image.full.len() as u16,
            data,
        };
        let full = variant(true, image.full.clone());
        let preview = variant(false, image.preview.clone());
        let datagram = preview.encode();
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_media(&k, datagram.clone(), now_ms)?;
        if let Some(st) = &i.store {
            let _ = st.put_media(&media_id, &Chat::Contact(k), &full.encode());
        }
        // What we keep in our history is the full image; its reference, the preview sent.
        i.stored_outgoing(Chat::Contact(k), full, id, estimate, false, Some(&datagram))
    }

    /// How a small file would go over LoRa to `to` (§13.2, ADR-0023): its size on the air,
    /// compressed or not, fragments and airtime on the current path. None when it does not fit
    /// (over 8 KB on the air even compressed, or over 64 KiB).
    pub fn lora_file(
        &self,
        to: Vec<u8>,
        name: String,
        data: Vec<u8>,
    ) -> Result<Option<LoraFile>, LichenError> {
        let k = key(&to)?;
        let Some((_, datagram)) = small_file(name, data, 0) else {
            return Ok(None);
        };
        let (fragments, airtime_ms) = self.lock().core.estimate_send(&k, datagram.len());
        Ok(Some(LoraFile {
            wire_bytes: datagram.len() as u32,
            compressed: datagram.get(5) == Some(&3),
            fragments,
            airtime_ms,
        }))
    }

    /// Sends a small file over LoRa (§13.2, ADR-0022), compressed when that is smaller
    /// (ADR-0023): at most 8 KB on the air, to a contact; refused when over budget (§13.4).
    /// The name is fitted to 64 bytes, its extension kept.
    pub fn send_file(
        &self,
        to: Vec<u8>,
        name: String,
        data: Vec<u8>,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        if data.is_empty() {
            return Err(LichenError::Empty);
        }
        let (content, datagram) = small_file(name, data, sent_at).ok_or(LichenError::TooLarge)?;
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_media(&k, datagram.clone(), now_ms)?;
        i.stored_outgoing(
            Chat::Contact(k),
            content,
            id,
            estimate,
            false,
            Some(&datagram),
        )
    }

    /// Announces content offered over the bridge (§14.1) to a contact or a group. `media_id`
    /// links it to a LoRa preview sent before (empty: a new one).
    #[allow(clippy::too_many_arguments)]
    pub fn send_announce(
        &self,
        chat: ChatRef,
        media_id: Vec<u8>,
        kind: u8,
        hash: Vec<u8>,
        size: u64,
        providers: Vec<Provider>,
        name: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let media_id: [u8; 8] = match <[u8; 8]>::try_from(media_id.as_slice()) {
            Ok(m) => m,
            Err(_) => i.core.random_id(),
        };
        let content = Content::Announce(lc_core::Announce {
            sent_at,
            media_id,
            kind,
            hash: hash
                .as_slice()
                .try_into()
                .map_err(|_| LichenError::InvalidKey)?,
            size: u32::try_from(size).map_err(|_| LichenError::TooLarge)?,
            providers: providers
                .iter()
                .map(|p| key(&p.key).map(|k| (p.transport, k)))
                .collect::<Result<_, _>>()?,
            name,
        });
        let datagram = content.encode();
        match c {
            // Lichen content never goes into a plain MeshCore chat (§15).
            Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_) => Err(LichenError::NotAllowed),
            Chat::Contact(k) => {
                let estimate = i.core.estimate_send(&k, datagram.len());
                let id = i.core.send_datagram(&k, datagram, now_ms)?;
                i.stored_outgoing(Chat::Contact(k), content, id, estimate, false, None)
            }
            Chat::Group(g) => {
                i.core.send_group_datagram(&g, &datagram, now_ms)?;
                let msg_ref = i.reference(&Chat::Group(g), true, None, &datagram);
                let row = store::Message {
                    id: 0,
                    chat: Chat::Group(g),
                    outgoing: true,
                    author: None,
                    content,
                    at_ms: unix_ms(),
                    status: Status::Flooded,
                    fragments: 1,
                    airtime_ms: 0,
                    snr4: None,
                    read: true,
                    msg_ref,
                };
                let stored = i
                    .store
                    .as_ref()
                    .and_then(|s| s.insert(&row).ok())
                    .unwrap_or(row);
                i.persist();
                Ok(i.present(stored))
            }
        }
    }
}

impl LichenCore {
    /// A message that waits in the outbox like a text (§6.1).
    pub(crate) fn send_message(
        &self,
        chat: ChatRef,
        content: Content,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
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
}
