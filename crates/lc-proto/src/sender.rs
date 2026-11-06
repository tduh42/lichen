//! Sender side of ACK-on-Error fragmentation (§7.1, §7.3, §7.4).

use crate::{CHUNK_MAX, DATAGRAM_MAX, MAX_FRAGMENTS, Millis, reason};

/// Pacing and timer parameters, computed by the caller from the path and the radio preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// Δ: spacing between two fragments handed to the companion.
    pub pace_ms: Millis,
    /// W: how long the sender waits for an ACK after the last fragment of a round.
    pub ack_wait_ms: Millis,
    /// R_max: hard cap on rounds, bounds the airtime of one datagram.
    pub max_rounds: u8,
    /// Consecutive ACKs confirming no new fragment before giving up.
    pub max_stalled_rounds: u8,
    /// Consecutive rounds without any ACK before giving up (peer unreachable).
    pub max_silent_rounds: u8,
    /// Receiver: assumed gap while only one fragment has arrived.
    pub rx_first_gap_ms: Millis,
    /// Receiver: smallest gap estimate.
    pub rx_min_gap_ms: Millis,
    /// Receiver: largest gap estimate (a long pause must not push the timers out forever).
    pub rx_max_gap_ms: Millis,
    /// Receiver: margin added to the reassembly timer.
    pub rx_margin_ms: Millis,
    /// Receiver: incomplete ACKs sent without progress before giving up.
    pub rx_max_acks: u8,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            pace_ms: 2_000,
            ack_wait_ms: 10_000,
            max_rounds: 32,
            max_stalled_rounds: 4,
            max_silent_rounds: 6,
            rx_first_gap_ms: 10_000,
            rx_min_gap_ms: 1_000,
            rx_max_gap_ms: 60_000,
            rx_margin_ms: 3_000,
            rx_max_acks: 4,
        }
    }
}

impl Timing {
    /// §7.3 and §7.4 from the airtime of a full frame, of an ACK frame, and the hop count.
    pub fn for_path(frame_airtime_ms: Millis, ack_airtime_ms: Millis, hops: u8) -> Self {
        let k = match hops {
            0 => 1,
            1 => 3,
            _ => 5,
        };
        Self {
            pace_ms: frame_airtime_ms * k + 100,
            ack_wait_ms: (Millis::from(hops) + 1) * (2 * frame_airtime_ms + 2 * ack_airtime_ms)
                + 2_000,
            ..Self::default()
        }
    }
}

/// Where the transfer stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendState {
    Sending,
    WaitingAck,
    Delivered,
    Failed,
}

/// What the sender wants on the air now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendEmit {
    Data { index: u8, attempt: u8 },
    Abort { reason: u8 },
}

/// Why a datagram cannot be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    Empty,
    TooLarge,
    BadChunk,
    /// A datagram is already in flight.
    Busy,
    /// The `tag` counter is exhausted: renew the session.
    Exhausted,
}

/// One datagram being sent.
pub struct Sender {
    tag: u16,
    total: u8,
    chunk: usize,
    len: usize,
    data: [u8; DATAGRAM_MAX],
    round: u8,
    /// Fragments still to send in this round.
    pending: u64,
    next_tx: Millis,
    ack_deadline: Millis,
    state: SendState,
    abort: Option<u8>,
    last_ack_seq: Option<u8>,
    rounds_without_ack: u8,
    /// Fragments the receiver has confirmed so far.
    acked: u64,
    stalled_rounds: u8,
}

impl core::fmt::Debug for Sender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Sender")
            .field("tag", &self.tag)
            .field("total", &self.total)
            .field("len", &self.len)
            .field("round", &self.round)
            .field("pending", &self.pending)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

const fn mask(total: u8) -> u64 {
    if total as usize >= MAX_FRAGMENTS {
        u64::MAX
    } else {
        (1u64 << total) - 1
    }
}

impl Sender {
    /// Prepares `datagram` cut into `chunk`-byte fragments; round 0 starts at `now`.
    pub fn new(tag: u16, datagram: &[u8], chunk: usize, now: Millis) -> Result<Self, SendError> {
        if datagram.is_empty() {
            return Err(SendError::Empty);
        }
        if chunk == 0 || chunk > CHUNK_MAX {
            return Err(SendError::BadChunk);
        }
        let total = datagram.len().div_ceil(chunk);
        if total > MAX_FRAGMENTS {
            return Err(SendError::TooLarge);
        }
        let total = total as u8;
        let mut data = [0u8; DATAGRAM_MAX];
        data.get_mut(..datagram.len())
            .ok_or(SendError::TooLarge)?
            .copy_from_slice(datagram);
        Ok(Self {
            tag,
            total,
            chunk,
            len: datagram.len(),
            data,
            round: 0,
            pending: mask(total),
            next_tx: now,
            ack_deadline: now,
            state: SendState::Sending,
            abort: None,
            last_ack_seq: None,
            rounds_without_ack: 0,
            acked: 0,
            stalled_rounds: 0,
        })
    }

