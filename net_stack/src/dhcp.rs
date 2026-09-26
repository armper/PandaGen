//! DHCP client (RFC 2131): DISCOVER, OFFER, REQUEST, ACK over UDP 68/67.
//!
//! The client only builds and parses BOOTP payloads and tracks its state;
//! the caller broadcasts the payloads through the interface and feeds
//! back datagrams that arrive on port 68.

use crate::{Config, Ipv4, Mac};

pub const DHCP_SERVER_PORT: u16 = 67;
pub const DHCP_CLIENT_PORT: u16 = 68;

/// Shortest lease the client will honour. Renewal happens at half the lease
/// and spins for up to a second with the network lock held, so a very short
/// lease is a way for a server to keep this machine busy.
pub const MIN_LEASE_SECONDS: u32 = 60;
/// Longest, so a server cannot park an address indefinitely.
pub const MAX_LEASE_SECONDS: u32 = 7 * 24 * 60 * 60;
/// Used when the server omits option 51, rather than never renewing.
pub const DEFAULT_LEASE_SECONDS: u32 = 3600;

/// Whether an address is one a host may actually use.
fn usable_address(ip: [u8; 4]) -> bool {
    ip != [0, 0, 0, 0]
        && ip[0] != 127
        && ip[0] & 0xF0 != 0xE0 // multicast
        && ip != [255, 255, 255, 255]
}

/// Whether a netmask is a run of ones followed by a run of zeroes.
fn contiguous_netmask(mask: [u8; 4]) -> bool {
    let value = u32::from_be_bytes(mask);
    value != 0 && (!value).wrapping_add(1).count_ones() <= 1
}
/// Minimum BOOTP payload; shorter requests are padded.
pub const MIN_PAYLOAD: usize = 300;
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MESSAGE_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_PARAMETER_LIST: u8 = 55;
const OPT_CLIENT_ID: u8 = 61;
const OPT_END: u8 = 255;

pub const MSG_DISCOVER: u8 = 1;
pub const MSG_OFFER: u8 = 2;
pub const MSG_REQUEST: u8 = 3;
pub const MSG_ACK: u8 = 5;
pub const MSG_NAK: u8 = 6;

/// Fields of a server reply the client cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    pub message_type: u8,
    pub xid: u32,
    pub your_ip: Ipv4,
    pub subnet_mask: Option<Ipv4>,
    pub router: Option<Ipv4>,
    pub dns: Option<Ipv4>,
    pub server_id: Option<Ipv4>,
    pub lease_seconds: Option<u32>,
}

