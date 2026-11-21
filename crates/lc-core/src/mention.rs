//! Mentions in a MeshCore text (protocol §15.9, ADR-0048): `@[name]`, as observed in the stock
//! app's messages. A mention names a person by the name their radio advertises, not a message.
//! One at the very start of a text makes it a reply to that person; anywhere else, it is a
//! mention. The apps get a text cut into spans (text, link, place, mention), so they parse
//! nothing.
//!
//! The text comes from anyone: the scanner never panics and stays linear in its length.

use crate::links::{self, Found};

/// A mention, by byte offsets of the whole `@[name]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub start: usize,
    pub end: usize,
    pub name: String,
}

/// The mentions in `text`, in order: `@[`, then a name of one character or more without `]`
/// or a line break, then `]`. An `@[` that does not close is text.
pub fn find(text: &str) -> Vec<Mention> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text
        .get(from..)
        .and_then(|t| t.find("@["))
        .map(|i| from + i)
    {
        let name_at = at + 2;
        let rest = text.get(name_at..).unwrap_or("");
        let close = rest.find([']', '\n', '\r']);
        match close {
            Some(n) if n > 0 && rest.get(n..n + 1) == Some("]") => {
                out.push(Mention {
                    start: at,
                    end: name_at + n + 1,
                    name: rest.get(..n).unwrap_or("").to_owned(),
                });
                from = name_at + n + 1;
            }
            _ => from = name_at,
        }
    }
    out
}

/// The person a text answers: a mention at its very start (§15.9).
pub fn reply_to(text: &str) -> Option<String> {
    find(text)
        .into_iter()
        .next()
        .filter(|m| m.start == 0)
        .map(|m| m.name)
}

/// Whether `text` mentions `name`, compared exactly (§15.9): our radio's name, for "mentions
/// us". An empty name is never mentioned.
pub fn mentions(text: &str, name: &str) -> bool {
    !name.is_empty() && find(text).iter().any(|m| m.name == name)
}

/// The text of a reply to `name`: `@[name] ` and `text` (§15.9). `None` for a name that
/// cannot be written in a mention (empty, or holding `]` or a line break).
pub fn reply_text(name: &str, text: &str) -> Option<String> {
    let writable = !name.is_empty() && !name.contains([']', '\n', '\r']);
    writable.then(|| format!("@[{name}] {text}"))
}

/// `text` as one line shows it (§15.9): each mention written `@name`, without its brackets,
/// the rest as it is. For a conversation's preview and a notification.
pub fn shown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for m in find(text) {
        out.push_str(text.get(at..m.start).unwrap_or(""));
        out.push('@');
        out.push_str(&m.name);
        at = m.end;
    }
    out.push_str(text.get(at..).unwrap_or(""));
    out
}

/// What a span of a MeshCore text is.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text,
    /// An address (see [`links::Found::Link`]).
    Link(String),
    /// A place, WGS 84 degrees.
    Place {
        lat: f64,
        lon: f64,
    },
    /// A mention of the person with this name.
    Mention(String),
}

/// A span of a MeshCore text, by byte offsets.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub start: usize,
    pub end: usize,
    pub piece: Piece,
}

