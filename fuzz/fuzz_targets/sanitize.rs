#![no_main]

use libfuzzer_sys::fuzz_target;
use ulo_core::tools::sanitize_display;
use ulo_tui::markdown::{clip_styled, visible_width, wrap_styled};

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
