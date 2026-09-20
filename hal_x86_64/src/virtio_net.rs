//! virtio-net driver over any `VirtioTransport`.
//!
//! Legacy layout: queue 0 receives, queue 1 transmits, and every buffer
//! starts with a 10-byte `virtio_net_hdr` (no mergeable buffers, since no
//! features are negotiated). Receive buffers are posted up front and
//! re-posted after each frame is copied out; transmit is polled.

use crate::virtio::{
    QueuePlacement, VirtioTransport, VirtqAvail, VirtqDesc, VirtqUsed, Virtqueue,
    VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE, VIRTQ_MAX_SIZE,
};
use crate::virtio_blk::QueueMemory;

/// Legacy virtio-net header size.
pub const NET_HDR_LEN: usize = 10;
/// Largest Ethernet frame we handle.
pub const MAX_FRAME_LEN: usize = 1514;
/// Bytes reserved per receive or transmit buffer (header + frame, padded).
pub const NET_BUF_LEN: usize = 2048;

const _: () = assert!(NET_BUF_LEN >= NET_HDR_LEN + MAX_FRAME_LEN);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    NotReady,
    FrameTooLarge,
    QueueFull,
    Timeout,
}

/// DMA buffers for the driver, with physical addresses.
#[derive(Debug, Clone, Copy)]
pub struct NetDma {
    /// `rx_count` buffers of `NET_BUF_LEN` bytes each.
    pub rx: *mut u8,
    pub rx_phys: u64,
    pub rx_count: usize,
    /// One `NET_BUF_LEN` transmit buffer.
    pub tx: *mut u8,
    pub tx_phys: u64,
}

pub struct VirtioNetDevice<T: VirtioTransport> {
    transport: T,
    rx: Virtqueue,
    tx: Virtqueue,
    dma: NetDma,
    mac: [u8; 6],
    frames_received: u64,
    frames_sent: u64,
    /// Frames dropped because the device reported an impossible index or
    /// length.
    rx_errors: u64,
    /// Transmit completions refused because the device reported a descriptor
    /// id outside the table. Indexing on it would abort the kernel.
    tx_errors: u64,
}

