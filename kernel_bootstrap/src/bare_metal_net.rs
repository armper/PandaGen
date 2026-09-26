//! Bare-metal networking: virtio-net over PCI plus the `net_stack` protocols.
//!
//! Everything is polled from the boot CPU's command path; there is no
//! network interrupt yet. Buffers live in page-aligned statics whose
//! physical addresses come from the kernel image mapping.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::bare_metal_storage::StorageBootInfo;
use hal_x86_64::virtio::{QueuePlacement, VIRTQ_MAX_SIZE};
use hal_x86_64::virtio_net::{MAX_FRAME_LEN, NET_BUF_LEN};
use hal_x86_64::{
    LegacyQueueLayout, NetDma, QueueMemory, RealPortIo, VirtioNetDevice, VirtioPciLegacy,
    VirtqAvail, VirtqDesc, VirtqUsed,
};
use net_stack::dhcp::{self, DHCP_CLIENT_PORT, DHCP_SERVER_PORT};
use net_stack::wire::{fmt_ipv4, fmt_mac};
use net_stack::{Config, Event, Interface, Ipv4, SendError};

/// UDP port the kernel echoes datagrams on.
pub const UDP_ECHO_PORT: u16 = 7777;
/// UDP port for remote IPC calls (see `remote_ipc`).
pub const REMOTE_PORT: u16 = remote_ipc::KERNEL_REMOTE_PORT;
/// TCP port that echoes lines back to the peer.
pub const TCP_ECHO_PORT: u16 = 7779;
/// TCP port serving signed command lines (see `remote_ipc::line`).
pub const TCP_COMMAND_PORT: u16 = remote_ipc::line::KERNEL_COMMAND_PORT;
/// TCP port serving HTTP.
pub const HTTP_PORT: u16 = 8080;
/// Largest body `/bytes/N` will produce.
const MAX_GENERATED_BODY: usize = 1 << 20;

/// Live kernel numbers for the status page, supplied by the main loop.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemStatus {
    pub cpus_online: usize,
    pub cpus_total: u32,
    pub uptime_ticks: u64,
    pub heap_used: usize,
    pub heap_total: usize,
    pub frames: u64,
    pub presents: u64,
    pub storage: &'static str,
}

/// A body this connection is still streaming out.
#[derive(Clone, Copy, Default)]
struct HttpStream {
    remaining: usize,
    offset: usize,
    close_when_done: bool,
    /// Which connection this stream is for.
    ///
    /// Indexed by TCP slot, like `progress`, and for the same reason: the
    /// slot is reused as soon as `accept` finds it Closed. Without this, a
    /// client that aborted a large download handed its remaining body to
    /// whoever landed in its slot next -- and that client's own request was
    /// never answered, because `http_service` returns early while a stream
    /// is running, and its connection was closed when the stranger's stream
    /// finished.
    peer: net_stack::Ipv4,
    peer_port: u16,
}

/// How long a client may take to finish sending a request head or a command
/// line, in ticks (100 Hz), before the connection is reset.
///
/// The TCP reaper's rule is "a segment was exchanged", not "the request made
/// progress", so one byte a minute held a slot for ever. Eight slots serve
/// every port on this machine and six per port, so six dribbling sockets
/// took the whole machine off the network -- including the signed command
/// port -- with no authentication and almost no traffic. nginx and Apache
/// both carry a separate head deadline for exactly this.
const HTTP_HEAD_DEADLINE_TICKS: u64 = 1_000;

/// When a connection first had input it could not yet act on.
///
/// Indexed by TCP slot, so it must also remember *which* connection it is
/// about: the state used to survive a connection ending mid-request, and
/// the next client to land in that slot inherited a deadline that had
/// already expired -- reset on sight, before a tick of its own.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct ConnProgress {
    /// Tick at which the incomplete input first arrived, or 0 for idle.
    started: u64,
    /// The peer this was about, so a new occupant of the slot is noticed.
    peer_port: u16,
    peer: [u8; 4],
}

/// Formats into a fixed buffer; the HTTP path must not allocate, because it
/// runs with the network lock held.
struct FixedBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> FixedBuf<N> {
    fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl<const N: usize> Write for FixedBuf<N> {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let bytes = text.as_bytes();
        let room = N - self.len;
        let take = bytes.len().min(room);
        self.buf[self.len..self.len + take].copy_from_slice(&bytes[..take]);
        self.len += take;
        if take < bytes.len() {
            return Err(core::fmt::Error);
        }
        Ok(())
    }
}

/// A datagram received on `REMOTE_PORT`, handed to the kernel's remote
/// command server.
pub struct RemoteDatagram {
    pub src: Ipv4,
    pub src_port: u16,
    pub bytes: alloc::vec::Vec<u8>,
}

/// A request for the kernel's remote command server.
pub enum RemoteRequest {
    /// A `remote_ipc` envelope on `REMOTE_PORT`.
    Udp(RemoteDatagram),
    /// One signed line on a `TCP_COMMAND_PORT` connection.
    TcpLine {
        conn: usize,
        /// The connection the line arrived on, so the reply can be checked
        /// against it after the command has run.
        peer: net_stack::Ipv4,
        peer_port: u16,
        line: alloc::vec::Vec<u8>,
    },
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
    http_requests: u64,
    /// Requests closed for taking too long to send their head.
    http_timeouts: u64,
    http_bytes: u64,
    http_streams: [HttpStream; net_stack::tcp::MAX_CONNECTIONS],
    /// Per-slot deadline for input that has not yet formed a complete
    /// request head or command line.
    progress: [ConnProgress; net_stack::tcp::MAX_CONNECTIONS],
    /// Bytes of a declared HTTP body still owed before the next request on
    /// that connection may be parsed.
    http_owed: [usize; net_stack::tcp::MAX_CONNECTIONS],
    tcp_echoed_bytes: u64,
    tcp_accepted_seen: u64,
    /// How the address was obtained: "dhcp", "static", or "none".
    address_source: &'static str,
    lease_seconds: u32,
    dns: Option<Ipv4>,
    dhcp_server: Ipv4,
    lease_started_tick: u64,
    renewals: u32,
    /// The `fetch` or `resolve` under way (NET-033), and the lines it has
    /// for the Terminal.
    fetch: Option<Fetch>,
    fetch_lines: Vec<String>,
    /// The request under way is a Web card's (WEB-001): its lines are not
    /// the Terminal's, and its end is a `WebOutcome`.
    card_fetch: Option<u64>,
    /// The card's fetch under way has had its answer.
    card_answered: bool,
    web_outcomes: Vec<(u64, crate::web::WebOutcome)>,
    /// Requests waiting for the one under way (NET-034).
    queue: alloc::collections::VecDeque<Queued>,
}

// SAFETY: the stack is only ever driven from one CPU at a time, under the
// spinlock that holds it.
unsafe impl Send for NetStack {}

impl NetStack {
    /// Find a virtio-net PCI function, bring it up, and configure the
    /// interface with QEMU user-networking defaults.
    pub fn probe(boot: StorageBootInfo, remote_enabled: bool) -> Option<Self> {
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
        let mut iface = Interface::new(Config::unconfigured(mac));
        iface.bind(UDP_ECHO_PORT);
        if remote_enabled {
            iface.bind(REMOTE_PORT);
        }
        iface.bind(DHCP_CLIENT_PORT);
        iface.bind(DNS_CLIENT_PORT);
        let _ = iface.tcp_listen(TCP_ECHO_PORT);
        if remote_enabled {
            let _ = iface.tcp_listen(TCP_COMMAND_PORT);
        }
        let _ = iface.tcp_listen(HTTP_PORT);
        iface.tcp_mut().set_iss_source(crate::random::u32);
        Some(Self {
            device,
            iface,
            rx_frame: [0; MAX_FRAME_LEN],
            tx_frame: [0; MAX_FRAME_LEN],
            udp_echoed: 0,
            http_requests: 0,
            http_timeouts: 0,
            http_bytes: 0,
            http_streams: [HttpStream::default(); net_stack::tcp::MAX_CONNECTIONS],
            progress: [ConnProgress::default(); net_stack::tcp::MAX_CONNECTIONS],
            http_owed: [0; net_stack::tcp::MAX_CONNECTIONS],
            tcp_echoed_bytes: 0,
            tcp_accepted_seen: 0,
            address_source: "none",
            lease_seconds: 0,
            dhcp_server: [0; 4],
            lease_started_tick: 0,
            renewals: 0,
            dns: None,
            fetch: None,
            fetch_lines: Vec::new(),
            card_fetch: None,
            card_answered: false,
            web_outcomes: Vec::new(),
            queue: alloc::collections::VecDeque::new(),
        })
    }

