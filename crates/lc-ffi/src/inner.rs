//! The state behind the lock: the core, the store, and how stored messages are presented.

use super::*;

pub(crate) struct Inner {
    pub(crate) core: Core,
    /// The identity seed (ADR-0013), for the recovery phrase.
    pub(crate) seed: [u8; 32],
    pub(crate) store: Option<Store>,
    pub(crate) data_dir: Option<PathBuf>,
    /// The database's key (ADR-0036), when the app encrypts it.
    pub(crate) db_key: Option<[u8; 32]>,
    /// Core message id → row id, for the messages sent by this run.
    pub(crate) inflight: HashMap<u64, i64>,
    /// Events to hand out after the current one (`MessageChanged`).
    pub(crate) extra: VecDeque<LichenEvent>,
    /// The latest `now_ms` the app gave (to answer requests from inside event handling).
    pub(crate) last_now: u64,
    /// When a message replayed from a request arrived (ADR-0046): its time in the history.
    pub(crate) arrived_ms: Option<i64>,
    /// Names the user gave contacts on this phone (node → nickname).
    pub(crate) nicknames: HashMap<PubKey, String>,
    /// Unfinished texts (§6.1), by row id.
    pub(crate) outbox: HashMap<i64, Pending>,
    /// How long a text waits before it expires (ms).
    pub(crate) outbox_expiry_ms: i64,
    /// Unix ms minus `now_ms`, from `set_clock`.
    pub(crate) clock_offset: Option<i64>,
    /// Our reactions being sent: core id → conversation and target (§6.2).
    /// Our reactions on their way, with the timestamp of the change each carries.
    pub(crate) reaction_inflight: HashMap<u64, (Chat, MessageRef, u32)>,
    /// Our edits and deletes on their way (§6.4), like reactions.
    pub(crate) amend_inflight: HashMap<u64, (Chat, MessageRef, u32)>,
    /// Our disappearing-message timers on their way (§6.5): core id → chat and timestamp.
    pub(crate) timer_inflight: HashMap<u64, (Chat, u32)>,
    /// The recent log of problem reports (ADR-0045).
    pub(crate) diag: report::Diag,
    /// Requests for the full variant of our images that wait for the user (§13.3), oldest
    /// first, at most [`FULL_REQUESTS_MAX`]. In memory only: lost when the app stops, and the
    /// contact can ask again.
    pub(crate) full_requests: Vec<FullRequest>,
    /// Full images the user agreed to send, while on their way: core id, contact, image. The
    /// same request again meanwhile is answered by that transfer.
    pub(crate) full_sending: Vec<(u64, PubKey, lc_core::content::MediaId)>,
    /// Our advert, owed since a card was added with the radio away (§4.7): flooded or not.
    pub(crate) advert_owed: Option<bool>,
}

/// A contact asks for the full variant of one of our images (§13.3): it goes only if the user
/// accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FullRequest {
    pub(crate) node: PubKey,
    pub(crate) media_id: lc_core::content::MediaId,
    /// Our image's row.
    pub(crate) message_id: i64,
    /// Its airtime when it was asked for, on the path known then.
    pub(crate) airtime_ms: u64,
}

/// Requests for full images kept at once: a contact asking for more cannot grow the list.
pub(crate) const FULL_REQUESTS_MAX: usize = 32;

/// The database of `identity` in `dir`: one per identity (ADR-0010).
pub(crate) fn database_file(dir: &std::path::Path, identity: &PubKey) -> PathBuf {
    let hex: String = identity
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    dir.join(format!("id-{hex}.sqlite"))
}

/// How long a text waits for the radio and its recipient (§6.1).
pub(crate) const OUTBOX_EXPIRY_MS: i64 = 24 * 3_600_000;

/// An unfinished text in the outbox (§6.1).
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub(crate) core_id: u64,
    pub(crate) chat: Chat,
    /// Unix ms.
    pub(crate) expires_ms: i64,
    /// Its status in the database: queued, or sending once a frame went out.
    pub(crate) stored: Status,
    pub(crate) fragments: u8,
    /// The status the app was last told.
    pub(crate) presented: MessageStatus,
}