/// Parse a BOOTREPLY; `None` for anything that is not a well-formed DHCP reply.
pub fn parse_reply(p: &[u8]) -> Option<Reply> {
    if p.len() < 240 || p[0] != 2 || p[236..240] != MAGIC_COOKIE {
        return None;
    }
    let xid = u32::from_be_bytes([p[4], p[5], p[6], p[7]]);
    let mut your_ip = [0u8; 4];
    your_ip.copy_from_slice(&p[16..20]);
    let mut reply = Reply {
        message_type: 0,
        xid,
        your_ip,
        subnet_mask: None,
        router: None,
        dns: None,
        server_id: None,
        lease_seconds: None,
    };
    let mut i = 240;
    while i < p.len() {
        let code = p[i];
        if code == OPT_END {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = *p.get(i + 1)? as usize;
        let value = p.get(i + 2..i + 2 + len)?;
        let ip = |v: &[u8]| -> Option<Ipv4> {
            if v.len() >= 4 {
                Some([v[0], v[1], v[2], v[3]])
            } else {
                None
            }
        };
        match code {
            OPT_MESSAGE_TYPE => reply.message_type = *value.first()?,
            OPT_SUBNET_MASK => reply.subnet_mask = ip(value),
            OPT_ROUTER => reply.router = ip(value),
            OPT_DNS => reply.dns = ip(value),
            OPT_SERVER_ID => reply.server_id = ip(value),
            OPT_LEASE_TIME => {
                if value.len() >= 4 {
                    reply.lease_seconds =
                        Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
                }
            }
            _ => {}
        }
        i += 2 + len;
    }
    if reply.message_type == 0 {
        return None;
    }
    Some(reply)
}

/// Write a BOOTREQUEST with the given message type into `out`; `request`
/// carries the offered address and server for a REQUEST.
pub fn build_request(
    out: &mut [u8],
    xid: u32,
    mac: Mac,
    message_type: u8,
    request: Option<(Ipv4, Ipv4)>,
) -> Option<usize> {
    build_request_with_ciaddr(out, xid, mac, message_type, request, None)
}

/// `build_request` with an optional client address (`ciaddr`), which a
/// renewing client sets instead of the requested-address option.
pub fn build_request_with_ciaddr(
    out: &mut [u8],
    xid: u32,
    mac: Mac,
    message_type: u8,
    request: Option<(Ipv4, Ipv4)>,
    ciaddr: Option<Ipv4>,
) -> Option<usize> {
    if out.len() < MIN_PAYLOAD {
        return None;
    }
    out[..MIN_PAYLOAD].fill(0);
    out[0] = 1; // BOOTREQUEST
    out[1] = 1; // Ethernet
    out[2] = 6;
    out[4..8].copy_from_slice(&xid.to_be_bytes());
    out[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); // ask for broadcast replies
    if let Some(ip) = ciaddr {
        out[12..16].copy_from_slice(&ip);
    }
    out[28..34].copy_from_slice(&mac);
    out[236..240].copy_from_slice(&MAGIC_COOKIE);
    let mut i = 240;
    let mut put = |bytes: &[u8]| {
        out[i..i + bytes.len()].copy_from_slice(bytes);
        i += bytes.len();
    };
    put(&[OPT_MESSAGE_TYPE, 1, message_type]);
    put(&[OPT_CLIENT_ID, 7, 1]);
    put(&mac);
    if let Some((ip, server)) = request {
        put(&[OPT_REQUESTED_IP, 4]);
        put(&ip);
        put(&[OPT_SERVER_ID, 4]);
        put(&server);
    }
    put(&[OPT_PARAMETER_LIST, 3, OPT_SUBNET_MASK, OPT_ROUTER, OPT_DNS]);
    put(&[OPT_END]);
    Some(i.max(MIN_PAYLOAD))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Init,
    Selecting,
    Requesting { offered: Ipv4, server: Ipv4 },
    Bound,
}

/// What the caller should do after feeding the client a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing to send.
    None,
    /// Broadcast the payload written into the scratch buffer (this many bytes).
    Send(usize),
    /// The lease is bound; apply this configuration.
    Bound {
        config: Config,
        lease_seconds: u32,
        dns: Option<Ipv4>,
        /// Server to renew with later.
        server: Ipv4,
    },
}

pub struct Client {
    mac: Mac,
    xid: u32,
    state: State,
}

