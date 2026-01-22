//! # Framebuffer Text Console
//!
//! This crate provides a simple text console that renders to a framebuffer.
//!
//! ## Philosophy
//!
//! This is NOT a terminal emulator. No ANSI escape codes, no VT100, no TTY model.
//! It's a deterministic text renderer: same snapshot → same pixels.
//!
//! ## Design Principles
//!
//! 1. **Minimal and deterministic**: Simple text layout with monospace font
//! 2. **Testable**: Pure logic tests for text layout and clamping
//! 3. **Clamps, doesn't scroll**: Text that doesn't fit is clipped (for now)
//! 4. **Explicit cursor**: Cursor position is passed in, not tracked internally

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod font;
pub mod scrollback;
pub mod styling;

#[cfg(feature = "editor-integration")]
pub mod combined_view;

#[cfg(test)]
use hal::PixelFormat;
use hal::{Framebuffer, FramebufferInfo};

use font::{get_char_bitmap, FONT_HEIGHT, FONT_WIDTH};
pub use scrollback::{Line, ScrollbackBuffer};
pub use styling::{Banner, RedrawManager, Style, StyledText};

#[cfg(feature = "editor-integration")]
pub use combined_view::{CombinedView, ViewMode};

#[cfg(any(debug_assertions, feature = "perf_debug"))]
#[derive(Debug, Default, Clone)]
pub struct RenderPerfStats {
    /// Timestamp at frame start (tick count or monotonic units from caller)
    pub frame_start_ticks: Option<u64>,
    /// Duration of the last frame in ticks
    pub last_frame_ticks: Option<u64>,
    /// Number of glyph draw calls
    pub glyph_draws: usize,
    /// Number of framebuffer pixel writes
    pub pixel_writes: usize,
    /// Number of screen clears
    pub clear_calls: usize,
    /// Number of draw_text_at calls
    pub text_draw_calls: usize,
    /// Number of cursor draws
    pub cursor_draws: usize,
    /// Number of status line redraws
    pub status_redraws: usize,
    /// Number of allocations performed during rendering (approximate)
    pub allocations: usize,
    /// Number of flushes/blits (if any)
    pub flushes: usize,
    /// Dirty lines updated this frame
    pub dirty_lines: usize,
    /// Dirty spans updated this frame
    pub dirty_spans: usize,
}

#[cfg(any(debug_assertions, feature = "perf_debug"))]
impl RenderPerfStats {
    fn reset_frame(&mut self) {
        self.glyph_draws = 0;
        self.pixel_writes = 0;
        self.clear_calls = 0;
        self.text_draw_calls = 0;
        self.cursor_draws = 0;
        self.status_redraws = 0;
        self.allocations = 0;
        self.flushes = 0;
        self.dirty_lines = 0;
        self.dirty_spans = 0;
    }
}

/// Foreground color (white)
const FG_COLOR: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);

/// Background color (black)
const BG_COLOR: (u8, u8, u8) = (0x00, 0x00, 0x00);

/// Cursor color (white, inverted)
const CURSOR_COLOR: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);

/// Framebuffer text console with scrollback support
pub struct ConsoleFb<F: Framebuffer> {
    framebuffer: F,
    cols: usize,
    rows: usize,
    scrollback: Option<ScrollbackBuffer>,
    #[cfg(any(debug_assertions, feature = "perf_debug"))]
    perf: RenderPerfStats,
}

impl<F: Framebuffer> ConsoleFb<F> {
    /// Create a new framebuffer console without scrollback
    pub fn new(framebuffer: F) -> Self {
        let info = framebuffer.info();
        let cols = info.width / FONT_WIDTH;
        let rows = info.height / FONT_HEIGHT;
        Self {
            framebuffer,
            cols,
            rows,
            scrollback: None,
            #[cfg(any(debug_assertions, feature = "perf_debug"))]
            perf: RenderPerfStats::default(),
        }
    }

    /// Create a new framebuffer console with scrollback buffer
    pub fn with_scrollback(framebuffer: F, max_lines: usize) -> Self {
        let info = framebuffer.info();
        let cols = info.width / FONT_WIDTH;
        let rows = info.height / FONT_HEIGHT;
        let scrollback = ScrollbackBuffer::new(cols, rows, max_lines);
        Self {
            framebuffer,
            cols,
            rows,
            scrollback: Some(scrollback),
            #[cfg(any(debug_assertions, feature = "perf_debug"))]
            perf: RenderPerfStats::default(),
        }
    }

    /// Returns the latest performance stats (gated)
    #[cfg(any(debug_assertions, feature = "perf_debug"))]
    pub fn perf_stats(&self) -> &RenderPerfStats {
        &self.perf
    }