impl Inner {
    /// A deleted message was an announcement: its offer and its download go too (§6.4,
    /// review of 2026-11-15, SR-28), unless another message still announces the same content.
    /// A hash is not a message: a contact who announced again content we sent, then deleted
    /// their message, must not make us stop offering ours or lose our copy (second read of
    /// 2026-11-17, N7). Unsure (the store failed), they stay.
    pub(crate) fn media_gone(&mut self, content: &Content) {
        if let Content::Announce(a) = content {
            let kept = match &self.store {
                Some(st) => st.announces(&a.hash).unwrap_or(true),
                None => false,
            };
            if !kept {
                self.extra.push_back(LichenEvent::MediaGone {
                    hash: a.hash.to_vec(),
                });
            }
        }
    }

    /// Verified contact `node` keeps the name it had when verified: a node's advertised name
    /// is its owner's to change, and a verified contact renamed by its node would keep its
    /// badge under any name (review of 2026-11-15, SR-30). Kept as the contact's nickname,
    /// which the user can still change; nothing changes when they gave one.
    pub(crate) fn pin_name(&mut self, node: &PubKey) {
        if self.nicknames.contains_key(node) {
            return;
        }
        let Some(name) = self
            .core
            .contacts()
            .find(|c| c.key == *node && c.verified)
            .map(|c| c.name.clone())
            .filter(|n| !n.trim().is_empty())
        else {
            return;
        };
        if let Some(s) = &self.store {
            let _ = s.set_nickname(node, Some(&name));
        }
        self.nicknames.insert(*node, name);
    }

    /// Contacts whose radio now advertises another name than the one last saved: a line in
    /// each conversation we have with them, lichen or MeshCore, so the change is seen even
    /// when the name shown stays (a verified contact keeps its own, SR-30). Compared with the
    /// stored names, a rename made while the app was closed shows at the next contact list.
    pub(crate) fn note_renames(&mut self) {
        let Some(saved) = self.store.as_ref().and_then(|s| s.contacts().ok()) else {
            return;
        };
        let before: HashMap<PubKey, String> = saved.into_iter().map(|c| (c.key, c.name)).collect();
        let renamed: Vec<(PubKey, String, String)> = self
            .core
            .contacts()
            .filter_map(|c| {
                let old = before.get(&c.key)?.trim();
                let new = c.name.trim();
                (!old.is_empty() && !new.is_empty() && old != new)
                    .then(|| (c.key, old.to_owned(), new.to_owned()))
            })
            .collect();
        for (node, old, new) in renamed {
            let shown = self.nicknames.get(&node).cloned().unwrap_or(old);
            for chat in [Chat::Contact(node), Chat::Mesh(node)] {
                let ours = self
                    .store
                    .as_ref()
                    .and_then(|s| s.has_messages(&chat).ok())
                    .unwrap_or(false);
                if !ours {
                    continue;
                }
                self.record(store::Message {
                    id: 0,
                    chat,
                    outgoing: false,
                    author: None,
                    content: Content::Notice(lc_core::content::Notice {
                        kind: lc_core::content::notice::NODE_RENAMED,
                        actor: node,
                        actor_name: shown.clone(),
                        subjects: Vec::new(),
                        text: new.clone(),
                    }),
                    at_ms: self.wall_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: None,
                    read: true,
                    msg_ref: None,
                });
            }
        }
    }

