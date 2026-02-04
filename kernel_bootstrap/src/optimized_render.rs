//! Optimized editor renderer with dirty-region tracking
//!
//! This module provides an incremental renderer that tracks which cells
//! have changed and only redraws the minimum necessary.
//!
//! ## Design Philosophy
//! - Zero full-screen clears during normal editing
//! - Track dirty cells/lines, redraw only changes
//! - Cursor movement only redraws old and new cursor positions
//! - Deterministic: same state produces same output
//!
//! ## Performance Characteristics
//! - Typing a character: redraws 1-2 cells (character + cursor)
//! - Cursor movement: redraws 2 cells (old + new position)
//! - Mode change: redraws status line only
//! - Scroll: uses scroll_up optimization where available

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::display_sink::DisplaySink;
use crate::minimal_editor::MinimalEditor;

#[cfg(debug_assertions)]
use crate::render_stats;

/// Cell content for comparison
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: u8,
    attr: u8,
}

impl Cell {
    const EMPTY: Cell = Cell {
        ch: b' ',
        attr: 0x07,
    };

    fn new(ch: u8, attr: u8) -> Self {
        Self { ch, attr }
    }
}

/// Previous frame state for diffing
pub struct EditorRenderCache {
    /// Previous frame's cells (row-major order)
    cells: Vec<Cell>,
    /// Previous cursor position (col, row)
    cursor_pos: Option<(usize, usize)>,
    /// Previous status line content
    status_line: String,
    /// Screen dimensions
    cols: usize,
    rows: usize,
    /// Whether cache is valid (initialized with current dimensions)
    pub valid: bool,
    /// Previous scroll offset
    scroll_offset: usize,
}

impl EditorRenderCache {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            cursor_pos: None,
            status_line: String::new(),
            cols: 0,
            rows: 0,
            valid: false,
            scroll_offset: 0,
        }
    }

    /// Check if cache is valid
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Ensure cache matches screen dimensions
    fn ensure_size(&mut self, cols: usize, rows: usize) {
        if self.cols != cols || self.rows != rows {
            self.cols = cols;
            self.rows = rows;
            self.cells.clear();
            self.cells.resize(cols * rows, Cell::EMPTY);
            self.valid = false;
            self.cursor_pos = None;
            self.status_line.clear();
        }
    }

    /// Get cell at position
    fn get(&self, col: usize, row: usize) -> Cell {
        if col < self.cols && row < self.rows {
            self.cells[row * self.cols + col]
        } else {
            Cell::EMPTY
        }
    }

    /// Set cell at position
    fn set(&mut self, col: usize, row: usize, cell: Cell) {
        if col < self.cols && row < self.rows {
            self.cells[row * self.cols + col] = cell;
        }
    }

    /// Invalidate entire cache (forces full redraw)
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    #[cfg(debug_assertions)]
    fn is_cursor_only_update(
        &self,
        editor: &MinimalEditor,
        cols: usize,
        attr: u8,
        cursor_pos: Option<(usize, usize)>,
    ) -> bool {
        let status = editor.status_line();
        if status != self.status_line {
            return false;
        }

        let (_cursor_col, cursor_row) = match cursor_pos {
            Some(pos) => pos,
            None => return false,
        };

        let line = editor.get_viewport_line(cursor_row);
        let line_bytes = line.map(|s| s.as_bytes()).unwrap_or(&[]);
        for col in 0..cols {
            let ch = line_bytes.get(col).copied().unwrap_or(b' ');
            let expected = Cell::new(ch, attr);
            if self.get(col, cursor_row) != expected {
                return false;
            }
        }

        true
    }
}

impl Default for EditorRenderCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Render statistics for a single frame (debug-only instrumentation).
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameRenderStats {
    /// Number of dirty content lines updated this frame.
    pub dirty_lines_count: usize,
    /// Number of dirty spans updated this frame.
    pub dirty_spans_count: usize,
    /// Number of glyph blits issued this frame.
    pub glyph_blits_count: usize,
    /// Number of rectangle fills issued this frame.
    pub rect_fills_count: usize,
    /// Estimated pixel writes (glyphs + fills).
    pub pixels_written: usize,
    /// Number of flush calls issued.
    pub flush_calls: usize,
    /// Number of full redraws issued.
    pub full_redraws: usize,
    /// Compatibility counters for tests.
    pub cells_written: usize,
    pub lines_redrawn: usize,
    pub cursor_redraws: usize,
    pub full_clear: bool,
}

#[cfg(not(debug_assertions))]
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameRenderStats;

