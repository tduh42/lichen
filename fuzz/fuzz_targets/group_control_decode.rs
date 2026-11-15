//! Group control decoders (GROUP_STATE, SENDER_KEY, SENDER_KEYS, GROUP_MISSING, GROUP_RESEND,
//! GROUP_SYNC; §10.1, §10.2, §10.6, §10.7) on arbitrary bytes: never panic, and what decodes
//! encodes back to the same bytes.
#![no_main]
use lc_core::group::Control;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(c) = Control::decode(data) {
        assert_eq!(c.encode(), data);
    }
});
