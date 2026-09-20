use crate::wire::*;
use crate::*;

const OUR_MAC: Mac = [0x52, 0x54, 0, 0x12, 0x34, 0x56];
const GW_MAC: Mac = [0x52, 0x55, 0x0a, 0, 2, 2];

fn iface() -> Interface {
    Interface::new(Config::qemu_user(OUR_MAC))
}

#[test]
fn checksum_matches_rfc_example() {
    // Classic example: header with checksum field zero sums to 0xb861.
    let hdr = [
        0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    assert_eq!(checksum(&hdr), 0xb861);
    let mut with = hdr;
    with[10..12].copy_from_slice(&0xb861u16.to_be_bytes());
    assert_eq!(checksum(&with), 0);
    // Odd length pads the last byte on the high side.
    assert_eq!(checksum(&[0x01]), !0x0100);
}

#[test]
fn ipv4_parse_and_addresses() {
    assert_eq!(parse_ipv4("10.0.2.2"), Some([10, 0, 2, 2]));
    assert_eq!(parse_ipv4("10.0.2"), None);
    assert_eq!(parse_ipv4("10.0.2.256"), None);
    assert_eq!(parse_ipv4("1.2.3.4.5"), None);
    let cfg = Config::qemu_user(OUR_MAC);
    assert_eq!(cfg.next_hop([10, 0, 2, 2]), [10, 0, 2, 2]);
    assert_eq!(cfg.next_hop([8, 8, 8, 8]), [10, 0, 2, 2]);
    assert_eq!(cfg.next_hop([10, 0, 2, 77]), [10, 0, 2, 77]);
    extern crate std;
    assert_eq!(std::format!("{}", fmt_ipv4([10, 0, 2, 15])), "10.0.2.15");
    assert_eq!(std::format!("{}", fmt_mac(OUR_MAC)), "52:54:00:12:34:56");
}

#[test]
fn arp_request_reply_round_trip() {
    let mut a = iface();
    let mut buf = [0u8; 1514];
    let len = a.arp_request([10, 0, 2, 2], &mut buf).unwrap();
    assert_eq!(len, MIN_FRAME_LEN);
    let (eth, payload) = EthernetHeader::parse(&buf[..len]).unwrap();
    assert_eq!(eth.dst, MAC_BROADCAST);
    assert_eq!(eth.src, OUR_MAC);
    assert_eq!(eth.ethertype, ETHERTYPE_ARP);
    let arp = ArpPacket::parse(payload).unwrap();
    assert_eq!(arp.operation, ARP_OP_REQUEST);
    assert_eq!(arp.sender_ip, [10, 0, 2, 15]);
    assert_eq!(arp.target_ip, [10, 0, 2, 2]);

    // The gateway asks who has 10.0.2.15; we answer and learn its MAC.
    let who_has = ArpPacket {
        operation: ARP_OP_REQUEST,
        sender_mac: GW_MAC,
        sender_ip: [10, 0, 2, 2],
        target_mac: [0; 6],
        target_ip: [10, 0, 2, 15],
    };
    let mut frame = [0u8; 1514];
    let flen = build_arp(&mut frame, GW_MAC, MAC_BROADCAST, &who_has).unwrap();
    let mut out = [0u8; 1514];
    match a.receive(&frame[..flen], &mut out) {
        Event::Transmit(n) => {
            let (eth, payload) = EthernetHeader::parse(&out[..n]).unwrap();
            assert_eq!(eth.dst, GW_MAC);
            let reply = ArpPacket::parse(payload).unwrap();
            assert_eq!(reply.operation, ARP_OP_REPLY);
            assert_eq!(reply.sender_mac, OUR_MAC);
            assert_eq!(reply.target_ip, [10, 0, 2, 2]);
        }
        other => panic!("expected reply, got {other:?}"),
    }
    assert_eq!(a.arp_cache().lookup([10, 0, 2, 2]), Some(GW_MAC));
    assert_eq!(a.counters().arp_replies_sent, 1);

    // ARP for someone else is ignored.
    let other = ArpPacket {
        target_ip: [10, 0, 2, 99],
        ..who_has
    };
    let flen = build_arp(&mut frame, GW_MAC, MAC_BROADCAST, &other).unwrap();
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
}

#[test]
fn arp_cache_replaces_and_evicts() {
    let mut c = ArpCache::<2>::new();
    assert!(c.is_empty());
    c.insert([1, 1, 1, 1], [1; 6]);
    c.insert([1, 1, 1, 1], [2; 6]);
    assert_eq!(c.len(), 1);
    assert_eq!(c.lookup([1, 1, 1, 1]), Some([2; 6]));
    c.insert([2, 2, 2, 2], [3; 6]);
    c.insert([3, 3, 3, 3], [4; 6]);
    assert_eq!(c.lookup([1, 1, 1, 1]), None, "oldest evicted");
    assert_eq!(c.lookup([3, 3, 3, 3]), Some([4; 6]));
}

#[test]
fn ping_needs_arp_then_sends_and_matches_reply() {
    let mut a = iface();
    let mut out = [0u8; 1514];
    let payload = b"PandaGen ping";
    assert_eq!(
        a.ping([10, 0, 2, 2], payload, &mut out),
        Err(SendError::NeedArp)
    );
    let alen = a.pending_frame_len();
    let (_, p) = EthernetHeader::parse(&out[..alen]).unwrap();
    assert_eq!(ArpPacket::parse(p).unwrap().target_ip, [10, 0, 2, 2]);

    // Gateway replies to the ARP request.
    let reply = ArpPacket {
        operation: ARP_OP_REPLY,
        sender_mac: GW_MAC,
        sender_ip: [10, 0, 2, 2],
        target_mac: OUR_MAC,
        target_ip: [10, 0, 2, 15],
    };
    let mut frame = [0u8; 1514];
    let flen = build_arp(&mut frame, GW_MAC, OUR_MAC, &reply).unwrap();
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);

    let (len, seq) = a.ping([10, 0, 2, 2], payload, &mut out).unwrap();
    assert_eq!(seq, 1);
    assert!(a.has_outstanding_ping());
    let (eth, ipp) = EthernetHeader::parse(&out[..len]).unwrap();
    assert_eq!(eth.dst, GW_MAC);
    assert_eq!(eth.ethertype, ETHERTYPE_IPV4);
    let (ip, body) = Ipv4Header::parse(ipp).unwrap();
    assert_eq!(ip.src, [10, 0, 2, 15]);
    assert_eq!(ip.dst, [10, 0, 2, 2]);
    assert_eq!(ip.protocol, IP_PROTO_ICMP);
    assert_eq!(ip.ttl, DEFAULT_TTL);
    let icmp = Icmp::parse(body).unwrap();
    assert_eq!(icmp.kind, ICMP_ECHO_REQUEST);
    assert_eq!(icmp.payload, payload);

    // A reply with the wrong seq is ignored; the right one completes the ping.
    let wrong = Icmp {
        kind: ICMP_ECHO_REPLY,
        code: 0,
        ident: icmp.ident,
        seq: 9,
        payload,
    };
    let flen = build_icmp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &wrong,
    )
    .unwrap();
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
    let right = Icmp { seq, ..wrong };
    let flen = build_icmp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &right,
    )
    .unwrap();
    assert_eq!(
        a.receive(&frame[..flen], &mut out),
        Event::EchoReply {
            from: [10, 0, 2, 2],
            seq,
            ttl: DEFAULT_TTL
        }
    );
    assert!(!a.has_outstanding_ping());
    assert_eq!(a.counters().echo_replies_received, 1);
    // Second ping uses the cache directly and the next sequence number.
    let (_, seq2) = a.ping([10, 0, 2, 2], payload, &mut out).unwrap();
    assert_eq!(seq2, 2);
    a.cancel_ping();
    assert!(!a.has_outstanding_ping());
}

