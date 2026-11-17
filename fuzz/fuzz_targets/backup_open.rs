//! Backup files (docs/spec/backup.md, ADR-0037): arbitrary bytes as a file, and arbitrary
//! contents sealed under a known seed. Reading never panics, and the entries of sealed contents
//! parse or are refused without reading more than was sealed.
#![no_main]
use std::io::{Read, Write};

use lc_core::backup::{Opener, Sealer, kind, read_entry, valid_file_name};
use libfuzzer_sys::fuzz_target;

const SEED: [u8; 32] = [0x5E; 32];

fuzz_target!(|data: &[u8]| {
    if let Ok(mut r) = Opener::new(data, &SEED) {
        let _ = r.read_to_end(&mut Vec::new());
    }
    let mut file = Vec::new();
    let mut w = Sealer::new(&mut file, &SEED, [1; 32]).expect("a header");
    w.write_all(data).expect("into memory");
    w.finish().expect("into memory");
    let mut r = Opener::new(&file[..], &SEED).expect("our own header");
    let mut sink = std::io::sink();
    let mut read = 0usize;
    while let Ok((k, name, size)) = read_entry(&mut r) {
        if k == kind::END {
            break;
        }
        let _ = valid_file_name(&name);
        read += std::io::copy(&mut (&mut r).take(size), &mut sink).unwrap_or(0) as usize;
        if read > data.len() {
            panic!("more contents than were sealed");
        }
    }
});
