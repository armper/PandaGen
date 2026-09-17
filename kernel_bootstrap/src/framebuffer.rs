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

/// Source pixel format for desktop presentation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DesktopSurfaceFormat {
    /// 32-bit RGBA pixels emitted by the GUI host raster path.
    Rgba8888,
    /// Framebuffer-native 32-bit pixels already encoded for the target.
    NativeRgb32,
}

impl DesktopSurfaceFormat {
    const fn bytes_per_pixel(self) -> usize {
        4
    }
}

/// Full-frame desktop pixel buffer presented into the framebuffer.
#[derive(Debug, Copy, Clone)]
pub struct DesktopSurface<'a> {
    pub width: usize,
    pub height: usize,
    pub stride_pixels: usize,
    pub pixels: &'a [u8],
    pub format: DesktopSurfaceFormat,
}

impl<'a> DesktopSurface<'a> {
    pub const fn rgba8888(width: usize, height: usize, pixels: &'a [u8]) -> Self {
        Self {
            width,
            height,
            stride_pixels: width,
            pixels,
            format: DesktopSurfaceFormat::Rgba8888,
        }
    }

    pub const fn native_rgb32(
        width: usize,
        height: usize,
        stride_pixels: usize,
        pixels: &'a [u8],
    ) -> Self {
        Self {
            width,
            height,
            stride_pixels,
            pixels,
            format: DesktopSurfaceFormat::NativeRgb32,
        }
    }

    pub const fn required_bytes(&self) -> usize {
        self.height * self.stride_pixels * self.format.bytes_per_pixel()
    }
}

/// Presentation result for a desktop frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DesktopPresentStats {
    pub copied_pixels: usize,
    pub source_bytes: usize,
}

/// Presentation error for desktop frames.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DesktopPresentError {
    DimensionMismatch {
        expected_width: usize,
        expected_height: usize,
        actual_width: usize,
        actual_height: usize,
    },
    BufferLengthMismatch {
        expected: usize,
        actual: usize,
    },
    /// The source pixel format cannot be presented into this framebuffer.
    UnsupportedSourceFormat,
    StrideMismatch {
        expected_stride_pixels: usize,
        actual_stride_pixels: usize,
    },
}

/// Axis-aligned pixel rectangle used for damage tracking (GFX-019).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DamageRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl DamageRect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn area(&self) -> usize {
        self.width * self.height
    }

    const fn right(&self) -> usize {
        self.x.saturating_add(self.width)
    }

    const fn bottom(&self) -> usize {
        self.y.saturating_add(self.height)
    }

    /// Smallest rectangle covering both `self` and `other`.
    pub fn union(&self, other: DamageRect) -> DamageRect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        DamageRect::new(x, y, right - x, bottom - y)
    }

    /// Part of `self` inside `clip`, or `None` when they do not overlap.
    pub fn intersect(&self, clip: DamageRect) -> Option<DamageRect> {
        let x = self.x.max(clip.x);
        let y = self.y.max(clip.y);
        let right = self.right().min(clip.right());
        let bottom = self.bottom().min(clip.bottom());
        if right <= x || bottom <= y {
            return None;
        }
        Some(DamageRect::new(x, y, right - x, bottom - y))
    }
}

/// Accumulates the bounding box of everything drawn since the last present.
///
/// A single bounding box is deliberately simple: the text workspace draws in
/// row bands, so the box is usually tight, and a present of one box is one
/// predictable copy loop. Finer-grained region lists can come later without
/// changing the presenter contract.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct DamageTracker {
    bounds: Option<DamageRect>,
}

impl DamageTracker {
    pub const fn new() -> Self {
        Self { bounds: None }
    }