#[test]
fn answers_echo_requests_addressed_to_us() {
    let mut a = iface();
    let mut out = [0u8; 1514];
    let mut frame = [0u8; 1514];
    // Learn the gateway first (ARP request from it).
    let who_has = ArpPacket {
        operation: ARP_OP_REQUEST,
        sender_mac: GW_MAC,
        sender_ip: [10, 0, 2, 2],
        target_mac: [0; 6],
        target_ip: [10, 0, 2, 15],
    };
    let flen = build_arp(&mut frame, GW_MAC, MAC_BROADCAST, &who_has).unwrap();
    let _ = a.receive(&frame[..flen], &mut out);

    let req = Icmp {
        kind: ICMP_ECHO_REQUEST,
        code: 0,
        ident: 7,
        seq: 3,
        payload: b"hello",
    };
    let flen = build_icmp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &req,
    )
    .unwrap();
    match a.receive(&frame[..flen], &mut out) {
        Event::Transmit(n) => {
            let (_, ipp) = EthernetHeader::parse(&out[..n]).unwrap();
            let (ip, body) = Ipv4Header::parse(ipp).unwrap();
            assert_eq!(ip.dst, [10, 0, 2, 2]);
            let reply = Icmp::parse(body).unwrap();
            assert_eq!(reply.kind, ICMP_ECHO_REPLY);
            assert_eq!(reply.ident, 7);
            assert_eq!(reply.seq, 3);
            assert_eq!(reply.payload, b"hello");
        }
        other => panic!("expected reply, got {other:?}"),
    }
    // For another address: ignored.
    let flen = build_icmp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 16],
        &req,
    )
    .unwrap();
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
    // Corrupted checksum: dropped.
    let flen = build_icmp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &req,
    )
    .unwrap();
    frame[ETH_HDR_LEN + IPV4_HDR_LEN + 10] ^= 0xFF;
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
    assert_eq!(a.counters().dropped, 1);
    // Frame for another MAC: ignored even if IP matches.
    let flen = build_icmp(
        &mut frame,
        GW_MAC,
        [1; 6],
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &req,
    )
    .unwrap();
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
    // Truncated frame.
    assert_eq!(a.receive(&frame[..10], &mut out), Event::None);
}

