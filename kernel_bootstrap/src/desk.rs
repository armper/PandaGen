//! The desk (GFX-050/052): a window manager for cards, a dock and a top bar.
//!
//! Apps own windows; the shell stays out of the way. This module holds the
//! pure part -- where the windows are, which one has focus, what is being
//! dragged or resized -- and turns pointer deliveries and key bytes into
//! changes to that state. It builds the `DesktopWindow` list the compositor
//! paints; the kernel loop does the painting, the presenting, the file I/O
//! and the console it asks for.
//!
//! Everything here is host-testable: nothing touches hardware.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use graphics_rasterizer::RasterRect;
use input_types::{PointerButton, PointerEventKind};
use services_gui_host::{
    Delivery, DesktopTab, DesktopWindow, DesktopWindowLayer, DesktopWindowRole, HitRegion,
    SurfaceRect, WindowStyle,
};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

use crate::notepad::{Notepad, NotepadEffect};

pub const TOP_BAR_HEIGHT: usize = 28;
pub const DOCK_HEIGHT: usize = 56;
pub const DOCK_MARGIN: usize = 12;
pub const NOTEPAD_SIZE: (usize, usize) = (720, 480);
pub const TERMINAL_SIZE: (usize, usize) = (800, 520);
/// Successive windows open offset by this much.
pub const CASCADE_STEP: usize = 32;
/// The smallest a card can be resized to.
pub const MIN_CARD_SIZE: (usize, usize) = (240, 140);
/// How close to an edge a drag must end to snap there.
pub const SNAP_MARGIN: usize = 6;
/// Ctrl+Tab, as the parser delivers it.
pub const KEY_CTRL_TAB: u8 = 0x85;

/// The apps the dock offers, in dock order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskApp {
    Notepad,
    /// The machine's console -- the `WS >` prompt and everything it can do --
    /// as a card, so the desk never has to be left to reach it.
    Terminal,
}

impl DeskApp {
    pub const ALL: [DeskApp; 2] = [DeskApp::Notepad, DeskApp::Terminal];

    pub const fn name(self) -> &'static str {
        match self {
            DeskApp::Notepad => "Notepad",
            DeskApp::Terminal => "Terminal",
        }
    }

    /// Two letters for the dock tile; there are no icons in this tree.
    pub const fn monogram(self) -> &'static str {
        match self {
            DeskApp::Notepad => "Np",
            DeskApp::Terminal => "Tm",
        }
    }

    const fn size(self) -> (usize, usize) {
        match self {
            DeskApp::Notepad => NOTEPAD_SIZE,
            DeskApp::Terminal => TERMINAL_SIZE,
        }
    }
}

/// What an open window holds.
#[derive(Debug, Clone)]
pub enum AppState {
    Notepad(Notepad),
    /// The console's state lives in the workspace; the card only shows it.
    Terminal,
}

/// One open window.
#[derive(Debug, Clone)]
pub struct DeskWindow {
    pub id: ViewId,
    pub app: DeskApp,
    pub bounds: RasterRect,
    pub z: usize,
    /// The bounds before a snap, so the next header drag un-snaps to them.
    pub restore: Option<RasterRect>,
    pub state: AppState,
}

impl DeskWindow {
    pub fn notepad(&self) -> Option<&Notepad> {
        match &self.state {
            AppState::Notepad(notepad) => Some(notepad),
            AppState::Terminal => None,
        }
    }

    pub fn notepad_mut(&mut self) -> Option<&mut Notepad> {
        match &mut self.state {
            AppState::Notepad(notepad) => Some(notepad),
            AppState::Terminal => None,
        }
    }
}

/// A header drag in progress: which window, and where inside its header
/// the pointer grabbed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    id: ViewId,
    grab_x: usize,
    grab_y: usize,
}

/// A corner resize in progress: which window, and how far inside the
/// corner the pointer grabbed it, so the corner stays under the pointer
/// rather than jumping to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Resize {
    id: ViewId,
    grab_dx: usize,
    grab_dy: usize,
}

/// What the desk asks the kernel to do after handling input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskRequest {
    /// Perform this file operation for the window with `id`, then call
    /// [`Desk::io_done`].
    Io { id: ViewId, effect: NotepadEffect },
    /// A key for the console, which the workspace owns.
    Terminal(u8),
}

