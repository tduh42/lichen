//! Device → app frames, alone and through the USB stream decoder, and what servers answer
//! inside them (status, neighbours, §16.5): never panic.
#![no_main]
use lc_meshcore::server::{Neighbours, ServerStatus};
use lc_meshcore::{Response, usb};
use libfuzzer_sys::fuzz_target;

fn inner(frame: &[u8]) {
    match Response::decode(frame) {
        Ok(Response::StatusResponse { data, .. }) => {
            let _ = ServerStatus::decode(data);
        }
        Ok(Response::BinaryResponse { data, .. }) => {
            for len in [0, 4, 6, 32] {
                if let Some(n) = Neighbours::decode(data, len) {
                    assert!(n.iter().count() <= usize::from(n.count));
                }
            }
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    inner(data);
    let mut d = usb::Decoder::new();
    for &b in data {
        if d.push(b) {
            assert!(!d.frame().is_empty() && d.frame().len() <= lc_meshcore::FRAME_MAX);
            inner(d.frame());
        }
    }
});
