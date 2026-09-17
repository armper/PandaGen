//! Bare-metal networking: virtio-net over PCI plus the `net_stack` protocols.
//!
//! Everything is polled from the boot CPU's command path; there is no
//! network interrupt yet. Buffers live in page-aligned statics whose
//! physical addresses come from the kernel image mapping.

extern crate alloc;

use core::fmt::Write;

use crate::bare_metal_storage::StorageBootInfo;
use hal_x86_64::virtio::{QueuePlacement, VIRTQ_MAX_SIZE};
use hal_x86_64::virtio_net::{MAX_FRAME_LEN, NET_BUF_LEN};
use hal_x86_64::{
    LegacyQueueLayout, NetDma, QueueMemory, RealPortIo, VirtioNetDevice, VirtioPciLegacy,
    VirtqAvail, VirtqDesc, VirtqUsed,
};
use net_stack::wire::{fmt_ipv4, fmt_mac};
use net_stack::{Config, Event, Interface, Ipv4, SendError};

/// UDP port the kernel echoes datagrams on.
pub const UDP_ECHO_PORT: u16 = 7777;
/// UDP port for remote IPC calls (see `remote_ipc`).
pub const REMOTE_PORT: u16 = remote_ipc::KERNEL_REMOTE_PORT;

/// A datagram received on `REMOTE_PORT`, handed to the kernel's remote
/// command server.
pub struct RemoteDatagram {
    pub src: Ipv4,
    pub src_port: u16,
    pub bytes: alloc::vec::Vec<u8>,
}

const RX_BUFFERS: usize = 8;
const QUEUE_AREA_BYTES: usize = 12288;

#[repr(C, align(4096))]
struct QueueArea([u8; QUEUE_AREA_BYTES]);
static mut RX_QUEUE_AREA: QueueArea = QueueArea([0; QUEUE_AREA_BYTES]);
static mut TX_QUEUE_AREA: QueueArea = QueueArea([0; QUEUE_AREA_BYTES]);

#[repr(C, align(4096))]
struct NetBuffers {
    rx: [u8; NET_BUF_LEN * RX_BUFFERS],
    tx: [u8; NET_BUF_LEN],
}
static mut NET_BUFFERS: NetBuffers = NetBuffers {
    rx: [0; NET_BUF_LEN * RX_BUFFERS],
    tx: [0; NET_BUF_LEN],
};

/// Ticks (10 ms) to wait for an ARP or echo reply.
const REPLY_TIMEOUT_TICKS: u64 = 100;

pub struct NetStack {
    device: VirtioNetDevice<VirtioPciLegacy<RealPortIo>>,
    iface: Interface,
    rx_frame: [u8; MAX_FRAME_LEN],
    tx_frame: [u8; MAX_FRAME_LEN],
    udp_echoed: u64,
}

// SAFETY: the stack is only ever driven from one CPU at a time, under the
// spinlock that holds it.
unsafe impl Send for NetStack {}

impl NetStack {
    /// Find a virtio-net PCI function, bring it up, and configure the
    /// interface with QEMU user-networking defaults.
    pub fn probe(boot: StorageBootInfo) -> Option<Self> {
        let mut io = RealPortIo::new();
        let info = hal_x86_64::pci::find_virtio_net(&mut io)?;
        let base = info.io_base()?;
        hal_x86_64::pci::enable_io_and_bus_master(&mut io, info.address);

        // SAFETY: the statics are only touched here, once, before the
        // device owns them.
        let device = unsafe {
            let rx_queue = queue_memory(core::ptr::addr_of_mut!(RX_QUEUE_AREA.0) as *mut u8, boot)?;
            let tx_queue = queue_memory(core::ptr::addr_of_mut!(TX_QUEUE_AREA.0) as *mut u8, boot)?;
            let rx = core::ptr::addr_of_mut!(NET_BUFFERS.rx) as *mut u8;
            let tx = core::ptr::addr_of_mut!(NET_BUFFERS.tx) as *mut u8;
            let dma = NetDma {
                rx,
                rx_phys: boot.image_phys(rx as usize)?,
                rx_count: RX_BUFFERS,
                tx,
                tx_phys: boot.image_phys(tx as usize)?,
            };
            let transport = VirtioPciLegacy::new(RealPortIo::new(), base);
            VirtioNetDevice::new(transport, rx_queue, tx_queue, dma).ok()?
        };
        let mac = device.mac();
        let mut iface = Interface::new(Config::qemu_user(mac));
        iface.bind(UDP_ECHO_PORT);
        iface.bind(REMOTE_PORT);
        Some(Self {
            device,
            iface,
            rx_frame: [0; MAX_FRAME_LEN],
            tx_frame: [0; MAX_FRAME_LEN],
            udp_echoed: 0,
        })
    }

