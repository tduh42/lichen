//! Plain MeshCore text with people who use stock clients (§15, ADR-0026): direct messages
//! and channel posts, which the companion encrypts, routes and acknowledges itself. lichen
//! drains the companion's queue, sends what stock clients send, and keeps these chats apart
//! from its own.

use std::collections::VecDeque;

use lc_meshcore::Command;
use sha2::{Digest, Sha256};

use crate::core::{Core, Error, Event, Expect, FailReason, MsgId, Outbound, PubKey, Stage};
use lc_proto::Millis;

/// A channel as the app knows it: `SHA-256(secret)[0..8]` (§15.3).
pub type ChannelId = [u8; 8];

/// Longest plain text (MeshCore's `MAX_TEXT_LEN`).
pub const TEXT_MAX: usize = lc_meshcore::command::TEXT_MAX;

/// Longest channel or node name.
pub const NAME_MAX: usize = lc_meshcore::command::CHANNEL_NAME_MAX;

/// MeshCore's public channel.
pub const PUBLIC_NAME: &str = "Public";
pub const PUBLIC_SECRET: [u8; 16] = [
    0x8b, 0x33, 0x87, 0xe9, 0xc5, 0xcd, 0xea, 0x6a, 0xc9, 0xe5, 0xed, 0xba, 0xa1, 0x15, 0xcd, 0x72,
];

/// No confirmation within the companion's timeout plus this: the next attempt (§15.2).
const CONFIRM_MARGIN_MS: Millis = 2_000;
/// Attempts of a direct message; the last one floods.
const ATTEMPTS: u8 = 3;
/// Retries of a received message are recognised for this long (§15.1).
const RECENT_MS: Millis = 3_600_000;
const RECENT_MAX: usize = 512;
/// The slot scan stops after this many free slots in a row (§15.3).
const EMPTY_RUN: u8 = 4;
/// Messages from an unknown key prefix, kept until the contacts are fetched again.
const HELD_MAX: usize = 32;

/// A channel slot of the companion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub index: u8,
    pub name: String,
    pub secret: [u8; 16],
}

impl Channel {
    pub fn id(&self) -> ChannelId {
        channel_id(&self.secret)
    }
}

/// A plain conversation: a node, a channel, or a room server's posts (§15.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlainChat {
    Direct(PubKey),
    Channel(ChannelId),
    Room(PubKey),
}

pub fn channel_id(secret: &[u8; 16]) -> ChannelId {
    let h = Sha256::digest(secret);
    let mut id = [0u8; 8];
    id.copy_from_slice(&h[..8]);
    id
}

/// A hashtag channel from what the user typed (`#hiking`, `Hiking`): its name and secret
/// (§15.3), or `None` when the tag is empty or has other characters than letters, digits
/// and `-`.
pub fn hashtag(tag: &str) -> Option<(String, [u8; 16])> {
    let tag = tag.trim().trim_start_matches('#').to_lowercase();
    if tag.is_empty()
        || tag.len() + 1 > NAME_MAX
        || !tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return None;
    }
    let secret = lc_meshcore::air::region_key(&tag);
    Some((format!("#{tag}"), secret))
}