/// `text` cut into spans that cover it, in order: mentions, then the links and places outside
/// them (ADR-0034), and the text between.
pub fn spans(text: &str) -> Vec<Part> {
    let mentions = find(text);
    let mut marked: Vec<Part> = mentions
        .iter()
        .map(|m| Part {
            start: m.start,
            end: m.end,
            piece: Piece::Mention(m.name.clone()),
        })
        .collect();
    for s in links::find(text) {
        if mentions
            .iter()
            .all(|m| s.end <= m.start || s.start >= m.end)
        {
            marked.push(Part {
                start: s.start,
                end: s.end,
                piece: match s.found {
                    Found::Link(url) => Piece::Link(url),
                    Found::Place { lat, lon } => Piece::Place { lat, lon },
                },
            });
        }
    }
    marked.sort_by_key(|p| p.start);
    let mut out = Vec::with_capacity(marked.len() * 2 + 1);
    let mut at = 0;
    for p in marked {
        if p.start > at {
            out.push(Part {
                start: at,
                end: p.start,
                piece: Piece::Text,
            });
        }
        at = p.end;
        out.push(p);
    }
    if at < text.len() {
        out.push(Part {
            start: at,
            end: text.len(),
            piece: Piece::Text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<String> {
        find(text).into_iter().map(|m| m.name).collect()
    }

    #[test]
    fn the_mentions_of_a_text() {
        assert_eq!(names("@[Lucie] thanks"), ["Lucie"]);
        // Several, one at the end, emoji names.
        assert_eq!(
            names("@[Marc] and @[🏔️ Hut] say hi to @[hub west]"),
            ["Marc", "🏔️ Hut", "hub west"]
        );
        // A name is anything up to the first `]`: a `[` is part of it, a `]` ends it.
        assert_eq!(names("@[a[b] x"), ["a[b"]);
        assert_eq!(names("@[Hut [west]] ok"), ["Hut [west"]);
        // Not mentions: unclosed, empty, across a line, a lone @ or [.
        for text in [
            "@[Lucie",
            "@[] x",
            "@[Lu\ncie] x",
            "@ Lucie",
            "[Lucie]",
            "@@[",
            "@[",
        ] {
            assert!(names(text).is_empty(), "{text:?}");
        }
        // An unclosed one before a closed one.
        assert_eq!(names("@[x @[Marc] y"), ["x @[Marc"]);
        assert_eq!(names("@[x\n@[Marc] y"), ["Marc"]);
        let m = &find("hi @[Marc]")[0];
        assert_eq!((m.start, m.end), (3, 10));
    }

    #[test]
    fn a_leading_mention_is_a_reply() {
        assert_eq!(reply_to("@[Lucie] thanks").as_deref(), Some("Lucie"));
        assert_eq!(reply_to("@[Lucie]").as_deref(), Some("Lucie"));
        assert_eq!(reply_to("thanks @[Lucie]"), None);
        assert_eq!(reply_to(" @[Lucie] thanks"), None);
        assert_eq!(reply_to("@[Lucie thanks"), None);
    }

    #[test]
    fn mentions_us_exactly() {
        assert!(mentions("coming up, @[ThinkNode M5]?", "ThinkNode M5"));
        for other in ["thinknode m5", "ThinkNode", "ThinkNode M5 ", ""] {
            assert!(!mentions("coming up, @[ThinkNode M5]?", other), "{other:?}");
        }
        assert!(!mentions("@[ThinkNode M5 ] hi", "ThinkNode M5"));
        assert!(!mentions("ThinkNode M5, hi", "ThinkNode M5"));
    }

    #[test]
    fn a_reply_s_text() {
        assert_eq!(
            reply_text("Lucie", "thanks").as_deref(),
            Some("@[Lucie] thanks")
        );
        assert_eq!(
            reply_text("Lucie", "thanks").map(String::into_bytes),
            Some(b"@[Lucie] thanks".to_vec())
        );
        assert_eq!(
            reply_to(&reply_text("🏔️ Hut", "ok").unwrap()).as_deref(),
            Some("🏔️ Hut")
        );
        for bad in ["", "a]b", "a\nb"] {
            assert_eq!(reply_text(bad, "x"), None, "{bad:?}");
        }
    }

    #[test]
    fn shown_without_brackets() {
        assert_eq!(
            shown("Coming up, @[ThinkNode M5]? @[Lucie] too"),
            "Coming up, @ThinkNode M5? @Lucie too"
        );
        assert_eq!(shown("@[Lucie"), "@[Lucie");
        assert_eq!(shown("no mention"), "no mention");
        assert_eq!(shown("@[🏔️ Hut] ok"), "@🏔️ Hut ok");
    }

    #[test]
    fn spans_cover_the_text() {
        let text = "@[Lucie] see https://example.org at 45.1234, 5.6789 @[Marc]";
        let parts = spans(text);
        let pieces: Vec<(&str, &Piece)> = parts
            .iter()
            .map(|p| (&text[p.start..p.end], &p.piece))
            .collect();
        assert_eq!(pieces[0], ("@[Lucie]", &Piece::Mention("Lucie".into())));
        assert_eq!(pieces[1], (" see ", &Piece::Text));
        assert_eq!(
            pieces[2],
            (
                "https://example.org",
                &Piece::Link("https://example.org".into())
            )
        );
        assert!(matches!(pieces[4].1, Piece::Place { .. }), "{pieces:?}");
        assert_eq!(
            *pieces.last().unwrap(),
            ("@[Marc]", &Piece::Mention("Marc".into()))
        );
        // Contiguous, from 0 to the end.
        let mut at = 0;
        for p in &parts {
            assert_eq!(p.start, at);
            at = p.end;
        }
        assert_eq!(at, text.len());
        // A link inside a mention's name is the mention's.
        let parts = spans("@[www.example.org] hi");
        assert_eq!(parts[0].piece, Piece::Mention("www.example.org".into()));
        assert!(parts.iter().all(|p| !matches!(p.piece, Piece::Link(_))));
        assert!(spans("").is_empty());
        assert_eq!(spans("plain").len(), 1);
    }
}
