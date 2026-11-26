//! Each node's signal (protocol §16.1, ADR-0049): what our radio heard of a node directly, and
//! how good that is for the current preset.
//!
//! Readings come from `LOG_RX_DATA` ([`crate::Core`] attributes them); the runner stores them
//! ([`crate::store::Store::put_signal`]) and gives the last one back at connection
//! ([`crate::Core::restore_signal`]).

/// How good a reading is (§16.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignalRating {
    Good,
    Fair,
    Bad,
    /// No reading.
    None,
}

/// One reception of a node: SNR × 4, RSSI (dBm; `None` when the radio does not measure it),
/// and when, Unix seconds by the phone's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub snr4: i8,
    pub rssi: Option<i8>,
    pub at_s: u64,
}

/// Readings are kept this long (§16.1)...
pub const HISTORY_KEEP_S: u64 = 7 * 86_400;
/// ...and this many per radio and node at most, the newest.
pub const HISTORY_MAX: usize = 500;

/// Margin over the demodulation limit for a good reading, SNR × 4 (10 dB).
pub const GOOD_MARGIN4: i16 = 40;
/// Margin over the demodulation limit for a fair reading, SNR × 4 (5 dB).
pub const FAIR_MARGIN4: i16 = 20;
/// RSSI of a good reading or better, dBm.
pub const GOOD_RSSI_DBM: i8 = -115;
/// RSSI of a fair reading or better, dBm.
pub const FAIR_RSSI_DBM: i8 = -126;

/// The LoRa demodulation limit at spreading factor `sf`, SNR × 4: −2.5 dB × (SF − 4)
/// (Semtech SX1261/2 datasheet, Table 6-1: SF7 −7.5 dB … SF12 −20 dB). Clamped to SF5–SF12.
pub fn snr_limit4(sf: u8) -> i16 {
    -10 * (i16::from(sf.clamp(5, 12)) - 4)
}

/// Where the ratings start at a spreading factor (§16.1): a reading at a limit or above it has
/// the rating above that limit, as long as the other figure allows it. SNR × 4 and dBm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub good_snr4: i16,
    pub fair_snr4: i16,
    pub good_rssi: i8,
    pub fair_rssi: i8,
}

/// The limits at spreading factor `sf`: what [`rate`] compares with, and what the apps draw.
pub fn limits(sf: u8) -> Limits {
    let floor = snr_limit4(sf);
    Limits {
        good_snr4: floor + GOOD_MARGIN4,
        fair_snr4: floor + FAIR_MARGIN4,
        good_rssi: GOOD_RSSI_DBM,
        fair_rssi: FAIR_RSSI_DBM,
    }
}

/// The rating of a reading at spreading factor `sf` (§16.1).
pub fn rate(snr4: i8, rssi: Option<i8>, sf: u8) -> SignalRating {
    let l = limits(sf);
    let snr4 = i16::from(snr4);
    let by_snr = if snr4 >= l.good_snr4 {
        SignalRating::Good
    } else if snr4 >= l.fair_snr4 {
        SignalRating::Fair
    } else {
        SignalRating::Bad
    };
    let by_rssi = match rssi {
        None => SignalRating::Good,
        Some(r) if r >= l.good_rssi => SignalRating::Good,
        Some(r) if r >= l.fair_rssi => SignalRating::Fair,
        Some(_) => SignalRating::Bad,
    };
    if rank(by_rssi) < rank(by_snr) {
        by_rssi
    } else {
        by_snr
    }
}

/// Worse first.
const fn rank(r: SignalRating) -> u8 {
    match r {
        SignalRating::None => 0,
        SignalRating::Bad => 1,
        SignalRating::Fair => 2,
        SignalRating::Good => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_follow_the_datasheet() {
        assert_eq!(snr_limit4(7), -30);
        assert_eq!(snr_limit4(12), -80);
        // Outside SF5–SF12: the nearest.
        assert_eq!(snr_limit4(4), -10);
        assert_eq!(snr_limit4(13), -80);
    }

    #[test]
    fn the_worse_of_snr_and_rssi() {
        assert_eq!(rate(40, Some(-127), 8), SignalRating::Bad);
        assert_eq!(rate(-36, Some(-60), 8), SignalRating::Bad);
        assert_eq!(rate(-36, None, 12), SignalRating::Good);
    }
}
