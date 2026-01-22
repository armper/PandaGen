//! # Virtio Block Device
//!
//! Implements the `BlockDevice` trait for virtio-blk devices.
//! Provides persistent storage via QEMU's virtio-blk backend.
//!
//! ## Architecture
//! - virtio-blk sector size: 512 bytes
//! - PandaGen block size: 4096 bytes (8 sectors)
//! - Single virtqueue for requests
//! - Polling-based completion (no interrupts required)

use core::prelude::v1::*;

use super::virtio::{
    VirtioMmioDevice, VirtqDesc, Virtqueue, VIRTIO_DEVICE_ID_BLOCK, VIRTIO_STATUS_ACKNOWLEDGE,
    VIRTIO_STATUS_DRIVER, VIRTIO_STATUS_DRIVER_OK, VIRTIO_STATUS_FEATURES_OK, VIRTQ_DESC_F_NEXT,
    VIRTQ_DESC_F_WRITE, VIRTQ_MAX_SIZE,
};
use hal::{BlockDevice, BlockError, BLOCK_SIZE};

/// Virtio-blk request types
const VIRTIO_BLK_T_IN: u32 = 0; // Read
const VIRTIO_BLK_T_OUT: u32 = 1; // Write
const VIRTIO_BLK_T_FLUSH: u32 = 4; // Flush

/// Virtio-blk status codes
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

/// Virtio-blk sector size (standard)
const SECTOR_SIZE: usize = 512;

/// Sectors per PandaGen block
const SECTORS_PER_BLOCK: u64 = (BLOCK_SIZE / SECTOR_SIZE) as u64;

/// Maximum operations before considering timeout
const MAX_POLL_ITERATIONS: u32 = 1_000_000;

/// Virtio-blk request header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtioBlkReqHeader {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

/// Virtio-blk device
pub struct VirtioBlkDevice {
    device: VirtioMmioDevice,
    queue: Virtqueue,
    capacity_sectors: u64,
    /// Pre-allocated request header buffer
    req_header: [u8; 16],
    /// Pre-allocated status byte buffer
    status_byte: [u8; 1],
}

impl VirtioBlkDevice {
    /// Create a new virtio-blk device
    ///
    /// # Safety
    /// The caller must ensure:
    /// - `base_addr` points to a valid virtio-blk MMIO device
    /// - Memory regions for virtqueue structures are valid and exclusive
    ///
    /// # Arguments
    /// * `base_addr` - Base address of virtio MMIO device
    /// * `desc_ptr` - Descriptor table pointer
    /// * `avail_ptr` - Available ring pointer
    /// * `used_ptr` - Used ring pointer
    pub unsafe fn new(
        base_addr: usize,
        desc_ptr: *mut VirtqDesc,
        avail_ptr: *mut super::virtio::VirtqAvail,
        used_ptr: *mut super::virtio::VirtqUsed,
    ) -> Result<Self, BlockError> {
        // Create and validate device
        let device = VirtioMmioDevice::new(base_addr).ok_or(BlockError::NotReady)?;

        if device.device_id() != VIRTIO_DEVICE_ID_BLOCK {
            return Err(BlockError::NotReady);
        }

        // Initialize device
        device.set_status(0); // Reset
        device.add_status(VIRTIO_STATUS_ACKNOWLEDGE);
        device.add_status(VIRTIO_STATUS_DRIVER);

        // Feature negotiation (minimal - accept defaults)
        device.set_driver_features(0, 0);
        device.add_status(VIRTIO_STATUS_FEATURES_OK);

        // Check if features are accepted
        if (device.status() & VIRTIO_STATUS_FEATURES_OK) == 0 {
            return Err(BlockError::NotReady);
        }

        // Setup virtqueue
        device.select_queue(0);
        let queue_size = device.queue_max_size().min(VIRTQ_MAX_SIZE as u32) as u16;
        device.set_queue_size(queue_size as u32);

        let queue = Virtqueue::new(queue_size, desc_ptr, avail_ptr, used_ptr);

        // Set queue addresses (physical addresses)
        let desc_addr = desc_ptr as u64;
        let avail_addr = avail_ptr as u64;
        let used_addr = used_ptr as u64;

        device.set_queue_desc(desc_addr);
        device.set_queue_avail(avail_addr);
        device.set_queue_used(used_addr);
        device.set_queue_ready(true);

        // Mark driver as ready
        device.add_status(VIRTIO_STATUS_DRIVER_OK);

        // Read capacity from config space
        let capacity_sectors = device.read_config_u64(0);

        Ok(Self {
            device,
            queue,
            capacity_sectors,
            req_header: [0; 16],
            status_byte: [0; 1],
        })
    }