impl FrameRenderStats {
    const PIXELS_PER_CELL: usize = 8 * 16;

    #[inline]
    fn record_dirty_line(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.dirty_lines_count += 1;
            self.lines_redrawn += 1;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = self;
        }
    }

    #[inline]
    fn record_dirty_span(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.dirty_spans_count += 1;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = self;
        }
    }

    #[inline]
    fn record_glyph_blits(&mut self, count: usize) {
        #[cfg(debug_assertions)]
        {
            if count == 0 {
                return;
            }
            self.glyph_blits_count += count;
            self.cells_written += count;
            self.pixels_written += count * Self::PIXELS_PER_CELL;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = (self, count);
        }
    }

    #[inline]
    fn record_rect_fill(&mut self, cells: usize) {
        #[cfg(debug_assertions)]
        {
            if cells == 0 {
                return;
            }
            self.rect_fills_count += 1;
            self.cells_written += cells;
            self.pixels_written += cells * Self::PIXELS_PER_CELL;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = (self, cells);
        }
    }

    #[inline]
    fn record_cursor_redraw(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.cursor_redraws += 1;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = self;
        }
    }

    #[inline]
    fn record_full_redraw(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.full_redraws += 1;
            self.full_clear = true;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = self;
        }
    }

    #[inline]
    fn record_flush_call(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.flush_calls += 1;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = self;
        }
    }
}

/// Render editor to display sink with minimal redraws
///
/// Returns statistics about what was rendered.
pub fn render_editor_optimized(
    sink: &mut dyn DisplaySink,
    editor: &MinimalEditor,
    cache: &mut EditorRenderCache,
    normal_attr: u8,
    bold_attr: u8,
    force_full: bool,
    current_tick: u64,
) -> FrameRenderStats {
    let (cols, rows) = sink.dims();
    cache.ensure_size(cols, rows);

    #[cfg(debug_assertions)]
    render_stats::frame_begin(current_tick);

    let mut stats = FrameRenderStats::default();

    // Calculate viewport
    let viewport_rows = rows.saturating_sub(1);
    let current_scroll = editor.scroll_offset();

    let cursor_pos = editor.get_viewport_cursor().map(|pos| {
        (
            pos.col.min(cols.saturating_sub(1)),
            pos.row.min(viewport_rows.saturating_sub(1)),
        )
    });

    #[cfg(debug_assertions)]
    let pre_cursor_only = cache.is_cursor_only_update(editor, cols, normal_attr, cursor_pos);

    // Full redraws are allowed ONLY for:
    // - Editor open/close (cache invalidated by caller)
    // - Layout or viewport size changes
    // - Theme or font changes (caller must invalidate cache)
    // - Explicit "invalidate all" request (force_full)
    // Typing, Enter, Esc, cursor moves, and :w must stay incremental.
    let need_full_redraw = force_full || !cache.valid || cache.scroll_offset != current_scroll;

    #[cfg(debug_assertions)]
    let pre_dirty_rows = editor.scroll_offset();

    if need_full_redraw {
        stats.record_full_redraw();
        stats.record_flush_call();
        #[cfg(debug_assertions)]
        render_stats::record_full_clear();

        // Full redraw of all content lines
        for viewport_row in 0..viewport_rows {
            render_line_full(
                sink,
                editor,
                cache,
                viewport_row,
                cols,
                normal_attr,
                &mut stats,
            );
        }
        cache.scroll_offset = current_scroll;
        cache.valid = true;
    } else {
        // Incremental update: only redraw changed lines
        // First, get previous cursor position to restore that cell
        if let Some((old_col, old_row)) = cache.cursor_pos {
            if old_row < viewport_rows {
                // Restore the cell under old cursor
                let cell = cache.get(old_col, old_row);
                sink.write_at(old_col, old_row, cell.ch, cell.attr);
                stats.record_glyph_blits(1);
                stats.record_cursor_redraw();
                #[cfg(debug_assertions)]
                render_stats::record_char_draw();
            }
        }

        // Now check which lines need updating by comparing content
        for viewport_row in 0..viewport_rows {
            if line_needs_update(editor, cache, viewport_row, cols, normal_attr) {
                render_line_incremental(
                    sink,
                    editor,
                    cache,
                    viewport_row,
                    cols,
                    normal_attr,
                    &mut stats,
                );
            }
        }
    }

    // Always update status line if changed
    let status = editor.status_line();
    let status_row = rows.saturating_sub(1);
    if need_full_redraw || status != cache.status_line {
        render_status_line(sink, status, cache, status_row, cols, bold_attr, &mut stats);
        cache.status_line.clear();
        cache.status_line.push_str(status);
    }

    // Draw cursor at new position
    if let Some((cursor_col, cursor_row)) = cursor_pos {
        // Draw cursor (inverted or underscore)
        sink.draw_cursor(cursor_col, cursor_row, normal_attr);
        cache.cursor_pos = Some((cursor_col, cursor_row));
        stats.record_cursor_redraw();
        #[cfg(debug_assertions)]
        render_stats::record_char_draw();
    } else {
        cache.cursor_pos = None;
    }

    #[cfg(debug_assertions)]
    {
        let _ = pre_dirty_rows;
        if !need_full_redraw {
            debug_assert!(
                stats.full_redraws == 0,
                "full redraws must not occur during incremental passes"
            );
        }
        debug_assert!(
            stats.flush_calls <= 1,
            "render pass must flush at most once"
        );
        if pre_cursor_only {
            debug_assert!(
                stats.dirty_lines_count == 0,
                "cursor-only updates must not dirty lines"
            );
        }
        let _frame_stats = render_stats::frame_end(0);
    }

    stats
}

