//! A program's card (PROC-005): a card on the desk whose content a
//! program in ring 3 describes.
//!
//! The program sends views (`app_protocol`): widgets in colour roles,
//! buttons that stand for keys. The card checks each view against its
//! canvas and draws it with the desk's own widgets in the desk's theme;
//! a view that does not check out is refused, and the last good one
//! stays. Keys typed into the card and buttons clicked in it go back to
//! the program as key events, and the card tells it its size whenever
//! that changes. The program never touches the desk's memory, and the
//! desk never runs the program's code.

extern crate alloc;

use alloc::format;
use alloc::string::String;

use app_protocol::{Event, Kind, OwnedOp, Role, View, ViewError};
use view_types::PixelRect;

use crate::widgets::{rect, ButtonKind, Palette, Ui};

#[derive(Debug, Clone)]
pub struct ProgramCard {
    /// The program's thread.
    pub thread: u32,
    pub name: String,
    view: Option<View>,
    /// The canvas size the program was last told.
    told: Option<(u16, u16)>,
    /// Why the last view was refused, if it was.
    refused: Option<ViewError>,
}

impl ProgramCard {
    pub fn new(thread: u32, name: &str) -> Self {
        Self {
            thread,
            name: String::from(name),
            view: None,
            told: None,
            refused: None,
        }
    }

    /// A view from the program, checked against a `w` x `h` canvas.
    pub fn set_view(&mut self, bytes: &[u8], w: u32, h: u32) {
        match View::decode(bytes, clamp16(w), clamp16(h)) {
            Ok(view) => {
                self.view = Some(view);
                self.refused = None;
            }
            Err(why) => self.refused = Some(why),
        }
    }

    /// The size event to send, if the canvas is not the size the program
    /// was last told.
    pub fn resized(&mut self, w: u32, h: u32) -> Option<Event> {
        let size = (clamp16(w), clamp16(h));
        if self.told == Some(size) {
            return None;
        }
        self.told = Some(size);
        Some(Event::Size {
            w: size.0,
            h: size.1,
        })
    }

    pub fn title(&self) -> String {
        match &self.view {
            Some(view) if !view.title.is_empty() => view.title.clone(),
            _ => self.name.clone(),
        }
    }

    pub fn footer(&self) -> String {
        match (&self.refused, &self.view) {
            (Some(why), _) => format!("{} sent a view the desk refused ({why:?})", self.name),
            (None, Some(view)) => view.footer.clone(),
            (None, None) => format!("{} is starting", self.name),
        }
    }

    /// The card's content, drawn from the program's view.
    pub fn ui(&self, w: u32, h: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let mut ui = Ui::new(palette, hover);
        let Some(view) = &self.view else {
            let line = format!("Starting {}...", self.name);
            ui.text_centered(&rect(0, 0, w, h), &line, palette.muted, 1);
            return ui;
        };
        for op in &view.ops {
            match op {
                OwnedOp::Fill { area, role, radius } => {
                    ui.fill(pixels(*area), color(&palette, *role), *radius as u32)
                }
                OwnedOp::Outline {
                    area,
                    role,
                    radius,
                    thickness,
                } => ui.outline(
                    pixels(*area),
                    color(&palette, *role),
                    *radius as u32,
                    (*thickness).max(1) as u32,
                ),
                OwnedOp::Text {
                    x,
                    y,
                    role,
                    scale,
                    text,
                } => ui.text(*x as i32, *y as i32, text, color(&palette, *role), *scale),
                OwnedOp::TextRight {
                    right,
                    y,
                    role,
                    scale,
                    text,
                } => ui.text_right(
                    *right as i32,
                    *y as i32,
                    text,
                    color(&palette, *role),
                    *scale,
                ),
                OwnedOp::Button {
                    area,
                    kind,
                    key,
                    label,
                } => ui.button(pixels(*area), label, *key, button_kind(*kind)),
                OwnedOp::Line {
                    from,
                    to,
                    role,
                    thickness,
                } => ui.line(
                    from.0 as i32,
                    from.1 as i32,
                    to.0 as i32,
                    to.1 as i32,
                    color(&palette, *role),
                    *thickness as i32,
                ),
            }
        }
        ui
    }
}

