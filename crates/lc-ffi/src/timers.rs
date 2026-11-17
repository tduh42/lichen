//! Disappearing messages (§6.5, ADR-0030): a conversation's timer, shared with the other
//! side (or the group) as TIMER, and the messages each phone removes when their time is up.

use super::*;
use lc_core::content::notice;

impl Inner {
    /// The name we show for node `node`: the user's nickname, else the node's.
    pub(crate) fn name_of(&self, node: &PubKey) -> String {
        self.nicknames.get(node).cloned().unwrap_or_else(|| {
            self.core
                .contacts()
                .find(|c| c.key == *node)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        })
    }

    /// Records the notice of a timer change in `chat`: `actor` did it ("" and our node: us).
    fn timer_notice(&mut self, chat: Chat, actor: PubKey, actor_name: String, seconds: u32) {
        self.record(store::Message {
            id: 0,
            chat,
            outgoing: false,
            author: None,
            content: Content::Notice(lc_core::content::Notice {
                kind: notice::TIMER,
                actor,
                actor_name,
                subjects: Vec::new(),
                text: seconds.to_string(),
            }),
            at_ms: unix_ms(),
            status: Status::Received,
            fragments: 0,
            airtime_ms: 0,
            snr4: None,
            read: true,
            msg_ref: None,
        });
    }

    /// The user sets `chat`'s timer. A lichen conversation is told (TIMER, persisted until it
    /// goes); a MeshCore chat keeps it on this phone only.
    pub(crate) fn set_timer(&mut self, chat: Chat, seconds: u32) -> Result<(), LichenError> {
        if seconds > lc_core::content::TIMER_MAX_S {
            return Err(LichenError::TooLarge);
        }
        let st = self.store.as_ref().ok_or(LichenError::Storage)?;
        let (held, held_ts) = st.timer(&chat).map_err(|_| LichenError::Storage)?;
        if held == seconds {
            return Ok(());
        }
        let ts = u32::try_from(unix_ms() / 1000)
            .unwrap_or(u32::MAX)
            .max(held_ts.saturating_add(1));
        let shared = !chat.is_plain();
        st.set_timer(&chat, seconds, ts, shared)
            .map_err(|_| LichenError::Storage)?;
        let me = self.core.node_key().copied().unwrap_or([0; 32]);
        let name = self.core.node_name().to_owned();
        self.timer_notice(chat, me, name, seconds);
        if shared {
            self.submit_timer(chat, seconds, ts);
        }
        self.persist();
        Ok(())
    }

    /// Sends our timer to `chat` and follows it until it is there (§6.5).
    pub(crate) fn submit_timer(&mut self, chat: Chat, seconds: u32, ts: u32) {
        let datagram = Content::Timer {
            sent_at: ts,
            seconds,
        }
        .encode();
        let now = self.last_now;
        let id = match &chat {
            Chat::Contact(k) => self.core.send_datagram(k, datagram, now),
            Chat::Group(g) => self.core.send_group_datagram(g, &datagram, now),
            Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_) => return,
        };
        // Not accepted now: it stays pending and goes again at the next start.
        if let Ok(id) = id {
            self.timer_inflight.insert(id, (chat, ts));
        }
    }

    /// Our timers not sent yet go again (after a restart).
    pub(crate) fn resume_timers(&mut self) {
        let pending = self
            .store
            .as_ref()
            .and_then(|s| s.pending_timers().ok())
            .unwrap_or_default();
        for (chat, seconds, ts) in pending {
            self.submit_timer(chat, seconds, ts);
        }
    }

    /// A TIMER from `from` in `chat`: the newest setting holds, and a change is noted.
    pub(crate) fn on_timer(&mut self, chat: Chat, from: &PubKey, seconds: u32, ts: u32) {
        let changed = self
            .store
            .as_ref()
            .is_some_and(|s| s.set_timer(&chat, seconds, ts, false).unwrap_or(false));
        if changed {
            let name = self.name_of(from);
            self.timer_notice(chat, *from, name, seconds);
        }
    }

    /// Removes the messages whose time is up, and tells the app (§6.5).
    pub(crate) fn vanish(&mut self) {
        let gone = self
            .store
            .as_ref()
            .and_then(|s| s.vanish(unix_ms()).ok())
            .unwrap_or_default();
        for m in gone {
            self.media_gone(&m.content);
            self.extra.push_back(LichenEvent::MessageRemoved {
                id: m.id,
                chat: ChatRef::from_store(&m.chat),
            });
        }
    }

    /// When the next message disappears, as `now_ms`.
    pub(crate) fn next_vanish(&self) -> Option<u64> {
        let at = self.store.as_ref()?.next_vanish().ok()??;
        u64::try_from(at - self.offset()).ok()
    }
}
