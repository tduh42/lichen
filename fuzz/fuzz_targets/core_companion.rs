//! The whole core driven by a hostile companion (review of 2026-11-15, SR-37): any sequence
//! of frames, as a radio could send them, with time going on. Nothing panics, and what the
//! core keeps stays bounded.
#![no_main]
use lc_core::{Config, Core};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut core = Core::new(Config::new("fuzz", [3; 32], [4; 32]));
    core.set_clock(1_793_000_000_000, 0);
    let mut now = 0u64;
    core.connect(now);
    // `len (1) ‖ frame (len)`, repeated; a zero length advances the clock a minute.
    let mut rest = data;
    while let Some((&len, tail)) = rest.split_first() {
        if len == 0 {
            now += 60_000;
            core.tick(now);
            rest = tail;
            continue;
        }
        let n = usize::from(len).min(tail.len());
        let (frame, tail) = tail.split_at(n);
        core.on_companion_frame(frame, now);
        now += 50;
        core.tick(now);
        while core.poll_companion(now).is_some() {}
        while core.poll_event().is_some() {}
        rest = tail;
    }
    assert!(core.contacts().count() <= 4_096);
});
