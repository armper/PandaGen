//! # Virtio Transport Layer
//!
//! Minimal virtio MMIO support for PandaGen.
//! Implements the core virtio transport mechanism with virtqueues.
//!
//! ## Safety
//! All unsafe code is isolated to MMIO register access and ring pointer operations.
//! Higher-level APIs are safe Rust.

use core::prelude::v1::*;

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU16, Ordering};

/// Virtio MMIO register offsets
const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
const VIRTIO_MMIO_VERSION: usize = 0x004;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;
const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;
const VIRTIO_MMIO_DEVICE_FEATURES_SEL: usize = 0x014;
const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;
const VIRTIO_MMIO_DRIVER_FEATURES_SEL: usize = 0x024;
const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;
const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
const VIRTIO_MMIO_INTERRUPT_STATUS: usize = 0x060;
const VIRTIO_MMIO_INTERRUPT_ACK: usize = 0x064;
const VIRTIO_MMIO_STATUS: usize = 0x070;
const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const VIRTIO_MMIO_QUEUE_AVAIL_LOW: usize = 0x090;
const VIRTIO_MMIO_QUEUE_AVAIL_HIGH: usize = 0x094;
const VIRTIO_MMIO_QUEUE_USED_LOW: usize = 0x0a0;
const VIRTIO_MMIO_QUEUE_USED_HIGH: usize = 0x0a4;
#[allow(dead_code)] // Reserved for future use
const VIRTIO_MMIO_CONFIG_GENERATION: usize = 0x0fc;
const VIRTIO_MMIO_CONFIG: usize = 0x100;

/// Virtio device status flags
pub const VIRTIO_STATUS_ACKNOWLEDGE: u32 = 1;
pub const VIRTIO_STATUS_DRIVER: u32 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u32 = 4;
pub const VIRTIO_STATUS_FEATURES_OK: u32 = 8;
pub const VIRTIO_STATUS_FAILED: u32 = 128;

/// Virtio device IDs
pub const VIRTIO_DEVICE_ID_BLOCK: u32 = 2;

/// Magic value for virtio MMIO devices
const VIRTIO_MMIO_MAGIC: u32 = 0x74726976; // "virt" in little-endian

/// Virtqueue descriptor flags
pub const VIRTQ_DESC_F_NEXT: u16 = 1;
pub const VIRTQ_DESC_F_WRITE: u16 = 2;

/// Maximum queue size
pub const VIRTQ_MAX_SIZE: usize = 256;

/// Virtqueue descriptor
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqDesc {
    /// Physical address of buffer
    pub addr: u64,
    /// Length of buffer
    pub len: u32,
    /// Flags (NEXT, WRITE, etc.)
    pub flags: u16,
    /// Next descriptor if flags & NEXT
    pub next: u16,
}

impl VirtqDesc {
    pub const fn new() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        }
    }
}

/// Virtqueue available ring
#[repr(C)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: AtomicU16,
    pub ring: [u16; VIRTQ_MAX_SIZE],
    pub used_event: u16, // Only if VIRTIO_F_EVENT_IDX
}

impl VirtqAvail {
    pub const fn new() -> Self {
        Self {
            flags: 0,
            idx: AtomicU16::new(0),
            ring: [0; VIRTQ_MAX_SIZE],
            used_event: 0,
        }
    }
}

/// Virtqueue used element
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

/// Virtqueue used ring
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: AtomicU16,
    pub ring: [VirtqUsedElem; VIRTQ_MAX_SIZE],
    pub avail_event: u16, // Only if VIRTIO_F_EVENT_IDX
}

impl VirtqUsed {
    pub const fn new() -> Self {
        Self {
            flags: 0,
            idx: AtomicU16::new(0),
            ring: [VirtqUsedElem { id: 0, len: 0 }; VIRTQ_MAX_SIZE],
            avail_event: 0,
        }
    }
}

/// Virtqueue - manages a single virtqueue
pub struct Virtqueue {
    /// Queue size (must be power of 2)
    pub size: u16,
    /// Descriptor table
    pub desc: &'static mut [VirtqDesc],
    /// Available ring
    pub avail: &'static mut VirtqAvail,
    /// Used ring
    pub used: &'static mut VirtqUsed,
    /// Last seen used index
    pub last_used_idx: u16,
    /// Free descriptor indices
    pub free_head: u16,
    /// Number of free descriptors
    pub num_free: u16,
}

