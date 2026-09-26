#![no_std]
//! Minimal IPv4 stack for bare metal: Ethernet II framing, ARP, IPv4 and
//! ICMP echo, plus an `Interface` that answers ARP and ping and can send
//! its own echo requests. No allocation; every frame is built into a
//! caller-provided buffer so the kernel can DMA it directly.

pub mod dhcp;
pub mod dns;
pub mod http;
pub mod tcp;
pub mod wire;

#[cfg(test)]
mod tests;

use wire::{
    ArpPacket, EthernetHeader, Icmp, Ipv4Header, Udp, ARP_OP_REPLY, ARP_OP_REQUEST, ETHERTYPE_ARP,
    ETHERTYPE_IPV4, ICMP_ECHO_REPLY, ICMP_ECHO_REQUEST, IP_PROTO_ICMP, IP_PROTO_TCP, IP_PROTO_UDP,
    MAC_BROADCAST,
};

/// Ports an interface can listen on at once.
pub const MAX_BOUND_PORTS: usize = 4;

pub type Mac = [u8; 6];
pub type Ipv4 = [u8; 4];

/// Static interface configuration (no DHCP yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub mac: Mac,
    pub ip: Ipv4,
    pub netmask: Ipv4,
    pub gateway: Ipv4,
}

impl Config {
    /// QEMU user-mode networking defaults.
    pub const fn qemu_user(mac: Mac) -> Self {
        Self {
            mac,
            ip: [10, 0, 2, 15],
            netmask: [255, 255, 255, 0],
            gateway: [10, 0, 2, 2],
        }
    }

    /// No address yet (before DHCP): only broadcasts and frames sent to our
    /// MAC are accepted.
    pub const fn unconfigured(mac: Mac) -> Self {
        Self {
            mac,
            ip: [0; 4],
            netmask: [0; 4],
            gateway: [0; 4],
        }
    }

    pub fn is_configured(&self) -> bool {
        self.ip != [0; 4]
    }

    fn same_subnet(&self, ip: Ipv4) -> bool {
        (0..4).all(|i| (self.ip[i] & self.netmask[i]) == (ip[i] & self.netmask[i]))
    }

    /// Next hop for `ip`: itself on the local subnet, else the gateway.
    pub fn next_hop(&self, ip: Ipv4) -> Ipv4 {
        if self.same_subnet(ip) {
            ip
        } else {
            self.gateway
        }
    }
}

/// What the interface wants the caller to do after handling a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Nothing of interest (or a frame for someone else).
    None,
    /// An outgoing frame was written into the buffer with this length; send it.
    Transmit(usize),
    /// An echo reply for our outstanding ping arrived.
    EchoReply { from: Ipv4, seq: u16, ttl: u8 },
    /// A datagram for a bound port arrived; its payload is
    /// `frame[payload_offset..payload_offset + payload_len]`.
    Udp {
        src: Ipv4,
        src_port: u16,
        dst_port: u16,
        payload_offset: usize,
        payload_len: usize,
    },
    /// A TCP connection gained readable data or changed state. Any reply
    /// segments are fetched with `tcp_next_frame`.
    TcpReady { conn: usize },
}

/// Outstanding ping, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ping {
    target: Ipv4,
    seq: u16,
}

/// Fixed-size ARP cache.
#[derive(Debug, Clone, Copy)]
pub struct ArpCache<const N: usize> {
    entries: [Option<(Ipv4, Mac)>; N],
    next: usize,
}

impl<const N: usize> ArpCache<N> {
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            next: 0,
        }
    }

    pub fn lookup(&self, ip: Ipv4) -> Option<Mac> {
        self.entries
            .iter()
            .flatten()
            .find(|(cached, _)| *cached == ip)
            .map(|(_, mac)| *mac)
    }

    pub fn insert(&mut self, ip: Ipv4, mac: Mac) {
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|e| matches!(e, Some((cached, _)) if *cached == ip))
        {
            *slot = Some((ip, mac));
            return;
        }
        self.entries[self.next] = Some((ip, mac));
        self.next = (self.next + 1) % N;
    }

    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<const N: usize> Default for ArpCache<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The next hop's MAC is unknown; an ARP request was written instead.
    NeedArp,
    BufferTooSmall,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub frames_in: u64,
    pub arp_replies_sent: u64,
    pub arp_requests_sent: u64,
    pub echo_replies_sent: u64,
    pub echo_requests_sent: u64,
    pub echo_replies_received: u64,
    pub udp_received: u64,
    pub udp_sent: u64,
    pub udp_unbound: u64,
    pub dropped: u64,
}

