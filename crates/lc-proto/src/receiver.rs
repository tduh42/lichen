//! Receiver side of ACK-on-Error fragmentation (§7.2, §7.4): fixed 157-byte slots, bitmap ACKs,
//! reassembly timer.

use crate::sender::Timing;
use crate::{CHUNK_MAX, DATAGRAM_MAX, MAX_FRAGMENTS, Millis, reason};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxState {
    Collecting,
    Done,
    Failed,
}

/// Result of storing a fragment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    Duplicate,
    Stored,
    /// All fragments are in and consistent: [`Reassembly::datagram`] is ready.
    Complete,
    /// All fragments are in but their lengths are inconsistent.
    Malformed,
    /// The fragment cannot belong to this datagram.
    Rejected,
}

/// What the receiver wants on the air now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxEmit {
    Ack {
        seq: u8,
        bitmap: u64,
        complete: bool,
    },
    Abort {
        reason: u8,
    },
}

/// One datagram being received.
pub struct Reassembly {
    tag: u16,
    total: u8,
    slots: [u8; DATAGRAM_MAX],
    lens: [u8; MAX_FRAGMENTS],
    got: u64,
    highest: u8,
    /// Index that closes the current round: `total − 1` at first, then the highest index
    /// reported missing in our last ACK (§7.2).
    round_last: u8,
    last_arrival: Option<Millis>,
    gap: Millis,
    deadline: Millis,
    next_ack_seq: u8,
    acks_without_progress: u8,
    ack_due: bool,
    /// When the last ACK is sent again (`Timing::ack_repeat_ms`).
    repeat_at: Option<Millis>,
    abort: Option<u8>,
    state: RxState,
    len: usize,
}

impl core::fmt::Debug for Reassembly {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reassembly")
            .field("tag", &self.tag)
            .field("total", &self.total)
            .field("got", &self.got)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

const fn full(total: u8) -> u64 {
    if total as usize >= MAX_FRAGMENTS {
        u64::MAX
    } else {
        (1u64 << total) - 1
    }
}

impl Reassembly {
    pub fn new(tag: u16, total: u8, now: Millis, timing: &Timing) -> Self {
        let mut r = Self {
            tag,
            total,
            slots: [0; DATAGRAM_MAX],
            lens: [0; MAX_FRAGMENTS],
            got: 0,
            highest: 0,
            round_last: total.saturating_sub(1),
            last_arrival: None,
            gap: timing.rx_first_gap_ms,
            deadline: now,
            next_ack_seq: 0,
            acks_without_progress: 0,
            ack_due: false,
            repeat_at: None,
            abort: None,
            state: RxState::Collecting,
            len: 0,
        };
        r.reset(tag, total, now, timing);
        r
    }

    /// Reuses the buffer for a new datagram.
    pub fn reset(&mut self, tag: u16, total: u8, now: Millis, timing: &Timing) {
        self.tag = tag;
        self.total = total;
        self.lens = [0; MAX_FRAGMENTS];
        self.got = 0;
        self.highest = 0;
        self.round_last = total.saturating_sub(1);
        self.last_arrival = None;
        self.gap = timing.rx_first_gap_ms;
        self.deadline = now + (Millis::from(total) + 1) * self.gap + timing.rx_margin_ms;
        self.next_ack_seq = 0;
        self.acks_without_progress = 0;
        self.ack_due = false;
        self.repeat_at = None;
        self.abort = None;
        self.state = RxState::Collecting;
        self.len = 0;
    }

    pub const fn tag(&self) -> u16 {
        self.tag
    }

    pub const fn total(&self) -> u8 {
        self.total
    }

    pub const fn state(&self) -> RxState {
        self.state
    }

    pub const fn bitmap(&self) -> u64 {
        self.got
    }

