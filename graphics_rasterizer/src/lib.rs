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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
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
        let offset = self.offset(x, y)?;
        Some(RgbaColor::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }

    pub fn clear(&mut self, color: RgbaColor) {
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[color.r, color.g, color.b, color.a]);
        }
    }

    pub fn fill_rect(&mut self, rect: RasterRect, color: RgbaColor) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        let x_end = rect.x.saturating_add(rect.width).min(self.width);
        let y_end = rect.y.saturating_add(rect.height).min(self.height);
        for y in rect.y.min(self.height)..y_end {
            for x in rect.x.min(self.width)..x_end {
                self.set_pixel(x, y, color);
            }
        }
    }

    pub fn draw_border(&mut self, rect: RasterRect, thickness: usize, color: RgbaColor) {
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

    pub fn draw_text(&mut self, x: usize, y: usize, text: &str, color: RgbaColor) {
        let mut cursor_x = x;
        for ch in text.chars() {
            self.draw_glyph(cursor_x, y, glyph_for(ch), color);
            cursor_x = cursor_x.saturating_add(GLYPH_WIDTH + 1);
            if cursor_x >= self.width {
                break;
            }
        }
    }

    fn draw_glyph(&mut self, x: usize, y: usize, glyph: [u8; GLYPH_HEIGHT], color: RgbaColor) {
        for (row, pattern) in glyph.iter().enumerate() {
            let y = y + row;
            if y >= self.height {
                break;
            }

            for column in 0..GLYPH_WIDTH {
                let mask = 1 << (GLYPH_WIDTH - 1 - column);
                if pattern & mask != 0 {
                    self.set_pixel(x + column, y, color);
                }
            }
        }
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

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;

fn glyph_for(ch: char) -> [u8; GLYPH_HEIGHT] {
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

        buffer.draw_text(2, 2, "Ab?", ACCENT);

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
}
