//! A node's shared advert (§15.3): never panic; an accepted one holds at most 32 bytes of
//! app_data, a name inside it, and its signature check never panics either.
#![no_main]
use lc_meshcore::advert::{APP_DATA_MAX, decode_shared};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((p, a)) = decode_shared(data) {
        assert!(a.app_data.len() <= APP_DATA_MAX);
        assert!(a.name.len() < a.app_data.len().max(1));
        assert_eq!(p.payload.len(), 100 + a.app_data.len());
        let _ = a.verify();
    }
});
