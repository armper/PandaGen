//! Custom-component graphical host contract (GFX-040).
//!
//! A component that wants to live in the workspace window does not draw
//! pixels and does not know where it is on screen. It describes a
//! `HostedSurface` for the rows it is given and reacts to a small set of
//! `HostEvent`s. The host (shell + compositor) owns geometry, chrome, focus,
//! hit testing, and painting. This is the same shape the editor, file
//! picker, pipeline view, and scrollback already use, made explicit so new
//! apps plug in without touching the desktop builder.
//!
//! The contract is intentionally text-cell based for now: lines, a caret, a
//! highlighted row, a status string. Richer content (images, custom draw
//! ops) can be added as new `HostedSurface` fields without changing the
//! event side.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

use crate::{DesktopWindow, DesktopWindowRole, SurfaceRect};

/// What a hosted component shows this frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HostedSurface {
    /// Window title.
    pub title: String,
    /// Content lines, top to bottom; the host clips to the rows available.
    pub lines: Vec<String>,
    /// Caret as (line, column) in content cells, if the component has one.
    pub caret: Option<(usize, usize)>,
    /// Highlighted content line (selection, current line).
    pub highlight: Option<usize>,
    /// Left status-strip text.
    pub status: String,
}

impl HostedSurface {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn with_lines(mut self, lines: Vec<String>) -> Self {
        self.lines = lines;
        self
    }

    pub fn with_caret(mut self, line: usize, column: usize) -> Self {
        self.caret = Some((line, column));
        self
    }

    pub fn with_highlight(mut self, line: Option<usize>) -> Self {
        self.highlight = line;
        self
    }

    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }

    /// Build the workspace window for this surface.
    pub fn into_window(self, id: ViewId, rect: SurfaceRect) -> DesktopWindow {
        let mut frame = ViewFrame::new(
            id,
            ViewKind::TextBuffer,
            0,
            ViewContent::text_buffer(self.lines),
            0,
        )
        .with_title(self.title);
        if let Some((line, column)) = self.caret {
            frame = frame.with_cursor(CursorPosition::new(line, column));
        }
        DesktopWindow::new(frame, rect)
            .with_role(DesktopWindowRole::Main)
            .with_highlight(self.highlight)
    }
}

/// Input the host delivers to a hosted component.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostEvent {
    /// Pointer moved over content `line`.
    Hover { line: usize },
    /// Primary press on content `line`.
    Activate { line: usize },
    /// Wheel notches; positive is away from the user.
    Wheel { notches: i32 },
    /// A keyboard byte (the same byte model the kernel workspace uses).
    Key(u8),
}

/// What a component did with an event.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostResponse {
    /// Nothing changed.
    Ignored,
    /// State changed; the host should redraw.
    Redraw,
    /// The component asks to be closed.
    Close,
}

/// A component hosted in the workspace window.
pub trait HostedComponent {
    /// Describe the surface for a window with `rows` content rows.
    fn surface(&self, rows: usize) -> HostedSurface;
    /// React to an event.
    fn handle(&mut self, event: HostEvent) -> HostResponse;
}

/// A hosted list of activatable entries: the simplest useful component and
/// the reference implementation of the contract. Hover and Up/Down move the
/// selection, Enter or a click activates it, Esc or `q` closes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListComponent {
    pub title: String,
    pub entries: Vec<String>,
    pub selection: usize,
    pub status: String,
    /// Index activated by the last Activate/Enter, consumed by the host.
    pub activated: Option<usize>,
}

impl ListComponent {
    pub fn new(title: impl Into<String>, entries: Vec<String>) -> Self {
        Self {
            title: title.into(),
            entries,
            selection: 0,
            status: String::new(),
            activated: None,
        }
    }

    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }

    /// Take the last activation, if any.
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    fn select(&mut self, index: usize) -> HostResponse {
        if self.entries.is_empty() {
            return HostResponse::Ignored;
        }
        let index = index.min(self.entries.len() - 1);
        if index == self.selection {
            HostResponse::Ignored
        } else {
            self.selection = index;
            HostResponse::Redraw
        }
    }
}

/// Keyboard bytes shared with the kernel workspace input model.
pub const KEY_UP: u8 = 0x80;
pub const KEY_DOWN: u8 = 0x81;
pub const KEY_ESCAPE: u8 = 0x1b;
pub const KEY_ENTER: u8 = b'\n';

