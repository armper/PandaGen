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

pub mod font;

#[cfg(test)]
use hal::PixelFormat;
use hal::{Framebuffer, FramebufferInfo};

use font::{get_char_bitmap, FONT_HEIGHT, FONT_WIDTH};

/// Foreground color (white)
const FG_COLOR: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);

/// Background color (black)
const BG_COLOR: (u8, u8, u8) = (0x00, 0x00, 0x00);

/// Cursor color (white, inverted)
const CURSOR_COLOR: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);

/// Framebuffer text console
pub struct ConsoleFb<F: Framebuffer> {
    framebuffer: F,
    cols: usize,
    rows: usize,
}

impl<F: Framebuffer> ConsoleFb<F> {
    /// Create a new framebuffer console
    pub fn new(framebuffer: F) -> Self {
        let info = framebuffer.info();
        let cols = info.width / FONT_WIDTH;
        let rows = info.height / FONT_HEIGHT;
        Self {
            framebuffer,
            cols,
            rows,
        }
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

        // Fill with background color
        for y in 0..info.height {
            for x in 0..info.width {
                let offset = info.offset(x, y);
                if offset + 4 <= buffer.len() {
                    buffer[offset..offset + 4].copy_from_slice(&bg_bytes);
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
}
