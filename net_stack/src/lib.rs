#![no_std]
//! Minimal IPv4 stack for bare metal: Ethernet II framing, ARP, IPv4 and
//! ICMP echo, plus an `Interface` that answers ARP and ping and can send
//! its own echo requests. No allocation; every frame is built into a
//! caller-provided buffer so the kernel can DMA it directly.

pub mod wire;

#[cfg(test)]
mod tests;

use wire::{
    ArpPacket, EthernetHeader, Icmp, Ipv4Header, ARP_OP_REPLY, ARP_OP_REQUEST, ETHERTYPE_ARP,
    ETHERTYPE_IPV4, ICMP_ECHO_REPLY, ICMP_ECHO_REQUEST, IP_PROTO_ICMP, MAC_BROADCAST,
};

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
                dropped: 0,
            },
            pending_len: 0,
        }
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
            ETHERTYPE_IPV4 => self.receive_ipv4(payload, out),
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

    fn receive_ipv4(&mut self, payload: &[u8], out: &mut [u8]) -> Event {
        let Some((ip, body)) = Ipv4Header::parse(payload) else {
            self.counters.dropped += 1;
            return Event::None;
        };
        if ip.dst != self.config.ip || ip.protocol != IP_PROTO_ICMP {
            return Event::None;
        }
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
