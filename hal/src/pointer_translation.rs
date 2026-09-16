//! Pointer packet translation
//!
//! Turns relative `HalPointerPacket`s into absolute, typed `PointerEvent`s.
//! The translator owns the two pieces of state a packet stream implies but
//! never carries: the absolute position (confined to a surface) and the set
//! of buttons currently held. Everything above this layer works with
//! self-describing events and never sees deltas or raw bits.
//!
//! Translation is allocation-free: one packet expands to at most one move,
//! one wheel, and one event per button that changed.

use crate::pointer::HalPointerPacket;
use input_types::{
    Modifiers, PointerButton, PointerButtons, PointerDelta, PointerEvent, PointerPosition,
};

/// Maximum events a single packet can expand to.
pub const MAX_EVENTS_PER_PACKET: usize = 2 + 8;

/// Fixed-capacity batch of events produced from one packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEventBatch {
    events: [Option<PointerEvent>; MAX_EVENTS_PER_PACKET],
    len: usize,
}

impl PointerEventBatch {
    const fn empty() -> Self {
        Self {
            events: [None; MAX_EVENTS_PER_PACKET],
            len: 0,
        }
    }

    fn push(&mut self, event: PointerEvent) {
        if self.len < MAX_EVENTS_PER_PACKET {
            self.events[self.len] = Some(event);
            self.len += 1;
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &PointerEvent> {
        self.events[..self.len].iter().filter_map(|e| e.as_ref())
    }

    pub fn get(&self, index: usize) -> Option<&PointerEvent> {
        self.events.get(index).and_then(|e| e.as_ref())
    }
}

/// Stateful packet-to-event translator confined to a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerTranslator {
    position: PointerPosition,
    buttons: PointerButtons,
    surface_width: u32,
    surface_height: u32,
    /// Multiplies raw counts before applying; `1` is device speed.
    scale_numerator: i32,
    scale_denominator: i32,
}

/// Button bits recognised in a packet, in report order.
const PACKET_BUTTONS: [(u8, PointerButton); 5] = [
    (HalPointerPacket::BUTTON_PRIMARY, PointerButton::Primary),
    (HalPointerPacket::BUTTON_SECONDARY, PointerButton::Secondary),
    (HalPointerPacket::BUTTON_MIDDLE, PointerButton::Middle),
    (1 << 3, PointerButton::Extra(0)),
    (1 << 4, PointerButton::Extra(1)),
];

impl PointerTranslator {
    /// Translator confined to a `width` x `height` surface, starting centred.
    pub fn new(width: u32, height: u32) -> Self {
        let centre = PointerPosition::new(
            i32::try_from(width / 2).unwrap_or(i32::MAX),
            i32::try_from(height / 2).unwrap_or(i32::MAX),
        );
        Self {
            position: centre.clamp_to(width, height),
            buttons: PointerButtons::NONE,
            surface_width: width,
            surface_height: height,
            scale_numerator: 1,
            scale_denominator: 1,
        }
    }

    /// Apply a motion multiplier `numerator / denominator` (denominator > 0).
    pub fn with_scale(mut self, numerator: i32, denominator: i32) -> Self {
        if denominator > 0 {
            self.scale_numerator = numerator;
            self.scale_denominator = denominator;
        }
        self
    }

    pub const fn position(&self) -> PointerPosition {
        self.position
    }

    pub const fn buttons(&self) -> PointerButtons {
        self.buttons
    }

    /// Change the confinement surface, keeping the pointer inside it.
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_width = width;
        self.surface_height = height;
        self.position = self.position.clamp_to(width, height);
    }

    /// Move the pointer to an absolute position (clamped).
    pub fn warp(&mut self, position: PointerPosition) {
        self.position = position.clamp_to(self.surface_width, self.surface_height);
    }

    /// Expand a packet into events, applying `modifiers` to each.
    pub fn translate(
        &mut self,
        packet: HalPointerPacket,
        modifiers: Modifiers,
    ) -> PointerEventBatch {
        let mut batch = PointerEventBatch::empty();

        // Motion first so button events carry the post-motion position,
        // which is where the user sees the click land.
        if packet.has_motion() {
            let dx = i32::from(packet.dx) * self.scale_numerator / self.scale_denominator;
            // Device Y grows upward; desktop Y grows downward.
            let dy = -(i32::from(packet.dy) * self.scale_numerator / self.scale_denominator);
            let previous = self.position;
            let next = previous
                .offset(PointerDelta::new(dx, dy))
                .clamp_to(self.surface_width, self.surface_height);
            let applied = PointerDelta::new(next.x - previous.x, next.y - previous.y);
            self.position = next;
            if !applied.is_zero() {
                batch.push(
                    PointerEvent::moved(next, applied, self.buttons).with_modifiers(modifiers),
                );
            }
        }

        for (bit, button) in PACKET_BUTTONS {
            let now_down = packet.button_down(bit);
            let was_down = self.buttons.contains(button);
            if now_down && !was_down {
                let event = PointerEvent::button_pressed(self.position, button, self.buttons)
                    .with_modifiers(modifiers);
                self.buttons = event.buttons;
                batch.push(event);
            } else if !now_down && was_down {
                let event = PointerEvent::button_released(self.position, button, self.buttons)
                    .with_modifiers(modifiers);
                self.buttons = event.buttons;
                batch.push(event);
            }
        }

        if packet.wheel != 0 {
            batch.push(
                PointerEvent::wheel(self.position, 0, i32::from(packet.wheel), self.buttons)
                    .with_modifiers(modifiers),
            );
        }

        batch
    }

