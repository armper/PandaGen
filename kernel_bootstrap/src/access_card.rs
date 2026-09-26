//! The Access card (FS-008): the people on this machine, and roles.
//!
//! Everyone, each with a tile: their initial, their name, their clearance
//! as four small segments -- which the administrator can change -- and
//! whether they can sign in yet. The administrator adds someone with a
//! name, a passphrase (masked) and a clearance. Below, the roles the one
//! looking owns or is in: what each gives, over what, and -- on their own
//! roles -- a toggle for every person, in or out.
//!
//! As with Sharing, the card shows what the kernel's guard says
//! ([`AccessInfo`]) and asks for every change; it is shown again as the
//! guard then sees it.

extern crate alloc;

use crate::widgets::{rect, ButtonKind, Palette, Ui};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use authority::Rights;

/// Person `p`'s clearance segment `l`: `CLEARANCE_KEY + p * 4 + l`.
pub const CLEARANCE_KEY: u8 = 0xC0;
/// Role `r`, person `p` in or out: `MEMBER_KEY + r * 8 + p`.
pub const MEMBER_KEY: u8 = 0xE0;
/// The name field, the passphrase field.
pub const NAME_FIELD_KEY: u8 = 0xB8;
pub const PASS_FIELD_KEY: u8 = 0xB9;
/// Most people rows, and roles.
pub const PEOPLE: usize = 6;
pub const ROLES: usize = 3;
const LEVELS: [&str; 4] = ["P", "I", "C", "S"];
const LEVEL_NAMES: [&str; 4] = ["public", "internal", "confidential", "secret"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonLine {
    pub name: String,
    /// 0 public .. 3 secret.
    pub clearance: u8,
    pub passphrase: bool,
    pub admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleLine {
    pub name: String,
    pub rights: u8,
    /// `#work`, `doc plan`, `all`.
    pub scope: String,
    pub members: Vec<String>,
    /// The one looking owns it: they may change who is in it.
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccessInfo {
    pub me: String,
    pub admin: bool,
    pub people: Vec<PersonLine>,
    pub roles: Vec<RoleLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessAct {
    AddPerson {
        name: String,
        passphrase: String,
        clearance: u8,
    },
    SetClearance {
        name: String,
        clearance: u8,
    },
    Join {
        role: String,
        person: String,
    },
    Leave {
        role: String,
        person: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessEffect {
    None,
    Redraw,
    Close,
    Act(AccessAct),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Passphrase,
}

#[derive(Debug, Clone)]
pub struct AccessView {
    info: Option<AccessInfo>,
    error: Option<String>,
    name: String,
    passphrase: String,
    clearance: u8,
    field: Field,
    pub message: Option<String>,
}

impl Default for AccessView {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessView {
    pub fn new() -> Self {
        Self {
            info: None,
            error: None,
            name: String::new(),
            passphrase: String::new(),
            clearance: 1,
            field: Field::Name,
            message: None,
        }
    }

    pub fn info(&self) -> Option<&AccessInfo> {
        self.info.as_ref()
    }

    pub fn loaded(&mut self, info: Result<AccessInfo, String>, message: Option<String>) {
        match info {
            Ok(i) => {
                self.info = Some(i);
                self.error = None;
            }
            Err(e) => {
                self.info = None;
                self.error = Some(e);
            }
        }
        if let Some(m) = message {
            // A person added: the fields are done with.
            if m.starts_with("Added") {
                self.name.clear();
                self.passphrase.clear();
                self.field = Field::Name;
            }
            self.message = Some(m);
        }
    }

    pub fn footer(&self) -> String {
        match &self.info {
            Some(i) if i.admin => {
                "Type a name, Tab, a passphrase, then Enter adds them   Esc closes".into()
            }
            Some(_) => "Only the administrator adds people and sets clearances".into(),
            None => "Asking the filesystem...".into(),
        }
    }

    pub fn handle_byte(&mut self, byte: u8) -> AccessEffect {
        use crate::notepad::{CTRL_W, ESC};
        let Some(info) = self.info.clone() else {
            return match byte {
                CTRL_W | ESC => AccessEffect::Close,
                _ => AccessEffect::None,
            };
        };
        match byte {
            CTRL_W | ESC => AccessEffect::Close,
            NAME_FIELD_KEY => {
                self.field = Field::Name;
                AccessEffect::Redraw
            }
            PASS_FIELD_KEY => {
                self.field = Field::Passphrase;
                AccessEffect::Redraw
            }
            b'\t' => {
                self.field = match self.field {
                    Field::Name => Field::Passphrase,
                    Field::Passphrase => Field::Name,
                };
                AccessEffect::Redraw
            }
            b'\n' => {
                if !info.admin {
                    self.message = Some("Only the administrator adds people".into());
                    return AccessEffect::Redraw;
                }
                if !authority::valid_name(&self.name) {
                    self.message = Some("A name is letters, digits, - or _".into());
                    return AccessEffect::Redraw;
                }
                if self.passphrase.chars().count() < authority::credential::MIN_PASSPHRASE {
                    self.message = Some(alloc::format!(
                        "A passphrase is at least {} characters",
                        authority::credential::MIN_PASSPHRASE
                    ));
                    self.field = Field::Passphrase;
                    return AccessEffect::Redraw;
                }
                AccessEffect::Act(AccessAct::AddPerson {
                    name: self.name.clone(),
                    passphrase: core::mem::take(&mut self.passphrase),
                    clearance: self.clearance,
                })
            }
            0x08 | 0x7F => {
                match self.field {
                    Field::Name => self.name.pop(),
                    Field::Passphrase => self.passphrase.pop(),
                };
                AccessEffect::Redraw
            }
            k if (CLEARANCE_KEY..CLEARANCE_KEY + (PEOPLE as u8 + 1) * 4).contains(&k) => {
                let (row, level) = (((k - CLEARANCE_KEY) / 4) as usize, (k - CLEARANCE_KEY) % 4);
                if !info.admin {
                    return AccessEffect::None;
                }
                // The last row is the new person's clearance.
                if row == PEOPLE {
                    self.clearance = level;
                    return AccessEffect::Redraw;
                }
                match info.people.get(row) {
                    Some(p) if p.clearance != level => AccessEffect::Act(AccessAct::SetClearance {
                        name: p.name.clone(),
                        clearance: level,
                    }),
                    _ => AccessEffect::None,
                }
            }
            k if k >= MEMBER_KEY && ((k - MEMBER_KEY) as usize) < ROLES * 8 => {
                let (r, p) = (
                    ((k - MEMBER_KEY) / 8) as usize,
                    ((k - MEMBER_KEY) % 8) as usize,
                );
                let (Some(role), Some(person)) = (info.roles.get(r), info.people.get(p)) else {
                    return AccessEffect::None;
                };
                if !role.mine {
                    return AccessEffect::None;
                }
                let (role, person) = (role.name.clone(), person.name.clone());
                let is_in = info.roles[r].members.contains(&person);
                AccessEffect::Act(if is_in {
                    AccessAct::Leave { role, person }
                } else {
                    AccessAct::Join { role, person }
                })
            }
            0x20..=0x7E if info.admin => {
                match self.field {
                    Field::Name if self.name.len() < 24 => self.name.push(byte as char),
                    Field::Passphrase if self.passphrase.len() < 64 => {
                        self.passphrase.push(byte as char)
                    }
                    _ => {}
                }
                self.message = None;
                AccessEffect::Redraw
            }
            _ => AccessEffect::None,
        }
    }

    fn segments(ui: &mut Ui, p: &Palette, x: i32, y: i32, on: u8, key: Option<u8>) {
        for (l, name) in LEVELS.iter().enumerate() {
            let seg = rect(x + l as i32 * 26, y, 24, 22);
            let lit = l as u8 == on;
            ui.fill(seg, if lit { p.accent } else { p.raised }, 6);
            ui.text_centered(&seg, name, if lit { p.on_accent } else { p.muted }, 1);
            if let Some(key) = key {
                ui.hit_area(seg, key + l as u8);
            }
        }
    }

    pub fn ui(&self, w: u32, h: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let p = palette;
        let mut ui = Ui::new(palette, hover);
        let w_i = w as i32;
        let Some(info) = &self.info else {
            ui.text(
                0,
                0,
                self.error.as_deref().unwrap_or("Asking the filesystem..."),
                p.muted,
                1,
            );
            return ui;
        };
        ui.text(0, 0, "People", p.text, 2);
        ui.text_right(
            w_i,
            8,
            &alloc::format!(
                "you are {}{}",
                info.me,
                if info.admin {
                    ", the administrator"
                } else {
                    ""
                }
            ),
            p.muted,
            1,
        );
        let mut y = 40;
        for (i, person) in info.people.iter().take(PEOPLE).enumerate() {
            let avatar = rect(0, y, 30, 30);
            ui.fill(
                avatar,
                if person.name == info.me {
                    p.accent
                } else {
                    p.raised
                },
                15,
            );
            let initial: String = person
                .name
                .chars()
                .take(1)
                .flat_map(|c| c.to_uppercase())
                .collect();
            ui.text_centered(
                &avatar,
                &initial,
                if person.name == info.me {
                    p.on_accent
                } else {
                    p.text
                },
                1,
            );
            ui.text(40, y + 7, &person.name, p.text, 1);
            let status = if !person.passphrase {
                "cannot sign in yet"
            } else if person.admin {
                "administrator"
            } else {
                "can sign in"
            };
            ui.text(40 + 18 * 8, y + 7, status, p.muted, 1);
            Self::segments(
                &mut ui,
                &p,
                w_i - 4 * 26,
                y + 4,
                person.clearance,
                info.admin.then_some(CLEARANCE_KEY + i as u8 * 4),
            );
            y += 38;
        }
        ui.text_right(
            w_i,
            y,
            "clearance: public, internal, confidential, secret",
            p.muted,
            1,
        );
        y += 26;

        if info.admin {
            ui.line(0, y, w_i, y, p.hairline, 1);
            y += 12;
            ui.text(0, y, "Add someone", p.muted, 1);
            y += 20;
            let name_field = rect(0, y, 170, 32);
            let pass_field = rect(180, y, 170, 32);
            for (field, which, key) in [
                (name_field, Field::Name, NAME_FIELD_KEY),
                (pass_field, Field::Passphrase, PASS_FIELD_KEY),
            ] {
                ui.fill(field, p.raised, 10);
                ui.outline(
                    field,
                    if self.field == which {
                        p.accent
                    } else {
                        p.hairline
                    },
                    10,
                    1,
                );
                ui.hit_area(field, key);
            }
            if self.name.is_empty() {
                ui.text(12, y + 8, "name", p.muted, 1);
            } else {
                ui.text(12, y + 8, &self.name, p.text, 1);
            }
            if self.passphrase.is_empty() {
                ui.text(192, y + 8, "passphrase", p.muted, 1);
            } else {
                for i in 0..self.passphrase.chars().count().min(10) {
                    ui.fill(rect(192 + i as i32 * 14, y + 12, 8, 8), p.text, 4);
                }
            }
            Self::segments(
                &mut ui,
                &p,
                362,
                y + 5,
                self.clearance,
                Some(CLEARANCE_KEY + PEOPLE as u8 * 4),
            );
            ui.button(rect(w_i - 80, y, 80, 32), "Add", b'\n', ButtonKind::Primary);
            y += 44;
        }

        ui.line(0, y, w_i, y, p.hairline, 1);
        y += 12;
        ui.text(0, y, "Roles", p.text, 2);
        y += 36;
        if info.roles.is_empty() {
            ui.text(
                0,
                y,
                "None yet -- in the Terminal: role add <name> <rights> #tag",
                p.muted,
                1,
            );
        }
        for (r, role) in info.roles.iter().take(ROLES).enumerate() {
            ui.text(0, y, &role.name, p.text, 1);
            ui.text(
                18 * 8,
                y,
                &alloc::format!("{} over {}", Rights(role.rights), role.scope),
                p.muted,
                1,
            );
            let mut x = 0;
            let row_y = y + 20;
            for (i, person) in info.people.iter().take(PEOPLE).enumerate() {
                let is_in = role.members.contains(&person.name);
                if !role.mine && !is_in {
                    continue;
                }
                let pw = person.name.len() as u32 * 8 + 20;
                let pill = rect(x, row_y, pw, 24);
                ui.fill(pill, if is_in { p.accent } else { p.raised }, 12);
                ui.text_centered(
                    &pill,
                    &person.name,
                    if is_in { p.on_accent } else { p.muted },
                    1,
                );
                if role.mine {
                    ui.hit_area(pill, MEMBER_KEY + r as u8 * 8 + i as u8);
                }
                x += pw as i32 + 6;
            }
            y += 56;
        }
        if let Some(m) = &self.message {
            ui.text(0, h as i32 - 18, m, p.muted, 1);
        }
        let _ = LEVEL_NAMES;
        ui
    }
}

/// A clearance's name, for messages.
pub fn level_name(level: u8) -> &'static str {
    LEVEL_NAMES
        .get(level as usize)
        .copied()
        .unwrap_or("internal")
}

/// Words for a role's scope.
pub fn scope_words(scope: &authority::Scope, name_of_doc: impl Fn(u64) -> String) -> String {
    match scope {
        authority::Scope::Tag(t) => alloc::format!("#{t}"),
        authority::Scope::Doc(d) => name_of_doc(d.0),
        authority::Scope::Everything => "everything".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(admin: bool) -> AccessInfo {
        AccessInfo {
            me: "owner".into(),
            admin,
            people: alloc::vec![
                PersonLine {
                    name: "owner".into(),
                    clearance: 3,
                    passphrase: true,
                    admin: true
                },
                PersonLine {
                    name: "armando".into(),
                    clearance: 1,
                    passphrase: false,
                    admin: false
                },
            ],
            roles: alloc::vec![RoleLine {
                name: "workers".into(),
                rights: Rights::READ.0,
                scope: "#work".into(),
                members: alloc::vec!["armando".into()],
                mine: true,
            }],
        }
    }

    fn palette() -> Palette {
        Palette::from_theme(&services_gui_host::Theme::DEFAULT)
    }

    #[test]
    fn the_administrator_adds_someone_with_a_passphrase_and_sets_clearances() {
        let mut v = AccessView::new();
        v.loaded(Ok(info(true)), None);
        for b in b"bea" {
            v.handle_byte(*b);
        }
        assert_eq!(
            v.handle_byte(b'\n'),
            AccessEffect::Redraw,
            "no passphrase yet"
        );
        // The view has moved to the passphrase field itself.
        for b in b"pw" {
            v.handle_byte(*b);
        }
        assert_eq!(v.handle_byte(b'\n'), AccessEffect::Redraw, "too short");
        for b in b"12" {
            v.handle_byte(*b);
        }
        v.handle_byte(CLEARANCE_KEY + PEOPLE as u8 * 4 + 2);
        assert_eq!(
            v.handle_byte(b'\n'),
            AccessEffect::Act(AccessAct::AddPerson {
                name: "bea".into(),
                passphrase: "pw12".into(),
                clearance: 2
            })
        );
        // The passphrase is drawn as dots only.
        let ops = v.ui(560, 520, palette(), None).into_ops();
        assert!(!crate::sign_in::texts(&ops)
            .iter()
            .any(|t| t.contains("pw12")));
        v.loaded(Ok(info(true)), Some("Added bea".into()));
        assert_eq!(v.message.as_deref(), Some("Added bea"));
        // Armando's clearance to confidential; his own again is nothing.
        assert_eq!(
            v.handle_byte(CLEARANCE_KEY + 4 + 2),
            AccessEffect::Act(AccessAct::SetClearance {
                name: "armando".into(),
                clearance: 2
            })
        );
        assert_eq!(v.handle_byte(CLEARANCE_KEY + 4 + 1), AccessEffect::None);
        // Out of the role, and (the owner) into it.
        assert_eq!(
            v.handle_byte(MEMBER_KEY + 1),
            AccessEffect::Act(AccessAct::Leave {
                role: "workers".into(),
                person: "armando".into()
            })
        );
        assert_eq!(
            v.handle_byte(MEMBER_KEY),
            AccessEffect::Act(AccessAct::Join {
                role: "workers".into(),
                person: "owner".into()
            })
        );
        // Clicks land on what is drawn: Armando's "C".
        let ui = v.ui(560, 456, palette(), None);
        assert_eq!(
            ui.hit(560 - 4 * 26 + 2 * 26 + 10, 40 + 38 + 12),
            Some(CLEARANCE_KEY + 4 + 2)
        );
    }

    #[test]
    fn someone_else_sees_and_cannot_change() {
        let mut v = AccessView::new();
        let mut i = info(false);
        i.me = "armando".into();
        i.roles[0].mine = false;
        v.loaded(Ok(i), None);
        assert_eq!(v.handle_byte(b'b'), AccessEffect::None, "no typing");
        assert_eq!(v.handle_byte(CLEARANCE_KEY + 4 + 2), AccessEffect::None);
        assert_eq!(v.handle_byte(MEMBER_KEY + 1), AccessEffect::None);
        assert_eq!(v.handle_byte(b'\n'), AccessEffect::Redraw);
        assert!(v.message.as_deref().unwrap().contains("administrator"));
        assert_eq!(level_name(2), "confidential");
    }
}
