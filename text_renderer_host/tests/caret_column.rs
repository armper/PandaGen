//! The caret must be drawn where the next keystroke will land.
//!
//! Honest note: this file pins *this crate's* side of the contract and
//! passes either way, because the renderer was already character-based. The
//! defect was on the publishing side, and
//! `services_editor_vi/tests/caret_column.rs` is the test that fails without
//! the fix. This one exists so a later change here cannot quietly move the
//! contract out from under it.

use text_renderer_host::TextRenderer;
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

/// The editor counted columns in bytes and this renderer consumes them as
/// character indices, so on any line holding a character outside ASCII the
/// caret was drawn one character further right per preceding multi-byte
/// character. `CursorPosition::column` is a character column.
#[test]
fn the_caret_marker_lands_on_the_character_it_names() {
    for (line, column, expected) in [
        ("héllo", 2, "hé|llo"),
        ("héllo", 0, "|héllo"),
        ("ascii", 3, "asc|ii"),
        ("ééé", 1, "é|éé"),
    ] {
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec![line.to_string()]),
            0,
        )
        .with_cursor(CursorPosition::new(0, column));

        let mut renderer = TextRenderer::new();
        let rendered = renderer.render_snapshot(Some(&frame), None);
        assert!(
            rendered.contains(expected),
            "line {line:?} column {column}: expected {expected:?} in {rendered:?}"
        );
    }
}
