//! Integration tests for the vi editor
//!
//! These tests validate complete editing workflows using simulated keyboard input.

use input_types::{InputEvent, KeyCode, KeyEvent, Modifiers};
use services_editor_vi::{
    DocumentHandle, Editor, EditorAction, EditorIo, OpenOptions, OpenResult, SaveResult,
    StorageEditorIo,
};
use services_editor_vi::io::IoError;
use services_storage::{JournaledStorage, ObjectId, TransactionalStorage, VersionId};
use std::cell::RefCell;
use std::rc::Rc;

struct SharedStorageIo {
    storage: Rc<RefCell<JournaledStorage>>,
}

impl SharedStorageIo {
    fn new(storage: Rc<RefCell<JournaledStorage>>) -> Self {
        Self { storage }
    }
}

impl EditorIo for SharedStorageIo {
    fn open(&mut self, options: OpenOptions) -> Result<OpenResult, IoError> {
        let object_id = options
            .object_id
            .ok_or(IoError::NotFound)?;

        let mut storage = self.storage.borrow_mut();
        let mut tx = storage
            .begin_transaction()
            .map_err(|err| IoError::StorageError(err.to_string()))?;
        let version_id = storage
            .read(&tx, object_id)
            .map_err(|err| IoError::StorageError(err.to_string()))?;
        let data = storage
            .read_data(&tx, object_id)
            .map_err(|err| IoError::StorageError(err.to_string()))?;
        let _ = storage.rollback(&mut tx);

        let content = String::from_utf8(data).map_err(|_| IoError::InvalidUtf8)?;
        let handle = DocumentHandle::new(object_id, version_id, options.path, false);

        Ok(OpenResult { content, handle })
    }

    fn save(&mut self, handle: &DocumentHandle, content: &str) -> Result<SaveResult, IoError> {
        let mut storage = self.storage.borrow_mut();
        let mut tx = storage
            .begin_transaction()
            .map_err(|err| IoError::StorageError(err.to_string()))?;
        let new_version_id = storage
            .write(&mut tx, handle.object_id, content.as_bytes())
            .map_err(|err| IoError::StorageError(err.to_string()))?;
        storage
            .commit(&mut tx)
            .map_err(|err| IoError::StorageError(err.to_string()))?;

        Ok(SaveResult::new(
            new_version_id,
            false,
            "Saved successfully",
            Some(handle.object_id),
        ))
    }

    fn save_as(&mut self, _path: &str, _content: &str) -> Result<SaveResult, IoError> {
        Err(IoError::PermissionDenied(
            "No directory capability".to_string(),
        ))
    }
}

fn press_key(code: KeyCode) -> InputEvent {
    InputEvent::key(KeyEvent::pressed(code, Modifiers::none()))
}

fn press_key_shift(code: KeyCode) -> InputEvent {
    InputEvent::key(KeyEvent::pressed(code, Modifiers::SHIFT))
}

#[test]
fn test_basic_insert_and_save() {
    // Test A: Basic insert + save
    // Type: i hello <Esc> :w <Enter> :q <Enter>

    let mut editor = Editor::new();

    // Enter insert mode
    editor.process_input(press_key(KeyCode::I)).unwrap();

    // Type "hello"
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();

    // Exit insert mode
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    // Verify content and dirty flag
    assert_eq!(editor.get_content(), "hello");
    assert!(editor.state().is_dirty());

    // Enter command mode and save
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('w');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should have saved
    assert!(matches!(result, EditorAction::Saved(_)));
    assert!(!editor.state().is_dirty());
    assert!(editor.state().status_message().contains("Saved"));

    // Enter command mode and quit
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('q');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should quit successfully
    assert_eq!(result, EditorAction::Quit);
}

#[test]
fn test_quit_blocked_when_dirty() {
    // Test B: Quit blocked when dirty
    // Edit without saving, :q should refuse, then :q! exits

    let mut editor = Editor::new();

    // Make some edits
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    assert!(editor.state().is_dirty());

    // Try to quit
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('q');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should refuse to quit with new error message
    assert_eq!(result, EditorAction::Continue);
    assert_eq!(editor.state().status_message(), "Unsaved changes — use :w or :q!");

    // Force quit with :q!
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('q');
    editor.state_mut().append_to_command('!');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should quit successfully
    assert_eq!(result, EditorAction::Quit);
}