    /// Returns mutable access to performance stats (gated)
    #[cfg(any(debug_assertions, feature = "perf_debug"))]
    pub fn perf_stats_mut(&mut self) -> &mut RenderPerfStats {
        &mut self.perf
    }

    /// Mark the start of a frame (gated)
    #[cfg(any(debug_assertions, feature = "perf_debug"))]
    pub fn perf_frame_start(&mut self, timestamp_ticks: u64) {
        self.perf.frame_start_ticks = Some(timestamp_ticks);
    }

    /// Mark the end of a frame (gated)
    #[cfg(any(debug_assertions, feature = "perf_debug"))]
    pub fn perf_frame_end(&mut self, timestamp_ticks: u64) {
        if let Some(start) = self.perf.frame_start_ticks {
            self.perf.last_frame_ticks = Some(timestamp_ticks.saturating_sub(start));
        }
    }

    /// Reset per-frame counters (gated)
    #[cfg(any(debug_assertions, feature = "perf_debug"))]
    pub fn perf_reset_frame(&mut self) {
        self.perf.reset_frame();
    }

    /// Returns the number of text columns
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Returns the number of text rows
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Clear the screen with background color
    pub fn clear(&mut self) {
        let info = self.framebuffer.info();
        let bg_bytes = info.format.to_bytes(BG_COLOR.0, BG_COLOR.1, BG_COLOR.2);
        let buffer = self.framebuffer.buffer_mut();

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            self.perf.clear_calls += 1;
        }

