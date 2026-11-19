//! The clock, positions, shares and SOS.

use super::*;

#[uniffi::export]
impl LichenCore {
    // ----- location (protocol.md §12) ---------------------------------------------------

    /// The wall clock: `unix_ms` is the current Unix time in milliseconds at `now_ms`. Call it
    /// at start and whenever the clock may have jumped; positions wait for it.
    pub fn set_clock(&self, unix_ms: u64, now_ms: u64) {
        let mut i = self.lock();
        i.clock_offset = Some(
            i64::try_from(unix_ms)
                .unwrap_or(i64::MAX)
                .saturating_sub(i64::try_from(now_ms).unwrap_or(0)),
        );
        i.core.set_clock(unix_ms, now_ms);
    }

    /// Our position, from the location service.
    pub fn update_location(&self, fix: LocationFix, now_ms: u64) {
        let mut i = self.lock();
        i.core.update_location(
            Fix {
                lat: fix.lat,
                lon: fix.lon,
                alt_m: fix.alt_m,
                accuracy_m: fix.accuracy_m,
                heading_deg: fix.heading_deg,
                speed_mps: fix.speed_mps,
                unix_s: fix.unix_s,
            },
            now_ms,
        );
        i.persist();
    }

    /// Shares our position with `chat` at `precision` until `until_s` (Unix seconds); replaces
    /// a share with the same conversation.
    pub fn start_share(
        &self,
        chat: ChatRef,
        precision: SharePrecision,
        until_s: u64,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.core.start_share(c, precision.into(), until_s, now_ms)?;
        i.save_shares();
        i.persist();
        Ok(())
    }

    /// Stops sharing with `chat` (the receiver removes our marker).
    pub fn stop_share(&self, chat: ChatRef, now_ms: u64) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.core.stop_share(&c, now_ms);
        i.save_shares();
        i.persist();
        Ok(())
    }

    pub fn shares(&self) -> Vec<ShareInfo> {
        let mut v: Vec<ShareInfo> = self
            .lock()
            .core
            .shares()
            .iter()
            .map(|s| ShareInfo {
                chat: ChatRef::from_store(&s.chat),
                precision: s.precision.into(),
                until_s: s.until,
            })
            .collect();
        v.sort_by_key(|s| s.until_s);
        v
    }

    /// SOS: our exact position to every share and every group, every 2 minutes.
    pub fn set_sos(&self, on: bool, now_ms: u64) {
        let mut i = self.lock();
        i.core.set_sos(on, now_ms);
        i.persist();
    }

    pub fn sos(&self) -> bool {
        self.lock().core.sos()
    }

    /// The positions to show, one per sender, extrapolated to `now_ms`.
    pub fn positions(&self, now_ms: u64) -> Vec<PeerPosition> {
        let i = self.lock();
        let now_s = i
            .core
            .unix_s(now_ms)
            .unwrap_or_else(|| u64::try_from(i.wall_ms() / 1000).unwrap_or(0));
        let all = i
            .store
            .as_ref()
            .and_then(|s| s.positions().ok())
            .unwrap_or_default();
        best_per_sender(all)
            .iter()
            .map(|p| PeerPosition::new(p, now_s))
            .collect()
    }

    /// What our SOS is doing now.
    pub fn sos_status(&self, now_ms: u64) -> SosStatus {
        let st = self.lock().core.sos_status(now_ms);
        SosStatus {
            on: st.on,
            fix_fresh: st.fix_fresh,
            beacon_only: st.beacon_only,
            recipients: st.recipients,
            last_queued_s: st.last_queued,
            refused: st.refused,
        }
    }
}
