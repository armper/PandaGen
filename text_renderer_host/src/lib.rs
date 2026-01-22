//! # Text Renderer Host
//!
//! This crate provides a text-based renderer host for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Rendering is a host concern**, not a component concern
//! - **Components never print** - they publish views
//! - **Views are rendered, not streamed** - immutable frames
//! - **Renderer is dumb and replaceable** - no business logic
//! - **Renderer is NOT a terminal** - no ANSI, no cursor addressing, no terminal state
//!
//! ## Responsibilities
//!
//! The text renderer host:
//! - Subscribes to workspace view snapshots
//! - Renders focused component's views
//! - Renders status line consistently
//! - Redraws on revision change
//! - Stops rendering when cancelled (budget exhaustion)
//!
//! ## Non-Responsibilities
//!
//! The text renderer host does NOT:
//! - Emit ANSI escape codes
//! - Maintain terminal state
//! - Implement cursor addressing
//! - Mix rendering with workspace logic
//! - Generate input events
//!
//! This is presentation, not authority.
//!
//! ## Performance Debugging
//!
//! Enable the `perf_debug` feature flag to get detailed performance metrics:
//! ```bash
//! cargo build --features perf_debug
//! ```

use view_types::{CursorPosition, ViewContent, ViewFrame};
use std::collections::HashMap;

#[cfg(feature = "perf_debug")]
use std::time::Instant;

/// Default separator width for status line
/// This could be made configurable in the future based on terminal width
const SEPARATOR_WIDTH: usize = 80;

/// Cache of rendered content for incremental updates
#[derive(Debug, Clone)]
struct ViewCache {
    /// Cached lines (line index -> rendered text)
    lines: HashMap<usize, String>,
    /// Last cursor position rendered
    last_cursor: Option<CursorPosition>,
}

impl ViewCache {
    fn new() -> Self {
        Self {
            lines: HashMap::new(),
            last_cursor: None,
        }
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.last_cursor = None;
    }

    fn get_line(&self, line_idx: usize) -> Option<&str> {
        self.lines.get(&line_idx).map(|s| s.as_str())
    }

    fn set_line(&mut self, line_idx: usize, content: String) {
        self.lines.insert(line_idx, content);
    }
}

/// Rendering statistics for performance monitoring
#[derive(Debug, Default, Clone)]
pub struct RenderStats {
    /// Number of characters written in the last frame
    pub chars_written_per_frame: usize,
    /// Number of lines redrawn in the last frame
    pub lines_redrawn_per_frame: usize,
    
    #[cfg(feature = "perf_debug")]
    /// Total number of glyph draw calls
    pub glyph_draws: usize,
    #[cfg(feature = "perf_debug")]
    /// Total number of clear operations (viewport or line clears)
    pub clear_operations: usize,
    #[cfg(feature = "perf_debug")]
    /// Total number of flush/blit operations
    pub flush_operations: usize,
    #[cfg(feature = "perf_debug")]
    /// Number of status line redraws
    pub status_line_redraws: usize,
    #[cfg(feature = "perf_debug")]
    /// Frame render time in microseconds
    pub frame_time_us: u64,
}

impl RenderStats {
    /// Reset all stats to zero (called at frame begin)
    fn reset(&mut self) {
        self.chars_written_per_frame = 0;
        self.lines_redrawn_per_frame = 0;
        
        #[cfg(feature = "perf_debug")]
        {
            self.glyph_draws = 0;
            self.clear_operations = 0;
            self.flush_operations = 0;
            self.status_line_redraws = 0;
            self.frame_time_us = 0;
        }
    }
    
    #[cfg(feature = "perf_debug")]
    /// Record a glyph draw operation
    fn record_glyph_draw(&mut self, count: usize) {
        self.glyph_draws += count;
    }
    
    #[cfg(feature = "perf_debug")]
    /// Record a clear operation
    fn record_clear(&mut self) {
        self.clear_operations += 1;
    }
    
    #[cfg(feature = "perf_debug")]
    /// Record a flush/blit operation
    fn record_flush(&mut self) {
        self.flush_operations += 1;
    }
    
    #[cfg(feature = "perf_debug")]
    /// Record a status line redraw
    fn record_status_redraw(&mut self) {
        self.status_line_redraws += 1;
    }
}

/// Performance overlay that can be displayed on screen (when perf_debug enabled)
#[cfg(feature = "perf_debug")]
pub struct PerfOverlay {
    /// Whether the overlay is visible
    pub visible: bool,
}

