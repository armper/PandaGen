//! Bare-metal framebuffer wrapper for console_fb
//!
//! This module provides a minimal inline framebuffer implementation
//! to avoid pulling in external dependencies with std requirements.

use crate::BootInfo;
use crate::display_sink::DisplaySink;

/// Font character width in pixels
const FONT_WIDTH: usize = 8;
/// Font character height in pixels
const FONT_HEIGHT: usize = 16;

/// Pixel format for the framebuffer
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit RGB (0xXXRRGGBB) - most common format
    Rgb32,
}

impl PixelFormat {
    /// Returns the number of bytes per pixel
    pub const fn bytes_per_pixel(&self) -> usize {
        match self {
            PixelFormat::Rgb32 => 4,
        }
    }

    /// Converts RGB color to the pixel format's byte representation
    pub fn to_bytes(&self, r: u8, g: u8, b: u8) -> [u8; 4] {
        match self {
            PixelFormat::Rgb32 => [b, g, r, 0],
        }
    }
}

/// Framebuffer information
#[derive(Debug, Copy, Clone)]
pub struct FramebufferInfo {
    /// Width in pixels
    pub width: usize,
    /// Height in pixels
    pub height: usize,
    /// Stride in pixels (may be larger than width for alignment)
    pub stride_pixels: usize,
    /// Pixel format
    pub format: PixelFormat,
}

impl FramebufferInfo {
    /// Calculate the byte offset for a pixel at (x, y)
    pub const fn offset(&self, x: usize, y: usize) -> usize {
        y * self.stride_pixels * self.format.bytes_per_pixel() + x * self.format.bytes_per_pixel()
    }

    /// Returns total buffer size in bytes
    pub const fn buffer_size(&self) -> usize {
        self.height * self.stride_pixels * self.format.bytes_per_pixel()
    }
}

/// Bare-metal framebuffer wrapper
///
/// # Safety
///
/// This wraps a raw pointer to video memory. The caller must ensure:
/// - The pointer remains valid for the lifetime of this object
/// - Only one BareMetalFramebuffer exists for a given address
/// - Access is synchronized if used from multiple contexts
pub struct BareMetalFramebuffer {
    info: FramebufferInfo,
    buffer: &'static mut [u8],
}

impl BareMetalFramebuffer {
    /// Create a new bare-metal framebuffer from BootInfo
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The framebuffer address in BootInfo is valid
    /// - No other references to the framebuffer exist
    /// - The framebuffer memory remains valid for the lifetime of this object
    ///
    /// Returns None if no framebuffer is available in BootInfo.
    pub unsafe fn from_boot_info(boot_info: &BootInfo) -> Option<Self> {
        let addr = boot_info.framebuffer_addr?;
        if boot_info.framebuffer_width == 0
            || boot_info.framebuffer_height == 0
            || boot_info.framebuffer_pitch == 0
            || boot_info.framebuffer_bpp == 0
        {
            return None;
        }

        // Determine pixel format based on bpp and mask info
        // For now, assume RGB32 for 32bpp (most common)
        let format = if boot_info.framebuffer_bpp == 32 {
            PixelFormat::Rgb32
        } else {
            // Fallback to RGB32 for other formats too
            PixelFormat::Rgb32
        };

        let info = FramebufferInfo {
            width: boot_info.framebuffer_width as usize,
            height: boot_info.framebuffer_height as usize,
            stride_pixels: boot_info.framebuffer_pitch as usize / format.bytes_per_pixel(),
            format,
        };

        let buffer_size = info.buffer_size();
        let buffer = core::slice::from_raw_parts_mut(addr, buffer_size);

        Some(Self { info, buffer })
    }

    /// Returns the number of text columns based on the font width
    pub fn cols(&self) -> usize {
        self.info.width / FONT_WIDTH
    }

    /// Returns the number of text rows based on the font height
    pub fn rows(&self) -> usize {
        self.info.height / FONT_HEIGHT
    }

    /// Returns framebuffer information
    pub fn info(&self) -> FramebufferInfo {
        self.info
    }