    pub fn mac(&self) -> [u8; 6] {
        self.device.mac()
    }

    /// Obtain an address by DHCP (DISCOVER/OFFER/REQUEST/ACK) within
    /// `REPLY_TIMEOUT_TICKS`; on failure fall back to the QEMU user-network
    /// static configuration. Returns true when bound by DHCP.
    pub fn dhcp(&mut self, now: &dyn Fn() -> u64, log: &mut impl Write) -> bool {
        self.dhcp_exchange(false, now, log)
    }

    /// Renew the current lease with the server that granted it; on failure
    /// start over with a fresh DISCOVER. Returns true when bound.
    pub fn dhcp_renew(&mut self, now: &dyn Fn() -> u64, log: &mut impl Write) -> bool {
        if self.address_source != "dhcp" {
            return self.dhcp_exchange(false, now, log);
        }
        if self.dhcp_exchange(true, now, log) {
            return true;
        }
        let _ = writeln!(log, "net: dhcp renewal failed; rediscovering");
        self.dhcp_exchange(false, now, log)
    }

    /// Renew automatically once half the lease has elapsed (100 Hz ticks).
    ///
    /// The clock must be live. An earlier version captured the current tick
    /// into a constant closure, which made the reply-wait condition
    /// `now() - start < REPLY_TIMEOUT_TICKS` read `0 < 100` forever: with no
    /// DHCP server answering, the boot CPU span here until reset.
    fn maybe_renew(&mut self, clock: &dyn Fn() -> u64, log: &mut impl Write) {
        if self.address_source != "dhcp" || self.lease_seconds == 0 {
            return;
        }
        let now = clock();
        let half_lease_ticks = (self.lease_seconds as u64) * 100 / 2;
        if now.saturating_sub(self.lease_started_tick) >= half_lease_ticks {
            // Move the mark first so a failed attempt retries after another
            // half-lease rather than on every pass.
            self.lease_started_tick = now;
            let _ = self.dhcp_renew(clock, log);
        }
    }

    fn dhcp_exchange(&mut self, renew: bool, now: &dyn Fn() -> u64, log: &mut impl Write) -> bool {
        let mac = self.device.mac();
        let xid =
            (hal_x86_64::rdtsc() as u32) ^ u32::from_le_bytes([mac[2], mac[3], mac[4], mac[5]]);
        let mut client = dhcp::Client::new(mac, xid);
        let mut payload = [0u8; 400];
        if renew {
            let ip = self.iface.config().ip;
            let server = self.dhcp_server;
            let Some(len) = client.renew(ip, server, &mut payload) else {
                return false;
            };
            let request = payload;
            if !self.unicast_dhcp(server, &request[..len]) {
                return false;
            }
        } else {
            let Some(len) = client.discover(&mut payload) else {
                return self.dhcp_fallback(log, "discover build failed");
            };
            if !self.broadcast(&payload[..len]) {
                return self.dhcp_fallback(log, "discover transmit failed");
            }
        }
        let start = now();
        while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS {
            // The renewal path runs on every `service()` call and spins here
            // for up to a second with the network lock held. Keep TCP's
            // timers turning and its queued output moving, or established
            // connections go silent for the duration.
            self.keep_tcp_alive(now);
            while let Some(rx_len) = self.device.poll_receive(&mut self.rx_frame) {
                match self
                    .iface
                    .receive(&self.rx_frame[..rx_len], &mut self.tx_frame)
                {
                    Event::Transmit(n) => {
                        let _ = self.device.transmit(&self.tx_frame[..n]);
                    }
                    Event::Udp {
                        dst_port,
                        payload_offset,
                        payload_len,
                        ..
                    } if dst_port == DHCP_CLIENT_PORT => {
                        let reply = &self.rx_frame[payload_offset..payload_offset + payload_len];
                        match client.handle(reply, &mut payload) {
                            dhcp::Step::Send(n) => {
                                let request = payload;
                                if !self.broadcast(&request[..n]) {
                                    return self.dhcp_fallback(log, "request transmit failed");
                                }
                            }
                            dhcp::Step::Bound {
                                config,
                                lease_seconds,
                                dns,
                                server,
                            } => {
                                self.iface.set_config(config);
                                self.address_source = "dhcp";
                                self.lease_seconds = lease_seconds;
                                self.dns = dns;
                                self.dhcp_server = server;
                                self.lease_started_tick = now();
                                if renew {
                                    self.renewals += 1;
                                }
                                let _ = writeln!(
                                    log,
                                    "net: dhcp {} ip={} mask={} gw={} lease={}s server={}",
                                    if renew { "renewed" } else { "bound" },
                                    fmt_ipv4(config.ip),
                                    fmt_ipv4(config.netmask),
                                    fmt_ipv4(config.gateway),
                                    lease_seconds,
                                    fmt_ipv4(server)
                                );
                                return true;
                            }
                            dhcp::Step::None => {}
                        }
                    }
                    _ => {}
                }
            }
            core::hint::spin_loop();
        }
        if renew {
            let _ = writeln!(log, "net: dhcp renew: no reply");
            return false;
        }
        self.dhcp_fallback(log, "no reply")
    }

    /// Unicast a DHCP payload to the server (RENEWING).
    fn unicast_dhcp(&mut self, server: Ipv4, payload: &[u8]) -> bool {
        match self.iface.udp_send(
            server,
            DHCP_SERVER_PORT,
            DHCP_CLIENT_PORT,
            payload,
            &mut self.tx_frame,
        ) {
            Ok(len) => self.device.transmit(&self.tx_frame[..len]).is_ok(),
            Err(SendError::NeedArp) => {
                // Clear after transmitting, or the next caller that reads
                // `pending_frame_len` sends this frame's bytes again -- the
                // TCP flush does exactly that, emitting the first 60 bytes
                // of an unrelated frame.
                let len = self.iface.pending_frame_len();
                let _ = self.device.transmit(&self.tx_frame[..len]);
                self.iface.clear_pending_frame();
                false
            }
            Err(_) => false,
        }
    }

    fn broadcast(&mut self, payload: &[u8]) -> bool {
        match self.iface.udp_broadcast(
            DHCP_SERVER_PORT,
            DHCP_CLIENT_PORT,
            payload,
            &mut self.tx_frame,
        ) {
            Ok(len) => self.device.transmit(&self.tx_frame[..len]).is_ok(),
            Err(_) => false,
        }
    }