impl Client {
    pub const fn new(mac: Mac, xid: u32) -> Self {
        Self {
            mac,
            xid,
            state: State::Init,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn xid(&self) -> u32 {
        self.xid
    }

    /// Write a DISCOVER into `out` and enter `Selecting`.
    pub fn discover(&mut self, out: &mut [u8]) -> Option<usize> {
        let len = build_request(out, self.xid, self.mac, MSG_DISCOVER, None)?;
        self.state = State::Selecting;
        Some(len)
    }

    /// Start a renewal of `ip` with `server` (RFC 2131 RENEWING): a REQUEST
    /// carrying `ciaddr`, to be sent unicast to the server. The ACK is
    /// handled like the initial one and yields `Step::Bound`.
    pub fn renew(&mut self, ip: Ipv4, server: Ipv4, out: &mut [u8]) -> Option<usize> {
        self.xid = self.xid.wrapping_add(1);
        let len = build_request_with_ciaddr(out, self.xid, self.mac, MSG_REQUEST, None, Some(ip))?;
        self.state = State::Requesting {
            offered: ip,
            server,
        };
        Some(len)
    }

    /// Feed a datagram received on port 68.
    pub fn handle(&mut self, payload: &[u8], out: &mut [u8]) -> Step {
        let Some(reply) = parse_reply(payload) else {
            return Step::None;
        };
        if reply.xid != self.xid {
            return Step::None;
        }
        match (self.state, reply.message_type) {
            (State::Selecting, MSG_OFFER) => {
                let Some(server) = reply.server_id else {
                    return Step::None;
                };
                match build_request(
                    out,
                    self.xid,
                    self.mac,
                    MSG_REQUEST,
                    Some((reply.your_ip, server)),
                ) {
                    Some(len) => {
                        self.state = State::Requesting {
                            offered: reply.your_ip,
                            server,
                        };
                        Step::Send(len)
                    }
                    None => Step::None,
                }
            }
            (State::Requesting { offered, server }, MSG_ACK) if reply.your_ip == offered => {
                // The kernel trusts whatever answers, so check what it said.
                // An address of 0.0.0.0, loopback or multicast, or a netmask
                // that is not a run of ones, is not a usable configuration
                // and used to be accepted anyway.
                if !usable_address(reply.your_ip) {
                    self.state = State::Init;
                    return Step::None;
                }
                let netmask = reply.subnet_mask.unwrap_or([255, 255, 255, 0]);
                if !contiguous_netmask(netmask) {
                    self.state = State::Init;
                    return Step::None;
                }
                self.state = State::Bound;
                Step::Bound {
                    server: reply.server_id.unwrap_or(server),
                    config: Config {
                        mac: self.mac,
                        ip: reply.your_ip,
                        netmask,
                        gateway: reply.router.unwrap_or(reply.server_id.unwrap_or([0; 4])),
                    },
                    // Floored and capped. A lease of one second made the
                    // kernel renew twice a second, and each renewal spins
                    // for up to a second waiting for a reply with the
                    // network lock held -- so a rogue server that answered
                    // slowly could keep the machine inside that loop for as
                    // long as it liked. A lease of zero disabled renewal
                    // for ever, so the kernel kept an address the server had
                    // since given to somebody else.
                    lease_seconds: reply
                        .lease_seconds
                        .unwrap_or(DEFAULT_LEASE_SECONDS)
                        .clamp(MIN_LEASE_SECONDS, MAX_LEASE_SECONDS),
                    dns: reply.dns,
                }
            }
            (State::Requesting { .. }, MSG_NAK) => {
                self.state = State::Init;
                Step::None
            }
            _ => Step::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: Mac = [0x52, 0x54, 0, 0x12, 0x34, 0x56];

    fn reply(kind: u8, xid: u32, your_ip: Ipv4, extra: &[u8]) -> [u8; 320] {
        let mut p = [0u8; 320];
        p[0] = 2;
        p[4..8].copy_from_slice(&xid.to_be_bytes());
        p[16..20].copy_from_slice(&your_ip);
        p[236..240].copy_from_slice(&MAGIC_COOKIE);
        let mut i = 240;
        p[i..i + 3].copy_from_slice(&[OPT_MESSAGE_TYPE, 1, kind]);
        i += 3;
        p[i..i + extra.len()].copy_from_slice(extra);
        i += extra.len();
        p[i] = OPT_END;
        p
    }

    const SERVER_OPTS: &[u8] = &[
        OPT_SERVER_ID,
        4,
        10,
        0,
        2,
        2,
        OPT_SUBNET_MASK,
        4,
        255,
        255,
        255,
        0,
        OPT_ROUTER,
        4,
        10,
        0,
        2,
        2,
        OPT_DNS,
        4,
        10,
        0,
        2,
        3,
        OPT_LEASE_TIME,
        4,
        0,
        1,
        0x51,
        0x80,
    ];

    #[test]
    fn discover_has_bootp_header_and_options() {
        let mut client = Client::new(MAC, 0xDEADBEEF);
        let mut buf = [0u8; 400];
        let len = client.discover(&mut buf).unwrap();
        assert_eq!(len, MIN_PAYLOAD);
        assert_eq!(client.state(), State::Selecting);
        assert_eq!(buf[0], 1);
        assert_eq!(&buf[4..8], &0xDEADBEEFu32.to_be_bytes());
        assert_eq!(&buf[10..12], &[0x80, 0]);
        assert_eq!(&buf[28..34], &MAC);
        assert_eq!(&buf[236..240], &MAGIC_COOKIE);
        assert_eq!(&buf[240..243], &[OPT_MESSAGE_TYPE, 1, MSG_DISCOVER]);
        assert!(buf[243..len]
            .windows(2)
            .any(|w| w == [OPT_PARAMETER_LIST, 3]));
        assert!(build_request(&mut buf[..100], 1, MAC, MSG_DISCOVER, None).is_none());
    }

    #[test]
    fn parse_reply_reads_options_and_rejects_garbage() {
        let p = reply(MSG_OFFER, 7, [10, 0, 2, 15], SERVER_OPTS);
        let r = parse_reply(&p).unwrap();
        assert_eq!(r.message_type, MSG_OFFER);
        assert_eq!(r.xid, 7);
        assert_eq!(r.your_ip, [10, 0, 2, 15]);
        assert_eq!(r.subnet_mask, Some([255, 255, 255, 0]));
        assert_eq!(r.router, Some([10, 0, 2, 2]));
        assert_eq!(r.dns, Some([10, 0, 2, 3]));
        assert_eq!(r.server_id, Some([10, 0, 2, 2]));
        assert_eq!(r.lease_seconds, Some(86400));
        assert!(parse_reply(&p[..200]).is_none(), "truncated");
        let mut bad = p;
        bad[0] = 1;
        assert!(parse_reply(&bad).is_none(), "request, not reply");
        let mut no_cookie = p;
        no_cookie[236] = 0;
        assert!(parse_reply(&no_cookie).is_none());
        let no_type = reply(0, 7, [10, 0, 2, 15], &[]);
        assert!(parse_reply(&no_type).is_none());
    }

    #[test]
    fn full_handshake_binds_with_offered_address() {
        let mut client = Client::new(MAC, 42);
        let mut buf = [0u8; 400];
        client.discover(&mut buf).unwrap();

        // Offers with a foreign xid are ignored.
        let foreign = reply(MSG_OFFER, 43, [10, 0, 2, 15], SERVER_OPTS);
        assert_eq!(client.handle(&foreign, &mut buf), Step::None);
        assert_eq!(client.state(), State::Selecting);

        let offer = reply(MSG_OFFER, 42, [10, 0, 2, 15], SERVER_OPTS);
        match client.handle(&offer, &mut buf) {
            Step::Send(len) => {
                assert_eq!(len, MIN_PAYLOAD);
                assert_eq!(&buf[240..243], &[OPT_MESSAGE_TYPE, 1, MSG_REQUEST]);
                let opts = &buf[243..len];
                assert!(opts
                    .windows(6)
                    .any(|w| w == [OPT_REQUESTED_IP, 4, 10, 0, 2, 15]));
                assert!(opts
                    .windows(6)
                    .any(|w| w == [OPT_SERVER_ID, 4, 10, 0, 2, 2]));
            }
            other => panic!("expected request, got {other:?}"),
        }
        assert_eq!(
            client.state(),
            State::Requesting {
                offered: [10, 0, 2, 15],
                server: [10, 0, 2, 2]
            }
        );

        // An ACK for a different address is ignored; the right one binds.
        let wrong = reply(MSG_ACK, 42, [10, 0, 2, 16], SERVER_OPTS);
        assert_eq!(client.handle(&wrong, &mut buf), Step::None);
        let ack = reply(MSG_ACK, 42, [10, 0, 2, 15], SERVER_OPTS);
        assert_eq!(
            client.handle(&ack, &mut buf),
            Step::Bound {
                config: Config {
                    mac: MAC,
                    ip: [10, 0, 2, 15],
                    netmask: [255, 255, 255, 0],
                    gateway: [10, 0, 2, 2],
                },
                lease_seconds: 86400,
                dns: Some([10, 0, 2, 3]),
                server: [10, 0, 2, 2],
            }
        );
        assert_eq!(client.state(), State::Bound);
    }

    #[test]
    fn renewal_requests_with_ciaddr_and_rebinds_on_ack() {
        let mut client = Client::new(MAC, 9);
        let mut buf = [0u8; 400];
        let len = client
            .renew([10, 0, 2, 15], [10, 0, 2, 2], &mut buf)
            .unwrap();
        assert_eq!(len, MIN_PAYLOAD);
        assert_eq!(
            &buf[12..16],
            &[10, 0, 2, 15],
            "ciaddr carries the current address"
        );
        assert_eq!(&buf[240..243], &[OPT_MESSAGE_TYPE, 1, MSG_REQUEST]);
        let opts = &buf[243..len];
        assert!(!opts.windows(2).any(|w| w == [OPT_REQUESTED_IP, 4]));
        assert!(!opts.windows(2).any(|w| w == [OPT_SERVER_ID, 4]));
        let xid = client.xid();
        assert_eq!(
            client.state(),
            State::Requesting {
                offered: [10, 0, 2, 15],
                server: [10, 0, 2, 2]
            }
        );
        let ack = reply(MSG_ACK, xid, [10, 0, 2, 15], SERVER_OPTS);
        match client.handle(&ack, &mut buf) {
            Step::Bound {
                config,
                lease_seconds,
                server,
                ..
            } => {
                assert_eq!(config.ip, [10, 0, 2, 15]);
                assert_eq!(lease_seconds, 86400);
                assert_eq!(server, [10, 0, 2, 2]);
            }
            other => panic!("expected bound, got {other:?}"),
        }
    }

    #[test]
    fn nak_returns_to_init() {
        let mut client = Client::new(MAC, 5);
        let mut buf = [0u8; 400];
        client.discover(&mut buf).unwrap();
        let offer = reply(MSG_OFFER, 5, [10, 0, 2, 15], SERVER_OPTS);
        assert!(matches!(client.handle(&offer, &mut buf), Step::Send(_)));
        let nak = reply(MSG_NAK, 5, [0; 4], &[OPT_SERVER_ID, 4, 10, 0, 2, 2]);
        assert_eq!(client.handle(&nak, &mut buf), Step::None);
        assert_eq!(client.state(), State::Init);
    }
}

#[cfg(test)]
mod hostile_server_tests {
    use super::*;

    #[test]
    fn a_netmask_that_is_not_a_run_of_ones_is_refused() {
        assert!(contiguous_netmask([255, 255, 255, 0]));
        assert!(contiguous_netmask([255, 255, 255, 255]));
        assert!(contiguous_netmask([128, 0, 0, 0]));
        // A mask of zero puts everything on-link and the gateway is never
        // used, so nothing off the segment is ever reachable.
        assert!(!contiguous_netmask([0, 0, 0, 0]));
        // Non-contiguous masks are not a thing hosts implement.
        assert!(!contiguous_netmask([255, 0, 255, 0]));
        assert!(!contiguous_netmask([255, 255, 0, 1]));
    }

    #[test]
    fn an_address_a_host_cannot_use_is_refused() {
        assert!(usable_address([10, 0, 2, 15]));
        assert!(!usable_address([0, 0, 0, 0]));
        assert!(!usable_address([127, 0, 0, 1]));
        assert!(!usable_address([224, 0, 0, 1]), "multicast");
        assert!(!usable_address([239, 1, 2, 3]), "multicast");
        assert!(!usable_address([255, 255, 255, 255]));
    }

    #[test]
    fn a_lease_is_floored_and_capped() {
        // Renewal happens at half the lease and spins for up to a second
        // with the network lock held, so a one-second lease made the kernel
        // renew twice a second -- a rogue server that answered slowly could
        // keep the machine in that loop for as long as it liked. A lease of
        // zero disabled renewal entirely, so the kernel kept an address the
        // server had since given to someone else.
        assert_eq!(
            1u32.clamp(MIN_LEASE_SECONDS, MAX_LEASE_SECONDS),
            MIN_LEASE_SECONDS
        );
        assert_eq!(
            u32::MAX.clamp(MIN_LEASE_SECONDS, MAX_LEASE_SECONDS),
            MAX_LEASE_SECONDS
        );
        const {
            assert!(MIN_LEASE_SECONDS >= 60, "half a lease must be many seconds");
            assert!(DEFAULT_LEASE_SECONDS >= MIN_LEASE_SECONDS);
        }
    }
}