    pub fn mac(&self) -> [u8; 6] {
        self.device.mac()
    }

    pub fn ip(&self) -> Ipv4 {
        self.iface.config().ip
    }

    /// One line per fact about the interface.
    pub fn write_status(&self, out: &mut impl Write) {
        let cfg = self.iface.config();
        let c = self.iface.counters();
        let _ = writeln!(
            out,
            "net: virtio-net-pci mac={} ip={} gw={}",
            fmt_mac(cfg.mac),
            fmt_ipv4(cfg.ip),
            fmt_ipv4(cfg.gateway)
        );
        let _ = writeln!(
            out,
            "net: rx={} tx={} arp_cache={} echo_sent={} echo_recv={} dropped={}",
            self.device.frames_received(),
            self.device.frames_sent(),
            self.iface.arp_cache().len(),
            c.echo_requests_sent,
            c.echo_replies_received,
            c.dropped
        );
        let _ = writeln!(
            out,
            "net: udp port {} recv={} sent={} echoed={} unbound={}",
            UDP_ECHO_PORT, c.udp_received, c.udp_sent, self.udp_echoed, c.udp_unbound
        );
    }

    /// Pull in every pending frame, letting the interface answer ARP and
    /// echo requests. Returns the first `EchoReply` seen, if any.
    pub fn poll(&mut self) -> Option<Event> {
        self.poll_for_echo()
    }

