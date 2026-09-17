#![no_std]
//! Minimal IPv4 stack for bare metal: Ethernet II framing, ARP, IPv4 and
//! ICMP echo, plus an `Interface` that answers ARP and ping and can send
//! its own echo requests. No allocation; every frame is built into a
//! caller-provided buffer so the kernel can DMA it directly.

pub mod wire;

#[cfg(test)]
mod tests;

use wire::{
    ArpPacket, EthernetHeader, Icmp, Ipv4Header, Udp, ARP_OP_REPLY, ARP_OP_REQUEST, ETHERTYPE_ARP,
    ETHERTYPE_IPV4, ICMP_ECHO_REPLY, ICMP_ECHO_REQUEST, IP_PROTO_ICMP, IP_PROTO_UDP, MAC_BROADCAST,
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
}

impl Interface {
    pub const fn new(config: Config) -> Self {
        Self {
            config,
            arp: ArpCache::new(),
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
        }
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

    pub fn counters(&self) -> Counters {
        self.counters
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
            ETHERTYPE_IPV4 => self.receive_ipv4(eth.src, frame.as_ptr() as usize, payload, out),
            _ => Event::None,
        }
    }

    fn receive_arp(&mut self, payload: &[u8], out: &mut [u8]) -> Event {
        let Some(arp) = ArpPacket::parse(payload) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        // Learn from any ARP we see that involves us.
        if arp.target_ip == self.config.ip {
            self.arp.insert(arp.sender_ip, arp.sender_mac);
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
        frame_start: usize,
        payload: &[u8],
        out: &mut [u8],
    ) -> Event {
        let Some((ip, body)) = Ipv4Header::parse(payload) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        if ip.dst != self.config.ip {
            return Event::None;
        }
        // Neighbour learning: whoever sends us IPv4 directly is reachable at
        // that MAC (covers hosts that never ARP us, like the QEMU gateway
        // forwarding host traffic).
        if self.config.same_subnet(ip.src) {
            self.arp.insert(ip.src, src_mac);
        }
        match ip.protocol {
            IP_PROTO_ICMP => self.receive_icmp(ip, body, out),
            IP_PROTO_UDP => {
                let Some(udp) = Udp::parse(body, ip.src, ip.dst) else {
                    self.counters.dropped += 1;
                    return Event::None;
                };
                if !self.is_bound(udp.dst_port) {
                    self.counters.udp_unbound += 1;
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
