//! x86_64 PS/2 mouse (i8042 auxiliary device) driver
//!
//! Three layers, each testable on the host:
//!
//! - `Ps2MousePacketParser`: pure byte-stream parser for 3-byte standard and
//!   4-byte IntelliMouse packets with sync recovery. The kernel IRQ 12 path
//!   queues raw bytes and feeds this parser from the main loop.
//! - `Ps2MouseInit`: the controller/device bring-up sequence (enable the aux
//!   port, enable IRQ 12 in the command byte, set defaults, probe for a wheel,
//!   enable reporting) over any `PortIo`.
//! - `X86Ps2Mouse`: a polling `PointerDevice` for setups without interrupts.
//!
//! ## Packet format (byte 0)
//!
//! bit 0 left, bit 1 right, bit 2 middle, bit 3 always set, bit 4 X sign,
//! bit 5 Y sign, bit 6 X overflow, bit 7 Y overflow. Bytes 1 and 2 are the
//! low 8 bits of the 9-bit signed X and Y deltas. IntelliMouse adds byte 3:
//! low nibble is the signed wheel delta, bits 4 and 5 are buttons 4 and 5.

use crate::port_io::PortIo;
use core::prelude::v1::*;
use hal::pointer::{HalPointerPacket, PointerDevice};

const PS2_DATA_PORT: u16 = 0x60;
const PS2_STATUS_PORT: u16 = 0x64;

const STATUS_OBF: u8 = 0x01;
const STATUS_IBF: u8 = 0x02;
/// Output buffer holds auxiliary (mouse) data rather than keyboard data.
const STATUS_AUX_DATA: u8 = 0x20;

const CTRL_READ_COMMAND_BYTE: u8 = 0x20;
const CTRL_WRITE_COMMAND_BYTE: u8 = 0x60;
const CTRL_ENABLE_AUX: u8 = 0xA8;
const CTRL_WRITE_AUX: u8 = 0xD4;

const CMD_BYTE_AUX_IRQ: u8 = 0x02;
const CMD_BYTE_AUX_CLOCK_DISABLE: u8 = 0x20;

const MOUSE_SET_DEFAULTS: u8 = 0xF6;
const MOUSE_ENABLE_REPORTING: u8 = 0xF4;
const MOUSE_SET_SAMPLE_RATE: u8 = 0xF3;
const MOUSE_GET_ID: u8 = 0xF2;
const MOUSE_ACK: u8 = 0xFA;

const ID_INTELLIMOUSE: u8 = 0x03;

const SYNC_BIT: u8 = 0x08;
const X_SIGN: u8 = 0x10;
const Y_SIGN: u8 = 0x20;
const X_OVERFLOW: u8 = 0x40;
const Y_OVERFLOW: u8 = 0x80;

/// Bounded spin count for controller handshakes.
const WAIT_LIMIT: usize = 10_000;

/// Stateful parser for the PS/2 mouse byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ps2MousePacketParser {
    bytes: [u8; 4],
    len: usize,
    packet_len: usize,
    resyncs: u32,
}

impl Ps2MousePacketParser {
    /// `wheel` selects 4-byte IntelliMouse packets.
    pub const fn new(wheel: bool) -> Self {
        Self {
            bytes: [0; 4],
            len: 0,
            packet_len: if wheel { 4 } else { 3 },
            resyncs: 0,
        }
    }

    pub const fn packet_len(&self) -> usize {
        self.packet_len
    }

    /// Number of bytes discarded while hunting for a packet start.
    pub const fn resyncs(&self) -> u32 {
        self.resyncs
    }

