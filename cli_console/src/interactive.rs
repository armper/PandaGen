//! Interactive Input Demo
//!
//! This module demonstrates how interactive components can receive keyboard input
//! using the input service and focus manager.

use core_types::TaskId;
use input_types::{InputEvent, KeyCode, KeyEvent};
use ipc::ChannelId;
use services_focus_manager::FocusManager;
use services_input::{InputService, InputSubscriptionCap};

/// Interactive console
///
/// A minimal demonstration of an interactive component that:
/// - Subscribes to keyboard input
/// - Requests focus
/// - Processes input events
/// - Translates events to actions
/// - Maintains command history
/// - Supports cursor movement and line editing
pub struct InteractiveConsole {
    /// Task ID for this console
    task_id: TaskId,
    /// Input subscription capability
    subscription: Option<InputSubscriptionCap>,
    /// Buffer of received events (for demo purposes)
    event_buffer: Vec<InputEvent>,
    /// Typed text buffer (simple demo)
    text_buffer: String,
    /// Cursor position within text buffer
    cursor_pos: usize,
    /// Command history (max 100 commands)
    history: Vec<String>,
    /// Current history position (for up/down arrow navigation)
    history_pos: Option<usize>,
}

impl InteractiveConsole {
    /// Creates a new interactive console
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            subscription: None,
            event_buffer: Vec::new(),
            text_buffer: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_pos: None,
        }
    }

    /// Subscribes to keyboard input
    pub fn subscribe(
        &mut self,
        input_service: &mut InputService,
        channel: ChannelId,
    ) -> Result<(), String> {
        let cap = input_service
            .subscribe_keyboard(self.task_id, channel)
            .map_err(|e| format!("Failed to subscribe: {:?}", e))?;

        self.subscription = Some(cap);
        Ok(())
    }

    /// Requests focus
    pub fn request_focus(&self, focus_manager: &mut FocusManager) -> Result<(), String> {
        let cap = self
            .subscription
            .as_ref()
            .ok_or("Not subscribed to input")?;

        focus_manager
            .request_focus(*cap)
            .map_err(|e| format!("Failed to request focus: {:?}", e))?;

        Ok(())
    }

    /// Checks if this console has focus
    pub fn has_focus(&self, focus_manager: &FocusManager) -> bool {
        if let Some(cap) = &self.subscription {
            focus_manager.has_focus(cap)
        } else {
            false
        }
    }

    /// Processes an input event
    ///
    /// Returns Ok(Some(command)) if a command should be executed,
    /// Ok(None) if no command, Err on error.
    pub fn process_event(&mut self, event: InputEvent) -> Result<Option<String>, String> {
        self.event_buffer.push(event.clone());

        // Only process key press events (not release or repeat for this demo)
        if let Some(key_event) = event.as_key() {
            if !key_event.is_pressed() {
                return Ok(None);
            }

            return self.handle_key_press(key_event);
        }

        Ok(None)
    }

    /// Handles a key press event
    fn handle_key_press(&mut self, event: &KeyEvent) -> Result<Option<String>, String> {
        // Handle special keys
        match event.code {
            KeyCode::Enter => {
                // Execute command
                let command = self.text_buffer.clone();
                if !command.is_empty() {
                    // Add to history (max 100 commands)
                    if self.history.len() >= 100 {
                        self.history.remove(0);
                    }
                    self.history.push(command.clone());
                }
                self.text_buffer.clear();
                self.cursor_pos = 0;
                self.history_pos = None;
                return Ok(Some(command));
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.text_buffer.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                }
                return Ok(None);
            }
            KeyCode::Delete => {
                if self.cursor_pos < self.text_buffer.len() {
                    self.text_buffer.remove(self.cursor_pos);
                }
                return Ok(None);
            }
            KeyCode::Escape => {
                self.text_buffer.clear();
                self.cursor_pos = 0;
                self.history_pos = None;
                return Ok(None);
            }
            KeyCode::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
                return Ok(None);
            }
            KeyCode::Right => {
                if self.cursor_pos < self.text_buffer.len() {
                    self.cursor_pos += 1;
                }
                return Ok(None);
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
                return Ok(None);
            }
            KeyCode::End => {
                self.cursor_pos = self.text_buffer.len();
                return Ok(None);
            }
            KeyCode::Up => {
                // Navigate history backward
                if !self.history.is_empty() {
                    let pos = match self.history_pos {
                        None => self.history.len() - 1,
                        Some(p) if p > 0 => p - 1,
                        Some(p) => p,
                    };
                    self.history_pos = Some(pos);
                    self.text_buffer = self.history[pos].clone();
                    self.cursor_pos = self.text_buffer.len();
                }
                return Ok(None);
            }
            KeyCode::Down => {
                // Navigate history forward
                if let Some(pos) = self.history_pos {
                    if pos + 1 < self.history.len() {
                        self.history_pos = Some(pos + 1);
                        self.text_buffer = self.history[pos + 1].clone();
                        self.cursor_pos = self.text_buffer.len();
                    } else {
                        // At end of history, clear buffer
                        self.history_pos = None;
                        self.text_buffer.clear();
                        self.cursor_pos = 0;
                    }
                }
                return Ok(None);
            }
            _ => {}
        }

        // Handle simple text input (very basic, no shift/caps)
        let ch = self.key_to_char(event)?;
        if let Some(c) = ch {
            self.text_buffer.insert(self.cursor_pos, c);
            self.cursor_pos += 1;
            self.history_pos = None; // Reset history position on new input
        }

        Ok(None)
    }

    /// Converts a key event to a character (simplified)
    fn key_to_char(&self, event: &KeyEvent) -> Result<Option<char>, String> {
        // This is a simplified mapping; a real implementation would be more complex
        match event.code {
            KeyCode::A => Ok(Some('a')),
            KeyCode::B => Ok(Some('b')),
            KeyCode::C => Ok(Some('c')),
            KeyCode::D => Ok(Some('d')),
            KeyCode::E => Ok(Some('e')),
            KeyCode::F => Ok(Some('f')),
            KeyCode::G => Ok(Some('g')),
            KeyCode::H => Ok(Some('h')),
            KeyCode::I => Ok(Some('i')),
            KeyCode::J => Ok(Some('j')),
            KeyCode::K => Ok(Some('k')),
            KeyCode::L => Ok(Some('l')),
            KeyCode::M => Ok(Some('m')),
            KeyCode::N => Ok(Some('n')),
            KeyCode::O => Ok(Some('o')),
            KeyCode::P => Ok(Some('p')),
            KeyCode::Q => Ok(Some('q')),
            KeyCode::R => Ok(Some('r')),
            KeyCode::S => Ok(Some('s')),
            KeyCode::T => Ok(Some('t')),
            KeyCode::U => Ok(Some('u')),
            KeyCode::V => Ok(Some('v')),
            KeyCode::W => Ok(Some('w')),
            KeyCode::X => Ok(Some('x')),
            KeyCode::Y => Ok(Some('y')),
            KeyCode::Z => Ok(Some('z')),
            KeyCode::Space => Ok(Some(' ')),
            _ => Ok(None), // Ignore other keys for now
        }
    }

    /// Returns the current text buffer
    pub fn text_buffer(&self) -> &str {
        &self.text_buffer
    }

    /// Returns the current cursor position
    pub fn cursor_pos(&self) -> usize {
        self.cursor_pos
    }

    /// Returns the event buffer (for testing)
    pub fn event_buffer(&self) -> &[InputEvent] {
        &self.event_buffer
    }

    /// Returns the command history
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Clears the event buffer (for testing)
    #[cfg(test)]
    pub fn clear_event_buffer(&mut self) {
        self.event_buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::Modifiers;

    #[test]
    fn test_interactive_console_creation() {
        let task_id = TaskId::new();
        let console = InteractiveConsole::new(task_id);

        assert_eq!(console.task_id, task_id);
        assert!(console.subscription.is_none());
        assert_eq!(console.text_buffer(), "");
    }

    #[test]
    fn test_subscribe_to_input() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);
        let mut input_service = InputService::new();
        let channel = ChannelId::new();

        console.subscribe(&mut input_service, channel).unwrap();

        assert!(console.subscription.is_some());
    }

    #[test]
    fn test_request_focus() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);
        let mut input_service = InputService::new();
        let mut focus_manager = FocusManager::new();
        let channel = ChannelId::new();

        console.subscribe(&mut input_service, channel).unwrap();
        console.request_focus(&mut focus_manager).unwrap();

        assert!(console.has_focus(&focus_manager));
    }

    #[test]
    fn test_process_simple_text() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        let event = InputEvent::key(KeyEvent::pressed(KeyCode::H, Modifiers::none()));
        console.process_event(event).unwrap();

        assert_eq!(console.text_buffer(), "h");
    }

    #[test]
    fn test_process_multiple_keys() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::H,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::E,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::L,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::L,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::O,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "hello");
    }

    #[test]
    fn test_backspace() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::B,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Backspace,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "a");
    }

    #[test]
    fn test_escape_clears_buffer() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::B,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Escape,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "");
    }

    #[test]
    fn test_enter_returns_command() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::L,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::S,
                Modifiers::none(),
            )))
            .unwrap();

        let result = console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Enter,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(result, Some("ls".to_string()));
        assert_eq!(console.text_buffer(), ""); // Buffer cleared after enter
    }

    #[test]
    fn test_space_key() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::H,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Space,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::I,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "h i");
    }

    #[test]
    fn test_only_process_pressed_events() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Pressed event should be processed
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();

        // Released event should be ignored
        console
            .process_event(InputEvent::key(KeyEvent::released(
                KeyCode::B,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "a"); // Only 'a' processed
    }

    #[test]
    fn test_losing_focus() {
        let task_id1 = TaskId::new();
        let task_id2 = TaskId::new();
        let mut console1 = InteractiveConsole::new(task_id1);
        let mut console2 = InteractiveConsole::new(task_id2);
        let mut input_service = InputService::new();
        let mut focus_manager = FocusManager::new();
        let channel1 = ChannelId::new();
        let channel2 = ChannelId::new();

        console1.subscribe(&mut input_service, channel1).unwrap();
        console1.request_focus(&mut focus_manager).unwrap();
        assert!(console1.has_focus(&focus_manager));

        console2.subscribe(&mut input_service, channel2).unwrap();
        console2.request_focus(&mut focus_manager).unwrap();

        // console1 should no longer have focus
        assert!(!console1.has_focus(&focus_manager));
        assert!(console2.has_focus(&focus_manager));
    }

    #[test]
    fn test_simulated_typing_session() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Simulate typing "ls"
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::L,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::S,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.text_buffer(), "ls");

        let command = console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Enter,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(command, Some("ls".to_string()));
        assert_eq!(console.text_buffer(), "");
    }

    #[test]
    fn test_command_history() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type and execute first command
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::L,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::S,
                Modifiers::none(),
            )))
            .unwrap();
        let cmd1 = console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Enter,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(cmd1, Some("ls".to_string()));

        // Type and execute second command
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::C,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::D,
                Modifiers::none(),
            )))
            .unwrap();
        let cmd2 = console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Enter,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(cmd2, Some("cd".to_string()));

        // Check history
        assert_eq!(console.history().len(), 2);
        assert_eq!(console.history()[0], "ls");
        assert_eq!(console.history()[1], "cd");
    }

    #[test]
    fn test_history_navigation() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Add commands to history
        console.history.push("ls".to_string());
        console.history.push("cd".to_string());
        console.history.push("pwd".to_string());

        // Press up arrow - should get last command
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Up,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.text_buffer(), "pwd");

        // Press up again - should get second-to-last
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Up,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.text_buffer(), "cd");

        // Press down - should get back to last
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Down,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.text_buffer(), "pwd");
    }

    #[test]
    fn test_simulated_typing_session_old() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);
        let mut input_service = InputService::new();
        let mut focus_manager = FocusManager::new();
        let channel = ChannelId::new();

        // Setup
        console.subscribe(&mut input_service, channel).unwrap();
        console.request_focus(&mut focus_manager).unwrap();

        // Simulate typing "ls"
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::L,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::S,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "ls");

        // Execute command
        let command = console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Enter,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(command, Some("ls".to_string()));
        assert_eq!(console.text_buffer(), "");
    }

    #[test]
    fn test_cursor_left_right() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type "abc"
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::B,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::C,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "abc");
        assert_eq!(console.cursor_pos(), 3);

        // Move left twice
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Left,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 2);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Left,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 1);

        // Move right once
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Right,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 2);
    }

    #[test]
    fn test_cursor_home_end() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type "hello"
        for ch in [KeyCode::H, KeyCode::E, KeyCode::L, KeyCode::L, KeyCode::O] {
            console
                .process_event(InputEvent::key(KeyEvent::pressed(ch, Modifiers::none())))
                .unwrap();
        }

        assert_eq!(console.cursor_pos(), 5);

        // Home key
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Home,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 0);

        // End key
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::End,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 5);
    }

    #[test]
    fn test_insert_at_cursor() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type "ac"
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::C,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "ac");

        // Move left
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Left,
                Modifiers::none(),
            )))
            .unwrap();

        // Insert 'b'
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::B,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "abc");
        assert_eq!(console.cursor_pos(), 2);
    }

    #[test]
    fn test_backspace_at_cursor() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type "abc"
        for ch in [KeyCode::A, KeyCode::B, KeyCode::C] {
            console
                .process_event(InputEvent::key(KeyEvent::pressed(ch, Modifiers::none())))
                .unwrap();
        }

        // Move to position 2 (between 'b' and 'c')
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Left,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.cursor_pos(), 2);

        // Backspace (should remove 'b')
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Backspace,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "ac");
        assert_eq!(console.cursor_pos(), 1);
    }

    #[test]
    fn test_delete_at_cursor() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type "abc"
        for ch in [KeyCode::A, KeyCode::B, KeyCode::C] {
            console
                .process_event(InputEvent::key(KeyEvent::pressed(ch, Modifiers::none())))
                .unwrap();
        }

        // Move to position 1 (after 'a', before 'b')
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Home,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Right,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.cursor_pos(), 1);

        // Delete (should remove 'b')
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Delete,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "ac");
        assert_eq!(console.cursor_pos(), 1);
    }

    #[test]
    fn test_backspace_at_start() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Home,
                Modifiers::none(),
            )))
            .unwrap();

        // Backspace at position 0 should do nothing
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Backspace,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "a");
        assert_eq!(console.cursor_pos(), 0);
    }

    #[test]
    fn test_delete_at_end() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();

        // Delete at end should do nothing
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Delete,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "a");
        assert_eq!(console.cursor_pos(), 1);
    }

    #[test]
    fn test_cursor_movement_bounds() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Type "ab"
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::A,
                Modifiers::none(),
            )))
            .unwrap();
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::B,
                Modifiers::none(),
            )))
            .unwrap();

        // Try to move right beyond end
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Right,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 2); // Should stay at 2

        // Move to start
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Home,
                Modifiers::none(),
            )))
            .unwrap();

        // Try to move left beyond start
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Left,
                Modifiers::none(),
            )))
            .unwrap();
        assert_eq!(console.cursor_pos(), 0); // Should stay at 0
    }

    #[test]
    fn test_history_navigation_updates_cursor() {
        let task_id = TaskId::new();
        let mut console = InteractiveConsole::new(task_id);

        // Add commands to history
        console.history.push("ls".to_string());
        console.history.push("pwd".to_string());

        // Press up arrow
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Up,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "pwd");
        assert_eq!(console.cursor_pos(), 3); // Cursor at end

        // Press up again
        console
            .process_event(InputEvent::key(KeyEvent::pressed(
                KeyCode::Up,
                Modifiers::none(),
            )))
            .unwrap();

        assert_eq!(console.text_buffer(), "ls");
        assert_eq!(console.cursor_pos(), 2); // Cursor at end
    }
}
