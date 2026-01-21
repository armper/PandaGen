//! Bare-metal framebuffer wrapper for console_fb
//!
//! This module provides a minimal inline framebuffer implementation
//! to avoid pulling in external dependencies with std requirements.

use crate::BootInfo;

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

    /// Clear the screen with a color
    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        let info = self.info();
        let bg_bytes = info.format.to_bytes(r, g, b);

        // Fill with background color
        for y in 0..info.height {
            for x in 0..info.width {
                let offset = info.offset(x, y);
                write_pixel(self.buffer, offset, bg_bytes);
            }
        }
    }

    /// Draw a single character at (col, row) with foreground/background colors
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
                write_pixel(self.buffer, offset, *color);
            }
        }

        true
    }

    /// Draw text starting at (col, row)
    pub fn draw_text_at(
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

    /// Draw a cursor at (col, row) by inverting colors
    pub fn draw_cursor(&mut self, col: usize, row: usize, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let _ = self.draw_char_at(col, row, b'_', fg, bg);
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
