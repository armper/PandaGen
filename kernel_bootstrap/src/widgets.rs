//! Widgets (GFX-081): real controls for cards, drawn in pixels.
//!
//! An app that wants graphics builds a [`Ui`] each frame: it places
//! buttons, labels and fills in the card's content area, in that area's
//! own pixel space, and the `Ui` turns them into the compositor's draw
//! operations and into hit rectangles. A click is answered by [`Ui::hit`]
//! with the *key byte* the control stands for, so the pointer and the
//! keyboard reach exactly the same code in the app -- the rule the header
//! chips set (GFX-056), kept for everything inside the card.
//!
//! Immediate mode, no retained tree, no layout solver: a grid helper and
//! rectangles are enough for the cards this desk has, and every frame is
//! rebuilt from the app's state, which is what the rest of the desk does
//! with text. Host-testable: it is arithmetic over rectangles.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use services_gui_host::Theme;
use view_types::{Color, DrawOp, PixelRect, TextStyle};

/// The desktop font's cell, and the widget layer's unit of measure.
pub const GLYPH_W: u32 = 8;
pub const GLYPH_H: u32 = 16;
/// Corner radius of a button.
pub const BUTTON_RADIUS: u32 = 6;

/// The theme, as the six colours widgets need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub surface: Color,
    pub raised: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    /// Text on an accent fill.
    pub on_accent: Color,
}

fn color(c: graphics_rasterizer::RgbaColor) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            surface: color(theme.surface),
            raised: color(theme.surface_raised),
            text: color(theme.text),
            muted: color(theme.text_muted),
            accent: color(theme.accent),
            on_accent: color(theme.background),
        }
    }
}

/// How a button reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// A raised key with plain text: the digits.
    Plain,
    /// A raised key with accent text: the operators.
    Accent,
    /// A raised key with muted text: clear, delete.
    Quiet,
    /// An accent fill: the one thing to press.
    Primary,
}

pub const fn rect(x: i32, y: i32, width: u32, height: u32) -> PixelRect {
    PixelRect {
        x: if x < 0 { 0 } else { x as u32 },
        y: if y < 0 { 0 } else { y as u32 },
        width,
        height,
    }
}

fn contains(r: &PixelRect, x: i32, y: i32) -> bool {
    x >= r.x as i32 && y >= r.y as i32 && x < (r.x + r.width) as i32 && y < (r.y + r.height) as i32
}

/// `area` cut into `cols` x `rows` cells with `gap` between them, row by
/// row. Cells are whole pixels; the remainder is left at the right and
/// the bottom.
pub fn grid(area: PixelRect, cols: u32, rows: u32, gap: u32) -> Vec<PixelRect> {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let cell_w = area.width.saturating_sub(gap * (cols - 1)) / cols;
    let cell_h = area.height.saturating_sub(gap * (rows - 1)) / rows;
    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for r in 0..rows {
        for c in 0..cols {
            cells.push(PixelRect {
                x: area.x + c * (cell_w + gap),
                y: area.y + r * (cell_h + gap),
                width: cell_w,
                height: cell_h,
            });
        }
    }
    cells
}

/// The width of `text` drawn at `scale`.
pub fn text_width(text: &str, scale: u8) -> u32 {
    text.chars().count() as u32 * GLYPH_W * scale.max(1) as u32
}

/// One frame's worth of controls.
#[derive(Debug, Clone)]
pub struct Ui {
    palette: Palette,
    hover: Option<(i32, i32)>,
    ops: Vec<DrawOp>,
    hits: Vec<(PixelRect, u8)>,
}

