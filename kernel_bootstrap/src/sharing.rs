//! The Sharing card (FS-007): who has a document, and giving it to more.
//!
//! One document, drawn: its name and owner; its sensitivity as four
//! segments (the owner can change it, within their clearance); everyone
//! who holds a grant on it, with what they may do and a Revoke; and, for
//! whoever may share it, a row of people and a row of rights to pick from
//! and a Share button. What is shown comes from the kernel
//! ([`SharingInfo`], built by the filesystem's guard); what is done goes
//! back as a request, and the card is shown again as the guard then sees
//! it -- so the card never claims more than the guard allowed.

extern crate alloc;

use crate::widgets::{rect, ButtonKind, Palette, Ui};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use authority::Rights;
use view_types::DrawOp;

/// A click on sensitivity segment `i` answers `LABEL_KEY + i`.
pub const LABEL_KEY: u8 = 0xB4;
/// A click on grant row `i`'s Revoke answers `REVOKE_KEY + i`.
pub const REVOKE_KEY: u8 = 0xC0;
/// A click on person `i` answers `PERSON_KEY + i`.
pub const PERSON_KEY: u8 = 0xD0;
/// A click on right `i` (in [`CHOOSABLE`]) answers `RIGHT_KEY + i`.
pub const RIGHT_KEY: u8 = 0xE0;
/// The rights the card offers, in order. (Owning is not given away here.)
pub const CHOOSABLE: [(Rights, &str); 6] = [
    (Rights::READ, "Read"),
    (Rights::HISTORY, "History"),
    (Rights::WRITE, "Write"),
    (Rights::TAG, "Tag"),
    (Rights::DELETE, "Delete"),
    (Rights::SHARE, "Share"),
];
/// Most grant rows shown; the rest are counted.
pub const ROWS: usize = 4;
/// Most people offered.
pub const PEOPLE: usize = 8;
const LABELS: [&str; 4] = ["Public", "Internal", "Confidential", "Secret"];

/// One grant on the document, as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantLine {
    pub id: u32,
    pub holder: String,
    pub rights: u8,
    pub until: Option<u64>,
    pub since: Option<u64>,
    /// Neither revoked nor expired.
    pub live: bool,
    /// The one looking may revoke it: they own the document or gave it.
    pub revocable: bool,
}

/// What the guard says about a document, for the card.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SharingInfo {
    pub name: String,
    pub owner: String,
    /// 0 public .. 3 secret.
    pub label: u8,
    /// The one looking owns it (and so may relabel it).
    pub can_relabel: bool,
    /// The one looking may share it.
    pub can_share: bool,
    /// What the one looking holds, as rights bits.
    pub held: u8,
    pub grants: Vec<GrantLine>,
    /// Whom it can be shared with: everyone but the one looking and the
    /// owner.
    pub people: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharingEffect {
    None,
    Redraw,
    Close,
    Share { to: String, rights: u8 },
    Revoke(u32),
    Relabel(u8),
}

#[derive(Debug, Clone)]
pub struct SharingView {
    pub name: String,
    info: Option<SharingInfo>,
    /// Why the guard would not say (not there, not signed in).
    error: Option<String>,
    person: usize,
    rights: u8,
    /// What the last act did, or why not.
    pub message: Option<String>,
}

