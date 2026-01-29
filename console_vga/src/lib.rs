//! # VGA Text Console
//!
//! This crate provides a VGA text mode console that writes to physical memory 0xB8000.
//!
//! ## Philosophy
//!
//! This is NOT a terminal emulator. No ANSI escape codes, no VT100, no TTY model.
//! It's a deterministic text renderer: same snapshot → same VGA cells.
//!
//! ## Design Principles
//!
//! 1. **Minimal and deterministic**: Simple 80x25 text with attributes
//! 2. **Testable**: Pure logic tests for text layout and clamping
//! 3. **No unsafe except MMIO**: Isolated to memory writes
//! 4. **Explicit cursor**: Cursor position is passed in, not tracked internally

#![cfg_attr(not(test), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::ptr;
#[cfg(test)]
use serde::{Deserialize, Serialize};

#[cfg(feature = "alloc")]
pub mod scrollback;
#[cfg(feature = "alloc")]
pub mod selection;
#[cfg(test)]
pub mod themes;
pub mod tiling;

#[cfg(feature = "alloc")]
pub use scrollback::{VgaLine, VgaScrollback};
#[cfg(feature = "alloc")]
pub use selection::{Clipboard, SelectionManager, SelectionRange};
#[cfg(test)]
pub use themes::{ColorPair, ColorRole, Theme, ThemeManager};
pub use tiling::{SplitLayout, TileBounds, TileId, TileManager};

/// VGA text mode dimensions
pub const VGA_WIDTH: usize = 80;
pub const VGA_HEIGHT: usize = 25;

/// VGA color codes
#[cfg_attr(test, derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VgaColor {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// Style enum (compatible with console_fb::Style)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Normal,
    Bold,
    Error,
    Success,
    Info,
}

impl Style {
    /// Convert style to VGA attribute byte
    pub fn to_vga_attr(self) -> u8 {
        match self {
            Style::Normal => VgaColor::make_attr(VgaColor::LightGray, VgaColor::Black),
            Style::Bold => VgaColor::make_attr(VgaColor::LightGreen, VgaColor::Black),
            Style::Error => VgaColor::make_attr(VgaColor::LightRed, VgaColor::Black),
            Style::Success => VgaColor::make_attr(VgaColor::LightGreen, VgaColor::Black),
            Style::Info => VgaColor::make_attr(VgaColor::LightCyan, VgaColor::Black),
        }
    }
}

impl VgaColor {
    /// Make a VGA attribute byte from foreground and background colors
    pub const fn make_attr(fg: VgaColor, bg: VgaColor) -> u8 {
        (bg as u8) << 4 | (fg as u8)
    }
}

/// VGA text console
pub struct VgaConsole {
    buffer: *mut u8,
}

impl VgaConsole {
    /// Create a new VGA console with the given virtual address of the VGA buffer
    ///
    /// # Safety
    ///
    /// The caller must ensure that `virt_addr` points to a valid, mapped VGA text buffer.
    pub unsafe fn new(virt_addr: usize) -> Self {
        Self {
            buffer: virt_addr as *mut u8,
        }
    }

    /// Clear the screen with the given attribute
    pub fn clear(&mut self, attr: u8) {
        for row in 0..VGA_HEIGHT {
            self.clear_row(row, attr);
        }
    }

    /// Clear a single row with the given attribute
    pub fn clear_row(&mut self, row: usize, attr: u8) {
        if row >= VGA_HEIGHT {
            return;
        }

        let cell = ((attr as u16) << 8) | b' ' as u16;
        let offset = row * VGA_WIDTH * 2;
        unsafe {
            let ptr = self.buffer.add(offset) as *mut u16;
            for col in 0..VGA_WIDTH {
                ptr::write_volatile(ptr.add(col), cell);
            }
        }
    }

    /// Write a line at the given row and clear the rest of the row
    ///
    /// The line does not wrap and is clipped to the screen width.
    pub fn write_line_at(&mut self, row: usize, text: &str, attr: u8) {
        if row >= VGA_HEIGHT {
            return;
        }

        let offset = row * VGA_WIDTH * 2;
        let space_cell = ((attr as u16) << 8) | b' ' as u16;
        let mut col = 0usize;

        unsafe {
            let ptr = self.buffer.add(offset) as *mut u16;
            for &byte in text.as_bytes().iter().take(VGA_WIDTH) {
                let cell = ((attr as u16) << 8) | (byte as u16);
                ptr::write_volatile(ptr.add(col), cell);
                col += 1;
            }
            for i in col..VGA_WIDTH {
                ptr::write_volatile(ptr.add(i), space_cell);
            }
        }
    }