#[test]
fn test_navigation_with_hjkl() {
    let mut editor = Editor::new();
    editor.new_document();

    // Create multi-line content
    editor
        .state_mut()
        .set_mode(services_editor_vi::EditorMode::Insert);
    editor.process_input(press_key(KeyCode::A)).unwrap();
    editor.process_input(press_key(KeyCode::B)).unwrap();
    editor.process_input(press_key(KeyCode::C)).unwrap();
    editor.process_input(press_key(KeyCode::Enter)).unwrap();
    editor.process_input(press_key(KeyCode::D)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::F)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    // Should be at position (1, 3) after typing
    let pos = editor.state().cursor().position();
    assert_eq!(pos.row, 1);
    assert_eq!(pos.col, 3);

    // Navigate left (h)
    editor.process_input(press_key(KeyCode::H)).unwrap();
    assert_eq!(editor.state().cursor().position().col, 2);

    // Navigate up (k)
    editor.process_input(press_key(KeyCode::K)).unwrap();
    assert_eq!(editor.state().cursor().position().row, 0);

    // Navigate right (l)
    editor.process_input(press_key(KeyCode::L)).unwrap();
    assert_eq!(editor.state().cursor().position().col, 3);

    // Navigate down (j)
    editor.process_input(press_key(KeyCode::J)).unwrap();
    assert_eq!(editor.state().cursor().position().row, 1);
}

#[test]
fn test_editor_open_and_save_with_storage_io() {
    let mut storage = JournaledStorage::new();
    let object_id = ObjectId::new();

    let mut tx = storage.begin_transaction().unwrap();
    let initial_version = storage.write(&mut tx, object_id, b"hello").unwrap();
    storage.commit(&mut tx).unwrap();

    let io = StorageEditorIo::new(storage);
    let mut editor = Editor::new();
    editor.set_io(Box::new(io));

    editor
        .open_with(OpenOptions::new().with_object(object_id))
        .unwrap();
    assert_eq!(editor.get_content(), "hello");
    assert_eq!(editor.document().unwrap().version_id, initial_version);

    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::X)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('w');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();
    assert!(matches!(result, EditorAction::Saved(_)));
    assert_ne!(editor.document().unwrap().version_id, initial_version);
}

#[test]
fn test_delete_char_in_normal_mode() {
    let mut editor = Editor::new();

    // Insert "hello"
    editor.process_input(press_key(KeyCode::I)).unwrap();
    for &key in &[KeyCode::H, KeyCode::E, KeyCode::L, KeyCode::L, KeyCode::O] {
        editor.process_input(press_key(key)).unwrap();
    }
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    // Move cursor to start
    for _ in 0..5 {
        editor.process_input(press_key(KeyCode::H)).unwrap();
    }

    // Delete first character with 'x'
    editor.process_input(press_key(KeyCode::X)).unwrap();

    assert_eq!(editor.get_content(), "ello");
    assert!(editor.state().is_dirty());
}

#[test]
fn test_write_quit_combined() {
    let mut editor = Editor::new();

    // Make some edits
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::T)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::S)).unwrap();
    editor.process_input(press_key(KeyCode::T)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    assert!(editor.state().is_dirty());

    // Use :wq to save and quit
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('w');
    editor.state_mut().append_to_command('q');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should quit (and save happened internally)
    assert_eq!(result, EditorAction::Quit);
    assert!(!editor.state().is_dirty());
}

#[test]
fn test_backspace_line_join() {
    let mut editor = Editor::new();

    // Create two lines: "a\nb"
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::A)).unwrap();
    editor.process_input(press_key(KeyCode::Enter)).unwrap();
    editor.process_input(press_key(KeyCode::B)).unwrap();

    // Cursor is at (1, 1) after typing 'b'
    // First backspace deletes 'b'
    editor.process_input(press_key(KeyCode::Backspace)).unwrap();
    assert_eq!(editor.get_content(), "a\n");

    // Now cursor is at (1, 0)
    // Second backspace joins lines
    editor.process_input(press_key(KeyCode::Backspace)).unwrap();
    assert_eq!(editor.get_content(), "a");
}

