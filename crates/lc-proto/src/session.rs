//! An established session with one peer (§5, §7, §8): per-direction keys, the `tag` counter,
//! one datagram in flight each way, the replay window, and the sealing and opening of DATA, ACK
//! and ABORT frames.

use crate::header::{FrameError, Header, Kind};
use crate::receiver::{Reassembly, RxEmit, RxState, Stored};
use crate::sender::{SendEmit, SendError, SendState, Sender, Timing};
use crate::window::{ReplayWindow, Verdict};
use crate::{HEADER_LEN, Key, Millis, TAG_LEN, aead};

/// Session keys, one per direction. Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionKeys {
    pub send: Key,
    pub recv: Key,
}

impl core::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SessionKeys { .. }")
    }
}

/// Things the app should know about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// Our datagram `tag` was fully acknowledged.
    Delivered { tag: u16 },
    /// Our datagram `tag` was given up (too many rounds, or cancelled).
    Failed { tag: u16 },
    /// The peer refused our datagram `tag`.
    PeerAborted { tag: u16, reason: u8 },
    /// An incoming datagram `tag` was given up.
    ReceiveFailed { tag: u16 },
}

/// Result of handling a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Received<'a> {
    Nothing,
    Datagram { tag: u16, data: &'a [u8] },
}

/// Send `tag`s from this value on require a new session (§4.4).
pub const TAG_LIMIT: u16 = 0xFF00;
const EVENTS: usize = 8;

pub struct Session {
    keys: SessionKeys,
    local: u8,
    peer: u8,
    next_tag: u16,
    timing: Timing,
    sender: Option<Sender>,
    sender_reported: bool,
    rx: Option<Reassembly>,
    window: ReplayWindow,
    events: [Option<SessionEvent>; EVENTS],
}

impl core::fmt::Debug for Session {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Session")
            .field("local", &self.local)
            .field("peer", &self.peer)
            .field("next_tag", &self.next_tag)
            .field("sender", &self.sender)
            .field("rx", &self.rx)
            .finish_non_exhaustive()
    }
}

fn seal_frame(key: &Key, header: &Header, plain: &[u8], out: &mut [u8]) -> Option<usize> {
    let len = HEADER_LEN + plain.len() + TAG_LEN;
    let frame = out.get_mut(..len)?;
    let (head, rest) = frame.split_at_mut(HEADER_LEN);
    head.copy_from_slice(&header.encode());
    let (ct, tag_slot) = rest.split_at_mut(plain.len());
    ct.copy_from_slice(plain);
    let tag = aead::seal(key, &header.nonce(), head, ct);
    tag_slot.copy_from_slice(&tag);
    Some(len)
}

impl Session {
    /// A session between `local` and `peer` (identity hashes) with fresh keys.
    pub fn new(keys: SessionKeys, local: u8, peer: u8, timing: Timing) -> Self {
        Self {
            keys,
            local,
            peer,
            next_tag: 0,
            timing,
            sender: None,
            sender_reported: true,
            rx: None,
            window: ReplayWindow::new(),
            events: [None; EVENTS],
        }
    }