    /// Blit a full screen buffer of VGA cells into the VGA text buffer.
    ///
    /// Each cell is a u16 with low byte = character, high byte = attribute.
    pub fn blit_from_cells(&mut self, cells: &[u16]) {
        let total_cells = VGA_WIDTH * VGA_HEIGHT;
        if cells.len() < total_cells {
            return;
        }

        let total_bytes = total_cells * 2;
        unsafe {
            ptr::copy_nonoverlapping(
                cells.as_ptr() as *const u8,
                self.buffer,
                total_bytes,
            );
        }
    }

    /// Write a character at the given column and row with the given attribute
    ///
    /// Returns true if the character was written (within bounds)
    pub fn write_at(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> bool {
        if col >= VGA_WIDTH || row >= VGA_HEIGHT {
            return false;
        }

        let offset = (row * VGA_WIDTH + col) * 2;
        unsafe {
            ptr::write_volatile(self.buffer.add(offset), ch);
            ptr::write_volatile(self.buffer.add(offset + 1), attr);
        }
        true
    }

    /// Write a string at the given column and row with the given attribute
    ///
    /// Text wraps to the next row if it exceeds column width.
    /// Returns the number of characters actually written.
    pub fn write_str_at(&mut self, mut col: usize, mut row: usize, text: &str, attr: u8) -> usize {
        let mut written = 0;

        for byte in text.bytes() {
            if byte == b'\n' {
                row += 1;
                col = 0;
                if row >= VGA_HEIGHT {
                    break;
                }
                continue;
            }

            if col >= VGA_WIDTH {
                col = 0;
                row += 1;
            }

            if row >= VGA_HEIGHT {
                break;
            }

            if self.write_at(col, row, byte, attr) {
                written += 1;
            }

            col += 1;
        }

        written
    }

    /// Present a text snapshot to the VGA buffer
    ///
    /// Clears screen, draws lines, and optionally draws cursor.
    /// Lines beyond visible rows are clipped.
    pub fn present_snapshot(
        &mut self,
        snapshot_text: &str,
        cursor_col: Option<usize>,
        cursor_row: Option<usize>,
        style: Style,
    ) {
        let attr = style.to_vga_attr();
        self.clear(attr);

        for (row, line) in snapshot_text.lines().enumerate() {
            if row >= VGA_HEIGHT {
                break;
            }
            self.write_str_at(0, row, line, attr);
        }

        // Draw cursor if specified (invert attribute)
        if let (Some(col), Some(row)) = (cursor_col, cursor_row) {
            self.draw_cursor(col, row, attr);
        }
    }

    /// Draw cursor at (col, row) by inverting the attribute
    pub fn draw_cursor(&mut self, col: usize, row: usize, attr: u8) {
        if col >= VGA_WIDTH || row >= VGA_HEIGHT {
            return;
        }

        // Invert foreground and background
        let inverted_attr = ((attr & 0x0F) << 4) | ((attr & 0xF0) >> 4);

        let offset = (row * VGA_WIDTH + col) * 2;
        unsafe {
            // Read current character, write with inverted attribute
            let ch = ptr::read_volatile(self.buffer.add(offset));
            ptr::write_volatile(self.buffer.add(offset + 1), inverted_attr);
            // If it's a space, make it visible by writing underscore
            if ch == b' ' {
                ptr::write_volatile(self.buffer.add(offset), b'_');
            }
        }
    }

    /// Scroll the entire VGA text buffer up by the given number of rows.
    ///
    /// Lines that scroll off the top are discarded; new lines at the bottom
    /// are cleared with spaces using the provided attribute.
    pub fn scroll_up(&mut self, lines: usize, attr: u8) {
        if lines == 0 {
            return;
        }

        if lines >= VGA_HEIGHT {
            self.clear(attr);
            return;
        }

        let row_bytes = VGA_WIDTH * 2;
        let total_bytes = VGA_HEIGHT * row_bytes;
        let offset = lines * row_bytes;

        unsafe {
            // Move visible rows up in-place.
            ptr::copy(self.buffer.add(offset), self.buffer, total_bytes - offset);
        }

        // Clear the bottom rows.
        for row in (VGA_HEIGHT - lines)..VGA_HEIGHT {
            self.clear_row(row, attr);
        }
    }

    /// Render scrollback buffer to VGA display
    ///
    /// Displays the visible portion of the scrollback buffer
    #[cfg(feature = "alloc")]
    pub fn render_scrollback(&mut self, scrollback: &VgaScrollback) {
        // Clear screen first
        self.clear(
            scrollback
                .visible_lines()
                .first()
                .and_then(|line| line.attrs.first().copied())
                .unwrap_or(Style::Normal.to_vga_attr()),
        );

        // Render visible lines
        for (row, line) in scrollback.visible_lines().iter().enumerate() {
            if row >= VGA_HEIGHT {
                break;
            }

            for (col, (&ch, &attr)) in line.text.iter().zip(line.attrs.iter()).enumerate() {
                if col >= VGA_WIDTH {
                    break;
                }
                self.write_at(col, row, ch, attr);
            }
        }
    }

    /// Highlight a selection range by inverting attributes
    ///
    /// This visually shows selected text
    #[cfg(feature = "alloc")]
    pub fn highlight_selection(&mut self, selection: selection::SelectionRange) {
        let ((start_col, start_row), (end_col, end_row)) = selection.normalized();

        for row in start_row..=end_row {
            if row >= VGA_HEIGHT {
                break;
            }

            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row {
                end_col.min(VGA_WIDTH - 1)
            } else {
                VGA_WIDTH - 1
            };

            for col in col_start..=col_end {
                let offset = (row * VGA_WIDTH + col) * 2;
                unsafe {
                    // Read current attribute, invert it
                    let attr = ptr::read_volatile(self.buffer.add(offset + 1));
                    let inverted_attr = ((attr & 0x0F) << 4) | ((attr & 0xF0) >> 4);
                    ptr::write_volatile(self.buffer.add(offset + 1), inverted_attr);
                }
            }
        }
    }
}

// VgaConsole is Send + Sync because it only accesses VGA memory via volatile writes
// Multiple threads can safely write to different parts of VGA memory
unsafe impl Send for VgaConsole {}
unsafe impl Sync for VgaConsole {}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate alloc;
    use alloc::vec;