/// Check if a line needs to be updated
fn line_needs_update(
    editor: &MinimalEditor,
    cache: &EditorRenderCache,
    viewport_row: usize,
    cols: usize,
    attr: u8,
) -> bool {
    let line = editor.get_viewport_line(viewport_row);
    let line_bytes = line.map(|s| s.as_bytes()).unwrap_or(&[]);

    // Compare each cell
    for col in 0..cols {
        let ch = line_bytes.get(col).copied().unwrap_or(b' ');
        let expected = Cell::new(ch, attr);
        if cache.get(col, viewport_row) != expected {
            return true;
        }
    }
    false
}

/// Render a single line with full overwrite
fn render_line_full(
    sink: &mut dyn DisplaySink,
    editor: &MinimalEditor,
    cache: &mut EditorRenderCache,
    viewport_row: usize,
    cols: usize,
    attr: u8,
    stats: &mut FrameRenderStats,
) {
    let line = editor.get_viewport_line(viewport_row);
    let line_bytes = line.map(|s| s.as_bytes()).unwrap_or(&[]);
    let line_len = line_bytes.len().min(cols);

    // Write line content
    if line_len > 0 {
        if let Some(line_str) = line {
            let write_len = line_str.len().min(cols);
            sink.write_str_at(0, viewport_row, &line_str[..write_len], attr);
            stats.record_glyph_blits(write_len);
            #[cfg(debug_assertions)]
            render_stats::record_pixel_writes((write_len * 128) as u64);
        }
    }

    // Clear remaining cells on line in a single span (fast path for framebuffer)
    if line_len < cols {
        let cleared = sink.clear_span(line_len, viewport_row, cols - line_len, attr);
        for col in line_len..cols {
            cache.set(col, viewport_row, Cell::new(b' ', attr));
        }
        stats.record_rect_fill(cleared);
    }

    // Update cache for written content
    for (col, &byte) in line_bytes.iter().enumerate().take(cols) {
        cache.set(col, viewport_row, Cell::new(byte, attr));
    }

    stats.record_dirty_line();
    stats.record_dirty_span();
    #[cfg(debug_assertions)]
    render_stats::record_line_clear();
}

/// Render a single line incrementally (only changed cells)
fn render_line_incremental(
    sink: &mut dyn DisplaySink,
    editor: &MinimalEditor,
    cache: &mut EditorRenderCache,
    viewport_row: usize,
    cols: usize,
    attr: u8,
    stats: &mut FrameRenderStats,
) {
    let line = editor.get_viewport_line(viewport_row);
    let line_bytes = line.map(|s| s.as_bytes()).unwrap_or(&[]);

    // Find the span of changed cells and batch update
    let mut span_start: Option<usize> = None;
    let mut span_end = 0usize;

    for col in 0..cols {
        let ch = line_bytes.get(col).copied().unwrap_or(b' ');
        let new_cell = Cell::new(ch, attr);
        let old_cell = cache.get(col, viewport_row);

        if new_cell != old_cell {
            if span_start.is_none() {
                span_start = Some(col);
            }
            span_end = col + 1;
            cache.set(col, viewport_row, new_cell);
        }
    }

    // Write the changed span
    if let Some(start) = span_start {
        // For efficiency, write the entire changed span as a string if possible
        if let Some(line_str) = line {
            if start < line_str.len() {
                let write_end = span_end.min(line_str.len());
                sink.write_str_at(start, viewport_row, &line_str[start..write_end], attr);
                stats.record_glyph_blits(write_end - start);
                #[cfg(debug_assertions)]
                render_stats::record_pixel_writes(((write_end - start) * 128) as u64);
            }
        }

        // Write any trailing spaces in one span (fast path for framebuffer)
        let line_len = line_bytes.len();
        if span_end > line_len {
            let clear_start = line_len.max(start);
            let clear_len = span_end.saturating_sub(clear_start);
            if clear_len > 0 {
                let cleared = sink.clear_span(clear_start, viewport_row, clear_len, attr);
                stats.record_rect_fill(cleared);
                #[cfg(debug_assertions)]
                render_stats::record_char_draw();
            }
        }

        stats.record_dirty_line();
        stats.record_dirty_span();
    }
}

