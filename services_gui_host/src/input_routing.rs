//! Pointer focus, keyboard focus, and capture policy (GFX-024).
//!
//! PandaGen makes input ownership explicit. This router is the single place
//! where a raw `PointerEvent` becomes a decision about *who* receives it:
//!
//! - **Hover**: the window under the pointer receives moves and wheel events;
//!   crossing a window edge yields `Leave`/`Enter` deliveries.
//! - **Focus follows click**: pressing a button on a focusable window makes it
//!   the keyboard-focus owner. Status and notification surfaces are not
//!   focusable, so clicking them never steals the keyboard.
//! - **Implicit capture on press**: the window that receives a press captures
//!   the pointer until every button is released, so drags that leave the
//!   window keep reporting to it. Capture start and end are delivered as
//!   `PointerCapture::Gained`/`Lost` events, never inferred.
//! - **Loss is explicit**: if the capturing window disappears, or the device
//!   layer reports capture lost, the capturer is told and capture ends.
//!
//! The router is pure state over a window list and a compositor; it never
//! touches hardware and is fully unit tested.

use alloc::vec::Vec;
use input_types::{ButtonState, PointerButton, PointerCapture, PointerEvent, PointerEventKind};
use serde::{Deserialize, Serialize};
use view_types::ViewId;

use crate::{Compositor, DesktopWindow, DesktopWindowRole, HitTarget};

impl DesktopWindowRole {
    /// Whether clicking a window with this role moves keyboard focus to it.
    pub const fn is_focusable(self) -> bool {
        !matches!(
            self,
            DesktopWindowRole::Status | DesktopWindowRole::Notification
        )
    }

    /// Stable lowercase name for logs and status output.
    pub const fn label(self) -> &'static str {
        match self {
            DesktopWindowRole::Main => "main",
            DesktopWindowRole::Status => "status",
            DesktopWindowRole::Overlay => "overlay",
            DesktopWindowRole::Palette => "palette",
            DesktopWindowRole::Notification => "notification",
            DesktopWindowRole::Modal => "modal",
        }
    }
}

/// One thing the router decided a consumer should receive.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Delivery {
    /// A pointer event addressed to a window, with the hit (if the pointer is
    /// over that window; a captured drag outside the window has `None`).
    Pointer {
        target: ViewId,
        event: PointerEvent,
        hit: Option<HitTarget>,
    },
    /// A pointer event over bare desktop (no window).
    Desktop { event: PointerEvent },
    /// Pointer entered a window's bounds.
    Enter { target: ViewId },
    /// Pointer left a window's bounds.
    Leave { target: ViewId },
    /// Capture ownership changed for `target`.
    Capture {
        target: ViewId,
        transition: PointerCapture,
        event: PointerEvent,
    },
    /// Keyboard focus moved (`None` means no window has focus).
    FocusChanged {
        previous: Option<ViewId>,
        current: Option<ViewId>,
    },
}

/// Active capture: which window and which button started it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureState {
    pub target: ViewId,
    pub started_by: PointerButton,
}

/// Pointer/keyboard focus router over a desktop window list.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopInputRouter {
    keyboard_focus: Option<ViewId>,
    hovered: Option<ViewId>,
    capture: Option<CaptureState>,
}

