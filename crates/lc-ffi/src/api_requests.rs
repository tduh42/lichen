//! Introduction requests (protocol.md §4.6, ADR-0046): someone using a contact's radio, with an
//! identity the contact does not have, wants to write. Nothing of theirs reaches the
//! conversation until the user accepts them.

use super::*;

/// What a request sent that shows as a message (reactions, edits and timers apply once it is
/// accepted).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RequestLine {
    /// When it arrived, Unix ms.
    pub at_ms: i64,
    /// Show it as a line of text (`display`): a photo or a voice message is named, not
    /// decoded, and nothing it announces is fetched before the request is accepted.
    pub message: Message,
}

/// A request waiting for the user.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct IdentityRequest {
    /// The contact whose radio it came through.
    pub node: Vec<u8>,
    /// Who it is: an identity this phone does not know for that contact.
    pub identity: Vec<u8>,
    /// The contact's name as shown (its nickname, else its radio's): "Someone using <name>'s
    /// radio".
    pub name: String,
    /// When it first came, Unix ms.
    pub at_ms: i64,
    /// What it sent, the oldest first (the last 50).
    pub messages: Vec<RequestLine>,
}

/// A message that shows as a line in a request: not a change to another one.
fn shown(c: &Content) -> bool {
    !matches!(
        c,
        Content::Reaction { .. }
            | Content::Edit { .. }
            | Content::Delete { .. }
            | Content::Timer { .. }
            | Content::Notice(_)
            | Content::Location(_)
            | Content::MediaRequest { .. }
    )
}

impl Inner {
    /// Node `node`'s name as shown: its nickname, else its contact's (any kind), else the name
    /// the radio reported for it (a node heard, a repeater, a room); empty when unknown.
    pub(crate) fn shown_name(&self, node: &PubKey) -> String {
        self.shown_name_in(node, &self.core.nodes())
    }

    /// [`Self::shown_name`] with the radio's nodes already read.
    pub(crate) fn shown_name_in(&self, node: &PubKey, nodes: &[lc_core::Node]) -> String {
        if let Some(n) = self.nicknames.get(node) {
            return n.clone();
        }
        self.core
            .contacts()
            .find(|c| c.key == *node)
            .map(|c| c.name.clone())
            .or_else(|| {
                nodes
                    .iter()
                    .find(|n| n.key == *node)
                    .map(|n| n.name.clone())
            })
            .unwrap_or_default()
    }
}

#[uniffi::export]
impl LichenCore {
    /// Introductions we did not start become requests (`ask`, the default) or pin their
    /// identity at once: off for the demo network's friends only.
    pub fn set_request_consent(&self, ask: bool) {
        self.lock().core.set_request_consent(ask);
    }

    /// The requests waiting for the user, the oldest first.
    pub fn requests(&self) -> Vec<IdentityRequest> {
        let i = self.lock();
        let Some(st) = i.store.as_ref() else {
            return Vec::new();
        };
        st.requests()
            .unwrap_or_default()
            .into_iter()
            .map(|q| IdentityRequest {
                node: q.node.to_vec(),
                identity: q.identity.to_vec(),
                name: i.shown_name(&q.node),
                at_ms: q.at,
                messages: st
                    .request_messages(&q.identity)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|m| {
                        let c = Content::decode(&m.plaintext)?;
                        shown(&c).then(|| RequestLine {
                            at_ms: m.at_ms,
                            message: c.into(),
                        })
                    })
                    .collect(),
            })
            .collect()
    }

    /// Accepts request `identity`: it becomes the identity of the contact it came through,
    /// unverified, and what it sent moves into that conversation, which the app opens.
    /// Returns the contact's node.
    pub fn accept_request(&self, identity: Vec<u8>) -> Result<Vec<u8>, LichenError> {
        let id = key(&identity)?;
        let mut i = self.lock();
        let node = i
            .core
            .accept_request(&id)
            .ok_or(LichenError::UnknownContact)?;
        i.save_contacts();
        // Now, so that the conversation opens with them; the core's event finds nothing left.
        i.adopt_request(&node, &id);
        i.extra.push_back(LichenEvent::ContactsUpdated);
        i.extra.push_back(LichenEvent::RequestsChanged);
        Ok(node.to_vec())
    }

    /// Blocks request `identity`: that identity only, not the radio it came through, which is
    /// someone else's (ADR-0034). Listed with the blocked, as a request. Its messages go.
    pub fn block_request(&self, identity: Vec<u8>) -> Result<(), LichenError> {
        let id = key(&identity)?;
        let mut i = self.lock();
        let node = i
            .store
            .as_ref()
            .and_then(|s| s.requests().ok())
            .and_then(|v| v.into_iter().find(|q| q.identity == id))
            .map(|q| q.node);
        let name = node.map(|n| i.shown_name(&n)).unwrap_or_default();
        i.core.block_request(&id);
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        // A request's block names its identity as the node (`BlockedContact::request`).
        st.block(&id, Some(&id), &name, i.wall_ms())
            .and_then(|()| st.delete_request(&id))
            .map_err(|_| LichenError::Storage)?;
        i.extra.push_back(LichenEvent::RequestsChanged);
        Ok(())
    }

    /// Deletes request `identity` and what it sent: forgotten. If it writes again, it is a
    /// new request.
    pub fn delete_request(&self, identity: Vec<u8>) -> Result<(), LichenError> {
        let id = key(&identity)?;
        let mut i = self.lock();
        i.core.forget_request(&id);
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.delete_request(&id).map_err(|_| LichenError::Storage)?;
        i.extra.push_back(LichenEvent::RequestsChanged);
        Ok(())
    }
}