    // Mock VGA buffer for testing
    struct MockVgaBuffer {
        data: alloc::vec::Vec<u8>,
    }

    impl MockVgaBuffer {
        fn new() -> Self {
            Self {
                data: vec![0u8; VGA_WIDTH * VGA_HEIGHT * 2],
            }
        }

        fn as_ptr(&mut self) -> *mut u8 {
            self.data.as_mut_ptr()
        }

        fn get_char(&self, col: usize, row: usize) -> u8 {
            let offset = (row * VGA_WIDTH + col) * 2;
            self.data[offset]
        }

        fn get_attr(&self, col: usize, row: usize) -> u8 {
            let offset = (row * VGA_WIDTH + col) * 2;
            self.data[offset + 1]
        }
    }

    #[test]
    fn test_vga_dimensions() {
        assert_eq!(VGA_WIDTH, 80);
        assert_eq!(VGA_HEIGHT, 25);
    }

    #[test]
    fn test_vga_color_attr() {
        let attr = VgaColor::make_attr(VgaColor::White, VgaColor::Black);
        assert_eq!(attr, 0x0F); // White on black

        let attr = VgaColor::make_attr(VgaColor::LightGreen, VgaColor::Black);
        assert_eq!(attr, 0x0A); // Light green on black
    }

    #[test]
    fn test_style_to_vga_attr() {
        assert_eq!(Style::Normal.to_vga_attr(), 0x07); // Light gray on black
        assert_eq!(Style::Bold.to_vga_attr(), 0x0A); // Light green on black
        assert_eq!(Style::Error.to_vga_attr(), 0x0C); // Light red on black
    }

    #[test]
    fn test_vga_console_write_at() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Write a character
        assert!(console.write_at(0, 0, b'A', 0x07));
        assert_eq!(buffer.get_char(0, 0), b'A');
        assert_eq!(buffer.get_attr(0, 0), 0x07);