impl<T: VirtioTransport> VirtioNetDevice<T> {
    /// Bring the device up with receive queue `rx_queue` and transmit queue
    /// `tx_queue`, and post every receive buffer.
    ///
    /// # Safety
    /// The queue memory and DMA buffers must stay valid and mapped for the
    /// life of the device, with correct physical addresses.
    pub unsafe fn new(
        mut transport: T,
        rx_queue: QueueMemory,
        tx_queue: QueueMemory,
        dma: NetDma,
    ) -> Result<Self, NetError> {
        transport.begin();
        if !transport.negotiate_no_features() {
            return Err(NetError::NotReady);
        }
        let rx = Self::bring_up_queue(&mut transport, 0, rx_queue)?;
        let tx = Self::bring_up_queue(&mut transport, 1, tx_queue)?;
        transport.driver_ok();
        let raw = transport.read_config_u64(0);
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&raw.to_le_bytes()[..6]);
        let mut device = Self {
            transport,
            rx,
            tx,
            dma,
            mac,
            frames_received: 0,
            frames_sent: 0,
            rx_errors: 0,
            tx_errors: 0,
        };
        device.post_all_rx();
        Ok(device)
    }

    unsafe fn bring_up_queue(
        transport: &mut T,
        index: u16,
        memory: QueueMemory,
    ) -> Result<Virtqueue, NetError> {
        let wanted = transport.queue_max_size(index).min(VIRTQ_MAX_SIZE as u16);
        if wanted == 0 {
            return Err(NetError::NotReady);
        }
        let size = transport
            .setup_queue(index, wanted, memory.placement)
            .ok_or(NetError::NotReady)?;
        if size as usize > VIRTQ_MAX_SIZE {
            return Err(NetError::NotReady);
        }
        Ok(Virtqueue::new(size, memory.desc, memory.avail, memory.used))
    }

    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Descriptor table pointer, for tests that emulate a hostile device.
    #[cfg(test)]
    pub(crate) fn queue_desc_ptr(&self) -> *mut VirtqDesc {
        self.rx.desc.as_ptr() as *mut VirtqDesc
    }

    pub fn frames_received(&self) -> u64 {
        self.frames_received
    }

    pub fn frames_sent(&self) -> u64 {
        self.frames_sent
    }

    /// Frames dropped because the device reported an impossible index or length.
    pub fn tx_errors(&self) -> u64 {
        self.tx_errors
    }

    pub fn rx_errors(&self) -> u64 {
        self.rx_errors
    }

    fn rx_buffer(&self, slot: usize) -> (*mut u8, u64) {
        let offset = slot * NET_BUF_LEN;
        // SAFETY: slot < rx_count, buffers are contiguous.
        (
            unsafe { self.dma.rx.add(offset) },
            self.dma.rx_phys + offset as u64,
        )
    }

    fn post_rx(&mut self, slot: usize) -> bool {
        let Some(head) = self.rx.alloc_desc(1) else {
            return false;
        };
        let (_, phys) = self.rx_buffer(slot);
        self.rx.desc[head as usize] = VirtqDesc {
            addr: phys,
            len: NET_BUF_LEN as u32,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };
        // Descriptor index doubles as the slot id: with one descriptor per
        // buffer and `rx_count <= size`, `alloc_desc` hands them out in order
        // at start-up, so we record the mapping explicitly instead.
        self.rx_slot_of_desc_set(head, slot);
        self.rx.add_to_avail(head);
        true
    }

    fn rx_slot_of_desc_set(&mut self, desc: u16, slot: usize) {
        // The descriptor's `next` field is unused for single-descriptor
        // chains, so it stores the slot.
        self.rx.desc[desc as usize].next = slot as u16;
    }

    fn post_all_rx(&mut self) {
        let count = self.dma.rx_count.min(self.rx.size as usize);
        for slot in 0..count {
            if !self.post_rx(slot) {
                break;
            }
        }
        self.transport.notify(0);
    }

    /// Copy the next received frame (without the virtio header) into `out`.
    /// Returns the frame length, or `None` when nothing is pending. Frames
    /// larger than `out` are dropped.
    pub fn poll_receive(&mut self, out: &mut [u8]) -> Option<usize> {
        let (id, len) = self.rx.get_used()?;
        // The used ring is device-writable memory. A descriptor id or buffer
        // slot out of range would index past the descriptor table or read far
        // beyond the DMA region, so both are checked before use.
        if id as usize >= self.rx.size as usize {
            self.rx_errors += 1;
            return None;
        }
        let desc = id as u16;
        let slot = self.rx.desc[desc as usize].next as usize;
        if slot >= self.dma.rx_count {
            self.rx_errors += 1;
            self.rx.free_desc(desc);
            return None;
        }
        self.rx.free_desc(desc);
        let len = len as usize;
        let mut result = None;
        if len > NET_HDR_LEN {
            // A frame longer than the buffer is dropped rather than
            // truncated: a truncated frame is a different frame.
            let frame_len = len - NET_HDR_LEN;
            if frame_len <= MAX_FRAME_LEN && frame_len <= out.len() && len <= NET_BUF_LEN {
                let (buf, _) = self.rx_buffer(slot);
                // SAFETY: the device wrote `len` bytes into this buffer.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        buf.add(NET_HDR_LEN),
                        out.as_mut_ptr(),
                        frame_len,
                    );
                }
                self.frames_received += 1;
                result = Some(frame_len);
            }
        }
        if self.post_rx(slot) {
            self.transport.notify(0);
        }
        result
    }

    /// Send one Ethernet frame and wait (polled) for the device to consume it.
    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), NetError> {
        if frame.len() > MAX_FRAME_LEN {
            return Err(NetError::FrameTooLarge);
        }
        // Reclaim any earlier completed transmit descriptors. The id comes
        // from the device-writable used ring, so it is checked the same way
        // `poll_receive` checks its own.
        while let Some((id, _)) = self.tx.get_used() {
            if id as usize >= self.tx.size as usize || !self.tx.free_desc(id as u16) {
                self.tx_errors += 1;
            }
        }
        let head = self.tx.alloc_desc(2).ok_or(NetError::QueueFull)?;
        let data = head + 1;
        // SAFETY: tx buffer is NET_BUF_LEN bytes of DMA memory.
        unsafe {
            core::ptr::write_bytes(self.dma.tx, 0, NET_HDR_LEN);
            core::ptr::copy_nonoverlapping(
                frame.as_ptr(),
                self.dma.tx.add(NET_HDR_LEN),
                frame.len(),
            );
        }
        self.tx.desc[head as usize] = VirtqDesc {
            addr: self.dma.tx_phys,
            len: NET_HDR_LEN as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: data,
        };
        self.tx.desc[data as usize] = VirtqDesc {
            addr: self.dma.tx_phys + NET_HDR_LEN as u64,
            len: frame.len() as u32,
            flags: 0,
            next: 0,
        };
        self.tx.add_to_avail(head);
        self.transport.notify(1);
        let mut spins = 0u32;
        loop {
            if let Some((id, _)) = self.tx.get_used() {
                if id as usize >= self.tx.size as usize || !self.tx.free_desc(id as u16) {
                    self.tx_errors += 1;
                    continue;
                }
                if id == head as u32 {
                    self.frames_sent += 1;
                    return Ok(());
                }
                continue;
            }
            spins += 1;
            if spins > 4_000_000 {
                return Err(NetError::Timeout);
            }
            core::hint::spin_loop();
        }
    }
}

