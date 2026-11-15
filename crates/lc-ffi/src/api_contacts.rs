//! The radio, the airtime budget, contacts and groups as the app lists them.

use super::*;

#[uniffi::export]
impl LichenCore {
    pub fn radio(&self) -> Radio {
        let i = self.lock();
        let r = i.core.radio();
        Radio {
            freq_khz: i.core.freq_khz(),
            bw_hz: r.bw_hz,
            sf: r.sf,
            cr: r.cr,
        }
    }

    pub fn budget(&self, now_ms: u64) -> Budget {
        let (used_ms, limit_ms) = self.lock().core.budget(now_ms);
        Budget { used_ms, limit_ms }
    }

    pub fn contacts(&self) -> Vec<ContactInfo> {
        let i = self.lock();
        let mut v: Vec<ContactInfo> = i
            .core
            .contacts()
            .map(|c| ContactInfo::of(c, &i.nicknames))
            .collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    /// Names contact `node` on this phone only (`None` or blank: its node's name again).
    pub fn rename_contact(&self, node: Vec<u8>, name: Option<String>) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        if !i.core.contacts().any(|c| c.key == k) {
            return Err(LichenError::UnknownContact);
        }
        let name = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        if let Some(s) = &i.store {
            let _ = s.set_nickname(&k, name.as_deref());
        }
        match name {
            Some(n) => i.nicknames.insert(k, n),
            None => i.nicknames.remove(&k),
        };
        i.extra.push_back(LichenEvent::ContactsUpdated);
        Ok(())
    }

    /// "Let this group locate me" (protocol.md §11): answer its members' pings and power
    /// requests outside SOS. Kept across restarts.
    pub fn set_search_consent(
        &self,
        gid: Vec<u8>,
        on: bool,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        if !i.core.groups().any(|x| x.gid == g) {
            return Err(LichenError::UnknownGroup);
        }
        if let Some(st) = &i.store {
            let _ = st.set_search_consent(&g, on);
        }
        i.core.set_search_consent(&g, on, now_ms);
        i.persist();
        Ok(())
    }

    pub fn search_consent(&self, gid: Vec<u8>) -> bool {
        self::gid(&gid).is_ok_and(|g| self.lock().core.search_consent(&g))
    }

    pub fn groups(&self) -> Vec<GroupInfo> {
        let i = self.lock();
        let mut v: Vec<GroupInfo> = i
            .core
            .groups()
            .map(|g| GroupInfo {
                gid: g.gid.to_vec(),
                name: g.name.clone(),
                admin: g
                    .members
                    .iter()
                    .find(|m| m.key == g.admin)
                    .map_or(g.admin, |m| m.node)
                    .to_vec(),
                members: g
                    .members
                    .iter()
                    .map(|m| {
                        let contact = i.core.contacts().find(|c| c.key == m.node);
                        ContactInfo {
                            key: m.node.to_vec(),
                            name: i
                                .nicknames
                                .get(&m.node)
                                .cloned()
                                .unwrap_or_else(|| m.name.clone()),
                            node_name: m.name.clone(),
                            hops: None,
                            identity: Some(m.key.to_vec()),
                            verified: contact
                                .is_some_and(|c| c.verified && c.identity == Some(m.key)),
                            person: true,
                        }
                    })
                    .collect(),
                keys_known: g.keys_known() as u32,
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}