/// One network interface's protocol state.
pub struct Interface {
    config: Config,
    arp: ArpCache<16>,
    ping: Option<Ping>,
    next_seq: u16,
    ident: u16,
    counters: Counters,
    pending_len: usize,
    bound: [Option<u16>; MAX_BOUND_PORTS],
    /// Addresses we have sent an ARP request for and not yet heard back on.
    /// An ARP reply is only believed when it answers one of these: an
    /// unsolicited reply is the oldest spoof there is, and Linux refuses it
    /// by default (`arp_accept = 0`).
    solicited: [Ipv4; 4],
    tcp: tcp::Tcp,
}

impl Interface {
    pub const fn new(config: Config) -> Self {
        Self {
            config,
            arp: ArpCache::new(),
            solicited: [[0; 4]; 4],
            ping: None,
            next_seq: 1,
            ident: 0x5047, // "PG"
            counters: Counters {
                frames_in: 0,
                arp_replies_sent: 0,
                arp_requests_sent: 0,
                echo_replies_sent: 0,
                echo_requests_sent: 0,
                echo_replies_received: 0,
                udp_received: 0,
                udp_sent: 0,
                udp_unbound: 0,
                dropped: 0,
            },
            pending_len: 0,
            bound: [None; MAX_BOUND_PORTS],
            tcp: tcp::Tcp::new(),
        }
    }

    /// Accept TCP connections on `port`; false when no slot is free.
    pub fn tcp_listen(&mut self, port: u16) -> bool {
        self.tcp.listen(port)
    }

    pub fn tcp(&self) -> &tcp::Tcp {
        &self.tcp
    }

    pub fn tcp_mut(&mut self) -> &mut tcp::Tcp {
        &mut self.tcp
    }

    /// Advance TCP's clock (ticks) for retransmission timers.
    pub fn tcp_tick(&mut self, now: u64) {
        self.tcp.set_now(now);
    }

    /// Frame the next TCP segment to send (immediate replies first, then
    /// data, FINs, and retransmissions). `None` when nothing is pending or
    /// the peer's MAC is unknown.
    pub fn tcp_next_frame(&mut self, out: &mut [u8]) -> Option<usize> {
        // Resolve the next hop *before* touching TCP. `take_reply` removes
        // the pending reply and `poll` mutates the connection -- it counts a
        // retransmission and advances the timer -- so doing this the other
        // way round threw away a SYN-ACK or a RST with nothing to resend it,
        // and spent retransmission attempts on segments that never reached
        // the wire. Five such passes and the connection was closed as if the
        // peer were dead. Nothing on this path asked for the address either,
        // so nothing drove resolution: it waited for the gateway to ARP us.
        // The immediate reply is for one peer and cannot be deferred, so it
        // is resolved on its own terms.
        let mut skip = [[0u8; 4]; tcp::MAX_CONNECTIONS];
        let mut skipped = 0;
        let dst_mac = if let Some(peer) = self.tcp.reply_peer() {
            let hop = self.config.next_hop(peer);
            match self.arp.lookup(hop) {
                Some(mac) => mac,
                None => return self.ask_for(hop, out),
            }
        } else {
            // Otherwise serve the first connection whose next hop we know.
            // Taking only the first with work meant one unresolvable peer
            // held up every other connection: nothing was polled at all, no
            // retransmission timer advanced, and data, ACKs and FINs for
            // every port waited behind it.
            let mut peers = [[0u8; 4]; tcp::MAX_CONNECTIONS];
            let count = self.tcp.peers_with_work(&mut peers);
            // Nothing to send. This used to fall through to `peers.first()?`
            // on a *fixed-size array*, which is never `None`, so an idle
            // stack built a broadcast ARP request for `next_hop([0,0,0,0])`
            // on every single poll -- and Phase 319's other half then
            // transmitted it. Phase 324 put that flush inside the ping and
            // DHCP spin loops, turning it into a CPU-rate broadcast flood.
            if count == 0 {
                return None;
            }
            let mut found = None;
            for peer in &peers[..count] {
                let hop = self.config.next_hop(*peer);
                match self.arp.lookup(hop) {
                    Some(mac) => {
                        found = Some(mac);
                        break;
                    }
                    None => {
                        skip[skipped] = *peer;
                        skipped += 1;
                    }
                }
            }
            match found {
                Some(mac) => mac,
                // Nobody is reachable. Ask about the first, and leave every
                // connection's state untouched so nothing is spent.
                None => {
                    let hop = self.config.next_hop(peers[0]);
                    return self.ask_for(hop, out);
                }
            }
        };

        let (outgoing, payload_index) = match self.tcp.take_reply() {
            Some(reply) => (reply, None),
            None => {
                let seg = self.tcp.poll_skipping(&skip[..skipped])?;
                let index = self.tcp.index_of(&seg);
                (seg, index)
            }
        };
        let payload: &[u8] = match payload_index {
            Some(index) if outgoing.payload.1 > 0 => self.tcp.payload(index, outgoing.payload),
            _ => &[],
        };
        let segment = wire::Tcp {
            src_port: outgoing.local_port,
            dst_port: outgoing.peer_port,
            seq: outgoing.seq,
            ack: outgoing.ack,
            flags: outgoing.flags,
            window: outgoing.window,
            payload,
        };
        wire::build_tcp(
            out,
            self.config.mac,
            dst_mac,
            self.config.ip,
            outgoing.peer,
            &segment,
            outgoing.mss,
        )
    }