/// Helpers for building queue memory in tests and simple kernels.
pub fn queue_placement_identity(
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail,
    used: *mut VirtqUsed,
) -> QueuePlacement {
    QueuePlacement {
        desc_phys: desc as u64,
        avail_phys: avail as u64,
        used_phys: used as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtio::VirtqUsedElem;
    use alloc::boxed::Box;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::sync::atomic::Ordering;

    extern crate alloc;

    /// Device model: consumes transmit chains immediately, and lets the
    /// test deliver frames into posted receive buffers.
    struct FakeNet {
        rx: QueueMemory,
        tx: QueueMemory,
        size: u16,
        rx_seen: u16,
        tx_seen: u16,
        sent: Vec<Vec<u8>>,
        mac: [u8; 6],
        /// Posted receive descriptors not yet filled, in order.
        rx_pending: Vec<u16>,
    }

    impl FakeNet {
        unsafe fn drain_rx_avail(&mut self) {
            let avail = &*self.rx.avail;
            let idx = avail.idx.load(Ordering::Acquire);
            while self.rx_seen != idx {
                let head = avail.ring[(self.rx_seen as usize) % self.size as usize];
                self.rx_pending.push(head);
                self.rx_seen = self.rx_seen.wrapping_add(1);
            }
        }

        unsafe fn deliver(&mut self, frame: &[u8]) -> bool {
            self.drain_rx_avail();
            if self.rx_pending.is_empty() {
                return false;
            }
            let head = self.rx_pending.remove(0);
            let d = *self.rx.desc.add(head as usize);
            assert_ne!(d.flags & VIRTQ_DESC_F_WRITE, 0);
            assert_eq!(d.len as usize, NET_BUF_LEN);
            let buf = d.addr as usize as *mut u8;
            core::ptr::write_bytes(buf, 0, NET_HDR_LEN);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf.add(NET_HDR_LEN), frame.len());
            let used = &mut *self.rx.used;
            let uidx = used.idx.load(Ordering::Acquire);
            used.ring[(uidx as usize) % self.size as usize] = VirtqUsedElem {
                id: head as u32,
                len: (NET_HDR_LEN + frame.len()) as u32,
            };
            used.idx.store(uidx.wrapping_add(1), Ordering::Release);
            true
        }

        /// Report an arbitrary completion, as a buggy or hostile device
        /// would. The driver must not trust `id`.
        unsafe fn deliver_raw(&mut self, id: u32, len: u32) {
            let used = &mut *self.rx.used;
            let uidx = used.idx.load(Ordering::Acquire);
            used.ring[(uidx as usize) % self.size as usize] = VirtqUsedElem { id, len };
            used.idx.store(uidx.wrapping_add(1), Ordering::Release);
        }

        /// As `deliver_raw`, but on the *transmit* queue.
        unsafe fn deliver_tx_raw(&mut self, id: u32, len: u32) {
            let used = &mut *self.tx.used;
            let uidx = used.idx.load(Ordering::Acquire);
            used.ring[(uidx as usize) % self.size as usize] = VirtqUsedElem { id, len };
            used.idx.store(uidx.wrapping_add(1), Ordering::Release);
        }

        unsafe fn process_tx(&mut self) {
            let avail = &*self.tx.avail;
            let idx = avail.idx.load(Ordering::Acquire);
            while self.tx_seen != idx {
                let head = avail.ring[(self.tx_seen as usize) % self.size as usize];
                let hdr = *self.tx.desc.add(head as usize);
                assert_eq!(hdr.len as usize, NET_HDR_LEN);
                assert_ne!(hdr.flags & VIRTQ_DESC_F_NEXT, 0);
                let data = *self.tx.desc.add(hdr.next as usize);
                assert_eq!(data.flags, 0);
                let bytes =
                    core::slice::from_raw_parts(data.addr as usize as *const u8, data.len as usize);
                self.sent.push(bytes.to_vec());
                let used = &mut *self.tx.used;
                let uidx = used.idx.load(Ordering::Acquire);
                used.ring[(uidx as usize) % self.size as usize] = VirtqUsedElem {
                    id: head as u32,
                    len: 0,
                };
                used.idx.store(uidx.wrapping_add(1), Ordering::Release);
                self.tx_seen = self.tx_seen.wrapping_add(1);
            }
        }
    }

    impl VirtioTransport for FakeNet {
        fn begin(&mut self) {}
        fn negotiate_features(&mut self, _wanted: u64) -> Option<u64> {
            Some(0)
        }
        fn setup_queue(&mut self, index: u16, size: u16, placement: QueuePlacement) -> Option<u16> {
            let expected = if index == 0 { self.rx } else { self.tx };
            assert_eq!(placement.desc_phys, expected.desc as u64);
            assert_eq!(size, self.size);
            Some(self.size)
        }
        fn queue_max_size(&mut self, _index: u16) -> u16 {
            self.size
        }
        fn driver_ok(&mut self) {}
        fn notify(&mut self, queue: u16) {
            unsafe {
                match queue {
                    0 => self.drain_rx_avail(),
                    1 => self.process_tx(),
                    _ => panic!("bad queue"),
                }
            }
        }
        fn read_config_u64(&mut self, offset: usize) -> u64 {
            assert_eq!(offset, 0);
            let mut b = [0u8; 8];
            b[..6].copy_from_slice(&self.mac);
            u64::from_le_bytes(b)
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
            placement: queue_placement_identity(desc, avail, used),
        }
    }

    fn leak_dma(rx_count: usize) -> NetDma {
        let rx = Box::leak(vec![0u8; NET_BUF_LEN * rx_count].into_boxed_slice()).as_mut_ptr();
        let tx = Box::leak(vec![0u8; NET_BUF_LEN].into_boxed_slice()).as_mut_ptr();
        NetDma {
            rx,
            rx_phys: rx as u64,
            rx_count,
            tx,
            tx_phys: tx as u64,
        }
    }

    fn device(rx_count: usize) -> VirtioNetDevice<FakeNet> {
        let rx = leak_queue();
        let tx = leak_queue();
        let fake = FakeNet {
            rx,
            tx,
            size: 8,
            rx_seen: 0,
            tx_seen: 0,
            sent: Vec::new(),
            mac: [0x52, 0x54, 0, 0x12, 0x34, 0x56],
            rx_pending: Vec::new(),
        };
        unsafe { VirtioNetDevice::new(fake, rx, tx, leak_dma(rx_count)) }.expect("device")
    }

    #[test]
    fn reads_mac_and_posts_receive_buffers() {
        let dev = device(4);
        assert_eq!(dev.mac(), [0x52, 0x54, 0, 0x12, 0x34, 0x56]);
        assert_eq!(dev.transport.rx_pending.len(), 4);
        assert_eq!(dev.rx.num_free, 4);
    }

    #[test]
    fn transmit_prepends_header_and_completes() {
        let mut dev = device(2);
        let frame: Vec<u8> = (0..60u8).collect();
        dev.transmit(&frame).unwrap();
        assert_eq!(dev.transport.sent, vec![frame.clone()]);
        assert_eq!(dev.frames_sent(), 1);
        dev.transmit(&frame).unwrap();
        assert_eq!(dev.frames_sent(), 2);
        assert_eq!(dev.tx.num_free, dev.tx.size);
        assert_eq!(
            dev.transmit(&vec![0u8; MAX_FRAME_LEN + 1]),
            Err(NetError::FrameTooLarge)
        );
    }

    #[test]
    fn receive_strips_header_and_reposts_buffer() {
        let mut dev = device(2);
        let mut out = [0u8; MAX_FRAME_LEN];
        assert_eq!(dev.poll_receive(&mut out), None);
        let frame: Vec<u8> = (0..100u8).collect();
        assert!(unsafe { dev.transport.deliver(&frame) });
        assert_eq!(dev.poll_receive(&mut out), Some(100));
        assert_eq!(&out[..100], &frame[..]);
        assert_eq!(dev.frames_received(), 1);
        // The buffer went back to the device: two more deliveries still fit.
        assert!(unsafe { dev.transport.deliver(&frame) });
        assert!(unsafe { dev.transport.deliver(&frame) });
        assert!(!unsafe { dev.transport.deliver(&frame) });
        assert_eq!(dev.poll_receive(&mut out), Some(100));
        assert_eq!(dev.poll_receive(&mut out), Some(100));
        assert_eq!(dev.poll_receive(&mut out), None);
    }

    #[test]
    fn many_receives_do_not_exhaust_descriptors() {
        let mut dev = device(3);
        let mut out = [0u8; MAX_FRAME_LEN];
        for i in 0..50u8 {
            assert!(unsafe { dev.transport.deliver(&[i; 42]) });
            assert_eq!(dev.poll_receive(&mut out), Some(42));
            assert_eq!(out[0], i);
        }
        assert_eq!(dev.frames_received(), 50);
    }

    /// The transmit side of the receive guard above, which it did not have.
    /// `transmit` reclaims completed descriptors from the same
    /// device-writable used ring and passed the id straight to `free_desc`,
    /// which indexes the descriptor table. A panic here is a dead machine.
    #[test]
    fn a_device_reported_transmit_id_out_of_range_is_dropped() {
        let mut dev = device(2);
        unsafe { dev.transport.deliver_tx_raw(9999, 0) };
        let frame: alloc::vec::Vec<u8> = (0..60u8).collect();
        assert!(dev.transmit(&frame).is_ok(), "the frame must still go out");
        assert!(
            dev.tx_errors() >= 1,
            "the bad id must be counted, not ignored"
        );

        // And the driver is not wedged by it.
        assert!(dev.transmit(&frame).is_ok());
    }

    #[test]
    fn a_device_reported_descriptor_id_out_of_range_is_dropped() {
        // The used ring is device-writable. Indexing the descriptor table
        // with an id from it would panic, and a panic here is a dead machine.
        let mut dev = device(3);
        let mut out = [0u8; MAX_FRAME_LEN];
        unsafe { dev.transport.deliver_raw(9999, 128) };
        assert_eq!(dev.poll_receive(&mut out), None);
        assert_eq!(dev.rx_errors(), 1);
        // The driver is not wedged: a well-formed frame still arrives.
        assert!(unsafe { dev.transport.deliver(&[7u8; 64]) });
        assert_eq!(dev.poll_receive(&mut out), Some(64));
        assert_eq!(&out[..64], &[7u8; 64]);
    }

    #[test]
    fn a_buffer_slot_out_of_range_is_dropped() {
        // The slot index lives in the descriptor, which the device can also
        // reach. Trusting it would read far past the DMA region and then
        // echo that memory back to the network.
        let mut dev = device(3);
        let mut out = [0u8; MAX_FRAME_LEN];
        let head = dev.transport.rx_pending[0];
        unsafe {
            (*dev.queue_desc_ptr().add(head as usize)).next = 4096;
            dev.transport.deliver_raw(head as u32, 128);
        }
        assert_eq!(dev.poll_receive(&mut out), None);
        assert_eq!(dev.rx_errors(), 1);
    }

    #[test]
    fn an_oversized_length_is_dropped_rather_than_truncated() {
        // A truncated frame is a different frame; a hardened stack drops it.
        let mut dev = device(3);
        let mut out = [0u8; MAX_FRAME_LEN];
        let head = dev.transport.rx_pending[0];
        unsafe {
            dev.transport
                .deliver_raw(head as u32, (NET_BUF_LEN + 64) as u32)
        };
        assert_eq!(dev.poll_receive(&mut out), None);
    }
}