impl HostedComponent for ListComponent {
    fn surface(&self, rows: usize) -> HostedSurface {
        HostedSurface::new(self.title.clone())
            .with_lines(self.entries.iter().take(rows).cloned().collect())
            .with_highlight(if self.entries.is_empty() {
                None
            } else {
                Some(self.selection)
            })
            .with_status(self.status.clone())
    }

    fn handle(&mut self, event: HostEvent) -> HostResponse {
        match event {
            HostEvent::Hover { line } => self.select(line),
            HostEvent::Activate { line } => {
                self.select(line);
                if self.entries.is_empty() {
                    HostResponse::Ignored
                } else {
                    self.activated = Some(self.selection);
                    HostResponse::Redraw
                }
            }
            HostEvent::Wheel { notches } => {
                if notches > 0 {
                    self.select(self.selection.saturating_sub(notches as usize))
                } else {
                    self.select(
                        self.selection
                            .saturating_add(notches.unsigned_abs() as usize),
                    )
                }
            }
            HostEvent::Key(KEY_UP) | HostEvent::Key(b'k') => {
                self.select(self.selection.saturating_sub(1))
            }
            HostEvent::Key(KEY_DOWN) | HostEvent::Key(b'j') => {
                self.select(self.selection.saturating_add(1))
            }
            HostEvent::Key(KEY_ENTER) | HostEvent::Key(b'\r') => self.handle(HostEvent::Activate {
                line: self.selection,
            }),
            HostEvent::Key(KEY_ESCAPE) | HostEvent::Key(b'q') => HostResponse::Close,
            HostEvent::Key(_) => HostResponse::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn test_surface_becomes_a_main_window_with_caret_and_highlight() {
        let id = ViewId::new();
        let window = HostedSurface::new("Demo")
            .with_lines(vec!["a".to_string(), "b".to_string()])
            .with_caret(1, 3)
            .with_highlight(Some(0))
            .with_status("ok")
            .into_window(id, SurfaceRect::new(1, 2, 30, 10));
        assert_eq!(window.role, DesktopWindowRole::Main);
        assert_eq!(window.frame.view_id, id);
        assert_eq!(window.frame.title.as_deref(), Some("Demo"));
        assert_eq!(window.frame.cursor, Some(CursorPosition::new(1, 3)));
        assert_eq!(window.highlight_line, Some(0));
        assert_eq!(window.rect, SurfaceRect::new(1, 2, 30, 10));
    }

    #[test]
    fn test_list_component_follows_the_contract() {
        let mut list = ListComponent::new(
            "Pick",
            vec!["one".to_string(), "two".to_string(), "three".to_string()],
        )
        .with_status("Enter to choose");
        let surface = list.surface(2);
        assert_eq!(surface.lines, vec!["one", "two"], "clipped to rows");
        assert_eq!(surface.highlight, Some(0));
        assert_eq!(surface.status, "Enter to choose");

        assert_eq!(
            list.handle(HostEvent::Hover { line: 1 }),
            HostResponse::Redraw
        );
        assert_eq!(
            list.handle(HostEvent::Hover { line: 1 }),
            HostResponse::Ignored
        );
        assert_eq!(
            list.handle(HostEvent::Hover { line: 99 }),
            HostResponse::Redraw
        );
        assert_eq!(list.selection, 2, "clamped to the last entry");
        assert_eq!(
            list.handle(HostEvent::Wheel { notches: 1 }),
            HostResponse::Redraw
        );
        assert_eq!(list.selection, 1);
        assert_eq!(list.handle(HostEvent::Key(KEY_DOWN)), HostResponse::Redraw);
        assert_eq!(list.handle(HostEvent::Key(b'k')), HostResponse::Redraw);
        assert_eq!(list.selection, 1);
        assert_eq!(list.handle(HostEvent::Key(KEY_ENTER)), HostResponse::Redraw);
        assert_eq!(list.take_activated(), Some(1));
        assert_eq!(list.take_activated(), None);
        assert_eq!(
            list.handle(HostEvent::Activate { line: 0 }),
            HostResponse::Redraw
        );
        assert_eq!(list.take_activated(), Some(0));
        assert_eq!(list.handle(HostEvent::Key(b'x')), HostResponse::Ignored);
        assert_eq!(list.handle(HostEvent::Key(KEY_ESCAPE)), HostResponse::Close);

        let mut empty = ListComponent::new("Empty", vec![]);
        assert_eq!(empty.surface(5).highlight, None);
        assert_eq!(
            empty.handle(HostEvent::Activate { line: 0 }),
            HostResponse::Ignored
        );
    }
}
