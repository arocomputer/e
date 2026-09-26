#![no_main]

use e_core::tools::sanitize_display;
use e_tui::markdown::{clip_styled, visible_width, wrap_styled};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let clean = sanitize_display(&input);
    assert!(clean
        .chars()
        .all(|character| !character.is_control() || character == '\n'));
    let _ = visible_width(&input);
    let _ = clip_styled(&input, 80);
    let _ = wrap_styled(&input, 80);
});