    fn dhcp_fallback(&mut self, log: &mut impl Write, why: &str) -> bool {
        let config = Config::qemu_user(self.device.mac());
        self.iface.set_config(config);
        self.address_source = "static";
        let _ = writeln!(
            log,
            "net: dhcp failed ({why}); using static ip={} gw={}",
            fmt_ipv4(config.ip),
            fmt_ipv4(config.gateway)
        );
        false
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
            "net: virtio-net-pci mac={} ip={} gw={} via {}",
            fmt_mac(cfg.mac),
            fmt_ipv4(cfg.ip),
            fmt_ipv4(cfg.gateway),
            self.address_source
        );
        if self.address_source == "dhcp" {
            let _ = writeln!(
                out,
                "net: lease={}s renewals={} server={} dns={}",
                self.lease_seconds,
                self.renewals,
                fmt_ipv4(self.dhcp_server),
                self.dns.map(fmt_ipv4).map_or_else(
                    || alloc::string::String::from("none"),
                    |d| alloc::format!("{d}")
                )
            );
        }
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
        let t = self.iface.tcp();
        let _ = writeln!(
            out,
            "net: tcp {}/{} conns={} accepted={} refused={} reaped={} in={} out={} rexmit={} rst={} echoed={}B",
            TCP_ECHO_PORT,
            TCP_COMMAND_PORT,
            t.connections().count(),
            t.accepted,
            t.refused,
            t.reaped,
            t.segments_in,
            t.segments_out,
            t.retransmits,
            t.resets_sent,
            self.tcp_echoed_bytes
        );
        // Which slots are occupied, and by what. A table full of connections
        // the peer already closed looks identical, from the counters alone,
        // to a table full of live ones.
        let mut line = FixedBuf::<192>::new();
        for (index, conn) in t.connections() {
            let _ = write!(line, " {index}:{}:{:?}", conn.local_port, conn.state);
        }
        let _ = writeln!(
            out,
            "net: tcp slots {}/{}{}",
            t.connections().count(),
            net_stack::tcp::MAX_CONNECTIONS,
            core::str::from_utf8(line.as_bytes()).unwrap_or("")
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
    pub fn service(
        &mut self,
        clock: &dyn Fn() -> u64,
        status: SystemStatus,
        log: &mut impl Write,
    ) -> Option<RemoteRequest> {
        self.iface.tcp_tick(clock());
        self.maybe_renew(clock, log);
        let mut remote = None;
        while remote.is_none() {
            let Some(len) = self.device.poll_receive(&mut self.rx_frame) else {
                break;
            };
            crate::random::stir(len as u64);
            match self
                .iface
                .receive(&self.rx_frame[..len], &mut self.tx_frame)
            {
                Event::Transmit(n) => {
                    let _ = self.device.transmit(&self.tx_frame[..n]);
                }
                Event::TcpReady { conn } => {
                    let port = self.iface.tcp().connection(conn).map(|c| c.local_port);
                    // A connection this machine opened is its fetch's
                    // (`advance_fetch` reads it), never a service's.
                    let outbound = self
                        .iface
                        .tcp()
                        .connection(conn)
                        .is_some_and(|c| c.outbound);
                    match port {
                        _ if outbound => {}
                        Some(HTTP_PORT) => self.http_service(conn, status, log),
                        Some(TCP_COMMAND_PORT) => {
                            if let Some(line) = self.tcp_command_service(conn, log) {
                                if let Some((peer, peer_port)) = self.peer_of(conn) {
                                    remote = Some(RemoteRequest::TcpLine {
                                        conn,
                                        peer,
                                        peer_port,
                                        line,
                                    });
                                }
                            }
                        }
                        _ => self.tcp_echo_service(conn, log),
                    }
                }
                Event::Udp {
                    src,
                    src_port,
                    dst_port,
                    payload_offset,
                    payload_len,
                } if dst_port == REMOTE_PORT => {
                    let payload = &self.rx_frame[payload_offset..payload_offset + payload_len];
                    remote = Some(RemoteRequest::Udp(RemoteDatagram {
                        src,
                        src_port,
                        bytes: payload.to_vec(),
                    }));
                }
                Event::Udp {
                    src,
                    src_port,
                    dst_port,
                    payload_offset,
                    payload_len,
                } if dst_port == DNS_CLIENT_PORT => {
                    let payload =
                        self.rx_frame[payload_offset..payload_offset + payload_len].to_vec();
                    self.dns_reply(src, src_port, &payload);
                }
                Event::Udp {
                    src,
                    src_port,
                    dst_port,
                    payload_offset,
                    payload_len,
                } if dst_port == UDP_ECHO_PORT => {
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
                            self.iface.clear_pending_frame();
                        }
                        Err(_) => {}
                    }
                }
                _ => {}
            }
        }
        // A client may have pipelined several requests into one segment.
        // Without this pass the second one waits for unrelated traffic to
        // arrive, because only a received frame produces `TcpReady`.
        if remote.is_none() {
            remote = self.buffered_command_line(log);
        }
        // The same pass for HTTP, which never had one. A `TcpReady` raised
        // while `service` is elsewhere -- inside the DHCP exchange's spin
        // loop, which can run for a second with the network lock held -- was
        // simply discarded. The client had sent its whole request and been
        // acknowledged, so it never retransmitted, and the request hung
        // until the 120-second idle reaper. That is F2 again, fixed for the
        // command port and never for this one.
        self.buffered_http(status, log);
        // The fetch under way: its request out, its window updates, its end.
        self.advance_fetch(clock());
        self.settle_card_fetch();
        self.pump_queue(clock());
        // Top up any response still streaming out, then push everything.
        self.pump_http();
        self.flush_tcp();
        remote
    }

    /// Serve one HTTP request on `conn`, if a complete head has arrived.
    fn http_service(&mut self, conn: usize, status: SystemStatus, log: &mut impl Write) {
        use net_stack::http::{self, Method, Parse};

        enum Route {
            Index,
            Health,
            Bytes(usize),
            NotFound,
            MethodNotAllowed,
        }
        enum Action {
            Wait,
            HeadTooLarge,
            Malformed,
            Unsupported,
            Serve {
                /// Head plus any declared body: everything to consume before
                /// the next request begins.
                head_len: usize,
                keep_alive: bool,
                method: Method,
                route: Route,
            },
        }

        // Whose request this slot is serving. `http_progress` learned this
        // in Phase 320 and `http_owed`, added by the same commit, did not --
        // so a client that declared a body and left handed its unpaid
        // remainder to whoever landed in the slot next, whose request was
        // then drained as body bytes and never answered. Six of those and
        // the server answers nobody.
        if !self.progress_matches_connection(conn) {
            self.http_owed[conn] = 0;
        }

        // Not while a declared body is still owed. Whatever arrives next on
        // this connection is that body, not a request.
        if self.http_owed[conn] > 0 {
            let owed = self.http_owed[conn];
            let taken = self.http_drain(conn, owed);
            self.http_owed[conn] = owed - taken;
            if self.http_owed[conn] > 0 {
                // The peer has gone, so the rest of the body never arrives:
                // let the slot go rather than holding it to a deadline that
                // cannot be met. Without this the abandoned connection kept
                // its own slot until the deadline, and six of them filled
                // the port.
                if self
                    .iface
                    .tcp()
                    .connection(conn)
                    .is_some_and(|c| c.peer_closed())
                {
                    self.http_owed[conn] = 0;
                    self.clear_progress(conn);
                    self.iface.tcp_mut().close(conn);
                    return;
                }
                // Still owed and still connected: a client that stops
                // mid-body is under the same deadline as an unfinished head.
                self.enforce_progress_deadline(conn);
                return;
            }
            self.clear_progress(conn);
        }

        // Not while a response is still streaming. `http_service` runs on
        // any segment, including the client's ACK of the headers, so a
        // pipelined request used to be answered *into the middle* of the
        // body already being written: the new status line landed inside the
        // declared Content-Length, and `http_begin_stream` overwrote the
        // stream state so the first body was truncated far below its
        // promised length. Leave the request buffered; `pump_http` will
        // finish, and the next segment re-enters here.
        if self.stream_belongs_to_connection(conn) {
            return;
        }

        // Decide while borrowing the receive buffer, act after releasing it.
        let action = {
            let Some(connection) = self.iface.tcp().connection(conn) else {
                return;
            };
            let buffered = connection.peek();
            match http::parse(buffered) {
                Parse::Incomplete if buffered.len() >= http::MAX_HEAD_BYTES => Action::HeadTooLarge,
                Parse::Incomplete => Action::Wait,
                Parse::Malformed => Action::Malformed,
                Parse::Unsupported => Action::Unsupported,
                Parse::Complete(request) => {
                    let path = request.path();
                    let route = if request.method == Method::Other {
                        Route::MethodNotAllowed
                    } else if path == "/" {
                        Route::Index
                    } else if path == "/health" {
                        Route::Health
                    } else if let Some(count) = path
                        .strip_prefix("/bytes/")
                        .and_then(|n| n.parse::<usize>().ok())
                    {
                        Route::Bytes(count.min(MAX_GENERATED_BODY))
                    } else {
                        Route::NotFound
                    };
                    Action::Serve {
                        // The declared body is drained with the head, or it
                        // stays in the buffer and is parsed as the next
                        // request -- one request producing two responses,
                        // and a partial body fusing with the next real
                        // request into a different method entirely.
                        head_len: request.head_len.saturating_add(request.body_len),
                        keep_alive: request.keep_alive,
                        method: request.method,
                        route,
                    }
                }
            }
        };

        let (head_len, keep_alive, method, route) = match action {
            Action::Wait => {
                // A head that never finishes gets a deadline of its own.
                if self.enforce_progress_deadline(conn) {
                    return;
                }
                // Nothing to parse yet. If the peer has also hung up, there
                // never will be: every HTTP client closes its keep-alive
                // connection this way, and a server that does not close its
                // own half leaves the slot in CloseWait until the idle
                // reaper. Eight slots serve every port, so a handful of
                // ordinary requests would take the whole machine off the
                // network.
                if self.http_streams[conn].remaining == 0
                    && self
                        .iface
                        .tcp()
                        .connection(conn)
                        .is_some_and(|c| c.peer_closed())
                {
                    self.iface.tcp_mut().close(conn);
                }
                return;
            }
            Action::HeadTooLarge => {
                self.clear_progress(conn);
                self.http_drain(conn, usize::MAX);
                self.http_respond(conn, 431, "text/plain", b"header too large\n", false);
                return;
            }
            Action::Malformed => {
                self.clear_progress(conn);
                self.http_drain(conn, usize::MAX);
                self.http_respond(conn, 400, "text/plain", b"bad request\n", false);
                return;
            }
            Action::Unsupported => {
                self.clear_progress(conn);
                self.http_drain(conn, usize::MAX);
                self.http_respond(conn, 501, "text/plain", b"not implemented\n", false);
                return;
            }
            Action::Serve {
                head_len,
                keep_alive,
                method,
                route,
            } => (head_len, keep_alive, method, route),
        };

        // Drain the head *and* any declared body. `http_drain` stops when
        // the buffer runs dry, and `parse` answers as soon as the blank line
        // arrives -- so a body split across segments was not consumed, and
        // arrived afterwards into an empty buffer where the next pass parsed
        // it as a request. That is H3 again for any body that is not already
        // buffered, which for a `Content-Length` above the 2 KiB receive
        // buffer is every body.
        let drained = self.http_drain(conn, head_len);
        // Only worth tracking when another request may follow. If this
        // response closes the connection there is no next request to
        // protect, and owing bytes on a closing connection kept its slot
        // alive to no purpose.
        self.http_owed[conn] = if keep_alive {
            head_len.saturating_sub(drained)
        } else {
            0
        };
        self.http_requests += 1;
        // This request finished; the next one starts its own clock.
        self.clear_progress(conn);

        // HEAD gets the headers a GET would get and none of the body. The
        // method was parsed and then used only to pick 405, so HEAD fell
        // through to the same writes as GET: `HEAD /bytes/200000` answered
        // with `Content-Length: 200000` *and* 200,000 bytes, which a
        // conformant client treats as the start of the next response.
        // `curl -I /health` never noticed, because it discards three spare
        // bytes without complaint.
        let body_allowed = method != Method::Head;

        match route {
            Route::Index => {
                let mut body = FixedBuf::<1600>::new();
                write_status_page(&mut body, status);
                self.http_respond_maybe_body(
                    conn,
                    200,
                    "text/html; charset=utf-8",
                    body.as_bytes(),
                    keep_alive,
                    body_allowed,
                );
            }
            Route::Health => self.http_respond_maybe_body(
                conn,
                200,
                "text/plain",
                b"ok\n",
                keep_alive,
                body_allowed,
            ),
            Route::Bytes(count) => {
                let _ = writeln!(log, "net: http /bytes/{count} on conn{conn}");
                if body_allowed {
                    self.http_begin_stream(conn, count, keep_alive);
                } else {
                    // Headers describing the body a GET would send.
                    self.http_head_only(conn, 200, "application/octet-stream", count, keep_alive);
                }
            }
            Route::NotFound => self.http_respond_maybe_body(
                conn,
                404,
                "text/plain",
                b"not found\n",
                false,
                body_allowed,
            ),
            Route::MethodNotAllowed => {
                self.http_respond(conn, 405, "text/plain", b"method not allowed\n", false)
            }
        }
    }

    /// Status line and headers only, with `Content-Length` describing the
    /// body a GET would have sent. For HEAD.
    fn http_head_only(
        &mut self,
        conn: usize,
        status: u16,
        content_type: &str,
        body_len: usize,
        keep_alive: bool,
    ) {
        let mut head = [0u8; 256];
        let Some(head_len) =
            net_stack::http::write_headers(&mut head, status, content_type, body_len, keep_alive)
        else {
            return;
        };
        if !self.http_write_whole(conn, &head[..head_len], &[]) {
            self.iface.tcp_mut().close(conn);
            return;
        }
        if !keep_alive {
            self.iface.tcp_mut().close(conn);
        }
    }

    /// As `http_respond`, but omits the body when the method forbids one.
    fn http_respond_maybe_body(
        &mut self,
        conn: usize,
        status: u16,
        content_type: &str,
        body: &[u8],
        keep_alive: bool,
        body_allowed: bool,
    ) {
        if body_allowed {
            self.http_respond(conn, status, content_type, body, keep_alive);
        } else {
            self.http_head_only(conn, status, content_type, body.len(), keep_alive);
        }
    }

    /// Consume up to `count` buffered bytes; returns how many were consumed.
    fn http_drain(&mut self, conn: usize, count: usize) -> usize {
        let mut sink = [0u8; 256];
        let mut left = count;
        while left > 0 {
            let take = left.min(sink.len());
            let got = self.iface.tcp_mut().read(conn, &mut sink[..take]);
            if got == 0 {
                break;
            }
            left -= got;
        }
        count - left
    }

    /// Write a complete response, or none of it.
    ///
    /// `tcp::write` returns how many bytes fitted and silently drops the
    /// rest, and every one of these responders ignored the return. The send
    /// buffer is 2048 bytes and the index page is 842 bytes of body under
    /// 122 bytes of headers, so three pipelined `GET /` requests queue 964
    /// twice and then find 120 bytes of room: the third response's own
    /// status line was cut in half and its body never entered the buffer at
    /// all. The client is left holding a partial header with no terminator
    /// and hangs until the 120-second idle reaper.
    ///
    /// A truncated response cannot be repaired, so refuse to start one. The
    /// caller closes the connection instead, which a client can see and act
    /// on. `tcp_reply` on the command port already checked its write; this
    /// is the sibling three functions away that did not.
    fn http_write_whole(&mut self, conn: usize, head: &[u8], body: &[u8]) -> bool {
        let room = match self.iface.tcp().connection(conn) {
            Some(connection) => connection.writable(),
            None => return false,
        };
        if room < head.len() + body.len() {
            return false;
        }
        let wrote_head = self.iface.tcp_mut().write(conn, head);
        let wrote_body = self.iface.tcp_mut().write(conn, body);
        wrote_head == head.len() && wrote_body == body.len()
    }

    /// Headers plus a body small enough to hand over in one go.
    fn http_respond(
        &mut self,
        conn: usize,
        status: u16,
        content_type: &str,
        body: &[u8],
        keep_alive: bool,
    ) {
        let mut head = [0u8; 256];
        let Some(head_len) =
            net_stack::http::write_headers(&mut head, status, content_type, body.len(), keep_alive)
        else {
            return;
        };
        if !self.http_write_whole(conn, &head[..head_len], body) {
            // No room for the whole thing. Better a connection the client
            // sees end than a header it waits on for two minutes.
            self.iface.tcp_mut().close(conn);
            return;
        }
        self.http_bytes += body.len() as u64;
        if !keep_alive {
            self.iface.tcp_mut().close(conn);
        }
    }

    /// Headers for a body that will be produced over several passes.
    fn http_begin_stream(&mut self, conn: usize, count: usize, keep_alive: bool) {
        let mut head = [0u8; 256];
        let Some(head_len) = net_stack::http::write_headers(
            &mut head,
            200,
            "application/octet-stream",
            count,
            keep_alive,
        ) else {
            return;
        };
        if !self.http_write_whole(conn, &head[..head_len], &[]) {
            self.iface.tcp_mut().close(conn);
            return;
        }
        let (peer, peer_port) = match self.iface.tcp().connection(conn) {
            Some(connection) => (connection.peer, connection.peer_port),
            None => return,
        };
        self.http_streams[conn] = HttpStream {
            remaining: count,
            offset: 0,
            close_when_done: !keep_alive,
            peer,
            peer_port,
        };
    }

    /// Feed every streaming response as much as its send buffer will take.
    fn pump_http(&mut self) {
        for conn in 0..self.http_streams.len() {
            if self.http_streams[conn].remaining == 0 {
                continue;
            }
            if !self.stream_belongs_to_connection(conn) {
                continue;
            }
            let Some(connection) = self.iface.tcp().connection(conn) else {
                self.http_streams[conn] = HttpStream::default();
                continue;
            };
            let room = connection.writable();
            if room == 0 {
                continue;
            }
            let stream = self.http_streams[conn];
            let take = stream.remaining.min(room).min(512);
            let mut chunk = [0u8; 512];
            for (i, slot) in chunk.iter_mut().take(take).enumerate() {
                // A recognisable, position-dependent pattern, so a client
                // can tell truncation from corruption.
                *slot = BODY_PATTERN[(stream.offset + i) % BODY_PATTERN.len()];
            }
            let wrote = self.iface.tcp_mut().write(conn, &chunk[..take]);
            self.http_streams[conn].remaining -= wrote;
            self.http_streams[conn].offset += wrote;
            self.http_bytes += wrote as u64;
            if self.http_streams[conn].remaining == 0 {
                if stream.close_when_done {
                    self.iface.tcp_mut().close(conn);
                }
                self.http_streams[conn] = HttpStream::default();
            }
        }
    }

    /// A complete command line already sitting in some command-port
    /// connection's receive buffer.
    /// Reset a connection that has had input it cannot act on for longer
    /// than `HTTP_HEAD_DEADLINE_TICKS`. Returns true when it did.
    ///
    /// TCP's reaper asks whether a *segment* arrived, not whether the
    /// request made progress, so one byte every so often looked perfectly
    /// alive and held a slot for the full 120-second idle timeout. Eight
    /// slots serve every port and six per port, so a handful of dribbling
    /// sockets took the whole machine off the network at almost no traffic.
    ///
    /// Phase 311 gave this to the HTTP port only. The signed command port
    /// waits for a newline and had no deadline of its own, so the same
    /// attack aimed one port over still worked.
    fn enforce_progress_deadline(&mut self, conn: usize) -> bool {
        let Some(connection) = self.iface.tcp().connection(conn) else {
            self.progress[conn] = ConnProgress::default();
            return false;
        };
        let now = self.iface.tcp().now();
        let here = ConnProgress {
            started: 0,
            peer_port: connection.peer_port,
            peer: connection.peer,
        };
        let tracked = self.progress[conn];
        if tracked.peer_port != here.peer_port || tracked.peer != here.peer {
            // A different connection in this slot: start its own clock.
            self.progress[conn] = ConnProgress {
                started: now.max(1),
                ..here
            };
            return false;
        }
        if tracked.started == 0 {
            self.progress[conn] = ConnProgress {
                started: now.max(1),
                ..here
            };
            return false;
        }
        if now.saturating_sub(tracked.started) < HTTP_HEAD_DEADLINE_TICKS {
            return false;
        }
        self.http_timeouts += 1;
        self.progress[conn] = ConnProgress::default();
        // `abort`, not `close`: a polite FIN leaves the slot in FinWait for
        // the closing timeout, so the client that would not finish goes on
        // holding it, which is the whole attack.
        self.iface.tcp_mut().abort(conn);
        true
    }

    /// Mark this connection as having made progress.
    fn clear_progress(&mut self, conn: usize) {
        self.progress[conn] = ConnProgress::default();
        if let Some(connection) = self.iface.tcp().connection(conn) {
            // Keep the identity, so per-slot state stays attributable to the
            // connection it belongs to.
            self.progress[conn].peer_port = connection.peer_port;
            self.progress[conn].peer = connection.peer;
        }
    }

    /// Whether the per-slot state belongs to the connection in that slot.
    ///
    /// Anything indexed by TCP slot has to ask this: the slot is reused as
    /// soon as `accept` finds it Closed, and state left behind by the
    /// previous occupant is state applied to a stranger.
    fn progress_matches_connection(&self, conn: usize) -> bool {
        match self.iface.tcp().connection(conn) {
            Some(connection) => {
                self.progress[conn].peer_port == connection.peer_port
                    && self.progress[conn].peer == connection.peer
            }
            None => false,
        }
    }

    /// Who is on the other end of `conn`, if anyone still is.
    fn peer_of(&self, conn: usize) -> Option<(net_stack::Ipv4, u16)> {
        self.iface
            .tcp()
            .connection(conn)
            .map(|connection| (connection.peer, connection.peer_port))
    }

    /// Whether the streaming response in `conn`'s slot belongs to the
    /// connection now in it, clearing it when it does not.
    ///
    /// `progress_matches_connection`'s doc says anything indexed by TCP slot
    /// has to ask. The stream state was the sibling that did not.
    fn stream_belongs_to_connection(&mut self, conn: usize) -> bool {
        if self.http_streams[conn].remaining == 0 {
            return false;
        }
        let matches = match self.iface.tcp().connection(conn) {
            Some(connection) => {
                self.http_streams[conn].peer_port == connection.peer_port
                    && self.http_streams[conn].peer == connection.peer
            }
            None => false,
        };
        if !matches {
            self.http_streams[conn] = HttpStream::default();
        }
        matches
    }

    /// Serve any HTTP connection with bytes already buffered.
    ///
    /// Only a received frame produces `TcpReady`, and a `TcpReady` raised
    /// while the kernel is busy elsewhere is lost, so without this sweep a
    /// complete request could sit acknowledged and unanswered until the idle
    /// reaper closed the connection.
    fn buffered_http(&mut self, status: SystemStatus, log: &mut impl Write) {
        let mut pending = [usize::MAX; net_stack::tcp::MAX_CONNECTIONS];
        let mut count = 0;
        let mut finished = [usize::MAX; net_stack::tcp::MAX_CONNECTIONS];
        let mut done = 0;
        for (index, conn) in self.iface.tcp().connections() {
            if conn.local_port != HTTP_PORT {
                continue;
            }
            if conn.readable() > 0 {
                if count < pending.len() {
                    pending[count] = index;
                    count += 1;
                }
            } else if conn.peer_closed() && done < finished.len() {
                // The peer has gone and there is nothing left to read, so
                // nothing more will ever arrive on this connection. Closing
                // our half is what releases the slot.
                //
                // `http_service` does this too, but only on paths that reach
                // its `Wait` arm -- and the declared-body check added in
                // Phase 320 returns before it, so a keep-alive request that
                // promised a body and then hung up sat in CloseWait holding
                // a slot until the idle reaper. Six of those fill the port.
                // A sweep rather than an event, so no early return can skip
                // it.
                finished[done] = index;
                done += 1;
            }
        }
        for &index in &pending[..count] {
            self.http_service(index, status, log);
        }
        for &index in &finished[..done] {
            self.http_owed[index] = 0;
            self.clear_progress(index);
            self.iface.tcp_mut().close(index);
        }
    }

    fn buffered_command_line(&mut self, log: &mut impl Write) -> Option<RemoteRequest> {
        let mut pending = [usize::MAX; net_stack::tcp::MAX_CONNECTIONS];
        let mut count = 0;
        for (index, conn) in self.iface.tcp().connections() {
            if conn.local_port == TCP_COMMAND_PORT && conn.readable() > 0 && count < pending.len() {
                pending[count] = index;
                count += 1;
            }
        }
        for &index in &pending[..count] {
            if let Some(line) = self.tcp_command_service(index, log) {
                let (peer, peer_port) = self.peer_of(index)?;
                return Some(RemoteRequest::TcpLine {
                    conn: index,
                    peer,
                    peer_port,
                    line,
                });
            }
        }
        None
    }

    /// Command port: hand one complete line to the caller; close after the
    /// peer does.
    fn tcp_command_service(
        &mut self,
        conn: usize,
        log: &mut impl Write,
    ) -> Option<alloc::vec::Vec<u8>> {
        self.log_new_connection(conn, log);
        let mut line = [0u8; 1024];
        let taken = self.iface.tcp_mut().read_line(conn, &mut line);
        match taken {
            // A complete line: this connection is making progress.
            Some(_) => self.clear_progress(conn),
            None => {
                // A line that never ends. `read_line` only yields at a
                // newline or a full 2 KiB buffer, so six connections
                // dribbling a byte a minute held every slot on the signed
                // command port for ever -- unauthenticated, at almost no
                // traffic. Phase 311 gave the HTTP port a deadline for
                // exactly this and left the command port without one.
                if self.enforce_progress_deadline(conn) {
                    return None;
                }
            }
        }
        if self
            .iface
            .tcp()
            .connection(conn)
            .is_some_and(|c| c.peer_closed() && c.readable() == 0)
        {
            self.iface.tcp_mut().close(conn);
        }
        taken.map(|n| line[..n].to_vec())
    }

    /// Send a reply line on a command connection.
    /// Reply only if `conn` still holds the connection that asked.
    ///
    /// A command's reply target is a slot index held across the command's
    /// execution -- up to `RemoteCommandServer::TIMEOUT_TICKS`. If the
    /// authenticated caller's connection goes away inside that window and
    /// the slot is reused, the output of a *signed, privileged* command was
    /// written to whoever landed in the slot, on any port, unauthenticated.
    pub fn tcp_reply_to(
        &mut self,
        conn: usize,
        peer: net_stack::Ipv4,
        peer_port: u16,
        line: &[u8],
    ) -> bool {
        match self.iface.tcp().connection(conn) {
            Some(connection) if connection.peer == peer && connection.peer_port == peer_port => {}
            _ => return false,
        }
        self.tcp_reply(conn, line)
    }

    pub fn tcp_reply(&mut self, conn: usize, line: &[u8]) -> bool {
        let written = self.iface.tcp_mut().write(conn, line);
        let ok = written == line.len() && self.iface.tcp_mut().write(conn, b"\n") == 1;
        self.flush_tcp();
        ok
    }

    /// Pending command lines on other connections are picked up by later
    /// `service` calls; nothing else to do here.
    fn log_new_connection(&mut self, conn: usize, log: &mut impl Write) {
        let accepted = self.iface.tcp().accepted;
        if accepted != self.tcp_accepted_seen {
            self.tcp_accepted_seen = accepted;
            if let Some(c) = self.iface.tcp().connection(conn) {
                let _ = writeln!(
                    log,
                    "net: tcp conn{conn} from {}:{} port {} ({:?})",
                    fmt_ipv4(c.peer),
                    c.peer_port,
                    c.local_port,
                    c.state
                );
            }
        }
    }

    /// Echo service on `TCP_ECHO_PORT`: whatever arrives is sent back; when
    /// the peer closes, so do we.
    fn tcp_echo_service(&mut self, conn: usize, log: &mut impl Write) {
        let accepted = self.iface.tcp().accepted;
        if accepted != self.tcp_accepted_seen {
            self.tcp_accepted_seen = accepted;
            if let Some(c) = self.iface.tcp().connection(conn) {
                let _ = writeln!(
                    log,
                    "net: tcp conn{conn} from {}:{} ({:?})",
                    fmt_ipv4(c.peer),
                    c.peer_port,
                    c.state
                );
            }
        }
        let mut chunk = [0u8; 512];
        loop {
            let n = self.iface.tcp_mut().read(conn, &mut chunk);
            if n == 0 {
                break;
            }
            let written = self.iface.tcp_mut().write(conn, &chunk[..n]);
            self.tcp_echoed_bytes += written as u64;
        }
        if self
            .iface
            .tcp()
            .connection(conn)
            .is_some_and(|c| c.peer_closed())
        {
            self.iface.tcp_mut().close(conn);
        }
    }

    /// Transmit every pending TCP segment.
    fn flush_tcp(&mut self) {
        loop {
            match self.iface.tcp_next_frame(&mut self.tx_frame) {
                Some(n) => {
                    if self.device.transmit(&self.tx_frame[..n]).is_err() {
                        break;
                    }
                }
                None => {
                    // `None` may mean "nothing to send" or "I wrote an ARP
                    // request into the buffer because I could not resolve a
                    // next hop". The UDP and ping paths all transmit that
                    // frame; this one dropped it on the floor, so the fix
                    // that was meant to drive resolution never put a single
                    // request on the wire and recovery still depended on the
                    // gateway ARPing us first.
                    let pending = self.iface.pending_frame_len();
                    if pending > 0 {
                        let _ = self.device.transmit(&self.tx_frame[..pending]);
                        self.iface.clear_pending_frame();
                    }
                    break;
                }
            }
        }
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
                    self.iface.clear_pending_frame();
                    let hop = self.iface.config().next_hop(target);
                    let start = now();
                    while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS
                        && self.iface.arp_cache().lookup(hop).is_none()
                    {
                        self.keep_tcp_alive(now);
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
    /// Keep the rest of the stack alive while something else is waiting.
    ///
    /// `ping` and the DHCP exchange both spin for up to a second at a time
    /// with the network lock held, and their wait loops drained TCP events
    /// and threw them away. Nothing was lost -- the data stays in the
    /// receive buffer and the buffered sweeps pick it up -- but for those
    /// seconds no TCP timer advanced and nothing queued was transmitted, so
    /// `net ping` to an unreachable address took the whole stack off the air
    /// for up to six seconds and connections went quiet long enough to look
    /// dead.
    fn keep_tcp_alive(&mut self, now: &dyn Fn() -> u64) {
        self.iface.tcp_tick(now());
        self.flush_tcp();
    }

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
                        self.keep_tcp_alive(now);
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
                    let sent = self.device.transmit(&self.tx_frame[..len]);
                    self.iface.clear_pending_frame();
                    if sent.is_err() {
                        let _ = writeln!(out, "net: arp transmit failed");
                        return false;
                    }
                    let hop = self.iface.config().next_hop(target);
                    let start = now();
                    while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS
                        && self.iface.arp_cache().lookup(hop).is_none()
                    {
                        // SC3's fix reached three of the four spin loops.
                        // `poll_for_echo` discards every `TcpReady` it sees,
                        // so without this the stack has no timer and no
                        // flush for up to three attempts of
                        // `REPLY_TIMEOUT_TICKS` -- three seconds on a cold
                        // ARP cache.
                        self.keep_tcp_alive(now);
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
                Event::Udp { .. } | Event::TcpReady { .. } | Event::None => {}
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

/// A recognisable, position-dependent body so a client can tell truncation
/// from corruption.
const BODY_PATTERN: &[u8; 16] = b"PandaGen-stream\n";

/// The HTML status page. Kept under the TCP send buffer so it goes out in
/// one piece.
fn write_status_page(out: &mut impl Write, status: SystemStatus) {
    let seconds = status.uptime_ticks / 100;
    let _ = write!(
        out,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<title>PandaGen</title><style>\
:root{{color-scheme:dark}}body{{background:#0f1720;color:#d7e3ef;\
font:14px/1.6 ui-monospace,Menlo,Consolas,monospace;margin:0;padding:2rem}}\
h1{{font-size:1.1rem;letter-spacing:.08em;text-transform:uppercase;\
color:#7fd6c2;margin:0 0 1.2rem}}table{{border-collapse:collapse}}\
td{{padding:.2rem 1.4rem .2rem 0;vertical-align:top}}\
td:first-child{{color:#7f93a8}}a{{color:#7fd6c2}}\
</style></head><body><h1>PandaGen</h1><table>\
<tr><td>uptime</td><td>{seconds} s</td></tr>\
<tr><td>cpus</td><td>{} of {} online</td></tr>\
<tr><td>heap</td><td>{} of {} KiB used</td></tr>\
<tr><td>frames</td><td>{} rendered, {} presented</td></tr>\
<tr><td>storage</td><td>{}</td></tr>\
</table><p><a href=\"/health\">/health</a> &middot; \
<a href=\"/bytes/65536\">/bytes/65536</a></p></body></html>",
        status.cpus_online,
        status.cpus_total,
        status.heap_used / 1024,
        status.heap_total / 1024,
        status.frames,
        status.presents,
        status.storage,
    );
}

// ---- Asking the network (NET-033): `resolve` and `fetch` from the Terminal. ----

/// The UDP port DNS queries go out from.
pub const DNS_CLIENT_PORT: u16 = 53053;
/// QEMU user networking's resolver, when DHCP named none.
const QEMU_DNS: Ipv4 = [10, 0, 2, 3];
/// Most of a response kept; the rest is counted, not stored.
const FETCH_MAX_BYTES: usize = 64 * 1024;
/// How long a fetch may take, all told (10 s).
const FETCH_TIMEOUT_TICKS: u64 = 1000;
/// A DNS query is asked again after this long, `DNS_TRIES` times in all.
const DNS_RETRY_TICKS: u64 = 100;
const DNS_TRIES: u8 = 3;
/// Lines of a body shown, and characters of each.
const FETCH_LINES: usize = 40;
const FETCH_LINE_CHARS: usize = 100;

enum FetchStage {
    /// A query is out to `server`, or waits on its next hop's address.
    Resolving {
        server: Ipv4,
        server_port: u16,
        id: u16,
        /// When the query last went out; `None` until it has.
        sent_at: Option<u64>,
        tries: u8,
    },
    Connecting {
        conn: usize,
    },
    Receiving {
        conn: usize,
    },
}

/// Requests that may wait behind the one under way (NET-034).
const QUEUE_MAX: usize = 8;

/// A request waiting its turn.
enum Queued {
    Fetch {
        url: String,
        /// The Web card request it answers, or `None` for the Terminal.
        card: Option<u64>,
    },
    Resolve {
        name: String,
        server: Option<(Ipv4, u16)>,
    },
}

/// The one request the machine has out (NET-033).
pub struct Fetch {
    /// `resolve`: say the address and stop.
    resolve_only: bool,
    host: String,
    port: u16,
    path: String,
    stage: FetchStage,
    started: u64,
    data: Vec<u8>,
    /// Bytes past `FETCH_MAX_BYTES`, counted and dropped.
    dropped: usize,
}

impl NetStack {
    /// Whether a `fetch` or `resolve` is under way.
    pub fn fetch_busy(&self) -> bool {
        self.fetch.is_some()
    }

    /// Look `name` up, through `server` or the resolver DHCP named. The
    /// answer arrives as a line from `take_fetch_lines`; behind another
    /// request, it waits its turn (NET-034).
    pub fn start_resolve(
        &mut self,
        name: &str,
        server: Option<(Ipv4, u16)>,
        now: u64,
    ) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err(String::from("resolve: which name?"));
        }
        self.enqueue(
            Queued::Resolve {
                name: String::from(name),
                server,
            },
            now,
        )
    }

    /// Fetch `url` over HTTP. Progress and the response arrive as lines
    /// from `take_fetch_lines`; behind another request, it waits its turn.
    pub fn start_fetch(&mut self, url: &str, now: u64) -> Result<(), String> {
        Self::check_url(url)?;
        self.enqueue(
            Queued::Fetch {
                url: String::from(url),
                card: None,
            },
            now,
        )
    }

    /// Fetch `url` for Web card request `card` (WEB-001): the end comes
    /// from `take_web_outcomes`, not as lines.
    pub fn start_web_fetch(&mut self, url: &str, card: u64, now: u64) -> Result<(), String> {
        Self::check_url(url)?;
        self.enqueue(
            Queued::Fetch {
                url: String::from(url),
                card: Some(card),
            },
            now,
        )
    }

    fn check_url(url: &str) -> Result<net_stack::http::Url<'_>, String> {
        use net_stack::http::{parse_url, UrlError};
        parse_url(url).map_err(|e| {
            String::from(match e {
                UrlError::NotHttp => "fetch: only http:// -- there is no TLS here yet",
                UrlError::Malformed => "fetch: that is not a URL (http://host[:port]/path)",
            })
        })
    }

    /// Wait behind the request under way, or start at once.
    fn enqueue(&mut self, request: Queued, now: u64) -> Result<(), String> {
        if self.queue.len() >= QUEUE_MAX {
            return Err(String::from("net: too many requests waiting"));
        }
        self.queue.push_back(request);
        self.pump_queue(now);
        Ok(())
    }

    /// Start waiting requests while none is under way.
    fn pump_queue(&mut self, now: u64) {
        while self.fetch.is_none() && self.card_fetch.is_none() {
            let Some(request) = self.queue.pop_front() else {
                return;
            };
            self.start_now(request, now);
        }
    }

    /// Start `request` now. A failure to start is its answer: a line for
    /// the Terminal, or the card's outcome.
    fn start_now(&mut self, request: Queued, now: u64) {
        let blank = |host: String, port: u16, path: String, resolve_only: bool| Fetch {
            resolve_only,
            host,
            port,
            path,
            stage: FetchStage::Connecting { conn: usize::MAX },
            started: now,
            data: Vec::new(),
            dropped: 0,
        };
        match request {
            Queued::Resolve { name, server } => {
                if let Err(why) = self.begin(blank(name, 0, String::new(), true), server, now) {
                    self.fetch_lines.push(why);
                }
            }
            Queued::Fetch { url, card } => {
                let fetch = match Self::check_url(&url) {
                    Ok(u) => blank(String::from(u.host), u.port, String::from(u.path), false),
                    Err(why) => {
                        self.fetch_lines.push(why);
                        return;
                    }
                };
                if let Some(card) = card {
                    self.fetch_lines.clear();
                    self.card_fetch = Some(card);
                    self.card_answered = false;
                }
                if let Err(why) = self.begin(fetch, None, now) {
                    self.fetch_lines.push(why);
                }
                self.settle_card_fetch();
            }
        }
    }

    fn begin(
        &mut self,
        mut fetch: Fetch,
        server: Option<(Ipv4, u16)>,
        now: u64,
    ) -> Result<(), String> {
        if !self.iface.config().is_configured() {
            return Err(String::from("net: no address yet"));
        }
        match net_stack::wire::parse_ipv4(&fetch.host) {
            // An address needs no looking up.
            Some(addr) if fetch.resolve_only => {
                self.fetch_lines.push(alloc::format!(
                    "resolve: {} is {}",
                    fetch.host,
                    fmt_ipv4(addr)
                ));
                return Ok(());
            }
            Some(addr) => self.connect_for(&mut fetch, addr)?,
            None => {
                if !self.iface.is_bound(DNS_CLIENT_PORT) && !self.iface.bind(DNS_CLIENT_PORT) {
                    return Err(String::from("net: no UDP port free for DNS"));
                }
                let (server, server_port) =
                    server.unwrap_or((self.dns.unwrap_or(QEMU_DNS), net_stack::dns::PORT));
                // An id no one watching the wire could guess at a glance.
                // A random id (SEC-030): a guessable one lets anyone on the
                // path answer first with an address of their choosing.
                let _ = now;
                let id = crate::random::u16();
                fetch.stage = FetchStage::Resolving {
                    server,
                    server_port,
                    id,
                    sent_at: None,
                    tries: 0,
                };
            }
        }
        self.fetch = Some(fetch);
        self.advance_fetch(now);
        Ok(())
    }

    /// Open the connection for `fetch` to `addr`.
    fn connect_for(&mut self, fetch: &mut Fetch, addr: Ipv4) -> Result<(), String> {
        let conn = self
            .iface
            .tcp_mut()
            .connect(addr, fetch.port)
            .ok_or_else(|| String::from("net: no connection free"))?;
        fetch.stage = FetchStage::Connecting { conn };
        Ok(())
    }

    /// Lines for the Terminal from the request under way. None while a
    /// Web card's is: those are the card's.
    pub fn take_fetch_lines(&mut self) -> Vec<String> {
        if self.card_fetch.is_some() {
            return Vec::new();
        }
        core::mem::take(&mut self.fetch_lines)
    }

    /// The Web cards' fetches that have ended, by the request each was
    /// started with.
    pub fn take_web_outcomes(&mut self) -> Vec<(u64, crate::web::WebOutcome)> {
        core::mem::take(&mut self.web_outcomes)
    }

    /// A card's fetch that has ended without a response ended with its
    /// last line: that is the card's answer.
    fn settle_card_fetch(&mut self) {
        let Some(card) = self.card_fetch else {
            return;
        };
        if self.fetch.is_some() {
            return;
        }
        self.card_fetch = None;
        let lines = core::mem::take(&mut self.fetch_lines);
        if !self.card_answered {
            let why = lines
                .into_iter()
                .last()
                .unwrap_or_else(|| String::from("The request ended with no answer"));
            self.web_outcomes.push((card, Err(why)));
        }
        self.card_answered = false;
    }

    /// A datagram on `DNS_CLIENT_PORT`: the answer, if it is ours.
    fn dns_reply(&mut self, src: Ipv4, src_port: u16, payload: &[u8]) {
        let Some(mut fetch) = self.fetch.take() else {
            return;
        };
        let FetchStage::Resolving {
            server,
            server_port,
            id,
            ..
        } = fetch.stage
        else {
            self.fetch = Some(fetch);
            return;
        };
        // Only the resolver we asked may answer.
        if src != server || src_port != server_port {
            self.fetch = Some(fetch);
            return;
        }
        use net_stack::dns::{parse_answer, DnsError};
        match parse_answer(id, &fetch.host, payload) {
            Ok(addr) if fetch.resolve_only => {
                self.fetch_lines.push(alloc::format!(
                    "resolve: {} is {}",
                    fetch.host,
                    fmt_ipv4(addr)
                ));
            }
            Ok(addr) => {
                self.fetch_lines.push(alloc::format!(
                    "fetch: {} is {}",
                    fetch.host,
                    fmt_ipv4(addr)
                ));
                match self.connect_for(&mut fetch, addr) {
                    Ok(()) => self.fetch = Some(fetch),
                    Err(why) => self.fetch_lines.push(why),
                }
            }
            // Someone else's, or a stale reply: keep waiting.
            Err(DnsError::NotOurs) => self.fetch = Some(fetch),
            Err(why) => {
                let said = match why {
                    DnsError::NoSuchName => "no such name",
                    DnsError::NoAddress => "the name has no IPv4 address",
                    DnsError::ServerFailure(_) => "the resolver failed",
                    _ => "the answer made no sense",
                };
                let verb = if fetch.resolve_only {
                    "resolve"
                } else {
                    "fetch"
                };
                self.fetch_lines
                    .push(alloc::format!("{verb}: {}: {said}", fetch.host));
            }
        }
    }

    /// Move the request under way along: send, resend, read, finish.
    fn advance_fetch(&mut self, now: u64) {
        let Some(mut fetch) = self.fetch.take() else {
            return;
        };
        let verb = if fetch.resolve_only {
            "resolve"
        } else {
            "fetch"
        };
        if now.saturating_sub(fetch.started) > FETCH_TIMEOUT_TICKS {
            if let FetchStage::Receiving { conn } | FetchStage::Connecting { conn } = fetch.stage {
                if conn != usize::MAX {
                    self.iface.tcp_mut().abort(conn);
                }
            }
            if matches!(fetch.stage, FetchStage::Receiving { .. }) && !fetch.data.is_empty() {
                self.fetch_lines
                    .push(String::from("fetch: timed out; what arrived:"));
                self.report(&fetch);
            } else {
                self.fetch_lines
                    .push(alloc::format!("{verb}: {}: no answer in 10 s", fetch.host));
            }
            return;
        }
        match fetch.stage {
            FetchStage::Resolving {
                server,
                server_port,
                id,
                sent_at,
                tries,
            } => {
                let due = sent_at.is_none_or(|at| now.saturating_sub(at) >= DNS_RETRY_TICKS);
                if due {
                    if tries >= DNS_TRIES {
                        self.fetch_lines.push(alloc::format!(
                            "{verb}: {}: {} did not answer",
                            fetch.host,
                            fmt_ipv4(server)
                        ));
                        return;
                    }
                    let mut query = [0u8; 300];
                    let Ok(len) = net_stack::dns::build_query(id, &fetch.host, &mut query) else {
                        self.fetch_lines
                            .push(alloc::format!("{verb}: {}: not a name", fetch.host));
                        return;
                    };
                    match self.iface.udp_send(
                        server,
                        server_port,
                        DNS_CLIENT_PORT,
                        &query[..len],
                        &mut self.tx_frame,
                    ) {
                        Ok(n) => {
                            let _ = self.device.transmit(&self.tx_frame[..n]);
                            fetch.stage = FetchStage::Resolving {
                                server,
                                server_port,
                                id,
                                sent_at: Some(now),
                                tries: tries + 1,
                            };
                        }
                        // The next hop's address first; the query goes on a
                        // later pass, once the reply is in.
                        Err(SendError::NeedArp) => {
                            let n = self.iface.pending_frame_len();
                            let _ = self.device.transmit(&self.tx_frame[..n]);
                            self.iface.clear_pending_frame();
                        }
                        Err(_) => {
                            self.fetch_lines
                                .push(alloc::format!("{verb}: could not send the query"));
                            return;
                        }
                    }
                }
                self.fetch = Some(fetch);
            }
            FetchStage::Connecting { conn } => {
                let state = self
                    .iface
                    .tcp()
                    .connection(conn)
                    .map(|c| c.is_established());
                match state {
                    None => {
                        self.fetch_lines.push(alloc::format!(
                            "fetch: {}:{} refused or did not answer",
                            fetch.host,
                            fetch.port
                        ));
                    }
                    Some(false) => self.fetch = Some(fetch),
                    Some(true) => {
                        let mut request = [0u8; 1024];
                        let url = net_stack::http::Url {
                            host: &fetch.host,
                            port: fetch.port,
                            path: &fetch.path,
                        };
                        match net_stack::http::write_request(&url, &mut request) {
                            Some(len) => {
                                self.iface.tcp_mut().write(conn, &request[..len]);
                                fetch.stage = FetchStage::Receiving { conn };
                                self.fetch = Some(fetch);
                            }
                            None => {
                                self.iface.tcp_mut().abort(conn);
                                self.fetch_lines
                                    .push(String::from("fetch: that URL is too long"));
                            }
                        }
                    }
                }
            }
            FetchStage::Receiving { conn } => {
                let mut buf = [0u8; 2048];
                loop {
                    let n = match self.iface.tcp().connection(conn) {
                        Some(_) => self.iface.tcp_mut().read(conn, &mut buf),
                        None => 0,
                    };
                    if n == 0 {
                        break;
                    }
                    let room = FETCH_MAX_BYTES.saturating_sub(fetch.data.len());
                    fetch.data.extend_from_slice(&buf[..n.min(room)]);
                    fetch.dropped += n.saturating_sub(room);
                }
                let (open, peer_done) = match self.iface.tcp().connection(conn) {
                    Some(c) => (true, c.peer_closed() && c.readable() == 0),
                    None => (false, true),
                };
                let complete = match net_stack::http::parse_response(&fetch.data) {
                    net_stack::http::ResponseParse::Complete(r) => r
                        .content_length
                        .is_some_and(|len| fetch.data.len() + fetch.dropped >= r.head_len + len),
                    _ => false,
                };
                if peer_done || complete {
                    if open {
                        self.iface.tcp_mut().close(conn);
                    }
                    self.report(&fetch);
                } else {
                    self.fetch = Some(fetch);
                }
            }
        }
    }

    /// The response, as lines for the Terminal.
    fn report(&mut self, fetch: &Fetch) {
        use net_stack::http::{dechunk, parse_response, ResponseParse};
        let response = match parse_response(&fetch.data) {
            ResponseParse::Complete(r) => r,
            ResponseParse::Incomplete if fetch.data.is_empty() => {
                self.fetch_lines.push(alloc::format!(
                    "fetch: {} closed without answering",
                    fetch.host
                ));
                return;
            }
            _ => {
                self.fetch_lines.push(alloc::format!(
                    "fetch: {} did not answer in HTTP",
                    fetch.host
                ));
                return;
            }
        };
        let raw = &fetch.data[response.head_len..];
        let mut decoded = Vec::new();
        let body: &[u8] = if response.chunked {
            decoded.resize(raw.len(), 0);
            match dechunk(raw, &mut decoded) {
                Some(n) => &decoded[..n],
                None => raw,
            }
        } else {
            match response.content_length {
                Some(len) => &raw[..len.min(raw.len())],
                None => raw,
            }
        };
        let total = body.len() + fetch.dropped;
        if let Some(card) = self.card_fetch {
            self.card_answered = true;
            let port = if fetch.port == 80 {
                String::new()
            } else {
                alloc::format!(":{}", fetch.port)
            };
            let path = if fetch.path.starts_with('?') {
                alloc::format!("/{}", fetch.path)
            } else {
                fetch.path.clone()
            };
            self.web_outcomes.push((
                card,
                Ok(crate::web::WebResponse {
                    url: alloc::format!("http://{}{port}{path}", fetch.host),
                    status: response.status,
                    reason: String::from(response.reason),
                    location: response.location.map(String::from),
                    content_type: response.content_type.map(String::from),
                    body: body.to_vec(),
                }),
            ));
            return;
        }
        self.fetch_lines.push(alloc::format!(
            "HTTP {} {}, {} bytes",
            response.status,
            response.reason,
            total
        ));
        if let Some(to) = response.location {
            self.fetch_lines.push(alloc::format!("  -> {to}"));
        }
        let text = core::str::from_utf8(body)
            .ok()
            .filter(|t| !t.contains('\0'));
        let Some(text) = text else {
            if !body.is_empty() {
                self.fetch_lines
                    .push(alloc::format!("  (not text: {total} bytes)"));
            }
            return;
        };
        let lines: Vec<&str> = text.lines().collect();
        for line in lines.iter().take(FETCH_LINES) {
            let shown: String = line.chars().take(FETCH_LINE_CHARS).collect();
            self.fetch_lines.push(shown);
        }
        if lines.len() > FETCH_LINES {
            self.fetch_lines.push(alloc::format!(
                "... {} more lines",
                lines.len() - FETCH_LINES
            ));
        }
        if fetch.dropped > 0 {
            self.fetch_lines.push(alloc::format!(
                "... and {} bytes past the first {} KiB",
                fetch.dropped,
                FETCH_MAX_BYTES / 1024
            ));
        }
    }
}
