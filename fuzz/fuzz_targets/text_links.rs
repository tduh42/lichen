//! Links and places in arbitrary text (ADR-0034): never panic, spans in order and on character
//! boundaries, links over http or https, places within the globe.
#![no_main]
use lc_core::links::{Found, find};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut last = 0;
    for s in find(text) {
        assert!(last <= s.start && s.start < s.end && s.end <= text.len());
        assert!(text.is_char_boundary(s.start) && text.is_char_boundary(s.end));
        last = s.end;
        match s.found {
            Found::Link(url) => {
                assert!(url.starts_with("https://") || url.starts_with("http://"));
                assert!(!url.contains(char::is_whitespace));
            }
            Found::Place { lat, lon } => {
                assert!(lat.abs() <= 90.0 && lon.abs() <= 180.0);
            }
        }
    }
});