    /// Perform a block I/O operation
    fn do_io(
        &mut self,
        req_type: u32,
        sector: u64,
        buffer: &mut [u8],
        buffer_len: usize,
        is_write: bool,
    ) -> Result<(), BlockError> {
        // Allocate descriptor chain: header + data + status
        let desc_head = self.queue.alloc_desc(3).ok_or(BlockError::IoError)?;

        // Setup request header
        let header = VirtioBlkReqHeader {
            req_type,
            _reserved: 0,
            sector,
        };

        // Copy header to buffer
        unsafe {
            let header_ptr = &header as *const VirtioBlkReqHeader as *const u8;
            core::ptr::copy_nonoverlapping(header_ptr, self.req_header.as_mut_ptr(), 16);
        }

        // Setup descriptor chain
        let desc_data = desc_head + 1;
        let desc_status = desc_head + 2;

        // Header descriptor (device reads)
        self.queue.desc[desc_head as usize] = VirtqDesc {
            addr: self.req_header.as_ptr() as u64,
            len: 16,
            flags: VIRTQ_DESC_F_NEXT,
            next: desc_data,
        };

        // Data descriptor
        self.queue.desc[desc_data as usize] = VirtqDesc {
            addr: buffer.as_ptr() as u64,
            len: buffer_len as u32,
            flags: VIRTQ_DESC_F_NEXT | if is_write { 0 } else { VIRTQ_DESC_F_WRITE },
            next: desc_status,
        };

        // Status descriptor (device writes)
        self.queue.desc[desc_status as usize] = VirtqDesc {
            addr: self.status_byte.as_ptr() as u64,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };

        // Add to available ring and notify
        self.queue.add_to_avail(desc_head);
        self.device.notify_queue(0);

        // Poll for completion with timeout
        let mut iterations = 0;
        while !self.queue.has_used() {
            iterations += 1;
            if iterations > MAX_POLL_ITERATIONS {
                return Err(BlockError::IoError);
            }
            core::hint::spin_loop();
        }

        // Get result
        let (used_id, _used_len) = self.queue.get_used().ok_or(BlockError::IoError)?;
        if used_id != desc_head as u32 {
            return Err(BlockError::IoError);
        }

        // Free descriptor chain
        self.queue.free_desc(desc_head);

        // Check status
        match self.status_byte[0] {
            VIRTIO_BLK_S_OK => Ok(()),
            VIRTIO_BLK_S_IOERR => Err(BlockError::IoError),
            VIRTIO_BLK_S_UNSUPP => Err(BlockError::IoError),
            _ => Err(BlockError::IoError),
        }
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn block_count(&self) -> u64 {
        self.capacity_sectors / SECTORS_PER_BLOCK
    }

    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count() {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }

        let sector = block_idx * SECTORS_PER_BLOCK;
        self.do_io(VIRTIO_BLK_T_IN, sector, buffer, BLOCK_SIZE, false)
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count() {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }

        let sector = block_idx * SECTORS_PER_BLOCK;

        // Need to cast to mutable for do_io, but we won't actually modify for writes
        let buffer_mut =
            unsafe { core::slice::from_raw_parts_mut(buffer.as_ptr() as *mut u8, buffer.len()) };

        self.do_io(VIRTIO_BLK_T_OUT, sector, buffer_mut, BLOCK_SIZE, true)
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        // Allocate descriptor chain: header + status
        let desc_head = self.queue.alloc_desc(2).ok_or(BlockError::IoError)?;

        // Setup flush request header
        let header = VirtioBlkReqHeader {
            req_type: VIRTIO_BLK_T_FLUSH,
            _reserved: 0,
            sector: 0,
        };

        unsafe {
            let header_ptr = &header as *const VirtioBlkReqHeader as *const u8;
            core::ptr::copy_nonoverlapping(header_ptr, self.req_header.as_mut_ptr(), 16);
        }

        let desc_status = desc_head + 1;

        // Header descriptor
        self.queue.desc[desc_head as usize] = VirtqDesc {
            addr: self.req_header.as_ptr() as u64,
            len: 16,
            flags: VIRTQ_DESC_F_NEXT,
            next: desc_status,
        };

        // Status descriptor
        self.queue.desc[desc_status as usize] = VirtqDesc {
            addr: self.status_byte.as_ptr() as u64,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };

        // Add to available ring and notify
        self.queue.add_to_avail(desc_head);
        self.device.notify_queue(0);

        // Poll for completion
        let mut iterations = 0;
        while !self.queue.has_used() {
            iterations += 1;
            if iterations > MAX_POLL_ITERATIONS {
                return Err(BlockError::IoError);
            }
            core::hint::spin_loop();
        }

        // Get result
        let (used_id, _) = self.queue.get_used().ok_or(BlockError::IoError)?;
        if used_id != desc_head as u32 {
            return Err(BlockError::IoError);
        }

        // Free descriptor chain
        self.queue.free_desc(desc_head);

        // Check status
        match self.status_byte[0] {
            VIRTIO_BLK_S_OK => Ok(()),
            _ => Err(BlockError::IoError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sector_calculation() {
        assert_eq!(SECTORS_PER_BLOCK, 8);
        assert_eq!(SECTOR_SIZE * SECTORS_PER_BLOCK as usize, BLOCK_SIZE);
    }

    #[test]
    fn test_request_header_size() {
        assert_eq!(core::mem::size_of::<VirtioBlkReqHeader>(), 16);
    }

    #[test]
    fn test_constants() {
        assert_eq!(VIRTIO_BLK_T_IN, 0);
        assert_eq!(VIRTIO_BLK_T_OUT, 1);
        assert_eq!(VIRTIO_BLK_T_FLUSH, 4);
        assert_eq!(VIRTIO_BLK_S_OK, 0);
        assert_eq!(VIRTIO_BLK_S_IOERR, 1);
        assert_eq!(VIRTIO_BLK_S_UNSUPP, 2);
    }
}