impl Virtqueue {
    /// Create a new virtqueue from pre-allocated memory
    ///
    /// # Safety
    /// The caller must ensure:
    /// - `desc_ptr`, `avail_ptr`, and `used_ptr` point to properly aligned, valid memory
    /// - The memory regions don't overlap
    /// - The memory is exclusively owned by this virtqueue
    pub unsafe fn new(
        size: u16,
        desc_ptr: *mut VirtqDesc,
        avail_ptr: *mut VirtqAvail,
        used_ptr: *mut VirtqUsed,
    ) -> Self {
        let desc = core::slice::from_raw_parts_mut(desc_ptr, size as usize);
        let avail = &mut *avail_ptr;
        let used = &mut *used_ptr;

        // Initialize descriptor free list
        for i in 0..size {
            desc[i as usize].next = if i + 1 < size { i + 1 } else { 0 };
        }

        Self {
            size,
            desc,
            avail,
            used,
            last_used_idx: 0,
            free_head: 0,
            num_free: size,
        }
    }

    /// Allocate a descriptor chain
    pub fn alloc_desc(&mut self, count: u16) -> Option<u16> {
        if self.num_free < count {
            return None;
        }

        let head = self.free_head;
        let mut desc_idx = head;

        for _ in 0..count {
            let next = self.desc[desc_idx as usize].next;
            desc_idx = next;
        }

        self.free_head = desc_idx;
        self.num_free -= count;
        Some(head)
    }

    /// Free a descriptor chain
    pub fn free_desc(&mut self, head: u16) {
        let mut desc_idx = head;
        loop {
            let desc = &self.desc[desc_idx as usize];
            let next = desc.next;
            let has_next = (desc.flags & VIRTQ_DESC_F_NEXT) != 0;

            self.num_free += 1;

            if !has_next {
                self.desc[desc_idx as usize].next = self.free_head;
                self.free_head = head;
                break;
            }
            desc_idx = next;
        }
    }

    /// Add a buffer to the available ring
    pub fn add_to_avail(&mut self, desc_head: u16) {
        let idx = self.avail.idx.load(Ordering::Acquire);
        self.avail.ring[(idx % self.size) as usize] = desc_head;
        self.avail.idx.store(idx.wrapping_add(1), Ordering::Release);
    }

    /// Check if there are used descriptors available
    pub fn has_used(&self) -> bool {
        self.last_used_idx != self.used.idx.load(Ordering::Acquire)
    }

    /// Get the next used descriptor
    pub fn get_used(&mut self) -> Option<(u32, u32)> {
        if !self.has_used() {
            return None;
        }

        let idx = self.last_used_idx;
        let elem = self.used.ring[(idx % self.size) as usize];
        self.last_used_idx = self.last_used_idx.wrapping_add(1);

        Some((elem.id, elem.len))
    }
}

/// Virtio MMIO device
pub struct VirtioMmioDevice {
    base_addr: usize,
}

impl VirtioMmioDevice {
    /// Create a new virtio MMIO device at the given base address
    ///
    /// # Safety
    /// The caller must ensure `base_addr` points to a valid virtio MMIO device
    pub unsafe fn new(base_addr: usize) -> Option<Self> {
        let device = Self { base_addr };

        // Check magic value
        if device.read_reg(VIRTIO_MMIO_MAGIC_VALUE) != VIRTIO_MMIO_MAGIC {
            return None;
        }

        // Check version (should be 2 for modern virtio)
        let version = device.read_reg(VIRTIO_MMIO_VERSION);
        if version < 2 {
            return None;
        }

        Some(device)
    }

    /// Read a 32-bit register
    unsafe fn read_reg(&self, offset: usize) -> u32 {
        read_volatile((self.base_addr + offset) as *const u32)
    }

    /// Write a 32-bit register
    unsafe fn write_reg(&self, offset: usize, value: u32) {
        write_volatile((self.base_addr + offset) as *mut u32, value)
    }

    /// Get device ID
    pub fn device_id(&self) -> u32 {
        unsafe { self.read_reg(VIRTIO_MMIO_DEVICE_ID) }
    }

    /// Get vendor ID
    pub fn vendor_id(&self) -> u32 {
        unsafe { self.read_reg(VIRTIO_MMIO_VENDOR_ID) }
    }

    /// Get device features
    pub fn device_features(&self, select: u32) -> u32 {
        unsafe {
            self.write_reg(VIRTIO_MMIO_DEVICE_FEATURES_SEL, select);
            self.read_reg(VIRTIO_MMIO_DEVICE_FEATURES)
        }
    }

