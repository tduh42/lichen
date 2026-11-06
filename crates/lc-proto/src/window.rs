//! Replay window (§8): highest `tag` seen and a bitmap of the 64 `tag`s below it that are
//! completed.

/// Where a `tag` stands relative to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Above everything seen so far.
    New,
    /// Inside the window, not completed.
    Pending,
    /// Inside the window, already completed.
    Completed,
    /// 64 or more below the highest `tag`: ignored.
    TooOld,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayWindow {
    highest: Option<u16>,
    /// Bit `d` is set when `highest − d` is completed.
    completed: u64,
}

impl ReplayWindow {
    pub const fn new() -> Self {
        Self {
            highest: None,
            completed: 0,
        }
    }

    pub const fn highest(&self) -> Option<u16> {
        self.highest
    }

    pub fn check(&self, tag: u16) -> Verdict {
        let Some(h) = self.highest else {
            return Verdict::New;
        };
        if tag > h {
            return Verdict::New;
        }
        let d = h - tag;
        if d >= 64 {
            Verdict::TooOld
        } else if (self.completed >> d) & 1 == 1 {
            Verdict::Completed
        } else {
            Verdict::Pending
        }
    }

    /// Records that a valid fragment of `tag` was seen.
    pub fn see(&mut self, tag: u16) {
        match self.highest {
            None => {
                self.highest = Some(tag);
                self.completed = 0;
            }
            Some(h) if tag > h => {
                let shift = u32::from(tag - h);
                self.completed = self.completed.checked_shl(shift).unwrap_or(0);
                self.highest = Some(tag);
            }
            Some(_) => {}
        }
    }

    /// Records that `tag` was completed (delivered to the app).
    pub fn complete(&mut self, tag: u16) {
        self.see(tag);
        if let Some(h) = self.highest {
            let d = u32::from(h - tag);
            if d < 64 {
                self.completed |= 1 << d;
            }
        }
    }
}
