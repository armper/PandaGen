//! virtio-blk driver over any `VirtioTransport`.
//!
//! The driver never hands the device a virtual address. The caller supplies
//! `QueueMemory` (virtqueue pointers plus their physical addresses) and
//! `DmaBuffers` (a request header, a status byte, and one block-sized data
//! buffer, all physically addressed). Reads and writes bounce through the
//! data buffer, so any caller slice works regardless of where it lives.
//! I/O is polled: the kernel has no virtio interrupt path yet, and a block
//! request on QEMU completes in microseconds.

use core::prelude::v1::*;

use super::virtio::{
    QueuePlacement, VirtioTransport, VirtqAvail, VirtqDesc, VirtqUsed, Virtqueue,
    VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE, VIRTQ_MAX_SIZE,
};
use hal::{BlockDevice, BlockError, BLOCK_SIZE};

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;

const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

const SECTOR_SIZE: usize = 512;
const SECTORS_PER_BLOCK: u64 = (BLOCK_SIZE / SECTOR_SIZE) as u64;
const MAX_POLL_ITERATIONS: u32 = 4_000_000;

/// Virtqueue memory with its physical addresses.
#[derive(Debug, Clone, Copy)]
pub struct QueueMemory {
    pub desc: *mut VirtqDesc,
    pub avail: *mut VirtqAvail,
    pub used: *mut VirtqUsed,
    pub placement: QueuePlacement,
}

