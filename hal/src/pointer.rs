//! Pointer device abstraction
//!
//! Hardware pointing devices (PS/2 mouse, USB HID mouse, trackpad) deliver
//! small relative-motion packets. This module defines the packet the HAL
//! hands upward and the trait an architecture driver implements.
//!
//! ## Philosophy
//!
//! - **Hardware is just a source**: a packet is motion and button bits, not authority
//! - **Relative at the edge**: absolute position is desktop policy (see `pointer_translation`)
//! - **Poll-based**: the driver drains whatever the interrupt path queued
//! - **Testable**: any `PointerDevice` can be faked with a packet list

/// Raw pointer packet from a device.
///
/// Motion is in device counts (a PS/2 mouse reports 1 count per mickey).
/// `buttons` uses the conventional bit layout: bit 0 primary, bit 1
/// secondary, bit 2 middle, bits 3.. extra buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HalPointerPacket {
    pub dx: i16,
    pub dy: i16,
    /// Wheel notches; positive is away from the user.
    pub wheel: i8,
    pub buttons: u8,
}

impl HalPointerPacket {
    pub const BUTTON_PRIMARY: u8 = 1 << 0;
    pub const BUTTON_SECONDARY: u8 = 1 << 1;
    pub const BUTTON_MIDDLE: u8 = 1 << 2;

    pub const fn new(dx: i16, dy: i16, wheel: i8, buttons: u8) -> Self {
        Self {
            dx,
            dy,
            wheel,
            buttons,
        }
    }

    pub const fn motion(dx: i16, dy: i16) -> Self {
        Self::new(dx, dy, 0, 0)
    }

    pub const fn has_motion(&self) -> bool {
        self.dx != 0 || self.dy != 0
    }

    pub const fn button_down(&self, bit: u8) -> bool {
        self.buttons & bit != 0
    }
}

/// A pointing device that yields packets when polled.
pub trait PointerDevice {
    /// Returns the next packet, or `None` when the device has nothing queued.
    fn poll_packet(&mut self) -> Option<HalPointerPacket>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_helpers() {
        let packet = HalPointerPacket::new(3, -2, 1, HalPointerPacket::BUTTON_PRIMARY);
        assert!(packet.has_motion());
        assert!(packet.button_down(HalPointerPacket::BUTTON_PRIMARY));
        assert!(!packet.button_down(HalPointerPacket::BUTTON_MIDDLE));
        assert!(!HalPointerPacket::default().has_motion());
        assert_eq!(HalPointerPacket::motion(1, 1).buttons, 0);
    }

    struct FakeMouse(Option<HalPointerPacket>);

    impl PointerDevice for FakeMouse {
        fn poll_packet(&mut self) -> Option<HalPointerPacket> {
            self.0.take()
        }
    }

    #[test]
    fn test_fake_device_drains_once() {
        let mut mouse = FakeMouse(Some(HalPointerPacket::motion(1, 0)));
        assert_eq!(mouse.poll_packet(), Some(HalPointerPacket::motion(1, 0)));
        assert_eq!(mouse.poll_packet(), None);
    }
}