#[test]
fn udp_checksum_round_trip_and_zero_checksum_accepted() {
    let src = [10, 0, 2, 2];
    let dst = [10, 0, 2, 15];
    let udp = Udp {
        src_port: 4000,
        dst_port: 7777,
        payload: b"hello udp",
    };
    let mut buf = [0u8; 64];
    let len = udp.write(&mut buf, src, dst).unwrap();
    assert_eq!(len, UDP_HDR_LEN + 9);
    let parsed = Udp::parse(&buf[..len], src, dst).unwrap();
    assert_eq!(parsed, udp);
    // Wrong addresses break the pseudo-header checksum.
    assert_eq!(Udp::parse(&buf[..len], src, [10, 0, 2, 16]), None);
    // Zero checksum means "none": accepted.
    buf[6] = 0;
    buf[7] = 0;
    assert!(Udp::parse(&buf[..len], src, [1, 2, 3, 4]).is_some());
    // Length field larger than the buffer: rejected.
    buf[4] = 0xFF;
    assert_eq!(Udp::parse(&buf[..len], src, dst), None);
}

#[test]
fn udp_delivery_only_to_bound_ports_and_learns_neighbour() {
    let mut a = iface();
    let mut out = [0u8; 1514];
    let mut frame = [0u8; 1514];
    let udp = Udp {
        src_port: 5555,
        dst_port: 7777,
        payload: b"ping?",
    };
    let flen = build_udp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &udp,
    )
    .unwrap();
    // Not bound yet: counted, not delivered, but the neighbour is learned.
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
    assert_eq!(a.counters().udp_unbound, 1);
    assert_eq!(a.arp_cache().lookup([10, 0, 2, 2]), Some(GW_MAC));

    assert!(a.bind(7777));
    assert!(a.bind(7777), "rebinding is idempotent");
    match a.receive(&frame[..flen], &mut out) {
        Event::Udp {
            src,
            src_port,
            dst_port,
            payload_offset,
            payload_len,
        } => {
            assert_eq!(src, [10, 0, 2, 2]);
            assert_eq!(src_port, 5555);
            assert_eq!(dst_port, 7777);
            assert_eq!(
                &frame[payload_offset..payload_offset + payload_len],
                b"ping?"
            );
        }
        other => panic!("expected udp, got {other:?}"),
    }
    assert_eq!(a.counters().udp_received, 1);

    // Reply goes straight out using the learned MAC.
    let len = a
        .udp_send([10, 0, 2, 2], 5555, 7777, b"pong!", &mut out)
        .unwrap();
    let (eth, ipp) = EthernetHeader::parse(&out[..len]).unwrap();
    assert_eq!(eth.dst, GW_MAC);
    let (ip, body) = Ipv4Header::parse(ipp).unwrap();
    assert_eq!(ip.protocol, IP_PROTO_UDP);
    let reply = Udp::parse(body, ip.src, ip.dst).unwrap();
    assert_eq!(reply.src_port, 7777);
    assert_eq!(reply.dst_port, 5555);
    assert_eq!(reply.payload, b"pong!");
    assert_eq!(a.counters().udp_sent, 1);

    a.unbind(7777);
    assert!(!a.is_bound(7777));
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);

    // Sending to an unknown host on the subnet needs ARP first.
    let mut b = iface();
    assert_eq!(
        b.udp_send([10, 0, 2, 9], 1, 2, b"x", &mut out),
        Err(SendError::NeedArp)
    );
    assert_eq!(b.pending_frame_len(), MIN_FRAME_LEN);
}