/// DMA-visible scratch buffers with their physical addresses.
#[derive(Debug, Clone, Copy)]
pub struct DmaBuffers {
    pub header: *mut [u8; 16],
    pub header_phys: u64,
    pub status: *mut u8,
    pub status_phys: u64,
    /// At least `BLOCK_SIZE` bytes.
    pub data: *mut u8,
    pub data_phys: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtioBlkReqHeader {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

pub struct VirtioBlkDevice<T: VirtioTransport> {
    transport: T,
    queue: Virtqueue,
    dma: DmaBuffers,
    capacity_sectors: u64,
}

impl<T: VirtioTransport> VirtioBlkDevice<T> {
    /// Bring the device up on queue 0.
    ///
    /// # Safety
    /// `queue` and `dma` must point at memory that stays valid and mapped
    /// for the life of the device, with correct physical addresses.
    pub unsafe fn new(
        mut transport: T,
        queue: QueueMemory,
        dma: DmaBuffers,
    ) -> Result<Self, BlockError> {
        transport.begin();
        if !transport.negotiate_no_features() {
            return Err(BlockError::NotReady);
        }
        let wanted = transport.queue_max_size(0).min(VIRTQ_MAX_SIZE as u16);
        if wanted == 0 {
            return Err(BlockError::NotReady);
        }
        let size = transport
            .setup_queue(0, wanted, queue.placement)
            .ok_or(BlockError::NotReady)?;
        if size as usize > VIRTQ_MAX_SIZE {
            return Err(BlockError::NotReady);
        }
        let queue = Virtqueue::new(size, queue.desc, queue.avail, queue.used);
        transport.driver_ok();
        let capacity_sectors = transport.read_config_u64(0);
        Ok(Self {
            transport,
            queue,
            dma,
            capacity_sectors,
        })
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors
    }

    fn write_header(&mut self, req_type: u32, sector: u64) {
        let header = VirtioBlkReqHeader {
            req_type,
            _reserved: 0,
            sector,
        };
        // SAFETY: header buffer is 16 bytes of DMA memory owned by the caller.
        unsafe {
            core::ptr::copy_nonoverlapping(
                &header as *const VirtioBlkReqHeader as *const u8,
                self.dma.header as *mut u8,
                16,
            );
            *self.dma.status = 0xFF;
        }
    }

    fn submit_and_wait(&mut self, desc_head: u16) -> Result<u8, BlockError> {
        self.queue.add_to_avail(desc_head);
        self.transport.notify(0);
        let mut iterations = 0;
        while !self.queue.has_used() {
            iterations += 1;
            if iterations > MAX_POLL_ITERATIONS {
                self.queue.free_desc(desc_head);
                return Err(BlockError::IoError);
            }
            core::hint::spin_loop();
        }
        let (used_id, _) = self.queue.get_used().ok_or(BlockError::IoError)?;
        self.queue.free_desc(desc_head);
        if used_id != desc_head as u32 {
            return Err(BlockError::IoError);
        }
        // SAFETY: status byte is DMA memory written by the device.
        Ok(unsafe { core::ptr::read_volatile(self.dma.status) })
    }

    fn block_io(&mut self, req_type: u32, sector: u64, write: bool) -> Result<(), BlockError> {
        let desc_head = self.queue.alloc_desc(3).ok_or(BlockError::IoError)?;
        let desc_data = desc_head + 1;
        let desc_status = desc_head + 2;
        self.write_header(req_type, sector);
        self.queue.desc[desc_head as usize] = VirtqDesc {
            addr: self.dma.header_phys,
            len: 16,
            flags: VIRTQ_DESC_F_NEXT,
            next: desc_data,
        };
        self.queue.desc[desc_data as usize] = VirtqDesc {
            addr: self.dma.data_phys,
            len: BLOCK_SIZE as u32,
            flags: VIRTQ_DESC_F_NEXT | if write { 0 } else { VIRTQ_DESC_F_WRITE },
            next: desc_status,
        };
        self.queue.desc[desc_status as usize] = VirtqDesc {
            addr: self.dma.status_phys,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };
        match self.submit_and_wait(desc_head)? {
            VIRTIO_BLK_S_OK => Ok(()),
            _ => Err(BlockError::IoError),
        }
    }
}

impl<T: VirtioTransport> BlockDevice for VirtioBlkDevice<T> {
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
        self.block_io(VIRTIO_BLK_T_IN, block_idx * SECTORS_PER_BLOCK, false)?;
        // SAFETY: the device has finished writing the bounce buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(self.dma.data, buffer.as_mut_ptr(), BLOCK_SIZE);
        }
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count() {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }
        // SAFETY: bounce buffer is at least BLOCK_SIZE bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(buffer.as_ptr(), self.dma.data, BLOCK_SIZE);
        }
        self.block_io(VIRTIO_BLK_T_OUT, block_idx * SECTORS_PER_BLOCK, true)
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        let desc_head = self.queue.alloc_desc(2).ok_or(BlockError::IoError)?;
        let desc_status = desc_head + 1;
        self.write_header(VIRTIO_BLK_T_FLUSH, 0);
        self.queue.desc[desc_head as usize] = VirtqDesc {
            addr: self.dma.header_phys,
            len: 16,
            flags: VIRTQ_DESC_F_NEXT,
            next: desc_status,
        };
        self.queue.desc[desc_status as usize] = VirtqDesc {
            addr: self.dma.status_phys,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };
        match self.submit_and_wait(desc_head)? {
            // Without VIRTIO_BLK_F_FLUSH negotiated the device answers UNSUPP;
            // writes are already durable on the host side in that case.
            VIRTIO_BLK_S_OK | VIRTIO_BLK_S_UNSUPP => Ok(()),
            _ => Err(BlockError::IoError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtio::{VirtqUsedElem, VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE};
    use alloc::boxed::Box;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::sync::atomic::Ordering;

    extern crate alloc;

    /// A device model that processes one request per `notify`, following the
    /// descriptor chain by (identity-mapped) physical address, against an
    /// in-memory disk of `sectors` sectors.
    struct FakeTransport {
        queue: QueueMemory,
        disk: Vec<u8>,
        sectors: u64,
        queue_size: u16,
        status: u8,
        began: bool,
        driver_ok: bool,
        last_used: u16,
        requests: Vec<u32>,
    }

    impl FakeTransport {
        fn new(queue: QueueMemory, sectors: u64) -> Self {
            Self {
                queue,
                disk: vec![0; sectors as usize * SECTOR_SIZE],
                sectors,
                queue_size: 8,
                status: 0,
                began: false,
                driver_ok: false,
                last_used: 0,
                requests: Vec::new(),
            }
        }

        unsafe fn process(&mut self) {
            let avail = &*self.queue.avail;
            let idx = avail.idx.load(Ordering::Acquire);
            while self.last_used != idx {
                let slot = (self.last_used as usize) % self.queue_size as usize;
                let head = avail.ring[slot];
                let mut cur = head;
                let mut chain = Vec::new();
                loop {
                    let d = *self.queue.desc.add(cur as usize);
                    chain.push(d);
                    if d.flags & VIRTQ_DESC_F_NEXT == 0 {
                        break;
                    }
                    cur = d.next;
                }
                // Header first, status byte last, optional data in between.
                let last = chain.len() - 1;
                assert!(
                    chain.len() == 2 || chain.len() == 3,
                    "header[, data], status"
                );
                assert_eq!(chain[0].len, 16);
                assert_eq!(chain[0].flags & VIRTQ_DESC_F_WRITE, 0);
                assert_eq!(chain[last].len, 1);
                assert_ne!(chain[last].flags & VIRTQ_DESC_F_WRITE, 0);
                let hdr = &*(chain[0].addr as usize as *const VirtioBlkReqHeader);
                let status = chain[last].addr as usize as *mut u8;
                self.requests.push(hdr.req_type);
                let off = hdr.sector as usize * SECTOR_SIZE;
                let code = match hdr.req_type {
                    VIRTIO_BLK_T_IN => {
                        assert_ne!(chain[1].flags & VIRTQ_DESC_F_WRITE, 0);
                        let data = chain[1].addr as usize as *mut u8;
                        let len = chain[1].len as usize;
                        core::ptr::copy_nonoverlapping(self.disk[off..].as_ptr(), data, len);
                        VIRTIO_BLK_S_OK
                    }
                    VIRTIO_BLK_T_OUT => {
                        assert_eq!(chain[1].flags & VIRTQ_DESC_F_WRITE, 0);
                        let data = chain[1].addr as usize as *const u8;
                        let len = chain[1].len as usize;
                        core::ptr::copy_nonoverlapping(data, self.disk[off..].as_mut_ptr(), len);
                        VIRTIO_BLK_S_OK
                    }
                    VIRTIO_BLK_T_FLUSH => {
                        assert_eq!(chain.len(), 2);
                        VIRTIO_BLK_S_UNSUPP
                    }
                    _ => 1,
                };
                *status = code;
                let used = &mut *self.queue.used;
                let uidx = used.idx.load(Ordering::Acquire);
                used.ring[(uidx as usize) % self.queue_size as usize] = VirtqUsedElem {
                    id: head as u32,
                    len: 1,
                };
                used.idx.store(uidx.wrapping_add(1), Ordering::Release);
                self.last_used = self.last_used.wrapping_add(1);
            }
        }
    }

    impl VirtioTransport for FakeTransport {
        fn begin(&mut self) {
            self.began = true;
        }
        fn negotiate_no_features(&mut self) -> bool {
            true
        }
        fn setup_queue(&mut self, index: u16, size: u16, placement: QueuePlacement) -> Option<u16> {
            assert_eq!(index, 0);
            assert_eq!(size, self.queue_size);
            assert_eq!(placement.desc_phys, self.queue.desc as u64);
            Some(self.queue_size)
        }
        fn queue_max_size(&mut self, _index: u16) -> u16 {
            self.queue_size
        }
        fn driver_ok(&mut self) {
            self.driver_ok = true;
            self.status = 4;
        }
        fn notify(&mut self, queue: u16) {
            assert_eq!(queue, 0);
            assert!(self.driver_ok);
            unsafe { self.process() }
        }
        fn read_config_u64(&mut self, offset: usize) -> u64 {
            assert_eq!(offset, 0);
            self.sectors
        }
    }

    fn leak_queue() -> QueueMemory {
        let desc = Box::leak(Box::new([VirtqDesc::new(); VIRTQ_MAX_SIZE])).as_mut_ptr();
        let avail: *mut VirtqAvail = Box::leak(Box::new(VirtqAvail::new()));
        let used: *mut VirtqUsed = Box::leak(Box::new(VirtqUsed::new()));
        QueueMemory {
            desc,
            avail,
            used,
            placement: QueuePlacement {
                desc_phys: desc as u64,
                avail_phys: avail as u64,
                used_phys: used as u64,
            },
        }
    }

    fn leak_dma() -> DmaBuffers {
        let header: *mut [u8; 16] = Box::leak(Box::new([0u8; 16]));
        let status: *mut u8 = Box::leak(Box::new(0u8));
        let data = Box::leak(vec![0u8; BLOCK_SIZE].into_boxed_slice()).as_mut_ptr();
        DmaBuffers {
            header,
            header_phys: header as u64,
            status,
            status_phys: status as u64,
            data,
            data_phys: data as u64,
        }
    }

    fn device(sectors: u64) -> VirtioBlkDevice<FakeTransport> {
        let queue = leak_queue();
        let transport = FakeTransport::new(queue, sectors);
        unsafe { VirtioBlkDevice::new(transport, queue, leak_dma()) }.expect("device")
    }

    #[test]
    fn init_reports_capacity_in_blocks() {
        let dev = device(64);
        assert!(dev.transport.began && dev.transport.driver_ok);
        assert_eq!(dev.capacity_sectors(), 64);
        assert_eq!(dev.block_count(), 64 / SECTORS_PER_BLOCK);
    }

    #[test]
    fn write_then_read_round_trips_through_bounce_buffer() {
        let mut dev = device(64);
        let mut block = vec![0u8; BLOCK_SIZE];
        for (i, b) in block.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        dev.write_block(3, &block).unwrap();
        let mut back = vec![0u8; BLOCK_SIZE];
        dev.read_block(3, &mut back).unwrap();
        assert_eq!(back, block);
        let mut other = vec![1u8; BLOCK_SIZE];
        dev.read_block(2, &mut other).unwrap();
        assert!(other.iter().all(|&b| b == 0));
        assert_eq!(
            dev.transport.requests,
            vec![VIRTIO_BLK_T_OUT, VIRTIO_BLK_T_IN, VIRTIO_BLK_T_IN]
        );
    }

    #[test]
    fn descriptors_are_recycled_across_many_requests() {
        let mut dev = device(64);
        let block = vec![7u8; BLOCK_SIZE];
        for i in 0..40 {
            dev.write_block(i % 4, &block).unwrap();
        }
        assert_eq!(dev.queue.num_free, dev.queue.size);
    }

    #[test]
    fn bounds_and_size_errors() {
        let mut dev = device(64);
        let mut small = [0u8; 16];
        assert_eq!(dev.read_block(0, &mut small), Err(BlockError::InvalidSize));
        let mut block = vec![0u8; BLOCK_SIZE];
        assert_eq!(
            dev.read_block(dev.block_count(), &mut block),
            Err(BlockError::OutOfBounds)
        );
    }

    #[test]
    fn unsupported_flush_is_not_an_error() {
        let mut dev = device(64);
        assert_eq!(dev.flush(), Ok(()));
    }
}
