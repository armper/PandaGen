//! Fake platform implementation for testing
//!
//! This module provides a simple, deterministic platform implementation
//! for unit testing the workspace runtime without requiring real I/O.

use super::{WorkspaceDisplay, WorkspaceInput, WorkspacePlatform, WorkspaceTick};
use input_types::KeyEvent;
use view_types::ViewFrame;

/// Fake platform for testing
///
/// Provides in-memory buffers for display output and pre-programmed input
/// events. All operations are deterministic and suitable for unit tests.
pub struct FakePlatform {
    display: FakeDisplay,
    input: FakeInput,
    tick: FakeTick,
}

impl FakePlatform {
    /// Creates a new fake platform with empty state
    pub fn new() -> Self {
        Self {
            display: FakeDisplay::new(),
            input: FakeInput::new(),
            tick: FakeTick::new(),
        }
    }

    /// Queues an input event to be delivered via poll_event
    pub fn queue_input(&mut self, event: KeyEvent) {
        self.input.queue(event);
    }

    /// Gets the contents of the rendered main view
    pub fn get_main_view(&self) -> Option<&str> {
        self.display.main_view.as_deref()
    }

    /// Gets the contents of the rendered status strip
    pub fn get_status_strip(&self) -> Option<&str> {
        self.display.status_strip.as_deref()
    }

    /// Gets the contents of the rendered breadcrumbs
    pub fn get_breadcrumbs(&self) -> Option<&str> {
        self.display.breadcrumbs.as_deref()
    }

    /// Checks if the display was cleared
    pub fn was_cleared(&self) -> bool {
        self.display.cleared
    }

    /// Checks if present() was called
    pub fn was_presented(&self) -> bool {
        self.display.presented
    }

    /// Resets the presentation flag
    pub fn reset_presented(&mut self) {
        self.display.presented = false;
    }
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspacePlatform for FakePlatform {
    fn display(&mut self) -> &mut dyn WorkspaceDisplay {
        &mut self.display
    }

    fn input(&mut self) -> &mut dyn WorkspaceInput {
        &mut self.input
    }

    fn tick(&mut self) -> &mut dyn WorkspaceTick {
        &mut self.tick
    }
}

/// Fake display implementation
struct FakeDisplay {
    main_view: Option<String>,
    status_view: Option<String>,
    status_strip: Option<String>,
    breadcrumbs: Option<String>,
    cleared: bool,
    presented: bool,
}

impl FakeDisplay {
    fn new() -> Self {
        Self {
            main_view: None,
            status_view: None,
            status_strip: None,
            breadcrumbs: None,
            cleared: false,
            presented: false,
        }
    }
}

impl WorkspaceDisplay for FakeDisplay {
    fn render_main_view(&mut self, frame: &ViewFrame) {
        // Store a simplified representation of the frame
        self.main_view = Some(format!("ViewFrame[{}:{}]", frame.view_id, frame.revision));
    }

    fn render_status_view(&mut self, frame: &ViewFrame) {
        self.status_view = Some(format!("StatusView[{}:{}]", frame.view_id, frame.revision));
    }

    fn render_status_strip(&mut self, content: &str) {
        self.status_strip = Some(content.to_string());
    }

    fn render_breadcrumbs(&mut self, content: &str) {
        self.breadcrumbs = Some(content.to_string());
    }

    fn clear(&mut self) {
        self.cleared = true;
    }

    fn present(&mut self) {
        self.presented = true;
    }
}

/// Fake input implementation
struct FakeInput {
    events: Vec<KeyEvent>,
}

impl FakeInput {
    fn new() -> Self {
        Self { events: Vec::new() }
    }

    fn queue(&mut self, event: KeyEvent) {
        self.events.push(event);
    }
}

impl WorkspaceInput for FakeInput {
    fn poll_event(&mut self) -> Option<KeyEvent> {
        if self.events.is_empty() {
            None
        } else {
            Some(self.events.remove(0))
        }
    }

    fn has_pending(&self) -> bool {
        !self.events.is_empty()
    }
}

/// Fake tick implementation
struct FakeTick {
    current: u64,
}

impl FakeTick {
    fn new() -> Self {
        Self { current: 0 }
    }
}

impl WorkspaceTick for FakeTick {
    fn advance(&mut self) -> u64 {
        self.current += 1;
        self.current
    }

    fn current(&self) -> u64 {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{KeyCode, Modifiers};

    #[test]
    fn test_fake_platform_creation() {
        let platform = FakePlatform::new();
        assert!(!platform.was_cleared());
        assert!(!platform.was_presented());
    }

    #[test]
    fn test_fake_input_queue() {
        let mut platform = FakePlatform::new();
        assert!(!platform.input().has_pending());

        let event = KeyEvent::pressed(KeyCode::A, Modifiers::none());
        platform.queue_input(event.clone());

        assert!(platform.input().has_pending());
        let polled = platform.input().poll_event();
        assert_eq!(polled, Some(event));
        assert!(!platform.input().has_pending());
    }

    #[test]
    fn test_fake_display_operations() {
        let mut platform = FakePlatform::new();

        platform.display().clear();
        assert!(platform.was_cleared());

        platform.display().render_status_strip("test status");
        assert_eq!(platform.get_status_strip(), Some("test status"));

        platform.display().render_breadcrumbs("/home/user");
        assert_eq!(platform.get_breadcrumbs(), Some("/home/user"));

        platform.display().present();
        assert!(platform.was_presented());
    }

    #[test]
    fn test_fake_tick_advances() {
        let mut platform = FakePlatform::new();
        assert_eq!(platform.tick().current(), 0);

        let tick1 = platform.tick().advance();
        assert_eq!(tick1, 1);
        assert_eq!(platform.tick().current(), 1);

        let tick2 = platform.tick().advance();
        assert_eq!(tick2, 2);
    }
}