    /// Opens the database of our identity (or one in memory) and restores contacts, groups
    /// and shares.
    pub(crate) fn open_store(&mut self, identity: &PubKey) {
        let store = match &self.data_dir {
            Some(dir) => {
                let _ = std::fs::create_dir_all(dir);
                let file = database_file(dir, identity);
                match &self.db_key {
                    Some(key) => Store::open_encrypted(&file, key),
                    None => Store::open(&file),
                }
            }
            None => Store::in_memory(),
        };
        let Ok(store) = store else {
            return;
        };
        // Their rows were another database's.
        self.full_requests.clear();
        self.full_sending.clear();
        self.nicknames = store.nicknames().unwrap_or_default();
        for c in store.contacts().unwrap_or_default() {
            self.core.restore_contact(c);
        }
        for snap in store.groups().unwrap_or_default() {
            self.core.restore_group(&snap);
        }
        // Invitations not answered yet (§10.1): they wait for the user across restarts.
        for snap in store.invitations().unwrap_or_default() {
            self.core.restore_invitation(&snap, self.last_now);
        }
        for (chat, precision, until) in store.shares().unwrap_or_default() {
            let _ = self.core.restore_share(chat, precision, until);
        }
        for gid in store.search_consents().unwrap_or_default() {
            self.core.restore_search_consent(&gid);
        }
        // Rooms logged in to at each connection (§15.8).
        if let Ok(Some(v)) = store.setting(crate::api_server::ROOM_LOGINS) {
            self.core
                .set_room_logins(crate::api_server::decode_room_logins(&v));
        }
        // Blocks (ADR-0034).
        for b in store.blocked().unwrap_or_default() {
            self.core.restore_block(b.node, b.identity);
        }
        // Requests (ADR-0046): their HELLOs are answered again.
        for q in store.requests().unwrap_or_default() {
            self.core.restore_request(q.node, q.identity);
        }
        // A TX power a search lowered before the app stopped: the radio kept it.
        if let Ok(Some(v)) = store.setting(TX_POWER_RESTORE)
            && let [dbm] = v.as_slice()
        {
            self.core.restore_tx_power(*dbm as i8);
        }
        let now_s = u64::try_from(self.wall_ms() / 1000).unwrap_or(0);
        let _ = store.prune_positions(now_s.saturating_sub(POSITION_KEEP_S));
        let _ = store.purge_orphans(self.wall_ms());
        self.store = Some(store);
        // What ran out while the app was stopped goes before anything is shown (§6.5).
        self.vanish();
        self.backfill_refs();
        self.resume_outbox();
        self.resume_reactions();
        self.resume_amendments();
        self.resume_timers();
    }