    /// Returns a mutable slice to the framebuffer pixel data
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        self.buffer
    }

    /// Clear the screen with a color (optimized with memset-style fill)
    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        let info = self.info();
        let bg_bytes = info.format.to_bytes(r, g, b);
        
        // Use optimized row fill
        for y in 0..info.height {
            self.fill_pixel_row(y, bg_bytes);
        }
    }
    
    /// Fill a single pixel row with a color (fast path)
    fn fill_pixel_row(&mut self, y: usize, color: [u8; 4]) {
        let info = self.info();
        if y >= info.height {
            return;
        }
        
        let bpp = info.format.bytes_per_pixel();
        let row_start = y * info.stride_pixels * bpp;
        let row_pixels = info.width;
        
        // Fast fill: write 4-byte color pattern directly
        // This is much faster than individual pixel writes
        for x in 0..row_pixels {
            let offset = row_start + x * bpp;
            if offset + 4 <= self.buffer.len() {
                // Direct memory write (no volatile needed for filling)
                self.buffer[offset] = color[0];
                self.buffer[offset + 1] = color[1];
                self.buffer[offset + 2] = color[2];
                self.buffer[offset + 3] = color[3];
            }
        }
    }
    
    /// Clear a text row (row of characters, not pixels) with background color
    /// This is much faster than drawing space characters
    pub fn clear_text_row(&mut self, text_row: usize, bg: (u8, u8, u8)) {
        if text_row >= self.rows() {
            return;
        }
        
        let info = self.info();
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);
        let y_start = text_row * FONT_HEIGHT;
        let y_end = (y_start + FONT_HEIGHT).min(info.height);
        
        for y in y_start..y_end {
            self.fill_pixel_row(y, bg_bytes);
        }
    }

    /// Draw a single character at (col, row) with foreground/background colors
    /// Optimized: writes 8 pixels per scan line at once instead of pixel-by-pixel
    pub fn draw_char_at(
        &mut self,
        col: usize,
        row: usize,
        ch: u8,
        fg: (u8, u8, u8),
        bg: (u8, u8, u8),
    ) -> bool {
        if col >= self.cols() || row >= self.rows() {
            return false;
        }

        let bitmap = get_char_bitmap(ch);
        let info = self.info();
        let fg_bytes = info.format.to_bytes(fg.0, fg.1, fg.2);
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);

        let x_offset = col * FONT_WIDTH;
        let y_offset = row * FONT_HEIGHT;
        let bpp = info.format.bytes_per_pixel();
        let stride = info.stride_pixels * bpp;

        for (row_idx, &row_data) in bitmap.iter().enumerate() {
            let y = y_offset + row_idx;
            if y >= info.height {
                break;
            }

            // Calculate base offset for this scan line
            let row_base = y * stride + x_offset * bpp;
            
            // Build 8 pixels worth of data (32 bytes for 4 bpp)
            let mut scanline: [u8; 32] = [0; 32];
            for col_idx in 0..FONT_WIDTH {
                let bit = (row_data >> (7 - col_idx)) & 1;
                let color = if bit == 1 { fg_bytes } else { bg_bytes };
                let off = col_idx * 4;
                scanline[off] = color[0];
                scanline[off + 1] = color[1];
                scanline[off + 2] = color[2];
                scanline[off + 3] = color[3];
            }
            
            // Write all 8 pixels at once
            if row_base + 32 <= self.buffer.len() {
                self.buffer[row_base..row_base + 32].copy_from_slice(&scanline);
            }
        }

        true
    }

    /// Draw text starting at (col, row) - optimized to write full scanlines
    /// For a row of text, this builds complete pixel rows and writes them at once
    pub fn draw_text_at(
        &mut self,
        col: usize,
        row: usize,
        text: &str,
        fg: (u8, u8, u8),
        bg: (u8, u8, u8),
    ) -> usize {
        // For short text or text with newlines, fall back to per-character
        if text.len() < 4 || text.bytes().any(|b| b == b'\n') {
            return self.draw_text_at_slow(col, row, text, fg, bg);
        }
        
        if row >= self.rows() || col >= self.cols() {
            return 0;
        }

        let info = self.info();
        let fg_bytes = info.format.to_bytes(fg.0, fg.1, fg.2);
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);
        let bpp = info.format.bytes_per_pixel();
        let stride = info.stride_pixels * bpp;
        
        let text_bytes = text.as_bytes();
        let max_chars = (self.cols() - col).min(text_bytes.len());
        let x_start = col * FONT_WIDTH;
        let y_start = row * FONT_HEIGHT;
        
        // For each scanline of the font (16 lines)
        for scanline_idx in 0..FONT_HEIGHT {
            let y = y_start + scanline_idx;
            if y >= info.height {
                break;
            }
            
            let row_base = y * stride + x_start * bpp;
            
            // Write each character's scanline
            for (char_idx, &ch) in text_bytes[..max_chars].iter().enumerate() {
                let bitmap = get_char_bitmap(ch);
                let row_data = bitmap[scanline_idx];
                
                // Build 8 pixels for this character's scanline
                let char_offset = row_base + char_idx * FONT_WIDTH * bpp;
                if char_offset + 32 > self.buffer.len() {
                    break;
                }
                
                for bit_idx in 0..FONT_WIDTH {
                    let bit = (row_data >> (7 - bit_idx)) & 1;
                    let color = if bit == 1 { fg_bytes } else { bg_bytes };
                    let off = char_offset + bit_idx * 4;
                    self.buffer[off] = color[0];
                    self.buffer[off + 1] = color[1];
                    self.buffer[off + 2] = color[2];
                    self.buffer[off + 3] = color[3];
                }
            }
        }
        
        max_chars
    }
    
    /// Fallback for text with newlines or very short text
    fn draw_text_at_slow(
        &mut self,
        mut col: usize,
        mut row: usize,
        text: &str,
        fg: (u8, u8, u8),
        bg: (u8, u8, u8),
    ) -> usize {
        let mut drawn = 0;

        for byte in text.bytes() {
            if byte == b'\n' {
                row += 1;
                col = 0;
                if row >= self.rows() {
                    break;
                }
                continue;
            }

            if col >= self.cols() {
                col = 0;
                row += 1;
            }

            if row >= self.rows() {
                break;
            }

            if self.draw_char_at(col, row, byte, fg, bg) {
                drawn += 1;
            }

            col += 1;
        }

        drawn
    }
    
    /// Draw text on a line and clear the rest with background color in ONE PASS
    /// This is more efficient than clear_line + draw_text (avoids double writes)
    pub fn draw_line(&mut self, row: usize, text: &str, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        if row >= self.rows() {
            return;
        }
        
        let info = self.info();
        let fg_bytes = info.format.to_bytes(fg.0, fg.1, fg.2);
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);
        let bpp = info.format.bytes_per_pixel();
        let stride = info.stride_pixels * bpp;
        let cols = self.cols();
        let y_start = row * FONT_HEIGHT;
        
        let text_bytes = text.as_bytes();
        let text_len = text_bytes.len().min(cols);
        
        // For each scanline of the font (16 lines)
        for scanline_idx in 0..FONT_HEIGHT {
            let y = y_start + scanline_idx;
            if y >= info.height {
                break;
            }
            
            let row_base = y * stride;
            
            // Draw text characters
            for (char_idx, &ch) in text_bytes[..text_len].iter().enumerate() {
                let bitmap = get_char_bitmap(ch);
                let row_data = bitmap[scanline_idx];
                let char_offset = row_base + char_idx * FONT_WIDTH * bpp;
                
                if char_offset + 32 > self.buffer.len() {
                    break;
                }
                
                for bit_idx in 0..FONT_WIDTH {
                    let bit = (row_data >> (7 - bit_idx)) & 1;
                    let color = if bit == 1 { fg_bytes } else { bg_bytes };
                    let off = char_offset + bit_idx * 4;
                    self.buffer[off] = color[0];
                    self.buffer[off + 1] = color[1];
                    self.buffer[off + 2] = color[2];
                    self.buffer[off + 3] = color[3];
                }
            }
            
            // Clear rest of line with background (after text ends)
            let clear_start_x = text_len * FONT_WIDTH;
            let clear_start = row_base + clear_start_x * bpp;
            let row_end = row_base + info.width * bpp;
            
            if clear_start < row_end && clear_start < self.buffer.len() {
                let end = row_end.min(self.buffer.len());
                // Fill remaining pixels with background
                for x in (clear_start..end).step_by(4) {
                    if x + 4 <= self.buffer.len() {
                        self.buffer[x] = bg_bytes[0];
                        self.buffer[x + 1] = bg_bytes[1];
                        self.buffer[x + 2] = bg_bytes[2];
                        self.buffer[x + 3] = bg_bytes[3];
                    }
                }
            }
        }
    }

    /// Draw a cursor at (col, row) by inverting colors
    pub fn draw_cursor(&mut self, col: usize, row: usize, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let _ = self.draw_char_at(col, row, b'_', fg, bg);
    }

    /// Scroll the framebuffer up by the given number of text rows.
    ///
    /// This moves pixel rows up by `lines * FONT_HEIGHT` and clears the bottom
    /// area with the background color. Uses fast memory copy and optimized clearing.
    pub fn scroll_up_text_lines(&mut self, lines: usize, bg: (u8, u8, u8)) {
        if lines == 0 {
            return;
        }

        let info = self.info();
        let pixel_rows = lines.saturating_mul(FONT_HEIGHT);
        if pixel_rows >= info.height {
            self.clear(bg.0, bg.1, bg.2);
            return;
        }

        let bytes_per_row = info.stride_pixels * info.format.bytes_per_pixel();
        let total_bytes = info.height * bytes_per_row;
        let offset = pixel_rows * bytes_per_row;

        // Fast memory move for the scroll
        unsafe {
            let ptr = self.buffer.as_mut_ptr();
            core::ptr::copy(ptr.add(offset), ptr, total_bytes - offset);
        }

        // Clear the bottom pixel rows using optimized row fill
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);
        let start_row = info.height - pixel_rows;
        for y in start_row..info.height {
            self.fill_pixel_row(y, bg_bytes);
        }
    }
}

