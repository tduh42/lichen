//! LoRa time on air (Semtech AN1200.13; SX1261/2 datasheet §6.1.4 for SF5/SF6), explicit
//! header, CRC on, low-data-rate optimisation when a symbol lasts 16 ms or more. Integer
//! arithmetic only.

/// Radio settings as reported by `SELF_INFO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoRaParams {
    /// Spreading factor, 5..12.
    pub sf: u8,
    /// Bandwidth in Hz.
    pub bw_hz: u32,
    /// Coding rate denominator, 5..8 (4/5..4/8).
    pub cr: u8,
    /// Preamble length in symbols.
    pub preamble: u16,
}

impl LoRaParams {
    /// MeshCore settings: preamble of 32 symbols when SF ≤ 8, otherwise 16
    /// (`RadioLibWrappers.h:56`). `None` if a value is out of range.
    pub const fn meshcore(sf: u8, bw_hz: u32, cr: u8) -> Option<Self> {
        if sf < 5 || sf > 12 || cr < 5 || cr > 8 || bw_hz < 7_000 {
            return None;
        }
        Some(Self {
            sf,
            bw_hz,
            cr,
            preamble: if sf <= 8 { 32 } else { 16 },
        })
    }

    /// Symbol duration in microseconds (rounded down).
    pub const fn symbol_us(&self) -> u64 {
        (1_000_000u64 << self.sf) / self.bw_hz as u64
    }

    /// Low-data-rate optimisation: on when a symbol lasts at least 16 ms.
    pub const fn ldro(&self) -> bool {
        (1_000_000u64 << self.sf) >= 16_000 * self.bw_hz as u64
    }

    /// Time on air of a frame of `len` bytes, in microseconds.
    pub const fn airtime_us(&self, len: usize) -> u64 {
        let sf = self.sf as i64;
        let pl = len as i64;
        let cr = (self.cr - 4) as i64;
        // Preamble in quarter-symbols: + 4.25 (SF7..12) or + 6.25 (SF5/6).
        let (pre_q, num) = if self.sf <= 6 {
            (self.preamble as i64 * 4 + 25, 8 * pl + 16 - 4 * sf + 20)
        } else {
            (self.preamble as i64 * 4 + 17, 8 * pl + 16 - 4 * sf + 28)
        };
        let den = 4 * (sf - if self.ldro() { 2 } else { 0 });
        let blocks = if num > 0 { (num + den - 1) / den } else { 0 };
        let payload_symbols = 8 + blocks * (cr + 4);
        let quarter_symbols = (pre_q + 4 * payload_symbols) as u64;
        quarter_symbols * (1_000_000u64 << self.sf) / (4 * self.bw_hz as u64)
    }
}