impl DesktopInputRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn keyboard_focus(&self) -> Option<ViewId> {
        self.keyboard_focus
    }

    pub const fn hovered(&self) -> Option<ViewId> {
        self.hovered
    }

    pub const fn capture(&self) -> Option<CaptureState> {
        self.capture
    }

    /// Move keyboard focus programmatically (keyboard shortcuts, launches).
    /// Returns the `FocusChanged` delivery if anything changed.
    pub fn set_keyboard_focus(&mut self, target: Option<ViewId>) -> Option<Delivery> {
        if self.keyboard_focus == target {
            return None;
        }
        let previous = self.keyboard_focus;
        self.keyboard_focus = target;
        Some(Delivery::FocusChanged {
            previous,
            current: target,
        })
    }

    /// Mark `focused` on the window that owns keyboard focus, clearing others.
    ///
    /// When the router has no focus owner the list is left as built, so a
    /// desktop can still present a default focused window.
    pub fn apply_focus(&self, windows: &mut [DesktopWindow]) {
        let Some(focus) = self.keyboard_focus else {
            return;
        };
        for window in windows.iter_mut() {
            window.focused = window.frame.view_id == focus;
        }
    }

    /// Route one pointer event against the current window list.
    pub fn route(
        &mut self,
        compositor: &Compositor,
        windows: &[DesktopWindow],
        event: PointerEvent,
    ) -> Vec<Delivery> {
        let mut out = Vec::new();

        // Windows can vanish between events; drop state that points at them.
        self.reconcile(windows, event, &mut out);

        let position = event.position;
        let hit = if position.x >= 0 && position.y >= 0 {
            compositor.hit_test(windows, position.x as usize, position.y as usize)
        } else {
            None
        };

        match event.kind {
            PointerEventKind::Capture(PointerCapture::Lost) => {
                self.end_capture(event, &mut out);
            }
            PointerEventKind::Capture(PointerCapture::Gained) => {
                // Device-level capture gain is informational for the desktop.
            }
            PointerEventKind::Move { .. } => {
                if let Some(capture) = self.capture {
                    let hit_on_target = hit.filter(|h| h.view_id == capture.target);
                    out.push(Delivery::Pointer {
                        target: capture.target,
                        event,
                        hit: hit_on_target,
                    });
                } else {
                    self.update_hover(hit, &mut out);
                    match hit {
                        Some(h) => out.push(Delivery::Pointer {
                            target: h.view_id,
                            event,
                            hit: Some(h),
                        }),
                        None => out.push(Delivery::Desktop { event }),
                    }
                }
            }
            PointerEventKind::Wheel { .. } => {
                let target = self.capture.map(|c| c.target).or(hit.map(|h| h.view_id));
                match target {
                    Some(target) => out.push(Delivery::Pointer {
                        target,
                        event,
                        hit: hit.filter(|h| h.view_id == target),
                    }),
                    None => out.push(Delivery::Desktop { event }),
                }
            }
            PointerEventKind::Button {
                button,
                state: ButtonState::Pressed,
            } => {
                if let Some(capture) = self.capture {
                    // Additional button during a capture stays with the capturer.
                    out.push(Delivery::Pointer {
                        target: capture.target,
                        event,
                        hit: hit.filter(|h| h.view_id == capture.target),
                    });
                } else {
                    self.update_hover(hit, &mut out);
                    match hit {
                        Some(h) => {
                            let role = windows[h.window_index].role;
                            if role.is_focusable() {
                                if let Some(change) = self.set_keyboard_focus(Some(h.view_id)) {
                                    out.push(change);
                                }
                            }
                            self.capture = Some(CaptureState {
                                target: h.view_id,
                                started_by: button,
                            });
                            out.push(Delivery::Capture {
                                target: h.view_id,
                                transition: PointerCapture::Gained,
                                event,
                            });
                            out.push(Delivery::Pointer {
                                target: h.view_id,
                                event,
                                hit: Some(h),
                            });
                        }
                        None => out.push(Delivery::Desktop { event }),
                    }
                }
            }
            PointerEventKind::Button {
                state: ButtonState::Released,
                ..
            } => match self.capture {
                Some(capture) => {
                    out.push(Delivery::Pointer {
                        target: capture.target,
                        event,
                        hit: hit.filter(|h| h.view_id == capture.target),
                    });
                    if event.buttons.is_empty() {
                        self.end_capture(event, &mut out);
                        // The pointer may have been released over another window.
                        self.update_hover(hit, &mut out);
                    }
                }
                None => match hit {
                    Some(h) => out.push(Delivery::Pointer {
                        target: h.view_id,
                        event,
                        hit: Some(h),
                    }),
                    None => out.push(Delivery::Desktop { event }),
                },
            },
        }

        out
    }

    fn update_hover(&mut self, hit: Option<HitTarget>, out: &mut Vec<Delivery>) {
        let now = hit.map(|h| h.view_id);
        if now == self.hovered {
            return;
        }
        if let Some(previous) = self.hovered {
            out.push(Delivery::Leave { target: previous });
        }
        if let Some(current) = now {
            out.push(Delivery::Enter { target: current });
        }
        self.hovered = now;
    }

    fn end_capture(&mut self, event: PointerEvent, out: &mut Vec<Delivery>) {
        if let Some(capture) = self.capture.take() {
            out.push(Delivery::Capture {
                target: capture.target,
                transition: PointerCapture::Lost,
                event,
            });
        }
    }

    fn reconcile(
        &mut self,
        windows: &[DesktopWindow],
        event: PointerEvent,
        out: &mut Vec<Delivery>,
    ) {
        let exists = |id: ViewId| windows.iter().any(|w| w.frame.view_id == id);
        if let Some(capture) = self.capture {
            if !exists(capture.target) {
                self.end_capture(event, out);
            }
        }
        if let Some(hovered) = self.hovered {
            if !exists(hovered) {
                out.push(Delivery::Leave { target: hovered });
                self.hovered = None;
            }
        }
        if let Some(focus) = self.keyboard_focus {
            if !exists(focus) {
                if let Some(change) = self.set_keyboard_focus(None) {
                    out.push(change);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SurfaceRect, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH};
    use alloc::string::ToString;
    use alloc::vec;
    use input_types::{PointerButtons, PointerDelta, PointerPosition};
    use view_types::{ViewContent, ViewFrame, ViewKind};

    fn window(role: DesktopWindowRole, rect: SurfaceRect) -> DesktopWindow {
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["x".to_string()]),
            0,
        );
        DesktopWindow::new(frame, rect).with_role(role)
    }

    /// Two side-by-side main windows and a status bar below them.
    fn desktop() -> Vec<DesktopWindow> {
        vec![
            window(DesktopWindowRole::Main, SurfaceRect::new(0, 0, 10, 10)),
            window(DesktopWindowRole::Main, SurfaceRect::new(10, 0, 10, 10)),
            window(DesktopWindowRole::Status, SurfaceRect::new(0, 10, 20, 3)),
        ]
    }

    fn at(cell_x: usize, cell_y: usize) -> PointerPosition {
        PointerPosition::new(
            (cell_x * RASTER_CELL_WIDTH + 3) as i32,
            (cell_y * RASTER_CELL_HEIGHT + 3) as i32,
        )
    }

    fn moved(to: PointerPosition, buttons: PointerButtons) -> PointerEvent {
        PointerEvent::moved(to, PointerDelta::new(1, 1), buttons)
    }

    fn targets(deliveries: &[Delivery]) -> Vec<&'static str> {
        deliveries
            .iter()
            .map(|d| match d {
                Delivery::Pointer { .. } => "pointer",
                Delivery::Desktop { .. } => "desktop",
                Delivery::Enter { .. } => "enter",
                Delivery::Leave { .. } => "leave",
                Delivery::Capture {
                    transition: PointerCapture::Gained,
                    ..
                } => "capture+",
                Delivery::Capture {
                    transition: PointerCapture::Lost,
                    ..
                } => "capture-",
                Delivery::FocusChanged { .. } => "focus",
            })
            .collect()
    }

    #[test]
    fn test_hover_moves_emit_enter_and_leave() {
        let compositor = Compositor::new();
        let windows = desktop();
        let (left, right) = (windows[0].frame.view_id, windows[1].frame.view_id);
        let mut router = DesktopInputRouter::new();

        let out = router.route(&compositor, &windows, moved(at(2, 2), PointerButtons::NONE));
        assert_eq!(targets(&out), vec!["enter", "pointer"]);
        assert_eq!(out[0], Delivery::Enter { target: left });
        assert_eq!(router.hovered(), Some(left));
        assert_eq!(router.keyboard_focus(), None, "hover never focuses");

        let out = router.route(
            &compositor,
            &windows,
            moved(at(12, 2), PointerButtons::NONE),
        );
        assert_eq!(targets(&out), vec!["leave", "enter", "pointer"]);
        assert_eq!(out[0], Delivery::Leave { target: left });
        assert_eq!(out[1], Delivery::Enter { target: right });

        // Same window again: just the pointer delivery with a content hit.
        let out = router.route(
            &compositor,
            &windows,
            moved(at(13, 3), PointerButtons::NONE),
        );
        assert_eq!(targets(&out), vec!["pointer"]);
        match out[0] {
            Delivery::Pointer { target, hit, .. } => {
                assert_eq!(target, right);
                assert!(hit.is_some());
            }
            other => panic!("{other:?}"),
        }

        // Off every window: leave, then a desktop delivery.
        let far = PointerPosition::new(
            (25 * RASTER_CELL_WIDTH) as i32,
            (25 * RASTER_CELL_HEIGHT) as i32,
        );
        let out = router.route(&compositor, &windows, moved(far, PointerButtons::NONE));
        assert_eq!(targets(&out), vec!["leave", "desktop"]);
        assert_eq!(router.hovered(), None);
    }

    #[test]
    fn test_click_focuses_and_captures_until_release() {
        let compositor = Compositor::new();
        let windows = desktop();
        let (left, right) = (windows[0].frame.view_id, windows[1].frame.view_id);
        let mut router = DesktopInputRouter::new();

        let press =
            PointerEvent::button_pressed(at(2, 2), PointerButton::Primary, PointerButtons::NONE);
        let out = router.route(&compositor, &windows, press);
        assert_eq!(targets(&out), vec!["enter", "focus", "capture+", "pointer"]);
        assert_eq!(
            out[1],
            Delivery::FocusChanged {
                previous: None,
                current: Some(left)
            }
        );
        assert_eq!(router.keyboard_focus(), Some(left));
        assert_eq!(
            router.capture(),
            Some(CaptureState {
                target: left,
                started_by: PointerButton::Primary
            })
        );

        // Dragging over the right window still reports to the capturer, with no hit.
        let out = router.route(
            &compositor,
            &windows,
            moved(at(12, 2), PointerButtons::PRIMARY),
        );
        assert_eq!(targets(&out), vec!["pointer"]);
        match out[0] {
            Delivery::Pointer { target, hit, .. } => {
                assert_eq!(target, left);
                assert_eq!(hit, None);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            router.hovered(),
            Some(left),
            "hover is frozen during capture"
        );

        // Release over the right window: capturer gets the release, capture
        // ends, and hover catches up to the window actually under the pointer.
        let release = PointerEvent::button_released(
            at(12, 2),
            PointerButton::Primary,
            PointerButtons::PRIMARY,
        );
        let out = router.route(&compositor, &windows, release);
        assert_eq!(targets(&out), vec!["pointer", "capture-", "leave", "enter"]);
        assert_eq!(router.capture(), None);
        assert_eq!(router.hovered(), Some(right));
        assert_eq!(
            router.keyboard_focus(),
            Some(left),
            "release does not refocus"
        );
    }

    #[test]
    fn test_capture_persists_while_any_button_is_held() {
        let compositor = Compositor::new();
        let windows = desktop();
        let left = windows[0].frame.view_id;
        let mut router = DesktopInputRouter::new();

        router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(at(2, 2), PointerButton::Primary, PointerButtons::NONE),
        );
        // Second button during capture: delivered to the capturer, no new capture.
        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(
                at(2, 2),
                PointerButton::Secondary,
                PointerButtons::PRIMARY,
            ),
        );
        assert_eq!(targets(&out), vec!["pointer"]);

        // Releasing only primary keeps capture (secondary still held).
        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::button_released(
                at(2, 2),
                PointerButton::Primary,
                PointerButtons::PRIMARY.union(PointerButtons::SECONDARY),
            ),
        );
        assert_eq!(targets(&out), vec!["pointer"]);
        assert_eq!(router.capture().map(|c| c.target), Some(left));

        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::button_released(
                at(2, 2),
                PointerButton::Secondary,
                PointerButtons::SECONDARY,
            ),
        );
        assert_eq!(targets(&out), vec!["pointer", "capture-"]);
        assert_eq!(router.capture(), None);
    }

    #[test]
    fn test_status_window_is_not_focusable_but_still_captures_clicks() {
        let compositor = Compositor::new();
        let windows = desktop();
        let (left, status) = (windows[0].frame.view_id, windows[2].frame.view_id);
        let mut router = DesktopInputRouter::new();
        router.set_keyboard_focus(Some(left));

        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(at(5, 11), PointerButton::Primary, PointerButtons::NONE),
        );
        assert_eq!(targets(&out), vec!["enter", "capture+", "pointer"]);
        assert_eq!(router.keyboard_focus(), Some(left));
        assert_eq!(router.capture().map(|c| c.target), Some(status));
        assert!(!DesktopWindowRole::Status.is_focusable());
        assert!(!DesktopWindowRole::Notification.is_focusable());
        assert!(DesktopWindowRole::Palette.is_focusable());
        assert!(DesktopWindowRole::Modal.is_focusable());
    }

    #[test]
    fn test_wheel_goes_to_hovered_or_capturing_window_without_focus_change() {
        let compositor = Compositor::new();
        let windows = desktop();
        let (left, right) = (windows[0].frame.view_id, windows[1].frame.view_id);
        let mut router = DesktopInputRouter::new();

        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::wheel(at(12, 2), 0, 1, PointerButtons::NONE),
        );
        assert_eq!(targets(&out), vec!["pointer"]);
        assert!(matches!(out[0], Delivery::Pointer { target, .. } if target == right));
        assert_eq!(router.keyboard_focus(), None);

        router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(at(2, 2), PointerButton::Primary, PointerButtons::NONE),
        );
        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::wheel(at(12, 2), 0, -1, PointerButtons::PRIMARY),
        );
        assert!(matches!(out[0], Delivery::Pointer { target, .. } if target == left));
    }

    #[test]
    fn test_vanished_window_loses_capture_focus_and_hover() {
        let compositor = Compositor::new();
        let mut windows = desktop();
        let left = windows[0].frame.view_id;
        let mut router = DesktopInputRouter::new();
        router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(at(2, 2), PointerButton::Primary, PointerButtons::NONE),
        );
        assert_eq!(router.keyboard_focus(), Some(left));

        windows.remove(0);
        let out = router.route(
            &compositor,
            &windows,
            moved(at(2, 2), PointerButtons::PRIMARY),
        );
        assert_eq!(targets(&out), vec!["capture-", "leave", "focus", "desktop"]);
        assert_eq!(
            out[2],
            Delivery::FocusChanged {
                previous: Some(left),
                current: None
            }
        );
        assert_eq!(router.capture(), None);
        assert_eq!(router.hovered(), None);
    }

    #[test]
    fn test_device_capture_lost_ends_capture_and_apply_focus_marks_windows() {
        let compositor = Compositor::new();
        let mut windows = desktop();
        let right = windows[1].frame.view_id;
        let mut router = DesktopInputRouter::new();
        router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(at(12, 2), PointerButton::Primary, PointerButtons::NONE),
        );
        let out = router.route(
            &compositor,
            &windows,
            PointerEvent::capture(at(12, 2), PointerCapture::Lost),
        );
        assert_eq!(targets(&out), vec!["capture-"]);

        windows[0].focused = true;
        router.apply_focus(&mut windows);
        assert!(!windows[0].focused);
        assert!(windows[1].focused);
        assert!(!windows[2].focused);
        assert_eq!(router.keyboard_focus(), Some(right));

        // No focus owner leaves the list untouched.
        let empty = DesktopInputRouter::new();
        windows[2].focused = true;
        empty.apply_focus(&mut windows);
        assert!(windows[2].focused);

        // Negative coordinates never hit anything.
        let mut router = DesktopInputRouter::new();
        let out = router.route(
            &compositor,
            &windows,
            moved(PointerPosition::new(-5, -5), PointerButtons::NONE),
        );
        assert_eq!(targets(&out), vec!["desktop"]);
    }
}
