//! Combined View Manager for CLI and Editor
//!
//! This module provides a unified view that integrates both the CLI console
//! and the vi editor into a single framebuffer display with proper separation.

use crate::ConsoleFb;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hal::Framebuffer;
use services_editor_vi::render::EditorView;
use services_editor_vi::state::EditorState;

#[cfg(any(debug_assertions, feature = "perf_debug"))]
use crate::RenderPerfStats;

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
    editor_cache: EditorRenderCache,
    perf_overlay_enabled: bool,
}

#[derive(Debug, Clone, Default)]
struct EditorRenderCache {
    lines: Vec<String>,
    status: String,
    cursor: Option<(usize, usize)>,
    cols: usize,
    rows: usize,
    valid: bool,
}

#[derive(Debug, Clone)]
struct LineUpdate {
    row: usize,
    start_col: usize,
    text: String,
    clear_from: Option<usize>,
}

#[derive(Debug, Clone, Default)]
struct EditorRenderPlan {
    line_updates: Vec<LineUpdate>,
    status_update: Option<String>,
    cursor_from: Option<(usize, usize)>,
    cursor_to: Option<(usize, usize)>,
    full_redraw: bool,
}

impl EditorRenderCache {
    fn invalidate(&mut self) {
        self.valid = false;
        self.lines.clear();
        self.status.clear();
        self.cursor = None;
    }
}

fn build_editor_lines(state: &EditorState, rows: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        if let Some(line) = state.buffer().line(row) {
            lines.push(line.to_string());
        } else {
            lines.push("~".to_string());
        }
    }
    lines
}

fn diff_line_update(old: &str, new: &str, cols: usize, row: usize) -> Option<LineUpdate> {
    if old == new {
        return None;
    }

    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();
    let max_len = old_bytes.len().max(new_bytes.len()).min(cols);

    let mut start = None;
    for idx in 0..max_len {
        let old_ch = old_bytes.get(idx).copied().unwrap_or(b' ');
        let new_ch = new_bytes.get(idx).copied().unwrap_or(b' ');
        if old_ch != new_ch {
            start = Some(idx);
            break;
        }
    }

    let start = start?;
    let mut end = start + 1;
    for idx in (start..max_len).rev() {
        let old_ch = old_bytes.get(idx).copied().unwrap_or(b' ');
        let new_ch = new_bytes.get(idx).copied().unwrap_or(b' ');
        if old_ch != new_ch {
            end = idx + 1;
            break;
        }
    }

    let new_end = end.min(new_bytes.len());
    let text = if start < new_end {
        new[start..new_end].to_string()
    } else {
        String::new()
    };

    let clear_from = if new_bytes.len() < old_bytes.len() {
        Some(new_bytes.len().min(cols))
    } else {
        None
    };

    Some(LineUpdate {
        row,
        start_col: start,
        text,
        clear_from,
    })
}

fn cursor_cell_byte(line: &str, col: usize) -> u8 {
    line.as_bytes().get(col).copied().unwrap_or(b' ')
}

