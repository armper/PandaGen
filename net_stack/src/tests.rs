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
