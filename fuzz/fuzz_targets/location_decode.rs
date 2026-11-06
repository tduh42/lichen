//! Location body decoder (§12.1) on arbitrary bytes: never panic, and what decodes encodes back
//! to the same position; the time window never panics either.
#![no_main]
use lc_proto::location::{Position, unwrap_time};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(p) = Position::decode(data) {
        assert_eq!(Position::decode(&p.encode()), Some(p));
    }
    if let Some((t, rest)) = data.split_first_chunk::<10>() {
        let t16 = u16::from_le_bytes([t[0], t[1]]);
        let now = u64::from_le_bytes([t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9]]) >> 1;
        if let Some(full) = unwrap_time(t16, now) {
            assert_eq!(full & 0xFFFF, u64::from(t16));
            assert!(full <= now + 600 && full + 18 * 3600 >= now);
        }
        let _ = rest;
    }
});