#[cfg(feature = "perf_debug")]
impl PerfOverlay {
    pub fn new() -> Self {
        Self { visible: false }
    }
    
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
    
    pub fn render(&self, stats: &RenderStats) -> String {
        if !self.visible {
            return String::new();
        }
        
        format!(
            "╔════ PERF ════╗\n\
             ║ Frame: {:>4}µs║\n\
             ║ Chars: {:>6}║\n\
             ║ Lines: {:>6}║\n\
             ║ Glyphs:{:>6}║\n\
             ║ Clears:{:>6}║\n\
             ║ Flush: {:>6}║\n\
             ║ Status:{:>6}║\n\
             ╚══════════════╝",
            stats.frame_time_us,
            stats.chars_written_per_frame,
            stats.lines_redrawn_per_frame,
            stats.glyph_draws,
            stats.clear_operations,
            stats.flush_operations,
            stats.status_line_redraws,
        )
    }
}

/// Text renderer that converts ViewFrames to text output
pub struct TextRenderer {
    /// Last rendered revision (to detect changes)
    last_main_revision: Option<u64>,
    last_status_revision: Option<u64>,
    /// Cache of rendered view content
    view_cache: ViewCache,
    /// Rendering statistics
    stats: RenderStats,
    
    #[cfg(feature = "perf_debug")]
    /// Performance overlay
    perf_overlay: PerfOverlay,
    
    #[cfg(feature = "perf_debug")]
    /// Frame start time for timing measurements
    frame_start: Option<Instant>,
}

impl TextRenderer {
    /// Creates a new text renderer
    pub fn new() -> Self {
        Self {
            last_main_revision: None,
            last_status_revision: None,
            view_cache: ViewCache::new(),
            stats: RenderStats::default(),
            
            #[cfg(feature = "perf_debug")]
            perf_overlay: PerfOverlay::new(),
            
            #[cfg(feature = "perf_debug")]
            frame_start: None,
        }
    }
    
    #[cfg(feature = "perf_debug")]
    /// Toggle performance overlay visibility
    pub fn toggle_perf_overlay(&mut self) {
        self.perf_overlay.toggle();
    }
    
    #[cfg(feature = "perf_debug")]
    /// Begin frame timing
    fn frame_begin(&mut self) {
        self.frame_start = Some(Instant::now());
    }
    
    #[cfg(feature = "perf_debug")]
    /// End frame timing and record duration
    fn frame_end(&mut self) {
        if let Some(start) = self.frame_start {
            self.stats.frame_time_us = start.elapsed().as_micros() as u64;
            self.frame_start = None;
        }
    }


    /// Get the latest rendering statistics
    pub fn stats(&self) -> &RenderStats {
        &self.stats
    }

    /// Checks if a redraw is needed based on revision changes
    pub fn needs_redraw(
        &self,
        main_frame: Option<&ViewFrame>,
        status_frame: Option<&ViewFrame>,
    ) -> bool {
        let main_changed = main_frame.map(|f| f.revision) != self.last_main_revision;
        let status_changed = status_frame.map(|f| f.revision) != self.last_status_revision;
        main_changed || status_changed
    }

    /// Renders a workspace snapshot to text output
    ///
    /// Returns the rendered text as a String.
    /// Performs a full screen redraw if revision has changed.
    pub fn render_snapshot(
        &mut self,
        main_view: Option<&ViewFrame>,
        status_view: Option<&ViewFrame>,
    ) -> String {
        // Begin frame timing
        #[cfg(feature = "perf_debug")]
        self.frame_begin();
        
        // Reset stats for new frame
        self.stats.reset();

        let mut output = String::new();

        // Full redraw - just render content (no ANSI codes)
        // Render main view (if present)
        if let Some(frame) = main_view {
            output.push_str(&self.render_view_frame(frame));
            self.last_main_revision = Some(frame.revision);
        } else {
            output.push_str("(no view)\n");
            self.last_main_revision = None;
            self.view_cache.clear();
            
            #[cfg(feature = "perf_debug")]
            self.stats.record_clear();
        }

        // Separator line
        output.push('\n');
        output.push_str(&"─".repeat(SEPARATOR_WIDTH));
        output.push('\n');

        // Render status view (if present)
        if let Some(frame) = status_view {
            output.push_str(&self.render_status_line(frame));
            self.last_status_revision = Some(frame.revision);
            
            #[cfg(feature = "perf_debug")]
            self.stats.record_status_redraw();
        } else {
            output.push_str("(no status)\n");
            self.last_status_revision = None;
        }

        // Update stats
        self.stats.chars_written_per_frame = output.len();
        
        #[cfg(feature = "perf_debug")]
        {
            self.stats.record_flush();
            self.frame_end();
            
            // Append perf overlay if visible
            if self.perf_overlay.visible {
                output.push('\n');
                output.push_str(&self.perf_overlay.render(&self.stats));
            }
        }

        output
    }