    pub fn mark(&mut self, rect: DamageRect) {
        if rect.is_empty() {
            return;
        }
        self.bounds = Some(match self.bounds {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    pub const fn peek(&self) -> Option<DamageRect> {
        self.bounds
    }

    pub const fn is_clean(&self) -> bool {
        self.bounds.is_none()
    }

    /// Return and clear the accumulated damage.
    pub fn take(&mut self) -> Option<DamageRect> {
        self.bounds.take()
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
    damage: DamageTracker,
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
            damage: DamageTracker::new(),
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
            damage: DamageTracker::new(),
        }
    }

    /// Full-surface rectangle for this framebuffer.
    pub const fn full_rect(&self) -> DamageRect {
        DamageRect::new(0, 0, self.info.width, self.info.height)
    }

    /// Damage accumulated since the last `take_damage`.
    pub const fn damage(&self) -> Option<DamageRect> {
        self.damage.peek()
    }

    /// Return and clear accumulated damage.
    pub fn take_damage(&mut self) -> Option<DamageRect> {
        self.damage.take()
    }

    fn mark_damage(&mut self, rect: DamageRect) {
        self.damage.mark(rect);
    }

    /// Mark the whole surface damaged so the next present copies everything.
    ///
    /// Used when the hardware framebuffer was written by another path (for
    /// example the graphics desktop) and the shadow must be re-presented in full.
    pub fn invalidate(&mut self) {
        self.mark_all_damaged();
    }

    fn mark_all_damaged(&mut self) {
        let rect = self.full_rect();
        self.mark_damage(rect);
    }

    /// Pixel rectangle covering `len` text cells starting at (col, row).
    fn text_cell_rect(&self, col: usize, row: usize, len: usize) -> DamageRect {
        let x = col * FONT_WIDTH;
        let y = row * FONT_HEIGHT;
        let width = (len * FONT_WIDTH).min(self.info.width.saturating_sub(x));
        let height = FONT_HEIGHT.min(self.info.height.saturating_sub(y));
        DamageRect::new(x, y, width, height)
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
        // Raw access can write anywhere; treat the whole surface as damaged.
        self.mark_all_damaged();
        self.buffer
    }

    /// Returns a read-only view of the framebuffer pixel data.
    ///
    /// Used by the presenter to read a shadow (off-screen) framebuffer as a
    /// native desktop surface without exposing raw blit access.
    pub fn buffer(&self) -> &[u8] {
        self.buffer
    }

    /// Present a whole shadow framebuffer into this (hardware) framebuffer.
    ///
    /// The shadow is the backbuffer the text workspace draws into. Routing it
    /// through `present_desktop_surface` means the raw hardware copy has a
    /// single, validated entry point shared with the graphical desktop path
    /// (GFX-017). A shadow whose geometry does not match the target is
    /// rejected instead of silently copying a prefix of bytes.
    pub fn present_shadow(
        &mut self,
        shadow: &BareMetalFramebuffer,
    ) -> Result<DesktopPresentStats, DesktopPresentError> {
        let info = shadow.info();
        if info.format != self.info().format {
            return Err(DesktopPresentError::UnsupportedSourceFormat);
        }
        let surface = DesktopSurface::native_rgb32(
            info.width,
            info.height,
            info.stride_pixels,
            shadow.buffer(),
        );
        self.present_desktop_surface(surface)
    }

    /// Present only the shadow's accumulated damage into this framebuffer.
    ///
    /// This is the GFX-019 present path: a paced present copies just the
    /// bounding box of what changed since the last present, so a single
    /// keystroke costs one text row band rather than a full-frame copy. The
    /// shadow's damage is consumed even if the present is rejected, since a
    /// rejected present is a geometry contract bug that a retry cannot fix.
    pub fn present_shadow_damage(
        &mut self,
        shadow: &mut BareMetalFramebuffer,
    ) -> Result<DesktopPresentStats, DesktopPresentError> {
        let damage = shadow.take_damage();
        let info = shadow.info();
        let target = self.info();
        if info.format != target.format {
            return Err(DesktopPresentError::UnsupportedSourceFormat);
        }
        if info.width != target.width || info.height != target.height {
            return Err(DesktopPresentError::DimensionMismatch {
                expected_width: target.width,
                expected_height: target.height,
                actual_width: info.width,
                actual_height: info.height,
            });
        }
        if info.stride_pixels != target.stride_pixels {
            return Err(DesktopPresentError::StrideMismatch {
                expected_stride_pixels: target.stride_pixels,
                actual_stride_pixels: info.stride_pixels,
            });
        }
        if shadow.buffer().len() != self.buffer.len() {
            return Err(DesktopPresentError::BufferLengthMismatch {
                expected: self.buffer.len(),
                actual: shadow.buffer().len(),
            });
        }

        let Some(rect) = damage.and_then(|d| d.intersect(self.full_rect())) else {
            return Ok(DesktopPresentStats {
                copied_pixels: 0,
                source_bytes: 0,
            });
        };

        let bpp = target.format.bytes_per_pixel();
        let row_bytes = target.stride_pixels * bpp;
        let span_bytes = rect.width * bpp;
        let src = shadow.buffer();
        for y in rect.y..rect.y + rect.height {
            let start = y * row_bytes + rect.x * bpp;
            let end = start + span_bytes;
            self.buffer[start..end].copy_from_slice(&src[start..end]);
        }
        self.mark_damage(rect);

        Ok(DesktopPresentStats {
            copied_pixels: rect.area(),
            source_bytes: rect.height * span_bytes,
        })
    }

    /// Raw whole-buffer copy. Private on purpose: every caller must go through
    /// `present_desktop_surface`, which validates geometry and stride first.
    fn blit_from(&mut self, src: &[u8]) {
        debug_assert_eq!(src.len(), self.buffer.len());
        let len = src.len().min(self.buffer.len());
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), self.buffer.as_mut_ptr(), len);
        }
    }

    /// Present a desktop pixel buffer into the framebuffer.
    ///
    /// This is the explicit bridge from GUI-host raster output into the
    /// hardware-facing framebuffer path. The contract is intentionally strict:
    /// callers must provide a full-frame surface with dimensions that match the
    /// framebuffer target exactly.
    pub fn present_desktop_surface(
        &mut self,
        surface: DesktopSurface<'_>,
    ) -> Result<DesktopPresentStats, DesktopPresentError> {
        let info = self.info();
        if surface.width != info.width || surface.height != info.height {
            return Err(DesktopPresentError::DimensionMismatch {
                expected_width: info.width,
                expected_height: info.height,
                actual_width: surface.width,
                actual_height: surface.height,
            });
        }

        let expected = surface.required_bytes();
        if surface.pixels.len() != expected {
            return Err(DesktopPresentError::BufferLengthMismatch {
                expected,
                actual: surface.pixels.len(),
            });
        }

        match surface.format {
            DesktopSurfaceFormat::Rgba8888 => {
                self.present_rgba8888(surface.pixels, surface.stride_pixels)
            }
            DesktopSurfaceFormat::NativeRgb32 => {
                if surface.stride_pixels != info.stride_pixels {
                    return Err(DesktopPresentError::StrideMismatch {
                        expected_stride_pixels: info.stride_pixels,
                        actual_stride_pixels: surface.stride_pixels,
                    });
                }
                self.present_native_rgb32(surface.pixels);
            }
        }

        Ok(DesktopPresentStats {
            copied_pixels: surface.width * surface.height,
            source_bytes: surface.pixels.len(),
        })
    }

    /// Clear the screen with a color (optimized with memset-style fill)
    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        let info = self.info();
        let bg_bytes = info.format.to_bytes(r, g, b);

        // Use optimized row fill
        for y in 0..info.height {
            self.fill_pixel_row(y, bg_bytes);
        }
        self.mark_all_damaged();
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
        self.mark_damage(DamageRect::new(0, y_start, info.width, y_end - y_start));
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
        let damage = self.text_cell_rect(col, row, max_len);
        self.mark_damage(damage);

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
        let damage = self.text_cell_rect(col, row, 1);
        self.mark_damage(damage);

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
        let damage = self.text_cell_rect(col, row, max_chars);
        self.mark_damage(damage);

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
        self.mark_damage(DamageRect::new(
            0,
            y_start,
            info.width,
            FONT_HEIGHT.min(info.height.saturating_sub(y_start)),
        ));

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

    fn present_rgba8888(&mut self, pixels: &[u8], stride_pixels: usize) {
        self.present_rgba8888_bands(pixels, stride_pixels, 1, |bands| {
            for band in bands {
                // SAFETY: bands were built from live buffers by `split_bands`.
                unsafe { convert_rgba_rows(band) };
            }
        });
    }

    /// Convert and copy an RGBA surface in up to `workers` row bands. The
    /// caller's `run` must convert every band (on whichever CPUs it likes)
    /// before returning; the buffers stay valid for that call only.
    fn present_rgba8888_bands(
        &mut self,
        pixels: &[u8],
        stride_pixels: usize,
        workers: usize,
        run: impl FnOnce(&[PresentBand]),
    ) {
        let info = self.info();
        let mut bands = [PresentBand::EMPTY; MAX_PRESENT_BANDS];
        let count = split_bands(
            &mut bands,
            pixels.as_ptr(),
            stride_pixels,
            self.buffer.as_mut_ptr(),
            info,
            workers,
        );
        run(&bands[..count]);
        self.mark_all_damaged();
    }

    /// Present a desktop surface, letting the caller spread the pixel
    /// conversion over up to `workers` CPUs. Same contract as
    /// `present_desktop_surface`; native-format surfaces ignore `workers`.
    pub fn present_desktop_surface_with(
        &mut self,
        surface: DesktopSurface<'_>,
        workers: usize,
        run: impl FnOnce(&[PresentBand]),
    ) -> Result<DesktopPresentStats, DesktopPresentError> {
        let info = self.info();
        if surface.width != info.width || surface.height != info.height {
            return Err(DesktopPresentError::DimensionMismatch {
                expected_width: info.width,
                expected_height: info.height,
                actual_width: surface.width,
                actual_height: surface.height,
            });
        }
        let expected = surface.required_bytes();
        if surface.pixels.len() != expected {
            return Err(DesktopPresentError::BufferLengthMismatch {
                expected,
                actual: surface.pixels.len(),
            });
        }
        match surface.format {
            DesktopSurfaceFormat::Rgba8888 => {
                self.present_rgba8888_bands(surface.pixels, surface.stride_pixels, workers, run)
            }
            DesktopSurfaceFormat::NativeRgb32 => {
                if surface.stride_pixels != info.stride_pixels {
                    return Err(DesktopPresentError::StrideMismatch {
                        expected_stride_pixels: info.stride_pixels,
                        actual_stride_pixels: surface.stride_pixels,
                    });
                }
                self.present_native_rgb32(surface.pixels);
            }
        }
        Ok(DesktopPresentStats {
            copied_pixels: surface.width * surface.height,
            source_bytes: surface.pixels.len(),
        })
    }

    fn present_native_rgb32(&mut self, pixels: &[u8]) {
        self.blit_from(pixels);
        self.mark_all_damaged();
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
        self.mark_all_damaged();
    }
}

