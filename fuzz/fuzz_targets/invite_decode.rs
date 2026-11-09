//! Contact card (§4.7) and group invitation (§10.5) decoders on arbitrary text: never panic,
//! and what decodes encodes back to the same card or invitation.
#![no_main]
use lc_core::identity::{Card, Invite};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    for prefix in ["", "lichen:1:", "lichen:1:g:"] {
        let text = format!("{prefix}{text}");
        if let Some(c) = Card::decode(&text) {
            assert_eq!(Card::decode(&c.encode()), Some(c));
        }
        if let Some(i) = Invite::decode(&text) {
            assert_eq!(Invite::decode(&i.encode()), Some(i));
        }
    }
});
