#![no_main]

use libfuzzer_sys::fuzz_target;
use ulo_core::providers::SseSplitter;

fuzz_target!(|data: &[u8]| {
    let mut whole = SseSplitter::new();
    let expected = whole.feed_bytes(data);

    let mut bytewise = SseSplitter::new();
    let mut actual = Vec::new();
    for byte in data {
        actual.extend(bytewise.feed_bytes(std::slice::from_ref(byte)));
    }
    assert_eq!(actual, expected);
});