    /// Listen on a UDP port. Returns false when all slots are taken.
    pub fn bind(&mut self, port: u16) -> bool {
        if self.bound.contains(&Some(port)) {
            return true;
        }
        match self.bound.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some(port);
                true
            }
            None => false,
        }
    }

    pub fn unbind(&mut self, port: u16) {
        for slot in self.bound.iter_mut() {
            if *slot == Some(port) {
                *slot = None;
            }
        }
    }

    pub fn is_bound(&self, port: u16) -> bool {
        self.bound.contains(&Some(port))
    }

    /// Build a UDP datagram to `dst:dst_port` from `src_port` in `out`.
    /// Like `ping`, returns `NeedArp` (with an ARP request written) when the
    /// next hop's MAC is unknown.
    pub fn udp_send(
        &mut self,
        dst: Ipv4,
        dst_port: u16,
        src_port: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, SendError> {
        let hop = self.config.next_hop(dst);
        let Some(dst_mac) = self.arp.lookup(hop) else {
            return match self.arp_request(hop, out) {
                Some(len) => {
                    self.pending_len = len;
                    Err(SendError::NeedArp)
                }
                None => Err(SendError::BufferTooSmall),
            };
        };
        let udp = Udp {
            src_port,
            dst_port,
            payload,
        };
        let len = wire::build_udp(out, self.config.mac, dst_mac, self.config.ip, dst, &udp)
            .ok_or(SendError::BufferTooSmall)?;
        self.counters.udp_sent += 1;
        Ok(len)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Replace the address configuration (DHCP bind); the ARP cache is kept
    /// only if the MAC is unchanged, which it always is in practice.
    pub fn set_config(&mut self, config: Config) {
        if config.mac != self.config.mac {
            self.arp = ArpCache::new();
        }
        self.config = config;
    }

    /// Build a UDP datagram to the limited broadcast address (no ARP).
    pub fn udp_broadcast(
        &mut self,
        dst_port: u16,
        src_port: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, SendError> {
        let udp = Udp {
            src_port,
            dst_port,
            payload,
        };
        let len = wire::build_udp(
            out,
            self.config.mac,
            MAC_BROADCAST,
            self.config.ip,
            [255, 255, 255, 255],
            &udp,
        )
        .ok_or(SendError::BufferTooSmall)?;
        self.counters.udp_sent += 1;
        Ok(len)
    }

    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// Test-only: seed the cache without a handshake.
    #[cfg(test)]
    pub fn arp_cache_mut(&mut self) -> &mut ArpCache<16> {
        &mut self.arp
    }

    pub fn arp_cache(&self) -> &ArpCache<16> {
        &self.arp
    }

    /// Handle one received frame. If a response is needed it is written to
    /// `out` and reported through `Event::Transmit`.
    pub fn receive(&mut self, frame: &[u8], out: &mut [u8]) -> Event {
        self.counters.frames_in += 1;
        let Some((eth, payload)) = EthernetHeader::parse(frame) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        if eth.dst != self.config.mac && eth.dst != MAC_BROADCAST {
            return Event::None;
        }
        match eth.ethertype {
            ETHERTYPE_ARP => self.receive_arp(payload, out),
            ETHERTYPE_IPV4 => self.receive_ipv4(
                eth.src,
                eth.dst == self.config.mac,
                frame.as_ptr() as usize,
                payload,
                out,
            ),
            _ => Event::None,
        }
    }

    fn receive_arp(&mut self, payload: &[u8], out: &mut [u8]) -> Event {
        let Some(arp) = ArpPacket::parse(payload) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        // Learn from a request addressed to us -- that is a peer telling us
        // it wants to talk, and the reply we are about to send needs its
        // address anyway -- and from a reply only when we asked for it. An
        // unsolicited reply used to be believed, which is the oldest ARP
        // spoof there is; Linux refuses it by default (`arp_accept = 0`).
        if arp.target_ip == self.config.ip {
            match arp.operation {
                ARP_OP_REQUEST => self.arp.insert(arp.sender_ip, arp.sender_mac),
                ARP_OP_REPLY if self.solicited.contains(&arp.sender_ip) => {
                    self.arp.insert(arp.sender_ip, arp.sender_mac);
                }
                _ => {}
            }
        }
        if arp.operation == ARP_OP_REQUEST && arp.target_ip == self.config.ip {
            let reply = ArpPacket {
                operation: ARP_OP_REPLY,
                sender_mac: self.config.mac,
                sender_ip: self.config.ip,
                target_mac: arp.sender_mac,
                target_ip: arp.sender_ip,
            };
            let Some(len) = wire::build_arp(out, self.config.mac, arp.sender_mac, &reply) else {
                self.counters.dropped += 1;
                return Event::None;
            };
            self.counters.arp_replies_sent += 1;
            return Event::Transmit(len);
        }
        Event::None
    }

    fn receive_ipv4(
        &mut self,
        src_mac: Mac,
        eth_dst_is_ours: bool,
        frame_start: usize,
        payload: &[u8],
        out: &mut [u8],
    ) -> Event {
        let Some((ip, body)) = Ipv4Header::parse(payload) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        // Ours: our address, or (while unconfigured) anything unicast to our
        // MAC, which is how DHCP replies arrive.
        let broadcast = ip.dst == [255, 255, 255, 255];
        let for_us = ip.dst == self.config.ip
            || broadcast
            || (!self.config.is_configured() && eth_dst_is_ours);
        if !for_us {
            return Event::None;
        }
        // Neighbour learning, but only where there is nothing to overwrite.
        // Taking the source address of any datagram as gospel meant one
        // packet with a forged source -- an unbound UDP port would do --
        // pointed all our off-link traffic at the attacker's MAC. Linux does
        // not learn layer-3-to-layer-2 bindings from data packets at all;
        // this keeps the convenience (hosts that never ARP us, like the QEMU
        // gateway forwarding host traffic) without the overwrite.
        if self.config.same_subnet(ip.src) && self.arp.lookup(ip.src).is_none() {
            self.arp.insert(ip.src, src_mac);
        }
        // A broadcast datagram is accepted only for the protocols that need
        // it. Answering an echo -- ICMP or the UDP echo port -- sent to the
        // broadcast address with a forged source turns this machine into a
        // reflector aimed at whoever the attacker names, from one packet.
        // Linux sets `icmp_echo_ignore_broadcasts` by default and has not
        // shipped a UDP echo service in decades.
        if broadcast && ip.protocol != IP_PROTO_UDP {
            return Event::None;
        }
        match ip.protocol {
            IP_PROTO_ICMP => self.receive_icmp(ip, body, out),
            IP_PROTO_TCP => {
                let Some(seg) = wire::Tcp::parse(body, ip.src, ip.dst) else {
                    self.counters.dropped += 1;
                    return Event::None;
                };
                match self.tcp.receive(tcp::Segment {
                    src: ip.src,
                    src_port: seg.src_port,
                    dst_port: seg.dst_port,
                    seq: seg.seq,
                    ack: seg.ack,
                    flags: seg.flags,
                    window: seg.window,
                    payload: seg.payload,
                }) {
                    Some(conn) => Event::TcpReady { conn },
                    None => Event::None,
                }
            }
            IP_PROTO_UDP => {
                let Some(udp) = Udp::parse(body, ip.src, ip.dst) else {
                    self.counters.dropped += 1;
                    return Event::None;
                };
                if !self.is_bound(udp.dst_port) {
                    self.counters.udp_unbound += 1;
                    return Event::None;
                }
                // Broadcast UDP is only ever wanted for DHCP replies. Any
                // other port answering a broadcast -- the echo port, say --
                // reflects to whatever source the sender forged.
                if broadcast && udp.dst_port != crate::dhcp::DHCP_CLIENT_PORT {
                    self.counters.dropped += 1;
                    return Event::None;
                }
                self.counters.udp_received += 1;
                let body_offset = body.as_ptr() as usize - frame_start;
                Event::Udp {
                    src: ip.src,
                    src_port: udp.src_port,
                    dst_port: udp.dst_port,
                    payload_offset: body_offset + wire::UDP_HDR_LEN,
                    payload_len: udp.payload.len(),
                }
            }
            _ => Event::None,
        }
    }

    fn receive_icmp(&mut self, ip: Ipv4Header, body: &[u8], out: &mut [u8]) -> Event {
        let Some(icmp) = Icmp::parse(body) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        match icmp.kind {
            ICMP_ECHO_REQUEST => {
                let Some(dst_mac) = self.arp.lookup(self.config.next_hop(ip.src)) else {
                    self.counters.dropped += 1;
                    return Event::None;
                };
                let reply = Icmp {
                    kind: ICMP_ECHO_REPLY,
                    code: 0,
                    ident: icmp.ident,
                    seq: icmp.seq,
                    payload: icmp.payload,
                };
                let Some(len) = wire::build_icmp(
                    out,
                    self.config.mac,
                    dst_mac,
                    self.config.ip,
                    ip.src,
                    &reply,
                ) else {
                    self.counters.dropped += 1;
                    return Event::None;
                };
                self.counters.echo_replies_sent += 1;
                Event::Transmit(len)
            }
            ICMP_ECHO_REPLY => {
                if let Some(ping) = self.ping {
                    if ping.target == ip.src && icmp.ident == self.ident && icmp.seq == ping.seq {
                        self.ping = None;
                        self.counters.echo_replies_received += 1;
                        return Event::EchoReply {
                            from: ip.src,
                            seq: icmp.seq,
                            ttl: ip.ttl,
                        };
                    }
                }
                Event::None
            }
            _ => Event::None,
        }
    }

    /// Write an ARP request for `ip` into `out`.
    /// Write an ARP request for `hop` into `out` and remember its length,
    /// so a caller that got `None` can still transmit it.
    fn ask_for(&mut self, hop: Ipv4, out: &mut [u8]) -> Option<usize> {
        if let Some(len) = self.arp_request(hop, out) {
            self.pending_len = len;
        }
        None
    }

    pub fn arp_request(&mut self, ip: Ipv4, out: &mut [u8]) -> Option<usize> {
        let request = ArpPacket {
            operation: ARP_OP_REQUEST,
            sender_mac: self.config.mac,
            sender_ip: self.config.ip,
            target_mac: [0; 6],
            target_ip: ip,
        };
        let len = wire::build_arp(out, self.config.mac, MAC_BROADCAST, &request)?;
        self.counters.arp_requests_sent += 1;
        // Remember what we asked for, so the reply is believable.
        if !self.solicited.contains(&ip) {
            self.solicited.rotate_right(1);
            self.solicited[0] = ip;
        }
        Some(len)
    }

    /// Start a ping: writes the echo request into `out` and remembers it, or
    /// writes an ARP request for the next hop and reports `NeedArp` (call
    /// again once the reply has been received).
    pub fn ping(
        &mut self,
        target: Ipv4,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<(usize, u16), SendError> {
        let hop = self.config.next_hop(target);
        let Some(dst_mac) = self.arp.lookup(hop) else {
            return match self.arp_request(hop, out) {
                Some(len) if len > 0 => {
                    // Caller sends the ARP request; signal via the error.
                    self.pending_len = len;
                    Err(SendError::NeedArp)
                }
                _ => Err(SendError::BufferTooSmall),
            };
        };
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1).max(1);
        let request = Icmp {
            kind: ICMP_ECHO_REQUEST,
            code: 0,
            ident: self.ident,
            seq,
            payload,
        };
        let len = wire::build_icmp(
            out,
            self.config.mac,
            dst_mac,
            self.config.ip,
            target,
            &request,
        )
        .ok_or(SendError::BufferTooSmall)?;
        self.ping = Some(Ping { target, seq });
        self.counters.echo_requests_sent += 1;
        Ok((len, seq))
    }

    /// Length of the ARP request written when `ping` returned `NeedArp`.
    /// Forget a pending frame the caller has transmitted, so it is not sent
    /// again on the next pass.
    pub fn clear_pending_frame(&mut self) {
        self.pending_len = 0;
    }

    pub fn pending_frame_len(&self) -> usize {
        self.pending_len
    }

    /// Forget an outstanding ping (timeout).
    pub fn cancel_ping(&mut self) {
        self.ping = None;
    }

    pub fn has_outstanding_ping(&self) -> bool {
        self.ping.is_some()
    }
}
