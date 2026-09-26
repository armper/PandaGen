//! The sign-in screen (FS-006): who is here, and their passphrase.
//!
//! The whole screen under the veil, like the rest screen: the time large in
//! the bottom-left corner as it is there, and a panel with a tile for each
//! person who can sign in,
//! a passphrase field that shows a dot per character and never the
//! characters, a Sign in button, and what went wrong when something did.
//! Unlocking -- waking a rested desk whose person has a passphrase -- is the
//! same screen with only that person on it.
//!
//! The view holds the passphrase only until Enter: it goes to the kernel in
//! the request and the view forgets it. Deciding is the kernel's, through
//! the filesystem's guard; the desk only asks.

extern crate alloc;

use crate::widgets::{rect, text_width, ButtonKind, Palette, Ui};
use alloc::string::String;
use alloc::vec::Vec;
use view_types::{DrawOp, PixelRect};

/// What a click on a person's tile answers: `PERSON_KEY_FIRST + index`.
pub const PERSON_KEY_FIRST: u8 = 0xC0;
/// The longest passphrase the field takes.
pub const MAX_PASSPHRASE: usize = 64;
/// A person's tile, and the circle in it.
pub const TILE: (u32, u32) = (104, 112);
pub const AVATAR: u32 = 64;
/// The panel the tiles and the field sit on.
pub const PANEL: (u32, u32) = (460, 250);
/// What went wrong is said in red, whatever the accent.
pub const ERROR_INK: view_types::Color = view_types::Color::rgb(248, 113, 113);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Nobody is signed in: pick a person.
    SignIn,
    /// The person is still signed in; the rested desk asks for their
    /// passphrase before it shows again.
    Unlock,
}

/// What the view asks the kernel for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    SignIn { name: String, passphrase: String },
    Unlock { passphrase: String },
}

#[derive(Debug, Clone)]
pub struct SignInView {
    pub mode: Mode,
    people: Vec<String>,
    selected: usize,
    passphrase: String,
    /// What went wrong last, until the next key.
    pub message: Option<String>,
    /// A request is with the kernel.
    pub waiting: bool,
}

impl SignInView {
    /// Sign in as one of `people`, starting at `preferred` if it is there.
    pub fn sign_in(people: Vec<String>, preferred: Option<&str>) -> Self {
        let selected = preferred
            .and_then(|p| people.iter().position(|n| n == p))
            .unwrap_or(0);
        Self {
            mode: Mode::SignIn,
            people,
            selected,
            passphrase: String::new(),
            message: None,
            waiting: false,
        }
    }

    /// Unlock `name`'s rested desk.
    pub fn unlock(name: &str) -> Self {
        Self {
            mode: Mode::Unlock,
            ..Self::sign_in(alloc::vec![name.into()], None)
        }
    }

    pub fn people(&self) -> &[String] {
        &self.people
    }

    pub fn selected(&self) -> Option<&str> {
        self.people.get(self.selected).map(String::as_str)
    }

    /// How many characters are in the field (what the dots show).
    pub fn typed(&self) -> usize {
        self.passphrase.chars().count()
    }

    /// The kernel said no: say why, and clear the field for another try.
    pub fn refused(&mut self, why: &str) {
        self.waiting = false;
        self.passphrase.clear();
        self.message = Some(why.into());
    }

    fn submit(&mut self) -> Option<Ask> {
        if self.waiting {
            return None;
        }
        if self.passphrase.is_empty() {
            self.message = Some("Type your passphrase".into());
            return None;
        }
        let passphrase = core::mem::take(&mut self.passphrase);
        self.waiting = true;
        match self.mode {
            Mode::Unlock => Some(Ask::Unlock { passphrase }),
            Mode::SignIn => {
                let name = self.selected()?.into();
                Some(Ask::SignIn { name, passphrase })
            }
        }
    }

    fn choose(&mut self, index: usize) {
        if index < self.people.len() && index != self.selected {
            self.selected = index;
            self.passphrase.clear();
            self.message = None;
        }
    }

    /// A key: characters go in the field, Backspace takes one out, Esc
    /// clears it, Enter asks, Tab and the arrows move between people.
    pub fn key(&mut self, byte: u8) -> Option<Ask> {
        use crate::notepad::{ESC, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP};
        match byte {
            b'\n' | b'\r' => return self.submit(),
            0x08 | 0x7F => {
                self.passphrase.pop();
            }
            ESC => {
                self.passphrase.clear();
                self.message = None;
            }
            b'\t' | KEY_RIGHT | KEY_DOWN if !self.people.is_empty() => {
                self.choose((self.selected + 1) % self.people.len());
            }
            KEY_LEFT | KEY_UP if !self.people.is_empty() => {
                self.choose((self.selected + self.people.len() - 1) % self.people.len());
            }
            k if k >= PERSON_KEY_FIRST && ((k - PERSON_KEY_FIRST) as usize) < self.people.len() => {
                self.choose((k - PERSON_KEY_FIRST) as usize);
            }
            0x20..=0x7E => {
                if self.typed() < MAX_PASSPHRASE {
                    self.passphrase.push(byte as char);
                    self.message = None;
                }
            }
            _ => {}
        }
        None
    }

