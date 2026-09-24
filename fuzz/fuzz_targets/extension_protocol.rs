#![no_main]

use libfuzzer_sys::fuzz_target;
use ulo::core::extensions::parse_incoming;

fuzz_target!(|data: &[u8]| {
    let line = String::from_utf8_lossy(data);
    let _ = parse_incoming(&line);
});
