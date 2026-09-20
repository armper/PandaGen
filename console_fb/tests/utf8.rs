//! A line whose right margin falls inside a multi-byte character must lose
//! that character, not the whole line -- and `cols` counts columns.
//!
//! Honest note on what discriminates, because the first version of this file
//! claimed more than it delivered: `text_that_fits_is_unchanged` passes with
//! either truncation, by construction -- it is a non-regression guard on the
//! ASCII path, not a discriminator. The other four fail with the byte
//! truncation restored.

use console_fb::scrollback::{Line, ScrollbackBuffer};

#[test]
fn a_line_cut_inside_a_multibyte_character_keeps_what_fits() {
    // Nine columns of a ten-character line. The tenth character goes; under
    // the original byte truncation the cut landed inside the two-byte 'é'
    // and `as_str` returned "", losing all ten.
    let line = Line::from_text("abcdefghé!", 9);
    assert_eq!(
        line.as_str(),
        "abcdefghé",
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
        "abcdefghé",
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

/// `cols` is a column count, not a byte count.
///
/// The first fix here cut on a character boundary and went on counting
/// bytes, while every doc in the file says columns -- so a 9-column line of
/// two-byte characters kept four of them. For three-byte text (CJK,
/// box-drawing) a row showed a third of its content.
#[test]
fn cols_counts_characters_not_bytes() {
    let line = Line::from_text("ééééééééé", 9);
    assert_eq!(
        line.as_str().chars().count(),
        9,
        "a 9-column line of 2-byte characters kept {} of them: {:?}",
        line.as_str().chars().count(),
        line.as_str()
    );

    let boxes = Line::from_text("─────────", 9);
    assert_eq!(boxes.as_str().chars().count(), 9);

    let mut buffer = ScrollbackBuffer::new(9, 2, 10);
    buffer.push_line("ééééééééé");
    let visible = buffer.visible_lines();
    assert_eq!(visible[visible.len() - 1].as_str().chars().count(), 9);
}

/// Text that fits is untouched, and text longer than `cols` loses exactly
/// the characters that do not fit.
#[test]
fn text_that_fits_is_unchanged() {
    assert_eq!(Line::from_text("abcdef", 10).as_str(), "abcdef");
    assert_eq!(Line::from_text("abcdef", 6).as_str(), "abcdef");
    assert_eq!(Line::from_text("héllo", 6).as_str(), "héllo");
    // One character over the margin: lose one character, not the line.
    assert_eq!(Line::from_text("abcdefg", 6).as_str(), "abcdef");
    assert_eq!(Line::from_text("", 6).as_str(), "");
    assert_eq!(Line::from_text("abc", 0).as_str(), "");
}