/// Get bitmap data for a character (8x16 font)
fn get_char_bitmap(ch: u8) -> &'static [u8; 16] {
    let index = ch as usize;
    if index < FONT_DATA.len() {
        &FONT_DATA[index]
    } else {
        &FONT_DATA[0x3F] // '?' for unknown characters
    }
}

/// Simplified 8x16 font data (ASCII 0x00-0x7F)
static FONT_DATA: [[u8; 16]; 128] = include!("font_data_8x16.in");

fn write_pixel(buffer: &mut [u8], offset: usize, bytes: [u8; 4]) {
    if offset + 4 > buffer.len() {
        return;
    }
    unsafe {
        let ptr = buffer.as_mut_ptr().add(offset);
        core::ptr::write_volatile(ptr, bytes[0]);
        core::ptr::write_volatile(ptr.add(1), bytes[1]);
        core::ptr::write_volatile(ptr.add(2), bytes[2]);
        core::ptr::write_volatile(ptr.add(3), bytes[3]);
    }
}

impl DisplaySink for BareMetalFramebuffer {
    fn dims(&self) -> (usize, usize) {
        (self.cols(), self.rows())
    }

    fn clear(&mut self, attr: u8) {
        let (_, bg) = attr_to_rgb(attr);
        self.clear(bg.0, bg.1, bg.2);
    }