#[cfg(any(debug_assertions, feature = "perf_debug"))]
fn perf_overlay_text(stats: &RenderPerfStats) -> String {
    let render_ticks = stats.last_frame_ticks.unwrap_or(0);
    format!(
        " | t:{} draws:{} flush:{} dirty:{}:{}, px:{}",
        render_ticks,
        stats.glyph_draws,
        stats.flushes,
        stats.dirty_lines,
        stats.dirty_spans,
        stats.pixel_writes
    )
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
            editor_cache: EditorRenderCache::default(),
            perf_overlay_enabled: false,
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

    /// Enable or disable the performance overlay (gated by perf stats)
    pub fn set_perf_overlay_enabled(&mut self, enabled: bool) {
        self.perf_overlay_enabled = enabled;
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
        self.render_editor_with_ticks(editor_state, None, None);
    }

    /// Render editor mode with optional frame timestamps (ticks)
    pub fn render_editor_with_ticks(
        &mut self,
        editor_state: &EditorState,
        frame_start_ticks: Option<u64>,
        frame_end_ticks: Option<u64>,
    ) {
        let cols = self.console.cols();
        let rows = self.console.rows();
        let content_rows = rows.saturating_sub(self.reserved_lines);

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            self.console.perf_reset_frame();
            if let Some(start) = frame_start_ticks {
                self.console.perf_frame_start(start);
            }
        }

        if !self.editor_cache.valid
            || self.editor_cache.cols != cols
            || self.editor_cache.rows != rows
        {
            self.console.clear();

            let lines = build_editor_lines(editor_state, content_rows);
            for (row, line) in lines.iter().enumerate() {
                self.console.draw_text_at(0, row, line);
            }

            let status_row = rows.saturating_sub(1);
            let mut status = self.editor_view.render_status(editor_state);
            if self.perf_overlay_enabled {
                #[cfg(any(debug_assertions, feature = "perf_debug"))]
                {
                    status.push_str(&perf_overlay_text(self.console.perf_stats()));
                }
                #[cfg(not(any(debug_assertions, feature = "perf_debug")))]
                {
                    status.push_str(" | perf");
                }
            }
            self.console.draw_text_at(0, status_row, &status);

            let cursor_pos = editor_state.cursor().position();
            if cursor_pos.row < content_rows {
                self.console.draw_cursor(cursor_pos.col, cursor_pos.row);
                self.editor_cache.cursor = Some((cursor_pos.col, cursor_pos.row));
            } else {
                self.editor_cache.cursor = None;
            }

            self.editor_cache.lines = lines;
            self.editor_cache.status = status;
            self.editor_cache.cols = cols;
            self.editor_cache.rows = rows;
            self.editor_cache.valid = true;

            #[cfg(any(debug_assertions, feature = "perf_debug"))]
            {
                if let Some(end) = frame_end_ticks {
                    self.console.perf_frame_end(end);
                }
            }

            return;
        }

        let new_lines = build_editor_lines(editor_state, content_rows);
        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            self.console.perf_stats_mut().allocations += new_lines.len();
        }

        let mut plan = EditorRenderPlan::default();
        for row in 0..content_rows {
            let old_line = self.editor_cache.lines.get(row).map(|s| s.as_str()).unwrap_or("");
            let new_line = new_lines.get(row).map(|s| s.as_str()).unwrap_or("~");
            if let Some(update) = diff_line_update(old_line, new_line, cols, row) {
                plan.line_updates.push(update);
            }
        }

        let mut status = self.editor_view.render_status(editor_state);
        if self.perf_overlay_enabled {
            #[cfg(any(debug_assertions, feature = "perf_debug"))]
            {
                status.push_str(&perf_overlay_text(self.console.perf_stats()));
            }
            #[cfg(not(any(debug_assertions, feature = "perf_debug")))]
            {
                status.push_str(" | perf");
            }
        }
        if self.perf_overlay_enabled || status != self.editor_cache.status {
            plan.status_update = Some(status.clone());
        }

        let cursor_pos = editor_state.cursor().position();
        let cursor_to = if cursor_pos.row < content_rows {
            Some((cursor_pos.col, cursor_pos.row))
        } else {
            None
        };
        if self.editor_cache.cursor != cursor_to {
            plan.cursor_from = self.editor_cache.cursor;
            plan.cursor_to = cursor_to;
        }

        if plan.full_redraw {
            self.editor_cache.invalidate();
            self.render_editor_with_ticks(editor_state, frame_start_ticks, frame_end_ticks);
            return;
        }

        // Apply line updates
        for update in &plan.line_updates {
            if !update.text.is_empty() {
                self.console
                    .draw_text_at(update.start_col, update.row, &update.text);
            }
            if let Some(clear_from) = update.clear_from {
                let mut col = clear_from.min(cols);
                while col < cols {
                    self.console.draw_char_at(col, update.row, b' ');
                    col += 1;
                }
            }
        }

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            let stats = self.console.perf_stats_mut();
            stats.dirty_lines += plan.line_updates.len();
            stats.dirty_spans += plan.line_updates.len();
        }

        // Redraw status line if changed
        if let Some(status_line) = &plan.status_update {
            let status_row = rows.saturating_sub(1);
            self.console.draw_text_at(0, status_row, status_line);
            #[cfg(any(debug_assertions, feature = "perf_debug"))]
            {
                self.console.perf_stats_mut().status_redraws += 1;
            }
        }

        // Clear old cursor by redrawing the underlying cell
        if let Some((old_col, old_row)) = plan.cursor_from {
            if old_row < content_rows {
                let line = new_lines.get(old_row).map(|s| s.as_str()).unwrap_or("~");
                let cell = cursor_cell_byte(line, old_col);
                self.console.draw_char_at(old_col, old_row, cell);
            }
        }

        // Draw new cursor
        if let Some((new_col, new_row)) = plan.cursor_to {
            if new_row < content_rows {
                self.console.draw_cursor(new_col, new_row);
            }
        }

        // Update cache
        self.editor_cache.lines = new_lines;
        self.editor_cache.status = status;
        self.editor_cache.cursor = cursor_to;
        self.editor_cache.cols = cols;
        self.editor_cache.rows = rows;
        self.editor_cache.valid = true;

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            if let Some(end) = frame_end_ticks {
                self.console.perf_frame_end(end);
            }
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
    use services_editor_vi::state::Position;

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

    #[test]
    fn test_incremental_render_single_char_update() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        let mut state = EditorState::new();
        state.load_content("hello".to_string());

        view.render_editor(&state);

        state
            .buffer_mut()
            .insert_char(Position::new(0, 5), 'x');
        state
            .cursor_mut()
            .set_position(Position::new(0, 6));

        view.render_editor(&state);

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            let stats = view.console().perf_stats();
            assert_eq!(stats.clear_calls, 0);
            assert!(stats.dirty_lines <= 1);
            assert!(stats.dirty_spans <= 1);
        }
    }

    #[test]
    fn test_incremental_render_cursor_move() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        let mut view = CombinedView::new(console, 2);

        let mut state = EditorState::new();
        state.load_content("hello".to_string());
        state.cursor_mut().set_position(Position::new(0, 0));
        view.render_editor(&state);

        state.cursor_mut().set_position(Position::new(0, 1));
        view.render_editor(&state);

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            let stats = view.console().perf_stats();
            assert_eq!(stats.clear_calls, 0);
        }
    }
}
