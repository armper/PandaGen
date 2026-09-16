//! Scene primitives: scrollable regions (GFX-028).
//!
//! A `ScrollRegion` is a viewport onto content taller than itself. It owns
//! only the scroll offset and the content extent; drawing goes through a
//! `ContainerTarget`, so children paint in content coordinates and never
//! see the screen. The scrollbar thumb is computed, not stateful, so it can
//! never disagree with the offset it depicts.

use alloc::vec::Vec;
use graphics_rasterizer::{BitmapFont, ContainerTarget, RasterRect, RenderTarget, RgbaColor};
use serde::{Deserialize, Serialize};

/// Vertical viewport onto line-oriented content.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScrollRegion {
    /// Where the viewport sits in the parent target.
    pub viewport: RasterRect,
    /// Total content height in pixels.
    pub content_height: usize,
    /// First visible content pixel row.
    pub offset_y: usize,
}

impl ScrollRegion {
    pub fn new(viewport: RasterRect, content_height: usize) -> Self {
        Self {
            viewport,
            content_height,
            offset_y: 0,
        }
    }

    /// Largest offset that still shows a full viewport (or 0 when the
    /// content fits).
    pub fn max_offset(&self) -> usize {
        self.content_height.saturating_sub(self.viewport.height)
    }

    /// Whether content exceeds the viewport.
    pub fn is_scrollable(&self) -> bool {
        self.content_height > self.viewport.height
    }

    /// Clamp the offset into range; call after content changes.
    pub fn clamp(&mut self) {
        self.offset_y = self.offset_y.min(self.max_offset());
    }

    /// Scroll by a signed pixel delta, clamped.
    pub fn scroll_by(&mut self, delta: i32) {
        self.offset_y = if delta < 0 {
            self.offset_y.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            self.offset_y.saturating_add(delta as usize)
        };
        self.clamp();
    }

    /// Scroll so that `line` (of `line_height` px) is fully visible with the
    /// least movement.
    pub fn scroll_line_into_view(&mut self, line: usize, line_height: usize) {
        let top = line.saturating_mul(line_height);
        let bottom = top.saturating_add(line_height);
        if top < self.offset_y {
            self.offset_y = top;
        } else if bottom > self.offset_y + self.viewport.height {
            self.offset_y = bottom.saturating_sub(self.viewport.height);
        }
        self.clamp();
    }

    /// Range of line indices at least partially visible.
    pub fn visible_lines(&self, line_height: usize, line_count: usize) -> core::ops::Range<usize> {
        if line_height == 0 || self.viewport.height == 0 {
            return 0..0;
        }
        let first = self.offset_y / line_height;
        let last = (self.offset_y + self.viewport.height).div_ceil(line_height);
        first.min(line_count)..last.min(line_count)
    }

    /// Thumb rectangle inside `track` (a vertical strip), or `None` when the
    /// content fits and no scrollbar is needed. The thumb is never shorter
    /// than `min_len` pixels so it stays grabbable.
    pub fn scrollbar_thumb(&self, track: RasterRect, min_len: usize) -> Option<RasterRect> {
        if !self.is_scrollable() || track.height == 0 {
            return None;
        }
        let track_len = track.height;
        let len = (track_len * self.viewport.height / self.content_height)
            .max(min_len.min(track_len))
            .min(track_len);
        let travel = track_len - len;
        let max_offset = self.max_offset();
        let top = if max_offset == 0 {
            0
        } else {
            travel * self.offset_y / max_offset
        };
        Some(RasterRect::new(track.x, track.y + top, track.width, len))
    }

    /// Paint `lines` in content coordinates through the viewport, one
    /// `line_height` apart, drawing only lines that intersect it.
    pub fn render_lines(
        &self,
        target: &mut (impl RenderTarget + ?Sized),
        lines: &[impl AsRef<str>],
        font: &BitmapFont,
        line_height: usize,
        color: RgbaColor,
    ) -> usize {
        let range = self.visible_lines(line_height, lines.len());
        let painted = range.len();
        let mut view = ContainerTarget::scrolled(target, self.viewport, 0, self.offset_y);
        for index in range {
            view.draw_text_with_font(0, index * line_height, lines[index].as_ref(), font, color);
        }
        painted
    }

    /// Convenience: content height for `line_count` lines.
    pub fn content_for_lines(line_count: usize, line_height: usize) -> usize {
        line_count.saturating_mul(line_height)
    }