    /// Renders only the changed lines of a text buffer (incremental rendering)
    ///
    /// Returns a description of changed lines (for debugging/testing).
    /// In a real system, this would write directly to a framebuffer.
    pub fn render_incremental(
        &mut self,
        main_view: Option<&ViewFrame>,
        status_view: Option<&ViewFrame>,
    ) -> String {
        // Begin frame timing
        #[cfg(feature = "perf_debug")]
        self.frame_begin();
        
        // Reset stats for new frame
        self.stats.reset();

        let mut output = String::new();

        // Incremental rendering for main view
        if let Some(frame) = main_view {
            let changes = self.render_view_incremental(frame);
            output.push_str(&changes);
            self.last_main_revision = Some(frame.revision);
        } else {
            // No view - clear cache
            if !self.view_cache.lines.is_empty() {
                output.push_str("(view cleared)\n");
                self.view_cache.clear();
                
                #[cfg(feature = "perf_debug")]
                self.stats.record_clear();
            }
            self.last_main_revision = None;
        }

        // For now, always render status (it's just one line)
        if let Some(frame) = status_view {
            if status_view.map(|f| f.revision) != self.last_status_revision {
                output.push_str(&format!("[STATUS] {}\n", self.render_status_line(frame).trim()));
                self.stats.chars_written_per_frame += self.render_status_line(frame).len();
                
                #[cfg(feature = "perf_debug")]
                self.stats.record_status_redraw();
            }
            self.last_status_revision = Some(frame.revision);
        } else {
            self.last_status_revision = None;
        }
        
        #[cfg(feature = "perf_debug")]
        {
            self.stats.record_flush();
            self.frame_end();
            
            // Append perf overlay if visible
            if self.perf_overlay.visible {
                output.push('\n');
                output.push_str(&self.perf_overlay.render(&self.stats));
            }
        }

        output
    }

    /// Renders a view frame incrementally, returning only changes
    fn render_view_incremental(&mut self, frame: &ViewFrame) -> String {
        match &frame.content {
            ViewContent::TextBuffer { lines } => {
                self.render_text_buffer_incremental(lines, frame.cursor.as_ref())
            }
            _ => {
                // For non-text buffers, fall back to full render
                self.render_view_frame(frame)
            }
        }
    }

    /// Renders text buffer incrementally by diffing against cache
    fn render_text_buffer_incremental(
        &mut self,
        lines: &[String],
        cursor: Option<&CursorPosition>,
    ) -> String {
        let mut output = String::new();
        let mut lines_changed = 0;
        let mut chars_written = 0;

        // Check each line against cache
        for (line_idx, line) in lines.iter().enumerate() {
            let cursor_on_line = cursor.map_or(false, |c| c.line == line_idx);

            if cursor_on_line {
                // Cursor is on this line - must render with cursor
                let col = cursor.unwrap().column;
                let rendered_line = self.render_line_with_cursor(line, col);
                
                let line_changed = match self.view_cache.get_line(line_idx) {
                    Some(cached) => cached != &rendered_line,
                    None => true,
                };

                if line_changed {
                    output.push_str(&format!("[L{}] {}\n", line_idx, rendered_line));
                    chars_written += rendered_line.len();
                    
                    #[cfg(feature = "perf_debug")]
                    self.stats.record_glyph_draw(rendered_line.chars().count());
                    
                    self.view_cache.set_line(line_idx, rendered_line);
                    lines_changed += 1;
                }
            } else {
                // Cursor NOT on this line - compare raw line directly
                let line_changed = match self.view_cache.get_line(line_idx) {
                    Some(cached) => cached != line,
                    None => true,
                };

                if line_changed {
                    output.push_str(&format!("[L{}] {}\n", line_idx, line));
                    chars_written += line.len();
                    
                    #[cfg(feature = "perf_debug")]
                    self.stats.record_glyph_draw(line.chars().count());
                    
                    self.view_cache.set_line(line_idx, line.clone());
                    lines_changed += 1;
                }
            }
        }

        // Handle cursor-only changes (cursor moved but content didn't change)
        let cursor_moved = cursor != self.view_cache.last_cursor.as_ref();
        if cursor_moved && lines_changed == 0 {
            if let Some(cursor_pos) = cursor {
                output.push_str(&format!("[CURSOR] {}:{}\n", cursor_pos.line, cursor_pos.column));
                chars_written += 10; // Approximate cursor update cost
            }
        }

        self.view_cache.last_cursor = cursor.copied();
        self.stats.lines_redrawn_per_frame = lines_changed;
        self.stats.chars_written_per_frame = chars_written;

        if output.is_empty() {
            output.push_str("(no changes)\n");
        }

        output
    }

