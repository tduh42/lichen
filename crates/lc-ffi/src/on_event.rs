//! What each core event does to the store and to the app's event queue.

use super::*;

impl Inner {
    /// Request `identity` became contact `node`'s identity (ADR-0046): what it sent goes into
    /// the conversation as if it had just arrived there (references, reactions, edits and
    /// timers as for any message), each at the time it came; then the request is gone. Again
    /// later, nothing is left to move.
    pub(crate) fn adopt_request(&mut self, node: &PubKey, identity: &PubKey) {
        let Some(kept) = self
            .store
            .as_ref()
            .and_then(|s| s.request_messages(identity).ok())
        else {
            return;
        };
        for m in kept {
            let Some(content) = Content::decode(&m.plaintext) else {
                continue;
            };
            self.arrived_ms = Some(m.at_ms);
            self.on_event(&Event::Received {
                from: *node,
                content,
                plaintext: m.plaintext,
                snr4: m.snr4,
                rssi: 0,
            });
            self.arrived_ms = None;
        }
        if let Some(st) = &self.store {
            let _ = st.delete_request(identity);
        }
    }

    /// Records what an event means for the history. False: a resend already received, to
    /// keep from the app (§6.1).
    pub(crate) fn on_event(&mut self, e: &Event) -> bool {
        match e {
            Event::Connected { .. } => self.save_contacts(),
            Event::ContactsUpdated
            | Event::GroupJoined { .. }
            | Event::GroupUpdated { .. }
            | Event::IdentityLearned { .. } => {
                self.save_contacts();
            }
            Event::Received {
                from,
                content:
                    content @ Content::Image {
                        full: true,
                        media_id,
                        ..
                    },
                ..
            } if self
                .store
                .as_ref()
                .and_then(|s| s.find_image(&Chat::Contact(*from), media_id).ok().flatten())
                .is_some() =>
            {
                // The full variant replaces its preview.
                let chat = Chat::Contact(*from);
                if let Some(m) = self.store.as_ref().and_then(|s| {
                    let row = s.find_image(&chat, media_id).ok().flatten()?;
                    s.set_content(row, content).ok().flatten()
                }) {
                    let message = self.present(m);
                    self.extra
                        .push_back(LichenEvent::MessageChanged { message });
                }
            }
            Event::Received {
                from,
                content:
                    Content::Reaction {
                        sent_at,
                        target,
                        emoji,
                    },
                ..
            } => {
                let chat = Chat::Contact(*from);
                if let Some(by) = self.author_identity(&chat, false, None) {
                    self.on_reaction(chat, *target, by, from, emoji, *sent_at);
                }
                return false;
            }
            Event::GroupMessage {
                gid,
                from,
                content:
                    Content::Reaction {
                        sent_at,
                        target,
                        emoji,
                    },
                ..
            } => {
                self.on_reaction(Chat::Group(*gid), *target, *from, from, emoji, *sent_at);
                return false;
            }
            // Timers are state too (§6.5).
            Event::Received {
                from,
                content: Content::Timer { sent_at, seconds },
                ..
            } => {
                self.on_timer(Chat::Contact(*from), from, *seconds, *sent_at);
                return false;
            }
            Event::GroupMessage {
                gid,
                from,
                content: Content::Timer { sent_at, seconds },
                ..
            } => {
                self.on_timer(Chat::Group(*gid), from, *seconds, *sent_at);
                return false;
            }
            Event::Delivered { id } | Event::GroupSent { id }
                if self.timer_inflight.contains_key(id) =>
            {
                if let (Some((chat, ts)), Some(st)) = (self.timer_inflight.remove(id), &self.store)
                {
                    let _ = st.timer_sent(&chat, ts);
                }
                return false;
            }
            // A timer that failed stays pending: it goes again at the next start.
            Event::Failed { id, .. } if self.timer_inflight.contains_key(id) => {
                self.timer_inflight.remove(id);
                return false;
            }
            // Edits and deletes are state, like reactions (§6.4).
            Event::Received {
                from,
                content: c @ (Content::Edit { .. } | Content::Delete { .. }),
                ..
            } => {
                self.on_amendment_content(Chat::Contact(*from), from, c);
                return false;
            }
            Event::GroupMessage {
                gid,
                from,
                content: c @ (Content::Edit { .. } | Content::Delete { .. }),
                ..
            } => {
                self.on_amendment_content(Chat::Group(*gid), from, c);
                return false;
            }
            Event::MediaRequested {
                from,
                media_id,
                full: true,
            } => {
                let chat = Chat::Contact(*from);
                if let Some((to, datagram)) = self
                    .store
                    .as_ref()
                    .and_then(|s| s.media(media_id).ok().flatten())
                    && to == chat
                {
                    let now = self.last_now;
                    let _ = self.core.send_media(from, datagram, now);
                }
            }
            Event::Received {
                from,
                content,
                plaintext,
                snr4,
                ..
            } => {
                let chat = Chat::Contact(*from);
                if self.is_resend(&chat, None, content) {
                    return false;
                }
                // From the bytes as sent: a re-encoding may differ (§6.2, SR-20).
                let msg_ref = self.reference(&chat, false, None, plaintext);
                let at_ms = self.arrived_ms.unwrap_or_else(unix_ms);
                self.record(store::Message {
                    id: 0,
                    chat,
                    outgoing: false,
                    author: None,
                    content: content.clone(),
                    at_ms,
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                    msg_ref,
                });
            }
            Event::GroupMessage {
                gid,
                from,
                author,
                content,
                plaintext,
                snr4,
                late,
                ..
            } => {
                let chat = Chat::Group(*gid);
                if self.is_resend(&chat, Some(from), content) {
                    return false;
                }
                let msg_ref = self.reference(&chat, false, Some(*author), plaintext);
                self.record(store::Message {
                    id: 0,
                    chat,
                    outgoing: false,
                    author: Some(*from),
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: if *late {
                        Status::Late
                    } else {
                        Status::Received
                    },
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                    msg_ref,
                });
            }
            Event::GroupChanged { gid, notice } => {
                // A line in the group's conversation: read already, never notified.
                self.record(store::Message {
                    id: 0,
                    chat: Chat::Group(*gid),
                    outgoing: false,
                    author: None,
                    content: Content::Notice(notice.clone()),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: None,
                    read: true,
                    msg_ref: None,
                });
                return false;
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
            Event::Location {
                from,
                group,
                position,
                at,
                snr4,
            } => {
                if let Some(st) = &self.store {
                    let _ = st.put_position(&StoredPosition {
                        from: *from,
                        group: *group,
                        position: *position,
                        at: *at,
                        snr4: *snr4,
                    });
                }
            }
            Event::SharesChanged => self.save_shares(),
            Event::TxPower { previous, .. } => {
                if let Some(st) = &self.store {
                    let v = previous.map(|p| [p as u8]);
                    let _ = st.set_setting(TX_POWER_RESTORE, v.as_ref().map(|v| v.as_slice()));
                }
            }
            Event::Delivered { id } | Event::GroupSent { id }
                if self.reaction_inflight.contains_key(id) =>
            {
                if let (Some((chat, target, ts)), Some(me), Some(st)) = (
                    self.reaction_inflight.remove(id),
                    self.core.key().copied(),
                    &self.store,
                ) {
                    let _ = st.reaction_done(&chat, &target, &me, ts);
                }
                return false;
            }
            // A reaction that failed stays pending: it goes again at the next start (§6.2).
            Event::Failed { id, .. } if self.reaction_inflight.contains_key(id) => {
                self.reaction_inflight.remove(id);
                return false;
            }
            Event::Delivered { id } | Event::GroupSent { id }
                if self.amend_inflight.contains_key(id) =>
            {
                if let (Some((chat, target, ts)), Some(st)) =
                    (self.amend_inflight.remove(id), &self.store)
                {
                    let _ = st.amendment_done(&chat, &target, ts);
                }
                return false;
            }
            Event::Failed { id, .. } if self.amend_inflight.contains_key(id) => {
                self.amend_inflight.remove(id);
                return false;
            }
            Event::Delivered { id } => self.update(*id, Status::Delivered, true),
            Event::Failed { id, reason } => self.update(*id, Status::Failed(*reason), true),
            Event::GroupSent { id } => self.update(*id, Status::Flooded, true),
            // A plain MeshCore message (§15.1): a channel post is kept as it came.
            Event::PlainReceived(m) => {
                let text = match (&m.chat, &m.label) {
                    (PlainChat::Channel(_) | PlainChat::Room(_), Some(label)) => {
                        format!("{label}: {}", m.text)
                    }
                    _ => m.text.clone(),
                };
                self.record(store::Message {
                    id: 0,
                    chat: plain_chat(m.chat),
                    outgoing: false,
                    author: m.from,
                    content: Content::Text {
                        sent_at: m.sent_at,
                        text,
                    },
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: m.snr4,
                    read: false,
                    msg_ref: None,
                });
            }
            // Requests (§4.6, ADR-0046): kept apart until the user decides. One the user
            // settled since the event was queued is not brought back.
            Event::Requested { node, identity } | Event::RequestMessage { node, identity, .. }
                if !self
                    .core
                    .requests()
                    .any(|(i, n)| i == identity && n == node) => {}
            Event::Requested { node, identity } => {
                if let Some(st) = &self.store {
                    let _ = st.put_request(identity, node, unix_ms());
                }
            }
            Event::RequestMessage {
                identity,
                plaintext,
                snr4,
                ..
            } => {
                if let Some(st) = &self.store {
                    let _ = st.add_request_message(
                        identity,
                        &store::RequestMessage {
                            plaintext: plaintext.clone(),
                            at_ms: unix_ms(),
                            snr4: *snr4,
                        },
                    );
                }
            }
            Event::RequestAccepted { node, identity } => {
                self.save_contacts();
                self.adopt_request(node, identity);
            }
            Event::ChannelsUpdated => {
                if self.core.channels_known()
                    && let Some(st) = &self.store
                {
                    let _ = st.put_channels(self.core.channels());
                }
            }
            _ => {}
        }
        true
    }
}
