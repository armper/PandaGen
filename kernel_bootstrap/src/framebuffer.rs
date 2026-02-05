//! Bare-metal framebuffer wrapper for console_fb
//!
//! This module provides a minimal inline framebuffer implementation
//! to avoid pulling in external dependencies with std requirements.

extern crate alloc;

use crate::display_sink::DisplaySink;
use crate::BootInfo;
use alloc::vec::Vec;

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
    pub fn to_bytes(self, r: u8, g: u8, b: u8) -> [u8; 4] {
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

/// Glyph entry in the cache
#[derive(Clone)]
struct GlyphEntry {
    ready: bool,
    scanlines: [[u8; 32]; FONT_HEIGHT],
}

impl GlyphEntry {
    fn empty() -> Self {
        Self {
            ready: false,
            scanlines: [[0u8; 32]; FONT_HEIGHT],
        }
    }
}

/// A slot in the glyph cache that caches glyphs for specific fg/bg colors
struct GlyphCacheSlot {
    fg: [u8; 4],
    bg: [u8; 4],
    glyphs: Vec<GlyphEntry>,
    last_used: u64,
    valid: bool,
}

impl GlyphCacheSlot {
    fn new() -> Self {
        Self {
            fg: [0; 4],
            bg: [0; 4],
            glyphs: Vec::new(),
            last_used: 0,
            valid: false,
        }
    }

    fn matches(&self, fg: [u8; 4], bg: [u8; 4]) -> bool {
        self.valid && self.fg == fg && self.bg == bg
    }
}

/// Simple 2-slot glyph cache for framebuffer rendering
struct GlyphCache {
    slots: [GlyphCacheSlot; 2],
    clock: u64,
}

impl GlyphCache {
    fn new() -> Self {
        Self {
            slots: [GlyphCacheSlot::new(), GlyphCacheSlot::new()],
            clock: 0,
        }
    }

    fn glyph_for(&mut self, ch: u8, fg: [u8; 4], bg: [u8; 4]) -> &[[u8; 32]; FONT_HEIGHT] {
        let idx = if (ch as usize) < 128 {
            ch as usize
        } else {
            b'?' as usize
        };
        let slot_index = if self.slots[0].matches(fg, bg) {
            0
        } else if self.slots[1].matches(fg, bg) {
            1
        } else if self.slots[0].last_used <= self.slots[1].last_used {
            0
        } else {
            1
        };

        let slot = &mut self.slots[slot_index];
        if !slot.valid || slot.fg != fg || slot.bg != bg {
            slot.fg = fg;
            slot.bg = bg;
            slot.glyphs.clear();
            slot.valid = true;
        }

        slot.last_used = self.clock;
        self.clock += 1;

        // Ensure the glyphs vec is large enough for this index
        if idx >= slot.glyphs.len() {
            slot.glyphs.resize(idx + 1, GlyphEntry::empty());
        }

        if !slot.glyphs[idx].ready {
            let bitmap = get_char_bitmap(ch);
            for (row_idx, &row_data) in bitmap.iter().enumerate() {
                for bit_idx in 0..FONT_WIDTH {
                    let bit = (row_data >> (7 - bit_idx)) & 1;
                    let pixel = if bit == 1 { fg } else { bg };
                    let offset = bit_idx * 4;
                    slot.glyphs[idx].scanlines[row_idx][offset..offset + 4].copy_from_slice(&pixel);
                }
            }
            slot.glyphs[idx].ready = true;
        }

        &slot.glyphs[idx].scanlines
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
    glyph_cache: Option<GlyphCache>,
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

        Some(Self {
            info,
            buffer,
            glyph_cache: None,
        })
    }

    /// Create a framebuffer from existing info and buffer memory.
    ///
    /// # Safety
    ///
    /// The caller must ensure `buffer` is valid for writes and matches `info` size.
    pub unsafe fn from_info_and_buffer(info: FramebufferInfo, buffer: &'static mut [u8]) -> Self {
        Self {
            info,
            buffer,
            glyph_cache: None,
        }
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

    /// Blit from a source buffer into the framebuffer.
    pub fn blit_from(&mut self, src: &[u8]) {
        let len = src.len().min(self.buffer.len());
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), self.buffer.as_mut_ptr(), len);
        }
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

    /// Fill a single pixel row with a color (ultra-fast using u64 writes)
    fn fill_pixel_row(&mut self, y: usize, color: [u8; 4]) {
        let info = self.info();
        if y >= info.height {
            return;
        }

        let row_start = y * info.stride_pixels * 4;
        let row_pixels = info.width;

        if row_start >= self.buffer.len() {
            return;
        }

        // Pack single pixel and double pixel for fast writes
        let pixel = u32::from_le_bytes(color);
        let double_pixel = ((pixel as u64) << 32) | (pixel as u64);

        unsafe {
            let ptr = self.buffer.as_mut_ptr().add(row_start);
            let ptr64 = ptr as *mut u64;
            let pairs = row_pixels / 2;

            // Write 2 pixels at a time (8 bytes)
            for i in 0..pairs {
                core::ptr::write_unaligned(ptr64.add(i), double_pixel);
            }

            // Handle odd pixel if width is odd
            if row_pixels % 2 == 1 {
                let ptr32 = ptr as *mut u32;
                core::ptr::write_unaligned(ptr32.add(row_pixels - 1), pixel);
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

    /// Clear a span of text cells on a row with background color.
    /// This avoids per-character rasterization when clearing trailing spaces.
    pub fn clear_text_span(&mut self, col: usize, row: usize, len: usize, bg: (u8, u8, u8)) {
        if row >= self.rows() || col >= self.cols() || len == 0 {
            return;
        }

        let info = self.info();
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);
        let bpp = info.format.bytes_per_pixel();
        let stride = info.stride_pixels * bpp;

        let max_len = (self.cols() - col).min(len);
        let pixel_start = col * FONT_WIDTH;
        let pixel_width = max_len * FONT_WIDTH;

        for scanline in 0..FONT_HEIGHT {
            let y = row * FONT_HEIGHT + scanline;
            if y >= info.height {
                break;
            }

            let row_base = y * stride + pixel_start * bpp;
            let byte_len = pixel_width * bpp;
            if row_base + byte_len > self.buffer.len() {
                break;
            }

            let mut offset = row_base;
            let mut remaining = byte_len;
            while remaining >= 8 {
                unsafe {
                    let ptr = self.buffer.as_mut_ptr().add(offset) as *mut u64;
                    let packed = u64::from_le_bytes([
                        bg_bytes[0],
                        bg_bytes[1],
                        bg_bytes[2],
                        bg_bytes[3],
                        bg_bytes[0],
                        bg_bytes[1],
                        bg_bytes[2],
                        bg_bytes[3],
                    ]);
                    ptr.write_unaligned(packed);
                }
                offset += 8;
                remaining -= 8;
            }

            for _ in 0..(remaining / 4) {
                self.buffer[offset..offset + 4].copy_from_slice(&bg_bytes);
                offset += 4;
            }
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

        let info = self.info();
        let fg_bytes = info.format.to_bytes(fg.0, fg.1, fg.2);
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);

        // Copy glyph data to avoid borrowing issues
        let glyph = *self.glyph_cache_mut().glyph_for(ch, fg_bytes, bg_bytes);

        let x_offset = col * FONT_WIDTH;
        let y_offset = row * FONT_HEIGHT;
        let bpp = info.format.bytes_per_pixel();
        let stride = info.stride_pixels * bpp;

        for (row_idx, scanline) in glyph.iter().enumerate() {
            let y = y_offset + row_idx;
            if y >= info.height {
                break;
            }

            // Calculate base offset for this scan line
            let row_base = y * stride + x_offset * bpp;

            // Write all 8 pixels at once
            if row_base + 32 <= self.buffer.len() {
                self.buffer[row_base..row_base + 32].copy_from_slice(scanline);
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

        // Pre-fetch all glyphs to avoid borrowing issues
        let mut glyphs: Vec<[[u8; 32]; FONT_HEIGHT]> = Vec::with_capacity(max_chars);
        for &ch in text_bytes[..max_chars].iter() {
            glyphs.push(*self.glyph_cache_mut().glyph_for(ch, fg_bytes, bg_bytes));
        }

        // For each scanline of the font (16 lines)
        for scanline_idx in 0..FONT_HEIGHT {
            let y = y_start + scanline_idx;
            if y >= info.height {
                break;
            }

            let row_base = y * stride + x_start * bpp;

            // Write each character's scanline
            for (char_idx, glyph) in glyphs.iter().enumerate() {
                let row_data = &glyph[scanline_idx];

                // Copy 8 pixels for this character's scanline
                let char_offset = row_base + char_idx * FONT_WIDTH * bpp;
                if char_offset + 32 > self.buffer.len() {
                    break;
                }

                self.buffer[char_offset..char_offset + 32].copy_from_slice(row_data);
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
    /// Ultra-optimized: uses u64 writes for 2 pixels at once
    pub fn draw_line(&mut self, row: usize, text: &str, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        if row >= self.rows() {
            return;
        }

        let info = self.info();
        let fg_bytes = info.format.to_bytes(fg.0, fg.1, fg.2);
        let bg_bytes = info.format.to_bytes(bg.0, bg.1, bg.2);
        let stride = info.stride_pixels * 4; // bytes per row
        let cols = self.cols();
        let y_start = row * FONT_HEIGHT;

        let text_bytes = text.as_bytes();
        let text_len = text_bytes.len().min(cols);

        // Pre-compute u32 pixel values for fg and bg
        let fg_pixel = u32::from_le_bytes(fg_bytes);
        let bg_pixel = u32::from_le_bytes(bg_bytes);
        // Two bg pixels packed into u64 for fast clearing
        let bg_double = ((bg_pixel as u64) << 32) | (bg_pixel as u64);

        // For each scanline of the font (16 lines)
        for scanline_idx in 0..FONT_HEIGHT {
            let y = y_start + scanline_idx;
            if y >= info.height {
                break;
            }

            let row_base = y * stride;

            // Draw text characters using u32 writes
            for (char_idx, &ch) in text_bytes[..text_len].iter().enumerate() {
                let bitmap = get_char_bitmap(ch);
                let row_data = bitmap[scanline_idx];
                let char_offset = row_base + char_idx * FONT_WIDTH * 4;

                if char_offset + 32 > self.buffer.len() {
                    break;
                }

                // Write 8 pixels (one character width) using u32 writes
                unsafe {
                    let ptr = self.buffer.as_mut_ptr().add(char_offset) as *mut u32;
                    for bit_idx in 0..FONT_WIDTH {
                        let bit = (row_data >> (7 - bit_idx)) & 1;
                        let pixel = if bit == 1 { fg_pixel } else { bg_pixel };
                        core::ptr::write_unaligned(ptr.add(bit_idx), pixel);
                    }
                }
            }

            // Clear rest of line with background using u64 writes (2 pixels at a time)
            let clear_start_x = text_len * FONT_WIDTH;
            let clear_start = row_base + clear_start_x * 4;
            let row_end = row_base + info.width * 4;

            if clear_start < row_end && clear_start < self.buffer.len() {
                let end = row_end.min(self.buffer.len());
                let pixels_to_clear = (end - clear_start) / 4;

                unsafe {
                    let ptr = self.buffer.as_mut_ptr().add(clear_start);
                    let ptr64 = ptr as *mut u64;
                    let pairs = pixels_to_clear / 2;

                    // Write 2 pixels at a time
                    for i in 0..pairs {
                        core::ptr::write_unaligned(ptr64.add(i), bg_double);
                    }

                    // Handle odd pixel if any
                    if pixels_to_clear % 2 == 1 {
                        let ptr32 = ptr as *mut u32;
                        core::ptr::write_unaligned(ptr32.add(pixels_to_clear - 1), bg_pixel);
                    }
                }
            }
        }
    }

    /// Draw a cursor at (col, row) by inverting colors
    pub fn draw_cursor(&mut self, col: usize, row: usize, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let _ = self.draw_char_at(col, row, b'_', fg, bg);
    }

    fn glyph_cache_mut(&mut self) -> &mut GlyphCache {
        if self.glyph_cache.is_none() {
            self.glyph_cache = Some(GlyphCache::new());
        }
        self.glyph_cache.as_mut().expect("glyph cache initialized")
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

    fn clear_span(&mut self, col: usize, row: usize, len: usize, attr: u8) -> usize {
        let (_, bg) = attr_to_rgb(attr);
        self.clear_text_span(col, row, len, bg);
        len
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
        0 => (0x00, 0x00, 0x00),  // Black
        1 => (0x00, 0x00, 0xAA),  // Blue
        2 => (0x00, 0xAA, 0x00),  // Green
        3 => (0x00, 0xAA, 0xAA),  // Cyan
        4 => (0xAA, 0x00, 0x00),  // Red
        5 => (0xAA, 0x00, 0xAA),  // Magenta
        6 => (0xAA, 0x55, 0x00),  // Brown
        7 => (0xAA, 0xAA, 0xAA),  // Light Gray
        8 => (0x55, 0x55, 0x55),  // Dark Gray
        9 => (0x55, 0x55, 0xFF),  // Light Blue
        10 => (0x55, 0xFF, 0x55), // Light Green
        11 => (0x55, 0xFF, 0xFF), // Light Cyan
        12 => (0xFF, 0x55, 0x55), // Light Red
        13 => (0xFF, 0x55, 0xFF), // Pink
        14 => (0xFF, 0xFF, 0x55), // Yellow
        15 => (0xFF, 0xFF, 0xFF), // White
        _ => (0xAA, 0xAA, 0xAA),  // Default
    }
}
