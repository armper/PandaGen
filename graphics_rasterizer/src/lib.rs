//! Deterministic software rasterizer primitives for PandaGen graphics.
//!
//! The crate is `no_std` + `alloc` so the same pixel logic runs under host
//! tests and inside the bare-metal kernel image.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
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

/// Which glyph bitmaps a `BitmapFont` samples from.
/// Owned RGBA8888 image (icon, sprite, decoded picture) for blitting.
///
/// Pixels are tightly packed `[r, g, b, a]` rows, top to bottom. Alpha is
/// honoured on blit: 0 skips the pixel, 255 overwrites, anything between
/// blends source-over onto what the target already holds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaImage {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

/// Error building an image from raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// `bytes.len()` did not equal `width * height * 4`.
    LengthMismatch { expected: usize, actual: usize },
}

impl RgbaImage {
    /// Fully transparent image.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width.saturating_mul(height).saturating_mul(4)],
        }
    }

    /// Wrap tightly packed RGBA bytes.
    pub fn from_rgba(width: usize, height: usize, bytes: Vec<u8>) -> Result<Self, ImageError> {
        let expected = width.saturating_mul(height).saturating_mul(4);
        if bytes.len() != expected {
            return Err(ImageError::LengthMismatch {
                expected,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels: bytes,
        })
    }

    /// Build from text rows where `palette` maps each character to a colour
    /// (`None` is transparent). Rows are padded to the longest row.
    ///
    /// This is how small shell icons are authored in source without a binary
    /// asset pipeline.
    pub fn from_ascii_art(rows: &[&str], palette: impl Fn(char) -> Option<RgbaColor>) -> Self {
        let width = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        let mut image = Self::new(width, rows.len());
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if let Some(color) = palette(ch) {
                    image.set_pixel(x, y, color);
                }
            }
        }
        image
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
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y * self.width + x) * 4;
        Some(RgbaColor::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.width + x) * 4;
        self.pixels[offset..offset + 4].copy_from_slice(&[color.r, color.g, color.b, color.a]);
    }

    /// Borrowed view for blitting.
    pub fn as_ref(&self) -> ImageRef<'_> {
        ImageRef {
            width: self.width,
            height: self.height,
            pixels: &self.pixels,
        }
    }
}

/// Borrowed RGBA8888 image, so a buffer or a static asset can be blitted
/// without copying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageRef<'a> {
    width: usize,
    height: usize,
    pixels: &'a [u8],
}

