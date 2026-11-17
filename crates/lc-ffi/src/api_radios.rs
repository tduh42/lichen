//! Saved radios (ADR-0042): the radios this phone was paired with, to switch between.

use super::*;

/// How the app reaches a radio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LinkKind {
    Bluetooth,
    Usb,
    Tcp,
}

impl LinkKind {
    const fn code(self) -> u8 {
        match self {
            Self::Bluetooth => 0,
            Self::Usb => 1,
            Self::Tcp => 2,
        }
    }

    const fn of(code: u8) -> Self {
        match code {
            1 => Self::Usb,
            2 => Self::Tcp,
            _ => Self::Bluetooth,
        }
    }
}

/// A radio this phone was paired with.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SavedRadio {
    /// What the app connects to: a Bluetooth identifier, a USB device, `host:port`.
    pub address: String,
    pub link: LinkKind,
    /// The name it advertises (as last seen).
    pub name: String,
    /// Its node key, once it answered.
    pub node: Option<Vec<u8>>,
    /// Unix milliseconds.
    pub last_used_ms: i64,
    /// The radio connected now.
    pub current: bool,
}

#[uniffi::export]
impl LichenCore {
    /// The radios this phone was paired with, the most recently used first.
    pub fn saved_radios(&self) -> Vec<SavedRadio> {
        let i = self.lock();
        let node = i
            .core
            .online()
            .then(|| i.core.node_key().copied())
            .flatten();
        i.store
            .as_ref()
            .and_then(|s| s.radios().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|r| SavedRadio {
                current: node.is_some() && r.node == node,
                address: r.address,
                link: LinkKind::of(r.link),
                name: r.name,
                node: r.node.map(|k| k.to_vec()),
                last_used_ms: r.last_used_ms,
            })
            .collect()
    }

    /// Saves the radio connected now, reached at `address` over `link` (call it after
    /// `Connected`): its node and name come from the radio; `name` until it answered.
    pub fn remember_radio(
        &self,
        address: String,
        link: LinkKind,
        name: String,
    ) -> Result<(), LichenError> {
        let i = self.lock();
        let node = i.core.node_key().copied();
        let radio_name = i.core.node_name().trim().to_owned();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.put_radio(&store::SavedRadio {
            address,
            link: link.code(),
            name: if radio_name.is_empty() {
                name
            } else {
                radio_name
            },
            node,
            last_used_ms: unix_ms(),
        })
        .map_err(|_| LichenError::Storage)
    }

    /// Forgets a saved radio (the app unpairs it).
    pub fn forget_radio(&self, address: String) -> Result<(), LichenError> {
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.delete_radio(&address).map_err(|_| LichenError::Storage)
    }
}
