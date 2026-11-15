//! Our identity, contact cards, safety numbers, identity changes.

use super::*;

#[uniffi::export]
impl LichenCore {
    /// Our identity key (ADR-0013).
    pub fn key(&self) -> Option<Vec<u8>> {
        self.lock().core.key().map(|k| k.to_vec())
    }

    /// The connected node's key.
    pub fn node_key(&self) -> Option<Vec<u8>> {
        self.lock().core.node_key().map(|k| k.to_vec())
    }

    /// The 24 words that restore our identity (§2).
    pub fn recovery_phrase(&self) -> Vec<String> {
        lc_core::identity::recovery_phrase(&self.lock().seed)
    }

    /// Our fingerprint: 6 groups of 4 hexadecimal digits (§4.7).
    pub fn fingerprint(&self) -> Vec<String> {
        let i = self.lock();
        i.core
            .key()
            .map(|k| lc_core::identity::fingerprint(k).to_vec())
            .unwrap_or_default()
    }

    /// Our contact card for a QR code (`lichen:1:…`); needs the node connected.
    pub fn contact_card(&self, name: String) -> Result<String, LichenError> {
        let i = self.lock();
        let (Some(identity), Some(node)) = (i.core.key(), i.core.node_key()) else {
            return Err(LichenError::NoIdentity);
        };
        Ok(lc_core::identity::Card {
            identity: *identity,
            node: *node,
            name,
        }
        .encode())
    }

    /// Adds (or updates) the contact of a contact card (§4.7). Scanned in person (`scanned`),
    /// the card verifies the contact, and replaces an identity it no longer has. Pasted or
    /// received some other way, it proves nothing: a new contact is added unverified, and an
    /// identity different from the one known waits for the user like any identity change
    /// (§4.6; review of 2026-11-15, SR-23).
    pub fn add_contact_card(
        &self,
        card: String,
        scanned: bool,
    ) -> Result<ContactInfo, LichenError> {
        let c = lc_core::identity::Card::decode(&card).ok_or(LichenError::BadCode)?;
        let mut i = self.lock();
        if i.core.key() == Some(&c.identity) {
            return Err(LichenError::BadCode);
        }
        let name = i
            .core
            .contacts()
            .find(|k| k.key == c.node)
            .map_or(c.name.clone(), |k| k.name.clone());
        let path = i
            .core
            .contacts()
            .find(|k| k.key == c.node)
            .and_then(|k| k.out_path.clone());
        i.core.add_contact(c.node, &name, path);
        let adopted = i.core.present_identity(&c.node, c.identity)
            || (scanned && i.core.accept_identity(&c.node, &c.identity, true));
        if adopted && scanned {
            i.core.set_verified(&c.node, &c.identity, true);
            i.pin_name(&c.node);
        }
        i.save_contacts();
        let out = i
            .core
            .contacts()
            .find(|k| k.key == c.node)
            .map(|k| ContactInfo::of(k, &i.nicknames))
            .ok_or(LichenError::UnknownContact)?;
        i.extra.push_back(LichenEvent::ContactsUpdated);
        Ok(out)
    }

    /// The safety number with contact `node` (6 groups of 5 digits), once its identity is known.
    pub fn safety_number(&self, node: Vec<u8>) -> Result<Vec<String>, LichenError> {
        let k = key(&node)?;
        let i = self.lock();
        let peer = i
            .core
            .contacts()
            .find(|c| c.key == k)
            .ok_or(LichenError::UnknownContact)?
            .identity
            .ok_or(LichenError::IdentityUnknown)?;
        let me = i.core.key().ok_or(LichenError::NoIdentity)?;
        Ok(lc_core::identity::safety_number(me, &peer).to_vec())
    }

    /// The new identity a verified contact presented, waiting for the user.
    pub fn pending_identity(&self, node: Vec<u8>) -> Result<Option<Vec<u8>>, LichenError> {
        let k = key(&node)?;
        Ok(self.lock().core.pending_identity(&k).map(|i| i.to_vec()))
    }

    /// Accepts contact `node`'s new identity `expected`, the one whose safety number the user
    /// compared, verified when `verify`. False when another identity is pending since: show
    /// the new numbers and ask again.
    pub fn accept_identity(
        &self,
        node: Vec<u8>,
        expected: Vec<u8>,
        verify: bool,
    ) -> Result<bool, LichenError> {
        let (k, e) = (key(&node)?, key(&expected)?);
        let mut i = self.lock();
        let done = i.core.accept_identity(&k, &e, verify);
        if done && verify {
            i.pin_name(&k);
        }
        i.save_contacts();
        Ok(done)
    }

    /// Rejects contact `node`'s new identity `expected`: the verified one stays.
    pub fn reject_identity(&self, node: Vec<u8>, expected: Vec<u8>) -> Result<(), LichenError> {
        let (k, e) = (key(&node)?, key(&expected)?);
        self.lock().core.reject_identity(&k, &e);
        Ok(())
    }

    /// The safety number between us and an identity (for a pending one, before accepting it).
    pub fn safety_number_with(&self, identity: Vec<u8>) -> Result<Vec<String>, LichenError> {
        let peer = key(&identity)?;
        let i = self.lock();
        let me = i.core.key().ok_or(LichenError::NoIdentity)?;
        Ok(lc_core::identity::safety_number(me, &peer).to_vec())
    }

    /// Marks contact `node` verified (safety numbers matched) or not (they differ); `identity`
    /// is the one whose number the user compared. False when the contact's identity changed
    /// meanwhile: nothing is verified.
    pub fn set_verified(
        &self,
        node: Vec<u8>,
        identity: Vec<u8>,
        verified: bool,
    ) -> Result<bool, LichenError> {
        let (k, id) = (key(&node)?, key(&identity)?);
        let mut i = self.lock();
        let done = i.core.set_verified(&k, &id, verified);
        if done && verified {
            i.pin_name(&k);
        }
        i.save_contacts();
        Ok(done)
    }
}
