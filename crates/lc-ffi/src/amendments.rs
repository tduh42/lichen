//! Reactions, edits and deletes: kept as state, resumed after a restart (§6.2, §6.4).

use super::*;

impl Inner {
    /// Sends our reaction to `target` in `chat` (§6.2) and follows it until delivered.
    pub(crate) fn submit_reaction(&mut self, chat: Chat, target: MessageRef, emoji: &str, ts: u32) {
        let datagram = Content::Reaction {
            sent_at: ts,
            target,
            emoji: emoji.to_owned(),
        }
        .encode();
        let now = self.last_now;
        let id = match chat {
            Chat::Contact(k) => self.core.send_datagram(&k, datagram, now),
            Chat::Group(g) => self.core.send_group_datagram(&g, &datagram, now),
            Chat::Mesh(_) | Chat::Channel(_) => return,
        };
        // Not accepted now: it stays pending and goes again at the next start.
        if let Ok(id) = id {
            self.reaction_inflight.insert(id, (chat, target, ts));
        }
    }

    /// Sends our edit (`text`) or delete (`None`) of `target` (§6.4) and follows it until
    /// delivered.
    pub(crate) fn submit_amendment(
        &mut self,
        chat: Chat,
        target: MessageRef,
        text: Option<String>,
        ts: u32,
    ) {
        let datagram = match text {
            Some(text) => Content::Edit {
                sent_at: ts,
                target,
                text,
            },
            None => Content::Delete {
                sent_at: ts,
                target,
            },
        }
        .encode();
        let now = self.last_now;
        let id = match chat {
            Chat::Contact(k) => self.core.send_datagram(&k, datagram, now),
            Chat::Group(g) => self.core.send_group_datagram(&g, &datagram, now),
            Chat::Mesh(_) | Chat::Channel(_) => return,
        };
        // Not accepted now: it stays pending and goes again at the next start.
        if let Ok(id) = id {
            self.amend_inflight.insert(id, (chat, target, ts));
        }
    }

    /// Our edits and deletes not delivered when the app stopped go again, unless too old.
    pub(crate) fn resume_amendments(&mut self) {
        let Some(st) = &self.store else {
            return;
        };
        let now = unix_ms();
        for p in st.pending_amendments().unwrap_or_default() {
            if p.expires_ms.is_some_and(|e| e <= now) {
                if let Some(st) = &self.store {
                    let _ = st.amendment_done(&p.chat, &p.target, p.ts);
                }
            } else {
                let text = if p.deleted { None } else { p.text };
                self.submit_amendment(p.chat, p.target, text, p.ts);
            }
        }
    }

    pub(crate) fn on_amendment_content(&mut self, chat: Chat, by: &PubKey, c: &Content) {
        match c {
            Content::Edit {
                sent_at,
                target,
                text,
            } => self.on_amendment(chat, *target, by, Some(text), *sent_at),
            Content::Delete { sent_at, target } => {
                self.on_amendment(chat, *target, by, None, *sent_at);
            }
            _ => {}
        }
    }

    /// An edit (`text`) or a delete (`None`) of `target` from node `by` (§6.4). It applies
    /// only to that author's message, now or when it arrives.
    pub(crate) fn on_amendment(
        &mut self,
        chat: Chat,
        target: MessageRef,
        by: &PubKey,
        text: Option<&str>,
        ts: u32,
    ) {
        // The author dates both the target and the change: the 24 hours are also held to this
        // phone's clock. A change that comes more than a day (and an hour of slack) after the
        // target arrived here is not applied (review of 2026-11-15, SR-15).
        let late = self
            .store
            .as_ref()
            .and_then(|s| s.message_by_ref(&chat, &target).ok().flatten())
            .is_some_and(|m| {
                unix_ms() > m.at_ms + (i64::from(lc_core::content::EDIT_WINDOW_S) + 3_600) * 1_000
            });
        if late {
            return;
        }
        let changed = self.store.as_ref().is_some_and(|s| {
            s.amend(&chat, &target, Some(by), text, ts, false, None)
                .unwrap_or(false)
        });
        if !changed {
            return;
        }
        let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.message_by_ref(&chat, &target).ok().flatten())
        else {
            return;
        };
        self.show_amended(m);
    }

    /// Message `m` after a change (§6.4): taken back when its author deleted it.
    pub(crate) fn show_amended(&mut self, m: store::Message) {
        let Some(st) = &self.store else {
            return;
        };
        let deleted = st
            .amendment_for(&m)
            .ok()
            .flatten()
            .is_some_and(|a| a.deleted);
        let m = if deleted && !matches!(m.content, Content::Deleted { .. }) {
            match st.tombstone(m.id).ok().flatten() {
                Some(m) => m,
                None => return,
            }
        } else {
            m
        };
        let message = self.present(m);
        self.extra
            .push_back(LichenEvent::MessageChanged { message });
    }

    /// Our reactions not delivered when the app stopped go again, unless too old (§6.2).
    pub(crate) fn resume_reactions(&mut self) {
        let (Some(st), Some(me)) = (&self.store, self.core.key().copied()) else {
            return;
        };
        let now = unix_ms();
        for p in st.pending_reactions(&me).unwrap_or_default() {
            if p.expires_ms.is_some_and(|e| e <= now) {
                if let Some(st) = &self.store {
                    let _ = st.reaction_done(&p.chat, &p.target, &me, p.ts);
                }
            } else {
                self.submit_reaction(p.chat, p.target, &p.emoji, p.ts);
            }
        }
    }

    /// A reaction from `by` (`by_wire`: as the app knows the sender), §6.2.
    pub(crate) fn on_reaction(
        &mut self,
        chat: Chat,
        target: MessageRef,
        by: PubKey,
        by_wire: &PubKey,
        emoji: &str,
        ts: u32,
    ) {
        let applied = self.store.as_ref().is_some_and(|s| {
            s.upsert_reaction(&chat, &target, &by, emoji, ts, false, None)
                .unwrap_or(false)
        });
        if !applied {
            return;
        }
        let target_row = self
            .store
            .as_ref()
            .and_then(|s| s.message_by_ref(&chat, &target).ok().flatten())
            .map(|m| self.present(m));
        if let Some(t) = &target_row {
            self.extra
                .push_back(LichenEvent::MessageChanged { message: t.clone() });
        }
        self.extra.push_back(LichenEvent::Reacted {
            chat: ChatRef::from_store(&chat),
            by: by_wire.to_vec(),
            emoji: emoji.to_owned(),
            target: target_row,
        });
    }
}