    fn write_at(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> bool {
        let (fg, bg) = attr_to_rgb(attr);
        self.draw_char_at(col, row, ch, fg, bg)
    }

    fn write_str_at(&mut self, col: usize, row: usize, text: &str, attr: u8) -> usize {
        let (fg, bg) = attr_to_rgb(attr);
        self.draw_text_at(col, row, text, fg, bg)
    }

    fn draw_cursor(&mut self, col: usize, row: usize, attr: u8) {
        let (fg, bg) = attr_to_rgb(attr);
        self.draw_cursor(col, row, fg, bg);
    }
}

fn attr_to_rgb(attr: u8) -> ((u8, u8, u8), (u8, u8, u8)) {
    let fg_idx = attr & 0x0F;
    let bg_idx = (attr >> 4) & 0x0F;
    (vga_color(fg_idx), vga_color(bg_idx))
}

fn vga_color(idx: u8) -> (u8, u8, u8) {
    match idx {
        0 => (0x00, 0x00, 0x00), // Black
        1 => (0x00, 0x00, 0xAA), // Blue
        2 => (0x00, 0xAA, 0x00), // Green
        3 => (0x00, 0xAA, 0xAA), // Cyan
        4 => (0xAA, 0x00, 0x00), // Red
        5 => (0xAA, 0x00, 0xAA), // Magenta
        6 => (0xAA, 0x55, 0x00), // Brown
        7 => (0xAA, 0xAA, 0xAA), // Light Gray
        8 => (0x55, 0x55, 0x55), // Dark Gray
        9 => (0x55, 0x55, 0xFF), // Light Blue
        10 => (0x55, 0xFF, 0x55), // Light Green
        11 => (0x55, 0xFF, 0xFF), // Light Cyan
        12 => (0xFF, 0x55, 0x55), // Light Red
        13 => (0xFF, 0x55, 0xFF), // Pink
        14 => (0xFF, 0xFF, 0x55), // Yellow
        15 => (0xFF, 0xFF, 0xFF), // White
        _ => (0xAA, 0xAA, 0xAA), // Default
    }
}