#[test]
fn test_escape_cancels_command_mode() {
    let mut editor = Editor::new();

    // Enter command mode
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    assert_eq!(
        editor.state().mode(),
        services_editor_vi::EditorMode::Command
    );

    // Type partial command
    editor.state_mut().append_to_command('w');

    // Escape to cancel
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    // Should be back in normal mode
    assert_eq!(
        editor.state().mode(),
        services_editor_vi::EditorMode::Normal
    );
    assert_eq!(editor.state().command_buffer(), "");
}

#[test]
fn test_shift_modifier_for_uppercase() {
    let mut editor = Editor::new();

    editor.process_input(press_key(KeyCode::I)).unwrap();

    // Type "Hi" with shift
    editor.process_input(press_key_shift(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::I)).unwrap();

    assert_eq!(editor.get_content(), "Hi");
}

#[test]
fn test_punctuation_with_shift() {
    let mut editor = Editor::new();

    editor.process_input(press_key(KeyCode::I)).unwrap();

    // Type "hello!" - testing shift for punctuation
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    editor
        .process_input(press_key_shift(KeyCode::Num1))
        .unwrap(); // Shift+1 = !

    assert_eq!(editor.get_content(), "hello!");
}

#[test]
fn test_empty_command_error() {
    let mut editor = Editor::new();

    // Enter command mode
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();

    // Press enter without typing anything
    let result = editor.process_input(press_key(KeyCode::Enter));

    // Should be an error for empty command
    assert!(result.is_err());
}

#[test]
fn test_complex_editing_session() {
    let mut editor = Editor::new();

    // Insert first line
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::N)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::Space)).unwrap();
    editor.process_input(press_key(KeyCode::Num1)).unwrap();
    editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Insert second line
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::N)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::Space)).unwrap();
    editor.process_input(press_key(KeyCode::Num2)).unwrap();

    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    assert_eq!(editor.get_content(), "line 1\nline 2");

    // Navigate and delete
    editor.process_input(press_key(KeyCode::K)).unwrap(); // Up
    editor.process_input(press_key(KeyCode::H)).unwrap(); // Left
    editor.process_input(press_key(KeyCode::H)).unwrap(); // Left
    editor.process_input(press_key(KeyCode::X)).unwrap(); // Delete

    assert_eq!(editor.get_content(), "line1\nline 2");
}

#[test]
fn test_write_as_command_without_io() {
    // Test :w <path> command parsing and execution (without I/O, will fail gracefully)
    let mut editor = Editor::new();

    // Create some content
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    assert!(editor.state().is_dirty());

    // Try :w newfile.txt without I/O handler configured
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('w');
    editor.state_mut().append_to_command(' ');
    editor.state_mut().append_to_command('n');
    editor.state_mut().append_to_command('e');
    editor.state_mut().append_to_command('w');
    editor.state_mut().append_to_command('f');
    editor.state_mut().append_to_command('i');
    editor.state_mut().append_to_command('l');
    editor.state_mut().append_to_command('e');
    editor.state_mut().append_to_command('.');
    editor.state_mut().append_to_command('t');
    editor.state_mut().append_to_command('x');
    editor.state_mut().append_to_command('t');

    let result = editor.process_input(press_key(KeyCode::Enter));

    // Should fail gracefully since no I/O handler is configured
    assert!(result.is_err());
}

#[test]
fn test_save_as_with_storage_io() {
    // Test :w <path> with actual storage I/O
    // Create storage
    let storage = JournaledStorage::new();

    // Create filesystem view service and root
    use fs_view::DirectoryView;
    use services_fs_view::FileSystemViewService;

    let root_id = ObjectId::new();
    let root = DirectoryView::new(root_id);
    let mut fs_view = FileSystemViewService::new();
    fs_view.register_directory(root.clone());

    // Create editor with I/O
    let mut editor = Editor::new();
    let io = Box::new(StorageEditorIo::with_fs_view(storage, fs_view, root));
    editor.set_io(io);

    // Create some content
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    assert_eq!(editor.get_content(), "hello");
    assert!(editor.state().is_dirty());

    // Save as test.txt
    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('w');
    editor.state_mut().append_to_command(' ');
    editor.state_mut().append_to_command('t');
    editor.state_mut().append_to_command('e');
    editor.state_mut().append_to_command('s');
    editor.state_mut().append_to_command('t');
    editor.state_mut().append_to_command('.');
    editor.state_mut().append_to_command('t');
    editor.state_mut().append_to_command('x');
    editor.state_mut().append_to_command('t');

    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should have saved
    assert!(matches!(result, EditorAction::Saved(_)));
    assert!(!editor.state().is_dirty());
    assert!(editor.state().status_message().contains("Saved as"));
}