    /// Where the panel sits on a `w` x `h` screen: centred across, in the
    /// lower part, clear of what a picture wallpaper puts in its middle.
    pub fn panel(w: u32, h: u32) -> PixelRect {
        let (pw, ph) = PANEL;
        rect(
            (w.saturating_sub(pw) / 2) as i32,
            h.saturating_sub(ph + 140) as i32,
            pw,
            ph,
        )
    }

    /// The screen, in screen pixels: the time and date, the panel with
    /// the people, the field and the button, and the hint at the foot.
    pub fn ui(
        &self,
        w: u32,
        h: u32,
        clock: &str,
        date: Option<&str>,
        palette: Palette,
        hover: Option<(i32, i32)>,
    ) -> Ui {
        let p = palette;
        let mut ui = Ui::new(palette, hover);
        // The time and date where the rest screen has them (GFX-111).
        let margin = crate::desk::REST_MARGIN as i32;
        let scale = crate::desk::REST_CLOCK_SCALE;
        let clock_h = 16 * scale as i32;
        let clock_y = h as i32 - margin - 32 - 12 - clock_h;
        ui.text(margin, clock_y, clock, p.text, scale);
        if let Some(date) = date {
            ui.text(margin + 6, clock_y + clock_h + 12, date, p.muted, 2);
        }
        let _ = text_width;
        let panel = Self::panel(w, h);
        ui.fill(panel, p.surface, 22);
        ui.outline(panel, p.hairline, 22, 1);
        let title = match self.mode {
            Mode::SignIn => "Who is here?",
            Mode::Unlock => "Welcome back",
        };
        ui.text_centered(
            &rect(panel.x as i32, panel.y as i32 + 16, panel.width, 20),
            title,
            p.muted,
            1,
        );

        // The people: a tile each, the chosen one ringed in the accent.
        let (tw, th) = TILE;
        let shown = self.people.len().min(4) as u32;
        let row = shown * tw + shown.saturating_sub(1) * 12;
        let x0 = panel.x as i32 + (panel.width as i32 - row as i32) / 2;
        let ty = panel.y as i32 + 44;
        for (i, name) in self.people.iter().take(4).enumerate() {
            let tile = rect(x0 + i as i32 * (tw as i32 + 12), ty, tw, th);
            let chosen = i == self.selected;
            if chosen {
                ui.fill(tile, p.raised, 16);
                ui.outline(tile, p.accent, 16, 2);
            } else if ui.hovered(&tile) {
                ui.outline(tile, p.hairline, 16, 1);
            }
            let avatar = rect(
                tile.x as i32 + (tw as i32 - AVATAR as i32) / 2,
                tile.y as i32 + 10,
                AVATAR,
                AVATAR,
            );
            ui.fill(avatar, if chosen { p.accent } else { p.raised }, AVATAR / 2);
            let initial: String = name
                .chars()
                .take(1)
                .flat_map(|c| c.to_uppercase())
                .collect();
            ui.text_centered(
                &avatar,
                &initial,
                if chosen { p.on_accent } else { p.text },
                3,
            );
            ui.text_centered(
                &rect(tile.x as i32, avatar.y as i32 + AVATAR as i32 + 6, tw, 16),
                name,
                if chosen { p.text } else { p.muted },
                1,
            );
            if self.mode == Mode::SignIn {
                ui.hit_area(tile, PERSON_KEY_FIRST + i as u8);
            }
        }

        // The field: a dot a character, the caret after them.
        let fy = ty + th as i32 + 18;
        let field = rect(panel.x as i32 + 40, fy, panel.width - 80 - 112, 40);
        ui.fill(field, p.raised, 12);
        ui.outline(
            field,
            if self.message.is_some() {
                ERROR_INK
            } else {
                p.hairline
            },
            12,
            1,
        );
        let cy = fy + 20;
        if self.passphrase.is_empty() {
            ui.text(field.x as i32 + 14, cy - 8, "Passphrase", p.muted, 1);
        } else {
            let dots = self.typed().min(((field.width - 28) / 14) as usize);
            for i in 0..dots {
                ui.fill(
                    rect(field.x as i32 + 14 + i as i32 * 14, cy - 4, 8, 8),
                    p.text,
                    4,
                );
            }
            let cx = field.x as i32 + 14 + dots as i32 * 14 + 2;
            ui.fill(rect(cx, cy - 9, 2, 18), p.accent, 0);
        }
        let button = rect(field.x as i32 + field.width as i32 + 12, fy, 100, 40);
        let label = if self.waiting { "..." } else { "Sign in" };
        ui.button(button, label, b'\n', ButtonKind::Primary);
        if let Some(message) = &self.message {
            ui.text_centered(
                &rect(panel.x as i32, fy + 52, panel.width, 16),
                message,
                ERROR_INK,
                1,
            );
        }
        let hint = match self.mode {
            Mode::SignIn if self.people.len() > 1 => {
                "Enter signs in   Tab picks someone else   Esc clears"
            }
            Mode::SignIn => "Enter signs in   Esc clears",
            Mode::Unlock => "Enter unlocks   Esc clears",
        };
        ui.text_centered(
            &rect(
                panel.x as i32,
                panel.y as i32 + panel.height as i32 + 16,
                panel.width,
                16,
            ),
            hint,
            p.muted,
            1,
        );
        ui
    }

