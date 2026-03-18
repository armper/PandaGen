//! Deterministic software rasterizer primitives for PandaGen graphics.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RasterRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl RasterRect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(&self) -> usize {
        self.x + self.width
    }

    pub const fn bottom(&self) -> usize {
        self.y + self.height
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersect(&self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return None;
        }
        Some(Self::new(x, y, right - x, bottom - y))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitmapFont {
    glyph_width: usize,
    glyph_height: usize,
    advance_x: usize,
}

impl BitmapFont {
    pub const fn new(glyph_width: usize, glyph_height: usize, advance_x: usize) -> Self {
        Self {
            glyph_width,
            glyph_height,
            advance_x,
        }
    }

    pub const fn glyph_width(&self) -> usize {
        self.glyph_width
    }

    pub const fn glyph_height(&self) -> usize {
        self.glyph_height
    }

    pub const fn advance_x(&self) -> usize {
        self.advance_x
    }

    pub fn measure_text(&self, text: &str) -> (usize, usize) {
        (text.chars().count() * self.advance_x, self.glyph_height)
    }
}

pub const COMPACT_FONT: BitmapFont = BitmapFont::new(5, 7, 6);
pub const DESKTOP_FONT: BitmapFont = BitmapFont::new(8, 8, 9);

pub trait RenderTarget {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn write_pixel(&mut self, x: usize, y: usize, color: RgbaColor);
    fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor>;

    fn clear(&mut self, color: RgbaColor) {
        for y in 0..self.height() {
            for x in 0..self.width() {
                self.write_pixel(x, y, color);
            }
        }
    }

    fn fill_rect(&mut self, rect: RasterRect, color: RgbaColor) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        let x_end = rect.x.saturating_add(rect.width).min(self.width());
        let y_end = rect.y.saturating_add(rect.height).min(self.height());
        for y in rect.y.min(self.height())..y_end {
            for x in rect.x.min(self.width())..x_end {
                self.write_pixel(x, y, color);
            }
        }
    }

    fn draw_border(&mut self, rect: RasterRect, thickness: usize, color: RgbaColor) {
        if thickness == 0 || rect.width == 0 || rect.height == 0 {
            return;
        }

        let thickness = thickness.min(rect.width).min(rect.height);
        self.fill_rect(
            RasterRect::new(rect.x, rect.y, rect.width, thickness),
            color,
        );
        self.fill_rect(
            RasterRect::new(
                rect.x,
                rect.y + rect.height.saturating_sub(thickness),
                rect.width,
                thickness,
            ),
            color,
        );
        self.fill_rect(
            RasterRect::new(rect.x, rect.y, thickness, rect.height),
            color,
        );
        self.fill_rect(
            RasterRect::new(
                rect.x + rect.width.saturating_sub(thickness),
                rect.y,
                thickness,
                rect.height,
            ),
            color,
        );
    }

    fn draw_text(&mut self, x: usize, y: usize, text: &str, color: RgbaColor) {
        self.draw_text_with_font(x, y, text, &DESKTOP_FONT, color);
    }

    fn draw_text_with_font(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        font: &BitmapFont,
        color: RgbaColor,
    ) {
        let mut cursor_x = x;
        for ch in text.chars() {
            draw_glyph(self, cursor_x, y, font, ch, color);
            cursor_x = cursor_x.saturating_add(font.advance_x());
            if cursor_x >= self.width() {
                break;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearPixelFormat {
    Rgb32,
    Bgr32,
}

impl LinearPixelFormat {
    const fn bytes_per_pixel(self) -> usize {
        4
    }

    fn encode(self, color: RgbaColor) -> [u8; 4] {
        match self {
            Self::Rgb32 => [color.b, color.g, color.r, 0],
            Self::Bgr32 => [color.r, color.g, color.b, 0],
        }
    }

    fn decode(self, bytes: [u8; 4]) -> RgbaColor {
        match self {
            Self::Rgb32 => RgbaColor::new(bytes[2], bytes[1], bytes[0], 255),
            Self::Bgr32 => RgbaColor::new(bytes[0], bytes[1], bytes[2], 255),
        }
    }
}

pub struct LinearFramebufferTarget<'a> {
    width: usize,
    height: usize,
    stride_pixels: usize,
    format: LinearPixelFormat,
    buffer: &'a mut [u8],
}

pub struct ScissorTarget<'a, T: RenderTarget + ?Sized> {
    target: &'a mut T,
    scissor: RasterRect,
}

impl<'a, T: RenderTarget + ?Sized> ScissorTarget<'a, T> {
    pub fn new(target: &'a mut T, scissor: RasterRect) -> Self {
        Self { target, scissor }
    }
}

impl<'a> LinearFramebufferTarget<'a> {
    pub fn new(
        width: usize,
        height: usize,
        stride_pixels: usize,
        format: LinearPixelFormat,
        buffer: &'a mut [u8],
    ) -> Self {
        let required = height
            .saturating_mul(stride_pixels)
            .saturating_mul(format.bytes_per_pixel());
        assert!(
            buffer.len() >= required,
            "framebuffer target requires at least {required} bytes, got {}",
            buffer.len()
        );

        Self {
            width,
            height,
            stride_pixels,
            format,
            buffer,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.buffer
    }

    fn offset(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some((y * self.stride_pixels + x) * self.format.bytes_per_pixel())
    }
}

impl RgbaBuffer {
    pub fn new(width: usize, height: usize, clear: RgbaColor) -> Self {
        let mut pixels = vec![0; width.saturating_mul(height).saturating_mul(4)];
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[clear.r, clear.g, clear.b, clear.a]);
        }

        Self {
            width,
            height,
            pixels,
        }
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        <Self as RenderTarget>::pixel(self, x, y)
    }

    pub fn clear(&mut self, color: RgbaColor) {
        <Self as RenderTarget>::clear(self, color)
    }

    pub fn fill_rect(&mut self, rect: RasterRect, color: RgbaColor) {
        <Self as RenderTarget>::fill_rect(self, rect, color)
    }

    pub fn draw_border(&mut self, rect: RasterRect, thickness: usize, color: RgbaColor) {
        <Self as RenderTarget>::draw_border(self, rect, thickness, color)
    }

    pub fn draw_text(&mut self, x: usize, y: usize, text: &str, color: RgbaColor) {
        <Self as RenderTarget>::draw_text(self, x, y, text, color)
    }

    pub fn draw_text_with_font(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        font: &BitmapFont,
        color: RgbaColor,
    ) {
        <Self as RenderTarget>::draw_text_with_font(self, x, y, text, font, color)
    }

    fn offset(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some((y * self.width + x) * 4)
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        let Some(offset) = self.offset(x, y) else {
            return;
        };
        self.pixels[offset..offset + 4].copy_from_slice(&[color.r, color.g, color.b, color.a]);
    }
}

impl RenderTarget for RgbaBuffer {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        self.set_pixel(x, y, color);
    }

    fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        let offset = self.offset(x, y)?;
        Some(RgbaColor::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }
}

