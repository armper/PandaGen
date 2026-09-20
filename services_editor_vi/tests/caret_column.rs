//! The caret the editor publishes must name the same character the next
//! keystroke will land on.

use core_types::TaskId;
use ipc::ChannelId;
use services_editor_vi::state::Position;
use services_editor_vi::{DocumentHandle, Editor};
use services_storage::{ObjectId, VersionId};
use services_view_host::ViewHost;
use view_types::{ViewContent, ViewKind};

/// The editor counts columns in bytes -- `line_length` is `String::len` and
/// every edit indexes the line with that number -- while
/// `text_renderer_host` consumes `CursorPosition::column` as a character
/// index. On a line holding anything outside ASCII the two disagree, and the
/// caret is drawn one character further right per preceding multi-byte
/// character: the user types into a position they cannot see.
#[test]
fn the_published_caret_column_counts_characters() {
    let mut host = ViewHost::new();
    let owner = TaskId::new();
    let main = host
        .create_view(ViewKind::TextBuffer, None, owner, ChannelId::new())
        .unwrap();
    let status = host
        .create_view(ViewKind::StatusLine, None, owner, ChannelId::new())
        .unwrap();

    let mut editor = Editor::new();
    editor.set_view_handles(main.clone(), status);
    editor.load_document(
        "héllo".to_string(),
        DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
    );

    // Put the cursor on the character after 'é'. The editor's own column is
    // a byte offset, so that is 3; the character column is 2.
    editor
        .state_mut()
        .cursor_mut()
        .set_position(Position::new(0, 3));
    editor.publish_views(&mut host, 1).unwrap();

    let frame = host.get_latest(main.view_id).unwrap().unwrap();
    let cursor = frame.cursor.expect("a cursor must be published");
    assert_eq!(
        cursor.column, 2,
        "published column {} is a byte offset, not a character column; the \
         renderer draws the caret one character right of where the edit goes",
        cursor.column
    );

    // Sanity: the content is what we think it is.
    match &frame.content {
        ViewContent::TextBuffer { lines } => assert_eq!(lines[0], "héllo"),
        other => panic!("unexpected content: {other:?}"),
    }
}