    /// Set driver features
    pub fn set_driver_features(&self, select: u32, features: u32) {
        unsafe {
            self.write_reg(VIRTIO_MMIO_DRIVER_FEATURES_SEL, select);
            self.write_reg(VIRTIO_MMIO_DRIVER_FEATURES, features);
        }
    }

    /// Get device status
    pub fn status(&self) -> u32 {
        unsafe { self.read_reg(VIRTIO_MMIO_STATUS) }
    }

    /// Set device status
    pub fn set_status(&self, status: u32) {
        unsafe { self.write_reg(VIRTIO_MMIO_STATUS, status) }
    }

    /// Add status flags
    pub fn add_status(&self, status: u32) {
        let current = self.status();
        self.set_status(current | status);
    }

    /// Select a queue
    pub fn select_queue(&self, index: u32) {
        unsafe { self.write_reg(VIRTIO_MMIO_QUEUE_SEL, index) }
    }

    /// Get maximum queue size
    pub fn queue_max_size(&self) -> u32 {
        unsafe { self.read_reg(VIRTIO_MMIO_QUEUE_NUM_MAX) }
    }

    /// Set queue size
    pub fn set_queue_size(&self, size: u32) {
        unsafe { self.write_reg(VIRTIO_MMIO_QUEUE_NUM, size) }
    }

    /// Set queue ready
    pub fn set_queue_ready(&self, ready: bool) {
        unsafe { self.write_reg(VIRTIO_MMIO_QUEUE_READY, if ready { 1 } else { 0 }) }
    }

    /// Set queue descriptor address
    pub fn set_queue_desc(&self, addr: u64) {
        unsafe {
            self.write_reg(VIRTIO_MMIO_QUEUE_DESC_LOW, addr as u32);
            self.write_reg(VIRTIO_MMIO_QUEUE_DESC_HIGH, (addr >> 32) as u32);
        }
    }

    /// Set queue available ring address
    pub fn set_queue_avail(&self, addr: u64) {
        unsafe {
            self.write_reg(VIRTIO_MMIO_QUEUE_AVAIL_LOW, addr as u32);
            self.write_reg(VIRTIO_MMIO_QUEUE_AVAIL_HIGH, (addr >> 32) as u32);
        }
    }

    /// Set queue used ring address
    pub fn set_queue_used(&self, addr: u64) {
        unsafe {
            self.write_reg(VIRTIO_MMIO_QUEUE_USED_LOW, addr as u32);
            self.write_reg(VIRTIO_MMIO_QUEUE_USED_HIGH, (addr >> 32) as u32);
        }
    }

    /// Notify device about new buffers in queue
    pub fn notify_queue(&self, queue_index: u32) {
        unsafe { self.write_reg(VIRTIO_MMIO_QUEUE_NOTIFY, queue_index) }
    }

    /// Get interrupt status
    pub fn interrupt_status(&self) -> u32 {
        unsafe { self.read_reg(VIRTIO_MMIO_INTERRUPT_STATUS) }
    }

    /// Acknowledge interrupts
    pub fn interrupt_ack(&self, status: u32) {
        unsafe { self.write_reg(VIRTIO_MMIO_INTERRUPT_ACK, status) }
    }

    /// Read device-specific config space
    pub fn read_config_u32(&self, offset: usize) -> u32 {
        unsafe { self.read_reg(VIRTIO_MMIO_CONFIG + offset) }
    }

    /// Read device-specific config space (64-bit)
    pub fn read_config_u64(&self, offset: usize) -> u64 {
        unsafe {
            let low = self.read_reg(VIRTIO_MMIO_CONFIG + offset) as u64;
            let high = self.read_reg(VIRTIO_MMIO_CONFIG + offset + 4) as u64;
            low | (high << 32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtq_desc_size() {
        // Ensure descriptor has expected layout
        assert_eq!(core::mem::size_of::<VirtqDesc>(), 16);
    }

    #[test]
    fn test_virtq_desc_new() {
        let desc = VirtqDesc::new();
        assert_eq!(desc.addr, 0);
        assert_eq!(desc.len, 0);
        assert_eq!(desc.flags, 0);
        assert_eq!(desc.next, 0);
    }

    #[test]
    fn test_status_flags() {
        assert_eq!(VIRTIO_STATUS_ACKNOWLEDGE, 1);
        assert_eq!(VIRTIO_STATUS_DRIVER, 2);
        assert_eq!(VIRTIO_STATUS_DRIVER_OK, 4);
        assert_eq!(VIRTIO_STATUS_FEATURES_OK, 8);
        assert_eq!(VIRTIO_STATUS_FAILED, 128);
    }
}
