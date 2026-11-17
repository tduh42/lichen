//! The encrypted backup (docs/spec/backup.md, ADR-0037): one file with the messages, contacts,
//! groups, preferences, settings and media, opened by the 24 words.

use std::path::Path;

use lc_core::backup::{self, BackupError, Sources};

use super::*;

/// Store settings that belong to this phone's radio, never in a backup.
const RADIO_SETTINGS: &[&str] = &[TX_POWER_RESTORE];
/// Folders of the app's own, beside the databases, for a backup being made or read.
const EXPORT_WORK: &str = "backup-work";
const RESTORE_WORK: &str = "restore-work";
/// Media fetched over the bridge, as both apps keep them.
const DOWNLOADS: &str = "downloads";

/// What a backup holds.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BackupSummary {
    /// The file's size.
    pub bytes: u64,
    pub messages: u64,
    /// Media files.
    pub files: u64,
}

/// A backup restored.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RestoredBackup {
    /// The identity seed of the words: the app saves it as its identity, then starts a new core
    /// with it, which opens the restored data.
    pub identity_seed: Vec<u8>,
    /// The app's settings the backup carried (backup.md §4, JSON), empty when none.
    pub settings: Vec<u8>,
    pub messages: u64,
    pub files: u64,
}

fn refused(e: BackupError) -> LichenError {
    match e {
        BackupError::NotABackup => LichenError::NotABackup,
        BackupError::Newer => LichenError::NewerBackup,
        BackupError::WrongWords => LichenError::WrongBackupWords,
        BackupError::Damaged => LichenError::BackupDamaged,
        BackupError::Io(_) | BackupError::Database(_) => LichenError::Storage,
    }
}

#[uniffi::export]
impl LichenCore {
    /// Writes a backup of this phone's data to `path`, a file in the app's own folders that it
    /// then hands to the share sheet or the document the user picked. `settings`: the app's
    /// settings as JSON (backup.md §4). It takes as long as the data: call it off the main
    /// thread. The core keeps running meanwhile (the store is copied on a connection of its
    /// own). Not on the demo network.
    pub fn export_backup(
        &self,
        path: String,
        settings: Vec<u8>,
    ) -> Result<BackupSummary, LichenError> {
        let (database, key, dir, seed, salt) = {
            let mut i = self.lock();
            let dir = i.data_dir.clone().ok_or(LichenError::NotAllowed)?;
            let identity = i.core.key().copied().ok_or(LichenError::NoIdentity)?;
            i.writable()?;
            // Groups changed since the last event are written first.
            i.persist();
            let mut salt = [0u8; 32];
            for part in salt.chunks_mut(8) {
                part.copy_from_slice(&i.core.random_id());
            }
            (database_file(&dir, &identity), i.db_key, dir, i.seed, salt)
        };
        let made = backup::export(
            &Sources {
                database: &database,
                key: key.as_ref(),
                downloads: Some(&dir.join(DOWNLOADS)),
                settings: &settings,
                drop_settings: RADIO_SETTINGS,
                work: &dir.join(EXPORT_WORK),
            },
            Path::new(&path),
            &seed,
            salt,
        )
        .map_err(refused)?;
        Ok(BackupSummary {
            bytes: made.bytes,
            messages: made.messages,
            files: made.files,
        })
    }

    /// Restores backup `path` with its 24 `words` (backup.md §5) into `data_dir`, the folder
    /// the app gives its persistent cores: the backup's data replaces the data there for that
    /// identity. Any core can do it, a demo one too; nothing changes when it fails (other words,
    /// a damaged file). The app then saves the returned seed as its identity and starts a new
    /// core with it. When the backup is of this core's own identity and folder, this core's
    /// store is closed first and it stores nothing more. Call it off the main thread.
    pub fn restore_backup(
        &self,
        path: String,
        words: Vec<String>,
        data_dir: String,
    ) -> Result<RestoredBackup, LichenError> {
        let seed = lc_core::identity::seed_from_phrase(&words).ok_or(LichenError::BadCode)?;
        let dir = PathBuf::from(data_dir);
        std::fs::create_dir_all(&dir).map_err(|_| LichenError::Storage)?;
        let staged =
            backup::unpack(Path::new(&path), &seed, &dir.join(RESTORE_WORK)).map_err(refused)?;
        let identity = lc_meshcore::ecdh::public_key(&lc_meshcore::ecdh::expand_seed(&seed));
        let out = RestoredBackup {
            identity_seed: seed.to_vec(),
            settings: staged.settings.clone(),
            messages: staged.messages,
            files: staged.files.len() as u64,
        };
        let mut i = self.lock();
        // The store it replaces is closed first: nothing of it may be written after, and its
        // journal goes with it.
        if i.core.key() == Some(&identity) && i.data_dir.as_ref() == Some(&dir) {
            i.store = None;
        }
        staged
            .install(&database_file(&dir, &identity), &dir.join(DOWNLOADS))
            .map_err(refused)?;
        Ok(out)
    }
}