    /// Background servicing from the main loop: answer ARP, ping, and
    /// echo UDP datagrams on `UDP_ECHO_PORT`. Returns the first datagram
    /// for `REMOTE_PORT` seen (later ones wait in the receive queue).
    pub fn service(&mut self, log: &mut impl Write) -> Option<RemoteDatagram> {
        while let Some(len) = self.device.poll_receive(&mut self.rx_frame) {
            match self
                .iface
                .receive(&self.rx_frame[..len], &mut self.tx_frame)
            {
                Event::Transmit(n) => {
                    let _ = self.device.transmit(&self.tx_frame[..n]);
                }
                Event::Udp {
                    src,
                    src_port,
                    dst_port,
                    payload_offset,
                    payload_len,
                } if dst_port == REMOTE_PORT => {
                    let payload = &self.rx_frame[payload_offset..payload_offset + payload_len];
                    return Some(RemoteDatagram {
                        src,
                        src_port,
                        bytes: payload.to_vec(),
                    });
                }
                Event::Udp {
                    src,
                    src_port,
                    dst_port,
                    payload_offset,
                    payload_len,
                } => {
                    let payload = &self.rx_frame[payload_offset..payload_offset + payload_len];
                    match self
                        .iface
                        .udp_send(src, src_port, dst_port, payload, &mut self.tx_frame)
                    {
                        Ok(n) => {
                            if self.device.transmit(&self.tx_frame[..n]).is_ok() {
                                self.udp_echoed += 1;
                                let _ = writeln!(
                                    log,
                                    "net: udp echo {} bytes to {}:{}",
                                    payload_len,
                                    fmt_ipv4(src),
                                    src_port
                                );
                            }
                        }
                        Err(SendError::NeedArp) => {
                            let n = self.iface.pending_frame_len();
                            let _ = self.device.transmit(&self.tx_frame[..n]);
                        }
                        Err(_) => {}
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Reply to a remote caller from `REMOTE_PORT` (next hop already known
    /// since the request just arrived from it).
    pub fn udp_reply(&mut self, dst: Ipv4, port: u16, payload: &[u8]) -> bool {
        match self
            .iface
            .udp_send(dst, port, REMOTE_PORT, payload, &mut self.tx_frame)
        {
            Ok(len) => self.device.transmit(&self.tx_frame[..len]).is_ok(),
            Err(_) => false,
        }
    }

    /// Send one UDP datagram from the echo port, resolving the next hop
    /// first if needed.
    pub fn udp_send(
        &mut self,
        target: Ipv4,
        port: u16,
        payload: &[u8],
        now: &dyn Fn() -> u64,
        out: &mut impl Write,
    ) -> bool {
        for _ in 0..3 {
            match self
                .iface
                .udp_send(target, port, UDP_ECHO_PORT, payload, &mut self.tx_frame)
            {
                Ok(len) => {
                    return match self.device.transmit(&self.tx_frame[..len]) {
                        Ok(()) => {
                            let _ = writeln!(
                                out,
                                "net: sent {} bytes to {}:{}",
                                payload.len(),
                                fmt_ipv4(target),
                                port
                            );
                            true
                        }
                        Err(err) => {
                            let _ = writeln!(out, "net: transmit failed ({err:?})");
                            false
                        }
                    };
                }
                Err(SendError::NeedArp) => {
                    let len = self.iface.pending_frame_len();
                    let _ = self.device.transmit(&self.tx_frame[..len]);
                    let hop = self.iface.config().next_hop(target);
                    let start = now();
                    while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS
                        && self.iface.arp_cache().lookup(hop).is_none()
                    {
                        let _ = self.poll_for_echo();
                        core::hint::spin_loop();
                    }
                }
                Err(SendError::BufferTooSmall) => {
                    let _ = writeln!(out, "net: payload too large");
                    return false;
                }
            }
        }
        let _ = writeln!(
            out,
            "net: could not resolve next hop for {}",
            fmt_ipv4(target)
        );
        false
    }

    /// Ping `target`, writing progress lines to `out`. `now` returns the
    /// kernel tick. Returns true on a reply.
    pub fn ping(&mut self, target: Ipv4, now: &dyn Fn() -> u64, out: &mut impl Write) -> bool {
        const PAYLOAD: &[u8] = b"PandaGen ping";
        // Resolve the next hop first (bounded retries).
        for attempt in 0..3 {
            match self.iface.ping(target, PAYLOAD, &mut self.tx_frame) {
                Ok((len, seq)) => {
                    if let Err(err) = self.device.transmit(&self.tx_frame[..len]) {
                        let _ = writeln!(out, "net: transmit failed ({err:?})");
                        self.iface.cancel_ping();
                        return false;
                    }
                    let start = now();
                    while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS {
                        if let Some(event) = self.poll_for_echo() {
                            if let Event::EchoReply {
                                from,
                                seq: rseq,
                                ttl,
                            } = event
                            {
                                let _ = writeln!(
                                    out,
                                    "reply from {}: seq={} ttl={} time={} ticks",
                                    fmt_ipv4(from),
                                    rseq,
                                    ttl,
                                    now().saturating_sub(start)
                                );
                                return true;
                            }
                        }
                        core::hint::spin_loop();
                    }
                    self.iface.cancel_ping();
                    let _ = writeln!(out, "net: no reply from {} (seq={})", fmt_ipv4(target), seq);
                    return false;
                }
                Err(SendError::NeedArp) => {
                    let len = self.iface.pending_frame_len();
                    if self.device.transmit(&self.tx_frame[..len]).is_err() {
                        let _ = writeln!(out, "net: arp transmit failed");
                        return false;
                    }
                    let hop = self.iface.config().next_hop(target);
                    let start = now();
                    while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS
                        && self.iface.arp_cache().lookup(hop).is_none()
                    {
                        let _ = self.poll_for_echo();
                        core::hint::spin_loop();
                    }
                    if self.iface.arp_cache().lookup(hop).is_none() {
                        let _ = writeln!(
                            out,
                            "net: arp for {} unanswered (attempt {})",
                            fmt_ipv4(hop),
                            attempt + 1
                        );
                    }
                }
                Err(SendError::BufferTooSmall) => {
                    let _ = writeln!(out, "net: frame buffer too small");
                    return false;
                }
            }
        }
        let _ = writeln!(
            out,
            "net: could not resolve next hop for {}",
            fmt_ipv4(target)
        );
        false
    }

    /// Drain received frames; answer requests; return an echo reply event.
    fn poll_for_echo(&mut self) -> Option<Event> {
        let mut found = None;
        while let Some(len) = self.device.poll_receive(&mut self.rx_frame) {
            match self
                .iface
                .receive(&self.rx_frame[..len], &mut self.tx_frame)
            {
                Event::Transmit(n) => {
                    let _ = self.device.transmit(&self.tx_frame[..n]);
                }
                event @ Event::EchoReply { .. } => {
                    if found.is_none() {
                        found = Some(event);
                    }
                }
                // Datagrams arriving mid-ping are dropped; the main loop's
                // `service` handles UDP when no command holds the stack.
                Event::Udp { .. } | Event::None => {}
            }
        }
        found
    }
}

/// Lay a legacy virtqueue out in `area` and translate it to physical.
unsafe fn queue_memory(area: *mut u8, boot: StorageBootInfo) -> Option<QueueMemory> {
    core::ptr::write_bytes(area, 0, QUEUE_AREA_BYTES);
    let layout = LegacyQueueLayout::for_size(VIRTQ_MAX_SIZE as u16);
    let area_phys = boot.image_phys(area as usize)?;
    Some(QueueMemory {
        desc: area.add(layout.desc_offset) as *mut VirtqDesc,
        avail: area.add(layout.avail_offset) as *mut VirtqAvail,
        used: area.add(layout.used_offset) as *mut VirtqUsed,
        placement: QueuePlacement {
            desc_phys: area_phys + layout.desc_offset as u64,
            avail_phys: area_phys + layout.avail_offset as u64,
            used_phys: area_phys + layout.used_offset as u64,
        },
    })
}