    /// The key a click at `(x, y)` stands for, if any.
    pub fn hit(&self, w: u32, h: u32, palette: Palette, x: i32, y: i32) -> Option<u8> {
        self.ui(w, h, "", None, palette, None).hit(x, y)
    }
}

/// The ops, for a test to look through.
pub fn texts(ops: &[DrawOp]) -> Vec<String> {
    ops.iter()
        .filter_map(|op| match op {
            DrawOp::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notepad::{ESC, KEY_RIGHT};

    fn palette() -> Palette {
        Palette::from_theme(&services_gui_host::Theme::DEFAULT)
    }

    #[test]
    fn typing_shows_dots_and_enter_asks_once() {
        let mut v = SignInView::sign_in(
            alloc::vec!["alice".into(), "armando".into()],
            Some("armando"),
        );
        assert_eq!(v.selected(), Some("armando"));
        assert_eq!(v.key(b'\n'), None, "nothing typed");
        assert!(v.message.is_some());
        for b in b"pa55" {
            v.key(*b);
        }
        v.key(0x08);
        v.key(b'5');
        assert_eq!(v.typed(), 4);
        // The screen shows four dots and never the characters.
        let ops = v.ui(1280, 800, "12:00", None, palette(), None).into_ops();
        assert!(!texts(&ops).iter().any(|t| t.contains("pa55")));
        let dots = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::RoundedFill { rect, .. } if rect.width == 8 && rect.height == 8))
            .count();
        assert_eq!(dots, 4);
        assert_eq!(
            v.key(b'\n'),
            Some(Ask::SignIn {
                name: "armando".into(),
                passphrase: "pa55".into()
            })
        );
        assert_eq!(v.typed(), 0, "the view forgets it");
        assert_eq!(v.key(b'\n'), None, "one request at a time");
        v.refused("wrong name or passphrase");
        assert!(!v.waiting);
        assert!(
            texts(&v.ui(1280, 800, "", None, palette(), None).into_ops())
                .contains(&"wrong name or passphrase".into())
        );
    }

    #[test]
    fn people_are_picked_by_key_or_click_and_unlock_has_only_one() {
        let mut v = SignInView::sign_in(alloc::vec!["alice".into(), "armando".into()], None);
        v.key(b'x');
        v.key(KEY_RIGHT);
        assert_eq!(v.selected(), Some("armando"));
        assert_eq!(v.typed(), 0, "switching person clears the field");
        // A click on the first tile picks it.
        let panel = SignInView::panel(1280, 800);
        let (tw, _) = TILE;
        let row = 2 * tw + 12;
        let x = panel.x as i32 + (panel.width as i32 - row as i32) / 2 + 10;
        let key = v.hit(1280, 800, palette(), x, panel.y as i32 + 60).unwrap();
        v.key(key);
        assert_eq!(v.selected(), Some("alice"));
        v.key(b'q');
        v.key(ESC);
        assert_eq!(v.typed(), 0);
        let mut u = SignInView::unlock("alice");
        for b in b"pass" {
            u.key(*b);
        }
        assert_eq!(
            u.key(b'\n'),
            Some(Ask::Unlock {
                passphrase: "pass".into()
            })
        );
        assert!(
            u.hit(1280, 800, palette(), x, panel.y as i32 + 60)
                .is_none(),
            "no tiles to pick"
        );
    }
}
