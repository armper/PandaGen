//! Combined View Manager for CLI and Editor
//!
//! This module provides a unified view that integrates both the CLI console
//! and the vi editor into a single framebuffer display with proper separation.

use crate::{ConsoleFb, ScrollbackBuffer};
use alloc::format;
use hal::Framebuffer;
use services_editor_vi::render::EditorView;
use services_editor_vi::state::EditorState;

/// Combined view mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// CLI console is active
    Cli,
    /// Editor is active
    Editor,
}

/// Combined view manager
///
/// Manages display of both CLI console and editor in the same framebuffer.
/// Provides clean separation and focus transitions.
pub struct CombinedView<F: Framebuffer> {
    console: ConsoleFb<F>,
    editor_view: EditorView,
    mode: ViewMode,
    /// Lines reserved for status and prompt at bottom
    reserved_lines: usize,
}

impl<F: Framebuffer> CombinedView<F> {
    /// Create a new combined view
    ///
    /// # Arguments
    /// * `console` - Framebuffer console
    /// * `reserved_lines` - Lines reserved for status/prompt (default: 2)
    pub fn new(console: ConsoleFb<F>, reserved_lines: usize) -> Self {
        let editor_lines = console.rows().saturating_sub(reserved_lines);
        let editor_view = EditorView::new(editor_lines);

        Self {
            console,
            editor_view,
            mode: ViewMode::Cli,
            reserved_lines,
        }
    }

    /// Get current view mode
    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// Switch to CLI mode
    pub fn switch_to_cli(&mut self) {
        self.mode = ViewMode::Cli;
    }

    /// Switch to editor mode
    pub fn switch_to_editor(&mut self) {
        self.mode = ViewMode::Editor;
    }

    /// Get the framebuffer console
    pub fn console(&self) -> &ConsoleFb<F> {
        &self.console
    }

    /// Get mutable framebuffer console
    pub fn console_mut(&mut self) -> &mut ConsoleFb<F> {
        &mut self.console
    }

    /// Get the editor view
    pub fn editor_view(&self) -> &EditorView {
        &self.editor_view
    }

    /// Render CLI mode
    ///
    /// Shows scrollback output and a prompt line at the bottom.
    pub fn render_cli(&mut self, prompt: &str, input: &str, cursor_col: usize) {
        self.console.clear();

        // Collect visible lines first to avoid borrow checker issues
        let visible_lines: alloc::vec::Vec<alloc::string::String> =
            if let Some(scrollback) = self.console.scrollback() {
                scrollback
                    .visible_lines()
                    .iter()
                    .map(|line| alloc::string::String::from(line.as_str()))
                    .collect()
            } else {
                alloc::vec::Vec::new()
            };

        // Render scrollback in main area
        let output_lines = self.console.rows() - self.reserved_lines;
        for (row, line) in visible_lines.iter().enumerate() {
            if row >= output_lines {
                break;
            }
            self.console.draw_text_at(0, row, line);
        }

        // Render prompt and input line at bottom
        let prompt_row = self.console.rows() - 1;
        let prompt_with_input = format!("{}{}", prompt, input);
        self.console.draw_text_at(0, prompt_row, &prompt_with_input);

        // Draw cursor
        let cursor_col_abs = prompt.len() + cursor_col;
        self.console.draw_cursor(cursor_col_abs, prompt_row);
    }

    /// Render editor mode
    ///
    /// Shows editor content in main area and status line at bottom.
    pub fn render_editor(&mut self, editor_state: &EditorState) {
        self.console.clear();

        // Render editor content
        let content = self.editor_view.render(editor_state);
        let mut row = 0;

        for (idx, line) in content.lines().enumerate() {
            if idx >= self.console.rows() - self.reserved_lines {
                // Save last line for status
                break;
            }
            self.console.draw_text_at(0, row, line);
            row += 1;
        }

        // Render status line at bottom
        let status_row = self.console.rows() - 1;
        let status = self.editor_view.render_status(editor_state);
        self.console.draw_text_at(0, status_row, &status);

        // Draw editor cursor
        let cursor_pos = editor_state.cursor().position();
        if cursor_pos.row < self.console.rows() - self.reserved_lines {
            self.console.draw_cursor(cursor_pos.col, cursor_pos.row);
        }
    }

    /// Render based on current mode
    ///
    /// This is the main render function that should be called each frame.
    pub fn render(
        &mut self,
        cli_prompt: &str,
        cli_input: &str,
        cli_cursor: usize,
        editor_state: Option<&EditorState>,
    ) {
        match self.mode {
            ViewMode::Cli => {
                self.render_cli(cli_prompt, cli_input, cli_cursor);
            }
            ViewMode::Editor => {
                if let Some(state) = editor_state {
                    self.render_editor(state);
                } else {
                    // No editor state, fall back to CLI
                    self.render_cli(cli_prompt, cli_input, cli_cursor);
                }
            }
        }
    }

    /// Get number of lines available for content (excluding reserved lines)
    pub fn content_lines(&self) -> usize {
        self.console.rows().saturating_sub(self.reserved_lines)
    }

    /// Get number of columns
    pub fn cols(&self) -> usize {
        self.console.cols()
    }

    /// Get number of rows
    pub fn rows(&self) -> usize {
        self.console.rows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hal::{FramebufferInfo, PixelFormat};

    struct MockFramebuffer {
        info: FramebufferInfo,
        buffer: Vec<u8>,
    }

    impl MockFramebuffer {
        fn new(width: usize, height: usize) -> Self {
            let info = FramebufferInfo {
                width,
                height,
                stride_pixels: width,
                format: PixelFormat::Rgb32,
            };
            let buffer = vec![0; info.buffer_size()];
            Self { info, buffer }
        }
    }

    impl Framebuffer for MockFramebuffer {
        fn info(&self) -> FramebufferInfo {
            self.info
        }

        fn buffer_mut(&mut self) -> &mut [u8] {
            &mut self.buffer
        }
    }

    #[test]
    fn test_combined_view_creation() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let view = CombinedView::new(console, 2);

        assert_eq!(view.mode(), ViewMode::Cli);
    }

    #[test]
    fn test_mode_switching() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        assert_eq!(view.mode(), ViewMode::Cli);

        view.switch_to_editor();
        assert_eq!(view.mode(), ViewMode::Editor);

        view.switch_to_cli();
        assert_eq!(view.mode(), ViewMode::Cli);
    }

    #[test]
    fn test_content_lines() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let total_rows = console.rows();
        let view = CombinedView::new(console, 2);

        assert_eq!(view.content_lines(), total_rows - 2);
    }

    #[test]
    fn test_render_cli() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        // Should not crash
        view.render_cli("> ", "ls", 2);
    }

    #[test]
    fn test_render_editor() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        let editor_state = EditorState::new();

        // Should not crash
        view.render_editor(&editor_state);
    }

    #[test]
    fn test_render_with_mode() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        let editor_state = EditorState::new();

        // CLI mode
        view.render("> ", "test", 4, Some(&editor_state));
        assert_eq!(view.mode(), ViewMode::Cli);

        // Switch to editor mode
        view.switch_to_editor();
        view.render("> ", "test", 4, Some(&editor_state));
        assert_eq!(view.mode(), ViewMode::Editor);
    }

    #[test]
    fn test_render_editor_without_state() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        view.switch_to_editor();

        // Should fall back to CLI when no editor state
        view.render("> ", "fallback", 8, None);

        // Should not crash
    }
}