fn clamp16(v: u32) -> u16 {
    v.min(u16::MAX as u32) as u16
}

fn pixels(a: app_protocol::Area) -> PixelRect {
    PixelRect {
        x: a.x as u32,
        y: a.y as u32,
        width: a.w as u32,
        height: a.h as u32,
    }
}

fn color(p: &Palette, role: Role) -> view_types::Color {
    match role {
        Role::Surface => p.surface,
        Role::Raised => p.raised,
        Role::Text => p.text,
        Role::Muted => p.muted,
        Role::Accent => p.accent,
        Role::OnAccent => p.on_accent,
        Role::Hairline => p.hairline,
    }
}

fn button_kind(kind: Kind) -> ButtonKind {
    match kind {
        Kind::Plain => ButtonKind::Plain,
        Kind::Accent => ButtonKind::Accent,
        Kind::Quiet => ButtonKind::Quiet,
        Kind::Primary => ButtonKind::Primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_protocol::{Area, Op, ViewWriter};

    fn palette() -> Palette {
        Palette::from_theme(&services_gui_host::Theme::default())
    }

    fn counter_view(buf: &mut [u8]) -> &[u8] {
        let mut v = ViewWriter::new(buf, "Counter: 3", "Space adds one");
        v.op(Op::Text {
            x: 20,
            y: 20,
            role: Role::Text,
            scale: 2,
            text: "3",
        })
        .op(Op::Button {
            area: Area::new(20, 80, 100, 40),
            kind: Kind::Primary,
            key: b' ',
            label: "Add one",
        });
        v.finish().unwrap()
    }

    #[test]
    fn a_click_on_a_programs_button_is_its_key() {
        let mut card = ProgramCard::new(7, "counter");
        let mut buf = [0u8; 512];
        card.set_view(counter_view(&mut buf), 400, 300);
        let ui = card.ui(400, 300, palette(), None);
        assert_eq!(ui.hit(30, 90), Some(b' '));
        assert_eq!(ui.hit(5, 5), None);
        assert_eq!(card.title(), "Counter: 3");
        assert_eq!(card.footer(), "Space adds one");
    }

    #[test]
    fn a_view_that_does_not_fit_is_refused_and_the_last_good_one_stays() {
        let mut card = ProgramCard::new(7, "counter");
        assert_eq!(card.title(), "counter");
        assert!(card.footer().contains("starting"));
        let mut buf = [0u8; 512];
        let good = counter_view(&mut buf).to_vec();
        card.set_view(&good, 400, 300);
        // The same view on a canvas too small for its button.
        card.set_view(&good, 100, 100);
        assert!(card.footer().contains("refused"));
        assert_eq!(card.title(), "Counter: 3", "the good view stays");
        card.set_view(b"garbage", 400, 300);
        assert!(card.footer().contains("NotAView"));
    }

    #[test]
    fn the_program_hears_its_size_once_per_change() {
        let mut card = ProgramCard::new(7, "counter");
        assert_eq!(card.resized(400, 300), Some(Event::Size { w: 400, h: 300 }));
        assert_eq!(card.resized(400, 300), None);
        assert_eq!(card.resized(500, 300), Some(Event::Size { w: 500, h: 300 }));
        assert_eq!(
            card.resized(100_000, 5),
            Some(Event::Size { w: u16::MAX, h: 5 })
        );
    }

    #[test]
    fn roles_follow_the_theme() {
        let p = palette();
        assert_eq!(color(&p, Role::Accent), p.accent);
        assert_eq!(color(&p, Role::Surface), p.surface);
    }
}
