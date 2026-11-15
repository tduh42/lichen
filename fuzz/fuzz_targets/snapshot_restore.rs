//! Stored snapshots (groups and invitations, ADR-0010) on arbitrary bytes, as a damaged or
//! foreign database could hold them (review of 2026-11-15, SR-37): restoring never panics, and
//! a group that restores gives a snapshot that restores again.
#![no_main]
use lc_core::{Config, Core};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut core = Core::new(Config::new("fuzz", [5; 32], [6; 32]));
    if core.restore_group(data) {
        let gids: Vec<_> = core.groups().map(|g| g.gid).collect();
        for gid in gids {
            let snapshot = core.group_snapshot(&gid).expect("a restored group");
            let mut again = Core::new(Config::new("fuzz", [5; 32], [6; 32]));
            assert!(again.restore_group(&snapshot), "its own snapshot");
        }
    }
    let _ = core.restore_invitation(data, 0);
});
