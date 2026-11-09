//! GIPHY links turned into GIFs (§6.3) on arbitrary text: never panic, and what comes out is
//! an id of letters and digits that a GIF message carries back unchanged.
#![no_main]
use lc_core::content::{Content, GIF_GIPHY, GIF_TITLE_MAX, gif_link};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Some((id, title)) = gif_link(text) {
        assert!((1..=32).contains(&id.len()));
        assert!(id.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert!(title.len() <= GIF_TITLE_MAX);
        let gif = Content::Gif {
            sent_at: 1,
            provider: GIF_GIPHY,
            width: 0,
            height: 0,
            id,
            title,
        };
        assert_eq!(Content::decode(&gif.encode()), Some(gif));
    }
});
