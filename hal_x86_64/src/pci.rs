//! PCI configuration space access (legacy 0xCF8/0xCFC mechanism).
//!
//! Enough to find devices on bus 0 of a QEMU `pc` machine and read their
//! BARs: the kernel needs this to reach `virtio-blk-pci`. All access goes
//! through `PortIo`, so enumeration is testable with `FakePortIo`.

use crate::port_io::PortIo;
use core::prelude::v1::*;

pub const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
pub const PCI_CONFIG_DATA: u16 = 0xCFC;

pub const PCI_VENDOR_VIRTIO: u16 = 0x1AF4;
/// Transitional virtio-blk device id (legacy interface available).
pub const PCI_DEVICE_VIRTIO_BLK_TRANSITIONAL: u16 = 0x1001;
/// Modern-only virtio-blk device id (no legacy I/O BAR).
pub const PCI_DEVICE_VIRTIO_BLK_MODERN: u16 = 0x1042;
/// Transitional virtio-net device id.
pub const PCI_DEVICE_VIRTIO_NET_TRANSITIONAL: u16 = 0x1000;
/// Modern (virtio 1.0) virtio-net device id.
pub const PCI_DEVICE_VIRTIO_NET_MODERN: u16 = 0x1041;

const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self {
            bus,
            device,
            function,
        }
    }

    /// Config-address register value for `offset` (dword aligned).
    pub const fn config_address(self, offset: u8) -> u32 {
        0x8000_0000
            | (self.bus as u32) << 16
            | (self.device as u32 & 0x1F) << 11
            | (self.function as u32 & 0x07) << 8
            | (offset as u32 & 0xFC)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDeviceInfo {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub header_type: u8,
    /// Raw BAR0.
    pub bar0: u32,
}

impl PciDeviceInfo {
    /// I/O port base if BAR0 is an I/O BAR.
    pub const fn io_base(&self) -> Option<u16> {
        if self.bar0 & 1 == 1 {
            Some((self.bar0 & 0xFFFC) as u16)
        } else {
            None
        }
    }

    pub const fn is_virtio_blk(&self) -> bool {
        self.vendor_id == PCI_VENDOR_VIRTIO
            && (self.device_id == PCI_DEVICE_VIRTIO_BLK_TRANSITIONAL
                || self.device_id == PCI_DEVICE_VIRTIO_BLK_MODERN)
    }

    pub const fn is_virtio_net(&self) -> bool {
        self.vendor_id == PCI_VENDOR_VIRTIO
            && (self.device_id == PCI_DEVICE_VIRTIO_NET_TRANSITIONAL
                || self.device_id == PCI_DEVICE_VIRTIO_NET_MODERN)
    }
}

pub fn config_read32(io: &mut impl PortIo, address: PciAddress, offset: u8) -> u32 {
    io.outl(PCI_CONFIG_ADDRESS, address.config_address(offset));
    io.inl(PCI_CONFIG_DATA)
}

pub fn config_write32(io: &mut impl PortIo, address: PciAddress, offset: u8, value: u32) {
    io.outl(PCI_CONFIG_ADDRESS, address.config_address(offset));
    io.outl(PCI_CONFIG_DATA, value);
}

/// Read one function's header, or `None` if no device answers.
pub fn probe(io: &mut impl PortIo, address: PciAddress) -> Option<PciDeviceInfo> {
    let id = config_read32(io, address, 0x00);
    let vendor_id = (id & 0xFFFF) as u16;
    if vendor_id == 0xFFFF {
        return None;
    }
    let class = config_read32(io, address, 0x08);
    let header = config_read32(io, address, 0x0C);
    let bar0 = config_read32(io, address, 0x10);
    Some(PciDeviceInfo {
        address,
        vendor_id,
        device_id: (id >> 16) as u16,
        class_code: (class >> 24) as u8,
        subclass: (class >> 16) as u8,
        header_type: (header >> 16) as u8,
        bar0,
    })
}

/// Header type bit 7: this device has more functions than function 0.
const HEADER_MULTIFUNCTION: u8 = 0x80;
/// Header type 1 (low bits): a PCI-to-PCI bridge.
const HEADER_TYPE_BRIDGE: u8 = 0x01;
/// How many buses deep the search will go. Guards against a malformed or
/// hostile bridge configuration that points back at a bus already visited.
const MAX_BUS_DEPTH: usize = 8;

/// Enumerate every function of every device slot on `bus`.
///
/// This used to probe function 0 alone, which misses the other seven
/// functions of every multifunction device.
pub fn enumerate_bus(io: &mut impl PortIo, bus: u8, mut visit: impl FnMut(PciDeviceInfo)) {
    for device in 0..32u8 {
        let Some(first) = probe(io, PciAddress::new(bus, device, 0)) else {
            continue;
        };
        let multifunction = first.header_type & HEADER_MULTIFUNCTION != 0;
        visit(first);
        if !multifunction {
            continue;
        }
        for function in 1..8u8 {
            if let Some(info) = probe(io, PciAddress::new(bus, device, function)) {
                visit(info);
            }
        }
    }
}

/// Walk bus 0 and every bus reachable through a bridge, calling `visit` for
/// each function found.
///
/// Enumeration used to stop at bus 0, so a device behind a PCIe root port --
/// the ordinary topology on `-machine q35` -- was invisible. Storage then
/// fell back to the 32-block RAM disk and there was no network at all, with
/// only "no virtio-net device" to explain it.
pub fn enumerate_all(io: &mut impl PortIo, mut visit: impl FnMut(PciDeviceInfo)) {
    let mut pending = [0u8; MAX_BUS_DEPTH];
    let mut seen = [false; 256];
    let mut len = 1usize;
    pending[0] = 0;
    seen[0] = true;

    let mut index = 0usize;
    while index < len {
        let bus = pending[index];
        index += 1;
        for device in 0..32u8 {
            let Some(first) = probe(io, PciAddress::new(bus, device, 0)) else {
                continue;
            };
            let functions = if first.header_type & HEADER_MULTIFUNCTION != 0 {
                8
            } else {
                1
            };
            for function in 0..functions {
                let Some(info) = probe(io, PciAddress::new(bus, device, function)) else {
                    continue;
                };
                if info.header_type & 0x7F == HEADER_TYPE_BRIDGE {
                    // Secondary bus number is byte 1 of register 0x18.
                    let secondary = ((config_read32(io, info.address, 0x18) >> 8) & 0xFF) as u8;
                    if !seen[secondary as usize] && len < MAX_BUS_DEPTH {
                        seen[secondary as usize] = true;
                        pending[len] = secondary;
                        len += 1;
                    }
                }
                visit(info);
            }
        }
    }
}

/// First function anywhere matching `wanted`; stops probing at the first
/// match, so a device in slot 4 costs five probes and not thirty-two.
pub fn find_device(
    io: &mut impl PortIo,
    wanted: impl Fn(&PciDeviceInfo) -> bool,
) -> Option<PciDeviceInfo> {
    let mut pending = [0u8; MAX_BUS_DEPTH];
    let mut seen = [false; 256];
    let mut len = 1usize;
    pending[0] = 0;
    seen[0] = true;

    let mut index = 0usize;
    while index < len {
        let bus = pending[index];
        index += 1;
        for device in 0..32u8 {
            let Some(first) = probe(io, PciAddress::new(bus, device, 0)) else {
                continue;
            };
            let functions = if first.header_type & HEADER_MULTIFUNCTION != 0 {
                8
            } else {
                1
            };
            for function in 0..functions {
                let info = if function == 0 {
                    first
                } else {
                    match probe(io, PciAddress::new(bus, device, function)) {
                        Some(info) => info,
                        None => continue,
                    }
                };
                if wanted(&info) {
                    return Some(info);
                }
                if info.header_type & 0x7F == HEADER_TYPE_BRIDGE {
                    let secondary = ((config_read32(io, info.address, 0x18) >> 8) & 0xFF) as u8;
                    if !seen[secondary as usize] && len < MAX_BUS_DEPTH {
                        seen[secondary as usize] = true;
                        pending[len] = secondary;
                        len += 1;
                    }
                }
            }
        }
    }
    None
}

/// First virtio-blk function anywhere on the machine.
pub fn find_virtio_blk(io: &mut impl PortIo) -> Option<PciDeviceInfo> {
    find_device(io, PciDeviceInfo::is_virtio_blk)
}

/// First virtio-net function anywhere on the machine.
pub fn find_virtio_net(io: &mut impl PortIo) -> Option<PciDeviceInfo> {
    find_device(io, PciDeviceInfo::is_virtio_net)
}

/// Enable I/O space decoding and bus mastering (the device must DMA the
/// virtqueues and data buffers).
pub fn enable_io_and_bus_master(io: &mut impl PortIo, address: PciAddress) {
    let command_status = config_read32(io, address, 0x04);
    let command = (command_status & 0xFFFF) as u16 | PCI_COMMAND_IO_SPACE | PCI_COMMAND_BUS_MASTER;
    config_write32(
        io,
        address,
        0x04,
        (command_status & 0xFFFF_0000) | command as u32,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_io::FakePortIo;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn test_config_address_encoding() {
        let a = PciAddress::new(0, 4, 0).config_address(0x10);
        assert_eq!(a, 0x8000_2010);
        let b = PciAddress::new(1, 31, 7).config_address(0x0D);
        assert_eq!(b, 0x8001_FF0C, "offset dword aligned");
    }

    #[test]
    fn test_probe_and_find_virtio_blk() {
        let mut io = FakePortIo::new();
        // Slot 0..3: empty; slot 4: virtio-blk transitional with I/O BAR0.
        for _ in 0..4 {
            io.script_read32(PCI_CONFIG_DATA, 0xFFFF_FFFF);
        }
        io.script_read32(PCI_CONFIG_DATA, (0x1001u32 << 16) | 0x1AF4); // id
        io.script_read32(PCI_CONFIG_DATA, 0x0100_0000); // class: mass storage
        io.script_read32(PCI_CONFIG_DATA, 0x0000_0000); // header type 0
        io.script_read32(PCI_CONFIG_DATA, 0xC001); // BAR0: I/O at 0xC000
        let info = find_virtio_blk(&mut io).expect("device found");
        assert_eq!(info.address, PciAddress::new(0, 4, 0));
        assert!(info.is_virtio_blk());
        assert_eq!(info.io_base(), Some(0xC000));
        assert_eq!(info.class_code, 0x01);
        // Every probe wrote the config address first.
        assert_eq!(
            io.writes()[0..4].iter().map(|w| w.0).collect::<Vec<_>>(),
            vec![0xCF8, 0xCF9, 0xCFA, 0xCFB]
        );
    }

    /// One probe's four config reads: id, class, header, BAR0.
    fn script_function(io: &mut FakePortIo, id: u32, class: u32, header: u32, bar0: u32) {
        io.script_read32(PCI_CONFIG_DATA, id);
        if id & 0xFFFF == 0xFFFF {
            return;
        }
        io.script_read32(PCI_CONFIG_DATA, class);
        io.script_read32(PCI_CONFIG_DATA, header);
        io.script_read32(PCI_CONFIG_DATA, bar0);
    }

    #[test]
    fn a_device_behind_a_bridge_is_found() {
        // Enumeration stopped at bus 0, function 0. A virtio device behind a
        // PCIe root port -- the ordinary topology on `-machine q35` -- was
        // invisible: storage fell back to the 32-block RAM disk and there was
        // no network, with only "no virtio-net device" to explain it.
        let mut io = FakePortIo::new();
        // Bus 0, slot 0: a PCI-to-PCI bridge (header type 1) to bus 1.
        script_function(
            &mut io,
            (0x0001u32 << 16) | 0x8086,
            0x0604_0000,
            0x0001_0000,
            0,
        );
        io.script_read32(PCI_CONFIG_DATA, 0x0000_0100); // 0x18: secondary bus 1
                                                        // Bus 0, slots 1..31: empty.
        for _ in 1..32 {
            script_function(&mut io, 0xFFFF_FFFF, 0, 0, 0);
        }
        // Bus 1, slot 0: the virtio-blk device.
        script_function(
            &mut io,
            (0x1001u32 << 16) | 0x1AF4,
            0x0100_0000,
            0x0000_0000,
            0xC001,
        );

        let info = find_virtio_blk(&mut io).expect("the device behind the bridge must be found");
        assert_eq!(info.address, PciAddress::new(1, 0, 0));
        assert_eq!(info.io_base(), Some(0xC000));
    }

    #[test]
    fn functions_past_the_first_of_a_multifunction_device_are_probed() {
        // Only function 0 of each slot was ever probed, so seven eighths of
        // every multifunction device went unseen.
        let mut io = FakePortIo::new();
        // Bus 0, slot 0, function 0: multifunction (header bit 7), not what
        // we want.
        script_function(
            &mut io,
            (0x0001u32 << 16) | 0x8086,
            0x0600_0000,
            0x0080_0000,
            0,
        );
        // Function 1: the virtio-net device.
        script_function(
            &mut io,
            (0x1000u32 << 16) | 0x1AF4,
            0x0200_0000,
            0x0000_0000,
            0xC041,
        );

        let info = find_virtio_net(&mut io).expect("function 1 must be probed");
        assert_eq!(info.address, PciAddress::new(0, 0, 1));
        assert_eq!(info.io_base(), Some(0xC040));
    }

    #[test]
    fn test_enable_bus_master_preserves_status_bits() {
        let mut io = FakePortIo::new();
        io.script_read32(PCI_CONFIG_DATA, 0x0010_0000); // status=0x0010, command=0
        enable_io_and_bus_master(&mut io, PciAddress::new(0, 4, 0));
        assert_eq!(io.last_write32(PCI_CONFIG_DATA), Some(0x0010_0005));
    }

    #[test]
    fn test_memory_bar_is_not_io() {
        let info = PciDeviceInfo {
            address: PciAddress::new(0, 0, 0),
            vendor_id: 0x1AF4,
            device_id: 0x1042,
            class_code: 1,
            subclass: 0,
            header_type: 0,
            bar0: 0xFEB0_0000,
        };
        assert_eq!(info.io_base(), None);
        assert!(info.is_virtio_blk());
    }
}
