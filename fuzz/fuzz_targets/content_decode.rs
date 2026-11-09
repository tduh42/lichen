//! Datagram contents (§6, §12, §13) on arbitrary bytes: never panic, and what decodes encodes
//! back to the same bytes (a heading of 360..=510 in a position is normalised first; a small
//! file is compressed or not by our encoder's choice, ADR-0023, so only its content must survive).
#![no_main]
use lc_core::content::Content;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(c) = Content::decode(data) {
        let again = Content::decode(&c.encode());
        assert_eq!(again.as_ref(), Some(&c));
        // Texts decode lossily (invalid UTF-8 becomes U+FFFD), positions normalise.
        if !matches!(
            c,
            Content::Location(_)
                | Content::Text { .. }
                | Content::Reply { .. }
                | Content::File { .. }
        ) {
            assert_eq!(c.encode(), data);
        }
    }
});