    /// Helper to render a line with cursor marker
    fn render_line_with_cursor(&self, line: &str, col: usize) -> String {
        // Find byte offset for the cursor column
        let byte_pos = line.char_indices().map(|(i, _)| i).nth(col);

        match byte_pos {
            Some(pos) => {
                // Cursor is inside the string
                let (before, after) = line.split_at(pos);
                format!("{}|{}", before, after)
            }
            None => {
                // Cursor is at the end or beyond
                let char_count = line.chars().count();
                if col == char_count {
                    format!("{}|", line)
                } else {
                    let padding = (col - char_count).min(1000);
                    format!("{}{}|", line, " ".repeat(padding))
                }
            }
        }
    }

    /// Renders a single view frame
    fn render_view_frame(&self, frame: &ViewFrame) -> String {
        match &frame.content {
            ViewContent::TextBuffer { lines } => {
                self.render_text_buffer(lines, frame.cursor.as_ref())
            }
            ViewContent::StatusLine { text } => format!("{}\n", text),
            ViewContent::Panel { metadata } => format!("[Panel: {}]\n", metadata),
        }
    }

    /// Renders a text buffer with optional cursor
    fn render_text_buffer(&self, lines: &[String], cursor: Option<&CursorPosition>) -> String {
        let mut output = String::new();

        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(cursor_pos) = cursor {
                if cursor_pos.line == line_idx {
                    // Insert cursor marker at the correct column
                    // Use character-based indexing to handle Unicode correctly
                    let col = cursor_pos.column;
                    let chars: Vec<char> = line.chars().collect();

                    if col <= chars.len() {
                        // Cursor within or at end of line
                        let before: String = chars.iter().take(col).collect();
                        let after: String = chars.iter().skip(col).collect();
                        output.push_str(&before);
                        output.push('|'); // Cursor marker
                        output.push_str(&after);
                        output.push('\n');
                    } else {
                        // Cursor beyond line end - limit padding to reasonable max (1000 chars)
                        let padding = (col.saturating_sub(chars.len())).min(1000);
                        output.push_str(line);
                        output.push_str(&" ".repeat(padding));
                        output.push('|');
                        output.push('\n');
                    }
                    continue;
                }
            }
            output.push_str(line);
            output.push('\n');
        }

        // If cursor is on a line beyond the buffer (limit to reasonable max of 1000 lines)
        if let Some(cursor_pos) = cursor {
            if cursor_pos.line >= lines.len() && cursor_pos.line < lines.len() + 1000 {
                for _ in lines.len()..cursor_pos.line {
                    output.push('\n');
                }
                let padding = cursor_pos.column.min(1000);
                output.push_str(&" ".repeat(padding));
                output.push_str("|\n");
            }
        }

