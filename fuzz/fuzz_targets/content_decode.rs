//! Datagram contents (§6, §12, §13) on arbitrary bytes: never panic, and what decodes encodes
//! back to the same bytes (a heading of 360..=510 in a position is normalised first).
#![no_main]
use lc_core::content::Content;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(c) = Content::decode(data) {
        let again = Content::decode(&c.encode());
        assert_eq!(again.as_ref(), Some(&c));
        if !matches!(c, Content::Location(_) | Content::Text { .. }) {
            assert_eq!(c.encode(), data);
        }
    }
});
