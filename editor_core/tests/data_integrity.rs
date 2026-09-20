//! What the editor owes the file it opened.

use editor_core::{CoreOutcome, EditorCore, Key, TextBuffer};

/// E8, in the buffer `kernel_bootstrap`'s editor writes to disk.
///
/// `str::lines()` discards the final newline that every text file ends with.
/// E8 fixed this in `services_editor_vi::TextBuffer` with a
/// `trailing_newline` flag and left the buffer next door -- the one the
/// kernel actually ships.
#[test]
fn opening_and_saving_a_file_does_not_change_it() {
    for content in [
        "hello\n",
        "hello",
        "one\ntwo\nthree\n",
        "one\ntwo\nthree",
        "\n",
        "",
        "trailing blank line\n\n",
    ] {
        assert_eq!(
            TextBuffer::from_string(content.to_string()).as_string(),
            content,
            "a round trip changed {content:?}"
        );
    }
}

/// The other half of the same call: `lines()` also strips a `\r` from the
/// end of every line, so a CRLF file was silently rewritten to LF.
#[test]
fn a_crlf_file_keeps_its_line_endings() {
    for content in ["a\r\nb\r\n", "a\r\nb", "\r\n", "mixed\r\nendings\nhere\r\n"] {
        assert_eq!(
            TextBuffer::from_string(content.to_string()).as_string(),
            content,
            "a round trip changed {content:?}"
        );
    }
}

/// E7, on the three siblings E7's fix missed.
///
/// `backspace` returns `None` at row 0 column 0, and the handler pushed an
/// undo snapshot before finding that out. The stack holds 100 and drops from
/// the front, so keystrokes that changed nothing evicted the user's real
/// history.
#[test]
fn keys_that_change_nothing_do_not_evict_undo_history() {
    let mut editor = EditorCore::new();
    editor.load_content("important\n".to_string());
    editor.apply_key(Key::I);
    editor.apply_key(Key::Char('X'));
    editor.apply_key(Key::Escape);
    assert_eq!(editor.buffer().line(0), Some("Ximportant"));

    editor.apply_key(Key::I);
    for _ in 0..200 {
        editor.apply_key(Key::Left);
    }
    for _ in 0..150 {
        let outcome = editor.apply_key(Key::Backspace);
        assert_eq!(
            outcome,
            CoreOutcome::Continue,
            "backspace at 0,0 reported a change"
        );
    }
    assert_eq!(
        editor.buffer().line(0),
        Some("Ximportant"),
        "the buffer changed, so this is not the no-op case"
    );
    editor.apply_key(Key::Escape);

    for _ in 0..300 {
        editor.apply_key(Key::U);
    }
    assert_eq!(
        editor.buffer().line(0),
        Some("important"),
        "150 keystrokes that did nothing made the original text unreachable"
    );
}
