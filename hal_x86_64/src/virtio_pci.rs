//! virtio-pci legacy (transitional) transport over an I/O BAR.
//!
//! QEMU's `virtio-blk-pci` is transitional by default, so its BAR0 is a
//! legacy I/O register block. Legacy virtio has one difference that matters
//! here: the whole virtqueue (descriptors, then the available ring, then the
//! used ring at the next 4 KiB boundary) lives in one physically contiguous
//! region whose page frame number is written to `QUEUE_PFN`. All accesses go
//! through `PortIo`, so the handshake is testable with `FakePortIo`.

use crate::port_io::PortIo;
use core::prelude::v1::*;

pub const LEGACY_HOST_FEATURES: u16 = 0x00;
pub const LEGACY_GUEST_FEATURES: u16 = 0x04;
pub const LEGACY_QUEUE_PFN: u16 = 0x08;
pub const LEGACY_QUEUE_NUM: u16 = 0x0C;
pub const LEGACY_QUEUE_SEL: u16 = 0x0E;
pub const LEGACY_QUEUE_NOTIFY: u16 = 0x10;
pub const LEGACY_STATUS: u16 = 0x12;
pub const LEGACY_ISR: u16 = 0x13;
/// Device-specific config starts here when MSI-X is not enabled.
pub const LEGACY_CONFIG: u16 = 0x14;

pub const PAGE_SHIFT: u32 = 12;

/// Byte layout of a legacy virtqueue of `size` entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyQueueLayout {
    pub desc_offset: usize,
    pub avail_offset: usize,
    pub used_offset: usize,
    pub total_bytes: usize,
}

impl LegacyQueueLayout {
    pub const fn for_size(size: u16) -> Self {
        let size = size as usize;
        let desc_bytes = 16 * size;
        let avail_bytes = 6 + 2 * size;
        let used_bytes = 6 + 8 * size;
        let used_offset = (desc_bytes + avail_bytes + 4095) & !4095;
        Self {
            desc_offset: 0,
            avail_offset: desc_bytes,
            used_offset,
            total_bytes: (used_offset + used_bytes + 4095) & !4095,
        }
    }
}

pub struct VirtioPciLegacy<P: PortIo> {
    io: P,
    base: u16,
}

impl<P: PortIo> VirtioPciLegacy<P> {
    pub fn new(io: P, base: u16) -> Self {
        Self { io, base }
    }

    pub fn io_base(&self) -> u16 {
        self.base
    }

    pub fn host_features(&mut self) -> u32 {
        self.io.inl(self.base + LEGACY_HOST_FEATURES)
    }

    pub fn set_guest_features(&mut self, features: u32) {
        self.io.outl(self.base + LEGACY_GUEST_FEATURES, features);
    }

    pub fn status(&mut self) -> u8 {
        self.io.inb(self.base + LEGACY_STATUS)
    }

    pub fn set_status(&mut self, status: u8) {
        self.io.outb(self.base + LEGACY_STATUS, status);
    }

    pub fn add_status(&mut self, bits: u8) {
        let current = self.status();
        self.set_status(current | bits);
    }

    pub fn select_queue(&mut self, index: u16) {
        self.io.outw(self.base + LEGACY_QUEUE_SEL, index);
    }

    /// Queue size fixed by the device (0 means the queue does not exist).
    pub fn queue_size(&mut self) -> u16 {
        self.io.inw(self.base + LEGACY_QUEUE_NUM)
    }

    /// Point the selected queue at the physical page of its memory.
    pub fn set_queue_pfn(&mut self, phys: u64) {
        self.io
            .outl(self.base + LEGACY_QUEUE_PFN, (phys >> PAGE_SHIFT) as u32);
    }

    pub fn notify(&mut self, queue: u16) {
        self.io.outw(self.base + LEGACY_QUEUE_NOTIFY, queue);
    }

    pub fn isr(&mut self) -> u8 {
        self.io.inb(self.base + LEGACY_ISR)
    }

    pub fn read_config_u32(&mut self, offset: u16) -> u32 {
        self.io.inl(self.base + LEGACY_CONFIG + offset)
    }

    pub fn read_config_u64(&mut self, offset: u16) -> u64 {
        let lo = self.read_config_u32(offset) as u64;
        let hi = self.read_config_u32(offset + 4) as u64;
        lo | (hi << 32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_io::FakePortIo;

    #[test]
    fn test_legacy_layout_places_used_ring_on_a_page() {
        let l = LegacyQueueLayout::for_size(128);
        assert_eq!(l.desc_offset, 0);
        assert_eq!(l.avail_offset, 2048);
        assert_eq!(l.used_offset, 4096, "desc+avail (2310) rounds to a page");
        assert_eq!(l.total_bytes, 8192);
        let l = LegacyQueueLayout::for_size(256);
        assert_eq!(l.avail_offset, 4096);
        assert_eq!(l.used_offset, 8192);
        assert_eq!(l.total_bytes, 12288);
    }

    #[test]
    fn test_transport_register_traffic() {
        let mut io = FakePortIo::new();
        let base = 0xC000;
        io.script_read32(base + LEGACY_HOST_FEATURES, 0x1234_5678);
        io.script_read(base + LEGACY_STATUS, 0x03);
        io.script_read16(base + LEGACY_QUEUE_NUM, 128);
        io.script_read32(base + LEGACY_CONFIG, 0x0000_0800);
        io.script_read32(base + LEGACY_CONFIG + 4, 0x0000_0001);
        let mut t = VirtioPciLegacy::new(io, base);
        assert_eq!(t.host_features(), 0x1234_5678);
        t.set_guest_features(0);
        t.add_status(0x04);
        t.select_queue(0);
        assert_eq!(t.queue_size(), 128);
        t.set_queue_pfn(0x0012_3000);
        t.notify(0);
        assert_eq!(t.read_config_u64(0), 0x1_0000_0800);

        let w = t.io.writes();
        // guest features 0 (4 bytes), status 0x07, queue sel 0 (2 bytes),
        // pfn 0x123 (4 bytes), notify 0 (2 bytes)
        assert_eq!(w[4], (base + LEGACY_STATUS, 0x07));
        assert_eq!(w[5], (base + LEGACY_QUEUE_SEL, 0));
        assert_eq!(
            &w[7..11],
            &[
                (base + 8, 0x23),
                (base + 9, 0x01),
                (base + 10, 0),
                (base + 11, 0)
            ]
        );
        assert_eq!(w[11], (base + LEGACY_QUEUE_NOTIFY, 0));
        assert_eq!(t.io.remaining_reads(), 0);
    }
}