#[test]
fn bind_slots_are_limited() {
    let mut a = iface();
    for port in 1..=MAX_BOUND_PORTS as u16 {
        assert!(a.bind(port));
    }
    assert!(!a.bind(99));
    a.unbind(1);
    assert!(a.bind(99));
}

#[test]
fn unconfigured_interface_accepts_unicast_and_broadcast_dhcp_replies() {
    let mut a = Interface::new(Config::unconfigured(OUR_MAC));
    assert!(!a.config().is_configured());
    assert!(a.bind(crate::dhcp::DHCP_CLIENT_PORT));
    let mut out = [0u8; 1514];
    // Broadcast DISCOVER from 0.0.0.0 to 255.255.255.255 without ARP.
    let len = a
        .udp_broadcast(
            crate::dhcp::DHCP_SERVER_PORT,
            crate::dhcp::DHCP_CLIENT_PORT,
            b"disc",
            &mut out,
        )
        .unwrap();
    let (eth, ipp) = EthernetHeader::parse(&out[..len]).unwrap();
    assert_eq!(eth.dst, MAC_BROADCAST);
    let (ip, body) = Ipv4Header::parse(ipp).unwrap();
    assert_eq!(ip.src, [0, 0, 0, 0]);
    assert_eq!(ip.dst, [255, 255, 255, 255]);
    let udp = Udp::parse(body, ip.src, ip.dst).unwrap();
    assert_eq!((udp.src_port, udp.dst_port), (68, 67));

    // Replies arrive unicast to our MAC at the offered address, or broadcast.
    let mut frame = [0u8; 1514];
    let reply = Udp {
        src_port: 67,
        dst_port: 68,
        payload: b"offer",
    };
    let flen = build_udp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &reply,
    )
    .unwrap();
    assert!(matches!(
        a.receive(&frame[..flen], &mut out),
        Event::Udp { dst_port: 68, .. }
    ));
    let flen = build_udp(
        &mut frame,
        GW_MAC,
        MAC_BROADCAST,
        [10, 0, 2, 2],
        [255, 255, 255, 255],
        &reply,
    )
    .unwrap();
    assert!(matches!(
        a.receive(&frame[..flen], &mut out),
        Event::Udp { dst_port: 68, .. }
    ));

    // Once configured, unicast to a different address is no longer ours.
    a.set_config(Config::qemu_user(OUR_MAC));
    assert!(a.config().is_configured());
    let flen = build_udp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 16],
        &reply,
    )
    .unwrap();
    assert_eq!(a.receive(&frame[..flen], &mut out), Event::None);
    let flen = build_udp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &reply,
    )
    .unwrap();
    assert!(matches!(
        a.receive(&frame[..flen], &mut out),
        Event::Udp { .. }
    ));
}