/// Render the status line
fn render_status_line(
    sink: &mut dyn DisplaySink,
    status: &str,
    cache: &mut EditorRenderCache,
    row: usize,
    cols: usize,
    attr: u8,
    stats: &mut FrameRenderStats,
) {
    let status_bytes = status.as_bytes();
    let status_len = status_bytes.len().min(cols);

    // Write status text
    sink.write_str_at(0, row, &status[..status_len], attr);
    stats.record_glyph_blits(status_len);
    #[cfg(debug_assertions)]
    render_stats::record_pixel_writes((status_len * 128) as u64);

    // Clear remaining cells with a single span
    if status_len < cols {
        let cleared = sink.clear_span(status_len, row, cols - status_len, attr);
        for col in status_len..cols {
            cache.set(col, row, Cell::new(b' ', attr));
        }
        stats.record_rect_fill(cleared);
    }

    // Update cache
    for (col, &byte) in status_bytes.iter().enumerate().take(cols) {
        cache.set(col, row, Cell::new(byte, attr));
    }

    stats.record_dirty_line();
    stats.record_dirty_span();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test display sink for verification
    struct TestSink {
        cells: Vec<Vec<(u8, u8)>>,
        cursor: Option<(usize, usize)>,
        cols: usize,
        rows: usize,
        write_count: usize,
    }

    impl TestSink {
        fn new(cols: usize, rows: usize) -> Self {
            Self {
                cells: vec![vec![(b' ', 0x07); cols]; rows],
                cursor: None,
                cols,
                rows,
                write_count: 0,
            }
        }

        #[allow(dead_code)]
        fn get_line(&self, row: usize) -> String {
            self.cells[row]
                .iter()
                .map(|(ch, _)| *ch as char)
                .collect::<String>()
                .trim_end()
                .to_string()
        }
    }

    impl DisplaySink for TestSink {
        fn dims(&self) -> (usize, usize) {
            (self.cols, self.rows)
        }

        fn clear(&mut self, attr: u8) {
            for row in &mut self.cells {
                for cell in row {
                    *cell = (b' ', attr);
                }
            }
            self.write_count += self.cols * self.rows;
        }

        fn write_at(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> bool {
            if col < self.cols && row < self.rows {
                self.cells[row][col] = (ch, attr);
                self.write_count += 1;
                true
            } else {
                false
            }
        }

        fn write_str_at(&mut self, col: usize, row: usize, text: &str, attr: u8) -> usize {
            let mut written = 0;
            for (i, byte) in text.bytes().enumerate() {
                if self.write_at(col + i, row, byte, attr) {
                    written += 1;
                }
            }
            written
        }

        fn draw_cursor(&mut self, col: usize, row: usize, _attr: u8) {
            self.cursor = Some((col, row));
        }
    }

    #[test]
    fn test_cache_initialization() {
        let mut cache = EditorRenderCache::new();
        assert!(!cache.valid);

        cache.ensure_size(80, 25);
        assert_eq!(cache.cols, 80);
        assert_eq!(cache.rows, 25);
        assert_eq!(cache.cells.len(), 80 * 25);
    }

    #[test]
    fn test_incremental_vs_full_writes() {
        let mut sink = TestSink::new(80, 25);
        let mut cache = EditorRenderCache::new();
        let editor = MinimalEditor::new(24);

        // First render should be full
        let stats1 =
            render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 100);
        #[cfg(debug_assertions)]
        assert!(stats1.full_clear);
        let writes_full = sink.write_count;

        // Reset counter
        sink.write_count = 0;

        // Second render with no changes should be minimal
        let stats2 =
            render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 110);
        #[cfg(debug_assertions)]
        assert!(!stats2.full_clear);
        let writes_incremental = sink.write_count;

        // Incremental should write far fewer cells
        assert!(
            writes_incremental < writes_full / 10,
            "Incremental writes {} should be much less than full writes {}",
            writes_incremental,
            writes_full
        );
    }

    #[test]
    fn test_cursor_only_change_minimal_writes() {
        let mut sink = TestSink::new(80, 25);
        let mut cache = EditorRenderCache::new();
        let mut editor = MinimalEditor::new(24);

        // Enter insert mode and type some text
        editor.process_byte(b'i');
        editor.process_byte(b'H');
        editor.process_byte(b'e');
        editor.process_byte(b'l');
        editor.process_byte(b'l');
        editor.process_byte(b'o');

        // First render
        let _ = render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 100);
        sink.write_count = 0;

        // Move cursor (Escape to normal, then 'h' to move left)
        editor.process_byte(0x1B); // Escape

        // Render after mode change - should update status line + minimal
        let stats = render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 110);

        // Should have minimal writes (cursor restore + new cursor + status)
        #[cfg(debug_assertions)]
        {
            assert!(
                stats.cells_written < 200,
                "Cursor move should write few cells, got {}",
                stats.cells_written
            );
        }
    }

    #[test]
    fn test_typing_50_characters_performance() {
        // Simulate typing 50 characters and measure total cell writes
        // Before optimization: ~80*25*50 = 100,000 cell writes
        // After optimization: ~50*80 + overhead = ~5,000 cell writes
        let mut sink = TestSink::new(80, 25);
        let mut cache = EditorRenderCache::new();
        let mut editor = MinimalEditor::new(24);

        // Initial render
        let _ = render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 0);
        sink.write_count = 0;

        // Enter insert mode
        editor.process_byte(b'i');
        let _ = render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 10);
        sink.write_count = 0;

        // Type 50 characters, render after each
        let mut total_writes = 0usize;
        for i in 0..50 {
            let ch = b'a' + (i % 26) as u8;
            editor.process_byte(ch);
            let _stats = render_editor_optimized(
                &mut sink,
                &editor,
                &mut cache,
                0x07,
                0x0F,
                false,
                20 + i as u64,
            );
            total_writes += sink.write_count;
            sink.write_count = 0;
        }

        // Should be much less than full redraws would require
        // Full redraw per keystroke: 80*25 = 2000 cells * 50 = 100,000
        // Optimized: ~50 chars * avg 80 cells = ~4000 (just the line changes)
        let full_redraw_cost = 80 * 25 * 50;
        assert!(
            total_writes < full_redraw_cost / 10,
            "50 char typing should be <10% of full redraw cost. Got {} vs full {}",
            total_writes,
            full_redraw_cost
        );

        // More specifically, should be around 5-10k writes
        assert!(
            total_writes < 15000,
            "50 char typing should be under 15k writes, got {}",
            total_writes
        );
    }

    #[test]
    fn test_hjkl_movement_performance() {
        // Test that cursor movement is very cheap
        let mut sink = TestSink::new(80, 25);
        let mut cache = EditorRenderCache::new();
        let mut editor = MinimalEditor::new(24);

        // Set up some content
        editor.process_byte(b'i');
        for _ in 0..10 {
            editor.process_byte(b'x');
        }
        editor.process_byte(0x0D); // Enter
        for _ in 0..10 {
            editor.process_byte(b'y');
        }
        editor.process_byte(0x1B); // Escape to normal mode

        // Initial render
        let _ = render_editor_optimized(&mut sink, &editor, &mut cache, 0x07, 0x0F, false, 0);
        sink.write_count = 0;

        // Move cursor 20 times with h/j/k/l and measure writes
        let moves = [
            b'h', b'j', b'k', b'l', b'h', b'h', b'j', b'k', b'l', b'l', b'h', b'j', b'k', b'l',
            b'h', b'h', b'j', b'k', b'l', b'l',
        ];
        let mut total_writes = 0usize;

        for (idx, &movement) in moves.iter().enumerate() {
            editor.process_byte(movement);
            let _stats = render_editor_optimized(
                &mut sink,
                &editor,
                &mut cache,
                0x07,
                0x0F,
                false,
                10 + idx as u64,
            );
            total_writes += sink.write_count;
            sink.write_count = 0;
        }

        // Each cursor move should only write ~2 cells (old cursor restore + new cursor)
        // Plus minimal line changes. Should be well under 100 writes per move.
        let writes_per_move = total_writes / moves.len();
        assert!(
            writes_per_move < 100,
            "Cursor move should be under 100 writes, got {} per move",
            writes_per_move
        );
    }
}