    /// The reassembled datagram, once [`RxState::Done`].
    pub fn datagram(&self) -> &[u8] {
        if self.state == RxState::Done {
            self.slots.get(..self.len).unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// A fragment of a completed datagram arrived again: the sender missed our ACK.
    pub fn request_complete_ack(&mut self) {
        if self.state == RxState::Done {
            self.ack_due = true;
        }
    }

    /// The sender gave the datagram up.
    pub fn on_sender_abort(&mut self) {
        if self.state == RxState::Collecting {
            self.state = RxState::Failed;
            self.ack_due = false;
            self.repeat_at = None;
        }
    }

    /// Stores an authenticated fragment.
    pub fn on_fragment(&mut self, index: u8, data: &[u8], now: Millis, timing: &Timing) -> Stored {
        if self.state != RxState::Collecting {
            return Stored::Duplicate;
        }
        if index >= self.total || data.is_empty() || data.len() > CHUNK_MAX {
            return Stored::Rejected;
        }
        let bit = 1u64 << index;
        if self.got & bit != 0 {
            // The sender is alive. A repeated fragment that closes the round is its way of
            // asking for the ACK it missed (§7.1 step 5).
            self.acks_without_progress = 0;
            if index >= self.round_last {
                self.ack_due = true;
            }
            return Stored::Duplicate;
        }
        let start = usize::from(index) * CHUNK_MAX;
        let Some(slot) = self.slots.get_mut(start..start + data.len()) else {
            return Stored::Rejected;
        };
        slot.copy_from_slice(data);
        if let Some(len) = self.lens.get_mut(usize::from(index)) {
            *len = data.len() as u8;
        }
        self.got |= bit;
        self.acks_without_progress = 0;
        if let Some(last) = self.last_arrival {
            self.gap = now
                .saturating_sub(last)
                .clamp(timing.rx_min_gap_ms, timing.rx_max_gap_ms);
        }
        self.last_arrival = Some(now);
        self.highest = self.highest.max(index);

        if self.got == full(self.total) {
            return if self.assemble() {
                self.state = RxState::Done;
                self.ack_due = true;
                Stored::Complete
            } else {
                self.state = RxState::Failed;
                self.abort = Some(reason::MALFORMED);
                self.repeat_at = None;
                Stored::Malformed
            };
        }
        if index >= self.round_last {
            self.ack_due = true;
        }
        let after = Millis::from(self.total - 1 - self.highest);
        self.deadline = now + (after + 2) * self.gap + timing.rx_margin_ms;
        Stored::Stored
    }

    /// Checks fragment lengths and packs the slots into a contiguous datagram.
    fn assemble(&mut self) -> bool {
        let total = usize::from(self.total);
        let Some(lens) = self.lens.get(..total) else {
            return false;
        };
        let Some((&last, body)) = lens.split_last() else {
            return false;
        };
        let chunk = body.first().copied().unwrap_or(last);
        if body.iter().any(|&l| l != chunk) || last > chunk || last == 0 {
            return false;
        }
        let chunk = usize::from(chunk);
        for i in 1..total {
            let len = if i + 1 == total {
                usize::from(last)
            } else {
                chunk
            };
            let src = i * CHUNK_MAX;
            self.slots.copy_within(src..src + len, i * chunk);
        }
        self.len = (total - 1) * chunk + usize::from(last);
        true
    }

    /// The next emission due at `now`, if any.
    pub fn poll(&mut self, now: Millis, timing: &Timing) -> Option<RxEmit> {
        if let Some(reason) = self.abort.take() {
            return Some(RxEmit::Abort { reason });
        }
        if self.state == RxState::Collecting && !self.ack_due && now >= self.deadline {
            if self.acks_without_progress >= timing.rx_max_acks {
                self.state = RxState::Failed;
                self.repeat_at = None;
                return Some(RxEmit::Abort {
                    reason: reason::TIMEOUT,
                });
            }
            self.ack_due = true;
        }
        // The second copy of the last ACK (§7.2): a new sequence number, so a new nonce and
        // new bytes for MeshCore's duplicate filter.
        let repeat = !self.ack_due
            && self.state != RxState::Failed
            && self.repeat_at.is_some_and(|t| now >= t);
        if !self.ack_due && !repeat {
            return None;
        }
        self.repeat_at = None;
        self.ack_due = false;
        let seq = self.next_ack_seq;
        // Never reuse an ACK sequence number (it is part of the nonce).
        self.next_ack_seq = self.next_ack_seq.checked_add(1)?;
        if !repeat && timing.ack_repeat_ms > 0 {
            self.repeat_at = Some(now + timing.ack_repeat_ms);
        }
        if !repeat && self.state == RxState::Collecting {
            self.acks_without_progress = self.acks_without_progress.saturating_add(1);
            let missing_bits = full(self.total) & !self.got;
            let missing = Millis::from(missing_bits.count_ones());
            self.round_last = 63u8.saturating_sub(missing_bits.leading_zeros() as u8);
            self.deadline = now + (missing + 2) * self.gap + timing.rx_margin_ms;
        }
        Some(RxEmit::Ack {
            seq,
            bitmap: self.got,
            complete: self.state == RxState::Done,
        })
    }

    /// When `poll` should be called next.
    pub fn next_wakeup(&self) -> Option<Millis> {
        if self.ack_due || self.abort.is_some() {
            return Some(0);
        }
        let timer = match self.state {
            RxState::Collecting => Some(self.deadline),
            RxState::Done | RxState::Failed => None,
        };
        match (timer, self.repeat_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
