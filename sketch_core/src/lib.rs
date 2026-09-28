//! Sketch (GFX-080; a program since PROC-012): draw with the pointer.
//!
//! Press to start a stroke, move to draw, let go to finish. A toolbar
//! across the top picks the colour and undoes or clears; so do keys.
//!
//! The drawing is a document, `sketch`: one stroke per line, the colour
//! then the points, as text -- kept, versioned, and saved a second after
//! it changes. The program (`apps/sketch`) holds exactly that document,
//! to read and write, and nothing else a person keeps; the pointer comes
//! to it as events on its card, and each stroke goes back as one path.
//!
//! A whole drawing is one view, so it is bounded: [`MAX_STROKES`]
//! strokes and [`MAX_TOTAL_POINTS`] points in all. Past that, a new
//! stroke is refused and the footer says the drawing is full.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use app_protocol::{Area, Kind, Op, PointerKind, Role, ViewWriter};

/// The document the drawing lives in.
pub const SKETCH_FILE: &str = "sketch";
/// How long the drawing sits changed before it saves, in ticks.
pub const SAVE_AFTER_TICKS: u64 = 100;
/// The colours, in the order `c` cycles them.
pub const COLORS: [(&str, (u8, u8, u8)); 6] = [
    ("white", (236, 240, 248)),
    ("red", (239, 83, 80)),
    ("yellow", (250, 204, 21)),
    ("green", (52, 211, 153)),
    ("blue", (96, 165, 250)),
    ("pink", (244, 114, 182)),
];
/// The toolbar across the top of the canvas: its height, a swatch's size
/// and the gap between swatches. Strokes begin below it.
pub const TOOLBAR_H: u16 = 40;
pub const SWATCH: u16 = 22;
pub const SWATCH_GAP: u16 = 10;
/// Undo and Clear on the toolbar's right.
pub const TOOL_BUTTON: (u16, u16) = (64, 28);

/// Most points a stroke keeps, most strokes a drawing keeps, and most
/// points in all: what one view can carry.
pub const MAX_POINTS: usize = 2_000;
pub const MAX_STROKES: usize = 200;
pub const MAX_TOTAL_POINTS: usize = 12_000;