    /// Texts unfinished when the app stopped are submitted again with their stored plaintext;
    /// media unfinished fail (`Interrupted`), §6.1.
    pub(crate) fn resume_outbox(&mut self) {
        let Some(rows) = self.store.as_ref().and_then(|s| s.resume_unfinished().ok()) else {
            return;
        };
        for (m, expires) in rows {
            let expires = expires.unwrap_or(m.at_ms + self.outbox_expiry_ms);
            let datagram = m.content.encode();
            let now = self.last_now;
            let id = match (m.chat, &m.content) {
                (Chat::Contact(k), _) => self.core.send_datagram(&k, datagram, now),
                (Chat::Group(g), _) => self.core.send_group_datagram(&g, &datagram, now),
                (Chat::Mesh(k) | Chat::Room(k), Content::Text { sent_at, text }) => {
                    self.core.send_plain(&k, text, *sent_at, now)
                }
                (Chat::Channel(c), Content::Text { sent_at, text }) => {
                    self.core.send_channel(&c, text, *sent_at, now)
                }
                (Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_), _) => {
                    Err(lc_core::Error::NotAllowed)
                }
            };
            match id {
                Ok(id) if matches!(m.chat, Chat::Channel(_)) => {
                    self.inflight.insert(id, m.id);
                }
                Ok(id) => self.track(m.id, id, m.chat, expires, m.status, m.fragments),
                Err(_) => {
                    if let Some(st) = &self.store {
                        let _ = st.set_status(m.id, Status::Failed(CoreFail::Interrupted));
                    }
                }
            }
        }
    }

    /// Follows row `row`, sent as core message `core_id`, in the outbox.
    pub(crate) fn track(
        &mut self,
        row: i64,
        core_id: u64,
        chat: Chat,
        expires_ms: i64,
        stored: Status,
        fragments: u8,
    ) {
        self.inflight.insert(core_id, row);
        let mut p = Pending {
            core_id,
            chat,
            expires_ms,
            stored,
            fragments,
            presented: MessageStatus::Received,
        };
        p.presented = self.live_status(&p, self.core.outgoing(core_id, self.last_now));
        self.outbox.insert(row, p);
    }

    /// Our node's advert now, or at the next connection with the radio away (one owed
    /// advert, flooded if any asked for a flood).
    pub(crate) fn advertise(&mut self, flood: bool) {
        if self.core.online() {
            let now = self.last_now;
            self.core.send_advert(flood, now);
        } else {
            self.advert_owed = Some(self.advert_owed.unwrap_or(false) || flood);
        }
    }

    /// Unix ms minus the app's clock.
    pub(crate) fn offset(&self) -> i64 {
        self.clock_offset
            .unwrap_or_else(|| unix_ms() - i64::try_from(self.last_now).unwrap_or(0))
    }

    /// Unix ms now, for what the FFI dates, expires and sweeps: the wall clock the app gives
    /// (`set_clock`) at its last `now_ms`, the clock the core and `next_wakeup` run on. The
    /// system's when it is later: the app's, as of its last call, can only lag behind it while
    /// the app sleeps between wake-ups (B-core-3).
    pub(crate) fn wall_ms(&self) -> i64 {
        let system = unix_ms();
        self.clock_offset.map_or(system, |o| {
            o.saturating_add(i64::try_from(self.last_now).unwrap_or(0))
                .max(system)
        })
    }

    /// What the app should show for an unfinished text, from the core's view (§6.1).
    pub(crate) fn live_status(&self, p: &Pending, live: Option<Outbound>) -> MessageStatus {
        let sending = match p.stored {
            Status::Sending { sent, total } => MessageStatus::Sending { sent, total },
            _ => MessageStatus::Sending {
                sent: 0,
                total: p.fragments,
            },
        };
        let Some(o) = live else {
            return sending;
        };
        let reason = match o.stage {
            Stage::Transmitting => return sending,
            Stage::Offline => WaitReason::Radio,
            Stage::Behind => WaitReason::Earlier,
            Stage::FindingPath => WaitReason::Path,
            Stage::Connecting => WaitReason::Handshake,
            Stage::Budget { until } => WaitReason::Budget {
                at_ms: i64::try_from(until).unwrap_or(i64::MAX) + self.offset(),
            },
        };
        MessageStatus::Waiting {
            reason,
            started: o.started || !matches!(p.stored, Status::Queued),
            expires_at_ms: p.expires_ms,
        }
    }

    /// A stored message as the app should see it: an unfinished text with its live status, a
    /// reply with what it answers, the reactions to it.
    pub(crate) fn present(&self, m: store::Message) -> StoredMessage {
        let row = m.id;
        let quote = match &m.content {
            Content::Reply { target, .. } => Some(self.quote(&m.chat, target)),
            _ => None,
        };
        let reactions = m
            .msg_ref
            .map(|r| self.reaction_counts(&m.chat, &r))
            .unwrap_or_default();
        // Ours, with a reference, not taken back, within 24 hours (§6.4).
        let now_s = u32::try_from(self.wall_ms() / 1000).unwrap_or(u32::MAX);
        let changeable = m.outgoing
            && m.msg_ref.is_some()
            && !matches!(m.content, Content::Deleted { .. })
            && m.content
                .sent_at()
                .is_some_and(|t| now_s.saturating_sub(t) < lc_core::content::EDIT_WINDOW_S);
        let editable_kind = matches!(m.content, Content::Text { .. } | Content::Reply { .. });
        // Its author's latest text (§6.4).
        let edit = self
            .store
            .as_ref()
            .and_then(|s| s.amendment_for(&m).ok().flatten())
            .filter(|a| !a.deleted)
            .and_then(|a| a.text)
            .zip(m.content.sent_at());
        // A received channel post is stored as it came, "name: text" (§15.1), a room's post
        // likewise (§15.8).
        let label = match (&m.chat, m.outgoing, &m.content) {
            (Chat::Channel(_) | Chat::Room(_), false, Content::Text { sent_at, text }) => {
                let (name, body) = lc_core::plain::split_post(text);
                name.map(|n| {
                    (
                        n.to_owned(),
                        Message::Text {
                            sent_at: *sent_at,
                            text: body.to_owned(),
                        },
                    )
                })
            }
            _ => None,
        };
        let mut out = StoredMessage::from(m);
        if let Some((name, body)) = label {
            out.author_name = Some(name);
            out.message = body;
        }
        if let Some((text, sent_at)) = edit {
            out.message = Message::Text { sent_at, text };
            out.edited = true;
        }
        out.deletable = changeable;
        out.editable = changeable && editable_kind;
        if let Some(p) = self.outbox.get(&row) {
            out.status = self.live_status(p, self.core.outgoing(p.core_id, self.last_now));
        }
        out.quote = quote;
        out.reactions = reactions;
        out
    }

    /// What reference `target` of `chat` points at (§6.2).
    pub(crate) fn quote(&self, chat: &Chat, target: &MessageRef) -> Quote {
        let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.message_by_ref(chat, target).ok().flatten())
        else {
            return Quote {
                message_id: None,
                outgoing: false,
                author: None,
                kind: QuoteKind::Other,
                excerpt: String::new(),
            };
        };
        // The current text of an edited message (§6.4).
        let edited = self
            .store
            .as_ref()
            .and_then(|s| s.amendment_for(&m).ok().flatten())
            .and_then(|a| a.text);
        let (kind, excerpt) = match &m.content {
            Content::Text { text, .. } | Content::Reply { text, .. } => (
                QuoteKind::Text,
                edited
                    .as_deref()
                    .unwrap_or(text)
                    .chars()
                    .take(100)
                    .collect(),
            ),
            Content::Voice { .. } => (QuoteKind::Voice, String::new()),
            Content::File { name, .. } => (QuoteKind::File, name.clone()),
            Content::Sticker { emoji, .. } => (QuoteKind::Sticker, emoji.clone()),
            Content::Gif { title, .. } => (QuoteKind::Gif, title.clone()),
            Content::Announce(a) if a.kind == 3 => (QuoteKind::File, a.name.clone()),
            Content::Image { .. } | Content::Announce(_) => (QuoteKind::Photo, String::new()),
            _ => (QuoteKind::Other, String::new()),
        };
        Quote {
            message_id: Some(m.id),
            outgoing: m.outgoing,
            author: m.author.map(|a| a.to_vec()),
            kind,
            excerpt,
        }
    }

    /// The reactions to message `target` of `chat`, by emoji, the most frequent first.
    pub(crate) fn reaction_counts(&self, chat: &Chat, target: &MessageRef) -> Vec<ReactionCount> {
        let Some(st) = &self.store else {
            return Vec::new();
        };
        let me = self.core.key().copied();
        let mut out: Vec<ReactionCount> = Vec::new();
        for (reactor, emoji) in st.reactions_for(chat, target).unwrap_or_default() {
            let mine = Some(reactor) == me;
            match out.iter_mut().find(|c| c.emoji == emoji) {
                Some(c) => {
                    c.count += 1;
                    c.mine |= mine;
                }
                None => out.push(ReactionCount {
                    emoji,
                    count: 1,
                    mine,
                }),
            }
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.count));
        out
    }

    /// The identity of `chat`'s author of a message: ours, a group member's, or the contact's
    /// (§6.2).
    pub(crate) fn author_identity(
        &self,
        chat: &Chat,
        outgoing: bool,
        author: Option<PubKey>,
    ) -> Option<PubKey> {
        if outgoing {
            return self.core.key().copied();
        }
        match chat {
            Chat::Group(_) => author,
            Chat::Contact(k) => self
                .core
                .contacts()
                .find(|c| c.key == *k)
                .and_then(|c| c.identity),
            Chat::Mesh(_) | Chat::Channel(_) | Chat::Room(_) => None,
        }
    }

    /// The reference of a message of `chat` with `plaintext` as sent (§6.2).
    pub(crate) fn reference(
        &self,
        chat: &Chat,
        outgoing: bool,
        author: Option<PubKey>,
        plaintext: &[u8],
    ) -> Option<MessageRef> {
        if chat.is_plain() {
            return None;
        }
        self.author_identity(chat, outgoing, author)
            .map(|a| message_ref(&a, plaintext))
    }

    /// Fills in the references of messages from before version 9 (§6.2). An image's cannot
    /// be: the sender kept the full variant, not the preview that was sent.
    pub(crate) fn backfill_refs(&mut self) {
        let Some(st) = &self.store else {
            return;
        };
        loop {
            let rows = st.messages_without_ref(500).unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            for m in rows {
                let r = match m.content {
                    Content::Image { .. } => None,
                    ref c => self.reference(&m.chat, m.outgoing, m.author, &c.encode()),
                };
                if st.set_ref(m.id, r.as_ref()).is_err() {
                    return;
                }
            }
        }
    }
}