        // Fill with background color
        for y in 0..info.height {
            for x in 0..info.width {
                let offset = info.offset(x, y);
                if offset + 4 <= buffer.len() {
                    buffer[offset..offset + 4].copy_from_slice(&bg_bytes);
                    #[cfg(any(debug_assertions, feature = "perf_debug"))]
                    {
                        self.perf.pixel_writes += 1;
                    }
                }
            }
        }
    }

    /// Draw a single character at (col, row)
    ///
    /// Returns true if the character was drawn (within bounds)
    pub fn draw_char_at(&mut self, col: usize, row: usize, ch: u8) -> bool {
        if col >= self.cols || row >= self.rows {
            return false;
        }

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            self.perf.glyph_draws += 1;
        }

        let bitmap = get_char_bitmap(ch);
        let info = self.framebuffer.info();
        let buffer = self.framebuffer.buffer_mut();

        let fg_bytes = info.format.to_bytes(FG_COLOR.0, FG_COLOR.1, FG_COLOR.2);
        let bg_bytes = info.format.to_bytes(BG_COLOR.0, BG_COLOR.1, BG_COLOR.2);

        let x_offset = col * FONT_WIDTH;
        let y_offset = row * FONT_HEIGHT;

        for (row_idx, &row_data) in bitmap.iter().enumerate() {
            let y = y_offset + row_idx;
            if y >= info.height {
                break;
            }

            for col_idx in 0..FONT_WIDTH {
                let x = x_offset + col_idx;
                if x >= info.width {
                    break;
                }

                let bit = (row_data >> (7 - col_idx)) & 1;
                let color = if bit == 1 { &fg_bytes } else { &bg_bytes };

                let offset = info.offset(x, y);
                if offset + 4 <= buffer.len() {
                    buffer[offset..offset + 4].copy_from_slice(color);
                    #[cfg(any(debug_assertions, feature = "perf_debug"))]
                    {
                        self.perf.pixel_writes += 1;
                    }
                }
            }
        }

        true
    }

    /// Draw text starting at (col, row)
    ///
    /// Text wraps to next row if it exceeds column width.
    /// Returns the number of characters actually drawn.
    pub fn draw_text_at(&mut self, mut col: usize, mut row: usize, text: &str) -> usize {
        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            self.perf.text_draw_calls += 1;
        }
        let mut drawn = 0;

        for byte in text.bytes() {
            if byte == b'\n' {
                row += 1;
                col = 0;
                if row >= self.rows {
                    break;
                }
                continue;
            }

            if col >= self.cols {
                col = 0;
                row += 1;
            }

            if row >= self.rows {
                break;
            }

            if self.draw_char_at(col, row, byte) {
                drawn += 1;
            }

            col += 1;
        }

        drawn
    }

    /// Draw cursor at (col, row) as an inverted cell or underscore
    pub fn draw_cursor(&mut self, col: usize, row: usize) {
        if col >= self.cols || row >= self.rows {
            return;
        }

        #[cfg(any(debug_assertions, feature = "perf_debug"))]
        {
            self.perf.cursor_draws += 1;
        }

        let info = self.framebuffer.info();
        let buffer = self.framebuffer.buffer_mut();
        let cursor_bytes = info
            .format
            .to_bytes(CURSOR_COLOR.0, CURSOR_COLOR.1, CURSOR_COLOR.2);

        let x_offset = col * FONT_WIDTH;
        let y_offset = row * FONT_HEIGHT;

        // Draw underscore at bottom of character cell
        for row_idx in FONT_HEIGHT - 2..FONT_HEIGHT {
            let y = y_offset + row_idx;
            if y >= info.height {
                break;
            }

            for col_idx in 0..FONT_WIDTH {
                let x = x_offset + col_idx;
                if x >= info.width {
                    break;
                }

                let offset = info.offset(x, y);
                if offset + 4 <= buffer.len() {
                    buffer[offset..offset + 4].copy_from_slice(&cursor_bytes);
                    #[cfg(any(debug_assertions, feature = "perf_debug"))]
                    {
                        self.perf.pixel_writes += 1;
                    }
                }
            }
        }
    }

    /// Present a text snapshot to the framebuffer
    ///
    /// Clears screen, draws lines, and optionally draws cursor.
    /// Lines beyond visible rows are clipped.
    pub fn present_snapshot(
        &mut self,
        snapshot_text: &str,
        cursor_col: Option<usize>,
        cursor_row: Option<usize>,
    ) {
        self.clear();

        for (row, line) in snapshot_text.lines().enumerate() {
            if row >= self.rows {
                break;
            }
            self.draw_text_at(0, row, line);
        }

        // Draw cursor if specified
        if let (Some(col), Some(row)) = (cursor_col, cursor_row) {
            self.draw_cursor(col, row);
        }
    }

    /// Present from scrollback buffer
    ///
    /// Renders the visible viewport from the scrollback buffer.
    /// If no scrollback is configured, this does nothing.
    pub fn present_from_scrollback(
        &mut self,
        cursor_col: Option<usize>,
        cursor_row: Option<usize>,
    ) {
        // Collect visible lines first to avoid borrow checker issues
        let lines_to_draw: alloc::vec::Vec<alloc::string::String> =
            if let Some(ref scrollback) = self.scrollback {
                scrollback
                    .visible_lines()
                    .iter()
                    .map(|line| alloc::string::String::from(line.as_str()))
                    .collect()
            } else {
                return;
            };

        self.clear();

        for (row, line) in lines_to_draw.iter().enumerate() {
            if row >= self.rows {
                break;
            }
            self.draw_text_at(0, row, line);
        }

        // Draw cursor if specified
        if let (Some(col), Some(row)) = (cursor_col, cursor_row) {
            self.draw_cursor(col, row);
        }
    }

    /// Add text to scrollback buffer
    ///
    /// Appends text to the scrollback buffer and resets viewport to bottom.
    pub fn append_to_scrollback(&mut self, text: &str) {
        if let Some(ref mut scrollback) = self.scrollback {
            scrollback.push_text(text);
        }
    }

    /// Scroll the viewport up
    pub fn scroll_up(&mut self, lines: usize) -> bool {
        if let Some(ref mut scrollback) = self.scrollback {
            scrollback.scroll_up(lines)
        } else {
            false
        }
    }

    /// Scroll the viewport down
    pub fn scroll_down(&mut self, lines: usize) -> bool {
        if let Some(ref mut scrollback) = self.scrollback {
            scrollback.scroll_down(lines)
        } else {
            false
        }
    }

    /// Scroll to the bottom of the buffer
    pub fn scroll_to_bottom(&mut self) {
        if let Some(ref mut scrollback) = self.scrollback {
            scrollback.scroll_to_bottom();
        }
    }

    /// Scroll to the top of the buffer
    pub fn scroll_to_top(&mut self) {
        if let Some(ref mut scrollback) = self.scrollback {
            scrollback.scroll_to_top();
        }
    }

    /// Get mutable reference to scrollback buffer
    pub fn scrollback_mut(&mut self) -> Option<&mut ScrollbackBuffer> {
        self.scrollback.as_mut()
    }

    /// Get reference to scrollback buffer
    pub fn scrollback(&self) -> Option<&ScrollbackBuffer> {
        self.scrollback.as_ref()
    }
}

