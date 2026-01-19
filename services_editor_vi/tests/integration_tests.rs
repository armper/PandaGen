//! Integration tests for the vi editor
//!
//! These tests validate complete editing workflows using simulated keyboard input.

use input_types::{InputEvent, KeyCode, KeyEvent, Modifiers};
use services_editor_vi::{Editor, EditorAction};

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

    // Should refuse to quit
    assert_eq!(result, EditorAction::Continue);
    assert!(editor.state().status_message().contains("No write"));

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
