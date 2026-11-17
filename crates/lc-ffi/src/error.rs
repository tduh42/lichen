//! Errors returned to the app.

/// Errors returned to the app.
#[derive(Debug, PartialEq, Eq, uniffi::Error)]
pub enum LichenError {
    UnknownContact,
    UnknownGroup,
    Empty,
    TooLarge,
    NoIdentity,
    NotAllowed,
    /// A key or id does not have the expected length.
    InvalidKey,
    /// The medium does not fit in the airtime left this hour; it would in `wait_s` seconds.
    OverBudget {
        wait_s: u64,
    },
    /// Media go to contacts only (groups wait for the Internet bridge).
    MediaInGroup,
    /// The image could not be made into a thumbnail.
    BadImage,
    /// Not an image preview that can be asked for in full.
    NotAPreview,
    /// The contact's identity is not known yet: exchange a message or scan their code first.
    IdentityUnknown,
    /// Not a lichen contact code, or not 24 valid words.
    BadCode,
    /// No search runs for this member.
    NotSearching,
    /// Rate limited: possible again in `wait_s` seconds.
    TooSoon {
        wait_s: u64,
    },
    /// Our position is unknown or too old.
    NoPosition,
    /// The phone's database could not be read or written.
    Storage,
    /// A frame of the message already went out, or it is not waiting: it can no longer be
    /// cancelled (§6.1).
    NotCancellable,
    /// This message has no reference (an image from before replies existed): it cannot be
    /// answered or reacted to (§6.2).
    NotReferable,
    /// The phone's clock is not known yet.
    NoClock,
    /// The invitation has expired (§10.5).
    Expired,
    /// A radio setting outside what the radio takes (protocol §1.1).
    InvalidSetting,
    /// Not a database the MeshCore app exported (ADR-0033).
    NotAnExport,
    /// What was pasted names an identity other than the one this contact has: the change
    /// waits for the user, who compares safety numbers (§4.6).
    IdentityPending,
    /// Not a lichen backup file (ADR-0037).
    NotABackup,
    /// A backup made by a newer version of lichen: update the app first.
    NewerBackup,
    /// The 24 words are valid, but not the ones of this backup.
    WrongBackupWords,
    /// The backup file is cut short or was changed: nothing was restored.
    BackupDamaged,
}

impl std::fmt::Display for LichenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for LichenError {}

impl From<lc_core::Error> for LichenError {
    fn from(e: lc_core::Error) -> Self {
        match e {
            lc_core::Error::UnknownContact => Self::UnknownContact,
            lc_core::Error::UnknownGroup => Self::UnknownGroup,
            lc_core::Error::Empty => Self::Empty,
            lc_core::Error::TooLarge => Self::TooLarge,
            lc_core::Error::NoIdentity => Self::NoIdentity,
            lc_core::Error::IdentityUnknown => Self::IdentityUnknown,
            lc_core::Error::NotAllowed => Self::NotAllowed,
            lc_core::Error::OverBudget { wait_ms } => Self::OverBudget {
                wait_s: wait_ms.div_ceil(1000),
            },
            lc_core::Error::NotSearching => Self::NotSearching,
            lc_core::Error::TooSoon { wait_ms } => Self::TooSoon {
                wait_s: wait_ms.div_ceil(1000),
            },
            lc_core::Error::NoPosition => Self::NoPosition,
            lc_core::Error::Started | lc_core::Error::NotQueued => Self::NotCancellable,
            lc_core::Error::NoClock => Self::NoClock,
            lc_core::Error::Expired => Self::Expired,
            lc_core::Error::Invalid => Self::InvalidSetting,
            lc_core::Error::IdentityPending => Self::IdentityPending,
        }
    }
}
