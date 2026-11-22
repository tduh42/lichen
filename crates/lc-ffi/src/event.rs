//! Events the app polls.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FailReason {
    NoPath,
    Unreachable,
    Timeout,
    Refused,
    TooLarge,
    /// The app stopped before delivery.
    Interrupted,
    /// The contact, verified, now presents another identity: accept or reject it first.
    IdentityChanged,
    /// Not delivered within its expiry (24 h, §6.1).
    Expired,
}

impl From<CoreFail> for FailReason {
    fn from(r: CoreFail) -> Self {
        match r {
            CoreFail::NoPath => Self::NoPath,
            CoreFail::Unreachable => Self::Unreachable,
            CoreFail::Timeout => Self::Timeout,
            CoreFail::Refused => Self::Refused,
            CoreFail::TooLarge => Self::TooLarge,
            CoreFail::Interrupted => Self::Interrupted,
            CoreFail::IdentityChanged => Self::IdentityChanged,
            CoreFail::Expired => Self::Expired,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LichenEvent {
    Connected {
        key: Vec<u8>,
        name: String,
        radio: Radio,
    },
    /// We learned the identity behind contact `node`; `changed`: it replaced another one (to
    /// verify again).
    IdentityLearned {
        node: Vec<u8>,
        identity: Vec<u8>,
        changed: bool,
    },
    /// Contact `node`, verified, presented another identity, not adopted: ask the user to
    /// compare safety numbers, then `accept_identity` or `reject_identity`.
    IdentityConflict {
        node: Vec<u8>,
        identity: Vec<u8>,
    },
    ContactsUpdated,
    /// The radio's settings changed, or were read again: read `radio_settings()`.
    RadioSettings,
    /// What became of a setting sent to the radio (§1.1, B-user-5); also in
    /// `take_radio_outcomes()`.
    RadioOutcome {
        outcome: crate::RadioOutcome,
    },
    SessionEstablished {
        peer: Vec<u8>,
    },
    Received {
        from: Vec<u8>,
        message: Message,
        snr_db: f32,
        rssi: i8,
    },
    Progress {
        id: u64,
        sent: u32,
        total: u8,
    },
    Delivered {
        id: u64,
    },
    Failed {
        id: u64,
        reason: FailReason,
    },
    /// Group message `id` went out (its first copy, §10.3).
    GroupSent {
        id: u64,
    },
    /// `by` (a node in a direct conversation, an identity in a group) reacted to a message
    /// (§6.2); empty `emoji`: removed. `target`: the message, when it is on this phone.
    Reacted {
        chat: ChatRef,
        by: Vec<u8>,
        emoji: String,
        target: Option<StoredMessage>,
    },
    /// Message `id` of `chat` disappeared: its timer ran out (§6.5).
    MessageRemoved {
        id: i64,
        chat: ChatRef,
    },
    /// An announced content's message was deleted (§6.4): stop offering it, and remove its
    /// download (review of 2026-11-15, SR-28).
    MediaGone {
        hash: Vec<u8>,
    },
    /// Contact `node` speaks a newer protocol version (ADR-0029): update lichen to read
    /// everything it sends.
    NewerVersion {
        node: Vec<u8>,
        version: u8,
    },
    /// A group that lists us waits for the user (§10.1): `accept_group` or `decline_group`.
    /// `admin` is the admin's node.
    GroupInvited {
        gid: Vec<u8>,
        name: String,
        admin: Vec<u8>,
        members: u32,
    },
    GroupJoined {
        gid: Vec<u8>,
        name: String,
        admin: Vec<u8>,
    },
    GroupUpdated {
        gid: Vec<u8>,
    },
    GroupLeft {
        gid: Vec<u8>,
    },
    /// Node `node` joined group `gid`, which we administer, with an invitation (§10.5).
    MemberJoined {
        gid: Vec<u8>,
        node: Vec<u8>,
    },
    GroupMessage {
        gid: Vec<u8>,
        from: Vec<u8>,
        message: Message,
        snr_db: f32,
        rssi: i8,
        /// Recovered from a member after it was lost on the air (§10.6).
        late: bool,
    },
    /// A message was added to the history, or its status changed.
    MessageChanged {
        message: StoredMessage,
    },
    /// A position arrived (`end`: the sender stopped sharing; its marker is gone).
    Location {
        position: PeerPosition,
        end: bool,
    },
    /// One of our shares ended on its own.
    SharesChanged,
    /// `from` asked for a variant of one of our images. A request for the full variant that
    /// we can answer waits for the user and comes next as `FullImageRequested` (§13.3).
    MediaRequested {
        from: Vec<u8>,
    },
    /// Our search for `target` (identity) changed: read the estimate again (§11).
    SearchUpdated {
        target: Vec<u8>,
    },
    /// Member `by` (node) of group `gid` is searching for us, and we answer.
    Searched {
        gid: Vec<u8>,
        by: Vec<u8>,
    },
    /// A BEACON from member `from` (node) of `gid`: `sos` while it is in SOS, with or without a
    /// position (an SOS without GPS); clear when it left SOS.
    Beacon {
        gid: Vec<u8>,
        from: Vec<u8>,
        sos: bool,
        battery: Option<u8>,
    },
    /// Our radio's TX power is now `dbm` (`previous`: lowered by a search, to restore).
    TxPower {
        dbm: i8,
        previous: Option<i8>,
    },
    /// Our advert (`send_advert`, flooded or not) went to the air (`sent`: the radio answered
    /// OK), or did not: refused, unanswered, the radio gone, or no radio (§1.1).
    Advert {
        flood: bool,
        sent: bool,
    },
    /// A plain MeshCore message (§15.1), already in the history (`MessageChanged`): for the
    /// notification. `label`: the sender's name (contact, or a channel post's author).
    PlainReceived {
        chat: ChatRef,
        label: Option<String>,
        text: String,
        /// It mentions our radio's name (§15.9): notified even in a muted conversation, as
        /// "<label> mentioned you".
        mentions_us: bool,
    },
    /// The radio's MeshCore channels changed, or were read (§15.3): see `channels()`.
    ChannelsUpdated,
    /// A trace to `node` changed (§16.3): see `trace(node)`.
    NodeUpdated {
        node: Vec<u8>,
    },
    /// A conversation's preferences changed by themselves (an archived one came back with a
    /// message, ADR-0034): read `chat_prefs()` again.
    ChatPrefsChanged,
    /// Someone using contact `node`'s radio, with an identity the contact does not have,
    /// wants to write (§4.6, ADR-0046): a request, shown with `requests()`; the user accepts,
    /// blocks or deletes it. For the notification.
    IdentityRequested {
        node: Vec<u8>,
        identity: Vec<u8>,
    },
    /// A request's messages changed, or a request was settled: read `requests()` again.
    RequestsChanged,
    /// Contact `chat` asks for the full variant of our image `message_id` (§13.3), which
    /// would cost `airtime_ms` of our airtime: show it on that image, and notify outside the
    /// conversation. It goes only with `answer_full_image_request`; see also
    /// `full_image_requests`.
    FullImageRequested {
        chat: ChatRef,
        message_id: i64,
        airtime_ms: u64,
    },
}

pub(crate) fn snr(snr4: i8) -> f32 {
    f32::from(snr4) / 4.0
}

impl LichenEvent {
    /// `admin_node` maps a group admin's identity to its node (the app knows contacts by node).
    pub(crate) fn from_core(
        e: Event,
        freq_khz: u32,
        admin_node: impl Fn(&GroupId, &PubKey) -> PubKey,
    ) -> Self {
        match e {
            Event::Connected {
                key,
                name,
                freq_khz: f,
                radio,
            } => Self::Connected {
                key: key.to_vec(),
                name,
                radio: Radio {
                    freq_khz: if f == 0 { freq_khz } else { f },
                    bw_hz: radio.bw_hz,
                    sf: radio.sf,
                    cr: radio.cr,
                },
            },
            Event::IdentityLearned {
                node,
                identity,
                changed,
            } => Self::IdentityLearned {
                node: node.to_vec(),
                identity: identity.to_vec(),
                changed,
            },
            Event::IdentityConflict { node, identity } => Self::IdentityConflict {
                node: node.to_vec(),
                identity: identity.to_vec(),
            },
            Event::ContactsUpdated => Self::ContactsUpdated,
            Event::RadioSettings => Self::RadioSettings,
            Event::RadioOutcome(o) => Self::RadioOutcome { outcome: o.into() },
            Event::SessionEstablished { peer } => Self::SessionEstablished {
                peer: peer.to_vec(),
            },
            Event::Received {
                from,
                content,
                snr4,
                rssi,
                ..
            } => Self::Received {
                from: from.to_vec(),
                message: content.into(),
                snr_db: snr(snr4),
                rssi,
            },
            Event::Progress { id, sent, total } => Self::Progress { id, sent, total },
            Event::Delivered { id } => Self::Delivered { id },
            Event::Failed { id, reason } => Self::Failed {
                id,
                reason: reason.into(),
            },
            Event::GroupSent { id } => Self::GroupSent { id },
            Event::NewerVersion { node, version } => Self::NewerVersion {
                node: node.to_vec(),
                version,
            },
            Event::GroupInvited {
                gid,
                name,
                admin,
                members,
            } => Self::GroupInvited {
                gid: gid.to_vec(),
                name,
                admin: admin_node(&gid, &admin).to_vec(),
                members: u32::try_from(members).unwrap_or(u32::MAX),
            },
            Event::GroupJoined { gid, name, admin } => Self::GroupJoined {
                gid: gid.to_vec(),
                name,
                admin: admin_node(&gid, &admin).to_vec(),
            },
            Event::GroupUpdated { gid } => Self::GroupUpdated { gid: gid.to_vec() },
            Event::GroupLeft { gid } => Self::GroupLeft { gid: gid.to_vec() },
            // Recorded as a message instead (`on_event`).
            Event::GroupChanged { gid, .. } => Self::GroupUpdated { gid: gid.to_vec() },
            Event::MemberJoined { gid, node } => Self::MemberJoined {
                gid: gid.to_vec(),
                node: node.to_vec(),
            },
            Event::PlainReceived(m) => Self::PlainReceived {
                chat: ChatRef::from_store(&plain_chat(m.chat)),
                label: m.label,
                text: m.text,
                // Filled in with our radio's name (`LichenCore::event`).
                mentions_us: false,
            },
            Event::ChannelsUpdated => Self::ChannelsUpdated,
            Event::GroupMessage {
                gid,
                from,
                content,
                snr4,
                rssi,
                late,
                ..
            } => Self::GroupMessage {
                gid: gid.to_vec(),
                from: from.to_vec(),
                message: content.into(),
                snr_db: snr(snr4),
                rssi,
                late,
            },
            Event::Location {
                from,
                group,
                position,
                at,
                snr4,
            } => Self::Location {
                end: position.end(),
                position: PeerPosition::new(
                    &StoredPosition {
                        from,
                        group,
                        position,
                        at,
                        snr4,
                    },
                    at,
                ),
            },
            Event::SharesChanged => Self::SharesChanged,
            Event::MediaRequested { from, .. } => Self::MediaRequested {
                from: from.to_vec(),
            },
            Event::SearchUpdated { target } => Self::SearchUpdated {
                target: target.to_vec(),
            },
            Event::Searched { gid, by } => Self::Searched {
                gid: gid.to_vec(),
                by: by.to_vec(),
            },
            Event::TxPower { dbm, previous } => Self::TxPower { dbm, previous },
            Event::Advert { flood, sent } => Self::Advert { flood, sent },
            // The apps read the nodes with the contacts.
            Event::NodesUpdated => Self::ContactsUpdated,
            Event::NodeTool { node } => Self::NodeUpdated {
                node: node.to_vec(),
            },
            Event::Beacon {
                gid,
                from,
                sos,
                battery,
            } => Self::Beacon {
                gid: gid.to_vec(),
                from: from.to_vec(),
                sos,
                battery,
            },
            Event::Requested { node, identity } => Self::IdentityRequested {
                node: node.to_vec(),
                identity: identity.to_vec(),
            },
            Event::RequestMessage { .. } | Event::RequestAccepted { .. } => Self::RequestsChanged,
        }
    }
}