    /// Feed one byte; returns a packet when one completes.
    pub fn feed(&mut self, byte: u8) -> Option<HalPointerPacket> {
        if self.len == 0 && byte & SYNC_BIT == 0 {
            // Not a valid first byte: we are mid-packet after a dropped byte.
            self.resyncs = self.resyncs.saturating_add(1);
            return None;
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        if self.len < self.packet_len {
            return None;
        }
        self.len = 0;
        Some(Self::decode(&self.bytes[..self.packet_len]))
    }

    fn decode(bytes: &[u8]) -> HalPointerPacket {
        let flags = bytes[0];
        let mut dx = i16::from(bytes[1]);
        let mut dy = i16::from(bytes[2]);
        if flags & X_SIGN != 0 {
            dx -= 256;
        }
        if flags & Y_SIGN != 0 {
            dy -= 256;
        }
        // Overflow means the device saturated; report the maximum in the
        // signalled direction instead of a wrapped small value.
        if flags & X_OVERFLOW != 0 {
            dx = if flags & X_SIGN != 0 { -255 } else { 255 };
        }
        if flags & Y_OVERFLOW != 0 {
            dy = if flags & Y_SIGN != 0 { -255 } else { 255 };
        }

        let mut buttons = flags & 0x07;
        let mut wheel = 0i8;
        if let Some(&extra) = bytes.get(3) {
            let nibble = extra & 0x0F;
            wheel = if nibble & 0x08 != 0 {
                (nibble as i8) - 16
            } else {
                nibble as i8
            };
            if extra & 0x10 != 0 {
                buttons |= 1 << 3;
            }
            if extra & 0x20 != 0 {
                buttons |= 1 << 4;
            }
        }

        HalPointerPacket::new(dx, dy, wheel, buttons)
    }

    /// Drop any partial packet (device reset, overflow recovery).
    pub fn reset(&mut self) {
        self.len = 0;
    }
}

/// Result of the bring-up sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseInitReport {
    /// Command byte after enabling IRQ 12 and the aux clock.
    pub command_byte: u8,
    /// Device ID reported after the IntelliMouse probe (3 means wheel).
    pub device_id: u8,
    pub wheel: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseInitError {
    /// Controller input buffer never drained.
    InputBufferStuck,
    /// Controller never produced the requested byte.
    NoResponse,
    /// Device answered something other than ACK to `command`.
    Nack { command: u8, response: u8 },
}

/// i8042 handshakes over a `PortIo`.
struct Ps2Controller<'a, P: PortIo> {
    io: &'a mut P,
}

impl<'a, P: PortIo> Ps2Controller<'a, P> {
    fn wait_input_clear(&mut self) -> Result<(), MouseInitError> {
        for _ in 0..WAIT_LIMIT {
            if self.io.inb(PS2_STATUS_PORT) & STATUS_IBF == 0 {
                return Ok(());
            }
        }
        Err(MouseInitError::InputBufferStuck)
    }

    fn wait_output_full(&mut self) -> Result<(), MouseInitError> {
        for _ in 0..WAIT_LIMIT {
            if self.io.inb(PS2_STATUS_PORT) & STATUS_OBF != 0 {
                return Ok(());
            }
        }
        Err(MouseInitError::NoResponse)
    }

    fn write_command(&mut self, command: u8) -> Result<(), MouseInitError> {
        self.wait_input_clear()?;
        self.io.outb(PS2_STATUS_PORT, command);
        Ok(())
    }

    fn write_data(&mut self, data: u8) -> Result<(), MouseInitError> {
        self.wait_input_clear()?;
        self.io.outb(PS2_DATA_PORT, data);
        Ok(())
    }

    fn read_data(&mut self) -> Result<u8, MouseInitError> {
        self.wait_output_full()?;
        Ok(self.io.inb(PS2_DATA_PORT))
    }

    /// Send one byte to the mouse and require an ACK.
    fn mouse_byte(&mut self, byte: u8) -> Result<(), MouseInitError> {
        self.write_command(CTRL_WRITE_AUX)?;
        self.write_data(byte)?;
        let response = self.read_data()?;
        if response == MOUSE_ACK {
            Ok(())
        } else {
            Err(MouseInitError::Nack {
                command: byte,
                response,
            })
        }
    }
}

/// PS/2 mouse bring-up.
pub struct Ps2MouseInit;

impl Ps2MouseInit {
    /// Enable the aux port and IRQ 12, set defaults, probe for a wheel, and
    /// enable data reporting. Returns what was negotiated.
    pub fn initialize<P: PortIo>(io: &mut P) -> Result<MouseInitReport, MouseInitError> {
        let mut ctrl = Ps2Controller { io };

        ctrl.write_command(CTRL_ENABLE_AUX)?;

        ctrl.write_command(CTRL_READ_COMMAND_BYTE)?;
        let mut command_byte = ctrl.read_data()?;
        command_byte |= CMD_BYTE_AUX_IRQ;
        command_byte &= !CMD_BYTE_AUX_CLOCK_DISABLE;
        ctrl.write_command(CTRL_WRITE_COMMAND_BYTE)?;
        ctrl.write_data(command_byte)?;

        ctrl.mouse_byte(MOUSE_SET_DEFAULTS)?;

        // IntelliMouse magic: sample rates 200, 100, 80 then read the ID.
        for rate in [200u8, 100, 80] {
            ctrl.mouse_byte(MOUSE_SET_SAMPLE_RATE)?;
            ctrl.mouse_byte(rate)?;
        }
        ctrl.mouse_byte(MOUSE_GET_ID)?;
        let device_id = ctrl.read_data()?;

        ctrl.mouse_byte(MOUSE_ENABLE_REPORTING)?;

        Ok(MouseInitReport {
            command_byte,
            device_id,
            wheel: device_id == ID_INTELLIMOUSE,
        })
    }
}

/// Polling PS/2 mouse for interrupt-less setups.
pub struct X86Ps2Mouse<P: PortIo> {
    port_io: P,
    parser: Ps2MousePacketParser,
}

