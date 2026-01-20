//! # Framebuffer HAL
//!
//! This module defines hardware abstraction traits for framebuffer devices.
//!
//! ## Philosophy
//!
//! The framebuffer HAL provides a minimal, deterministic interface for pixel-based output.
//! No VT100/ANSI terminal emulation, no TTY model—just raw pixel access for text rendering.
//!
//! ## Design Principles
//!
//! 1. **Minimal and explicit**: Width, height, stride, and pixel format
//! 2. **Architecture-agnostic**: Works with any bootloader that provides a linear framebuffer
//! 3. **Testable**: Can be mocked with a simple buffer for testing
//! 4. **Deterministic**: Same snapshot → same pixels

/// Pixel format for the framebuffer
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit RGB (0xXXRRGGBB) - most common format
    Rgb32,
    /// 32-bit BGR (0xXXBBGGRR)
    Bgr32,
}

impl PixelFormat {
    /// Returns the number of bytes per pixel
    pub const fn bytes_per_pixel(&self) -> usize {
        match self {
            PixelFormat::Rgb32 | PixelFormat::Bgr32 => 4,
        }
    }

    /// Converts RGB color to the pixel format's byte representation
    pub fn to_bytes(&self, r: u8, g: u8, b: u8) -> [u8; 4] {
        match self {
            PixelFormat::Rgb32 => [b, g, r, 0],
            PixelFormat::Bgr32 => [r, g, b, 0],
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

/// Framebuffer trait for pixel-based output
///
/// This trait provides access to a linear framebuffer for drawing pixels.
/// Implementations handle platform-specific memory mapping and synchronization.
pub trait Framebuffer {
    /// Returns framebuffer information
    fn info(&self) -> FramebufferInfo;

    /// Returns a mutable slice to the framebuffer pixel data
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - Writes stay within bounds (use `info().buffer_size()`)
    /// - Proper synchronization if accessed from multiple contexts
    ///
    /// # Notes
    ///
    /// The slice may be backed by device memory (e.g., video RAM).
    /// Write ordering may matter on some platforms.
    fn buffer_mut(&mut self) -> &mut [u8];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_bytes_per_pixel() {
        assert_eq!(PixelFormat::Rgb32.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Bgr32.bytes_per_pixel(), 4);
    }

    #[test]
    fn test_pixel_format_to_bytes_rgb() {
        let bytes = PixelFormat::Rgb32.to_bytes(0xFF, 0xAA, 0x55);
        assert_eq!(bytes, [0x55, 0xAA, 0xFF, 0]);
    }

    #[test]
    fn test_pixel_format_to_bytes_bgr() {
        let bytes = PixelFormat::Bgr32.to_bytes(0xFF, 0xAA, 0x55);
        assert_eq!(bytes, [0xFF, 0xAA, 0x55, 0]);
    }

    #[test]
    fn test_framebuffer_info_offset() {
        let info = FramebufferInfo {
            width: 80,
            height: 25,
            stride_pixels: 80,
            format: PixelFormat::Rgb32,
        };
        assert_eq!(info.offset(0, 0), 0);
        assert_eq!(info.offset(1, 0), 4);
        assert_eq!(info.offset(0, 1), 80 * 4);
        assert_eq!(info.offset(10, 5), (5 * 80 + 10) * 4);
    }

    #[test]
    fn test_framebuffer_info_buffer_size() {
        let info = FramebufferInfo {
            width: 80,
            height: 25,
            stride_pixels: 80,
            format: PixelFormat::Rgb32,
        };
        assert_eq!(info.buffer_size(), 25 * 80 * 4);
    }

    #[test]
    fn test_framebuffer_info_stride() {
        // Test case where stride is larger than width (common for alignment)
        let info = FramebufferInfo {
            width: 1920,
            height: 1080,
            stride_pixels: 2048, // Aligned to power of 2
            format: PixelFormat::Rgb32,
        };
        assert_eq!(info.offset(0, 1), 2048 * 4);
        assert_eq!(info.buffer_size(), 1080 * 2048 * 4);
    }
}