#[test]
fn tcp_frames_are_built_and_parsed_through_the_interface() {
    use crate::wire::{Tcp, TCP_ACK, TCP_PSH, TCP_SYN};
    let mut a = iface();
    a.tcp_listen(7779);
    let mut out = [0u8; 1514];
    let mut frame = [0u8; 1514];
    // SYN from the gateway MAC/IP; the interface learns the neighbour and
    // answers SYN|ACK through tcp_next_frame.
    let syn = Tcp {
        src_port: 40000,
        dst_port: 7779,
        seq: 100,
        ack: 0,
        flags: TCP_SYN,
        window: 1000,
        payload: &[],
    };
    let flen = build_tcp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &syn,
        Some(1460),
    )
    .unwrap();
    assert_eq!(
        a.receive(&frame[..flen], &mut out),
        Event::TcpReady { conn: 0 }
    );
    let n = a.tcp_next_frame(&mut out).unwrap();
    let iss = {
        let (eth, ipp) = EthernetHeader::parse(&out[..n]).unwrap();
        assert_eq!(eth.dst, GW_MAC);
        let (ip, body) = Ipv4Header::parse(ipp).unwrap();
        assert_eq!(ip.protocol, IP_PROTO_TCP);
        let synack = Tcp::parse(body, ip.src, ip.dst).unwrap();
        assert_eq!(synack.flags, TCP_SYN | TCP_ACK);
        assert_eq!(synack.ack, 101);
        synack.seq
    };
    assert!(a.tcp_next_frame(&mut out).is_none());
    // Complete the handshake and send a line; the echo goes back framed.
    let ack = Tcp {
        seq: 101,
        ack: iss + 1,
        flags: TCP_ACK,
        ..syn
    };
    let flen = build_tcp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &ack,
        None,
    )
    .unwrap();
    assert_eq!(
        a.receive(&frame[..flen], &mut out),
        Event::TcpReady { conn: 0 }
    );
    let data = Tcp {
        seq: 101,
        ack: iss + 1,
        flags: TCP_ACK | TCP_PSH,
        payload: b"ping\n",
        ..syn
    };
    let flen = build_tcp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &data,
        None,
    )
    .unwrap();
    assert_eq!(
        a.receive(&frame[..flen], &mut out),
        Event::TcpReady { conn: 0 }
    );
    let mut line = [0u8; 16];
    assert_eq!(a.tcp_mut().read(0, &mut line), 5);
    assert_eq!(a.tcp_mut().write(0, b"pong\n"), 5);
    // First the ACK for the data, then our data segment.
    let n = a.tcp_next_frame(&mut out).unwrap();
    {
        let (_, ipp) = EthernetHeader::parse(&out[..n]).unwrap();
        let (ip, body) = Ipv4Header::parse(ipp).unwrap();
        assert_eq!(Tcp::parse(body, ip.src, ip.dst).unwrap().ack, 106);
    }
    let n = a.tcp_next_frame(&mut out).unwrap();
    {
        let (_, ipp) = EthernetHeader::parse(&out[..n]).unwrap();
        let (ip, body) = Ipv4Header::parse(ipp).unwrap();
        let echo = Tcp::parse(body, ip.src, ip.dst).unwrap();
        assert_eq!(echo.payload, b"pong\n");
        assert_eq!(echo.seq, iss + 1);
    }
    assert!(a.tcp_next_frame(&mut out).is_none());
}

// ---------------------------------------------------------------------------
// What a hostile but well-formed peer on the same segment can do.
// ---------------------------------------------------------------------------

#[test]
fn a_datagram_cannot_overwrite_an_arp_entry_with_its_own_source() {
    // Any accepted IPv4 packet inserted (ip.src, src_mac) into the cache,
    // before the protocol was even dispatched -- so one datagram to an
    // unbound UDP port with a forged source pointed all our off-link traffic
    // at the attacker's MAC. Linux does not learn layer-3-to-layer-2
    // bindings from data packets at all.
    let mut a = iface();
    let mut out = [0u8; 1514];

    // Learn the gateway honestly, by asking.
    a.arp_request([10, 0, 2, 2], &mut out).unwrap();
    let reply = ArpPacket {
        operation: ARP_OP_REPLY,
        sender_mac: GW_MAC,
        sender_ip: [10, 0, 2, 2],
        target_mac: OUR_MAC,
        target_ip: [10, 0, 2, 15],
    };
    let mut frame = [0u8; 1514];
    let flen = build_arp(&mut frame, GW_MAC, OUR_MAC, &reply).unwrap();
    a.receive(&frame[..flen], &mut out);
    assert_eq!(a.arp_cache().lookup([10, 0, 2, 2]), Some(GW_MAC));

    // Now an attacker sends a datagram claiming to be the gateway.
    const ATTACKER: Mac = [0xAA; 6];
    let mut frame = [0u8; 1514];
    let udp = Udp {
        src_port: 9999,
        dst_port: 9999,
        payload: b"x",
    };
    let len = build_udp(
        &mut frame,
        ATTACKER,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &udp,
    )
    .unwrap();
    a.receive(&frame[..len], &mut out);
    assert_eq!(
        a.arp_cache().lookup([10, 0, 2, 2]),
        Some(GW_MAC),
        "a data packet overwrote the gateway's hardware address"
    );
}

