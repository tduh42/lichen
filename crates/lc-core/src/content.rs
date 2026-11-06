//! Datagram contents (§6): `content_type ‖ body`.

use lc_proto::content;

/// A decoded datagram content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// `ts u32 ‖ UTF-8 text`.
    Text { sent_at: u32, text: String },
    /// A type this version does not interpret.
    Other { content_type: u8, body: Vec<u8> },
}

impl Content {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Content::Text { sent_at, text } => {
                let mut v = Vec::with_capacity(5 + text.len());
                v.push(content::TEXT);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(text.as_bytes());
                v
            }
            Content::Other { content_type, body } => {
                let mut v = Vec::with_capacity(1 + body.len());
                v.push(*content_type);
                v.extend_from_slice(body);
                v
            }
        }
    }

    pub fn decode(datagram: &[u8]) -> Option<Self> {
        let (&content_type, body) = datagram.split_first()?;
        if content_type == content::TEXT {
            let (ts, text) = body.split_at_checked(4)?;
            if text.is_empty() {
                return None;
            }
            Some(Content::Text {
                sent_at: u32::from_le_bytes(ts.try_into().ok()?),
                text: String::from_utf8_lossy(text).into_owned(),
            })
        } else {
            Some(Content::Other {
                content_type,
                body: body.to_vec(),
            })
        }
    }
}