impl<'a> ImageRef<'a> {
    /// `pixels.len()` must be `width * height * 4`.
    pub fn new(width: usize, height: usize, pixels: &'a [u8]) -> Result<Self, ImageError> {
        let expected = width.saturating_mul(height).saturating_mul(4);
        if pixels.len() != expected {
            return Err(ImageError::LengthMismatch {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y * self.width + x) * 4;
        Some(RgbaColor::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }
}

/// Source-over blend of `src` onto `dst` using `src.a`; result is opaque.
pub fn blend_over(dst: RgbaColor, src: RgbaColor) -> RgbaColor {
    match src.a {
        0 => dst,
        255 => RgbaColor::new(src.r, src.g, src.b, 255),
        alpha => {
            let a = alpha as u32;
            let inv = 255 - a;
            let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * inv + 127) / 255) as u8;
            RgbaColor::new(mix(src.r, dst.r), mix(src.g, dst.g), mix(src.b, dst.b), 255)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum GlyphSource {
    /// Hand-drawn 5x7 uppercase set with a handful of symbols. Compact and
    /// legible at very small cell sizes, but case-folding and sparse.
    #[default]
    Compact5x7,
    /// Full printable-ASCII 8x16 set (`FONT_8X16`), shared with the kernel
    /// text console. Distinct lowercase and all punctuation.
    Ascii8x16,
}

impl GlyphSource {
    const fn width(self) -> usize {
        match self {
            GlyphSource::Compact5x7 => 5,
            GlyphSource::Ascii8x16 => 8,
        }
    }

    const fn height(self) -> usize {
        match self {
            GlyphSource::Compact5x7 => 7,
            GlyphSource::Ascii8x16 => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitmapFont {
    glyph_width: usize,
    glyph_height: usize,
    advance_x: usize,
    #[serde(default)]
    source: GlyphSource,
}

impl BitmapFont {
    /// A font sampled from the compact 5x7 glyph set, scaled to the cell.
    pub const fn new(glyph_width: usize, glyph_height: usize, advance_x: usize) -> Self {
        Self {
            glyph_width,
            glyph_height,
            advance_x,
            source: GlyphSource::Compact5x7,
        }
    }

    /// The full-ASCII 8x16 font at native size with the given advance.
    pub const fn ascii_8x16(advance_x: usize) -> Self {
        Self {
            glyph_width: 8,
            glyph_height: 16,
            advance_x,
            source: GlyphSource::Ascii8x16,
        }
    }

    pub const fn with_source(mut self, source: GlyphSource) -> Self {
        self.source = source;
        self
    }

    pub const fn source(&self) -> GlyphSource {
        self.source
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
/// Desktop text font: full printable ASCII, 8x16 pixels, tiled at 8 px advance.
pub const DESKTOP_FONT: BitmapFont = BitmapFont::ascii_8x16(8);

/// Complete 8x16 bitmap font for ASCII 0x00..0x7F (one byte per row, MSB is
/// the leftmost pixel). Also used by the kernel framebuffer text console so
/// text and graphics modes share one glyph set.
pub static FONT_8X16: [[u8; 16]; 128] = include!("font_data_8x16.in");

/// 8x16 glyph for `ch`; non-ASCII and control characters map to `?`.
pub fn ascii_8x16_glyph(ch: char) -> &'static [u8; 16] {
    let index = ch as usize;
    if ch.is_ascii() && !ch.is_ascii_control() && index < FONT_8X16.len() {
        &FONT_8X16[index]
    } else if ch == ' ' {
        &FONT_8X16[b' ' as usize]
    } else {
        &FONT_8X16[b'?' as usize]
    }
}

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

    /// Horizontal run of `len` pixels starting at `(x, y)`.
    fn draw_hline(&mut self, x: usize, y: usize, len: usize, color: RgbaColor) {
        self.fill_rect(RasterRect::new(x, y, len, 1), color);
    }

    /// Vertical run of `len` pixels starting at `(x, y)`.
    fn draw_vline(&mut self, x: usize, y: usize, len: usize, color: RgbaColor) {
        self.fill_rect(RasterRect::new(x, y, 1, len), color);
    }

    /// One-pixel line between two points (inclusive), Bresenham.
    ///
    /// Endpoints may lie outside the target; only in-bounds pixels are
    /// written, so callers can draw against a virtual coordinate space.
    /// Bresenham, walking only the part of the segment that can land on the
    /// target.
    ///
    /// This used to walk the whole segment, testing each point for being in
    /// bounds and writing the ones that were. A `DrawOp::Line` carries `i32`
    /// coordinates, so `Line { x0: i32::MIN, x1: i32::MAX }` -- one draw op
    /// from one view -- was 4_294_967_296 iterations: measured at 7.9 seconds
    /// in an optimized build, with the compositor frozen throughout, and
    /// nothing stops a view sending a hundred of them.
    ///
    /// The walk is monotonic in both axes and steps its dominant axis exactly
    /// once per iteration, so the iterations that can produce a pixel form a
    /// contiguous range, and `line_skip` computes the state at the start of
    /// that range in closed form. The pixels written are *identical* to the
    /// full walk's -- `a_skipped_line_draws_what_the_full_walk_draws` checks
    /// that against a reference implementation of the old loop.
    fn draw_line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, color: RgbaColor) {
        let (width, height) = (self.width() as i64, self.height() as i64);
        if width <= 0 || height <= 0 {
            return;
        }
        // Nothing of the segment's bounding box reaches the target.
        if x0.max(x1) < 0 || y0.max(y1) < 0 || x0.min(x1) >= width || y0.min(y1) >= height {
            return;
        }

        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i64 = if x0 < x1 { 1 } else { -1 };
        let sy: i64 = if y0 < y1 { 1 } else { -1 };

        let Some(start) = line_skip(x0, y0, x1, y1, width, height) else {
            return;
        };
        let (mut x, mut y, mut err, mut remaining) = start;

        loop {
            if x >= 0 && y >= 0 && x < width && y < height {
                self.write_pixel(x as usize, y as usize, color);
            }
            if x == x1 && y == y1 {
                break;
            }
            // Past the last iteration that can produce a pixel. The walk is
            // monotonic, so nothing after this re-enters the target.
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            let twice = 2 * err;
            if twice >= dy {
                err += dy;
                x += sx;
            }
            if twice <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Filled rectangle with quarter-circle corners of `radius` pixels.
    /// A radius of zero is a plain `fill_rect`.
    fn fill_rounded_rect(&mut self, rect: RasterRect, radius: usize, color: RgbaColor) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let radius = clamp_radius(rect, radius);
        // Only the rows that can land on the target. `rect.height` comes
        // straight from a `DrawOp`'s `u32`, so this loop used to run up to
        // four billion times on a target a few hundred pixels tall, every
        // iteration past the bottom edge doing nothing but cost time.
        for row in 0..visible_rows(rect, self.height()) {
            if let Some((start, end)) = rounded_row_span(rect, radius, row) {
                self.fill_rect(RasterRect::new(start, rect.y + row, end - start, 1), color);
            }
        }
    }

    /// Rounded outline of `thickness` pixels; the interior is left untouched.
    fn draw_rounded_border(
        &mut self,
        rect: RasterRect,
        radius: usize,
        thickness: usize,
        color: RgbaColor,
    ) {
        if rect.width == 0 || rect.height == 0 || thickness == 0 {
            return;
        }
        let radius = clamp_radius(rect, radius);
        let thickness = thickness
            .min(rect.width.div_ceil(2))
            .min(rect.height.div_ceil(2));
        let inner = RasterRect::new(
            rect.x + thickness,
            rect.y + thickness,
            rect.width.saturating_sub(thickness * 2),
            rect.height.saturating_sub(thickness * 2),
        );
        let inner_radius = radius.saturating_sub(thickness);

        // Bounded to the target for the same reason as `fill_rounded_rect`.
        for row in 0..visible_rows(rect, self.height()) {
            let Some((outer_start, outer_end)) = rounded_row_span(rect, radius, row) else {
                continue;
            };
            let y = rect.y + row;
            let inner_span = if inner.width > 0 && inner.height > 0 && row >= thickness {
                rounded_row_span(inner, inner_radius, row - thickness)
            } else {
                None
            };
            match inner_span {
                Some((inner_start, inner_end)) => {
                    if inner_start > outer_start {
                        self.fill_rect(
                            RasterRect::new(outer_start, y, inner_start - outer_start, 1),
                            color,
                        );
                    }
                    if outer_end > inner_end {
                        self.fill_rect(
                            RasterRect::new(inner_end, y, outer_end - inner_end, 1),
                            color,
                        );
                    }
                }
                None => {
                    self.fill_rect(
                        RasterRect::new(outer_start, y, outer_end - outer_start, 1),
                        color,
                    );
                }
            }
        }
    }

    /// Blit an image with its top-left at `(x, y)`, honouring alpha.
    ///
    /// The origin may be negative or beyond the target; only overlapping
    /// pixels are touched. Fully transparent source pixels leave the target
    /// alone, which is what makes icons composable over any background.
    fn blit_image(&mut self, x: i64, y: i64, image: ImageRef<'_>) {
        let (width, height) = (self.width() as i64, self.height() as i64);
        let x_start = x.max(0);
        let y_start = y.max(0);
        let x_end = (x + image.width() as i64).min(width);
        let y_end = (y + image.height() as i64).min(height);
        if x_end <= x_start || y_end <= y_start {
            return;
        }
        for ty in y_start..y_end {
            for tx in x_start..x_end {
                let sx = (tx - x) as usize;
                let sy = (ty - y) as usize;
                let Some(src) = image.pixel(sx, sy) else {
                    continue;
                };
                match src.a {
                    0 => {}
                    255 => self.write_pixel(tx as usize, ty as usize, src),
                    _ => {
                        let dst = self
                            .pixel(tx as usize, ty as usize)
                            .unwrap_or(RgbaColor::new(0, 0, 0, 255));
                        self.write_pixel(tx as usize, ty as usize, blend_over(dst, src));
                    }
                }
            }
        }
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

/// Largest radius that still leaves the rectangle well-formed.
/// Where a Bresenham walk from `(x0, y0)` to `(x1, y1)` should start and how
/// many further iterations can matter, for a target of `width` x `height`.
///
/// Returns `(x, y, err, remaining)`: the walk's exact state at the first
/// iteration whose dominant coordinate is inside the target, and the number
/// of iterations after it that can still be. `None` when no iteration can
/// produce a pixel.
///
/// The closed form: in the x-dominant case the walk steps x once per
/// iteration, so after `n` iterations `x = x0 + n*sx`, and the number of y
/// steps taken is
///
/// ```text
/// m(n) = clamp(floor((-dx - 2*dy - 2*(n-1)*dy) / (2*dx)) + 1, 0, n)
/// ```
///
/// from which `err = (dx + dy) + n*dy + m*dx`. The y-dominant case is the
/// same with the axes exchanged. Both were checked against the full walk at
/// nearly two million sample points before being written down.
fn line_skip(
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    width: i64,
    height: i64,
) -> Option<(i64, i64, i64, i64)> {
    fn floor_div(a: i128, b: i128) -> i128 {
        let q = a / b;
        if (a % b != 0) && ((a < 0) != (b < 0)) {
            q - 1
        } else {
            q
        }
    }

    // The inclusive range of iteration counts for which `from + n*step` lies
    // in `[0, limit)`, intersected with `[0, total]`.
    fn window(from: i64, step: i64, limit: i64, total: i64) -> Option<(i64, i64)> {
        let (lo, hi) = if step > 0 {
            (-from, limit - 1 - from)
        } else {
            (from - limit + 1, from)
        };
        let lo = lo.max(0);
        let hi = hi.min(total);
        if lo > hi {
            None
        } else {
            Some((lo, hi))
        }
    }

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i64 = if x0 < x1 { 1 } else { -1 };
    let sy: i64 = if y0 < y1 { 1 } else { -1 };
    let err0 = dx + dy;

    // A single point: no stepping to skip.
    if dx == 0 && dy == 0 {
        return Some((x0, y0, err0, 0));
    }

    let x_dominant = dx >= -dy;
    let (total, from, step, limit) = if x_dominant {
        (dx, x0, sx, width)
    } else {
        (-dy, y0, sy, height)
    };
    let (lo, hi) = window(from, step, limit, total)?;

    // Steps taken on the minor axis after `lo` iterations. Both cases are the
    // same expression with the axes exchanged: the minor axis steps whenever
    // the count so far is at or below
    //
    //     (2 * minor_delta * n - major_delta) / (2 * major_delta)
    //
    // which is monotonic in `n`, so the count after `lo` iterations is that
    // bound at `lo - 1`, floored, plus one.
    let (major_delta, minor_delta) = if x_dominant {
        (dx as i128, (-dy) as i128)
    } else {
        ((-dy) as i128, dx as i128)
    };
    let minor = if lo == 0 || major_delta == 0 {
        0i128
    } else {
        let num = 2 * minor_delta * (lo as i128) - major_delta;
        (floor_div(num, 2 * major_delta) + 1).clamp(0, lo as i128)
    };

    let (n, m) = if x_dominant {
        (lo as i128, minor)
    } else {
        (minor, lo as i128)
    };
    let x = x0 + (n as i64) * sx;
    let y = y0 + (m as i64) * sy;
    let err = err0 as i128 + n * (dy as i128) + m * (dx as i128);

    Some((x, y, err as i64, hi - lo))
}

/// How many of `rect`'s rows can appear on a target `target_height` tall.
///
/// A rectangle's height is attacker-controlled -- `DrawOp` carries it as a
/// `u32` -- while the target is a few hundred pixels. Row loops must be
/// bounded by the target, not by the rectangle.
fn visible_rows(rect: RasterRect, target_height: usize) -> usize {
    target_height.saturating_sub(rect.y).min(rect.height)
}

fn clamp_radius(rect: RasterRect, radius: usize) -> usize {
    radius.min(rect.width / 2).min(rect.height / 2)
}

/// Horizontal pixel span `[start, end)` of a rounded rectangle at `row`
/// (0-based from the top of `rect`), or `None` when the row is empty.
fn rounded_row_span(rect: RasterRect, radius: usize, row: usize) -> Option<(usize, usize)> {
    if row >= rect.height || rect.width == 0 {
        return None;
    }
    if radius == 0 {
        return Some((rect.x, rect.x + rect.width));
    }
    // Distance of this row from the nearest corner-arc centre row, or zero
    // when the row is in the straight middle section.
    let from_top = row;
    let from_bottom = rect.height - 1 - row;
    let dy = if from_top < radius {
        radius - from_top
    } else if from_bottom < radius {
        radius - from_bottom
    } else {
        0
    };
    // Horizontal inset such that (inset, dy) lies on the quarter circle. Using
    // (radius - 0.5) keeps the outline visually round at small radii.
    let inset = if dy == 0 {
        0
    } else {
        let r2 = (radius * radius) as i64;
        let dy2 = ((dy - 1) * (dy - 1)) as i64;
        let dx = isqrt(r2 - dy2);
        radius - (dx as usize).min(radius)
    };
    let start = rect.x + inset;
    let end = rect.x + rect.width - inset;
    if end <= start {
        None
    } else {
        Some((start, end))
    }
}

/// Integer square root (floor), enough for pixel radii.
fn isqrt(value: i64) -> i64 {
    if value <= 0 {
        return 0;
    }
    let mut lo = 0i64;
    let mut hi = value.min(1 << 31);
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if mid * mid <= value {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
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

    pub fn draw_line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, color: RgbaColor) {
        <Self as RenderTarget>::draw_line(self, x0, y0, x1, y1, color)
    }

    pub fn blit_image(&mut self, x: i64, y: i64, image: ImageRef<'_>) {
        <Self as RenderTarget>::blit_image(self, x, y, image)
    }

    /// Borrow this buffer as an image, e.g. to composite an off-screen
    /// surface into another target.
    pub fn as_image(&self) -> ImageRef<'_> {
        ImageRef {
            width: self.width,
            height: self.height,
            pixels: &self.pixels,
        }
    }

    pub fn fill_rounded_rect(&mut self, rect: RasterRect, radius: usize, color: RgbaColor) {
        <Self as RenderTarget>::fill_rounded_rect(self, rect, radius, color)
    }

    pub fn draw_rounded_border(
        &mut self,
        rect: RasterRect,
        radius: usize,
        thickness: usize,
        color: RgbaColor,
    ) {
        <Self as RenderTarget>::draw_rounded_border(self, rect, radius, thickness, color)
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

/// A child drawing space inside a parent target (GFX-028).
///
/// Drawing at local `(x, y)` lands at parent
/// `(viewport.x + x - scroll_x, viewport.y + y - scroll_y)` and is clipped to
/// `viewport`. With zero scroll this is a clipping container with its own
/// origin; with a scroll offset it is a viewport onto larger content. Children
/// never learn where they are on screen, which keeps layout a parent concern.
pub struct ContainerTarget<'a, T: RenderTarget + ?Sized> {
    target: &'a mut T,
    viewport: RasterRect,
    scroll_x: usize,
    scroll_y: usize,
}

impl<'a, T: RenderTarget + ?Sized> ContainerTarget<'a, T> {
    /// Clipping container at `viewport` with local origin at its top-left.
    pub fn new(target: &'a mut T, viewport: RasterRect) -> Self {
        Self {
            target,
            viewport,
            scroll_x: 0,
            scroll_y: 0,
        }
    }

    /// Container whose local origin is shifted up/left by the scroll offset,
    /// so local `(scroll_x, scroll_y)` is the viewport's top-left.
    pub fn scrolled(
        target: &'a mut T,
        viewport: RasterRect,
        scroll_x: usize,
        scroll_y: usize,
    ) -> Self {
        Self {
            target,
            viewport,
            scroll_x,
            scroll_y,
        }
    }

    pub const fn viewport(&self) -> RasterRect {
        self.viewport
    }

    pub const fn scroll(&self) -> (usize, usize) {
        (self.scroll_x, self.scroll_y)
    }

    /// Map a local point to parent coordinates if it is inside the viewport.
    fn to_parent(&self, x: usize, y: usize) -> Option<(usize, usize)> {
        if x < self.scroll_x || y < self.scroll_y {
            return None;
        }
        let px = self.viewport.x.checked_add(x - self.scroll_x)?;
        let py = self.viewport.y.checked_add(y - self.scroll_y)?;
        if self.viewport.contains(px, py) {
            Some((px, py))
        } else {
            None
        }
    }
}

impl<T: RenderTarget + ?Sized> RenderTarget for ContainerTarget<'_, T> {
    /// Local extent that can reach the viewport: everything up to its right edge.
    fn width(&self) -> usize {
        self.scroll_x.saturating_add(self.viewport.width)
    }

    fn height(&self) -> usize {
        self.scroll_y.saturating_add(self.viewport.height)
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        if let Some((px, py)) = self.to_parent(x, y) {
            self.target.write_pixel(px, py, color);
        }
    }

    fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        let (px, py) = self.to_parent(x, y)?;
        self.target.pixel(px, py)
    }
}

/// Wraps a target and counts pixel writes and reads, for benchmarks and
/// work-bound tests (GFX-045). Counting is exact: every provided draw op
/// bottoms out in `write_pixel`.
pub struct CountingTarget<'a, T: RenderTarget + ?Sized> {
    target: &'a mut T,
    writes: u64,
    reads: u64,
}

impl<'a, T: RenderTarget + ?Sized> CountingTarget<'a, T> {
    pub fn new(target: &'a mut T) -> Self {
        Self {
            target,
            writes: 0,
            reads: 0,
        }
    }

    pub const fn writes(&self) -> u64 {
        self.writes
    }

    pub const fn reads(&self) -> u64 {
        self.reads
    }

    pub fn reset(&mut self) {
        self.writes = 0;
        self.reads = 0;
    }
}

impl<T: RenderTarget + ?Sized> RenderTarget for CountingTarget<'_, T> {
    fn width(&self) -> usize {
        self.target.width()
    }

    fn height(&self) -> usize {
        self.target.height()
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: RgbaColor) {
        self.writes += 1;
        self.target.write_pixel(x, y, color);
    }

    fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        // `pixel` takes `&self`; reads are counted by callers that care.
        self.target.pixel(x, y)
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
    match font.source() {
        GlyphSource::Compact5x7 => {
            let source = compact_glyph_for(ch);
            scale_glyph(font, &source, GlyphSource::Compact5x7)
        }
        GlyphSource::Ascii8x16 => {
            let source = ascii_8x16_glyph(ch);
            scale_glyph(font, source, GlyphSource::Ascii8x16)
        }
    }
}

/// Nearest-neighbour scale of a source bitmap into the font's cell.
fn scale_glyph(font: &BitmapFont, source: &[u8], kind: GlyphSource) -> [u16; MAX_GLYPH_HEIGHT] {
    let source_width = kind.width();
    let source_height = kind.height();
    let mut rows = [0u16; MAX_GLYPH_HEIGHT];
    let glyph_width = font.glyph_width().min(16);
    let glyph_height = font.glyph_height().min(MAX_GLYPH_HEIGHT);
    if glyph_width == 0 || glyph_height == 0 {
        return rows;
    }

    for (target_y, slot) in rows.iter_mut().enumerate().take(glyph_height) {
        let source_y = target_y * source_height / glyph_height;
        let source_row = source[source_y];
        let mut row = 0u16;

        for target_x in 0..glyph_width {
            let source_x = target_x * source_width / glyph_width;
            let bit = (source_row >> (source_width - 1 - source_x)) & 1;
            if bit != 0 {
                row |= 1 << (glyph_width - 1 - target_x);
            }
        }

        *slot = row;
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

    fn count(buffer: &RgbaBuffer, color: RgbaColor) -> usize {
        let mut n = 0;
        for y in 0..buffer.height() {
            for x in 0..buffer.width() {
                if buffer.pixel(x, y) == Some(color) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn test_draw_line_covers_endpoints_and_clips() {
        let mut buffer = RgbaBuffer::new(10, 10, CLEAR);
        buffer.draw_line(1, 1, 8, 8, ACCENT);
        assert_eq!(buffer.pixel(1, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(8, 8), Some(ACCENT));
        assert_eq!(buffer.pixel(4, 4), Some(ACCENT));
        assert_eq!(buffer.pixel(4, 5), Some(CLEAR));
        assert_eq!(
            count(&buffer, ACCENT),
            8,
            "diagonal touches one pixel per row"
        );

        // Steep, reversed, and partially off-target lines still draw in-bounds pixels.
        let mut buffer = RgbaBuffer::new(10, 10, CLEAR);
        buffer.draw_line(2, 9, 2, -5, ACCENT);
        assert_eq!(count(&buffer, ACCENT), 10);
        assert_eq!(buffer.pixel(2, 0), Some(ACCENT));
        assert_eq!(buffer.pixel(2, 9), Some(ACCENT));

        let mut buffer = RgbaBuffer::new(10, 10, CLEAR);
        buffer.draw_line(-3, 5, 30, 5, ACCENT);
        assert_eq!(count(&buffer, ACCENT), 10);

        // hline/vline are one-pixel-thick rect fills.
        let mut buffer = RgbaBuffer::new(10, 10, CLEAR);
        buffer.draw_hline(1, 2, 5, ACCENT);
        buffer.draw_vline(7, 0, 4, DETAIL);
        assert_eq!(count(&buffer, ACCENT), 5);
        assert_eq!(count(&buffer, DETAIL), 4);
        assert_eq!(buffer.pixel(5, 2), Some(ACCENT));
        assert_eq!(buffer.pixel(6, 2), Some(CLEAR));
    }

    #[test]
    fn test_fill_rounded_rect_rounds_corners_only() {
        let rect = RasterRect::new(2, 2, 16, 12);
        let mut plain = RgbaBuffer::new(20, 16, CLEAR);
        plain.fill_rect(rect, ACCENT);
        let mut zero = RgbaBuffer::new(20, 16, CLEAR);
        zero.fill_rounded_rect(rect, 0, ACCENT);
        assert_eq!(
            plain.as_bytes(),
            zero.as_bytes(),
            "radius 0 is a plain fill"
        );

        let mut rounded = RgbaBuffer::new(20, 16, CLEAR);
        rounded.fill_rounded_rect(rect, 4, ACCENT);
        // Corner pixels are clipped, edge midpoints and the centre are filled.
        for (x, y) in [(2, 2), (17, 2), (2, 13), (17, 13)] {
            assert_eq!(rounded.pixel(x, y), Some(CLEAR), "corner {x},{y}");
        }
        assert_eq!(rounded.pixel(10, 2), Some(ACCENT));
        assert_eq!(rounded.pixel(2, 8), Some(ACCENT));
        assert_eq!(rounded.pixel(17, 8), Some(ACCENT));
        assert_eq!(rounded.pixel(10, 13), Some(ACCENT));
        assert_eq!(rounded.pixel(9, 7), Some(ACCENT));
        let filled = count(&rounded, ACCENT);
        assert!(filled < 16 * 12 && filled > 16 * 12 - 4 * 8, "{filled}");

        // The shape is left/right and top/bottom symmetric.
        for y in 2..14 {
            for x in 2..18 {
                let mirror_x = 2 + 17 - x;
                let mirror_y = 2 + 13 - y;
                assert_eq!(rounded.pixel(x, y), rounded.pixel(mirror_x, y));
                assert_eq!(rounded.pixel(x, y), rounded.pixel(x, mirror_y));
            }
        }

        // Oversized radius is clamped to half the shorter side (a pill).
        let mut pill = RgbaBuffer::new(20, 16, CLEAR);
        pill.fill_rounded_rect(RasterRect::new(0, 0, 20, 6), 100, ACCENT);
        assert_eq!(pill.pixel(10, 3), Some(ACCENT));
        assert_eq!(pill.pixel(0, 0), Some(CLEAR));
    }

    #[test]
    fn test_draw_rounded_border_leaves_interior_untouched() {
        let rect = RasterRect::new(1, 1, 18, 14);
        let mut buffer = RgbaBuffer::new(20, 16, CLEAR);
        buffer.draw_rounded_border(rect, 4, 2, ACCENT);

        // Ring pixels on the straight edges, two thick.
        assert_eq!(buffer.pixel(9, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(9, 2), Some(ACCENT));
        assert_eq!(buffer.pixel(9, 3), Some(CLEAR));
        assert_eq!(buffer.pixel(1, 8), Some(ACCENT));
        assert_eq!(buffer.pixel(2, 8), Some(ACCENT));
        assert_eq!(buffer.pixel(3, 8), Some(CLEAR));
        // Interior stays clear, corner pixel stays clear.
        assert_eq!(buffer.pixel(9, 8), Some(CLEAR));
        assert_eq!(buffer.pixel(1, 1), Some(CLEAR));

        // Thickness covering everything degenerates to a filled rounded rect.
        let mut thick = RgbaBuffer::new(20, 16, CLEAR);
        thick.draw_rounded_border(rect, 4, 50, ACCENT);
        let mut filled = RgbaBuffer::new(20, 16, CLEAR);
        filled.fill_rounded_rect(rect, 4, ACCENT);
        assert_eq!(thick.as_bytes(), filled.as_bytes());

        // Primitives honour a scissor like everything else.
        let mut clipped = RgbaBuffer::new(20, 16, CLEAR);
        {
            let mut scissor = ScissorTarget::new(&mut clipped, RasterRect::new(0, 0, 10, 16));
            scissor.draw_rounded_border(rect, 4, 2, ACCENT);
            scissor.draw_line(0, 15, 19, 15, DETAIL);
        }
        assert_eq!(clipped.pixel(1, 8), Some(ACCENT));
        assert_eq!(clipped.pixel(18, 8), Some(CLEAR));
        assert_eq!(count(&clipped, DETAIL), 10);
    }

    #[test]
    fn test_image_construction_and_ascii_art() {
        assert_eq!(
            RgbaImage::from_rgba(2, 2, vec![0; 15]),
            Err(ImageError::LengthMismatch {
                expected: 16,
                actual: 15
            })
        );
        let image = RgbaImage::from_rgba(1, 1, vec![1, 2, 3, 4]).unwrap();
        assert_eq!(image.pixel(0, 0), Some(RgbaColor::new(1, 2, 3, 4)));
        assert_eq!(image.pixel(1, 0), None);

        let icon = RgbaImage::from_ascii_art(&["#.", "##", "#"], |ch| match ch {
            '#' => Some(ACCENT),
            _ => None,
        });
        assert_eq!((icon.width(), icon.height()), (2, 3));
        assert_eq!(icon.pixel(0, 0), Some(ACCENT));
        assert_eq!(icon.pixel(1, 0).map(|c| c.a), Some(0));
        assert_eq!(icon.pixel(1, 2).map(|c| c.a), Some(0), "short row padded");
        assert_eq!(icon.as_bytes().len(), 2 * 3 * 4);

        let view = ImageRef::new(2, 3, icon.as_bytes()).unwrap();
        assert_eq!(view.pixel(1, 1), Some(ACCENT));
        assert!(ImageRef::new(2, 2, icon.as_bytes()).is_err());
    }

    #[test]
    fn test_blit_image_respects_alpha_and_clips() {
        let mut buffer = RgbaBuffer::new(6, 4, CLEAR);
        let mut sprite = RgbaImage::new(3, 2);
        sprite.set_pixel(0, 0, ACCENT); // opaque
        sprite.set_pixel(1, 0, RgbaColor::new(200, 100, 50, 0)); // transparent
        sprite.set_pixel(2, 0, RgbaColor::new(255, 255, 255, 128)); // half
        sprite.set_pixel(0, 1, DETAIL);

        buffer.blit_image(1, 1, sprite.as_ref());
        assert_eq!(buffer.pixel(1, 1), Some(ACCENT));
        assert_eq!(buffer.pixel(2, 1), Some(CLEAR), "alpha 0 leaves target");
        let blended = buffer.pixel(3, 1).unwrap();
        assert_eq!(
            blended,
            blend_over(CLEAR, RgbaColor::new(255, 255, 255, 128))
        );
        assert!(blended.r > CLEAR.r && blended.r < 255);
        assert_eq!(blended.a, 255);
        assert_eq!(buffer.pixel(1, 2), Some(DETAIL));
        assert_eq!(buffer.pixel(0, 0), Some(CLEAR));

        // Negative and overflowing origins only touch the overlap.
        let mut buffer = RgbaBuffer::new(6, 4, CLEAR);
        let mut full = RgbaImage::new(3, 3);
        for y in 0..3 {
            for x in 0..3 {
                full.set_pixel(x, y, ACCENT);
            }
        }
        buffer.blit_image(-2, -2, full.as_ref());
        assert_eq!(count(&buffer, ACCENT), 1);
        assert_eq!(buffer.pixel(0, 0), Some(ACCENT));
        buffer.blit_image(5, 3, full.as_ref());
        assert_eq!(count(&buffer, ACCENT), 2);
        buffer.blit_image(40, 40, full.as_ref());
        assert_eq!(count(&buffer, ACCENT), 2);

        // A buffer can be blitted into another as an image, under a scissor.
        let mut layer = RgbaBuffer::new(4, 4, RgbaColor::new(0, 0, 0, 0));
        layer.fill_rect(RasterRect::new(0, 0, 4, 2), DETAIL);
        let mut dest = RgbaBuffer::new(8, 8, CLEAR);
        {
            let mut scissor = ScissorTarget::new(&mut dest, RasterRect::new(0, 0, 2, 8));
            scissor.blit_image(0, 0, layer.as_image());
        }
        assert_eq!(count(&dest, DETAIL), 4);
        assert_eq!(dest.pixel(3, 0), Some(CLEAR));
        assert_eq!(dest.pixel(0, 3), Some(CLEAR), "transparent rows untouched");
    }

    #[test]
    fn test_blend_over_endpoints_and_midpoint() {
        let dst = RgbaColor::new(0, 0, 0, 255);
        let src = RgbaColor::new(200, 100, 50, 255);
        assert_eq!(blend_over(dst, src), src);
        assert_eq!(blend_over(dst, RgbaColor::new(9, 9, 9, 0)), dst);
        let half = blend_over(dst, RgbaColor::new(200, 100, 50, 128));
        assert_eq!((half.r, half.g, half.b, half.a), (100, 50, 25, 255));
        let over_white = blend_over(
            RgbaColor::new(255, 255, 255, 255),
            RgbaColor::new(0, 0, 0, 128),
        );
        assert_eq!(over_white.r, 127);
    }

    #[test]
    fn test_container_translates_and_clips_child_drawing() {
        let mut parent = RgbaBuffer::new(20, 12, CLEAR);
        {
            let mut child = ContainerTarget::new(&mut parent, RasterRect::new(5, 3, 8, 4));
            assert_eq!((child.width(), child.height()), (8, 4));
            // Local origin is the viewport's corner.
            child.write_pixel(0, 0, ACCENT);
            // Overflowing the viewport is clipped, including via provided ops.
            child.fill_rect(RasterRect::new(6, 2, 10, 10), DETAIL);
            child.write_pixel(8, 0, ACCENT);
            assert_eq!(child.pixel(0, 0), Some(ACCENT));
            assert_eq!(child.pixel(8, 0), None, "outside the viewport reads None");
        }
        assert_eq!(parent.pixel(5, 3), Some(ACCENT));
        assert_eq!(parent.pixel(13, 3), Some(CLEAR));
        assert_eq!(parent.pixel(11, 5), Some(DETAIL));
        assert_eq!(parent.pixel(12, 6), Some(DETAIL));
        assert_eq!(
            parent.pixel(13, 6),
            Some(CLEAR),
            "clipped at viewport right edge"
        );
        assert_eq!(
            parent.pixel(12, 7),
            Some(CLEAR),
            "clipped at viewport bottom edge"
        );
        assert_eq!(count(&parent, DETAIL), 2 * 2);
    }

    #[test]
    fn test_scrolled_container_shows_a_window_onto_content() {
        let mut parent = RgbaBuffer::new(10, 10, CLEAR);
        {
            // Viewport 4x4 at (2,2), scrolled 3 px down and 1 right: local
            // (1,3) is the top-left visible pixel.
            let mut view =
                ContainerTarget::scrolled(&mut parent, RasterRect::new(2, 2, 4, 4), 1, 3);
            assert_eq!((view.width(), view.height()), (5, 7));
            view.write_pixel(0, 3, ACCENT); // scrolled off to the left
            view.write_pixel(1, 2, ACCENT); // scrolled off above
            view.write_pixel(1, 3, DETAIL); // first visible
            view.write_pixel(4, 6, DETAIL); // last visible
            view.write_pixel(5, 6, ACCENT); // past the right edge
            view.write_pixel(4, 7, ACCENT); // past the bottom edge
            assert_eq!(view.pixel(1, 3), Some(DETAIL));
            assert_eq!(view.pixel(0, 3), None);
        }
        assert_eq!(count(&parent, ACCENT), 0);
        assert_eq!(parent.pixel(2, 2), Some(DETAIL));
        assert_eq!(parent.pixel(5, 5), Some(DETAIL));

        // Text drawn in local coordinates scrolls with the content.
        let mut parent = RgbaBuffer::new(40, 40, CLEAR);
        let mut unscrolled = RgbaBuffer::new(40, 40, CLEAR);
        unscrolled.draw_text_with_font(2, 2, "A", &DESKTOP_FONT, ACCENT);
        {
            let mut view =
                ContainerTarget::scrolled(&mut parent, RasterRect::new(2, 2, 30, 30), 0, 5);
            view.draw_text_with_font(0, 5, "A", &DESKTOP_FONT, ACCENT);
        }
        assert_eq!(parent.as_bytes(), unscrolled.as_bytes());
    }

    #[test]
    fn test_containers_nest() {
        let mut parent = RgbaBuffer::new(30, 30, CLEAR);
        {
            let mut outer = ContainerTarget::new(&mut parent, RasterRect::new(10, 10, 10, 10));
            let mut inner = ContainerTarget::new(&mut outer, RasterRect::new(5, 5, 20, 20));
            // Inner viewport extends past the outer one; the outer clips it.
            inner.fill_rect(RasterRect::new(0, 0, 20, 20), ACCENT);
        }
        assert_eq!(count(&parent, ACCENT), 5 * 5);
        assert_eq!(parent.pixel(15, 15), Some(ACCENT));
        assert_eq!(parent.pixel(14, 15), Some(CLEAR));
        assert_eq!(parent.pixel(19, 19), Some(ACCENT));
        assert_eq!(parent.pixel(20, 20), Some(CLEAR));
    }

    #[test]
    fn test_counting_target_counts_every_pixel_write() {
        let mut buffer = RgbaBuffer::new(20, 10, CLEAR);
        let mut counting = CountingTarget::new(&mut buffer);
        counting.fill_rect(RasterRect::new(0, 0, 5, 4), ACCENT);
        assert_eq!(counting.writes(), 20);
        counting.draw_hline(0, 9, 100, DETAIL);
        assert_eq!(counting.writes(), 40, "clipped writes are not counted");
        counting.reset();
        counting.draw_text_with_font(0, 0, "I", &DESKTOP_FONT, ACCENT);
        // The 20x10 buffer clips the 16-row glyph: only rows 0..10 are written.
        let lit_visible = glyph_pixels(&DESKTOP_FONT, 'I')
            .iter()
            .filter(|(_, y)| *y < 10)
            .count() as u64;
        assert_eq!(counting.writes(), lit_visible);
    }

    #[test]
    fn property_scissor_and_container_never_write_outside_their_bounds() {
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..300 {
            let w = 4 + (next() % 40) as usize;
            let h = 4 + (next() % 30) as usize;
            let mut buffer = RgbaBuffer::new(w, h, CLEAR);
            let rx = (next() % w as u64) as usize;
            let ry = (next() % h as u64) as usize;
            let rw = 1 + (next() % (w as u64)) as usize;
            let rh = 1 + (next() % (h as u64)) as usize;
            let region = RasterRect::new(rx, ry, rw, rh);
            let use_container = next() % 2 == 0;
            {
                let ops = next() % 6;
                let a = (next() % 60) as i64 - 10;
                let b = (next() % 60) as i64 - 10;
                let c = (next() % 60) as i64 - 10;
                let d = (next() % 60) as i64 - 10;
                let mut draw = |t: &mut dyn RenderTarget| match ops {
                    0 => t.fill_rect(
                        RasterRect::new(a.max(0) as usize, b.max(0) as usize, 30, 30),
                        ACCENT,
                    ),
                    1 => t.draw_line(a, b, c, d, ACCENT),
                    2 => t.fill_rounded_rect(
                        RasterRect::new(a.max(0) as usize, b.max(0) as usize, 25, 20),
                        5,
                        ACCENT,
                    ),
                    3 => t.draw_rounded_border(
                        RasterRect::new(a.max(0) as usize, b.max(0) as usize, 25, 20),
                        4,
                        2,
                        ACCENT,
                    ),
                    4 => t.draw_text_with_font(
                        a.max(0) as usize,
                        b.max(0) as usize,
                        "Panda!",
                        &DESKTOP_FONT,
                        ACCENT,
                    ),
                    _ => t.draw_border(RasterRect::new(0, 0, 100, 100), 3, ACCENT),
                };
                if use_container {
                    let mut target = ContainerTarget::new(&mut buffer, region);
                    draw(&mut target);
                } else {
                    let mut target = ScissorTarget::new(&mut buffer, region);
                    draw(&mut target);
                }
            }
            for y in 0..h {
                for x in 0..w {
                    if !region.contains(x, y) {
                        assert_eq!(
                            buffer.pixel(x, y),
                            Some(CLEAR),
                            "leak at ({x},{y}) outside {region:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_desktop_font_reports_readable_metrics() {
        assert_eq!(DESKTOP_FONT.measure_text("Ab"), (16, 16));
        assert_eq!(DESKTOP_FONT.source(), GlyphSource::Ascii8x16);
        assert_eq!(COMPACT_FONT.source(), GlyphSource::Compact5x7);
    }

    fn glyph_pixels(font: &BitmapFont, ch: char) -> Vec<(usize, usize)> {
        let mut buffer = RgbaBuffer::new(font.glyph_width(), font.glyph_height(), CLEAR);
        buffer.draw_text_with_font(0, 0, &ch.to_string(), font, ACCENT);
        let mut lit = Vec::new();
        for y in 0..font.glyph_height() {
            for x in 0..font.glyph_width() {
                if buffer.pixel(x, y) == Some(ACCENT) {
                    lit.push((x, y));
                }
            }
        }
        lit
    }

    #[test]
    fn test_ascii_font_distinguishes_case_and_punctuation() {
        let upper = glyph_pixels(&DESKTOP_FONT, 'A');
        let lower = glyph_pixels(&DESKTOP_FONT, 'a');
        assert!(!upper.is_empty() && !lower.is_empty());
        assert_ne!(upper, lower, "lowercase must not fold to uppercase");

        let question = glyph_pixels(&DESKTOP_FONT, '?');
        for ch in ['\'', '>', '|', '<', '~', '`', '{'] {
            let glyph = glyph_pixels(&DESKTOP_FONT, ch);
            assert!(!glyph.is_empty(), "{ch:?} must have a glyph");
            assert_ne!(glyph, question, "{ch:?} must not fall back to '?'");
        }

        assert!(glyph_pixels(&DESKTOP_FONT, ' ').is_empty());
        assert_eq!(glyph_pixels(&DESKTOP_FONT, 'é'), question);
        assert_eq!(glyph_pixels(&DESKTOP_FONT, '\u{1}'), question);
    }

    #[test]
    fn test_ascii_font_matches_source_bitmap_at_native_size() {
        // At 8x16 the rasterized glyph must equal the source rows bit for bit.
        let lit = glyph_pixels(&DESKTOP_FONT, 'g');
        let source = ascii_8x16_glyph('g');
        let mut expected = Vec::new();
        for (y, row) in source.iter().enumerate() {
            for x in 0..8 {
                if (row >> (7 - x)) & 1 == 1 {
                    expected.push((x, y));
                }
            }
        }
        assert_eq!(lit, expected);
    }

    #[test]
    fn test_ascii_font_scales_to_smaller_cells() {
        let small = BitmapFont::new(4, 8, 5).with_source(GlyphSource::Ascii8x16);
        let lit = glyph_pixels(&small, 'M');
        assert!(!lit.is_empty());
        assert!(lit.iter().all(|&(x, y)| x < 4 && y < 8));
    }

    #[test]
    fn test_draw_text_with_desktop_font_advances_one_cell() {
        let mut buffer = RgbaBuffer::new(40, 20, CLEAR);
        buffer.draw_text_with_font(1, 1, "II", &DESKTOP_FONT, ACCENT);

        let first: Vec<usize> = (1..9)
            .filter(|&x| (1..17).any(|y| buffer.pixel(x, y) == Some(ACCENT)))
            .collect();
        let second: Vec<usize> = (9..17)
            .filter(|&x| (1..17).any(|y| buffer.pixel(x, y) == Some(ACCENT)))
            .collect();
        assert!(!first.is_empty() && !second.is_empty());
        let shifted: Vec<usize> = first.iter().map(|x| x + 8).collect();
        assert_eq!(
            second, shifted,
            "second glyph is the first shifted by advance"
        );
        assert!((17..40).all(|x| (0..20).all(|y| buffer.pixel(x, y) != Some(ACCENT))));
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

#[cfg(test)]
mod line_bounds_tests {
    use super::*;

    /// A target that records exactly which pixels were written, in order.
    struct Recorder {
        width: usize,
        height: usize,
        written: Vec<(usize, usize)>,
    }

    impl RenderTarget for Recorder {
        fn width(&self) -> usize {
            self.width
        }
        fn height(&self) -> usize {
            self.height
        }
        fn write_pixel(&mut self, x: usize, y: usize, _color: RgbaColor) {
            self.written.push((x, y));
        }
        fn pixel(&self, _x: usize, _y: usize) -> Option<RgbaColor> {
            None
        }
    }

    /// The walk exactly as it was before the skip was added.
    fn reference_walk(
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        width: i64,
        height: i64,
    ) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i64 = if x0 < x1 { 1 } else { -1 };
        let sy: i64 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            if x >= 0 && y >= 0 && x < width && y < height {
                out.push((x as usize, y as usize));
            }
            if x == x1 && y == y1 {
                break;
            }
            let twice = 2 * err;
            if twice >= dy {
                err += dy;
                x += sx;
            }
            if twice <= dx {
                err += dx;
                y += sy;
            }
        }
        out
    }

    /// The skip must be invisible: same pixels, same order, for every line.
    /// Coordinates well outside the target on both axes and in both
    /// directions, so the entering, crossing, leaving and never-arriving
    /// cases are all covered.
    #[test]
    fn a_skipped_line_draws_what_the_full_walk_draws() {
        let (width, height) = (37usize, 23usize);
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        for _ in 0..20_000 {
            let span = 160i64;
            let x0 = (rnd() % (span as u64 * 2)) as i64 - span;
            let y0 = (rnd() % (span as u64 * 2)) as i64 - span;
            let x1 = (rnd() % (span as u64 * 2)) as i64 - span;
            let y1 = (rnd() % (span as u64 * 2)) as i64 - span;

            let mut recorder = Recorder {
                width,
                height,
                written: Vec::new(),
            };
            recorder.draw_line(x0, y0, x1, y1, RgbaColor::new(1, 2, 3, 255));
            let expected = reference_walk(x0, y0, x1, y1, width as i64, height as i64);
            assert_eq!(
                recorder.written, expected,
                "line ({x0},{y0}) -> ({x1},{y1}) drew different pixels after the skip"
            );
        }
    }

    /// The whole point: a line spanning the `i32` range must cost about as
    /// much as one spanning the target. Before the skip this was 4.29 billion
    /// iterations and about eight seconds.
    #[test]
    fn an_enormous_line_costs_what_a_small_one_costs() {
        let mut recorder = Recorder {
            width: 800,
            height: 600,
            written: Vec::new(),
        };
        let started = std::time::Instant::now();
        recorder.draw_line(
            i32::MIN as i64,
            0,
            i32::MAX as i64,
            1,
            RgbaColor::new(1, 2, 3, 255),
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "an i32-spanning line took {elapsed:?}"
        );
        assert!(
            !recorder.written.is_empty(),
            "the line crosses the target, so it must still draw something"
        );
        assert!(
            recorder.written.len() <= 800,
            "wrote {} pixels on an 800-wide target",
            recorder.written.len()
        );
    }

    /// A `DrawOp`'s `u32` geometry against a target a few hundred pixels
    /// tall: the row loops must cost what the target costs, not what the
    /// rectangle claims. Both rounded shapes iterated `rect.height` times
    /// regardless, even though `fill_rect` underneath them clamped and drew
    /// nothing.
    #[test]
    fn an_enormous_rounded_rect_costs_what_the_target_costs() {
        for huge in [
            RasterRect::new(0, 0, u32::MAX as usize, u32::MAX as usize),
            RasterRect::new(0, 0, 40, u32::MAX as usize),
        ] {
            let mut recorder = Recorder {
                width: 37,
                height: 23,
                written: Vec::new(),
            };
            let color = RgbaColor::new(1, 2, 3, 255);

            let started = std::time::Instant::now();
            recorder.fill_rounded_rect(huge, 4, color);
            recorder.draw_rounded_border(huge, 4, 2, color);
            let elapsed = started.elapsed();

            assert!(
                elapsed < std::time::Duration::from_millis(200),
                "rounded shapes of {huge:?} took {elapsed:?}"
            );
            assert!(
                !recorder.written.is_empty(),
                "the shape covers the target, so it must still draw"
            );
        }
    }

    /// Clamping the row loop must not change what a rectangle inside the
    /// target draws.
    #[test]
    fn clamping_the_rows_does_not_change_a_shape_that_fits() {
        let rect = RasterRect::new(2, 3, 20, 12);
        let color = RgbaColor::new(1, 2, 3, 255);
        let mut fill = Recorder {
            width: 37,
            height: 23,
            written: Vec::new(),
        };
        fill.fill_rounded_rect(rect, 5, color);
        assert!(
            fill.written.iter().all(|&(x, y)| x < 37 && y < 23),
            "a fitting shape drew outside the target"
        );
        // Every row of the rectangle contributes, so the clamp cannot have
        // cut one off.
        let rows: std::collections::BTreeSet<usize> =
            fill.written.iter().map(|&(_, y)| y).collect();
        assert_eq!(rows.len(), 12, "the clamp dropped rows that fit");
    }

    /// Vertical and horizontal lines are the dominant-axis edge cases, and a
    /// zero-length one is the degenerate case the skip must not swallow.
    #[test]
    fn degenerate_lines_survive_the_skip() {
        for (x0, y0, x1, y1) in [
            (5i64, 5i64, 5i64, 5i64),
            (5, -1000, 5, 1000),
            (-1000, 5, 1000, 5),
            (0, 0, 0, 0),
        ] {
            let mut recorder = Recorder {
                width: 37,
                height: 23,
                written: Vec::new(),
            };
            recorder.draw_line(x0, y0, x1, y1, RgbaColor::new(1, 2, 3, 255));
            assert_eq!(
                recorder.written,
                reference_walk(x0, y0, x1, y1, 37, 23),
                "line ({x0},{y0}) -> ({x1},{y1})"
            );
        }
    }
}
