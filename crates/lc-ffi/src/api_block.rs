//! Blocking (ADR-0034): local, nothing is sent and the person is not told.

use super::*;

/// Someone the user blocked: their node, the identity it had then, and the name shown then.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BlockedContact {
    pub node: Vec<u8>,
    pub identity: Option<Vec<u8>>,
    pub name: String,
    /// Unix ms.
    pub at_ms: i64,
    /// A request's identity (ADR-0046), not a contact: show "Someone using <name>'s radio";
    /// `node` is the identity, for `unblock`.
    pub request: bool,
}

/// How long a lifted block's period is kept: a text is retried for 24 hours at most (§6.1), a
/// group post kept an hour (§10.6); a day more for clocks that differ.
pub(crate) const BLOCK_PERIOD_KEEP_MS: i64 = 48 * 3_600_000;

/// A stored block period as the core takes it (Unix seconds, the ends included).
pub(crate) fn block_period(p: &store::BlockPeriodRow) -> lc_core::BlockPeriod {
    let s = |ms: i64| u32::try_from(ms.max(0) / 1000).unwrap_or(u32::MAX);
    lc_core::BlockPeriod {
        node: p.node,
        identity: p.identity,
        from_s: s(p.from_ms),
        to_s: s(p.to_ms + 999),
    }
}

#[uniffi::export]
impl LichenCore {
    /// Blocks contact `node` and the identity it is known under: their introductions,
    /// direct messages (lichen and MeshCore) and group posts no longer reach this phone, and
    /// their group posts already here are hidden until unblocked. Their positions and SOS in
    /// groups still show. Our messages waiting for them fail. Kept across restarts.
    pub fn block(&self, node: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        let name = i
            .nicknames
            .get(&k)
            .cloned()
            .or_else(|| {
                i.core
                    .contacts()
                    .find(|c| c.key == k)
                    .map(|c| c.name.clone())
            })
            .ok_or(LichenError::UnknownContact)?;
        let identity = i.core.block(&k, now_ms);
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.block(&k, identity.as_ref(), &name, i.wall_ms())
            .map_err(|_| LichenError::Storage)
    }

    /// Unblocks `node`: what they send reaches this phone again, and their group posts
    /// kept here show again. What they sent meanwhile is gone, even when their phone sends it
    /// again now: the block's period is kept, and a message dated within it is dropped
    /// unanswered (ADR-0034).
    pub fn unblock(&self, node: Vec<u8>) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        let now = i.wall_ms();
        let lifted = i
            .store
            .as_ref()
            .ok_or(LichenError::Storage)?
            .unblock(&k, now)
            .map_err(|_| LichenError::Storage)?;
        i.core
            .unblock(&k, lifted.as_ref().and_then(|b| b.identity.as_ref()));
        if let Some(b) = lifted {
            i.core
                .add_block_period(block_period(&store::BlockPeriodRow {
                    node: b.node,
                    identity: b.identity,
                    from_ms: b.at,
                    to_ms: now.max(b.at),
                }));
        }
        Ok(())
    }

    /// Everyone blocked, the latest first.
    pub fn blocked(&self) -> Vec<BlockedContact> {
        self.lock()
            .store
            .as_ref()
            .and_then(|s| s.blocked().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|b| BlockedContact {
                node: b.node.to_vec(),
                request: b.identity == Some(b.node),
                identity: b.identity.map(|i| i.to_vec()),
                name: b.name,
                at_ms: b.at,
            })
            .collect()
    }

    /// Whether contact `node` is blocked, itself or through its identity.
    pub fn is_blocked(&self, node: Vec<u8>) -> bool {
        key(&node).is_ok_and(|k| self.lock().core.is_blocked(&k))
    }
}