impl RenderTarget for LinearFramebufferTarget<'_> {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        let Some(offset) = self.offset(x, y) else {
            return;
        };
        self.buffer[offset..offset + 4].copy_from_slice(&self.format.encode(color));
    }

    fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        let offset = self.offset(x, y)?;
        let bytes = [
            self.buffer[offset],
            self.buffer[offset + 1],
            self.buffer[offset + 2],
            self.buffer[offset + 3],
        ];
        Some(self.format.decode(bytes))
    }
}

impl<T: RenderTarget + ?Sized> RenderTarget for ScissorTarget<'_, T> {
    fn width(&self) -> usize {
        self.target.width()
    }

    fn height(&self) -> usize {
        self.target.height()
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        if self.scissor.contains(x, y) {
            self.target.write_pixel(x, y, color);
        }
    }

    fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        if self.scissor.contains(x, y) {
            self.target.pixel(x, y)
        } else {
            None
        }
    }
}

const SOURCE_GLYPH_WIDTH: usize = 5;
const SOURCE_GLYPH_HEIGHT: usize = 7;
const MAX_GLYPH_HEIGHT: usize = 16;

fn draw_glyph(
    target: &mut (impl RenderTarget + ?Sized),
    x: usize,
    y: usize,
    font: &BitmapFont,
    ch: char,
    color: RgbaColor,
) {
    let glyph = rasterize_glyph(font, ch);
    for (row, pattern) in glyph.iter().take(font.glyph_height()).enumerate() {
        let y = y + row;
        if y >= target.height() {
            break;
        }

        for column in 0..font.glyph_width() {
            let mask = 1 << (font.glyph_width() - 1 - column);
            if pattern & mask != 0 {
                target.write_pixel(x + column, y, color);
            }
        }
    }
}

fn rasterize_glyph(font: &BitmapFont, ch: char) -> [u16; MAX_GLYPH_HEIGHT] {
    let source = compact_glyph_for(ch);
    let mut rows = [0u16; MAX_GLYPH_HEIGHT];

    for target_y in 0..font.glyph_height() {
        let source_y = target_y * SOURCE_GLYPH_HEIGHT / font.glyph_height();
        let source_row = source[source_y];
        let mut row = 0u16;

        for target_x in 0..font.glyph_width() {
            let source_x = target_x * SOURCE_GLYPH_WIDTH / font.glyph_width();
            let bit = (source_row >> (SOURCE_GLYPH_WIDTH - 1 - source_x)) & 1;
            if bit != 0 {
                row |= 1 << (font.glyph_width() - 1 - target_x);
            }
        }

        rows[target_y] = row;
    }

    rows
}

