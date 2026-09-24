//! Sketch (GFX-080): draw with the pointer.
//!
//! The first card whose content is not text: strokes, drawn as lines in
//! the card's own pixel space through the compositor's graphics content.
//! Press to start a stroke, move to draw, let go to finish; the header's
//! chips (and keys) change the colour, undo a stroke and clear.
//!
//! The drawing is a document too, `sketch`: one stroke per line, the
//! colour then the points, as text -- so it is kept, versioned and saved
//! by the desk a second after it changes, like the task list.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::{Color, DrawOp, PixelRect};

/// The document the drawing lives in.
pub const SKETCH_FILE: &str = "sketch";
/// How long the drawing sits changed before it saves, in ticks.
pub const SAVE_AFTER_TICKS: u64 = 100;
/// The colours, in the order the Colour chip cycles them.
pub const COLORS: [(&str, Color); 6] = [
    ("white", Color::rgb(236, 240, 248)),
    ("red", Color::rgb(239, 83, 80)),
    ("yellow", Color::rgb(250, 204, 21)),
    ("green", Color::rgb(52, 211, 153)),
    ("blue", Color::rgb(96, 165, 250)),
    ("pink", Color::rgb(244, 114, 182)),
];
/// Most points a stroke keeps, and most strokes a drawing keeps.
pub const MAX_POINTS: usize = 2_000;
pub const MAX_STROKES: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stroke {
    pub color: usize,
    pub points: Vec<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchEffect {
    None,
    Redraw,
    Close,
}

#[derive(Debug, Clone)]
pub struct SketchView {
    strokes: Vec<Stroke>,
    /// A stroke is being drawn: the last one.
    drawing: bool,
    pub color: usize,
    pub loaded: bool,
    changed_at: Option<u64>,
    now: u64,
}

impl Default for SketchView {
    fn default() -> Self {
        Self::new()
    }
}

impl SketchView {
    pub fn new() -> Self {
        Self {
            strokes: Vec::new(),
            drawing: false,
            color: 0,
            loaded: false,
            changed_at: None,
            now: 0,
        }
    }

    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    fn changed(&mut self) {
        self.changed_at = Some(self.now);
    }

    /// The pointer pressed at `(x, y)` in the canvas.
    pub fn begin(&mut self, x: i32, y: i32) {
        if self.strokes.len() >= MAX_STROKES {
            return;
        }
        self.strokes.push(Stroke {
            color: self.color,
            points: alloc::vec![(x, y)],
        });
        self.drawing = true;
        self.changed();
    }

    /// The pointer moved with the button down.
    pub fn extend(&mut self, x: i32, y: i32) -> bool {
        if !self.drawing {
            return false;
        }
        let Some(stroke) = self.strokes.last_mut() else {
            return false;
        };
        if stroke.points.last() == Some(&(x, y)) || stroke.points.len() >= MAX_POINTS {
            return false;
        }
        stroke.points.push((x, y));
        self.changed();
        true
    }

    /// The button came up.
    pub fn end(&mut self) {
        self.drawing = false;
    }

    pub fn undo(&mut self) -> bool {
        self.drawing = false;
        if self.strokes.pop().is_some() {
            self.changed();
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) -> bool {
        self.drawing = false;
        if self.strokes.is_empty() {
            return false;
        }
        self.strokes.clear();
        self.changed();
        true
    }

    pub fn next_color(&mut self) {
        self.color = (self.color + 1) % COLORS.len();
    }

    /// `c` cycles the colour, `z` undoes a stroke, `x` clears.
    pub fn handle_byte(&mut self, byte: u8) -> SketchEffect {
        use crate::notepad::{CTRL_W, CTRL_Z, ESC};
        match byte {
            b'c' | b'C' => {
                self.next_color();
                SketchEffect::Redraw
            }
            b'z' | b'Z' | CTRL_Z => {
                if self.undo() {
                    SketchEffect::Redraw
                } else {
                    SketchEffect::None
                }
            }
            b'x' | b'X' => {
                if self.clear() {
                    SketchEffect::Redraw
                } else {
                    SketchEffect::None
                }
            }
            CTRL_W | ESC => SketchEffect::Close,
            _ => SketchEffect::None,
        }
    }

    /// The card's drawing: every stroke as lines (a lone point as a dot),
    /// and a swatch of the current colour in the top-right corner.
    pub fn ops(&self, width: u32) -> Vec<DrawOp> {
        let mut ops = Vec::new();
        for stroke in &self.strokes {
            let color = COLORS[stroke.color.min(COLORS.len() - 1)].1;
            if stroke.points.len() == 1 {
                let (x, y) = stroke.points[0];
                ops.push(DrawOp::Fill {
                    rect: PixelRect {
                        x: (x - 1).max(0) as u32,
                        y: (y - 1).max(0) as u32,
                        width: 3,
                        height: 3,
                    },
                    color,
                });
                continue;
            }
            for pair in stroke.points.windows(2) {
                let (x0, y0) = pair[0];
                let (x1, y1) = pair[1];
                ops.push(DrawOp::Line {
                    x0,
                    y0,
                    x1,
                    y1,
                    color,
                });
                // A second line one pixel down: a stroke, not a hairline.
                ops.push(DrawOp::Line {
                    x0,
                    y0: y0 + 1,
                    x1,
                    y1: y1 + 1,
                    color,
                });
            }
        }
        ops.push(DrawOp::Fill {
            rect: PixelRect {
                x: width.saturating_sub(14),
                y: 2,
                width: 12,
                height: 12,
            },
            color: COLORS[self.color].1,
        });
        ops
    }

    /// The document: one stroke per line, `colour: x,y x,y ...`.
    pub fn content(&self) -> String {
        let mut text = String::new();
        for stroke in &self.strokes {
            text.push_str(&alloc::format!("{}:", stroke.color));
            for (x, y) in &stroke.points {
                text.push_str(&alloc::format!(" {x},{y}"));
            }
            text.push('\n');
        }
        text
    }

    pub fn parse(text: &str) -> Vec<Stroke> {
        text.lines()
            .filter_map(|line| {
                let (color, rest) = line.split_once(':')?;
                let color: usize = color.trim().parse().ok()?;
                let points: Vec<(i32, i32)> = rest
                    .split_whitespace()
                    .filter_map(|p| {
                        let (x, y) = p.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                    .take(MAX_POINTS)
                    .collect();
                (!points.is_empty()).then_some(Stroke {
                    color: color.min(COLORS.len() - 1),
                    points,
                })
            })
            .take(MAX_STROKES)
            .collect()
    }

    /// The desk read the document.
    pub fn load(&mut self, text: &str) {
        self.strokes = Self::parse(text);
        self.drawing = false;
        self.loaded = true;
        self.changed_at = None;
    }

    /// The desk's clock; a changed drawing that has sat still asks to
    /// save -- not while a stroke is still being drawn.
    pub fn save_due(&mut self, now: u64) -> Option<String> {
        self.now = now;
        if self.drawing {
            return None;
        }
        let since = self.changed_at?;
        if now.saturating_sub(since) < SAVE_AFTER_TICKS {
            return None;
        }
        self.changed_at = None;
        Some(self.content())
    }

    pub fn is_dirty(&self) -> bool {
        self.changed_at.is_some()
    }

    pub fn footer(&self) -> String {
        let state = if self.is_dirty() { "   saving..." } else { "" };
        alloc::format!(
            "{} stroke{}   {}   C colour   Z undo   X clear{state}",
            self.strokes.len(),
            if self.strokes.len() == 1 { "" } else { "s" },
            COLORS[self.color].0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_are_drawn_as_lines_and_kept_as_text() {
        let mut sketch = SketchView::new();
        sketch.load("");
        assert!(sketch.strokes().is_empty() && sketch.loaded);
        sketch.begin(10, 10);
        assert!(sketch.extend(20, 15));
        assert!(!sketch.extend(20, 15), "the same point twice is nothing");
        sketch.extend(30, 30);
        sketch.end();
        assert!(!sketch.extend(40, 40), "not drawing");
        sketch.handle_byte(b'c');
        sketch.begin(5, 5);
        sketch.end();
        assert_eq!(sketch.strokes().len(), 2);
        assert_eq!(sketch.content(), "0: 10,10 20,15 30,30\n1: 5,5\n");
        let ops = sketch.ops(300);
        // Two segments, doubled, one dot, one swatch.
        assert_eq!(ops.len(), 2 * 2 + 1 + 1);
        assert!(matches!(
            ops[0],
            DrawOp::Line {
                x0: 10,
                y0: 10,
                x1: 20,
                y1: 15,
                ..
            }
        ));
        assert!(matches!(ops.last(), Some(DrawOp::Fill { rect, .. }) if rect.x == 286));
        assert_eq!(
            sketch.footer(),
            "2 strokes   red   C colour   Z undo   X clear   saving..."
        );
        // Round trip.
        let mut again = SketchView::new();
        again.load(&sketch.content());
        assert_eq!(again.strokes(), sketch.strokes());
        // Undo, clear, and the save clock.
        assert!(sketch.undo());
        assert_eq!(sketch.strokes().len(), 1);
        sketch.save_due(0);
        assert_eq!(
            sketch.save_due(SAVE_AFTER_TICKS),
            Some("0: 10,10 20,15 30,30\n".to_string())
        );
        assert!(!sketch.is_dirty());
        assert_eq!(sketch.handle_byte(b'x'), SketchEffect::Redraw);
        assert_eq!(sketch.handle_byte(b'x'), SketchEffect::None);
        assert_eq!(sketch.handle_byte(0x1B), SketchEffect::Close);
        // A stroke in progress does not save.
        sketch.begin(1, 1);
        assert_eq!(sketch.save_due(10 * SAVE_AFTER_TICKS), None);
        sketch.end();
        assert!(sketch.save_due(20 * SAVE_AFTER_TICKS).is_some());
        assert_eq!(
            SketchView::parse("junk\n9: 1,1\n"),
            alloc::vec![Stroke {
                color: 5,
                points: alloc::vec![(1, 1)]
            }]
        );
    }
}
