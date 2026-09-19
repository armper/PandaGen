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
    http_bytes: u64,
    http_streams: [HttpStream; net_stack::tcp::MAX_CONNECTIONS],
    tcp_echoed_bytes: u64,
    tcp_accepted_seen: u64,
    /// How the address was obtained: "dhcp", "static", or "none".
    address_source: &'static str,
    lease_seconds: u32,
    dns: Option<Ipv4>,
    dhcp_server: Ipv4,
    lease_started_tick: u64,
    renewals: u32,
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
        let _ = iface.tcp_listen(TCP_ECHO_PORT);
        if remote_enabled {
            let _ = iface.tcp_listen(TCP_COMMAND_PORT);
        }
        let _ = iface.tcp_listen(HTTP_PORT);
        Some(Self {
            device,
            iface,
            rx_frame: [0; MAX_FRAME_LEN],
            tx_frame: [0; MAX_FRAME_LEN],
            udp_echoed: 0,
            http_requests: 0,
            http_bytes: 0,
            http_streams: [HttpStream::default(); net_stack::tcp::MAX_CONNECTIONS],
            tcp_echoed_bytes: 0,
            tcp_accepted_seen: 0,
            address_source: "none",
            lease_seconds: 0,
            dhcp_server: [0; 4],
            lease_started_tick: 0,
            renewals: 0,
            dns: None,
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
                let len = self.iface.pending_frame_len();
                let _ = self.device.transmit(&self.tx_frame[..len]);
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
            match self
                .iface
                .receive(&self.rx_frame[..len], &mut self.tx_frame)
            {
                Event::Transmit(n) => {
                    let _ = self.device.transmit(&self.tx_frame[..n]);
                }
                Event::TcpReady { conn } => {
                    let port = self.iface.tcp().connection(conn).map(|c| c.local_port);
                    match port {
                        Some(HTTP_PORT) => self.http_service(conn, status, log),
                        Some(TCP_COMMAND_PORT) => {
                            if let Some(line) = self.tcp_command_service(conn, log) {
                                remote = Some(RemoteRequest::TcpLine { conn, line });
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
            Serve {
                head_len: usize,
                keep_alive: bool,
                route: Route,
            },
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
                        head_len: request.head_len,
                        keep_alive: request.keep_alive,
                        route,
                    }
                }
            }
        };

        let (head_len, keep_alive, route) = match action {
            Action::Wait => {
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
                self.http_drain(conn, usize::MAX);
                self.http_respond(conn, 431, "text/plain", b"header too large\n", false);
                return;
            }
            Action::Malformed => {
                self.http_drain(conn, usize::MAX);
                self.http_respond(conn, 400, "text/plain", b"bad request\n", false);
                return;
            }
            Action::Serve {
                head_len,
                keep_alive,
                route,
            } => (head_len, keep_alive, route),
        };

        self.http_drain(conn, head_len);
        self.http_requests += 1;

        match route {
            Route::Index => {
                let mut body = FixedBuf::<1600>::new();
                write_status_page(&mut body, status);
                self.http_respond(
                    conn,
                    200,
                    "text/html; charset=utf-8",
                    body.as_bytes(),
                    keep_alive,
                );
            }
            Route::Health => self.http_respond(conn, 200, "text/plain", b"ok\n", keep_alive),
            Route::Bytes(count) => {
                let _ = writeln!(log, "net: http /bytes/{count} on conn{conn}");
                self.http_begin_stream(conn, count, keep_alive);
            }
            Route::NotFound => self.http_respond(conn, 404, "text/plain", b"not found\n", false),
            Route::MethodNotAllowed => {
                self.http_respond(conn, 405, "text/plain", b"method not allowed\n", false)
            }
        }
    }

    /// Consume up to `count` buffered bytes.
    fn http_drain(&mut self, conn: usize, count: usize) {
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
        self.iface.tcp_mut().write(conn, &head[..head_len]);
        self.iface.tcp_mut().write(conn, body);
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
        self.iface.tcp_mut().write(conn, &head[..head_len]);
        self.http_streams[conn] = HttpStream {
            remaining: count,
            offset: 0,
            close_when_done: !keep_alive,
        };
    }

    /// Feed every streaming response as much as its send buffer will take.
    fn pump_http(&mut self) {
        for conn in 0..self.http_streams.len() {
            if self.http_streams[conn].remaining == 0 {
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
                return Some(RemoteRequest::TcpLine { conn: index, line });
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
        while let Some(n) = self.iface.tcp_next_frame(&mut self.tx_frame) {
            if self.device.transmit(&self.tx_frame[..n]).is_err() {
                break;
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