/// The console as the Terminal card shows it, built by the kernel from the
/// workspace: the lines that fit, and the caret on the prompt line.
#[derive(Debug, Clone, Default)]
pub struct TerminalView {
    pub lines: Vec<String>,
    /// `(line, character column)` of the caret within `lines`.
    pub cursor: Option<(usize, usize)>,
    pub status: String,
}

/// The whole desk.
#[derive(Debug, Clone)]
pub struct Desk {
    width: usize,
    height: usize,
    top_bar_id: ViewId,
    dock_id: ViewId,
    windows: Vec<DeskWindow>,
    focus: Option<ViewId>,
    drag: Option<Drag>,
    resize: Option<Resize>,
    next_z: usize,
    opened: usize,
    pointer: Option<(usize, usize)>,
    hovered_tile: Option<usize>,
}

impl Desk {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            top_bar_id: ViewId::new(),
            dock_id: ViewId::new(),
            windows: Vec::new(),
            focus: None,
            drag: None,
            resize: None,
            next_z: 1,
            opened: 0,
            pointer: None,
            hovered_tile: None,
        }
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn focus(&self) -> Option<ViewId> {
        self.focus
    }

    pub fn window(&self, id: ViewId) -> Option<&DeskWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: ViewId) -> Option<&mut DeskWindow> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn focused_window(&self) -> Option<&DeskWindow> {
        self.focus.and_then(|id| self.window(id))
    }

    /// Whether a key typed now belongs to an app rather than the shell.
    pub fn wants_keys(&self) -> bool {
        self.focused_window().is_some()
    }

    /// Whether the console has a card open, and if so how many content rows
    /// it shows -- so the kernel can build a [`TerminalView`] that fits.
    pub fn terminal_rows(&self) -> Option<usize> {
        let window = self.windows.iter().find(|w| w.app == DeskApp::Terminal)?;
        Some(Self::card_rows(window.bounds))
    }

    pub fn set_pointer(&mut self, position: Option<(usize, usize)>) {
        self.pointer = position;
    }

    /// The area cards may occupy: below the top bar, above the dock.
    fn work_area(&self) -> RasterRect {
        let top = TOP_BAR_HEIGHT;
        let bottom = self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN * 2);
        RasterRect::new(0, top, self.width, bottom.saturating_sub(top))
    }

    fn card_rows(bounds: RasterRect) -> usize {
        DesktopWindow::card(
            ViewFrame::new(
                ViewId::new(),
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(Vec::new()),
                0,
            ),
            bounds,
        )
        .with_footer(Some(String::new()))
        .content_rows()
    }

    /// Open `app` in a new card, cascaded from the last, and focus it.
    pub fn launch(&mut self, app: DeskApp) -> ViewId {
        let (w, h) = app.size();
        let area = self.work_area();
        let w = w.min(area.width);
        let h = h.min(area.height);
        let step = (self.opened % 6) * CASCADE_STEP;
        let x =
            (area.x + area.width.saturating_sub(w) / 2 + step).min(area.right().saturating_sub(w));
        let y = (area.y + 16 + step).min(area.bottom().saturating_sub(h).max(area.y));
        let id = ViewId::new();
        let z = self.next_z;
        self.next_z += 1;
        self.opened += 1;
        self.windows.push(DeskWindow {
            id,
            app,
            bounds: RasterRect::new(x, y, w, h),
            z,
            restore: None,
            state: match app {
                DeskApp::Notepad => AppState::Notepad(Notepad::new()),
                DeskApp::Terminal => AppState::Terminal,
            },
        });
        self.focus = Some(id);
        id
    }

    /// Bring `id` to the front and give it focus.
    pub fn raise(&mut self, id: ViewId) {
        let z = self.next_z;
        if let Some(window) = self.window_mut(id) {
            window.z = z;
            self.next_z += 1;
            self.focus = Some(id);
        }
    }

    pub fn close(&mut self, id: ViewId) {
        self.windows.retain(|w| w.id != id);
        if self.drag.map(|d| d.id) == Some(id) {
            self.drag = None;
        }
        if self.resize.map(|r| r.id) == Some(id) {
            self.resize = None;
        }
        if self.focus == Some(id) {
            // The top-most remaining window takes focus.
            self.focus = self.windows.iter().max_by_key(|w| w.z).map(|w| w.id);
        }
    }

    /// Focus the next window in z order (Ctrl+Tab).
    pub fn cycle_focus(&mut self) -> bool {
        if self.windows.len() < 2 {
            return false;
        }
        // The lowest window comes to the top, so repeated presses walk the
        // whole stack.
        let lowest = self.windows.iter().min_by_key(|w| w.z).map(|w| w.id);
        if let Some(id) = lowest {
            self.raise(id);
        }
        true
    }

    /// A dock tile was pressed: focus the app's window if it has one, else
    /// launch it.
    pub fn activate_dock_tile(&mut self, index: usize) -> bool {
        let Some(app) = DeskApp::ALL.get(index).copied() else {
            return false;
        };
        let existing = self
            .windows
            .iter()
            .filter(|w| w.app == app)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        match existing {
            Some(id) => self.raise(id),
            None => {
                self.launch(app);
            }
        }
        true
    }

    /// Snap a window whose drag ended at the desk's edge (GFX-052): left or
    /// right half, or the whole work area from the top edge.
    fn snap_if_at_edge(&mut self, id: ViewId, px: usize, py: usize) -> bool {
        let area = self.work_area();
        let target = if px < SNAP_MARGIN {
            Some(RasterRect::new(area.x, area.y, area.width / 2, area.height))
        } else if px + SNAP_MARGIN >= self.width {
            Some(RasterRect::new(
                area.x + area.width / 2,
                area.y,
                area.width - area.width / 2,
                area.height,
            ))
        } else if py <= area.y + SNAP_MARGIN {
            Some(area)
        } else {
            None
        };
        let Some(target) = target else {
            return false;
        };
        if let Some(window) = self.window_mut(id) {
            if window.restore.is_none() {
                window.restore = Some(window.bounds);
            }
            window.bounds = target;
            return true;
        }
        false
    }

    /// Apply routed pointer deliveries. Returns whether the screen changed.
    pub fn handle_deliveries(&mut self, deliveries: &[Delivery]) -> bool {
        let mut changed = false;
        for delivery in deliveries {
            match delivery {
                Delivery::FocusChanged { current, .. } => {
                    if let Some(id) = current {
                        if self.window(*id).is_some() {
                            self.raise(*id);
                            changed = true;
                        }
                    }
                }
                Delivery::Leave { target } if *target == self.dock_id => {
                    if self.hovered_tile.take().is_some() {
                        changed = true;
                    }
                }
                Delivery::Pointer { target, event, hit } => {
                    let press = event.is_press(PointerButton::Primary);
                    let release = event.is_release(PointerButton::Primary);
                    let region = hit.map(|h| h.region);
                    let px = event.position.x.max(0) as usize;
                    let py = event.position.y.max(0) as usize;

                    if *target == self.dock_id {
                        let tile = match region {
                            Some(HitRegion::DockTile { index }) => Some(index),
                            _ => None,
                        };
                        if self.hovered_tile != tile {
                            self.hovered_tile = tile;
                            changed = true;
                        }
                        if let (true, Some(index)) = (press, tile) {
                            changed |= self.activate_dock_tile(index);
                        }
                        continue;
                    }
                    if self.window(*target).is_none() {
                        continue;
                    }

                    match (press, release, region, &event.kind) {
                        (true, _, Some(HitRegion::Close), _) => {
                            self.close(*target);
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Resize), _) => {
                            self.raise(*target);
                            if let Some(window) = self.window(*target) {
                                self.resize = Some(Resize {
                                    id: *target,
                                    grab_dx: window.bounds.right().saturating_sub(px),
                                    grab_dy: window.bounds.bottom().saturating_sub(py),
                                });
                            }
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Header), _) => {
                            self.raise(*target);
                            if let Some(window) = self.window_mut(*target) {
                                // A snapped card un-snaps under the pointer.
                                if let Some(restore) = window.restore.take() {
                                    let half = restore.width / 2;
                                    window.bounds = RasterRect::new(
                                        px.saturating_sub(half),
                                        window.bounds.y,
                                        restore.width,
                                        restore.height,
                                    );
                                }
                                self.drag = Some(Drag {
                                    id: *target,
                                    grab_x: px.saturating_sub(window.bounds.x),
                                    grab_y: py.saturating_sub(window.bounds.y),
                                });
                            }
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Content { line, column }), _) => {
                            self.raise(*target);
                            if let Some(notepad) =
                                self.window_mut(*target).and_then(|w| w.notepad_mut())
                            {
                                notepad.place_cursor(line, column);
                            }
                            changed = true;
                        }
                        (true, _, _, _) => {
                            self.raise(*target);
                            changed = true;
                        }
                        (_, _, _, PointerEventKind::Wheel { dy, .. }) => {
                            if let Some(notepad) =
                                self.window_mut(*target).and_then(|w| w.notepad_mut())
                            {
                                notepad.scroll_by(-(*dy) * 3);
                                changed = true;
                            }
                        }
                        (_, _, _, PointerEventKind::Move { .. }) => {
                            if let Some(drag) = self.drag.filter(|d| d.id == *target) {
                                let (width, height) = (self.width, self.height);
                                if let Some(window) = self.window_mut(*target) {
                                    let max_x = width.saturating_sub(window.bounds.width);
                                    let max_y = height.saturating_sub(window.bounds.height);
                                    window.bounds.x = px.saturating_sub(drag.grab_x).min(max_x);
                                    window.bounds.y = py
                                        .saturating_sub(drag.grab_y)
                                        .max(TOP_BAR_HEIGHT)
                                        .min(max_y);
                                    changed = true;
                                }
                            } else if let Some(resize) = self.resize.filter(|r| r.id == *target) {
                                let (width, height) = (self.width, self.height);
                                if let Some(window) = self.window_mut(*target) {
                                    let (min_w, min_h) = MIN_CARD_SIZE;
                                    window.restore = None;
                                    window.bounds.width = (px + resize.grab_dx)
                                        .saturating_sub(window.bounds.x)
                                        .max(min_w)
                                        .min(width.saturating_sub(window.bounds.x));
                                    window.bounds.height = (py + resize.grab_dy)
                                        .saturating_sub(window.bounds.y)
                                        .max(min_h)
                                        .min(height.saturating_sub(window.bounds.y));
                                    changed = true;
                                }
                            }
                        }
                        (_, true, _, _) => {
                            if self.drag.map(|d| d.id) == Some(*target) {
                                self.drag = None;
                                changed |= self.snap_if_at_edge(*target, px, py);
                            }
                            if self.resize.map(|r| r.id) == Some(*target) {
                                self.resize = None;
                            }
                        }
                        _ => {}
                    }
                }
                Delivery::Capture { transition, .. } => {
                    if matches!(transition, input_types::PointerCapture::Lost) {
                        self.drag = None;
                        self.resize = None;
                    }
                }
                _ => {}
            }
        }
        changed
    }

    /// A key for the focused app. Returns what the kernel must do, if
    /// anything, and whether the screen changed.
    pub fn handle_key(&mut self, byte: u8) -> (Option<DeskRequest>, bool) {
        // Launching is global. The first build only answered Ctrl+T with
        // nothing focused, so with a Notepad open the keystroke fell into
        // the Notepad and did nothing -- a shortcut that only works when
        // there is nothing to use it on. Ctrl+N stays the Notepad's own
        // "new document" while a Notepad is focused; from a Terminal, or
        // from the bare desk, it opens one.
        if byte == KEY_CTRL_TAB {
            return (None, self.cycle_focus());
        }
        if byte == crate::notepad::CTRL_T {
            self.launch(DeskApp::Terminal);
            return (None, true);
        }
        let Some(id) = self.focus else {
            return (None, false);
        };
        if byte == crate::notepad::CTRL_N
            && self.window(id).map(|w| w.app) == Some(DeskApp::Terminal)
        {
            self.launch(DeskApp::Notepad);
            return (None, true);
        }
        let Some(window) = self.window_mut(id) else {
            return (None, false);
        };
        match &mut window.state {
            AppState::Terminal => {
                if byte == crate::notepad::CTRL_W {
                    self.close(id);
                    return (None, true);
                }
                (Some(DeskRequest::Terminal(byte)), true)
            }
            AppState::Notepad(notepad) => match notepad.handle_byte(byte) {
                NotepadEffect::None => (None, false),
                NotepadEffect::Redraw => (None, true),
                NotepadEffect::Close => {
                    self.close(id);
                    (None, true)
                }
                effect @ (NotepadEffect::Save { .. } | NotepadEffect::Open { .. }) => {
                    (Some(DeskRequest::Io { id, effect }), true)
                }
            },
        }
    }

    /// A key with no app focused: the desk's own shortcuts. Ctrl+N opens a
    /// Notepad and Ctrl+T a Terminal, so the machine is usable from the
    /// keyboard alone -- and so the gauntlet can drive it without knowing
    /// where the pointer starts.
    pub fn handle_shell_key(&mut self, byte: u8) -> bool {
        match byte {
            crate::notepad::CTRL_N => {
                self.launch(DeskApp::Notepad);
                true
            }
            crate::notepad::CTRL_T => {
                self.launch(DeskApp::Terminal);
                true
            }
            KEY_CTRL_TAB => self.cycle_focus(),
            _ => false,
        }
    }

    /// The kernel reports a file operation's outcome.
    pub fn io_done(
        &mut self,
        id: ViewId,
        effect: &NotepadEffect,
        result: Result<Option<String>, String>,
    ) {
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.io_done(effect, result);
        }
    }

    /// The window list for the compositor, top bar and dock included.
    ///
    /// `clock` is the top bar's right-hand text; `terminal` is the console
    /// as the kernel built it for the Terminal card, if one is open.
    pub fn windows(
        &mut self,
        clock: &str,
        caret_visible: bool,
        terminal: Option<&TerminalView>,
    ) -> Vec<DesktopWindow> {
        let mut out = Vec::with_capacity(self.windows.len() + 2);

        // Cards.
        let focus = self.focus;
        for window in &mut self.windows {
            let rows = Self::card_rows(window.bounds);
            let focused = focus == Some(window.id);
            let (lines, title, footer, cursor) = match &mut window.state {
                AppState::Notepad(notepad) => {
                    let lines = notepad.viewport_lines(rows);
                    let cursor = notepad.viewport_cursor();
                    (lines, notepad.title(), notepad.footer(), cursor)
                }
                AppState::Terminal => {
                    let view = terminal.cloned().unwrap_or_default();
                    (view.lines, "Terminal".to_string(), view.status, view.cursor)
                }
            };
            let mut frame = ViewFrame::new(
                window.id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(title);
            if caret_visible && focused {
                if let Some((line, column)) = cursor {
                    frame.cursor = Some(CursorPosition::new(line, column));
                }
            }
            let mut card = DesktopWindow::card(frame, window.bounds)
                .with_z_index(window.z)
                .with_footer(Some(footer));
            if focused {
                card = card.focused();
            }
            out.push(card);
        }

        // Top bar: the focused app's name on the left, the clock on the right.
        let left = self
            .focused_window()
            .map(|w| w.app.name().to_string())
            .unwrap_or_else(|| "PandaGen".to_string());
        let mut bar_frame = ViewFrame::new(
            self.top_bar_id,
            ViewKind::StatusLine,
            0,
            ViewContent::text_buffer(alloc::vec![clock.to_string()]),
            0,
        );
        bar_frame.title = Some(left);
        out.push(
            DesktopWindow::new(bar_frame, SurfaceRect::new(0, 0, 0, 0))
                .with_role(DesktopWindowRole::Status)
                .with_layer(DesktopWindowLayer::System)
                .with_style(WindowStyle::TopBar)
                .with_pixel_rect(RasterRect::new(0, 0, self.width, TOP_BAR_HEIGHT)),
        );

        // Dock: one tile per app, the running ones marked, the hovered one lit.
        let tabs: Vec<DesktopTab> = DeskApp::ALL
            .iter()
            .enumerate()
            .map(|(index, app)| {
                let mut tab =
                    DesktopTab::new(app.monogram(), self.windows.iter().any(|w| w.app == *app));
                tab.hovered = self.hovered_tile == Some(index);
                tab
            })
            .collect();
        let count = tabs.len();
        let pill_width = count * services_gui_host::DOCK_TILE
            + count.saturating_sub(1) * services_gui_host::DOCK_TILE_GAP
            + 32;
        let dock_frame = ViewFrame::new(
            self.dock_id,
            ViewKind::Panel,
            0,
            ViewContent::text_buffer(Vec::new()),
            0,
        );
        out.push(
            DesktopWindow::new(dock_frame, SurfaceRect::new(0, 0, 0, 0))
                .with_role(DesktopWindowRole::Launcher)
                .with_layer(DesktopWindowLayer::System)
                .with_style(WindowStyle::Dock)
                .with_tabs(tabs)
                .with_pixel_rect(RasterRect::new(
                    self.width.saturating_sub(pill_width) / 2,
                    self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN),
                    pill_width,
                    DOCK_HEIGHT,
                )),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{ButtonState, Modifiers, PointerButtons, PointerEvent, PointerPosition};
    use services_gui_host::{Compositor, DesktopInputRouter};

    fn pointer(kind: PointerEventKind, x: i32, y: i32, buttons: PointerButtons) -> PointerEvent {
        PointerEvent::new(kind, PointerPosition::new(x, y), buttons, Modifiers::NONE)
    }

    fn press(x: i32, y: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Button {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            },
            x,
            y,
            PointerButtons::PRIMARY,
        )
    }

    fn release(x: i32, y: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Button {
                button: PointerButton::Primary,
                state: ButtonState::Released,
            },
            x,
            y,
            PointerButtons::none(),
        )
    }

    fn moved(x: i32, y: i32, buttons: PointerButtons) -> PointerEvent {
        pointer(
            PointerEventKind::Move {
                delta: input_types::PointerDelta::new(0, 0),
            },
            x,
            y,
            buttons,
        )
    }

    fn wheel(x: i32, y: i32, dy: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Wheel { dx: 0, dy },
            x,
            y,
            PointerButtons::none(),
        )
    }

    /// Drive one event through the real router against the desk's own
    /// window list, then apply the deliveries.
    fn route(desk: &mut Desk, router: &mut DesktopInputRouter, event: PointerEvent) -> bool {
        let compositor = Compositor::new();
        let windows = desk.windows("00:00", true, None);
        let deliveries = router.route(&compositor, &windows, event);
        desk.handle_deliveries(&deliveries)
    }

    fn drag(desk: &mut Desk, router: &mut DesktopInputRouter, from: (i32, i32), to: (i32, i32)) {
        route(desk, router, press(from.0, from.1));
        route(desk, router, moved(to.0, to.1, PointerButtons::PRIMARY));
        route(desk, router, release(to.0, to.1));
    }

    #[test]
    fn ctrl_n_and_ctrl_t_on_the_bare_desk_open_apps() {
        let mut desk = Desk::new(1280, 800);
        assert!(desk.handle_shell_key(crate::notepad::CTRL_N));
        assert!(desk.handle_shell_key(crate::notepad::CTRL_T));
        assert_eq!(desk.window_count(), 2);
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );
        assert!(desk.terminal_rows().is_some());
        assert!(!desk.handle_shell_key(b'x'));
    }

    /// The first build answered the launch shortcuts only with nothing
    /// focused, so with a Notepad open Ctrl+T fell into the Notepad and did
    /// nothing. Launching is global; Ctrl+N is the Notepad's own inside one.
    #[test]
    fn launch_shortcuts_work_whatever_is_focused() {
        let mut desk = Desk::new(1280, 800);
        desk.handle_shell_key(crate::notepad::CTRL_N);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // Ctrl+T from inside the Notepad opens a Terminal.
        let (_, changed) = desk.handle_key(crate::notepad::CTRL_T);
        assert!(changed);
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // Ctrl+N from the Terminal opens a second Notepad...
        desk.handle_key(crate::notepad::CTRL_N);
        assert_eq!(desk.window_count(), 3);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // ...while inside a Notepad it is "new document", not a new window.
        for byte in b"draft" {
            desk.handle_key(*byte);
        }
        desk.handle_key(crate::notepad::CTRL_N);
        desk.handle_key(crate::notepad::CTRL_N); // confirm the discard
        assert_eq!(desk.window_count(), 3);
        assert_eq!(
            desk.focused_window()
                .and_then(|w| w.notepad())
                .map(|n| n.content()),
            Some(String::new())
        );
    }

    #[test]
    fn the_empty_desk_has_a_top_bar_and_a_dock_and_nothing_else() {
        let mut desk = Desk::new(1280, 800);
        let windows = desk.windows("12:34", true, None);
        assert_eq!(windows.len(), 2);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert!(
            matches!(&bar.frame.content, ViewContent::TextBuffer { lines } if lines[0] == "12:34")
        );
        assert!(windows.iter().any(|w| w.style == WindowStyle::Dock));
        assert!(!desk.wants_keys());
    }

    #[test]
    fn clicking_the_dock_tile_launches_notepad_once_and_then_focuses_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        // Two tiles: 2*40 + 8 = 88 wide, centred in the pill. The first
        // tile's centre is 44px left of the pill's centre... plus half a tile.
        let pill = dock.bounds();
        let first_x = (pill.x + (pill.width - 88) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;

        assert!(route(&mut desk, &mut router, press(first_x, y)));
        route(&mut desk, &mut router, release(first_x, y));
        assert_eq!(desk.window_count(), 1);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // A second click does not open a second Notepad.
        route(&mut desk, &mut router, press(first_x, y));
        route(&mut desk, &mut router, release(first_x, y));
        assert_eq!(desk.window_count(), 1);
    }

    #[test]
    fn hovering_a_dock_tile_lights_it_and_leaving_the_dock_clears_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let first_x = (pill.x + (pill.width - 88) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;

        assert!(route(
            &mut desk,
            &mut router,
            moved(first_x, y, PointerButtons::none())
        ));
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(dock.tabs[0].hovered && !dock.tabs[1].hovered);

        route(
            &mut desk,
            &mut router,
            moved(10, 300, PointerButtons::none()),
        );
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(!dock.tabs[0].hovered);
    }

    #[test]
    fn a_card_drags_by_its_header_and_stays_on_the_desk() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;

        let grab = ((before.x + 100) as i32, (before.y + 10) as i32);
        drag(&mut desk, &mut router, grab, (grab.0 + 50, grab.1 + 40));
        let after = desk.window(id).unwrap().bounds;
        assert_eq!((after.x, after.y), (before.x + 50, before.y + 40));

        // Dragging far off-screen clamps to the desk.
        drag(
            &mut desk,
            &mut router,
            (after.x as i32 + 100, after.y as i32 + 10),
            (1200, 700),
        );
        let clamped = desk.window(id).unwrap().bounds;
        assert!(clamped.right() <= 1280 && clamped.bottom() <= 800);
    }

    #[test]
    fn a_drag_to_the_edge_snaps_and_the_next_drag_unsnaps() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let area = desk.work_area();

        // To the left edge: the left half.
        drag(
            &mut desk,
            &mut router,
            ((before.x + 100) as i32, (before.y + 10) as i32),
            (2, 300),
        );
        let snapped = desk.window(id).unwrap().bounds;
        assert_eq!(
            (snapped.x, snapped.width),
            (0, area.width / 2),
            "not the left half"
        );
        assert_eq!(snapped.height, area.height);

        // To the top: the whole work area.
        drag(
            &mut desk,
            &mut router,
            (100, (snapped.y + 10) as i32),
            (600, TOP_BAR_HEIGHT as i32 + 2),
        );
        let maxed = desk.window(id).unwrap().bounds;
        assert_eq!(maxed, area, "not maximised");

        // Dragging it away restores the original size under the pointer.
        drag(
            &mut desk,
            &mut router,
            (600, (maxed.y + 10) as i32),
            (640, 400),
        );
        let restored = desk.window(id).unwrap().bounds;
        assert_eq!(
            (restored.width, restored.height),
            (before.width, before.height)
        );
    }

    #[test]
    fn the_corner_grip_resizes_within_limits() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let grip = ((before.right() - 4) as i32, (before.bottom() - 4) as i32);

        drag(&mut desk, &mut router, grip, (grip.0 - 100, grip.1 - 60));
        let smaller = desk.window(id).unwrap().bounds;
        assert_eq!(
            (smaller.width, smaller.height),
            (before.width - 100, before.height - 60)
        );

        // Never below the minimum, whatever the pointer does.
        let grip = ((smaller.right() - 4) as i32, (smaller.bottom() - 4) as i32);
        drag(&mut desk, &mut router, grip, (10, 10));
        let floor = desk.window(id).unwrap().bounds;
        assert_eq!((floor.width, floor.height), MIN_CARD_SIZE);
    }

    #[test]
    fn clicking_in_the_text_places_the_caret_and_the_wheel_scrolls() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        for i in 0..60 {
            for byte in alloc::format!("line {i}").bytes() {
                desk.handle_key(byte);
            }
            desk.handle_key(b'\n');
        }
        let bounds = desk.window(id).unwrap().bounds;
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let (ox, oy, pitch) = card.card_text_origin();

        // Wheel up scrolls the view back towards the top.
        let rows = Desk::card_rows(bounds);
        route(
            &mut desk,
            &mut router,
            wheel((ox + 40) as i32, (oy + 40) as i32, 10),
        );
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let first_shown = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines[0].clone(),
            _ => panic!(),
        };
        assert_ne!(
            first_shown,
            alloc::format!("line {}", 61 - rows),
            "the wheel did not scroll"
        );

        // Click on the third visible line, column 3.
        let (x, y) = ((ox + 8 * 3 + 2) as i32, (oy + pitch * 2 + 5) as i32);
        route(&mut desk, &mut router, press(x, y));
        route(&mut desk, &mut router, release(x, y));
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(
            notepad.viewport_cursor(),
            Some((2, 3)),
            "the caret did not follow the click"
        );
    }

    #[test]
    fn the_close_glyph_closes_and_focus_falls_to_the_next_card() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let first = desk.launch(DeskApp::Notepad);
        let second = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.focus(), Some(second));

        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == second).unwrap();
        let close = card.close_rect().unwrap();
        let (x, y) = (
            (close.x + close.width / 2) as i32,
            (close.y + close.height / 2) as i32,
        );
        route(&mut desk, &mut router, press(x, y));
        assert_eq!(desk.window_count(), 1);
        assert_eq!(desk.focus(), Some(first));
    }

    #[test]
    fn ctrl_tab_walks_the_stack() {
        let mut desk = Desk::new(1280, 800);
        let a = desk.launch(DeskApp::Notepad);
        let b = desk.launch(DeskApp::Terminal);
        let c = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.focus(), Some(c));
        desk.handle_key(KEY_CTRL_TAB);
        assert_eq!(desk.focus(), Some(a));
        desk.handle_key(KEY_CTRL_TAB);
        assert_eq!(desk.focus(), Some(b));
        desk.handle_key(KEY_CTRL_TAB);
        assert_eq!(desk.focus(), Some(c));
    }

    #[test]
    fn terminal_keys_go_to_the_console_and_its_view_fills_the_card() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Terminal);
        let (request, changed) = desk.handle_key(b'h');
        assert_eq!(request, Some(DeskRequest::Terminal(b'h')));
        assert!(changed);

        let view = TerminalView {
            lines: alloc::vec!["PandaGen Workspace".to_string(), "WS > h".to_string()],
            cursor: Some((1, 6)),
            status: "WS".to_string(),
        };
        let windows = desk.windows("", true, Some(&view));
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.frame.title.as_deref(), Some("Terminal"));
        assert_eq!(card.frame.cursor.map(|c| (c.line, c.column)), Some((1, 6)));

        // Ctrl+W closes the console card rather than reaching the console.
        let (request, _) = desk.handle_key(crate::notepad::CTRL_W);
        assert_eq!(request, None);
        assert_eq!(desk.window_count(), 0);
    }

    #[test]
    fn keys_go_to_the_focused_notepad_and_io_comes_back_as_a_request() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        for byte in b"hi" {
            let (request, changed) = desk.handle_key(*byte);
            assert!(request.is_none() && changed);
        }
        let (request, _) = desk.handle_key(crate::notepad::CTRL_S);
        assert!(request.is_none(), "the first save asks for a name");
        for byte in b"n.txt" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        match request {
            Some(DeskRequest::Io { id: got, effect }) => {
                assert_eq!(got, id);
                assert_eq!(
                    effect,
                    NotepadEffect::Save {
                        path: "n.txt".to_string(),
                        content: "hi".to_string()
                    }
                );
                desk.io_done(id, &effect, Ok(None));
            }
            other => panic!("expected a save request, got {other:?}"),
        }
        assert!(!desk.window(id).unwrap().notepad().unwrap().is_dirty());
        let windows = desk.windows("09:41", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert_eq!(bar.frame.title.as_deref(), Some("Notepad"));
    }
}