/// The stock app's QR code for a channel (§15.3, MeshCore's `docs/qr_codes.md`):
/// `meshcore://channel/add?name=<percent-encoded>&secret=<32 hex digits>`.
pub fn channel_qr(name: &str, secret: &[u8; 16]) -> String {
    let mut out = String::from("meshcore://channel/add?name=");
    for &b in name.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out.push_str("&secret=");
    for b in secret {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Decodes a query value: `+` is a space, `%XX` a byte; `None` for a bad escape or text
/// that is not UTF-8.
fn query_value(v: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(v.len());
    let mut it = v.bytes();
    while let Some(b) = it.next() {
        match b {
            b'+' => bytes.push(b' '),
            b'%' => {
                let hi = char::from(it.next()?).to_digit(16)?;
                let lo = char::from(it.next()?).to_digit(16)?;
                bytes.push(u8::try_from(hi << 4 | lo).ok()?);
            }
            b => bytes.push(b),
        }
    }
    String::from_utf8(bytes).ok()
}

/// A channel from the stock app's QR code (§15.3): its name and secret, or `None` when the
/// text is not one, the name is empty or longer than a slot takes, or the secret is not 16
/// bytes (all zero is no secret). Other parameters (`region_scope`) are ignored.
pub fn parse_channel_qr(text: &str) -> Option<(String, [u8; 16])> {
    let text = text.trim();
    let (base, query) = text.split_once('?')?;
    if !base.eq_ignore_ascii_case("meshcore://channel/add") {
        return None;
    }
    let (mut name, mut secret) = (None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "name" => name = Some(query_value(v)?),
            "secret" => {
                let hex = query_value(v)?;
                if hex.len() != 32 {
                    return None;
                }
                let mut out = [0u8; 16];
                for (o, i) in out.iter_mut().zip((0..32).step_by(2)) {
                    *o = u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()?;
                }
                secret = Some(out);
            }
            _ => {}
        }
    }
    let name = name?.trim().to_owned();
    let secret = secret.filter(|s| *s != [0; 16])?;
    (!name.is_empty() && name.len() <= NAME_MAX && !name.contains('\0')).then_some((name, secret))
}

/// Splits a channel post into its author's label and its body ("name: body", §15.1).
pub fn split_post(text: &str) -> (Option<&str>, &str) {
    match text.split_once(": ") {
        Some((name, body)) if !name.is_empty() && name.len() <= NAME_MAX => (Some(name), body),
        _ => (None, text),
    }
}

/// On-air bytes of a direct message over `path_bytes` (§15.5).
pub fn direct_bytes(text_len: usize, path_bytes: usize) -> usize {
    2 + path_bytes + 1 + 1 + 2 + (5 + text_len).div_ceil(16) * 16
}

/// On-air bytes of a channel post by `name` (§15.5).
pub fn channel_bytes(text_len: usize, name_len: usize) -> usize {
    2 + 1 + 2 + (5 + name_len + 2 + text_len).div_ceil(16) * 16
}

#[derive(Clone, Debug)]
struct Outgoing {
    id: MsgId,
    to: PubKey,
    text: String,
    ts: u32,
    attempt: u8,
}

#[derive(Clone, Debug)]
struct InFlight {
    msg: Outgoing,
    /// The `ack` of `SENT`, once the companion answered.
    tag: Option<u32>,
    /// The next attempt is due then (offline: frozen).
    deadline: Option<Millis>,
}

#[derive(Clone, Debug)]
enum Held {
    Direct {
        prefix: [u8; 6],
        text: String,
        ts: u32,
        snr4: Option<i8>,
        path_len: u8,
    },
    Channel {
        index: u8,
        text: String,
        ts: u32,
        snr4: Option<i8>,
        path_len: u8,
    },
}

/// The core's plain-text state.
#[derive(Debug, Default)]
pub(crate) struct Plain {
    /// A `SYNC_NEXT_MESSAGE` is queued or awaited.
    pub(crate) syncing: bool,
    channels: Vec<Channel>,
    /// The channel slots have all been read since the connection.
    scanned: bool,
    empty_run: u8,
    max_channels: Option<u8>,
    queue: VecDeque<Outgoing>,
    in_flight: Option<InFlight>,
    held: Vec<Held>,
    recent: VecDeque<([u8; 16], Millis)>,
}

/// One received plain message, as the app records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlainMessage {
    pub chat: PlainChat,
    /// The sender's node (direct messages).
    pub from: Option<PubKey>,
    /// The sender's name: the contact's, or the label of a channel post.
    pub label: Option<String>,
    pub text: String,
    /// The sender's timestamp.
    pub sent_at: u32,
    pub snr4: Option<i8>,
    /// Hops of the flood path; `None` when it came direct.
    pub hops: Option<u8>,
}

