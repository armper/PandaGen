//! Packet formats: parse from and build into byte slices, big-endian.

use crate::{Ipv4, Mac};

pub const MAC_BROADCAST: Mac = [0xFF; 6];
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETH_HDR_LEN: usize = 14;
pub const ARP_LEN: usize = 28;
pub const ARP_OP_REQUEST: u16 = 1;
pub const ARP_OP_REPLY: u16 = 2;
pub const IPV4_HDR_LEN: usize = 20;
pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_UDP: u8 = 17;
pub const IP_PROTO_TCP: u8 = 6;
pub const TCP_HDR_LEN: usize = 20;
pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;
pub const UDP_HDR_LEN: usize = 8;
pub const ICMP_HDR_LEN: usize = 8;
pub const ICMP_ECHO_REPLY: u8 = 0;
pub const ICMP_ECHO_REQUEST: u8 = 8;
/// Default TTL for frames we originate.
pub const DEFAULT_TTL: u8 = 64;
/// Minimum Ethernet payload; shorter frames are zero-padded.
pub const MIN_FRAME_LEN: usize = 60;

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn put16(b: &mut [u8], v: u16) {
    b.copy_from_slice(&v.to_be_bytes());
}

/// RFC 1071 one's-complement checksum.
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for c in &mut chunks {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EthernetHeader {
    pub dst: Mac,
    pub src: Mac,
    pub ethertype: u16,
}

impl EthernetHeader {
    pub fn parse(frame: &[u8]) -> Option<(Self, &[u8])> {
        if frame.len() < ETH_HDR_LEN {
            return None;
        }
        let mut dst = [0; 6];
        let mut src = [0; 6];
        dst.copy_from_slice(&frame[0..6]);
        src.copy_from_slice(&frame[6..12]);
        Some((
            Self {
                dst,
                src,
                ethertype: be16(&frame[12..14]),
            },
            &frame[ETH_HDR_LEN..],
        ))
    }

    pub fn write(&self, out: &mut [u8]) -> Option<()> {
        if out.len() < ETH_HDR_LEN {
            return None;
        }
        out[0..6].copy_from_slice(&self.dst);
        out[6..12].copy_from_slice(&self.src);
        put16(&mut out[12..14], self.ethertype);
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpPacket {
    pub operation: u16,
    pub sender_mac: Mac,
    pub sender_ip: Ipv4,
    pub target_mac: Mac,
    pub target_ip: Ipv4,
}

impl ArpPacket {
    /// Parse an Ethernet/IPv4 ARP packet.
    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < ARP_LEN
            || be16(&p[0..2]) != 1
            || be16(&p[2..4]) != ETHERTYPE_IPV4
            || p[4] != 6
            || p[5] != 4
        {
            return None;
        }
        let mut sender_mac = [0; 6];
        let mut target_mac = [0; 6];
        let mut sender_ip = [0; 4];
        let mut target_ip = [0; 4];
        sender_mac.copy_from_slice(&p[8..14]);
        sender_ip.copy_from_slice(&p[14..18]);
        target_mac.copy_from_slice(&p[18..24]);
        target_ip.copy_from_slice(&p[24..28]);
        Some(Self {
            operation: be16(&p[6..8]),
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        })
    }

    pub fn write(&self, out: &mut [u8]) -> Option<()> {
        if out.len() < ARP_LEN {
            return None;
        }
        put16(&mut out[0..2], 1);
        put16(&mut out[2..4], ETHERTYPE_IPV4);
        out[4] = 6;
        out[5] = 4;
        put16(&mut out[6..8], self.operation);
        out[8..14].copy_from_slice(&self.sender_mac);
        out[14..18].copy_from_slice(&self.sender_ip);
        out[18..24].copy_from_slice(&self.target_mac);
        out[24..28].copy_from_slice(&self.target_ip);
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Header {
    pub src: Ipv4,
    pub dst: Ipv4,
    pub protocol: u8,
    pub ttl: u8,
    pub total_len: u16,
}

impl Ipv4Header {
    /// Parse a header (options skipped) and return it with the payload,
    /// truncated to `total_len`. Rejects bad versions and checksums.
    pub fn parse(p: &[u8]) -> Option<(Self, &[u8])> {
        if p.len() < IPV4_HDR_LEN || p[0] >> 4 != 4 {
            return None;
        }
        let ihl = (p[0] & 0x0F) as usize * 4;
        if ihl < IPV4_HDR_LEN || p.len() < ihl || checksum(&p[..ihl]) != 0 {
            return None;
        }
        let total_len = be16(&p[2..4]) as usize;
        if total_len < ihl || total_len > p.len() {
            return None;
        }
        let mut src = [0; 4];
        let mut dst = [0; 4];
        src.copy_from_slice(&p[12..16]);
        dst.copy_from_slice(&p[16..20]);
        Some((
            Self {
                src,
                dst,
                protocol: p[9],
                ttl: p[8],
                total_len: total_len as u16,
            },
            &p[ihl..total_len],
        ))
    }

    /// Write a 20-byte header for `payload_len` bytes of payload.
    pub fn write(&self, out: &mut [u8], payload_len: usize) -> Option<()> {
        if out.len() < IPV4_HDR_LEN {
            return None;
        }
        let h = &mut out[..IPV4_HDR_LEN];
        h.fill(0);
        h[0] = 0x45;
        put16(&mut h[2..4], (IPV4_HDR_LEN + payload_len) as u16);
        put16(&mut h[6..8], 0x4000); // don't fragment
        h[8] = self.ttl;
        h[9] = self.protocol;
        h[12..16].copy_from_slice(&self.src);
        h[16..20].copy_from_slice(&self.dst);
        let sum = checksum(h);
        put16(&mut h[10..12], sum);
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icmp<'a> {
    pub kind: u8,
    pub code: u8,
    pub ident: u16,
    pub seq: u16,
    pub payload: &'a [u8],
}

impl<'a> Icmp<'a> {
    /// Parse an echo request/reply (checksum verified).
    pub fn parse(p: &'a [u8]) -> Option<Self> {
        if p.len() < ICMP_HDR_LEN || checksum(p) != 0 {
            return None;
        }
        Some(Self {
            kind: p[0],
            code: p[1],
            ident: be16(&p[4..6]),
            seq: be16(&p[6..8]),
            payload: &p[ICMP_HDR_LEN..],
        })
    }

    pub fn len(&self) -> usize {
        ICMP_HDR_LEN + self.payload.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn write(&self, out: &mut [u8]) -> Option<usize> {
        let len = self.len();
        if out.len() < len {
            return None;
        }
        let m = &mut out[..len];
        m[0] = self.kind;
        m[1] = self.code;
        m[2] = 0;
        m[3] = 0;
        put16(&mut m[4..6], self.ident);
        put16(&mut m[6..8], self.seq);
        m[ICMP_HDR_LEN..].copy_from_slice(self.payload);
        let sum = checksum(m);
        put16(&mut m[2..4], sum);
        Some(len)
    }
}

/// UDP header plus payload view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Udp<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: &'a [u8],
}

/// One's-complement sum of the IPv4 pseudo-header for UDP.
fn pseudo_header_sum(src: Ipv4, dst: Ipv4, udp_len: u16) -> u32 {
    pseudo_sum(src, dst, IP_PROTO_UDP, udp_len)
}

fn pseudo_sum(src: Ipv4, dst: Ipv4, protocol: u8, len: u16) -> u32 {
    let mut sum = 0u32;
    sum += u16::from_be_bytes([src[0], src[1]]) as u32;
    sum += u16::from_be_bytes([src[2], src[3]]) as u32;
    sum += u16::from_be_bytes([dst[0], dst[1]]) as u32;
    sum += u16::from_be_bytes([dst[2], dst[3]]) as u32;
    sum += protocol as u32;
    sum += len as u32;
    sum
}

/// TCP segment header plus payload view (options skipped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tcp<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload: &'a [u8],
}

impl<'a> Tcp<'a> {
    /// Parse a segment carried between `src` and `dst`, verifying the
    /// checksum and honouring the data offset.
    pub fn parse(p: &'a [u8], src: Ipv4, dst: Ipv4) -> Option<Self> {
        if p.len() < TCP_HDR_LEN || p.len() > u16::MAX as usize {
            return None;
        }
        let offset = (p[12] >> 4) as usize * 4;
        if offset < TCP_HDR_LEN || offset > p.len() {
            return None;
        }
        if checksum_with(pseudo_sum(src, dst, IP_PROTO_TCP, p.len() as u16), p) != 0 {
            return None;
        }
        Some(Self {
            src_port: be16(&p[0..2]),
            dst_port: be16(&p[2..4]),
            seq: u32::from_be_bytes([p[4], p[5], p[6], p[7]]),
            ack: u32::from_be_bytes([p[8], p[9], p[10], p[11]]),
            flags: p[13],
            window: be16(&p[14..16]),
            payload: &p[offset..],
        })
    }

    /// Write a 20-byte header (plus an MSS option when `mss` is given)
    /// and the payload, with the checksum computed.
    pub fn write(&self, out: &mut [u8], src: Ipv4, dst: Ipv4, mss: Option<u16>) -> Option<usize> {
        let opt_len = if mss.is_some() { 4 } else { 0 };
        let hdr_len = TCP_HDR_LEN + opt_len;
        let len = hdr_len + self.payload.len();
        if out.len() < len || len > u16::MAX as usize {
            return None;
        }
        let m = &mut out[..len];
        put16(&mut m[0..2], self.src_port);
        put16(&mut m[2..4], self.dst_port);
        m[4..8].copy_from_slice(&self.seq.to_be_bytes());
        m[8..12].copy_from_slice(&self.ack.to_be_bytes());
        m[12] = ((hdr_len / 4) as u8) << 4;
        m[13] = self.flags;
        put16(&mut m[14..16], self.window);
        put16(&mut m[16..18], 0);
        put16(&mut m[18..20], 0);
        if let Some(mss) = mss {
            m[20] = 2;
            m[21] = 4;
            put16(&mut m[22..24], mss);
        }
        m[hdr_len..].copy_from_slice(self.payload);
        let sum = checksum_with(pseudo_sum(src, dst, IP_PROTO_TCP, len as u16), m);
        put16(&mut m[16..18], sum);
        Some(len)
    }
}

/// Build a complete Ethernet/IPv4/TCP frame.
pub fn build_tcp(
    out: &mut [u8],
    src_mac: Mac,
    dst_mac: Mac,
    src_ip: Ipv4,
    dst_ip: Ipv4,
    tcp: &Tcp,
    mss: Option<u16>,
) -> Option<usize> {
    EthernetHeader {
        dst: dst_mac,
        src: src_mac,
        ethertype: ETHERTYPE_IPV4,
    }
    .write(out)?;
    let ip_start = ETH_HDR_LEN;
    let tcp_start = ip_start + IPV4_HDR_LEN;
    let tcp_len = tcp.write(out.get_mut(tcp_start..)?, src_ip, dst_ip, mss)?;
    Ipv4Header {
        src: src_ip,
        dst: dst_ip,
        protocol: IP_PROTO_TCP,
        ttl: DEFAULT_TTL,
        total_len: 0,
    }
    .write(&mut out[ip_start..], tcp_len)?;
    finish_frame(out, tcp_start + tcp_len)
}

/// Checksum of `data` folded together with a pseudo-header sum.
fn checksum_with(prefix: u32, data: &[u8]) -> u16 {
    let mut sum = prefix + !checksum(data) as u32;
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

impl<'a> Udp<'a> {
    /// Parse a datagram carried between `src` and `dst`; a zero checksum
    /// means "not computed" and is accepted.
    pub fn parse(p: &'a [u8], src: Ipv4, dst: Ipv4) -> Option<Self> {
        if p.len() < UDP_HDR_LEN {
            return None;
        }
        let len = be16(&p[4..6]) as usize;
        if len < UDP_HDR_LEN || len > p.len() {
            return None;
        }
        let p = &p[..len];
        if be16(&p[6..8]) != 0 && checksum_with(pseudo_header_sum(src, dst, len as u16), p) != 0 {
            return None;
        }
        Some(Self {
            src_port: be16(&p[0..2]),
            dst_port: be16(&p[2..4]),
            payload: &p[UDP_HDR_LEN..],
        })
    }

    pub fn len(&self) -> usize {
        UDP_HDR_LEN + self.payload.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// Write header and payload with a checksum over the pseudo-header.
    pub fn write(&self, out: &mut [u8], src: Ipv4, dst: Ipv4) -> Option<usize> {
        let len = self.len();
        if out.len() < len || len > u16::MAX as usize {
            return None;
        }
        let m = &mut out[..len];
        put16(&mut m[0..2], self.src_port);
        put16(&mut m[2..4], self.dst_port);
        put16(&mut m[4..6], len as u16);
        put16(&mut m[6..8], 0);
        m[UDP_HDR_LEN..].copy_from_slice(self.payload);
        let mut sum = checksum_with(pseudo_header_sum(src, dst, len as u16), m);
        if sum == 0 {
            sum = 0xFFFF;
        }
        put16(&mut m[6..8], sum);
        Some(len)
    }
}

/// Build a complete Ethernet/IPv4/UDP frame.
pub fn build_udp(
    out: &mut [u8],
    src_mac: Mac,
    dst_mac: Mac,
    src_ip: Ipv4,
    dst_ip: Ipv4,
    udp: &Udp,
) -> Option<usize> {
    EthernetHeader {
        dst: dst_mac,
        src: src_mac,
        ethertype: ETHERTYPE_IPV4,
    }
    .write(out)?;
    let ip_start = ETH_HDR_LEN;
    let udp_start = ip_start + IPV4_HDR_LEN;
    let udp_len = udp.write(out.get_mut(udp_start..)?, src_ip, dst_ip)?;
    Ipv4Header {
        src: src_ip,
        dst: dst_ip,
        protocol: IP_PROTO_UDP,
        ttl: DEFAULT_TTL,
        total_len: 0,
    }
    .write(&mut out[ip_start..], udp_len)?;
    finish_frame(out, udp_start + udp_len)
}

/// Pad a frame to the Ethernet minimum and return the final length.
fn finish_frame(out: &mut [u8], len: usize) -> Option<usize> {
    if len >= MIN_FRAME_LEN {
        return Some(len);
    }
    if out.len() < MIN_FRAME_LEN {
        return None;
    }
    out[len..MIN_FRAME_LEN].fill(0);
    Some(MIN_FRAME_LEN)
}

/// Build a complete ARP frame.
pub fn build_arp(out: &mut [u8], src: Mac, dst: Mac, arp: &ArpPacket) -> Option<usize> {
    EthernetHeader {
        dst,
        src,
        ethertype: ETHERTYPE_ARP,
    }
    .write(out)?;
    arp.write(&mut out[ETH_HDR_LEN..])?;
    finish_frame(out, ETH_HDR_LEN + ARP_LEN)
}

/// Build a complete Ethernet/IPv4/ICMP frame.
pub fn build_icmp(
    out: &mut [u8],
    src_mac: Mac,
    dst_mac: Mac,
    src_ip: Ipv4,
    dst_ip: Ipv4,
    icmp: &Icmp,
) -> Option<usize> {
    EthernetHeader {
        dst: dst_mac,
        src: src_mac,
        ethertype: ETHERTYPE_IPV4,
    }
    .write(out)?;
    let ip_start = ETH_HDR_LEN;
    let icmp_start = ip_start + IPV4_HDR_LEN;
    let icmp_len = icmp.write(out.get_mut(icmp_start..)?)?;
    Ipv4Header {
        src: src_ip,
        dst: dst_ip,
        protocol: IP_PROTO_ICMP,
        ttl: DEFAULT_TTL,
        total_len: 0,
    }
    .write(&mut out[ip_start..], icmp_len)?;
    finish_frame(out, icmp_start + icmp_len)
}

/// Format an IPv4 address as dotted decimal without allocating.
pub fn fmt_ipv4(ip: Ipv4) -> impl core::fmt::Display {
    struct D(Ipv4);
    impl core::fmt::Display for D {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
        }
    }
    D(ip)
}

/// Format a MAC address as colon-separated hex without allocating.
pub fn fmt_mac(mac: Mac) -> impl core::fmt::Display {
    struct D(Mac);
    impl core::fmt::Display for D {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                f,
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
            )
        }
    }
    D(mac)
}

/// Parse dotted decimal.
pub fn parse_ipv4(s: &str) -> Option<Ipv4> {
    let mut ip = [0u8; 4];
    let mut parts = s.split('.');
    for octet in ip.iter_mut() {
        *octet = parts.next()?.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(ip)
}