#[test]
fn test_open_nonexistent_file_shows_new_file() {
    // Test opening a nonexistent file shows [New File] status
    let storage = JournaledStorage::new();

    use fs_view::DirectoryView;
    use services_fs_view::FileSystemViewService;

    let root_id = ObjectId::new();
    let root = DirectoryView::new(root_id);
    let fs_view = FileSystemViewService::new();

    let mut editor = Editor::new();
    let io = Box::new(StorageEditorIo::with_fs_view(storage, fs_view, root));
    editor.set_io(io);

    // Try to open a nonexistent file
    let result = editor.open_with(OpenOptions::new().with_path("nonexistent.txt"));

    // Should succeed (creates new file buffer)
    assert!(result.is_ok());

    // Should show [New File] in status
    assert!(editor.state().status_message().contains("New File"));

    // Should have the filename as document label
    assert_eq!(editor.state().document_label(), Some("nonexistent.txt"));

    // Buffer should be empty
    assert_eq!(editor.get_content(), "");

    // Not dirty yet
    assert!(!editor.state().is_dirty());
}

#[test]
fn test_editor_persistence_across_reboot() {
    let storage = Rc::new(RefCell::new(JournaledStorage::new()));
    let object_id = ObjectId::new();

    // Seed initial content
    {
        let mut storage_mut = storage.borrow_mut();
        let mut tx = storage_mut.begin_transaction().unwrap();
        storage_mut
            .write(&mut tx, object_id, b"hello")
            .unwrap();
        storage_mut.commit(&mut tx).unwrap();
    }

    // Open in editor, modify, save
    let mut editor = Editor::new();
    editor.set_io(Box::new(SharedStorageIo::new(storage.clone())));
    editor
        .open_with(OpenOptions::new().with_object(object_id))
        .unwrap();

    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::X)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    editor
        .process_input(press_key_shift(KeyCode::Semicolon))
        .unwrap();
    editor.state_mut().append_to_command('w');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();
    assert!(matches!(result, EditorAction::Saved(_)));

    // Simulate reboot: recover from journal snapshot
    let journal = storage.borrow().journal_clone();
    let rebooted = JournaledStorage::from_journal(journal);
    let storage2 = Rc::new(RefCell::new(rebooted));

    let mut editor2 = Editor::new();
    editor2.set_io(Box::new(SharedStorageIo::new(storage2.clone())));
    editor2
        .open_with(OpenOptions::new().with_object(object_id))
        .unwrap();

    assert_eq!(editor2.get_content(), "xhello");
}

#[test]
fn test_undo_redo_insert_mode() {
    // Test undo/redo of insert mode edits
    let mut editor = Editor::new();

    // Enter insert mode (this saves undo snapshot)
    editor.process_input(press_key(KeyCode::I)).unwrap();

    // Type "hello"
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();

    // Exit insert mode
    editor.process_input(press_key(KeyCode::Escape)).unwrap();

    assert_eq!(editor.get_content(), "hello");

    // Undo - should remove all of "hello"
    editor.process_input(press_key(KeyCode::U)).unwrap();
    assert_eq!(editor.get_content(), "");
    assert!(editor.state().status_message().contains("Undo"));

    // Redo - should restore "hello"
    let ctrl_r = InputEvent::key(KeyEvent::pressed(KeyCode::R, input_types::Modifiers::CTRL));
    editor.process_input(ctrl_r).unwrap();
    assert_eq!(editor.get_content(), "hello");
    assert!(editor.state().status_message().contains("Redo"));
}