fn hops(path_len: u8) -> Option<u8> {
    (path_len != 0xFF).then_some(path_len & 0x3F)
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Core {
    // ----- app requests ----------------------------------------------------------------

    /// The companion's channels, in slot order (§15.3).
    pub fn channels(&self) -> &[Channel] {
        &self.plain.channels
    }

    /// Whether every channel slot has been read since the connection.
    pub fn channels_known(&self) -> bool {
        self.plain.scanned
    }

    /// Sends a plain MeshCore direct message to node `to` (§15.2). It waits while the
    /// companion is away; its outcome comes as `Delivered` or `Failed`.
    pub fn send_plain(
        &mut self,
        to: &PubKey,
        text: &str,
        sent_at: u32,
        now: Millis,
    ) -> Result<MsgId, Error> {
        if !self.contacts.contains_key(to) {
            return Err(Error::UnknownContact);
        }
        if text.is_empty() {
            return Err(Error::Empty);
        }
        if text.len() > TEXT_MAX {
            return Err(Error::TooLarge);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.plain.queue.push_back(Outgoing {
            id,
            to: *to,
            text: text.to_owned(),
            ts: sent_at,
            attempt: 0,
        });
        self.tick(now);
        Ok(id)
    }

    /// The longest post on a channel with our current name (§15.3).
    pub fn channel_text_max(&self) -> usize {
        TEXT_MAX.saturating_sub(2 + self.name.len())
    }

    /// Posts on channel `id` (§15.3); `GroupSent` once the companion took it.
    pub fn send_channel(
        &mut self,
        id: &ChannelId,
        text: &str,
        sent_at: u32,
        now: Millis,
    ) -> Result<MsgId, Error> {
        let Some(index) = self
            .plain
            .channels
            .iter()
            .find(|c| c.id() == *id)
            .map(|c| c.index)
        else {
            return Err(Error::UnknownGroup);
        };
        if text.is_empty() {
            return Err(Error::Empty);
        }
        if text.len() > self.channel_text_max() {
            return Err(Error::TooLarge);
        }
        if !self.online() {
            return Err(Error::NoIdentity);
        }
        let msg = self.next_id;
        self.next_id += 1;
        self.command(
            &Command::SendChannelTxtMsg {
                kind: 0,
                channel: index,
                ts: sent_at,
                text: text.as_bytes(),
            },
            Expect::ChannelPost(msg),
        );
        let airtime = self.airtime_ms(channel_bytes(text.len(), self.name.len()));
        self.record_airtime(now, airtime, None);
        self.tick(now);
        Ok(msg)
    }

    /// Airtime of a plain message (§15.5): a direct one to `to`, or a channel post.
    pub fn estimate_plain(&self, chat: &PlainChat, text_len: usize) -> Millis {
        let bytes = match chat {
            PlainChat::Direct(to) => {
                let path = self
                    .contacts
                    .get(to)
                    .and_then(|c| c.out_path.as_ref())
                    .map_or(0, |p| p.bytes.len());
                direct_bytes(text_len, path)
            }
            PlainChat::Channel(_) => channel_bytes(text_len, self.name.len()),
            PlainChat::Room(to) => direct_bytes(
                text_len,
                self.contacts
                    .get(to)
                    .and_then(|c| c.out_path.as_ref())
                    .map_or(0, |p| p.bytes.len()),
            ),
        };
        self.airtime_ms(bytes)
    }

    /// Adds a channel to the first free slot (§15.3). Returns its id; `ChannelsUpdated`
    /// follows once the companion took it. A channel already there is not added again.
    pub fn join_channel(&mut self, name: &str, secret: [u8; 16]) -> Result<ChannelId, Error> {
        let id = channel_id(&secret);
        if self.plain.channels.iter().any(|c| c.id() == id) {
            return Ok(id);
        }
        if name.is_empty() || name.len() > NAME_MAX || name.contains('\0') {
            return Err(Error::Empty);
        }
        if !self.plain.scanned {
            return Err(Error::NoIdentity);
        }
        let used: Vec<u8> = self.plain.channels.iter().map(|c| c.index).collect();
        let max = self.plain.max_channels.unwrap_or(8);
        let Some(index) = (0..max).find(|i| !used.contains(i)) else {
            return Err(Error::TooLarge);
        };
        let channel = Channel {
            index,
            name: name.to_owned(),
            secret,
        };
        self.command(
            &Command::SetChannel {
                index,
                name: name.as_bytes(),
                secret: &secret,
            },
            Expect::ChannelSet(index),
        );
        // Shown at once; the slot is read again to confirm.
        self.plain.channels.push(channel);
        self.plain.channels.sort_by_key(|c| c.index);
        self.command(&Command::GetChannel { index }, Expect::Channel(index));
        Ok(id)
    }

    /// Frees channel `id`'s slot (§15.3).
    pub fn leave_channel(&mut self, id: &ChannelId) -> Result<(), Error> {
        let Some(pos) = self.plain.channels.iter().position(|c| c.id() == *id) else {
            return Err(Error::UnknownGroup);
        };
        let index = self.plain.channels.remove(pos).index;
        self.command(
            &Command::SetChannel {
                index,
                name: b"",
                secret: &[0; 16],
            },
            Expect::ChannelSet(index),
        );
        self.events.push_back(Event::ChannelsUpdated);
        Ok(())
    }

    /// Sets the name our node advertises (§15.4) and announces it.
    pub fn set_node_name(&mut self, name: &str, now: Millis) -> Result<(), Error> {
        let name = name.trim();
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(Error::TooLarge);
        }
        self.command(
            &Command::SetAdvertName {
                name: name.as_bytes(),
            },
            Expect::Ok,
        );
        self.name = name.to_owned();
        self.send_advert(true, now);
        Ok(())
    }

    // ----- companion side --------------------------------------------------------------

    /// After `GET_CONTACTS` at connection: read the channel slots and drain the queue.
    pub(crate) fn plain_connected(&mut self) {
        self.plain.scanned = false;
        self.plain.empty_run = 0;
        self.plain.channels.clear();
        self.command(&Command::GetChannel { index: 0 }, Expect::Channel(0));
        self.plain.syncing = false;
        self.plain_sync();
    }

    /// The companion is gone: what waited for an answer goes again later (§6.1).
    pub(crate) fn plain_disconnected(&mut self) {
        self.plain.syncing = false;
        if let Some(f) = self.plain.in_flight.take() {
            self.plain.queue.push_front(f.msg);
        }
    }

    /// Asks for the next queued message, unless a request is already out.
    pub(crate) fn plain_sync(&mut self) {
        if !self.plain.syncing {
            self.plain.syncing = true;
            self.command(&Command::SyncNextMessage, Expect::Sync);
        }
    }

    pub(crate) fn plain_set_max_channels(&mut self, max: Option<u8>) {
        self.plain.max_channels = max;
    }

    /// A direct message from the queue (§15.1).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn on_contact_msg(
        &mut self,
        prefix: &[u8; 6],
        kind: u8,
        ts: u32,
        text: &[u8],
        snr4: Option<i8>,
        path_len: u8,
        now: Millis,
    ) {
        match kind {
            lc_meshcore::codes::txt::PLAIN => {}
            lc_meshcore::codes::txt::CLI_DATA => return self.on_cli_reply(prefix, ts, text),
            lc_meshcore::codes::txt::SIGNED_PLAIN => {
                return self.on_room_post(prefix, ts, text, snr4, path_len, now);
            }
            _ => return,
        }
        let text = text_of(text);
        if !self.deliver_direct(prefix, &text, ts, snr4, path_len, now)
            && self.plain.held.len() < HELD_MAX
        {
            self.plain.held.push(Held::Direct {
                prefix: *prefix,
                text,
                ts,
                snr4,
                path_len,
            });
        }
    }

    /// A channel post from the queue (§15.1).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn on_channel_msg(
        &mut self,
        index: u8,
        kind: u8,
        ts: u32,
        text: &[u8],
        snr4: Option<i8>,
        path_len: u8,
        now: Millis,
    ) {
        if kind != 0 {
            return;
        }
        let text = text_of(text);
        if !self.plain.scanned {
            if self.plain.held.len() < HELD_MAX {
                self.plain.held.push(Held::Channel {
                    index,
                    text,
                    ts,
                    snr4,
                    path_len,
                });
            }
            return;
        }
        self.deliver_channel(index, &text, ts, snr4, path_len, now);
    }

    fn deliver_direct(
        &mut self,
        prefix: &[u8; 6],
        text: &str,
        ts: u32,
        snr4: Option<i8>,
        path_len: u8,
        now: Millis,
    ) -> bool {
        let Some((key, name)) = self
            .contacts
            .values()
            .find(|c| c.key.starts_with(prefix))
            .map(|c| (c.key, c.name.clone()))
        else {
            return false;
        };
        if self.plain_seen(&PlainChat::Direct(key), ts, text, now) {
            return true;
        }
        self.events.push_back(Event::PlainReceived(PlainMessage {
            chat: PlainChat::Direct(key),
            from: Some(key),
            label: Some(name),
            text: text.to_owned(),
            sent_at: ts,
            snr4,
            hops: hops(path_len),
        }));
        true
    }

    fn deliver_channel(
        &mut self,
        index: u8,
        text: &str,
        ts: u32,
        snr4: Option<i8>,
        path_len: u8,
        now: Millis,
    ) {
        let Some(id) = self
            .plain
            .channels
            .iter()
            .find(|c| c.index == index)
            .map(Channel::id)
        else {
            return;
        };
        let chat = PlainChat::Channel(id);
        if self.plain_seen(&chat, ts, text, now) {
            return;
        }
        let (label, body) = split_post(text);
        self.events.push_back(Event::PlainReceived(PlainMessage {
            chat,
            from: None,
            label: label.map(str::to_owned),
            text: body.to_owned(),
            sent_at: ts,
            snr4,
            hops: hops(path_len),
        }));
    }

    /// Whether the same message was received in the last hour (a retry, §15.1); records it.
    pub(crate) fn plain_seen(
        &mut self,
        chat: &PlainChat,
        ts: u32,
        text: &str,
        now: Millis,
    ) -> bool {
        let mut h = Sha256::new();
        match chat {
            PlainChat::Direct(k) => {
                h.update([0]);
                h.update(k);
            }
            PlainChat::Channel(c) => {
                h.update([1]);
                h.update(c);
            }
            PlainChat::Room(k) => {
                h.update([2]);
                h.update(k);
            }
        }
        h.update(ts.to_le_bytes());
        h.update(text.as_bytes());
        let mut digest = [0u8; 16];
        digest.copy_from_slice(&h.finalize()[..16]);
        let recent = &mut self.plain.recent;
        while recent
            .front()
            .is_some_and(|(_, t)| now.saturating_sub(*t) > RECENT_MS)
            || recent.len() >= RECENT_MAX
        {
            recent.pop_front();
        }
        if recent.iter().any(|(d, _)| *d == digest) {
            return true;
        }
        recent.push_back((digest, now));
        false
    }

    /// The contacts were fetched again: messages from prefixes unknown until now.
    pub(crate) fn plain_contacts_updated(&mut self, now: Millis) {
        let held = std::mem::take(&mut self.plain.held);
        for h in held {
            if let Held::Direct {
                prefix,
                text,
                ts,
                snr4,
                path_len,
            } = h
            {
                // Still unknown: dropped (§15.1).
                self.deliver_direct(&prefix, &text, ts, snr4, path_len, now);
            } else {
                self.plain.held.push(h);
            }
        }
    }

    /// `CHANNEL_INFO` for slot `index`, or `None` for an `ERR` (past the last slot).
    pub(crate) fn on_channel_info(
        &mut self,
        index: u8,
        slot: Option<(&[u8], &[u8; 16])>,
        now: Millis,
    ) {
        let scanning = !self.plain.scanned;
        self.plain.channels.retain(|c| c.index != index);
        let used = slot.filter(|(name, secret)| !name.is_empty() || secret.iter().any(|&b| b != 0));
        match used {
            Some((name, secret)) => {
                self.plain.empty_run = 0;
                self.plain.channels.push(Channel {
                    index,
                    name: text_of(name),
                    secret: *secret,
                });
                self.plain.channels.sort_by_key(|c| c.index);
            }
            None => self.plain.empty_run = self.plain.empty_run.saturating_add(1),
        }
        if !scanning {
            self.events.push_back(Event::ChannelsUpdated);
            return;
        }
        let next = index.wrapping_add(1);
        let done = slot.is_none()
            || self.plain.empty_run >= EMPTY_RUN
            || self.plain.max_channels.is_some_and(|m| next >= m)
            || next == 0;
        if done {
            self.plain.scanned = true;
            self.events.push_back(Event::ChannelsUpdated);
            let held = std::mem::take(&mut self.plain.held);
            for h in held {
                match h {
                    Held::Channel {
                        index,
                        text,
                        ts,
                        snr4,
                        path_len,
                    } => self.deliver_channel(index, &text, ts, snr4, path_len, now),
                    direct => self.plain.held.push(direct),
                }
            }
        } else {
            self.command(&Command::GetChannel { index: next }, Expect::Channel(next));
        }
    }

    /// `SENT` for direct message `id`: its acknowledgement tag and timeout.
    pub(crate) fn on_plain_sent(&mut self, id: MsgId, tag: u32, timeout_ms: u32, now: Millis) {
        if let Some(f) = self.plain.in_flight.as_mut()
            && f.msg.id == id
        {
            f.tag = Some(tag);
            f.deadline = Some(now + Millis::from(timeout_ms) + CONFIRM_MARGIN_MS);
        }
    }

    /// The companion refused direct message `id` (`ERR`).
    pub(crate) fn on_plain_refused(&mut self, id: MsgId) {
        if self
            .plain
            .in_flight
            .as_ref()
            .is_some_and(|f| f.msg.id == id)
        {
            self.plain.in_flight = None;
            self.events.push_back(Event::Failed {
                id,
                reason: FailReason::Refused,
            });
        }
    }

    /// Push `SEND_CONFIRMED`.
    pub(crate) fn on_send_confirmed(&mut self, ack: u32) {
        if self
            .plain
            .in_flight
            .as_ref()
            .is_some_and(|f| f.tag == Some(ack))
            && let Some(f) = self.plain.in_flight.take()
        {
            self.events.push_back(Event::Delivered { id: f.msg.id });
        }
    }

    /// Starts the next direct message, or the next attempt of the one in flight.
    pub(crate) fn tick_plain(&mut self, now: Millis) {
        if let Some(f) = &self.plain.in_flight {
            if f.deadline.is_none_or(|d| now < d) {
                return;
            }
            let Some(mut f) = self.plain.in_flight.take() else {
                return;
            };
            f.msg.attempt += 1;
            if f.msg.attempt >= ATTEMPTS {
                self.events.push_back(Event::Failed {
                    id: f.msg.id,
                    reason: FailReason::Timeout,
                });
                return self.tick_plain(now);
            }
            self.transmit_plain(f.msg, now);
            return;
        }
        let Some(next) = self.plain.queue.front() else {
            return;
        };
        let airtime = self.estimate_plain(&PlainChat::Direct(next.to), next.text.len());
        if !self.budget_allows(now, airtime) {
            return;
        }
        if let Some(msg) = self.plain.queue.pop_front() {
            self.transmit_plain(msg, now);
        }
    }

    fn transmit_plain(&mut self, msg: Outgoing, now: Millis) {
        let Some(prefix) = msg.to.get(..6).and_then(|p| <[u8; 6]>::try_from(p).ok()) else {
            return;
        };
        // The last attempt floods (§15.2).
        if msg.attempt + 1 == ATTEMPTS {
            self.command(&Command::ResetPath { key: &msg.to }, Expect::Ok);
            if let Some(c) = self.contacts.get_mut(&msg.to) {
                c.out_path = None;
            }
        }
        self.command(
            &Command::SendTxtMsg {
                kind: 0,
                attempt: msg.attempt,
                ts: msg.ts,
                prefix: &prefix,
                text: msg.text.as_bytes(),
            },
            Expect::PlainSent(msg.id),
        );
        let path = self.contacts.get(&msg.to).and_then(|c| c.out_path.clone());
        let bytes = direct_bytes(msg.text.len(), path.as_ref().map_or(0, |p| p.bytes.len()));
        let airtime = self.airtime_ms(bytes);
        let hops = path.as_ref().map(|p| p.hops());
        self.record_airtime(now, airtime, hops);
        self.plain.in_flight = Some(InFlight {
            msg,
            tag: None,
            deadline: None,
        });
    }

    pub(crate) fn next_plain_wakeup(&self) -> Option<Millis> {
        match &self.plain.in_flight {
            Some(f) => f.deadline,
            None => (!self.plain.queue.is_empty()).then_some(0),
        }
    }

    /// Where plain direct message `id` stands (§6.1).
    pub(crate) fn plain_outgoing(&self, id: MsgId) -> Option<Outbound> {
        if self
            .plain
            .in_flight
            .as_ref()
            .is_some_and(|f| f.msg.id == id)
        {
            return Some(Outbound {
                stage: if self.online() {
                    Stage::Transmitting
                } else {
                    Stage::Offline
                },
                started: true,
            });
        }
        let pos = self.plain.queue.iter().position(|m| m.id == id)?;
        let started = self.plain.queue.get(pos).is_some_and(|m| m.attempt > 0);
        let stage = if !self.online() {
            Stage::Offline
        } else if pos > 0 || self.plain.in_flight.is_some() {
            Stage::Behind
        } else {
            Stage::Transmitting
        };
        Some(Outbound { stage, started })
    }

    /// Cancels plain message `id` if it has not left; `None` when it is not a plain one.
    pub(crate) fn plain_cancel(&mut self, id: MsgId) -> Option<Result<(), Error>> {
        if self
            .plain
            .in_flight
            .as_ref()
            .is_some_and(|f| f.msg.id == id)
        {
            return Some(Err(Error::Started));
        }
        let pos = self.plain.queue.iter().position(|m| m.id == id)?;
        if self.plain.queue.get(pos).is_some_and(|m| m.attempt > 0) {
            return Some(Err(Error::Started));
        }
        self.plain.queue.remove(pos);
        Some(Ok(()))
    }

    /// Gives up plain message `id` (expired, §6.1); whether it was found.
    pub(crate) fn plain_abandon(&mut self, id: MsgId) -> bool {
        let found = if self
            .plain
            .in_flight
            .as_ref()
            .is_some_and(|f| f.msg.id == id)
        {
            self.plain.in_flight = None;
            true
        } else if let Some(pos) = self.plain.queue.iter().position(|m| m.id == id) {
            self.plain.queue.remove(pos);
            true
        } else {
            false
        };
        if found {
            self.events.push_back(Event::Failed {
                id,
                reason: FailReason::Expired,
            });
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;

    /// Bob's node: its prefix is the vectors' 808182838485.
    const BOB: PubKey = {
        let mut k = [0x86; 32];
        let mut i = 0;
        while i < 6 {
            k[i] = 0x80 + i as u8;
            i += 1;
        }
        k
    };

    fn frame(h: &str) -> Vec<u8> {
        hex::decode(h).unwrap()
    }

    /// `CONTACT` for `key`, no path.
    fn contact(key: PubKey, name: &str) -> Vec<u8> {
        let mut f = vec![3];
        f.extend_from_slice(&key);
        f.extend_from_slice(&[1, 0, 0xFF]);
        f.extend_from_slice(&[0; 64]);
        let mut n = [0u8; 32];
        n[..name.len()].copy_from_slice(name.as_bytes());
        f.extend_from_slice(&n);
        f.extend_from_slice(&[0; 16]);
        f
    }

    fn channel_info(index: u8, name: &str, secret: &[u8; 16]) -> Vec<u8> {
        let mut f = vec![0x12, index];
        let mut n = [0u8; 32];
        n[..name.len()].copy_from_slice(name.as_bytes());
        f.extend_from_slice(&n);
        f.extend_from_slice(secret);
        f
    }

    /// Runs the companion side: every command the core sends is answered by `answer`.
    fn serve(core: &mut Core, now: Millis, mut answer: impl FnMut(&[u8]) -> Vec<Vec<u8>>) {
        while let Some(cmd) = core.poll_companion(now) {
            for r in answer(&cmd) {
                core.on_companion_frame(&r, now);
            }
        }
    }

    /// A companion with Bob as a contact, Public in slot 0, and `queue` waiting.
    fn connected(queue: Vec<Vec<u8>>) -> (Core, Vec<Event>) {
        let mut core = Core::new(Config::new("t", [200; 32], [1; 32]));
        let mut queue: VecDeque<Vec<u8>> = queue.into();
        core.connect(0);
        serve(&mut core, 0, |cmd| match cmd[0] {
            1 => vec![frame(
                "05011616101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f000000000000000000000000f2440d0024f4000008084d657368436f72652d63646466",
            )],
            22 => vec![frame(
                "0d0eaf2840e201003135204f6374203230323600456c6563726f77205468696e6b4e6f6465204d35000000000000000000000000000000000000000076312e31382e30000000000000000000000000000000",
            )],
            64 => vec![vec![0x1c]],
            // The radio's settings (§1.1): no board settings, auto-add off, a battery.
            40 => vec![vec![21]],
            59 => vec![vec![25, 0, 0]],
            20 => vec![vec![12, 0xAC, 0x0F]],
            4 => vec![
                vec![2, 1, 0, 0, 0],
                contact(BOB, "Bob"),
                vec![4, 0, 0, 0, 0],
            ],
            31 if cmd[1] == 0 => vec![channel_info(0, PUBLIC_NAME, &PUBLIC_SECRET)],
            31 => vec![channel_info(cmd[1], "", &[0; 16])],
            10 => vec![queue.pop_front().unwrap_or_else(|| vec![10])],
            _ => vec![vec![0]],
        });
        let events = std::iter::from_fn(|| core.poll_event()).collect();
        (core, events)
    }

    fn received(events: &[Event]) -> Vec<&PlainMessage> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::PlainReceived(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    /// At connection the slots are read and the companion's queue is drained (§15.1): a direct
    /// message from Bob, a post on Public, a retry of it (dropped), and a signed post from Bob,
    /// who is not a room server (dropped, §15.8).
    #[test]
    fn the_queue_is_drained_at_connection() {
        let post = frame("111400000000004002df6a616c6963653a206869");
        let (core, events) = connected(vec![
            frame("10280000808182838485ff004002df6a68656c6c6f"),
            post.clone(),
            post,
            frame("10280000808182838485ff024002df6a0102030468656c6c6f"),
        ]);
        assert!(core.channels_known());
        assert_eq!(core.channels().len(), 1);
        assert!(events.contains(&Event::ChannelsUpdated));
        let got = received(&events);
        assert_eq!(got.len(), 2, "{events:?}");
        assert_eq!(got[0].chat, PlainChat::Direct(BOB));
        assert_eq!(
            (got[0].label.as_deref(), got[0].text.as_str()),
            (Some("Bob"), "hello")
        );
        assert_eq!((got[0].snr4, got[0].hops), (Some(40), None));
        assert_eq!(got[1].chat, PlainChat::Channel(channel_id(&PUBLIC_SECRET)));
        assert_eq!(
            (got[1].label.as_deref(), got[1].text.as_str()),
            (Some("alice"), "hi")
        );
        assert_eq!(got[1].hops, Some(0));
    }

    /// A direct message goes three times when unconfirmed, the last time after `RESET_PATH`,
    /// then is delivered by the last confirmation (§15.2).
    #[test]
    fn a_direct_message_is_tried_three_times() {
        let (mut core, _) = connected(vec![]);
        let id = core
            .send_plain(&BOB, "salut", 1_793_000_000, 1_000)
            .unwrap();
        let mut seen = Vec::new();
        let mut tag = 0u32;
        let mut now = 1_000;
        for _ in 0..3 {
            serve(&mut core, now, |cmd| {
                seen.push(cmd.to_vec());
                if cmd[0] == 2 {
                    tag += 1;
                    let mut f = vec![6, 0];
                    f.extend_from_slice(&tag.to_le_bytes());
                    f.extend_from_slice(&1_000u32.to_le_bytes());
                    vec![f]
                } else {
                    vec![vec![0]]
                }
            });
            assert_eq!(core.outgoing(id, now).map(|o| o.started), Some(true));
            now += 3_001;
            core.tick(now);
        }
        let sends: Vec<&Vec<u8>> = seen.iter().filter(|c| c[0] == 2).collect();
        assert_eq!(sends.iter().map(|c| c[2]).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(&sends[0][7..13], &BOB[..6]);
        assert_eq!(&sends[0][13..], b"salut");
        let reset = seen.iter().position(|c| c[0] == 13).unwrap();
        assert!(reset < seen.iter().rposition(|c| c[0] == 2).unwrap());
        // The third timed out too: failed.
        assert!(std::iter::from_fn(|| core.poll_event()).any(|e| e
            == Event::Failed {
                id,
                reason: FailReason::Timeout
            }));

        // Another one, confirmed at its first attempt.
        let id = core.send_plain(&BOB, "encore", 1_793_000_100, now).unwrap();
        serve(&mut core, now, |cmd| {
            if cmd[0] == 2 {
                vec![frame("0600deadbeef88130000")]
            } else {
                vec![vec![0]]
            }
        });
        core.on_companion_frame(&frame("82deadbeefd2040000"), now + 500);
        assert!(std::iter::from_fn(|| core.poll_event()).any(|e| e == Event::Delivered { id }));
        assert!(core.outgoing(id, now).is_none());
    }

    /// Joining fills the first free slot; a hashtag's secret is derived (§15.3).
    #[test]
    fn joining_a_channel_takes_a_free_slot() {
        let (mut core, _) = connected(vec![]);
        let (name, secret) = hashtag("rando").unwrap();
        let id = core.join_channel(&name, secret).unwrap();
        let mut set = None;
        serve(&mut core, 0, |cmd| match cmd[0] {
            32 => {
                set = Some(cmd.to_vec());
                vec![vec![0]]
            }
            31 => vec![channel_info(1, "#rando", &secret)],
            _ => vec![vec![0]],
        });
        let set = set.unwrap();
        assert_eq!(set[1], 1);
        assert_eq!(&set[2..8], b"#rando");
        assert_eq!(&set[34..50], &secret);
        assert_eq!(
            core.channels().iter().map(Channel::id).collect::<Vec<_>>()[1],
            id
        );
        // Already there: not added twice.
        assert_eq!(core.join_channel(&name, secret).unwrap(), id);
        assert!(core.poll_companion(0).is_none());
        // A post on it.
        let post = core
            .send_channel(&id, "on part à 8 h", 1_793_000_000, 0)
            .unwrap();
        serve(&mut core, 0, |cmd| {
            assert_eq!(&cmd[..3], &[3, 0, 1]);
            vec![vec![0]]
        });
        assert!(
            std::iter::from_fn(|| core.poll_event()).any(|e| e == Event::GroupSent { id: post })
        );
    }

    #[test]
    fn hashtags_derive_like_stock_clients() {
        let (name, secret) = hashtag(" #Test ").unwrap();
        assert_eq!(name, "#test");
        // `air.toml`'s `grp_data_test` channel: hash 0xD9 (ADR-0008).
        assert_eq!(secret, lc_meshcore::air::region_key("test"));
        assert_eq!(Sha256::digest(secret)[0], 0xD9);
        assert!(hashtag("#").is_none());
        assert!(hashtag("rando vercors").is_none());
        assert!(hashtag("été").is_none());
        assert_eq!(hashtag("rando-vercors").unwrap().0, "#rando-vercors");
        // The public channel's hash, 0x11 (ADR-0008).
        assert_eq!(Sha256::digest(PUBLIC_SECRET)[0], 0x11);
    }

    #[test]
    fn posts_split_into_label_and_body() {
        assert_eq!(split_post("alice: hi"), (Some("alice"), "hi"));
        assert_eq!(split_post("alice: a: b"), (Some("alice"), "a: b"));
        assert_eq!(split_post("no label"), (None, "no label"));
        assert_eq!(split_post(": empty"), (None, ": empty"));
    }

    #[test]
    fn sizes_round_to_blocks() {
        // "hello": 5 + 5 = 10 → one block.
        assert_eq!(direct_bytes(5, 0), 2 + 1 + 1 + 2 + 16);
        assert_eq!(direct_bytes(11, 2), 2 + 2 + 1 + 1 + 2 + 16);
        assert_eq!(direct_bytes(12, 0), 2 + 1 + 1 + 2 + 32);
        assert_eq!(channel_bytes(5, 5), 2 + 1 + 2 + 32);
    }
}