    /// Lines as owned strings, for callers that build content per frame.
    pub fn line_offsets(&self, line_height: usize, line_count: usize) -> Vec<(usize, usize)> {
        self.visible_lines(line_height, line_count)
            .map(|index| (index, index * line_height))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use graphics_rasterizer::{RgbaBuffer, DESKTOP_FONT};

    const BG: RgbaColor = RgbaColor::new(0, 0, 0, 255);
    const FG: RgbaColor = RgbaColor::new(255, 255, 255, 255);

    #[test]
    fn test_offset_clamping_and_scrolling() {
        let mut region = ScrollRegion::new(RasterRect::new(0, 0, 10, 40), 100);
        assert!(region.is_scrollable());
        assert_eq!(region.max_offset(), 60);
        region.scroll_by(25);
        assert_eq!(region.offset_y, 25);
        region.scroll_by(1000);
        assert_eq!(region.offset_y, 60);
        region.scroll_by(-10);
        assert_eq!(region.offset_y, 50);
        region.scroll_by(-1000);
        assert_eq!(region.offset_y, 0);

        // Content shrinks: offset follows.
        region.offset_y = 60;
        region.content_height = 50;
        region.clamp();
        assert_eq!(region.offset_y, 10);
        region.content_height = 30;
        region.clamp();
        assert_eq!(region.offset_y, 0);
        assert!(!region.is_scrollable());
    }

    #[test]
    fn test_visible_lines_and_scroll_into_view() {
        let mut region = ScrollRegion::new(RasterRect::new(0, 0, 10, 40), 20 * 18);
        assert_eq!(region.visible_lines(18, 20), 0..3);
        region.offset_y = 18;
        assert_eq!(region.visible_lines(18, 20), 1..4);
        region.offset_y = 27;
        assert_eq!(region.visible_lines(18, 20), 1..4);
        assert_eq!(region.visible_lines(18, 2), 1..2, "capped at line count");
        assert_eq!(region.visible_lines(0, 20), 0..0);

        region.offset_y = 0;
        region.scroll_line_into_view(10, 18);
        assert_eq!(region.offset_y, 11 * 18 - 40);
        assert!(region.visible_lines(18, 20).contains(&10));
        region.scroll_line_into_view(2, 18);
        assert_eq!(region.offset_y, 36);
        // Already visible: no movement.
        let before = region.offset_y;
        region.scroll_line_into_view(3, 18);
        assert_eq!(region.offset_y, before);
        // Beyond content clamps.
        region.scroll_line_into_view(500, 18);
        assert_eq!(region.offset_y, region.max_offset());
    }

    #[test]
    fn test_scrollbar_thumb_geometry() {
        let track = RasterRect::new(90, 0, 4, 100);
        let mut region = ScrollRegion::new(RasterRect::new(0, 0, 90, 50), 200);
        let thumb = region.scrollbar_thumb(track, 8).unwrap();
        assert_eq!(thumb, RasterRect::new(90, 0, 4, 25));
        region.offset_y = region.max_offset();
        let thumb = region.scrollbar_thumb(track, 8).unwrap();
        assert_eq!(thumb.y + thumb.height, 100, "thumb reaches the track end");
        region.offset_y = 75;
        let thumb = region.scrollbar_thumb(track, 8).unwrap();
        assert_eq!(thumb.y, 37);

        // Very long content: thumb honours the minimum length.
        let long = ScrollRegion::new(RasterRect::new(0, 0, 90, 50), 100_000);
        assert_eq!(long.scrollbar_thumb(track, 8).unwrap().height, 8);

        // Fits: no scrollbar.
        let fits = ScrollRegion::new(RasterRect::new(0, 0, 90, 50), 50);
        assert_eq!(fits.scrollbar_thumb(track, 8), None);
    }

    #[test]
    fn test_render_lines_paints_only_visible_lines_through_viewport() {
        let lines: Vec<alloc::string::String> = (0..10).map(|i| i.to_string()).collect();
        let line_height = 18;
        let viewport = RasterRect::new(4, 4, 40, 36);
        let mut region =
            ScrollRegion::new(viewport, ScrollRegion::content_for_lines(10, line_height));
        let mut target = RgbaBuffer::new(60, 60, BG);
        let painted = region.render_lines(&mut target, &lines, &DESKTOP_FONT, line_height, FG);
        assert_eq!(painted, 2);

        // Line "0" at the viewport top; nothing above or outside the viewport.
        let mut reference = RgbaBuffer::new(60, 60, BG);
        reference.draw_text_with_font(4, 4, "0", &DESKTOP_FONT, FG);
        reference.draw_text_with_font(4, 22, "1", &DESKTOP_FONT, FG);
        assert_eq!(target.as_bytes(), reference.as_bytes());

        // Scroll by half a line: line 0 is clipped at the top edge and line 2 peeks in.
        region.scroll_by(9);
        let mut target = RgbaBuffer::new(60, 60, BG);
        let painted = region.render_lines(&mut target, &lines, &DESKTOP_FONT, line_height, FG);
        assert_eq!(painted, 3);
        for x in 0..60 {
            assert_eq!(target.pixel(x, 3), Some(BG), "nothing above the viewport");
            assert_eq!(target.pixel(x, 40), Some(BG), "nothing below the viewport");
        }
        assert_eq!(
            region.line_offsets(line_height, 10),
            vec![(0, 0), (1, 18), (2, 36)]
        );
    }
}