#[test]
fn test_undo_delete_char() {
    // Test undo of delete character (x)
    let mut editor = Editor::new();
    editor.load_document(
        "hello".to_string(),
        DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
    );

    // Delete first character
    editor.process_input(press_key(KeyCode::X)).unwrap();
    assert_eq!(editor.get_content(), "ello");

    // Undo
    editor.process_input(press_key(KeyCode::U)).unwrap();
    assert_eq!(editor.get_content(), "hello");
}

#[test]
fn test_undo_redo_multiple_edits() {
    // Test multiple undo/redo operations
    let mut editor = Editor::new();

    // First edit
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::A)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();
    assert_eq!(editor.get_content(), "a");

    // Second edit
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::B)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();
    assert_eq!(editor.get_content(), "ab");

    // Third edit
    editor.process_input(press_key(KeyCode::I)).unwrap();
    editor.process_input(press_key(KeyCode::C)).unwrap();
    editor.process_input(press_key(KeyCode::Escape)).unwrap();
    assert_eq!(editor.get_content(), "abc");

    // Undo twice
    editor.process_input(press_key(KeyCode::U)).unwrap();
    assert_eq!(editor.get_content(), "ab");
    editor.process_input(press_key(KeyCode::U)).unwrap();
    assert_eq!(editor.get_content(), "a");

    // Redo once
    let ctrl_r = InputEvent::key(KeyEvent::pressed(KeyCode::R, input_types::Modifiers::CTRL));
    editor.process_input(ctrl_r).unwrap();
    assert_eq!(editor.get_content(), "ab");
}

#[test]
fn test_search_basic() {
    // Test basic search functionality
    let mut editor = Editor::new();
    editor.load_document(
        "hello world\nfind this word\nhello again".to_string(),
        DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
    );

    // Enter search mode
    editor.process_input(press_key(KeyCode::Slash)).unwrap();
    assert_eq!(
        editor.state().mode(),
        services_editor_vi::EditorMode::Search
    );

    // Type search query "world"
    editor.process_input(press_key(KeyCode::W)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    editor.process_input(press_key(KeyCode::R)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::D)).unwrap();

    // Execute search
    editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should find first "world" on line 0
    assert_eq!(editor.state().cursor().position().row, 0);
    assert_eq!(editor.state().cursor().position().col, 6); // "world" starts at col 6 in "hello world"
}

#[test]
fn test_search_next() {
    // Test search next (n command)
    let mut editor = Editor::new();
    editor.load_document(
        "test word one\ntest word two\ntest word three".to_string(),
        DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
    );

    // Search for "word"
    editor.process_input(press_key(KeyCode::Slash)).unwrap();
    editor.process_input(press_key(KeyCode::W)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    editor.process_input(press_key(KeyCode::R)).unwrap();
    editor.process_input(press_key(KeyCode::D)).unwrap();
    editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Should be at first "word" (row 0, col 5)
    assert_eq!(editor.state().cursor().position().row, 0);

    // Press 'n' to find next
    editor.process_input(press_key(KeyCode::N)).unwrap();

    // Should be at second "word" (row 1, col 5)
    assert_eq!(editor.state().cursor().position().row, 1);

    // Press 'n' again
    editor.process_input(press_key(KeyCode::N)).unwrap();

    // Should be at third "word" (row 2, col 5)
    assert_eq!(editor.state().cursor().position().row, 2);
}

#[test]
fn test_search_not_found() {
    // Test search for non-existent pattern
    let mut editor = Editor::new();
    editor.load_document(
        "hello world".to_string(),
        DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
    );

    // Search for "notfound"
    editor.process_input(press_key(KeyCode::Slash)).unwrap();
    editor.process_input(press_key(KeyCode::N)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    editor.process_input(press_key(KeyCode::T)).unwrap();
    editor.process_input(press_key(KeyCode::F)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    editor.process_input(press_key(KeyCode::U)).unwrap();
    editor.process_input(press_key(KeyCode::N)).unwrap();
    editor.process_input(press_key(KeyCode::D)).unwrap();
    editor.process_input(press_key(KeyCode::Enter)).unwrap();

    // Cursor should still be at start
    assert_eq!(editor.state().cursor().position().row, 0);
    assert_eq!(editor.state().cursor().position().col, 0);

    // Status should indicate not found
    assert!(editor.state().status_message().contains("not found"));
}
