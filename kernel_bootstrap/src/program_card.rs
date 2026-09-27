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
    /// Its program's crashes, for deciding about restarting it (PROC-007).
    pub crashes: crate::supervision::Crashes,
    /// What the card says while it has no program: why it is waiting.
    status: Option<String>,
    /// It crashed too often to be restarted; the person decides.
    gave_up: bool,
}

/// The key of the "Start it again" button on a card that gave up.
pub const START_AGAIN: u8 = b'r';

impl ProgramCard {
    pub fn new(thread: u32, name: &str) -> Self {
        Self {
            thread,
            name: String::from(name),
            view: None,
            told: None,
            refused: None,
            crashes: crate::supervision::Crashes::default(),
            status: None,
            gave_up: false,
        }
    }

    /// Program `thread` takes the card: it starts from a blank card and
    /// hears the canvas size afresh.
    pub fn bind(&mut self, thread: u32) {
        self.thread = thread;
        self.view = None;
        self.told = None;
        self.refused = None;
        self.status = None;
    }

    /// Its program was ended by the kernel (PROC-007): the card waits,
    /// saying why, and whether it starts the program again. Returns
    /// whether to.
    pub fn crashed(&mut self, how: &str, now: u64) -> bool {
        use crate::supervision::{Decision, RESTARTS};
        self.thread = 0;
        self.view = None;
        match self.crashes.crashed(now) {
            Decision::Restart(n) => {
                self.status = Some(format!("{how}. Starting it again ({n} of {RESTARTS})."));
                self.gave_up = false;
                true
            }
            Decision::GiveUp => {
                self.status = Some(format!(
                    "{how}. It has been ended {} times in a minute.",
                    RESTARTS + 1
                ));
                self.gave_up = true;
                false
            }
        }
    }

    /// A key while the card has no program: "Start it again", if it gave
    /// up. Returns whether to start the program.
    pub fn waiting_key(&mut self, key: u8) -> bool {
        if !self.gave_up || !(key == START_AGAIN || key == b'\n') {
            return false;
        }
        self.gave_up = false;
        self.crashes.forgive();
        self.status = Some(format!("Starting {}...", self.name));
        true
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
        if self.gave_up {
            return String::from("R or the button starts it again   Ctrl+W closes");
        }
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
            let line = match &self.status {
                Some(status) => status.clone(),
                None => format!("Starting {}...", self.name),
            };
            // The line, wrapped to the card, above the button if any.
            let per_line = (w.saturating_sub(32) / crate::widgets::GLYPH_W).max(8) as usize;
            let lines = wrap(&line, per_line);
            let total = lines.len() as u32 * (crate::widgets::GLYPH_H + 4);
            let top = h.saturating_sub(total + if self.gave_up { 64 } else { 0 }) / 2;
            for (i, text) in lines.iter().enumerate() {
                let y = top + i as u32 * (crate::widgets::GLYPH_H + 4);
                ui.text_centered(
                    &rect(0, y as i32, w, crate::widgets::GLYPH_H),
                    text,
                    palette.muted,
                    1,
                );
            }
            if self.gave_up {
                let bw = 180.min(w);
                let area = rect(((w - bw) / 2) as i32, (top + total + 20) as i32, bw, 40);
                ui.button(area, "Start it again", START_AGAIN, ButtonKind::Primary);
            }
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

/// `text` in lines of at most `width` characters, broken at spaces.
fn wrap(text: &str, width: usize) -> alloc::vec::Vec<String> {
    let mut lines = alloc::vec::Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(core::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
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
    fn a_card_whose_program_keeps_crashing_gives_up_and_offers_a_button() {
        let mut card = ProgramCard::new(7, "fragile");
        let how = "fragile: ended by the kernel: it touched 0x0, not its memory (at 0x400010)";
        for n in 1..=3 {
            assert!(card.crashed(how, n * 10), "restart {n}");
            assert_eq!(card.thread, 0);
            assert!(card.footer().contains("starting"), "{}", card.footer());
            card.bind(10 + n as u32);
            assert!(
                card.resized(400, 300).is_some(),
                "the new program hears its size"
            );
        }
        assert!(!card.crashed(how, 40), "a fourth in the minute");
        let ui = card.ui(400, 300, palette(), None);
        // The button is the only thing to hit, and it is "Start it again".
        let hit = (0..300).step_by(4).find_map(|y| ui.hit(200, y));
        assert_eq!(hit, Some(START_AGAIN));
        assert!(!card.waiting_key(b'x'));
        assert!(card.waiting_key(START_AGAIN), "the person starts it");
        assert!(!card.waiting_key(START_AGAIN), "once");
        // Forgiven: three more restarts before giving up again.
        assert!(card.crashed(how, 50));
    }

    #[test]
    fn roles_follow_the_theme() {
        let p = palette();
        assert_eq!(color(&p, Role::Accent), p.accent);
        assert_eq!(color(&p, Role::Surface), p.surface);
    }
}
