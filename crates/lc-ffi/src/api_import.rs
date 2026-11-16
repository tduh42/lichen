//! History from the official MeshCore app (ADR-0033): its exported database, read once into
//! lichen's MeshCore chats.

use super::*;

/// What an import brought in.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImportReport {
    /// Direct messages and channel posts added.
    pub direct: u32,
    pub channel: u32,
    /// Already here (an earlier import of the same history).
    pub already_here: u32,
    /// Left out: room posts, repeater command lines, malformed rows.
    pub skipped: u32,
}

#[uniffi::export]
impl LichenCore {
    /// Imports the database the official MeshCore app exports ("export app database"): its
    /// direct messages and channel posts go into lichen's MeshCore chats, the people they were
    /// with keep their names. The file is read from a copy of our own, which is then removed.
    pub fn import_meshcore_app(&self, path: String) -> Result<ImportReport, LichenError> {
        let dir = self
            .lock()
            .data_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        // Our own copy, with no journal or WAL beside it (ADR-0033).
        let copy = dir.join(format!("meshcore-import-{}.db", unix_ms()));
        std::fs::copy(&path, &copy).map_err(|_| LichenError::NotAnExport)?;
        let read = lc_core::import::meshcore_app(&copy);
        let _ = std::fs::remove_file(&copy);
        let got = read.map_err(|_| LichenError::NotAnExport)?;
        let mut i = self.lock();
        i.writable()?;
        let mut report = ImportReport {
            direct: 0,
            channel: 0,
            already_here: 0,
            skipped: u32::try_from(got.skipped).unwrap_or(u32::MAX),
        };
        for (node, name, custom) in &got.people {
            if !i.core.contacts().any(|c| c.key == *node) {
                i.core.add_contact(*node, name, None);
            }
            if let Some(nick) = custom
                && !i.nicknames.contains_key(node)
            {
                if let Some(s) = &i.store {
                    let _ = s.set_nickname(node, Some(nick));
                }
                i.nicknames.insert(*node, nick.clone());
            }
        }
        i.save_contacts();
        for m in got.messages {
            let Content::Text { sent_at, text } = &m.content else {
                continue;
            };
            let here = i
                .store
                .as_ref()
                .and_then(|s| s.has_text(&m.chat, m.outgoing, *sent_at, text).ok())
                .unwrap_or(false);
            if here {
                report.already_here += 1;
                continue;
            }
            let direct = matches!(m.chat, Chat::Mesh(_));
            if i.record(m).is_some() {
                if direct {
                    report.direct += 1;
                } else {
                    report.channel += 1;
                }
            }
        }
        i.extra.push_back(LichenEvent::ContactsUpdated);
        Ok(report)
    }
}
