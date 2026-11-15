//! The object the apps hold.

use super::*;

/// Store setting: the TX power to restore (one i8), while a search's POWER holds it.
pub(crate) const TX_POWER_RESTORE: &str = "tx_power_restore";

/// The lichen core with its storage, shared between threads.
#[derive(uniffi::Object)]
pub struct LichenCore {
    pub(crate) inner: Mutex<Inner>,
}

impl std::fmt::Debug for LichenCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LichenCore")
    }
}

impl LichenCore {
    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
