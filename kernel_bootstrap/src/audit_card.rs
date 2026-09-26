//! The Audit card (FS-009): who tried what, and what the guard said.
//!
//! The decisions on the one looking's documents -- every document, for the
//! administrator -- newest first: when, who, what they asked for, which
//! document, allowed or refused, and the reason the guard gave. A chip
//! narrows it to the refusals; arrows and the wheel scroll. The kernel
//! builds the lines from the guard's audit log ([`AuditLine`]); nothing
//! here can change it.

extern crate alloc;

use crate::widgets::{rect, Palette, Ui};
use alloc::string::String;
use alloc::vec::Vec;

/// The "only refusals" chip's key.
pub const KEY_REFUSED_ONLY: u8 = b'r';
/// A row's height, in pixels.
pub const ROW: u32 = 26;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditLine {
    /// When, in words (`2026-09-26 01:57`).
    pub when: String,
    pub who: String,
    /// The right asked for (`read`, `history`...).
    pub right: String,
    pub doc: String,
    pub allowed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditEffect {
    None,
    Redraw,
    Close,
    Reload,
}

#[derive(Debug, Clone, Default)]
pub struct AuditView {
    lines: Option<Vec<AuditLine>>,
    error: Option<String>,
    pub refused_only: bool,
    scroll: usize,
}

impl AuditView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn loaded(&mut self, lines: Result<Vec<AuditLine>, String>) {
        match lines {
            Ok(l) => {
                self.lines = Some(l);
                self.error = None;
            }
            Err(e) => {
                self.lines = None;
                self.error = Some(e);
            }
        }
        self.scroll = self.scroll.min(self.shown().len().saturating_sub(1));
    }

    /// The lines shown: all of them, or the refusals.
    pub fn shown(&self) -> Vec<&AuditLine> {
        self.lines
            .iter()
            .flatten()
            .filter(|l| !self.refused_only || !l.allowed)
            .collect()
    }

    pub fn footer(&self) -> String {
        let Some(lines) = &self.lines else {
            return "Asking the filesystem...".into();
        };
        let refused = lines.iter().filter(|l| !l.allowed).count();
        alloc::format!(
            "{} decisions, {} refused   R: refusals only   Ctrl+R again   Esc closes",
            lines.len(),
            refused
        )
    }

    pub fn handle_byte(&mut self, byte: u8) -> AuditEffect {
        use crate::notepad::{CTRL_W, ESC, KEY_DOWN, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_UP};
        let n = self.shown().len();
        match byte {
            CTRL_W | ESC => AuditEffect::Close,
            b'r' | b'R' => {
                self.refused_only = !self.refused_only;
                self.scroll = 0;
                AuditEffect::Redraw
            }
            // Ctrl+R, or the chip: read the log again.
            0x12 => AuditEffect::Reload,
            KEY_DOWN if self.scroll + 1 < n => {
                self.scroll += 1;
                AuditEffect::Redraw
            }
            KEY_UP if self.scroll > 0 => {
                self.scroll -= 1;
                AuditEffect::Redraw
            }
            KEY_PAGE_DOWN => {
                self.scroll = (self.scroll + 10).min(n.saturating_sub(1));
                AuditEffect::Redraw
            }
            KEY_PAGE_UP => {
                self.scroll = self.scroll.saturating_sub(10);
                AuditEffect::Redraw
            }
            _ => AuditEffect::None,
        }
    }

    /// Scroll by `rows` (the wheel).
    pub fn scroll_by(&mut self, rows: i32) {
        let n = self.shown().len();
        self.scroll = (self.scroll as i32 + rows).clamp(0, n.saturating_sub(1) as i32) as usize;
    }

    pub fn ui(&self, w: u32, h: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let p = palette;
        let mut ui = Ui::new(palette, hover);
        let w_i = w as i32;
        let Some(_) = &self.lines else {
            ui.text(
                0,
                0,
                self.error.as_deref().unwrap_or("Asking the filesystem..."),
                p.muted,
                1,
            );
            return ui;
        };
        // Column heads.
        let cols = [0, 17 * 8, 27 * 8, 35 * 8, 51 * 8];
        for (x, head) in cols
            .iter()
            .zip(["when", "who", "asked", "document", "said"])
        {
            ui.text(*x, 0, head, p.muted, 1);
        }
        ui.line(0, 22, w_i, 22, p.hairline, 1);
        let shown = self.shown();
        if shown.is_empty() {
            let what = if self.refused_only {
                "Nothing refused -- no one has tried what they may not"
            } else {
                "Nothing yet: reads, writes and refusals on your documents land here"
            };
            ui.text(0, 34, what, p.muted, 1);
        }
        let rows = ((h.saturating_sub(30)) / ROW) as usize;
        for (i, line) in shown.iter().skip(self.scroll).take(rows).enumerate() {
            let y = 30 + i as i32 * ROW as i32;
            if !line.allowed {
                ui.fill(rect(-4, y - 3, w + 8, ROW - 2), p.raised, 6);
            }
            ui.text(cols[0], y + 2, &line.when, p.muted, 1);
            ui.text(cols[1], y + 2, &line.who, p.text, 1);
            ui.text(cols[2], y + 2, &line.right, p.text, 1);
            let doc: String = line.doc.chars().take(15).collect();
            ui.text(cols[3], y + 2, &doc, p.text, 1);
            let (said, ink) = if line.allowed {
                (alloc::format!("ok  {}", line.reason), p.muted)
            } else {
                (
                    alloc::format!("REFUSED  {}", line.reason),
                    crate::sign_in::ERROR_INK,
                )
            };
            let room = ((w_i - cols[4]) / 8).max(0) as usize;
            let said: String = said.chars().take(room).collect();
            ui.text(cols[4], y + 2, &said, ink, 1);
        }
        ui
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(allowed: bool, who: &str) -> AuditLine {
        AuditLine {
            when: "2026-09-26 01:57".into(),
            who: who.into(),
            right: "history".into(),
            doc: "plan".into(),
            allowed,
            reason: if allowed {
                "grant 1".into()
            } else {
                "holds only read".into()
            },
        }
    }

    #[test]
    fn the_log_shows_decisions_and_narrows_to_refusals() {
        let mut v = AuditView::new();
        assert!(v.footer().contains("Asking"));
        v.loaded(Ok(alloc::vec![
            line(false, "armando"),
            line(true, "armando"),
            line(false, "bea")
        ]));
        assert_eq!(v.shown().len(), 3);
        assert!(v.footer().starts_with("3 decisions, 2 refused"));
        assert_eq!(v.handle_byte(b'r'), AuditEffect::Redraw);
        assert_eq!(v.shown().len(), 2);
        let texts = crate::sign_in::texts(
            &v.ui(
                744,
                400,
                Palette::from_theme(&services_gui_host::Theme::DEFAULT),
                None,
            )
            .into_ops(),
        );
        assert!(
            texts.iter().any(|t| t == "REFUSED  holds only read"),
            "{texts:?}"
        );
        assert_eq!(v.handle_byte(crate::notepad::KEY_DOWN), AuditEffect::Redraw);
        assert_eq!(
            v.handle_byte(crate::notepad::KEY_DOWN),
            AuditEffect::None,
            "at the end"
        );
        assert_eq!(v.handle_byte(0x12), AuditEffect::Reload);
        v.loaded(Ok(Vec::new()));
        assert!(v.shown().is_empty());
        assert_eq!(v.handle_byte(crate::notepad::ESC), AuditEffect::Close);
    }
}