impl<P: PortIo> X86Ps2Mouse<P> {
    pub fn new(port_io: P, wheel: bool) -> Self {
        Self {
            port_io,
            parser: Ps2MousePacketParser::new(wheel),
        }
    }

    pub fn parser(&self) -> &Ps2MousePacketParser {
        &self.parser
    }
}

impl<P: PortIo> PointerDevice for X86Ps2Mouse<P> {
    fn poll_packet(&mut self) -> Option<HalPointerPacket> {
        // Only consume bytes flagged as auxiliary data; keyboard bytes must
        // stay in the buffer for the keyboard driver.
        let status = self.port_io.inb(PS2_STATUS_PORT);
        if status & (STATUS_OBF | STATUS_AUX_DATA) != (STATUS_OBF | STATUS_AUX_DATA) {
            return None;
        }
        let byte = self.port_io.inb(PS2_DATA_PORT);
        self.parser.feed(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_io::FakePortIo;
    use alloc::vec::Vec;

    #[test]
    fn test_parser_decodes_standard_packet_with_signs() {
        let mut parser = Ps2MousePacketParser::new(false);
        assert_eq!(parser.feed(0x09), None); // sync + left button
        assert_eq!(parser.feed(0x05), None);
        let packet = parser.feed(0x03).unwrap();
        assert_eq!(packet, HalPointerPacket::new(5, 3, 0, 1));

        // Negative deltas via sign bits: dx = -2 (0xFE), dy = -1 (0xFF).
        parser.feed(0x08 | X_SIGN | Y_SIGN);
        parser.feed(0xFE);
        let packet = parser.feed(0xFF).unwrap();
        assert_eq!(packet, HalPointerPacket::new(-2, -1, 0, 0));
    }

    #[test]
    fn test_parser_saturates_on_overflow() {
        let mut parser = Ps2MousePacketParser::new(false);
        parser.feed(SYNC_BIT | X_OVERFLOW | Y_OVERFLOW | Y_SIGN);
        parser.feed(0x01);
        let packet = parser.feed(0x01).unwrap();
        assert_eq!(packet.dx, 255);
        assert_eq!(packet.dy, -255);
    }

    #[test]
    fn test_parser_resyncs_after_dropped_byte() {
        let mut parser = Ps2MousePacketParser::new(false);
        // Garbage that lacks the sync bit is discarded until a header arrives.
        assert_eq!(parser.feed(0x01), None);
        assert_eq!(parser.feed(0x02), None);
        assert_eq!(parser.resyncs(), 2);
        parser.feed(0x08);
        parser.feed(0x10);
        assert_eq!(parser.feed(0x00).unwrap().dx, 16);

        // A partial packet can be dropped explicitly.
        parser.feed(0x08);
        parser.reset();
        assert_eq!(parser.feed(0x08), None);
        assert_eq!(parser.feed(0x01), None);
        assert_eq!(
            parser.feed(0x01).unwrap(),
            HalPointerPacket::new(1, 1, 0, 0)
        );
    }

    #[test]
    fn test_parser_decodes_intellimouse_wheel_and_extra_buttons() {
        let mut parser = Ps2MousePacketParser::new(true);
        assert_eq!(parser.packet_len(), 4);
        parser.feed(0x08);
        parser.feed(0x00);
        parser.feed(0x00);
        // wheel = -1 (0xF), button 4 down.
        let packet = parser.feed(0x1F).unwrap();
        assert_eq!(packet.wheel, -1);
        assert_eq!(packet.buttons, 1 << 3);

        parser.feed(0x08);
        parser.feed(0x00);
        parser.feed(0x00);
        let packet = parser.feed(0x27).unwrap();
        assert_eq!(packet.wheel, 7);
        assert_eq!(packet.buttons, 1 << 4);
    }

    #[test]
    fn test_polling_device_ignores_keyboard_bytes() {
        let mut io = FakePortIo::new();
        // Keyboard data waiting: OBF set, AUX clear -> not consumed.
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        // Then three aux bytes.
        io.script_reads(&[
            (PS2_STATUS_PORT, STATUS_OBF | STATUS_AUX_DATA),
            (PS2_DATA_PORT, 0x08),
            (PS2_STATUS_PORT, STATUS_OBF | STATUS_AUX_DATA),
            (PS2_DATA_PORT, 0x02),
            (PS2_STATUS_PORT, STATUS_OBF | STATUS_AUX_DATA),
            (PS2_DATA_PORT, 0x00),
            (PS2_STATUS_PORT, 0x00),
        ]);
        let mut mouse = X86Ps2Mouse::new(io, false);
        assert_eq!(mouse.poll_packet(), None);
        assert_eq!(mouse.poll_packet(), None);
        assert_eq!(mouse.poll_packet(), None);
        assert_eq!(mouse.poll_packet(), Some(HalPointerPacket::motion(2, 0)));
        assert_eq!(mouse.poll_packet(), None);
    }

    /// Script the controller reads the init sequence performs, in order.
    struct InitScript {
        reads: Vec<(u16, u8)>,
    }

    impl InitScript {
        fn new() -> Self {
            Self { reads: Vec::new() }
        }
        fn input_clear(&mut self) {
            self.reads.push((PS2_STATUS_PORT, 0x00));
        }
        fn data(&mut self, value: u8) {
            self.reads
                .push((PS2_STATUS_PORT, STATUS_OBF | STATUS_AUX_DATA));
            self.reads.push((PS2_DATA_PORT, value));
        }
        /// write_command(0xD4) + write_data(byte) + ACK
        fn mouse_byte_ack(&mut self) {
            self.input_clear();
            self.input_clear();
            self.data(MOUSE_ACK);
        }
    }

    fn full_init_script(command_byte_in: u8, device_id: u8) -> InitScript {
        let mut s = InitScript::new();
        s.input_clear(); // enable aux
        s.input_clear(); // read command byte (command)
        s.data(command_byte_in); // command byte value
        s.input_clear(); // write command byte (command)
        s.input_clear(); // write command byte (data)
        s.mouse_byte_ack(); // set defaults
        for _ in 0..3 {
            s.mouse_byte_ack(); // set sample rate
            s.mouse_byte_ack(); // rate value
        }
        s.mouse_byte_ack(); // get id
        s.data(device_id); // id byte
        s.mouse_byte_ack(); // enable reporting
        s
    }

    #[test]
    fn test_init_negotiates_irq12_and_wheel() {
        let mut io = FakePortIo::new();
        io.script_reads(&full_init_script(0x61, ID_INTELLIMOUSE).reads);

        let report = Ps2MouseInit::initialize(&mut io).expect("init succeeds");
        assert_eq!(
            report,
            MouseInitReport {
                command_byte: 0x43,
                device_id: 3,
                wheel: true,
            }
        );
        assert_eq!(io.remaining_reads(), 0, "every scripted read consumed");

        let writes = io.writes();
        assert_eq!(writes[0], (PS2_STATUS_PORT, CTRL_ENABLE_AUX));
        assert_eq!(writes[1], (PS2_STATUS_PORT, CTRL_READ_COMMAND_BYTE));
        assert_eq!(writes[2], (PS2_STATUS_PORT, CTRL_WRITE_COMMAND_BYTE));
        assert_eq!(writes[3], (PS2_DATA_PORT, 0x43));
        assert_eq!(writes[4], (PS2_STATUS_PORT, CTRL_WRITE_AUX));
        assert_eq!(writes[5], (PS2_DATA_PORT, MOUSE_SET_DEFAULTS));
        let sent_to_mouse: Vec<u8> = writes
            .windows(2)
            .filter(|w| w[0] == (PS2_STATUS_PORT, CTRL_WRITE_AUX))
            .map(|w| w[1].1)
            .collect();
        assert_eq!(
            sent_to_mouse,
            alloc::vec![
                MOUSE_SET_DEFAULTS,
                MOUSE_SET_SAMPLE_RATE,
                200,
                MOUSE_SET_SAMPLE_RATE,
                100,
                MOUSE_SET_SAMPLE_RATE,
                80,
                MOUSE_GET_ID,
                MOUSE_ENABLE_REPORTING
            ]
        );
    }

    #[test]
    fn test_init_reports_plain_mouse_without_wheel() {
        let mut io = FakePortIo::new();
        io.script_reads(&full_init_script(0x00, 0x00).reads);
        let report = Ps2MouseInit::initialize(&mut io).unwrap();
        assert!(!report.wheel);
        assert_eq!(report.command_byte, CMD_BYTE_AUX_IRQ);
    }

    #[test]
    fn test_init_fails_on_nack_and_stuck_controller() {
        let mut io = FakePortIo::new();
        let mut s = InitScript::new();
        s.input_clear();
        s.input_clear();
        s.data(0x61);
        s.input_clear();
        s.input_clear();
        // Set defaults answered with RESEND (0xFE) instead of ACK.
        s.input_clear();
        s.input_clear();
        s.data(0xFE);
        io.script_reads(&s.reads);
        assert_eq!(
            Ps2MouseInit::initialize(&mut io),
            Err(MouseInitError::Nack {
                command: MOUSE_SET_DEFAULTS,
                response: 0xFE,
            })
        );

        let mut io = FakePortIo::new();
        for _ in 0..WAIT_LIMIT {
            io.script_read(PS2_STATUS_PORT, STATUS_IBF);
        }
        assert_eq!(
            Ps2MouseInit::initialize(&mut io),
            Err(MouseInitError::InputBufferStuck)
        );
    }
}