impl Ui {
    /// `hover` is the pointer in canvas pixels, when it is over the card.
    pub fn new(palette: Palette, hover: Option<(i32, i32)>) -> Self {
        Self {
            palette,
            hover,
            ops: Vec::new(),
            hits: Vec::new(),
        }
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn hovered(&self, area: &PixelRect) -> bool {
        self.hover.is_some_and(|(x, y)| contains(area, x, y))
    }

    pub fn fill(&mut self, area: PixelRect, color: Color, radius: u32) {
        self.ops.push(if radius == 0 {
            DrawOp::Fill { rect: area, color }
        } else {
            DrawOp::RoundedFill {
                rect: area,
                radius,
                color,
            }
        });
    }

    pub fn outline(&mut self, area: PixelRect, color: Color, radius: u32, thickness: u32) {
        self.ops.push(DrawOp::RoundedBorder {
            rect: area,
            radius,
            thickness,
            color,
        });
    }

    /// Text with its top-left at `(x, y)`, `scale` times the font.
    pub fn text(&mut self, x: i32, y: i32, text: &str, color: Color, scale: u8) {
        if text.is_empty() || x < 0 || y < 0 {
            return;
        }
        self.ops.push(DrawOp::Text {
            x: x as u32,
            y: y as u32,
            text: String::from(text),
            color: Some(color),
            style: TextStyle {
                compact: false,
                muted: false,
                scale: scale.max(1),
            },
        });
    }

    /// Text ending at `right`.
    pub fn text_right(&mut self, right: i32, y: i32, text: &str, color: Color, scale: u8) {
        let x = right - text_width(text, scale) as i32;
        self.text(x, y, text, color, scale);
    }

    /// Text centred in `area`.
    pub fn text_centered(&mut self, area: &PixelRect, text: &str, color: Color, scale: u8) {
        let w = text_width(text, scale) as i32;
        let h = (GLYPH_H * scale.max(1) as u32) as i32;
        let x = area.x as i32 + (area.width as i32 - w) / 2;
        let y = area.y as i32 + (area.height as i32 - h) / 2;
        self.text(x, y, text, color, scale);
    }

    /// A button that stands for `key`: a rounded key with a centred
    /// label, outlined in the accent while the pointer is over it.
    pub fn button(&mut self, area: PixelRect, label: &str, key: u8, kind: ButtonKind) {
        let (fill, ink) = match kind {
            ButtonKind::Plain => (self.palette.raised, self.palette.text),
            ButtonKind::Accent => (self.palette.raised, self.palette.accent),
            ButtonKind::Quiet => (self.palette.raised, self.palette.muted),
            ButtonKind::Primary => (self.palette.accent, self.palette.on_accent),
        };
        self.fill(area, fill, BUTTON_RADIUS);
        if self.hovered(&area) {
            let outline = if kind == ButtonKind::Primary {
                self.palette.text
            } else {
                self.palette.accent
            };
            self.outline(area, outline, BUTTON_RADIUS, 2);
        }
        // One character reads as a key and is drawn large when the key is
        // tall enough; words stay at the font's size, so a row of buttons
        // is one size (GFX-083).
        let scale = if label.chars().count() == 1 && area.height >= 2 * GLYPH_H + 12 {
            2
        } else {
            1
        };
        self.text_centered(&area, label, ink, scale);
        self.hits.push((area, key));
    }

    /// Make `area` answer `key` to a click without drawing anything: for
    /// controls the app draws itself, such as a day or a row (GFX-083).
    pub fn hit_area(&mut self, area: PixelRect, key: u8) {
        self.hits.push((area, key));
    }

    /// The key under canvas pixel `(x, y)`, if a control is there. The
    /// last control placed wins, as it is drawn on top.
    pub fn hit(&self, x: i32, y: i32) -> Option<u8> {
        self.hits
            .iter()
            .rev()
            .find(|(area, _)| contains(area, x, y))
            .map(|(_, key)| *key)
    }

    pub fn into_ops(self) -> Vec<DrawOp> {
        self.ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette::from_theme(&Theme::DEFAULT)
    }

    #[test]
    fn a_grid_cuts_whole_cells_with_gaps() {
        let cells = grid(rect(0, 10, 284, 100), 4, 2, 6);
        assert_eq!(cells.len(), 8);
        assert_eq!(cells[0], rect(0, 10, 66, 47));
        assert_eq!(cells[1].x, 72);
        assert_eq!(cells[3].x, 216);
        assert_eq!(cells[4].y, 63);
        assert_eq!(text_width("12.5", 2), 64);
    }

    #[test]
    fn a_button_is_a_key_a_hit_answers_and_hover_outlines() {
        let mut ui = Ui::new(palette(), Some((30, 30)));
        ui.button(rect(10, 10, 60, 44), "7", b'7', ButtonKind::Plain);
        ui.button(rect(80, 10, 60, 44), "=", b'=', ButtonKind::Primary);
        assert_eq!(ui.hit(30, 30), Some(b'7'));
        assert_eq!(ui.hit(90, 20), Some(b'='));
        assert_eq!(ui.hit(75, 20), None, "the gap");
        assert!(ui.hovered(&rect(10, 10, 60, 44)));
        let ops = ui.into_ops();
        // Hovered: fill, outline, label; not hovered: fill, label.
        assert_eq!(ops.len(), 5);
        assert!(matches!(ops[1], DrawOp::RoundedBorder { .. }));
        assert!(matches!(&ops[2], DrawOp::Text { style, .. } if style.scale == 2));
        assert!(matches!(ops[3], DrawOp::RoundedFill { color, .. } if color == palette().accent));
        // Right-aligned text ends where asked.
        let mut ui = Ui::new(palette(), None);
        ui.text_right(200, 0, "43.5", palette().text, 2);
        assert!(matches!(ui.into_ops()[0], DrawOp::Text { x: 136, .. }));
    }
}