fn compact_glyph_for(ch: char) -> [u8; SOURCE_GLYPH_HEIGHT] {
    match ch {
        'A' | 'a' => [
            0b00100, 0b01010, 0b11111, 0b10001, 0b10001, 0b10001, 0b00000,
        ],
        'B' | 'b' => [
            0b11110, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110, 0b00000,
        ],
        'C' | 'c' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10001, 0b01110, 0b00000,
        ],
        'D' | 'd' => [
            0b11100, 0b10010, 0b10001, 0b10001, 0b10010, 0b11100, 0b00000,
        ],
        'E' | 'e' => [
            0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111, 0b00000,
        ],
        'F' | 'f' => [
            0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b00000,
        ],
        'G' | 'g' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b01111, 0b00000,
        ],
        'H' | 'h' => [
            0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b00000,
        ],
        'I' | 'i' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111, 0b00000,
        ],
        'J' | 'j' => [
            0b00111, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100, 0b00000,
        ],
        'K' | 'k' => [
            0b10001, 0b10010, 0b11100, 0b10010, 0b10001, 0b10001, 0b00000,
        ],
        'L' | 'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111, 0b00000,
        ],
        'M' | 'm' => [
            0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b00000,
        ],
        'N' | 'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b00000,
        ],
        'O' | 'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000,
        ],
        'P' | 'p' => [
            0b11110, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000, 0b00000,
        ],
        'Q' | 'q' => [
            0b01110, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101, 0b00000,
        ],
        'R' | 'r' => [
            0b11110, 0b10001, 0b11110, 0b10010, 0b10001, 0b10001, 0b00000,
        ],
        'S' | 's' => [
            0b01111, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110, 0b00000,
        ],
        'T' | 't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000,
        ],
        'U' | 'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000,
        ],
        'V' | 'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00000,
        ],
        'W' | 'w' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b11011, 0b10001, 0b00000,
        ],
        'X' | 'x' => [
            0b10001, 0b01010, 0b00100, 0b00100, 0b01010, 0b10001, 0b00000,
        ],
        'Y' | 'y' => [
            0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000,
        ],
        'Z' | 'z' => [
            0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111, 0b00000,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110, 0b00000,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110, 0b00000,
        ],
        '2' => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b01000, 0b11111, 0b00000,
        ],
        '3' => [
            0b11110, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110, 0b00000,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b11111, 0b00010, 0b00010, 0b00000,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b10001, 0b01110, 0b00000,
        ],
        '6' => [
            0b00110, 0b01000, 0b11110, 0b10001, 0b10001, 0b01110, 0b00000,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b00000,
        ],
        '8' => [
            0b01110, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110, 0b00000,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00010, 0b01100, 0b00000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110, 0b00000,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110, 0b00000,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b00100, 0b00010, 0b00000,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00100, 0b01000, 0b00000,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '#' => [
            0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000,
        ],
        ':' => [
            0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '?' => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00000, 0b00100, 0b00000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100, 0b00000,
        ],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00000, 0b00100, 0b00000,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAR: RgbaColor = RgbaColor::new(5, 10, 15, 255);
    const ACCENT: RgbaColor = RgbaColor::new(200, 100, 50, 255);
    const DETAIL: RgbaColor = RgbaColor::new(20, 220, 180, 255);

    fn paint_sample(target: &mut impl RenderTarget) {
        target.clear(CLEAR);
        target.fill_rect(RasterRect::new(1, 1, 6, 4), ACCENT);
        target.draw_border(RasterRect::new(0, 0, 8, 6), 1, DETAIL);
        target.draw_text(2, 2, "Ab", DETAIL);
    }

    #[test]
    fn test_fill_rect_clips_to_buffer_bounds() {
        let mut buffer = RgbaBuffer::new(4, 4, CLEAR);

        buffer.fill_rect(RasterRect::new(2, 1, 4, 3), ACCENT);

        assert_eq!(buffer.pixel(1, 1), Some(CLEAR));
        assert_eq!(buffer.pixel(2, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(3, 3), Some(ACCENT));
        assert_eq!(buffer.pixel(0, 3), Some(CLEAR));
    }

    #[test]
    fn test_draw_border_respects_thickness_without_filling_center() {
        let mut buffer = RgbaBuffer::new(8, 8, CLEAR);

        buffer.draw_border(RasterRect::new(1, 1, 6, 6), 2, ACCENT);

        assert_eq!(buffer.pixel(1, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(3, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(1, 4), Some(ACCENT));
        assert_eq!(buffer.pixel(3, 3), Some(CLEAR));
        assert_eq!(buffer.pixel(6, 6), Some(ACCENT));
    }

    #[test]
    fn test_draw_text_renders_supported_glyph_pixels() {
        let mut buffer = RgbaBuffer::new(24, 12, CLEAR);

        buffer.draw_text_with_font(2, 2, "Ab?", &COMPACT_FONT, ACCENT);

        assert_eq!(buffer.pixel(4, 2), Some(ACCENT));
        assert_eq!(buffer.pixel(2, 4), Some(ACCENT));
        assert_eq!(buffer.pixel(10, 4), Some(ACCENT));
        assert_eq!(buffer.pixel(16, 2), Some(ACCENT));
        assert_eq!(buffer.pixel(20, 10), Some(CLEAR));
    }

    #[test]
    fn test_clear_replaces_existing_pixels() {
        let mut buffer = RgbaBuffer::new(3, 2, CLEAR);
        buffer.fill_rect(RasterRect::new(0, 0, 3, 2), ACCENT);

        buffer.clear(CLEAR);

        assert_eq!(buffer.pixel(0, 0), Some(CLEAR));
        assert_eq!(buffer.pixel(2, 1), Some(CLEAR));
    }

    #[test]
    fn test_linear_framebuffer_target_matches_rgba_buffer_for_same_draw_ops() {
        let mut rgba = RgbaBuffer::new(8, 6, RgbaColor::new(0, 0, 0, 0));
        paint_sample(&mut rgba);

        let mut bytes = vec![0; 8 * 6 * 4];
        let mut framebuffer =
            LinearFramebufferTarget::new(8, 6, 8, LinearPixelFormat::Rgb32, &mut bytes);
        paint_sample(&mut framebuffer);

        for y in 0..6 {
            for x in 0..8 {
                assert_eq!(
                    framebuffer.pixel(x, y),
                    rgba.pixel(x, y),
                    "pixel mismatch at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn test_linear_framebuffer_target_respects_stride_and_rgb32_layout() {
        let mut bytes = vec![0xAA; 4 * 3 * 4];
        let mut framebuffer =
            LinearFramebufferTarget::new(3, 2, 4, LinearPixelFormat::Rgb32, &mut bytes);

        framebuffer.clear(CLEAR);
        framebuffer.write_pixel(1, 0, ACCENT);

        let offset = 4;
        assert_eq!(
            &framebuffer.as_bytes()[offset..offset + 4],
            &[ACCENT.b, ACCENT.g, ACCENT.r, 0]
        );
        let padding_offset = 3 * 4;
        assert_eq!(
            &framebuffer.as_bytes()[padding_offset..padding_offset + 4],
            &[0xAA, 0xAA, 0xAA, 0xAA]
        );
        assert_eq!(framebuffer.pixel(1, 0), Some(ACCENT));
        assert_eq!(framebuffer.pixel(2, 1), Some(CLEAR));
    }

    #[test]
    fn test_desktop_font_reports_readable_metrics() {
        assert_eq!(DESKTOP_FONT.measure_text("Ab"), (18, 8));
    }

    #[test]
    fn test_draw_text_with_desktop_font_preserves_glyph_gap() {
        let mut buffer = RgbaBuffer::new(32, 12, CLEAR);

        buffer.draw_text_with_font(1, 1, "II", &DESKTOP_FONT, ACCENT);

        assert_eq!(buffer.pixel(9, 1), Some(CLEAR));
        assert_eq!(buffer.pixel(10, 1), Some(ACCENT));
    }

    #[test]
    fn test_scissor_target_clips_fill_and_text() {
        let mut buffer = RgbaBuffer::new(12, 8, CLEAR);
        {
            let mut clipped = ScissorTarget::new(&mut buffer, RasterRect::new(2, 1, 4, 3));
            clipped.fill_rect(RasterRect::new(0, 0, 12, 8), ACCENT);
            clipped.draw_text_with_font(0, 1, "AB", &COMPACT_FONT, DETAIL);
        }

        assert_eq!(buffer.pixel(1, 1), Some(CLEAR));
        assert_eq!(buffer.pixel(5, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(2, 1), Some(DETAIL));
        assert_eq!(buffer.pixel(6, 2), Some(CLEAR));
        assert_eq!(buffer.pixel(3, 4), Some(CLEAR));
    }
}
