//! A line whose right margin falls inside a multi-byte character must lose
//! that character, not the whole line.

use console_fb::scrollback::{Line, ScrollbackBuffer};

#[test]
fn a_line_cut_inside_a_multibyte_character_keeps_what_fits() {
    // Byte 9 splits the two-byte 'é'.
    let line = Line::from_text("abcdefghé!", 9);
    assert_eq!(
        line.as_str(),
        "abcdefgh",
        "the whole line vanished instead of losing its last character"
    );
}

#[test]
fn scrollback_keeps_a_line_that_straddles_the_right_margin() {
    let mut buffer = ScrollbackBuffer::new(9, 3, 100);
    buffer.push_line("abcdefghé");
    let visible = buffer.visible_lines();
    assert_eq!(
        visible[visible.len() - 1].as_str(),
        "abcdefgh",
        "a line ending in a character that straddles the margin displayed as empty"
    );
}

/// Bytes assembled one at a time through `push` can still leave the buffer
/// cut mid-character, and `as_str` must fall back to the longest valid
/// prefix rather than to nothing.
#[test]
fn a_half_written_character_does_not_blank_the_line() {
    let mut line = Line::new(16);
    for byte in "abc".bytes() {
        line.push(byte, 16);
    }
    // The first byte of a two-byte sequence, with the second never arriving.
    line.push(0xc3, 16);
    assert_eq!(line.as_str(), "abc");
}

/// Text that fits is untouched, including text that is exactly `cols` bytes.
#[test]
fn text_that_fits_is_unchanged() {
    assert_eq!(Line::from_text("abcdef", 10).as_str(), "abcdef");
    assert_eq!(Line::from_text("abcdef", 6).as_str(), "abcdef");
    assert_eq!(Line::from_text("héllo", 6).as_str(), "héllo");
}