        output
    }

    /// Renders a status line
    fn render_status_line(&self, frame: &ViewFrame) -> String {
        match &frame.content {
            ViewContent::StatusLine { text } => format!("{}\n", text),
            _ => "(invalid status view)\n".to_string(),
        }
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use view_types::{ViewId, ViewKind};

    fn create_text_buffer_frame(
        lines: Vec<String>,
        cursor: Option<CursorPosition>,
        revision: u64,
    ) -> ViewFrame {
        let mut frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            revision,
            ViewContent::text_buffer(lines),
            0,
        );
        if let Some(cursor_pos) = cursor {
            frame = frame.with_cursor(cursor_pos);
        }
        frame
    }

    fn create_status_frame(text: String, revision: u64) -> ViewFrame {
        ViewFrame::new(
            ViewId::new(),
            ViewKind::StatusLine,
            revision,
            ViewContent::status_line(text),
            0,
        )
    }

    #[test]
    fn test_render_empty_snapshot() {
        let mut renderer = TextRenderer::new();
        let output = renderer.render_snapshot(None, None);
        assert!(output.contains("(no view)"));
        assert!(output.contains("(no status)"));
    }

    #[test]
    fn test_render_text_buffer_without_cursor() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hello".to_string(), "World".to_string()];
        let frame = create_text_buffer_frame(lines, None, 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("Hello"));
        assert!(output.contains("World"));
    }

    #[test]
    fn test_render_text_buffer_with_cursor() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hello".to_string(), "World".to_string()];
        let cursor = CursorPosition::new(0, 2); // At 'l' in "Hello"
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("He|llo")); // Cursor marker at position 2
    }

    #[test]
    fn test_render_cursor_at_line_end() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hi".to_string()];
        let cursor = CursorPosition::new(0, 2); // At end of "Hi"
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("Hi|"));
    }

    #[test]
    fn test_render_cursor_beyond_line() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hi".to_string()];
        let cursor = CursorPosition::new(0, 5); // Beyond "Hi"
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("Hi   |")); // Spaces then cursor
    }

    #[test]
    fn test_render_cursor_on_empty_line() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["".to_string()];
        let cursor = CursorPosition::new(0, 0);
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("|"));
    }

    #[test]
    fn test_render_cursor_beyond_buffer() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Line 1".to_string()];
        let cursor = CursorPosition::new(5, 3); // Line 5, column 3
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        // Should have empty lines and cursor on line 5
        let output_lines: Vec<&str> = output.lines().collect();
        assert!(output_lines.len() > 5);
    }

    #[test]
    fn test_render_status_line() {
        let mut renderer = TextRenderer::new();
        let status = create_status_frame("-- INSERT --".to_string(), 1);
        let output = renderer.render_snapshot(None, Some(&status));
        assert!(output.contains("-- INSERT --"));
    }

    #[test]
    fn test_render_with_both_views() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Content".to_string()];
        let main = create_text_buffer_frame(lines, None, 1);
        let status = create_status_frame("Status".to_string(), 1);
        let output = renderer.render_snapshot(Some(&main), Some(&status));
        assert!(output.contains("Content"));
        assert!(output.contains("Status"));
        assert!(output.contains("─")); // Separator
    }

    #[test]
    fn test_needs_redraw_on_revision_change() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Test".to_string()];
        let frame1 = create_text_buffer_frame(lines.clone(), None, 1);

        // First render
        renderer.render_snapshot(Some(&frame1), None);

        // Same revision - no redraw needed
        assert!(!renderer.needs_redraw(Some(&frame1), None));

        // New revision - redraw needed
        let frame2 = create_text_buffer_frame(lines, None, 2);
        assert!(renderer.needs_redraw(Some(&frame2), None));
    }

    #[test]
    fn test_needs_redraw_on_status_change() {
        let mut renderer = TextRenderer::new();
        let status1 = create_status_frame("Status 1".to_string(), 1);

        // First render
        renderer.render_snapshot(None, Some(&status1));

        // Same revision - no redraw needed
        assert!(!renderer.needs_redraw(None, Some(&status1)));

        // New revision - redraw needed
        let status2 = create_status_frame("Status 2".to_string(), 2);
        assert!(renderer.needs_redraw(None, Some(&status2)));
    }

    #[test]
    fn test_revision_tracking() {
        let mut renderer = TextRenderer::new();
        assert_eq!(renderer.last_main_revision, None);
        assert_eq!(renderer.last_status_revision, None);

        let lines = vec!["Test".to_string()];
        let main = create_text_buffer_frame(lines, None, 5);
        let status = create_status_frame("Status".to_string(), 10);

        renderer.render_snapshot(Some(&main), Some(&status));
        assert_eq!(renderer.last_main_revision, Some(5));
        assert_eq!(renderer.last_status_revision, Some(10));
    }

    #[test]
    fn test_incremental_render_first_frame() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hello".to_string(), "World".to_string()];
        let frame = create_text_buffer_frame(lines, None, 1);
        
        let output = renderer.render_incremental(Some(&frame), None);
        
        // First render should update all lines
        assert!(output.contains("[L0] Hello"));
        assert!(output.contains("[L1] World"));
        assert_eq!(renderer.stats().lines_redrawn_per_frame, 2);
    }

    #[test]
    fn test_incremental_render_no_changes() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hello".to_string()];
        let frame1 = create_text_buffer_frame(lines.clone(), None, 1);
        
        // First render
        renderer.render_incremental(Some(&frame1), None);
        
        // Second render with same content but different revision
        let frame2 = create_text_buffer_frame(lines, None, 2);
        let output = renderer.render_incremental(Some(&frame2), None);
        
        // Should report no changes
        assert!(output.contains("(no changes)"));
        assert_eq!(renderer.stats().lines_redrawn_per_frame, 0);
    }

    #[test]
    fn test_incremental_render_line_change() {
        let mut renderer = TextRenderer::new();
        
        // First render
        let lines1 = vec!["Hello".to_string(), "World".to_string()];
        let frame1 = create_text_buffer_frame(lines1, None, 1);
        renderer.render_incremental(Some(&frame1), None);
        
        // Second render with one line changed
        let lines2 = vec!["Hello".to_string(), "Rust".to_string()];
        let frame2 = create_text_buffer_frame(lines2, None, 2);
        let output = renderer.render_incremental(Some(&frame2), None);
        
        // Should only update changed line
        assert!(!output.contains("[L0]")); // Line 0 unchanged
        assert!(output.contains("[L1] Rust")); // Line 1 changed
        assert_eq!(renderer.stats().lines_redrawn_per_frame, 1);
    }

    #[test]
    fn test_incremental_render_cursor_only_move() {
        let mut renderer = TextRenderer::new();
        
        // First render with cursor at 0,0
        let lines = vec!["Hello".to_string()];
        let frame1 = create_text_buffer_frame(lines.clone(), Some(CursorPosition::new(0, 0)), 1);
        renderer.render_incremental(Some(&frame1), None);
        
        // Second render with cursor moved to 0,2
        let frame2 = create_text_buffer_frame(lines, Some(CursorPosition::new(0, 2)), 2);
        let output = renderer.render_incremental(Some(&frame2), None);
        
        // Should detect cursor movement
        assert!(output.contains("[CURSOR]") || output.contains("[L0]"));
    }

    #[test]
    fn test_render_stats() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Test line".to_string()];
        let frame = create_text_buffer_frame(lines, None, 1);
        
        renderer.render_incremental(Some(&frame), None);
        
        let stats = renderer.stats();
        assert!(stats.chars_written_per_frame > 0);
        assert_eq!(stats.lines_redrawn_per_frame, 1);
    }
    
    #[test]
    #[cfg(feature = "perf_debug")]
    fn test_perf_debug_instrumentation() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Test line".to_string(), "Another line".to_string()];
        let frame = create_text_buffer_frame(lines, None, 1);
        
        renderer.render_incremental(Some(&frame), None);
        
        let stats = renderer.stats();
        // With perf_debug, glyph draws should be counted
        assert!(stats.glyph_draws > 0);
        assert_eq!(stats.flush_operations, 1);
        assert!(stats.frame_time_us > 0);
    }
    
    #[test]
    #[cfg(feature = "perf_debug")]
    fn test_perf_overlay_toggle() {
        let mut renderer = TextRenderer::new();
        
        // Initially hidden
        assert!(!renderer.perf_overlay.visible);
        
        // Toggle to show
        renderer.toggle_perf_overlay();
        assert!(renderer.perf_overlay.visible);
        
        // Toggle to hide
        renderer.toggle_perf_overlay();
        assert!(!renderer.perf_overlay.visible);
    }
    
    #[test]
    #[cfg(feature = "perf_debug")]
    fn test_perf_overlay_in_output() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Test".to_string()];
        let frame = create_text_buffer_frame(lines, None, 1);
        
        // Enable overlay
        renderer.toggle_perf_overlay();
        
        let output = renderer.render_incremental(Some(&frame), None);
        
        // Output should contain performance overlay
        assert!(output.contains("PERF"));
        assert!(output.contains("Frame:"));
        assert!(output.contains("Chars:"));
        assert!(output.contains("Glyphs:"));
    }
    
    #[test]
    #[cfg(feature = "perf_debug")]
    fn test_clear_operations_counted() {
        let mut renderer = TextRenderer::new();
        
        // First render with content
        let lines = vec!["Test".to_string()];
        let frame1 = create_text_buffer_frame(lines, None, 1);
        renderer.render_incremental(Some(&frame1), None);
        
        // Clear view
        renderer.render_incremental(None, None);
        
        let stats = renderer.stats();
        assert_eq!(stats.clear_operations, 1);
    }
    
    #[test]
    #[cfg(feature = "perf_debug")]
    fn test_status_line_redraws_counted() {
        let mut renderer = TextRenderer::new();
        let status1 = create_status_frame("Status 1".to_string(), 1);
        
        renderer.render_incremental(None, Some(&status1));
        
        let stats = renderer.stats();
        assert_eq!(stats.status_line_redraws, 1);
    }
}