    pub const fn tag(&self) -> u16 {
        self.tag
    }

    pub const fn total(&self) -> u8 {
        self.total
    }

    pub const fn state(&self) -> SendState {
        self.state
    }

    pub const fn rounds_without_ack(&self) -> u8 {
        self.rounds_without_ack
    }

    /// True when the sender has nothing left to emit (finished and abort sent).
    pub const fn is_idle(&self) -> bool {
        matches!(self.state, SendState::Delivered | SendState::Failed) && self.abort.is_none()
    }

    /// Plaintext of fragment `index`.
    pub fn fragment(&self, index: u8) -> &[u8] {
        let start = usize::from(index) * self.chunk;
        let end = (start + self.chunk).min(self.len);
        self.data.get(start..end).unwrap_or(&[])
    }

    fn fail(&mut self, reason: u8) {
        self.state = SendState::Failed;
        self.abort = Some(reason);
    }

    /// Starts the next round with `pending` fragments. `progress` is `None` when no ACK came
    /// back. Fails past `max_rounds` rounds, `max_stalled_rounds` ACKs in a row confirming
    /// nothing new, or `max_silent_rounds` rounds in a row without any ACK.
    fn next_round(&mut self, pending: u64, progress: Option<bool>, now: Millis, timing: &Timing) {
        self.round = self.round.saturating_add(1);
        match progress {
            None => self.rounds_without_ack = self.rounds_without_ack.saturating_add(1),
            Some(p) => {
                self.rounds_without_ack = 0;
                self.stalled_rounds = if p {
                    0
                } else {
                    self.stalled_rounds.saturating_add(1)
                };
            }
        }
        if self.round >= timing.max_rounds
            || self.stalled_rounds >= timing.max_stalled_rounds
            || self.rounds_without_ack >= timing.max_silent_rounds
        {
            self.fail(reason::TIMEOUT);
            return;
        }
        self.pending = pending;
        self.state = SendState::Sending;
        self.next_tx = self.next_tx.max(now);
    }

    /// The next emission due at `now`, if any.
    pub fn poll(&mut self, now: Millis, timing: &Timing) -> Option<SendEmit> {
        if let Some(reason) = self.abort.take() {
            return Some(SendEmit::Abort { reason });
        }
        if self.state == SendState::WaitingAck && now >= self.ack_deadline {
            let last = 1u64 << (self.total - 1);
            self.next_round(last, None, now, timing);
            if let Some(reason) = self.abort.take() {
                return Some(SendEmit::Abort { reason });
            }
        }
        if self.state != SendState::Sending || now < self.next_tx || self.pending == 0 {
            return None;
        }
        let index = self.pending.trailing_zeros() as u8;
        self.pending &= !(1u64 << index);
        self.next_tx = now + timing.pace_ms;
        if self.pending == 0 {
            self.state = SendState::WaitingAck;
            self.ack_deadline = now + timing.ack_wait_ms;
        }
        Some(SendEmit::Data {
            index,
            attempt: self.round,
        })
    }

    /// Handles an authenticated ACK. Returns `false` if it was stale or irrelevant.
    pub fn on_ack(
        &mut self,
        seq: u8,
        bitmap: u64,
        complete: bool,
        now: Millis,
        timing: &Timing,
    ) -> bool {
        if !matches!(self.state, SendState::Sending | SendState::WaitingAck) {
            return false;
        }
        if self.last_ack_seq.is_some_and(|last| seq <= last) {
            return false;
        }
        self.last_ack_seq = Some(seq);
        let full = mask(self.total);
        if complete || bitmap & full == full {
            self.state = SendState::Delivered;
            return true;
        }
        let progress = bitmap & full & !self.acked != 0;
        self.acked |= bitmap & full;
        self.next_round(full & !bitmap, Some(progress), now, timing);
        true
    }

    /// The receiver refused the datagram: stop without sending an ABORT.
    pub fn on_peer_abort(&mut self) {
        self.state = SendState::Failed;
        self.abort = None;
    }

    /// Gives up the datagram and announces it with an ABORT.
    pub fn cancel(&mut self) {
        if matches!(self.state, SendState::Sending | SendState::WaitingAck) {
            self.fail(reason::CANCELLED);
        }
    }

    /// When `poll` should be called next.
    pub fn next_wakeup(&self) -> Option<Millis> {
        if self.abort.is_some() {
            return Some(0);
        }
        match self.state {
            SendState::Sending => Some(self.next_tx),
            SendState::WaitingAck => Some(self.ack_deadline),
            SendState::Delivered | SendState::Failed => None,
        }
    }
}