/// Calculate text dimensions (cols, rows) for a framebuffer
pub fn calculate_text_dimensions(info: &FramebufferInfo) -> (usize, usize) {
    let cols = info.width / FONT_WIDTH;
    let rows = info.height / FONT_HEIGHT;
    (cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_console_fb_dimensions() {
        let fb = MockFramebuffer::new(640, 480);
        let console = ConsoleFb::new(fb);
        assert_eq!(console.cols(), 640 / FONT_WIDTH);
        assert_eq!(console.rows(), 480 / FONT_HEIGHT);
    }

    #[test]
    fn test_console_fb_clear() {
        let fb = MockFramebuffer::new(64, 64);
        let mut console = ConsoleFb::new(fb);
        console.clear();
        // Check that buffer is filled with background color
        let buffer = console.framebuffer.buffer_mut();
        // Just verify it didn't crash and buffer is non-empty
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_draw_char_at_bounds() {
        let fb = MockFramebuffer::new(80, 80);
        let mut console = ConsoleFb::new(fb);

        // Valid position
        assert!(console.draw_char_at(0, 0, b'A'));

        // Out of bounds
        assert!(!console.draw_char_at(1000, 0, b'A'));
        assert!(!console.draw_char_at(0, 1000, b'A'));
    }

    #[test]
    fn test_draw_text_at() {
        let fb = MockFramebuffer::new(160, 160);
        let mut console = ConsoleFb::new(fb);

        let text = "Hello";
        let drawn = console.draw_text_at(0, 0, text);
        assert_eq!(drawn, text.len());
    }

    #[test]
    fn test_draw_text_with_newline() {
        let fb = MockFramebuffer::new(160, 160);
        let mut console = ConsoleFb::new(fb);

        let text = "Line1\nLine2";
        let drawn = console.draw_text_at(0, 0, text);
        // Should draw all characters except newline
        assert_eq!(drawn, text.len() - 1);
    }

    #[test]
    fn test_draw_cursor() {
        let fb = MockFramebuffer::new(160, 160);
        let mut console = ConsoleFb::new(fb);

        console.draw_cursor(0, 0);
        // Should not crash

        // Out of bounds should be safe
        console.draw_cursor(1000, 1000);
    }

    #[test]
    fn test_present_snapshot() {
        let fb = MockFramebuffer::new(160, 160);
        let mut console = ConsoleFb::new(fb);

        let snapshot = "Line 1\nLine 2\nLine 3";
        console.present_snapshot(snapshot, Some(0), Some(0));
        // Should not crash
    }

    #[test]
    fn test_calculate_text_dimensions() {
        let info = FramebufferInfo {
            width: 640,
            height: 480,
            stride_pixels: 640,
            format: PixelFormat::Rgb32,
        };
        let (cols, rows) = calculate_text_dimensions(&info);
        assert_eq!(cols, 640 / FONT_WIDTH);
        assert_eq!(rows, 480 / FONT_HEIGHT);
    }

    #[test]
    fn test_clamping_behavior() {
        let fb = MockFramebuffer::new(80, 80); // Very small framebuffer
        let mut console = ConsoleFb::new(fb);

        // Try to draw beyond visible area
        let long_text = "A".repeat(1000);
        let drawn = console.draw_text_at(0, 0, &long_text);

        // Should clamp to visible area
        assert!(drawn <= console.cols() * console.rows());
    }

    #[test]
    fn test_console_with_scrollback() {
        let fb = MockFramebuffer::new(160, 160);
        let console = ConsoleFb::with_scrollback(fb, 100);

        assert!(console.scrollback().is_some());
        assert_eq!(console.scrollback().unwrap().cols(), console.cols());
    }

    #[test]
    fn test_append_to_scrollback() {
        let fb = MockFramebuffer::new(160, 160);
        let mut console = ConsoleFb::with_scrollback(fb, 100);

        console.append_to_scrollback("Line 1\nLine 2");

        let scrollback = console.scrollback().unwrap();
        assert_eq!(scrollback.total_lines(), 2);
    }

    #[test]
    fn test_present_from_scrollback() {
        let fb = MockFramebuffer::new(160, 160);
        let mut console = ConsoleFb::with_scrollback(fb, 100);

        console.append_to_scrollback("Line 1\nLine 2\nLine 3");
        console.present_from_scrollback(None, None);

        // Should not crash
    }

    #[test]
    fn test_scrollback_viewport_operations() {
        let fb = MockFramebuffer::new(160, 48); // 3 rows
        let mut console = ConsoleFb::with_scrollback(fb, 100);

        // Add more lines than viewport can show
        for i in 1..=10 {
            console.append_to_scrollback(&format!("Line {}", i));
        }

        // Test scrolling
        assert!(console.scroll_up(2));
        assert!(console.scroll_down(1));
        console.scroll_to_top();
        console.scroll_to_bottom();

        // Should have scrollback
        assert!(console.scrollback().unwrap().at_bottom());
    }
}