/// Keys (the desk's).
pub const CTRL_W: u8 = 0x17;
pub const CTRL_Z: u8 = 0x1A;
pub const ESC: u8 = 0x1B;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stroke {
    pub color: usize,
    pub points: Vec<(u16, u16)>,
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

    fn total_points(&self) -> usize {
        self.strokes.iter().map(|s| s.points.len()).sum()
    }

    /// Whether the drawing has no room for another stroke.
    pub fn full(&self) -> bool {
        self.strokes.len() >= MAX_STROKES || self.total_points() >= MAX_TOTAL_POINTS
    }

    /// The pointer pressed at `(x, y)` in the canvas.
    pub fn begin(&mut self, x: u16, y: u16) {
        if self.full() {
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
    pub fn extend(&mut self, x: u16, y: u16) -> bool {
        if !self.drawing || self.total_points() >= MAX_TOTAL_POINTS {
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

    /// The pointer, as the card's events say it: a press below the
    /// toolbar begins a stroke, a move draws, a release ends. Whether the
    /// drawing changed.
    pub fn pointer(&mut self, kind: PointerKind, x: u16, y: u16) -> bool {
        match kind {
            PointerKind::Down if y >= TOOLBAR_H => {
                let before = self.strokes.len();
                self.begin(x, y);
                self.strokes.len() != before
            }
            PointerKind::Down => false,
            PointerKind::Move => self.extend(x, y.max(TOOLBAR_H)),
            PointerKind::Up => {
                self.end();
                false
            }
        }
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

    /// `c` cycles the colour, `1` to `6` pick one (GFX-106), `z` undoes a
    /// stroke, `x` clears.
    pub fn handle_byte(&mut self, byte: u8) -> SketchEffect {
        match byte {
            b'1'..=b'6' => {
                self.color = (byte - b'1') as usize;
                SketchEffect::Redraw
            }
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

    /// The card: the strokes, each a path, then the toolbar over the top
    /// of them -- a swatch a colour, the current one ringed, and Undo and
    /// Clear on the right. Each control answers its key to a click.
    pub fn draw(&self, width: u16, height: u16, view: &mut ViewWriter) {
        if width < 6 * (SWATCH + SWATCH_GAP) + 2 * TOOL_BUTTON.0 + 40 || height < TOOLBAR_H + 20 {
            view.op(Op::Text {
                x: 0,
                y: 0,
                role: Role::Muted,
                scale: 1,
                text: "Make me bigger",
            });
            return;
        }
        for stroke in &self.strokes {
            let (r, g, b) = COLORS[stroke.color.min(COLORS.len() - 1)].1;
            // Points past the card's edge (it has shrunk) are left out.
            let inside: Vec<(u16, u16)> = stroke
                .points
                .iter()
                .copied()
                .filter(|&(x, y)| x <= width && y <= height)
                .collect();
            if inside.is_empty() {
                continue;
            }
            view.op(Op::Path {
                role: Role::Rgb(r, g, b),
                thickness: 2,
                points: &inside,
            });
        }
        view.op(Op::Fill {
            area: Area::new(0, 0, width, TOOLBAR_H),
            role: Role::Raised,
            radius: 10,
        });
        let top = (TOOLBAR_H - SWATCH) / 2;
        for (i, (_, (r, g, b))) in COLORS.iter().enumerate() {
            let area = Area::new(10 + i as u16 * (SWATCH + SWATCH_GAP), top, SWATCH, SWATCH);
            if i == self.color {
                view.op(Op::Outline {
                    area: Area::new(area.x - 4, area.y - 4, SWATCH + 8, SWATCH + 8),
                    role: Role::Text,
                    radius: ((SWATCH + 8) / 2) as u8,
                    thickness: 2,
                });
            }
            view.op(Op::Fill {
                area,
                role: Role::Rgb(*r, *g, *b),
                radius: (SWATCH / 2) as u8,
            })
            .op(Op::Hit {
                area,
                key: b'1' + i as u8,
            });
        }
        let (bw, bh) = TOOL_BUTTON;
        let by = (TOOLBAR_H - bh) / 2;
        let clear = Area::new(width.saturating_sub(8 + bw), by, bw, bh);
        let undo = Area::new(width.saturating_sub(16 + 2 * bw), by, bw, bh);
        view.op(Op::Button {
            area: undo,
            kind: Kind::Plain,
            key: b'z',
            label: "Undo",
        })
        .op(Op::Button {
            area: clear,
            kind: Kind::Quiet,
            key: b'x',
            label: "Clear",
        });
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
                let points: Vec<(u16, u16)> = rest
                    .split_whitespace()
                    .filter_map(|p| {
                        let (x, y) = p.split_once(',')?;
                        // Kernel-era drawings kept signed points.
                        let (x, y): (i32, i32) = (x.parse().ok()?, y.parse().ok()?);
                        Some((
                            x.clamp(0, u16::MAX as i32) as u16,
                            y.clamp(0, u16::MAX as i32) as u16,
                        ))
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

    /// The document, read.
    pub fn load(&mut self, text: &str) {
        self.strokes = Self::parse(text);
        self.drawing = false;
        self.loaded = true;
        self.changed_at = None;
    }

    /// The clock, in ticks; a changed drawing that has sat still asks to
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
        let state = if self.full() {
            "   the drawing is full"
        } else if self.is_dirty() {
            "   saving..."
        } else {
            ""
        };
        alloc::format!(
            "{} stroke{}   {}   1-6 colour   Z undo   X clear{state}",
            self.strokes.len(),
            if self.strokes.len() == 1 { "" } else { "s" },
            COLORS[self.color].0
        )
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use alloc::string::ToString;
    use app_protocol::{OwnedOp, View};

    fn view_of(sketch: &SketchView, w: u16, h: u16) -> View {
        let mut buf = alloc::vec![0u8; app_protocol::VIEW_MAX];
        let footer = sketch.footer();
        let mut view = ViewWriter::new(&mut buf, "Sketch", &footer);
        sketch.draw(w, h, &mut view);
        View::decode(view.finish().unwrap(), w, h).expect("a view the desk takes")
    }

    fn hit(sketch: &SketchView, x: u16, y: u16) -> Option<u8> {
        view_of(sketch, 504, 400)
            .ops
            .iter()
            .rev()
            .find_map(|op| match op {
                OwnedOp::Button { area, key, .. } | OwnedOp::Hit { area, key }
                    if x >= area.x && y >= area.y && x < area.x + area.w && y < area.y + area.h =>
                {
                    Some(*key)
                }
                _ => None,
            })
    }

    #[test]
    fn strokes_are_drawn_as_paths_and_kept_as_text() {
        let mut sketch = SketchView::new();
        sketch.load("");
        assert!(sketch.strokes().is_empty() && sketch.loaded);
        // The pointer, as the card's events bring it.
        assert!(sketch.pointer(PointerKind::Down, 10, 50));
        assert!(sketch.pointer(PointerKind::Move, 20, 55));
        assert!(
            !sketch.pointer(PointerKind::Move, 20, 55),
            "the same point twice is nothing"
        );
        sketch.pointer(PointerKind::Move, 30, 70);
        sketch.pointer(PointerKind::Up, 30, 70);
        assert!(!sketch.pointer(PointerKind::Move, 40, 80), "not drawing");
        assert!(
            !sketch.pointer(PointerKind::Down, 40, 10),
            "the toolbar is not canvas"
        );
        sketch.handle_byte(b'c');
        sketch.pointer(PointerKind::Down, 5, 45);
        sketch.pointer(PointerKind::Up, 5, 45);
        assert_eq!(sketch.strokes().len(), 2);
        assert_eq!(sketch.content(), "0: 10,50 20,55 30,70\n1: 5,45\n");
        let view = view_of(&sketch, 504, 400);
        let paths: Vec<&OwnedOp> = view
            .ops
            .iter()
            .filter(|op| matches!(op, OwnedOp::Path { .. }))
            .collect();
        assert_eq!(paths.len(), 2);
        assert!(
            matches!(paths[0], OwnedOp::Path { points, role: Role::Rgb(236, 240, 248), .. } if points.len() == 3)
        );
        assert_eq!(
            sketch.footer(),
            "2 strokes   red   1-6 colour   Z undo   X clear   saving..."
        );
        // The toolbar: a swatch a colour, then Undo and Clear.
        let swatch = |i: u16| 10 + i * (SWATCH + SWATCH_GAP) + SWATCH / 2;
        assert_eq!(hit(&sketch, swatch(0), 20), Some(b'1'));
        assert_eq!(hit(&sketch, swatch(5), 20), Some(b'6'));
        assert_eq!(hit(&sketch, 504 - 8 - 32, 20), Some(b'x'));
        assert_eq!(hit(&sketch, 504 - 16 - 64 - 32, 20), Some(b'z'));
        assert_eq!(hit(&sketch, 250, 100), None, "the canvas is not a control");
        assert_eq!(sketch.handle_byte(b'5'), SketchEffect::Redraw);
        assert_eq!(sketch.color, 4);
        // Round trip, including a kernel-era drawing's signed points.
        let mut again = SketchView::new();
        again.load(&sketch.content());
        assert_eq!(again.strokes(), sketch.strokes());
        assert_eq!(
            SketchView::parse("0: -5,10\n")[0].points,
            alloc::vec![(0, 10)]
        );
        // Undo, clear, and the save clock.
        assert!(sketch.undo());
        sketch.save_due(0);
        assert_eq!(
            sketch.save_due(SAVE_AFTER_TICKS),
            Some("0: 10,50 20,55 30,70\n".to_string())
        );
        assert!(!sketch.is_dirty());
        assert_eq!(sketch.handle_byte(b'x'), SketchEffect::Redraw);
        assert_eq!(sketch.handle_byte(b'x'), SketchEffect::None);
        assert_eq!(sketch.handle_byte(ESC), SketchEffect::Close);
        // A stroke in progress does not save.
        sketch.pointer(PointerKind::Down, 1, 60);
        assert_eq!(sketch.save_due(10 * SAVE_AFTER_TICKS), None);
        sketch.pointer(PointerKind::Up, 1, 60);
        assert!(sketch.save_due(20 * SAVE_AFTER_TICKS).is_some());
    }

    #[test]
    fn a_full_drawing_still_fits_one_view_and_says_it_is_full() {
        let mut sketch = SketchView::new();
        let mut y = TOOLBAR_H;
        while !sketch.full() {
            sketch.pointer(PointerKind::Down, 0, y);
            for x in 1..MAX_POINTS as u16 {
                if !sketch.pointer(PointerKind::Move, x % 500, y + (x / 500)) {
                    break;
                }
            }
            sketch.pointer(PointerKind::Up, 0, 0);
            y = TOOLBAR_H + (y + 7) % 300;
        }
        assert!(sketch.footer().ends_with("the drawing is full"));
        let before = sketch.strokes().len();
        sketch.pointer(PointerKind::Down, 10, 100);
        assert_eq!(sketch.strokes().len(), before, "no room for another");
        // The whole drawing, in one view the desk takes.
        let view = view_of(&sketch, 520, 440);
        assert!(view.ops.len() <= app_protocol::OPS_MAX);
        // A card shrunk under the drawing leaves the rest out, and too small
        // a card says so.
        let _ = view_of(&sketch, 300, 200);
        assert_eq!(view_of(&sketch, 100, 30).ops.len(), 1);
    }
}