    /// Forget held buttons and recentre, for device reset or focus loss.
    pub fn reset(&mut self) {
        *self = Self::new(self.surface_width, self.surface_height)
            .with_scale(self.scale_numerator, self.scale_denominator);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{ButtonState, PointerEventKind};

    #[test]
    fn test_starts_centred_and_clamps_motion() {
        let mut t = PointerTranslator::new(100, 50);
        assert_eq!(t.position(), PointerPosition::new(50, 25));

        let batch = t.translate(HalPointerPacket::motion(10, 5), Modifiers::NONE);
        assert_eq!(batch.len(), 1);
        let moved = batch.get(0).unwrap();
        // Device +Y is up, desktop +Y is down.
        assert_eq!(moved.position, PointerPosition::new(60, 20));
        assert_eq!(
            moved.kind,
            PointerEventKind::Move {
                delta: PointerDelta::new(10, -5)
            }
        );

        // Large motion clamps to the surface and reports the applied delta.
        let batch = t.translate(HalPointerPacket::motion(1000, 1000), Modifiers::NONE);
        let moved = batch.get(0).unwrap();
        assert_eq!(moved.position, PointerPosition::new(99, 0));
        assert_eq!(
            moved.kind,
            PointerEventKind::Move {
                delta: PointerDelta::new(39, -20)
            }
        );

        // Pushing further into the corner produces no event at all.
        let batch = t.translate(HalPointerPacket::motion(5, 5), Modifiers::NONE);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_button_edges_become_press_and_release_with_held_set() {
        let mut t = PointerTranslator::new(10, 10);
        let down = HalPointerPacket::new(
            0,
            0,
            0,
            HalPointerPacket::BUTTON_PRIMARY | HalPointerPacket::BUTTON_MIDDLE,
        );
        let batch = t.translate(down, Modifiers::CTRL);
        assert_eq!(batch.len(), 2);
        let first = batch.get(0).unwrap();
        assert!(first.is_press(PointerButton::Primary));
        assert_eq!(first.buttons, PointerButtons::PRIMARY);
        assert!(first.modifiers.is_ctrl());
        let second = batch.get(1).unwrap();
        assert!(second.is_press(PointerButton::Middle));
        assert_eq!(
            second.buttons,
            PointerButtons::PRIMARY.union(PointerButtons::MIDDLE)
        );
        assert_eq!(t.buttons(), second.buttons);

        // Holding produces nothing new.
        assert!(t.translate(down, Modifiers::NONE).is_empty());

        // Releasing only primary.
        let batch = t.translate(
            HalPointerPacket::new(0, 0, 0, HalPointerPacket::BUTTON_MIDDLE),
            Modifiers::NONE,
        );
        assert_eq!(batch.len(), 1);
        let release = batch.get(0).unwrap();
        assert_eq!(
            release.button(),
            Some((PointerButton::Primary, ButtonState::Released))
        );
        assert_eq!(release.buttons, PointerButtons::MIDDLE);
    }

    #[test]
    fn test_motion_precedes_click_and_wheel_follows() {
        let mut t = PointerTranslator::new(100, 100);
        let batch = t.translate(
            HalPointerPacket::new(4, 0, -1, HalPointerPacket::BUTTON_SECONDARY),
            Modifiers::NONE,
        );
        assert_eq!(batch.len(), 3);
        assert!(batch.get(0).unwrap().is_move());
        assert!(batch.get(1).unwrap().is_press(PointerButton::Secondary));
        assert_eq!(batch.get(1).unwrap().position, PointerPosition::new(54, 50));
        assert_eq!(
            batch.get(2).unwrap().kind,
            PointerEventKind::Wheel { dx: 0, dy: -1 }
        );
        assert_eq!(batch.get(2).unwrap().buttons, PointerButtons::SECONDARY);
    }

    #[test]
    fn test_scale_and_resize_and_reset() {
        let mut t = PointerTranslator::new(200, 200).with_scale(3, 2);
        let batch = t.translate(HalPointerPacket::motion(4, 0), Modifiers::NONE);
        assert_eq!(
            batch.get(0).unwrap().position,
            PointerPosition::new(106, 100)
        );

        t.resize_surface(50, 50);
        assert_eq!(t.position(), PointerPosition::new(49, 49));

        t.translate(
            HalPointerPacket::new(0, 0, 0, HalPointerPacket::BUTTON_PRIMARY),
            Modifiers::NONE,
        );
        assert!(!t.buttons().is_empty());
        t.reset();
        assert!(t.buttons().is_empty());
        assert_eq!(t.position(), PointerPosition::new(25, 25));

        t.warp(PointerPosition::new(-10, 10));
        assert_eq!(t.position(), PointerPosition::new(0, 10));
    }

    #[test]
    fn test_extra_buttons_map_to_extra_variants() {
        let mut t = PointerTranslator::new(10, 10);
        let batch = t.translate(HalPointerPacket::new(0, 0, 0, 1 << 4), Modifiers::NONE);
        assert!(batch.get(0).unwrap().is_press(PointerButton::Extra(1)));
    }
}
