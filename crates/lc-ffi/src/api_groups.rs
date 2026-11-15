//! Groups: creation, members, invitations, leaving.

use super::*;

#[uniffi::export]
impl LichenCore {
    /// Groups that list us and wait for the user (§10.1): nothing of them is used, and no
    /// member becomes a contact, until `accept_group`.
    pub fn group_invitations(&self) -> Vec<GroupInvitation> {
        let i = self.lock();
        let me = i.core.key().copied();
        i.core
            .invitations()
            .iter()
            .map(|inv| {
                let admin = inv.admin_node().unwrap_or(*inv.admin());
                let admin_name = i
                    .nicknames
                    .get(&admin)
                    .cloned()
                    .or_else(|| {
                        i.core
                            .contacts()
                            .find(|c| c.key == admin)
                            .map(|c| c.name.clone())
                    })
                    .unwrap_or_default();
                GroupInvitation {
                    gid: inv.gid().to_vec(),
                    name: inv.name().to_owned(),
                    admin: admin.to_vec(),
                    admin_name,
                    members: inv
                        .members()
                        .iter()
                        .filter(|m| Some(m.key) != me && m.key != *inv.admin())
                        .map(|m| m.name.clone())
                        .collect(),
                }
            })
            .collect()
    }

    /// Joins the group of an invitation (§10.1).
    pub fn accept_group(&self, gid: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let g = super::chat::gid(&gid)?;
        let mut i = self.lock();
        i.core.accept_group(&g, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Whether a group that lists us, from an admin not verified in person, waits for the user
    /// (the default) or is joined at once (the demo network's friends).
    pub fn set_group_consent(&self, ask: bool) {
        self.lock().core.set_group_consent(ask);
    }

    /// Does not join: the invitation is forgotten, and the admin is not told.
    pub fn decline_group(&self, gid: Vec<u8>) -> Result<(), LichenError> {
        let g = super::chat::gid(&gid)?;
        self.lock().core.decline_group(&g);
        Ok(())
    }

    pub fn create_group(
        &self,
        name: String,
        members: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<Vec<u8>, LichenError> {
        let keys = members
            .iter()
            .map(|m| key(m))
            .collect::<Result<Vec<_>, _>>()?;
        let mut i = self.lock();
        let g = i.core.create_group(&name, &keys, now_ms)?;
        i.persist();
        Ok(g.to_vec())
    }

    /// Floods a text to the group and returns it as stored: waiting until its first copy has
    /// left (then `Flooded`, without acknowledgement), §6.1.
    pub fn send_group_text(
        &self,
        gid: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.writable()?;
        i.last_now = now_ms;
        let id = i.core.send_group_text(&g, &text, sent_at, now_ms)?;
        let content = Content::Text { sent_at, text };
        let estimate = i.core.estimate_group_send(content.encode().len());
        i.stored_outgoing(Chat::Group(g), content, id, estimate, true, None)
    }

    /// Invites contacts (nodes whose identity is known) into a group we administer (ADR-0017).
    pub fn add_members(
        &self,
        gid: Vec<u8>,
        members: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let ms = members
            .iter()
            .map(|m| key(m))
            .collect::<Result<Vec<_>, _>>()?;
        let mut i = self.lock();
        i.core.add_members(&g, &ms, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Renames a group we administer (§10.1); its members see a notice.
    pub fn rename_group(&self, gid: Vec<u8>, name: String, now_ms: u64) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.core.rename_group(&g, &name, now_ms)?;
        i.persist();
        Ok(())
    }

    /// An invitation to group `gid` (admin only): it admits anyone who scans it for 10
    /// minutes, until `revoke_invite` or the app stops (protocol.md §10.5).
    pub fn create_invite(&self, gid: Vec<u8>, now_ms: u64) -> Result<InviteCode, LichenError> {
        let g = self::gid(&gid)?;
        let invite = self.lock().core.create_invite(&g, now_ms)?;
        Ok(InviteCode {
            text: invite.encode(),
            token: invite.token.to_vec(),
            expires: u64::from(invite.expires),
        })
    }

    /// Closes an invitation: its code admits no one any more.
    pub fn revoke_invite(&self, token: Vec<u8>) {
        if let Ok(t) = <[u8; 16]>::try_from(token.as_slice()) {
            self.lock().core.revoke_invite(&t);
        }
    }

    /// Reads a scanned invitation; `BadCode` if the text is not one, or is ours.
    pub fn parse_invite(&self, text: String, now_ms: u64) -> Result<InviteInfo, LichenError> {
        let invite = lc_core::identity::Invite::decode(&text).ok_or(LichenError::BadCode)?;
        let i = self.lock();
        if i.core.key() == Some(&invite.admin) {
            return Err(LichenError::BadCode);
        }
        let expired = i
            .core
            .unix_s(now_ms)
            .is_some_and(|t| t > u64::from(invite.expires) + lc_core::group::INVITE_SKEW_S);
        let member = i
            .core
            .groups()
            .any(|g| g.admin == invite.admin && g.name == invite.group_name);
        Ok(InviteInfo {
            group_name: invite.group_name,
            admin_name: invite.admin_name,
            admin: invite.node.to_vec(),
            expires: u64::from(invite.expires),
            expired,
            member,
        })
    }

    /// Joins with a scanned invitation: the admin becomes a contact, verified (scanned in
    /// person), and is asked to add us; `GroupJoined` follows once admitted (§10.5).
    pub fn accept_invite(&self, text: String, now_ms: u64) -> Result<ContactInfo, LichenError> {
        let invite = lc_core::identity::Invite::decode(&text).ok_or(LichenError::BadCode)?;
        let mut i = self.lock();
        if i.core.key() == Some(&invite.admin) {
            return Err(LichenError::BadCode);
        }
        i.last_now = now_ms;
        i.core.join(&invite, now_ms)?;
        i.save_contacts();
        let out = i
            .core
            .contacts()
            .find(|k| k.key == invite.node)
            .map(|k| ContactInfo::of(k, &i.nicknames))
            .ok_or(LichenError::UnknownContact)?;
        i.extra.push_back(LichenEvent::ContactsUpdated);
        Ok(out)
    }

    /// Leaves a group: a member asks the admin to remove it and forgets the group at once; the
    /// admin closes it for everyone (ADR-0017). The conversation stays on the phone.
    pub fn leave_group(&self, gid: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.core.leave_group(&g, now_ms)?;
        i.persist();
        Ok(())
    }

    pub fn remove_member(
        &self,
        gid: Vec<u8>,
        member: Vec<u8>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let m = key(&member)?;
        let mut i = self.lock();
        i.core.remove_member(&g, &m, now_ms)?;
        i.persist();
        Ok(())
    }
}