        // Write out of bounds
        assert!(!console.write_at(VGA_WIDTH, 0, b'B', 0x07));
        assert!(!console.write_at(0, VGA_HEIGHT, b'C', 0x07));
    }

    #[test]
    fn test_vga_console_write_str_at() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        let text = "Hello";
        let written = console.write_str_at(0, 0, text, 0x07);
        assert_eq!(written, text.len());

        // Verify characters were written
        assert_eq!(buffer.get_char(0, 0), b'H');
        assert_eq!(buffer.get_char(1, 0), b'e');
        assert_eq!(buffer.get_char(2, 0), b'l');
        assert_eq!(buffer.get_char(3, 0), b'l');
        assert_eq!(buffer.get_char(4, 0), b'o');
    }

    #[test]
    fn test_vga_console_write_str_with_newline() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        let text = "Line1\nLine2";
        let written = console.write_str_at(0, 0, text, 0x07);
        assert_eq!(written, text.len() - 1); // Newline not counted

        // Verify first line
        assert_eq!(buffer.get_char(0, 0), b'L');
        assert_eq!(buffer.get_char(4, 0), b'1');

        // Verify second line
        assert_eq!(buffer.get_char(0, 1), b'L');
        assert_eq!(buffer.get_char(4, 1), b'2');
    }

    #[test]
    fn test_vga_console_clear() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Write something
        console.write_at(0, 0, b'A', 0x07);

        // Clear
        console.clear(0x07);

        // Verify all cells are spaces
        for row in 0..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                assert_eq!(buffer.get_char(col, row), b' ');
                assert_eq!(buffer.get_attr(col, row), 0x07);
            }
        }
    }

    #[test]
    fn test_vga_console_clamping() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Try to write beyond screen dimensions
        let long_text = "A".repeat(VGA_WIDTH * VGA_HEIGHT + 100);
        let written = console.write_str_at(0, 0, &long_text, 0x07);

        // Should clamp to visible area
        assert!(written <= VGA_WIDTH * VGA_HEIGHT);
    }

    #[test]
    fn test_vga_console_wrapping() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Write text that wraps to next line
        let text = "A".repeat(VGA_WIDTH + 5);
        let written = console.write_str_at(0, 0, &text, 0x07);
        assert_eq!(written, VGA_WIDTH + 5);

        // Verify wrap
        assert_eq!(buffer.get_char(VGA_WIDTH - 1, 0), b'A');
        assert_eq!(buffer.get_char(0, 1), b'A');
        assert_eq!(buffer.get_char(4, 1), b'A');
    }

    #[test]
    fn test_vga_console_present_snapshot() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        let snapshot = "Line 1\nLine 2\nLine 3";
        console.present_snapshot(snapshot, Some(0), Some(0), Style::Normal);

        // Verify lines were written
        assert_eq!(buffer.get_char(0, 0), b'L');
        assert_eq!(buffer.get_char(0, 1), b'L');
        assert_eq!(buffer.get_char(0, 2), b'L');

        // Verify cursor (should have inverted attribute or underscore)
        let cursor_attr = buffer.get_attr(0, 0);
        assert_ne!(cursor_attr, 0x07); // Should be inverted
    }

    #[test]
    fn test_cursor_visibility() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Clear with spaces
        console.clear(0x07);

        // Draw cursor at empty position
        console.draw_cursor(5, 5, 0x07);

        // Should have underscore at cursor position
        assert_eq!(buffer.get_char(5, 5), b'_');

        // Attribute should be inverted
        let cursor_attr = buffer.get_attr(5, 5);
        assert_eq!(cursor_attr, 0x70); // Inverted 0x07
    }

    #[test]
    fn test_render_scrollback() {
        use crate::scrollback::VgaScrollback;

        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        let mut scrollback = VgaScrollback::new(VGA_WIDTH, VGA_HEIGHT, 1000, 0x07);
        scrollback.push_line("Line 1", 0x07);
        scrollback.push_line("Line 2", 0x0A);
        scrollback.push_line("Line 3", 0x0C);

        console.render_scrollback(&scrollback);

        // Verify lines were rendered
        assert_eq!(buffer.get_char(0, 0), b'L');
        assert_eq!(buffer.get_attr(0, 0), 0x07);
        assert_eq!(buffer.get_char(0, 1), b'L');
        assert_eq!(buffer.get_attr(0, 1), 0x0A);
        assert_eq!(buffer.get_char(0, 2), b'L');
        assert_eq!(buffer.get_attr(0, 2), 0x0C);
    }

    #[test]
    fn test_highlight_selection() {
        use crate::selection::SelectionRange;

        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Write some text
        console.clear(0x07);
        console.write_str_at(0, 0, "Hello World", 0x07);

        // Create selection (characters 0-4 = "Hello")
        let selection = SelectionRange::new((0, 0), (4, 0));
        console.highlight_selection(selection);

        // Verify selection is highlighted (attributes inverted)
        // Original: 0x07 (light gray on black)
        // Inverted: 0x70 (black on light gray)
        assert_eq!(buffer.get_attr(0, 0), 0x70);
        assert_eq!(buffer.get_attr(1, 0), 0x70);
        assert_eq!(buffer.get_attr(4, 0), 0x70);

        // Character after selection should not be highlighted
        assert_eq!(buffer.get_attr(5, 0), 0x07);
    }

    #[test]
    fn test_scroll_up() {
        let mut buffer = MockVgaBuffer::new();
        let mut console = unsafe { VgaConsole::new(buffer.as_ptr() as usize) };

        // Fill first two rows with distinct characters.
        for col in 0..VGA_WIDTH {
            console.write_at(col, 0, b'A', 0x07);
            console.write_at(col, 1, b'B', 0x07);
        }

        console.scroll_up(1, 0x07);

        // Row 0 should now contain original row 1.
        assert_eq!(buffer.get_char(0, 0), b'B');
        assert_eq!(buffer.get_char(VGA_WIDTH - 1, 0), b'B');

        // Bottom row should be cleared.
        assert_eq!(buffer.get_char(0, VGA_HEIGHT - 1), b' ');
        assert_eq!(buffer.get_char(VGA_WIDTH - 1, VGA_HEIGHT - 1), b' ');
    }
}
