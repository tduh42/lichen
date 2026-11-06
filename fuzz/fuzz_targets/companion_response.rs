//! Device → app frames, alone and through the USB stream decoder: never panic.
#![no_main]
use lc_meshcore::{Response, usb};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = Response::decode(data);
    let mut d = usb::Decoder::new();
    for &b in data {
        if d.push(b) {
            assert!(!d.frame().is_empty() && d.frame().len() <= lc_meshcore::FRAME_MAX);
            let _ = Response::decode(d.frame());
        }
    }
});