/// Get bitmap data for a character (8x16 font shared with the rasterizer).
fn get_char_bitmap(ch: u8) -> &'static [u8; 16] {
    let index = ch as usize;
    if index < graphics_rasterizer::FONT_8X16.len() {
        &graphics_rasterizer::FONT_8X16[index]
    } else {
        &graphics_rasterizer::FONT_8X16[0x3F] // '?' for unknown characters
    }
}

/// Most CPUs a present is split across.
pub const MAX_PRESENT_BANDS: usize = 8;

/// A horizontal band of an RGBA -> framebuffer conversion, self-contained
/// so another CPU can run it from raw pointers.
#[derive(Debug, Clone, Copy)]
pub struct PresentBand {
    pub src: *const u8,
    pub src_stride_pixels: usize,
    pub dst: *mut u8,
    pub dst_stride_bytes: usize,
    pub width: usize,
    pub y0: usize,
    pub y1: usize,
    pub format: PixelFormat,
}

// SAFETY: a band is only handed to another CPU for the duration of one
// present, during which both buffers are exclusively borrowed by the caller.
unsafe impl Send for PresentBand {}

impl PresentBand {
    pub const EMPTY: Self = Self {
        src: core::ptr::null(),
        src_stride_pixels: 0,
        dst: core::ptr::null_mut(),
        dst_stride_bytes: 0,
        width: 0,
        y0: 0,
        y1: 0,
        format: PixelFormat::Rgb32,
    };

