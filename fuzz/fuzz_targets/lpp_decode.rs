//! A node's readings, CayenneLPP (§16.7): never panic, never read past the input, stop for good
//! once stopped, and what is kept decodes to the same items again.
#![no_main]
use lc_meshcore::lpp;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut it = lpp::items(data);
    let items: Vec<_> = it.by_ref().collect();
    assert!(it.next().is_none());
    let n = it.consumed();
    assert!(n <= data.len());
    assert_eq!(lpp::known_len(data), n);
    let again: Vec<_> = lpp::items(&data[..n]).collect();
    assert_eq!(again, items);
    // Every item takes at least three bytes.
    assert!(items.len() * 3 <= n);
});