impl SharingView {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            info: None,
            error: None,
            person: 0,
            rights: Rights::READ.0,
            message: None,
        }
    }

    pub fn info(&self) -> Option<&SharingInfo> {
        self.info.as_ref()
    }

    /// The rights picked to give.
    pub fn picked(&self) -> Rights {
        Rights(self.rights)
    }

    /// The kernel's answer: the document as the guard sees it now, and
    /// what the last act did.
    pub fn loaded(&mut self, info: Result<SharingInfo, String>, message: Option<String>) {
        match info {
            Ok(info) => {
                self.person = self.person.min(info.people.len().saturating_sub(1));
                self.info = Some(info);
                self.error = None;
            }
            Err(e) => {
                self.info = None;
                self.error = Some(e);
            }
        }
        if message.is_some() {
            self.message = message;
        }
    }

    pub fn footer(&self) -> String {
        match &self.info {
            Some(i) if i.can_share => {
                "Pick someone and what they may do, then Share   Esc closes".into()
            }
            Some(_) => "Only its owner and those they let share it can share it".into(),
            None => "Asking the filesystem...".into(),
        }
    }

    pub fn handle_byte(&mut self, byte: u8) -> SharingEffect {
        use crate::notepad::{CTRL_W, ESC};
        let Some(info) = self.info.clone() else {
            return match byte {
                CTRL_W | ESC => SharingEffect::Close,
                _ => SharingEffect::None,
            };
        };
        match byte {
            CTRL_W | ESC => SharingEffect::Close,
            b'\t' if !info.people.is_empty() => {
                self.person = (self.person + 1) % info.people.len().min(PEOPLE);
                SharingEffect::Redraw
            }
            b'\n' => {
                if !info.can_share {
                    self.message = Some("You cannot share this".into());
                    return SharingEffect::Redraw;
                }
                let Some(to) = info.people.get(self.person) else {
                    self.message = Some("No one else to share it with".into());
                    return SharingEffect::Redraw;
                };
                if self.rights == 0 {
                    self.message = Some("Pick what they may do".into());
                    return SharingEffect::Redraw;
                }
                SharingEffect::Share {
                    to: to.clone(),
                    rights: self.rights,
                }
            }
            k if (LABEL_KEY..LABEL_KEY + 4).contains(&k) => {
                if info.can_relabel && k - LABEL_KEY != info.label {
                    SharingEffect::Relabel(k - LABEL_KEY)
                } else {
                    SharingEffect::None
                }
            }
            k if (REVOKE_KEY..REVOKE_KEY + ROWS as u8).contains(&k) => {
                match info.grants.get((k - REVOKE_KEY) as usize) {
                    Some(g) if g.revocable && g.live => SharingEffect::Revoke(g.id),
                    _ => SharingEffect::None,
                }
            }
            k if (PERSON_KEY..PERSON_KEY + PEOPLE as u8).contains(&k) => {
                let i = (k - PERSON_KEY) as usize;
                if i < info.people.len() {
                    self.person = i;
                    SharingEffect::Redraw
                } else {
                    SharingEffect::None
                }
            }
            k if (RIGHT_KEY..RIGHT_KEY + CHOOSABLE.len() as u8).contains(&k) => {
                let bit = CHOOSABLE[(k - RIGHT_KEY) as usize].0 .0;
                // Only what the one sharing holds can be given.
                if Rights(info.held).contains(Rights(bit)) {
                    self.rights ^= bit;
                    SharingEffect::Redraw
                } else {
                    self.message = Some("You do not hold that to give".into());
                    SharingEffect::Redraw
                }
            }
            _ => SharingEffect::None,
        }
    }

    /// The card, in its canvas's pixels.
    pub fn ui(&self, w: u32, h: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let p = palette;
        let mut ui = Ui::new(palette, hover);
        let w_i = w as i32;
        if let Some(id) = crate::desk::DeskApp::Notepad.picture(32) {
            ui.push(DrawOp::Picture { x: 0, y: 2, id });
        }
        ui.text(44, 0, &self.name, p.text, 2);
        let Some(info) = &self.info else {
            let what = self.error.as_deref().unwrap_or("Asking the filesystem...");
            ui.text(44, 36, what, p.muted, 1);
            return ui;
        };
        let whose = if Rights(info.held).contains(Rights::OWN) && info.can_relabel {
            String::from("yours")
        } else {
            alloc::format!("owned by {}   you hold {}", info.owner, Rights(info.held))
        };
        ui.text(44, 36, &whose, p.muted, 1);

        // Sensitivity: four segments, the current one lit.
        ui.text(0, 66, "Sensitivity", p.muted, 1);
        let seg_w = (w - 18) / 4;
        for (i, name) in LABELS.iter().enumerate() {
            let seg = rect(i as i32 * (seg_w as i32 + 6), 86, seg_w, 30);
            let on = i as u8 == info.label;
            ui.fill(seg, if on { p.accent } else { p.raised }, 10);
            if !on && info.can_relabel && ui.hovered(&seg) {
                ui.outline(seg, p.accent, 10, 1);
            }
            ui.text_centered(&seg, name, if on { p.on_accent } else { p.text }, 1);
            if info.can_relabel {
                ui.hit_area(seg, LABEL_KEY + i as u8);
            }
        }

        // Who has it.
        ui.text(0, 134, "Who has it", p.muted, 1);
        let mut y = 154;
        if info.grants.is_empty() {
            ui.text(0, y + 4, "No one else: it is yours alone", p.muted, 1);
            y += 36;
        }
        for (i, g) in info.grants.iter().take(ROWS).enumerate() {
            let avatar = rect(0, y, 28, 28);
            ui.fill(avatar, if g.live { p.accent } else { p.raised }, 14);
            let initial: String = g
                .holder
                .chars()
                .take(1)
                .flat_map(|c| c.to_uppercase())
                .collect();
            ui.text_centered(
                &avatar,
                &initial,
                if g.live { p.on_accent } else { p.muted },
                1,
            );
            ui.text(
                38,
                y + 6,
                &g.holder,
                if g.live { p.text } else { p.muted },
                1,
            );
            // The rights, as small pills.
            let mut x = 38 + 12 * 8;
            for (bit, name) in CHOOSABLE {
                if g.rights & bit.0 != 0 {
                    let pw = name.len() as u32 * 8 + 12;
                    let pill = rect(x, y + 4, pw, 20);
                    ui.fill(pill, p.raised, 10);
                    ui.text_centered(&pill, &name.to_ascii_lowercase(), p.text, 1);
                    x += pw as i32 + 4;
                }
            }
            let state = if !g.live {
                Some("revoked or expired".to_string())
            } else {
                match (g.until, g.since) {
                    (Some(t), _) => Some(alloc::format!("until {t}")),
                    (None, Some(t)) => Some(alloc::format!("history since {t}")),
                    _ => None,
                }
            };
            if g.revocable && g.live {
                ui.button(
                    rect(w_i - 84, y + 1, 84, 26),
                    "Revoke",
                    REVOKE_KEY + i as u8,
                    ButtonKind::Quiet,
                );
            } else if let Some(state) = &state {
                ui.text_right(w_i, y + 6, state, p.muted, 1);
            }
            y += 36;
        }
        if info.grants.len() > ROWS {
            ui.text(
                38,
                y,
                &alloc::format!("and {} more", info.grants.len() - ROWS),
                p.muted,
                1,
            );
        }

        // Share with.
        let y0 = h as i32 - 128;
        ui.line(0, y0 - 10, w_i, y0 - 10, p.hairline, 1);
        if !info.can_share {
            ui.text(
                0,
                y0 + 4,
                &alloc::format!(
                    "You hold {} on it: sharing is for its owner",
                    Rights(info.held)
                ),
                p.muted,
                1,
            );
        } else if info.people.is_empty() {
            ui.text(
                0,
                y0 + 4,
                "No one else to share with -- add people in Access",
                p.muted,
                1,
            );
        } else {
            ui.text(0, y0, "Share with", p.muted, 1);
            let mut x = 0;
            for (i, name) in info.people.iter().take(PEOPLE).enumerate() {
                let pw = name.len() as u32 * 8 + 20;
                let pill = rect(x, y0 + 20, pw, 28);
                let on = i == self.person;
                ui.fill(pill, if on { p.accent } else { p.raised }, 14);
                ui.text_centered(&pill, name, if on { p.on_accent } else { p.text }, 1);
                ui.hit_area(pill, PERSON_KEY + i as u8);
                x += pw as i32 + 6;
            }
            let mut x = 0;
            for (i, (bit, name)) in CHOOSABLE.iter().enumerate() {
                let pw = name.len() as u32 * 8 + 20;
                let pill = rect(x, y0 + 58, pw, 28);
                let on = self.rights & bit.0 != 0;
                let can = Rights(info.held).contains(*bit);
                ui.fill(pill, if on { p.accent } else { p.raised }, 14);
                ui.text_centered(
                    &pill,
                    name,
                    if on {
                        p.on_accent
                    } else if can {
                        p.text
                    } else {
                        p.muted
                    },
                    1,
                );
                ui.hit_area(pill, RIGHT_KEY + i as u8);
                x += pw as i32 + 6;
            }
            ui.button(
                rect(w_i - 96, y0 + 58, 96, 28),
                "Share",
                b'\n',
                ButtonKind::Primary,
            );
        }
        if let Some(m) = &self.message {
            ui.text(0, h as i32 - 20, m, p.muted, 1);
        }
        ui
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> SharingInfo {
        SharingInfo {
            name: "plan".into(),
            owner: "alice".into(),
            label: 1,
            can_relabel: true,
            can_share: true,
            held: Rights::ALL.0,
            grants: alloc::vec![GrantLine {
                id: 3,
                holder: "armando".into(),
                rights: Rights::READ.0,
                until: None,
                since: None,
                live: true,
                revocable: true,
            }],
            people: alloc::vec!["armando".into(), "bea".into()],
        }
    }

    fn palette() -> Palette {
        Palette::from_theme(&services_gui_host::Theme::DEFAULT)
    }

    #[test]
    fn the_card_shares_revokes_and_relabels_by_click_or_key() {
        let mut v = SharingView::new("plan");
        assert_eq!(
            v.handle_byte(b'\n'),
            SharingEffect::None,
            "nothing loaded yet"
        );
        v.loaded(Ok(info()), None);
        // Pick Bea, add History, share.
        assert_eq!(v.handle_byte(PERSON_KEY + 1), SharingEffect::Redraw);
        v.handle_byte(RIGHT_KEY + 1);
        assert_eq!(
            v.handle_byte(b'\n'),
            SharingEffect::Share {
                to: "bea".into(),
                rights: Rights::READ.union(Rights::HISTORY).0
            }
        );
        assert_eq!(v.handle_byte(REVOKE_KEY), SharingEffect::Revoke(3));
        assert_eq!(v.handle_byte(LABEL_KEY + 2), SharingEffect::Relabel(2));
        assert_eq!(
            v.handle_byte(LABEL_KEY + 1),
            SharingEffect::None,
            "already internal"
        );
        // The drawn card answers the same keys where it draws them.
        let (w, h) = (544, 456);
        let ui = v.ui(w, h, palette(), None);
        assert_eq!(ui.hit(w as i32 - 40, h as i32 - 128 + 72), Some(b'\n'));
        assert_eq!(ui.hit(w as i32 - 40, 154 + 12), Some(REVOKE_KEY));
        assert_eq!(ui.hit(10, 100), Some(LABEL_KEY));
    }

    #[test]
    fn what_cannot_be_given_is_not_offered() {
        let mut v = SharingView::new("plan");
        let mut i = info();
        i.can_relabel = false;
        i.held = Rights::READ.union(Rights::SHARE).0;
        i.grants[0].revocable = false;
        v.loaded(Ok(i.clone()), None);
        v.handle_byte(RIGHT_KEY + 2);
        assert_eq!(v.picked(), Rights::READ, "write is not held");
        assert!(v.message.as_deref().unwrap().contains("do not hold"));
        assert_eq!(v.handle_byte(REVOKE_KEY), SharingEffect::None);
        assert_eq!(v.handle_byte(LABEL_KEY + 3), SharingEffect::None);
        v.handle_byte(RIGHT_KEY);
        assert_eq!(
            v.handle_byte(b'\n'),
            SharingEffect::Redraw,
            "nothing picked"
        );
        // Holding only read: no Share at all.
        i.can_share = false;
        i.held = Rights::READ.0;
        v.loaded(Ok(i), Some("Shared".into()));
        assert_eq!(v.message.as_deref(), Some("Shared"));
        v.handle_byte(RIGHT_KEY);
        assert_eq!(v.handle_byte(b'\n'), SharingEffect::Redraw);
        assert!(v.message.as_deref().unwrap().contains("cannot share"));
        v.loaded(Err("Not signed in".into()), None);
        assert!(v.info().is_none());
        assert_eq!(v.handle_byte(crate::notepad::ESC), SharingEffect::Close);
    }
}