    pub fn rows(&self) -> usize {
        self.y1.saturating_sub(self.y0)
    }
}

/// Split the frame into up to `workers` bands of roughly equal height.
/// Returns how many bands were written (0 when the frame is empty).
fn split_bands(
    bands: &mut [PresentBand; MAX_PRESENT_BANDS],
    src: *const u8,
    src_stride_pixels: usize,
    dst: *mut u8,
    info: FramebufferInfo,
    workers: usize,
) -> usize {
    let workers = workers.clamp(1, MAX_PRESENT_BANDS).min(info.height.max(1));
    if info.height == 0 || info.width == 0 {
        return 0;
    }
    let rows_per = info.height.div_ceil(workers);
    let mut count = 0;
    let mut y = 0;
    while y < info.height && count < workers {
        let y1 = (y + rows_per).min(info.height);
        bands[count] = PresentBand {
            src,
            src_stride_pixels,
            dst,
            dst_stride_bytes: info.stride_pixels * info.format.bytes_per_pixel(),
            width: info.width,
            y0: y,
            y1,
            format: info.format,
        };
        count += 1;
        y = y1;
    }
    count
}

/// Convert one band of RGBA8888 rows into the framebuffer's format.
///
/// # Safety
/// `band.src` must cover rows `y0..y1` at `src_stride_pixels`, and
/// `band.dst` rows `y0..y1` at `dst_stride_bytes`, for the whole call.
pub unsafe fn convert_rgba_rows(band: &PresentBand) {
    let PixelFormat::Rgb32 = band.format;
    for y in band.y0..band.y1 {
        let src_row = band.src.add(y * band.src_stride_pixels * 4);
        let dst_row = band.dst.add(y * band.dst_stride_bytes) as *mut u32;
        for x in 0..band.width {
            let px = src_row.add(x * 4);
            let r = *px as u32;
            let g = *px.add(1) as u32;
            let b = *px.add(2) as u32;
            core::ptr::write_volatile(dst_row.add(x), (r << 16) | (g << 8) | b);
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use services_gui_host::{Compositor, DesktopWindow, SurfaceRect, SurfaceSize};
    use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

    fn test_framebuffer(info: FramebufferInfo, fill: u8) -> BareMetalFramebuffer {
        let buffer = vec![fill; info.buffer_size()].into_boxed_slice();
        let leaked = Box::leak(buffer);
        unsafe { BareMetalFramebuffer::from_info_and_buffer(info, leaked) }
    }

    #[test]
    fn test_present_desktop_surface_converts_rgba8888_to_framebuffer_layout() {
        let info = FramebufferInfo {
            width: 2,
            height: 2,
            stride_pixels: 3,
            format: PixelFormat::Rgb32,
        };
        let mut framebuffer = test_framebuffer(info, 0xAA);
        let pixels = [
            10, 20, 30, 255, 40, 50, 60, 200, 70, 80, 90, 128, 100, 110, 120, 0,
        ];

        let stats = framebuffer
            .present_desktop_surface(DesktopSurface::rgba8888(2, 2, &pixels))
            .expect("desktop surface should present");

        assert_eq!(
            stats,
            DesktopPresentStats {
                copied_pixels: 4,
                source_bytes: 16,
            }
        );
        let bytes = framebuffer.buffer_mut();
        assert_eq!(&bytes[0..8], &[30, 20, 10, 0, 60, 50, 40, 0]);
        assert_eq!(&bytes[8..12], &[0xAA, 0xAA, 0xAA, 0xAA]);
        assert_eq!(&bytes[12..20], &[90, 80, 70, 0, 120, 110, 100, 0]);
        assert_eq!(&bytes[20..24], &[0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn test_present_shadow_copies_backbuffer_into_target() {
        let info = FramebufferInfo {
            width: 3,
            height: 2,
            stride_pixels: 4,
            format: PixelFormat::Rgb32,
        };
        let mut target = test_framebuffer(info, 0x00);
        let mut shadow = test_framebuffer(info, 0x00);
        shadow.clear(0x10, 0x20, 0x30);
        shadow.draw_char_at(0, 0, b'A', (0xFF, 0xFF, 0xFF), (0x10, 0x20, 0x30));

        let stats = target
            .present_shadow(&shadow)
            .expect("matching shadow should present");

        assert_eq!(
            stats,
            DesktopPresentStats {
                copied_pixels: 6,
                source_bytes: info.buffer_size(),
            }
        );
        assert_eq!(target.buffer(), shadow.buffer());
    }

    #[test]
    fn test_present_shadow_rejects_geometry_mismatch() {
        let target_info = FramebufferInfo {
            width: 4,
            height: 4,
            stride_pixels: 4,
            format: PixelFormat::Rgb32,
        };
        let shadow_info = FramebufferInfo {
            width: 4,
            height: 4,
            stride_pixels: 8,
            format: PixelFormat::Rgb32,
        };
        let mut target = test_framebuffer(target_info, 0x55);
        let shadow = test_framebuffer(shadow_info, 0xEE);

        let err = target
            .present_shadow(&shadow)
            .expect_err("stride mismatch must be rejected");
        assert_eq!(
            err,
            DesktopPresentError::StrideMismatch {
                expected_stride_pixels: 4,
                actual_stride_pixels: 8,
            }
        );
        // Target must be untouched after a rejected present.
        assert!(target.buffer().iter().all(|b| *b == 0x55));

        let smaller = test_framebuffer(
            FramebufferInfo {
                width: 2,
                height: 2,
                stride_pixels: 4,
                format: PixelFormat::Rgb32,
            },
            0xEE,
        );
        assert!(matches!(
            target.present_shadow(&smaller),
            Err(DesktopPresentError::DimensionMismatch { .. })
        ));
        assert!(target.buffer().iter().all(|b| *b == 0x55));
    }

    #[test]
    fn test_damage_rect_union_and_intersect() {
        let a = DamageRect::new(2, 2, 4, 4);
        let b = DamageRect::new(5, 1, 2, 2);
        assert_eq!(a.union(b), DamageRect::new(2, 1, 5, 5));
        assert_eq!(a.union(DamageRect::new(0, 0, 0, 0)), a);
        assert_eq!(a.intersect(b), Some(DamageRect::new(5, 2, 1, 1)));
        assert_eq!(a.intersect(DamageRect::new(10, 10, 1, 1)), None);
        assert_eq!(
            a.intersect(DamageRect::new(0, 0, 100, 3)),
            Some(DamageRect::new(2, 2, 4, 1))
        );
    }

    #[test]
    fn test_draw_calls_accumulate_tight_damage() {
        let info = FramebufferInfo {
            width: 64,
            height: 48,
            stride_pixels: 64,
            format: PixelFormat::Rgb32,
        };
        let mut fb = test_framebuffer(info, 0);
        assert_eq!(fb.damage(), None);

        fb.draw_char_at(1, 1, b'x', (255, 255, 255), (0, 0, 0));
        assert_eq!(fb.damage(), Some(DamageRect::new(8, 16, 8, 16)));

        fb.draw_text_at(3, 2, "abcd", (255, 255, 255), (0, 0, 0));
        assert_eq!(fb.damage(), Some(DamageRect::new(8, 16, 48, 32)));

        assert_eq!(fb.take_damage(), Some(DamageRect::new(8, 16, 48, 32)));
        assert_eq!(fb.damage(), None);

        fb.clear_text_span(0, 0, 2, (0, 0, 0));
        assert_eq!(fb.damage(), Some(DamageRect::new(0, 0, 16, 16)));
        fb.draw_line(2, "hi", (255, 255, 255), (0, 0, 0));
        assert_eq!(fb.damage(), Some(DamageRect::new(0, 0, 64, 48)));

        fb.take_damage();
        fb.scroll_up_text_lines(1, (0, 0, 0));
        assert_eq!(fb.damage(), Some(fb.full_rect()));
        fb.take_damage();
        fb.clear(1, 2, 3);
        assert_eq!(fb.damage(), Some(fb.full_rect()));
    }

    #[test]
    fn test_present_shadow_damage_copies_only_damaged_region() {
        let info = FramebufferInfo {
            width: 32,
            height: 32,
            stride_pixels: 40,
            format: PixelFormat::Rgb32,
        };
        let mut target = test_framebuffer(info, 0x55);
        let mut shadow = test_framebuffer(info, 0x00);
        shadow.clear(0x10, 0x20, 0x30);
        shadow.take_damage();
        shadow.draw_char_at(1, 1, b'A', (0xFF, 0xFF, 0xFF), (0x10, 0x20, 0x30));

        let stats = target
            .present_shadow_damage(&mut shadow)
            .expect("damage present should succeed");
        assert_eq!(stats.copied_pixels, 8 * 16);
        assert_eq!(stats.source_bytes, 16 * 8 * 4);
        assert_eq!(shadow.damage(), None, "present consumes shadow damage");

        let row_bytes = info.stride_pixels * 4;
        for y in 0..info.height {
            for x in 0..info.stride_pixels {
                let off = y * row_bytes + x * 4;
                let inside = (8..16).contains(&x) && (16..32).contains(&y);
                let expected = if inside {
                    &shadow.buffer()[off..off + 4]
                } else {
                    &[0x55u8; 4][..]
                };
                assert_eq!(&target.buffer()[off..off + 4], expected, "x={x} y={y}");
            }
        }

        // No damage -> nothing copied, target untouched.
        target.take_damage();
        let stats = target
            .present_shadow_damage(&mut shadow)
            .expect("clean shadow should present as no-op");
        assert_eq!(stats.copied_pixels, 0);
        assert_eq!(target.damage(), None);
    }

    #[test]
    fn test_present_shadow_damage_rejects_geometry_mismatch_and_consumes_damage() {
        let target_info = FramebufferInfo {
            width: 8,
            height: 8,
            stride_pixels: 8,
            format: PixelFormat::Rgb32,
        };
        let shadow_info = FramebufferInfo {
            width: 8,
            height: 8,
            stride_pixels: 16,
            format: PixelFormat::Rgb32,
        };
        let mut target = test_framebuffer(target_info, 0x55);
        let mut shadow = test_framebuffer(shadow_info, 0x00);
        shadow.clear(1, 1, 1);
        assert!(matches!(
            target.present_shadow_damage(&mut shadow),
            Err(DesktopPresentError::StrideMismatch { .. })
        ));
        assert!(target.buffer().iter().all(|b| *b == 0x55));
        assert_eq!(shadow.damage(), None);
    }

    #[test]
    fn test_present_desktop_surface_rejects_dimension_mismatch() {
        let info = FramebufferInfo {
            width: 4,
            height: 4,
            stride_pixels: 4,
            format: PixelFormat::Rgb32,
        };
        let mut framebuffer = test_framebuffer(info, 0);
        let pixels = [0u8; 4 * 4 * 4];

        let err = framebuffer
            .present_desktop_surface(DesktopSurface::rgba8888(2, 4, &pixels))
            .expect_err("mismatched dimensions should fail");

        assert_eq!(
            err,
            DesktopPresentError::DimensionMismatch {
                expected_width: 4,
                expected_height: 4,
                actual_width: 2,
                actual_height: 4,
            }
        );
    }

    #[test]
    fn test_present_desktop_surface_rejects_invalid_buffer_length() {
        let info = FramebufferInfo {
            width: 2,
            height: 2,
            stride_pixels: 2,
            format: PixelFormat::Rgb32,
        };
        let mut framebuffer = test_framebuffer(info, 0);
        let pixels = [0u8; 12];

        let err = framebuffer
            .present_desktop_surface(DesktopSurface::rgba8888(2, 2, &pixels))
            .expect_err("short pixel buffer should fail");

        assert_eq!(
            err,
            DesktopPresentError::BufferLengthMismatch {
                expected: 16,
                actual: 12,
            }
        );
    }

    #[test]
    fn test_present_desktop_surface_accepts_gui_host_raster_frame() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["A".to_string()]),
            10,
        )
        .with_title("Main");
        let surface = compositor.compose_desktop_rgba(
            SurfaceSize::new(6, 4),
            vec![DesktopWindow::new(frame, SurfaceRect::new(1, 1, 3, 2)).focused()],
        );

        let info = FramebufferInfo {
            width: surface.width,
            height: surface.height,
            stride_pixels: surface.width,
            format: PixelFormat::Rgb32,
        };
        let mut framebuffer = test_framebuffer(info, 0);

        let stats = framebuffer
            .present_desktop_surface(DesktopSurface::rgba8888(
                surface.width,
                surface.height,
                &surface.pixels,
            ))
            .expect("GUI host surface should present");

        assert_eq!(stats.copied_pixels, surface.width * surface.height);
        assert_eq!(stats.source_bytes, surface.pixels.len());
        assert_eq!(
            &framebuffer.buffer_mut()[0..4],
            &[0x1C, 0x12, 0x0C, 0x00],
            "background pixel should convert into framebuffer layout"
        );
    }

    #[test]
    fn test_present_desktop_surface_accepts_native_backbuffer_surface() {
        let info = FramebufferInfo {
            width: 2,
            height: 2,
            stride_pixels: 3,
            format: PixelFormat::Rgb32,
        };
        let mut framebuffer = test_framebuffer(info, 0);
        let native_pixels = [
            1, 2, 3, 0, 4, 5, 6, 0, 0xAA, 0xAA, 0xAA, 0xAA, 7, 8, 9, 0, 10, 11, 12, 0, 0xBB, 0xBB,
            0xBB, 0xBB,
        ];

        let stats = framebuffer
            .present_desktop_surface(DesktopSurface::native_rgb32(2, 2, 3, &native_pixels))
            .expect("native backbuffer surface should present");

        assert_eq!(stats.copied_pixels, 4);
        assert_eq!(stats.source_bytes, native_pixels.len());
        assert_eq!(framebuffer.buffer_mut(), &native_pixels);
    }

    #[test]
    fn test_present_desktop_surface_rejects_native_stride_mismatch() {
        let info = FramebufferInfo {
            width: 2,
            height: 2,
            stride_pixels: 3,
            format: PixelFormat::Rgb32,
        };
        let mut framebuffer = test_framebuffer(info, 0);
        let native_pixels = [0u8; 16];

        let err = framebuffer
            .present_desktop_surface(DesktopSurface::native_rgb32(2, 2, 2, &native_pixels))
            .expect_err("native stride mismatch should fail");

        assert_eq!(
            err,
            DesktopPresentError::StrideMismatch {
                expected_stride_pixels: 3,
                actual_stride_pixels: 2,
            }
        );
    }
}

#[cfg(test)]
mod band_tests {
    use super::*;
    extern crate alloc;
    use alloc::vec;

    fn info(width: usize, height: usize, stride: usize) -> FramebufferInfo {
        FramebufferInfo {
            width,
            height,
            stride_pixels: stride,
            format: PixelFormat::Rgb32,
        }
    }

    #[test]
    fn bands_cover_all_rows_without_overlap() {
        let mut bands = [PresentBand::EMPTY; MAX_PRESENT_BANDS];
        for (height, workers) in [(800, 4), (800, 3), (5, 8), (1, 4), (7, 1)] {
            let n = split_bands(
                &mut bands,
                core::ptr::null(),
                1,
                core::ptr::null_mut(),
                info(4, height, 4),
                workers,
            );
            assert!(n >= 1 && n <= workers.min(height));
            assert_eq!(bands[0].y0, 0);
            for i in 1..n {
                assert_eq!(bands[i].y0, bands[i - 1].y1);
            }
            assert_eq!(bands[n - 1].y1, height);
            assert_eq!(bands[..n].iter().map(|b| b.rows()).sum::<usize>(), height);
        }
    }

    #[test]
    fn empty_frame_yields_no_bands() {
        let mut bands = [PresentBand::EMPTY; MAX_PRESENT_BANDS];
        let n = split_bands(
            &mut bands,
            core::ptr::null(),
            1,
            core::ptr::null_mut(),
            info(0, 0, 0),
            4,
        );
        assert_eq!(n, 0);
    }

    #[test]
    fn convert_matches_pixel_format_and_respects_strides() {
        let width = 3;
        let height = 4;
        let src_stride = 5;
        let dst_stride_bytes = 6 * 4;
        let mut src = vec![0u8; src_stride * 4 * height];
        for y in 0..height {
            for x in 0..width {
                let o = (y * src_stride + x) * 4;
                src[o] = (x * 10) as u8;
                src[o + 1] = (y * 10) as u8;
                src[o + 2] = 7;
                src[o + 3] = 255;
            }
        }
        let mut dst = vec![0xAAu8; dst_stride_bytes * height];
        let mut bands = [PresentBand::EMPTY; MAX_PRESENT_BANDS];
        let n = split_bands(
            &mut bands,
            src.as_ptr(),
            src_stride,
            dst.as_mut_ptr(),
            info(width, height, 6),
            2,
        );
        assert_eq!(n, 2);
        for band in &bands[..n] {
            unsafe { convert_rgba_rows(band) };
        }
        for y in 0..height {
            for x in 0..width {
                let o = y * dst_stride_bytes + x * 4;
                let expected = PixelFormat::Rgb32.to_bytes((x * 10) as u8, (y * 10) as u8, 7);
                assert_eq!(&dst[o..o + 4], &expected, "pixel ({x},{y})");
            }
            // padding pixels untouched
            assert_eq!(dst[y * dst_stride_bytes + width * 4], 0xAA);
        }
    }
}