#[test]
fn an_unsolicited_arp_reply_is_not_believed() {
    let mut a = iface();
    let mut out = [0u8; 1514];
    const ATTACKER: Mac = [0xBB; 6];

    let reply = ArpPacket {
        operation: ARP_OP_REPLY,
        sender_mac: ATTACKER,
        sender_ip: [10, 0, 2, 2],
        target_mac: OUR_MAC,
        target_ip: [10, 0, 2, 15],
    };
    let mut frame = [0u8; 1514];
    let flen = build_arp(&mut frame, ATTACKER, OUR_MAC, &reply).unwrap();
    a.receive(&frame[..flen], &mut out);
    assert_eq!(
        a.arp_cache().lookup([10, 0, 2, 2]),
        None,
        "a reply to a question we never asked created a cache entry"
    );
}

#[test]
fn a_fragment_is_discarded_rather_than_read_as_a_whole_datagram() {
    // Flags and offset were read only to be written back on send, so a
    // non-first fragment reached the TCP or UDP parser as though its first
    // bytes were a transport header.
    let mut frame = [0u8; 1514];
    let udp = Udp {
        src_port: 1000,
        dst_port: 2000,
        payload: b"payload",
    };
    let len = build_udp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        &udp,
    )
    .unwrap();
    let (_, ipp) = EthernetHeader::parse(&frame[..len]).unwrap();
    assert!(Ipv4Header::parse(ipp).is_some(), "the whole datagram parses");

    // Set the More Fragments bit and fix the header checksum.
    let mut fragmented = frame;
    let ip_start = 14;
    fragmented[ip_start + 6] |= 0x20;
    fragmented[ip_start + 10] = 0;
    fragmented[ip_start + 11] = 0;
    let sum = checksum(&fragmented[ip_start..ip_start + 20]);
    fragmented[ip_start + 10] = (sum >> 8) as u8;
    fragmented[ip_start + 11] = sum as u8;
    let (_, ipp) = EthernetHeader::parse(&fragmented[..len]).unwrap();
    assert!(
        Ipv4Header::parse(ipp).is_none(),
        "a fragment must be discarded, not parsed as a complete datagram"
    );
}

#[test]
fn an_unresolved_next_hop_does_not_consume_the_segment() {
    // `take_reply` removed the pending reply and `poll` counted a
    // retransmission, and only then was the hardware address looked up -- so
    // an unknown next hop threw the segment away with nothing to resend it,
    // and spent retransmission attempts on segments that never reached the
    // wire. Five such passes and the connection was closed as if the peer
    // were dead. Nothing on this path asked for the address either.
    let mut a = iface();
    let mut out = [0u8; 1514];
    assert!(a.tcp_mut().listen(7000));

    // A SYN from off-subnet, so nothing is learned from it and the gateway
    // stays unresolved.
    let mut frame = [0u8; 1514];
    let seg = Tcp {
        src_port: 40000,
        dst_port: 7000,
        seq: 1000,
        ack: 0,
        flags: TCP_SYN,
        window: 65535,
        payload: &[],
    };
    let flen = build_tcp(
        &mut frame,
        GW_MAC,
        OUR_MAC,
        [8, 8, 8, 8],
        [10, 0, 2, 15],
        &seg,
        None,
    )
    .unwrap();
    a.receive(&frame[..flen], &mut out);

    let before = a.counters().arp_requests_sent;
    // The hop is unknown, so no frame comes out...
    assert_eq!(a.tcp_next_frame(&mut out), None);
    assert!(
        a.counters().arp_requests_sent > before,
        "nothing asked for the address, so nothing would ever resolve it"
    );

    // ...and the SYN-ACK is still owed once the address is known.
    a.arp_cache_mut().insert([10, 0, 2, 2], GW_MAC);
    let len = a
        .tcp_next_frame(&mut out)
        .expect("the handshake reply must still be pending");
    let (_, ipp) = EthernetHeader::parse(&out[..len]).unwrap();
    let (_, body) = Ipv4Header::parse(ipp).unwrap();
    let parsed = Tcp::parse(body, [10, 0, 2, 15], [8, 8, 8, 8]).unwrap();
    assert_eq!(parsed.flags, TCP_SYN | TCP_ACK);
}