    /// Updates pacing and timers (path or preset changed). Applies to the next emissions.
    pub fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
    }

    pub const fn timing(&self) -> &Timing {
        &self.timing
    }

    /// True when a new datagram can be sent now.
    pub fn can_send(&self) -> bool {
        self.sender.as_ref().is_none_or(Sender::is_idle) && self.next_tag < TAG_LIMIT
    }

    /// True when the `tag` counter requires a new handshake.
    pub const fn exhausted(&self) -> bool {
        self.next_tag >= TAG_LIMIT
    }

    /// The datagram in flight: its `tag`, the number of rounds in a row without any ACK, and
    /// whether the receiver ever acknowledged it.
    pub fn in_flight(&self) -> Option<(u16, u8, bool)> {
        self.sender
            .as_ref()
            .filter(|s| matches!(s.state(), SendState::Sending | SendState::WaitingAck))
            .map(|s| (s.tag(), s.rounds_without_ack(), s.acknowledged()))
    }

    /// True when the last fragment of a round went out and the sender waits for the ACK.
    pub fn awaiting_ack(&self) -> bool {
        self.sender
            .as_ref()
            .is_some_and(|s| s.state() == SendState::WaitingAck)
    }

    /// Starts sending `datagram` in `chunk`-byte fragments. Returns its `tag`.
    pub fn send(&mut self, datagram: &[u8], chunk: usize, now: Millis) -> Result<u16, SendError> {
        if self.exhausted() {
            return Err(SendError::Exhausted);
        }
        if !self.can_send() {
            return Err(SendError::Busy);
        }
        let tag = self.next_tag;
        self.sender = Some(Sender::new(tag, datagram, chunk, now)?);
        self.sender_reported = false;
        self.next_tag += 1;
        Ok(tag)
    }

    /// Gives up the datagram in flight (an ABORT goes out).
    pub fn cancel(&mut self) {
        if let Some(s) = self.sender.as_mut() {
            s.cancel();
        }
        self.report_sender();
    }

    fn push_event(&mut self, event: SessionEvent) {
        if let Some(slot) = self.events.iter_mut().find(|e| e.is_none()) {
            *slot = Some(event);
        } else {
            self.events.rotate_left(1);
            if let Some(last) = self.events.last_mut() {
                *last = Some(event);
            }
        }
    }

    /// Oldest pending event.
    pub fn take_event(&mut self) -> Option<SessionEvent> {
        let first = self.events.first_mut()?.take();
        self.events.rotate_left(1);
        first
    }

    fn report_sender(&mut self) {
        if self.sender_reported {
            return;
        }
        let Some(s) = self.sender.as_ref() else {
            return;
        };
        let tag = s.tag();
        match s.state() {
            SendState::Delivered => self.push_event(SessionEvent::Delivered { tag }),
            SendState::Failed => self.push_event(SessionEvent::Failed { tag }),
            SendState::Sending | SendState::WaitingAck => return,
        }
        self.sender_reported = true;
    }

    /// Handles a frame addressed to this session. The frame is decrypted in place; a received
    /// datagram borrows from the session until the next call.
    pub fn handle(&mut self, frame: &mut [u8], now: Millis) -> Result<Received<'_>, FrameError> {
        let (h, _) = Header::decode(frame)?;
        if h.dst != self.local || h.src != self.peer {
            return Err(FrameError::NotForUs);
        }
        if h.kind == Kind::Control {
            return Err(FrameError::Malformed);
        }
        let ct_len = frame.len() - HEADER_LEN - TAG_LEN;
        let (head, rest) = frame.split_at_mut(HEADER_LEN);
        let (ct, tag) = rest.split_at_mut(ct_len);
        let tag: &[u8; TAG_LEN] = (&*tag).try_into().map_err(|_| FrameError::Length)?;
        aead::open(&self.keys.recv, &h.nonce(), head, ct, tag).map_err(|_| FrameError::Auth)?;
        let plain: &[u8] = ct;
        match h.kind {
            Kind::Data => self.on_data(&h, plain, now),
            Kind::Ack => {
                let bitmap = plain
                    .get(..8)
                    .and_then(|b| b.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(FrameError::Malformed)?;
                let complete = plain.get(8).is_some_and(|f| f & 1 == 1);
                let timing = self.timing;
                match self.sender.as_mut() {
                    Some(s) if s.tag() == h.tag && s.total() == h.total => {
                        s.on_ack(h.attempt, bitmap, complete, now, &timing);
                    }
                    _ => return Err(FrameError::Stale),
                }
                self.report_sender();
                Ok(Received::Nothing)
            }
            Kind::Abort => {
                let reason = plain.first().copied().unwrap_or(0);
                if h.index == 0 {
                    match self.rx.as_mut() {
                        Some(r) if r.tag() == h.tag && r.state() == RxState::Collecting => {
                            r.on_sender_abort();
                            self.push_event(SessionEvent::ReceiveFailed { tag: h.tag });
                        }
                        _ => return Err(FrameError::Stale),
                    }
                } else {
                    match self.sender.as_mut() {
                        Some(s)
                            if s.tag() == h.tag
                                && matches!(
                                    s.state(),
                                    SendState::Sending | SendState::WaitingAck
                                ) =>
                        {
                            s.on_peer_abort();
                            self.sender_reported = true;
                            self.push_event(SessionEvent::PeerAborted { tag: h.tag, reason });
                        }
                        _ => return Err(FrameError::Stale),
                    }
                }
                Ok(Received::Nothing)
            }
            Kind::Control => Err(FrameError::Malformed),
        }
    }

    fn on_data(
        &mut self,
        h: &Header,
        plain: &[u8],
        now: Millis,
    ) -> Result<Received<'_>, FrameError> {
        match self.window.check(h.tag) {
            Verdict::TooOld => return Err(FrameError::Stale),
            Verdict::Completed => {
                return match self.rx.as_mut() {
                    Some(r) if r.tag() == h.tag => {
                        r.request_complete_ack();
                        Ok(Received::Nothing)
                    }
                    _ => Err(FrameError::Stale),
                };
            }
            Verdict::New | Verdict::Pending => {}
        }
        let timing = self.timing;
        match self.rx.as_mut() {
            Some(r) if r.tag() == h.tag => {
                if r.total() != h.total {
                    return Err(FrameError::Malformed);
                }
            }
            // A delayed fragment of an older datagram: the sender has moved on.
            Some(r) if r.tag() > h.tag => return Err(FrameError::Stale),
            Some(r) => r.reset(h.tag, h.total, now, &timing),
            None => self.rx = Some(Reassembly::new(h.tag, h.total, now, &timing)),
        }
        self.window.see(h.tag);
        let stored = match self.rx.as_mut() {
            Some(r) => r.on_fragment(h.index, plain, now, &timing),
            None => return Err(FrameError::Stale),
        };
        match stored {
            Stored::Complete => {
                self.window.complete(h.tag);
                let data = self.rx.as_ref().map(Reassembly::datagram).unwrap_or(&[]);
                Ok(Received::Datagram { tag: h.tag, data })
            }
            Stored::Malformed => {
                self.push_event(SessionEvent::ReceiveFailed { tag: h.tag });
                Ok(Received::Nothing)
            }
            Stored::Rejected => Err(FrameError::Malformed),
            Stored::Duplicate | Stored::Stored => Ok(Received::Nothing),
        }
    }

    /// Writes the next frame due at `now` into `out` (at least [`crate::FRAME_MAX`] bytes) and
    /// returns its length. ACKs and ABORTs go before data.
    pub fn poll_transmit(&mut self, now: Millis, out: &mut [u8]) -> Option<usize> {
        self.poll_ack(now, out).or_else(|| self.poll_data(now, out))
    }

    /// Like [`Session::poll_transmit`], receiver side only: ACKs and the receiver's ABORT.
    pub fn poll_ack(&mut self, now: Millis, out: &mut [u8]) -> Option<usize> {
        let timing = self.timing;
        if let Some(r) = self.rx.as_mut() {
            let tag = r.tag();
            let total = r.total();
            let state_before = r.state();
            if let Some(emit) = r.poll(now, &timing) {
                if state_before == RxState::Collecting && r.state() == RxState::Failed {
                    self.push_event(SessionEvent::ReceiveFailed { tag });
                }
                let (header, plain, len) = match emit {
                    RxEmit::Ack {
                        seq,
                        bitmap,
                        complete,
                    } => {
                        let mut p = [0u8; 9];
                        if let Some(b) = p.get_mut(..8) {
                            b.copy_from_slice(&bitmap.to_le_bytes());
                        }
                        if let Some(f) = p.get_mut(8) {
                            *f = u8::from(complete);
                        }
                        (self.header(tag, Kind::Ack, 0, total, seq), p, 9)
                    }
                    RxEmit::Abort { reason } => {
                        let mut p = [0u8; 9];
                        p[0] = reason;
                        (self.header(tag, Kind::Abort, 1, total, 0), p, 1)
                    }
                };
                return seal_frame(&self.keys.send, &header, plain.get(..len)?, out);
            }
        }
        None
    }

    /// Like [`Session::poll_transmit`], sender side only: DATA fragments and the sender's ABORT.
    pub fn poll_data(&mut self, now: Millis, out: &mut [u8]) -> Option<usize> {
        let timing = self.timing;
        let s = self.sender.as_mut()?;
        let emit = s.poll(now, &timing);
        let (tag, total) = (s.tag(), s.total());
        let len = match emit? {
            SendEmit::Data { index, attempt } => {
                let header = Header {
                    dst: self.peer,
                    src: self.local,
                    tag,
                    kind: Kind::Data,
                    index,
                    total,
                    attempt,
                };
                let s = self.sender.as_ref()?;
                seal_frame(&self.keys.send, &header, s.fragment(index), out)
            }
            SendEmit::Abort { reason } => {
                let header = self.header(tag, Kind::Abort, 0, total, 0);
                seal_frame(&self.keys.send, &header, &[reason], out)
            }
        };
        self.report_sender();
        len
    }

    const fn header(&self, tag: u16, kind: Kind, index: u8, total: u8, attempt: u8) -> Header {
        Header {
            dst: self.peer,
            src: self.local,
            tag,
            kind,
            index,
            total,
            attempt,
        }
    }

    /// When [`Session::poll_ack`] should be called next.
    pub fn next_ack_wakeup(&self) -> Option<Millis> {
        self.rx.as_ref().and_then(Reassembly::next_wakeup)
    }

    /// When [`Session::poll_data`] should be called next.
    pub fn next_data_wakeup(&self) -> Option<Millis> {
        self.sender.as_ref().and_then(Sender::next_wakeup)
    }

    /// When `poll_transmit` should be called next (`Some(0)` = now).
    pub fn next_wakeup(&self) -> Option<Millis> {
        let rx = self.rx.as_ref().and_then(Reassembly::next_wakeup);
        let tx = self.sender.as_ref().and_then(Sender::next_wakeup);
        match (rx, tx) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
