//! Platform adapter traits for workspace manager
//!
//! This module defines the platform abstraction layer that allows the workspace
//! manager to run in different environments (simulation, bare-metal kernel, etc.)
//! without being tightly coupled to any specific implementation.
//!
//! ## Philosophy
//!
//! - **Explicit, not implicit**: All platform interactions go through traits
//! - **Minimal surface**: Only abstract what's necessary for platform portability
//! - **Deterministic**: Implementations must be deterministic and testable
//! - **Capability-based**: Use existing capability types where applicable

pub mod fake;

pub use fake::FakePlatform;

use input_types::KeyEvent;
use view_types::ViewFrame;

/// Platform abstraction trait combining all workspace platform requirements
///
/// Implementations provide the environment-specific capabilities needed by the
/// workspace runtime. This includes display output, input event handling, time
/// stepping, and initial capabilities.
pub trait WorkspacePlatform {
    /// Get the display interface for rendering workspace content
    fn display(&mut self) -> &mut dyn WorkspaceDisplay;
    
    /// Get the input interface for handling keyboard events
    fn input(&mut self) -> &mut dyn WorkspaceInput;
    
    /// Get the tick interface for time stepping
    fn tick(&mut self) -> &mut dyn WorkspaceTick;
}

/// Display interface for rendering workspace content
///
/// Implementations handle the platform-specific rendering of ViewFrames and
/// status information to the user's display device.
pub trait WorkspaceDisplay {
    /// Render the main content view
    ///
    /// This is called when the focused component has a main view to display.
    /// The implementation should render the ViewFrame content to the appropriate
    /// region of the display.
    fn render_main_view(&mut self, frame: &ViewFrame);
    
    /// Render the status view
    ///
    /// This is called when the focused component has a status line to display.
    /// Typically rendered in a fixed region (e.g., bottom of screen).
    fn render_status_view(&mut self, frame: &ViewFrame);
    
    /// Render the workspace status strip
    ///
    /// Shows workspace-level status information (mode, file count, etc.)
    fn render_status_strip(&mut self, content: &str);
    
    /// Render context breadcrumbs
    ///
    /// Shows the current navigation context (directory, file, etc.)
    fn render_breadcrumbs(&mut self, content: &str);
    
    /// Clear the display
    ///
    /// Called when the workspace needs to clear the display before rendering
    /// new content.
    fn clear(&mut self);
    
    /// Present the rendered content to the display
    ///
    /// Called after all render operations to flush buffered content to the
    /// actual display device. Allows for double-buffering implementations.
    fn present(&mut self);
}

/// Input interface for handling keyboard events
///
/// Implementations provide keyboard events to the workspace runtime.
pub trait WorkspaceInput {
    /// Poll for the next input event
    ///
    /// Returns `Some(event)` if an input event is available, or `None` if no
    /// events are pending. This is a non-blocking operation.
    fn poll_event(&mut self) -> Option<KeyEvent>;
    
    /// Check if input is available without consuming it
    ///
    /// Returns `true` if `poll_event()` would return `Some`.
    fn has_pending(&self) -> bool;
}

/// Tick interface for explicit time stepping
///
/// The workspace runtime is explicitly driven by ticks rather than using
/// timers or sleeps. This makes the system deterministic and testable.
pub trait WorkspaceTick {
    /// Advance the logical time by one tick
    ///
    /// Returns the current tick number after incrementing.
    fn advance(&mut self) -> u64;
    
    /// Get the current tick number
    fn current(&self) -> u64;
}

/// Initial capabilities provided to the workspace
///
/// These capabilities are provided by the platform at initialization time
/// and allow the workspace to interact with system resources.
#[derive(Clone)]
pub struct WorkspaceCaps {
    /// Storage capability for file operations (optional)
    pub storage: Option<crate::EditorIoContext>,
    
    /// File system view capability (optional)
    pub fs_view: Option<()>, // Placeholder for now
    
    /// Settings registry capability (optional)
    pub settings: Option<()>, // Placeholder for now
}

impl WorkspaceCaps {
    /// Creates an empty set of capabilities
    pub fn empty() -> Self {
        Self {
            storage: None,
            fs_view: None,
            settings: None,
        }
    }
    
    /// Creates capabilities with storage
    pub fn with_storage(storage: crate::EditorIoContext) -> Self {
        Self {
            storage: Some(storage),
            fs_view: None,
            settings: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_workspace_caps_empty() {
        let caps = WorkspaceCaps::empty();
        assert!(caps.storage.is_none());
        assert!(caps.fs_view.is_none());
        assert!(caps.settings.is_none());
    }
    
    #[test]
    fn test_workspace_caps_with_storage() {
        use services_storage::JournaledStorage;
        let storage = JournaledStorage::new();
        let io_context = crate::EditorIoContext::new(storage);
        let caps = WorkspaceCaps::with_storage(io_context);
        assert!(caps.storage.is_some());
    }
}
