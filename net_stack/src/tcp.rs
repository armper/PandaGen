//! Minimal TCP for a machine that talks to a handful of peers at a time.
//!
//! Passive open for its servers and active open (`connect`) for its own
//! requests (NET-030), in-order receive (out-of-order segments are dropped
//! and re-acknowledged), one outstanding send segment with a fixed
//! retransmission timeout, and both close directions. Enough for line
//! protocols and HTTP; not a general-purpose stack.

use crate::wire::{TCP_ACK, TCP_FIN, TCP_PSH, TCP_RST, TCP_SYN};
use crate::Ipv4;

/// Connections tracked at once.
pub const MAX_CONNECTIONS: usize = 8;
/// Most of the table one listen port may occupy. The remainder is reserved,
/// so saturating the unauthenticated echo port cannot take the signed command
/// port offline.
pub const MAX_PER_PORT: usize = MAX_CONNECTIONS - 2;
/// Listen ports served at once.
pub const MAX_LISTEN_PORTS: usize = 4;
/// Connections this machine may open itself at once. The rest of the table
/// stays for its servers, so a runaway client cannot take them offline.
pub const MAX_OUTBOUND: usize = 2;
/// First ephemeral port for connections we open (RFC 6335's dynamic range).
pub const EPHEMERAL_FIRST: u16 = 49152;
/// Backstop for a half-open connection (100 Hz ticks). Retransmission
/// normally abandons it first, after `MAX_RETRIES` attempts one `RTO_TICKS`
/// apart; this only catches a connection the retransmit path somehow misses.
pub const SYN_TIMEOUT_TICKS: u64 = RTO_TICKS * (MAX_RETRIES as u64) + 300;
/// An established connection that says nothing for this long is reaped. With
/// a small table an idle peer is indistinguishable from a vanished one.
pub const IDLE_TIMEOUT_TICKS: u64 = 12_000;
/// A connection in a closing state is given this long to finish.
pub const CLOSING_TIMEOUT_TICKS: u64 = 1_000;
/// Receive and send buffer bytes per connection.
pub const BUFFER_BYTES: usize = 2048;
/// Largest payload we send in one segment.
pub const MSS: u16 = 1200;
/// Retransmission timeout in ticks (100 Hz).
pub const RTO_TICKS: u64 = 100;
/// Give up after this many retransmissions.
pub const MAX_RETRIES: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Closed,
    Listen,
    /// We sent a SYN and wait for the SYN|ACK (active open, NET-030).
    SynSent,
    SynReceived,
    Established,
    CloseWait,
    LastAck,
    FinWait1,
    FinWait2,
}

/// A segment to transmit: the caller frames it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outgoing {
    pub peer: Ipv4,
    pub peer_port: u16,
    pub local_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    /// Payload as (offset into the connection's send buffer, length).
    pub payload: (usize, usize),
    pub mss: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment<'a> {
    pub src: Ipv4,
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload: &'a [u8],
}

pub struct Connection {
    pub state: State,
    pub peer: Ipv4,
    pub peer_port: u16,
    pub local_port: u16,
    /// Next sequence number we expect from the peer.
    rcv_nxt: u32,
    /// Oldest unacknowledged sequence number of ours.
    snd_una: u32,
    /// Next sequence number we will send.
    snd_nxt: u32,
    /// Peer's advertised window.
    snd_wnd: u16,
    rx: [u8; BUFFER_BYTES],
    rx_len: usize,
    /// Bytes queued by the application; `tx[..tx_unacked]` are in flight.
    tx: [u8; BUFFER_BYTES],
    tx_len: usize,
    tx_unacked: usize,
    /// Tick of the last transmission of the in-flight data or FIN.
    last_send_tick: u64,
    /// Tick of the last segment exchanged either way, for reaping.
    last_activity: u64,
    retries: u8,
    /// FIN queued by the application (after all data).
    fin_pending: bool,
    fin_sent: bool,
    /// Peer sent a FIN that the application has not seen yet.
    peer_closed: bool,
    /// We opened this connection (`connect`), rather than accepting it.
    pub outbound: bool,
    /// Our SYN has not been sent yet: `poll` sends it first thing.
    syn_unsent: bool,
    /// The application read from a buffer that had (nearly) closed our
    /// window: the peer must be told it has room again, or a sender
    /// waiting on a zero window waits forever.
    window_update: bool,
}

impl Connection {
    const fn closed() -> Self {
        Self {
            state: State::Closed,
            peer: [0; 4],
            peer_port: 0,
            local_port: 0,
            rcv_nxt: 0,
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            rx: [0; BUFFER_BYTES],
            rx_len: 0,
            tx: [0; BUFFER_BYTES],
            tx_len: 0,
            tx_unacked: 0,
            last_send_tick: 0,
            last_activity: 0,
            retries: 0,
            fin_pending: false,
            fin_sent: false,
            peer_closed: false,
            outbound: false,
            syn_unsent: false,
            window_update: false,
        }
    }

    fn window(&self) -> u16 {
        (BUFFER_BYTES - self.rx_len).min(u16::MAX as usize) as u16
    }

    /// Bytes the application may read.
    pub fn readable(&self) -> usize {
        self.rx_len
    }

    /// The buffered bytes without consuming them. A protocol whose message
    /// boundary is not a newline (HTTP's blank line, for instance) needs to
    /// inspect what has arrived before deciding to take it.
    pub fn peek(&self) -> &[u8] {
        &self.rx[..self.rx_len]
    }

    /// Whether the handshake is done and data may be written.
    pub fn is_established(&self) -> bool {
        matches!(self.state, State::Established | State::CloseWait)
    }

    /// Room left in the send buffer.
    pub fn writable(&self) -> usize {
        BUFFER_BYTES - self.tx_len
    }

    pub fn peer_closed(&self) -> bool {
        self.peer_closed
    }

    pub fn send_buffer(&self) -> &[u8] {
        &self.tx[..self.tx_len]
    }
}

pub struct Tcp {
    listen_ports: [Option<u16>; MAX_LISTEN_PORTS],
    conns: [Connection; MAX_CONNECTIONS],
    next_iss: u32,
    now: u64,
    /// Immediate replies (ACK, SYN|ACK, RST) produced by `receive`.
    reply: Option<Outgoing>,
    pub segments_in: u64,
    pub segments_out: u64,
    pub retransmits: u64,
    pub resets_sent: u64,
    pub accepted: u64,
    /// Connections refused because the table (or this port's share) was full.
    pub refused: u64,
    /// Connections closed by the reaper or by exhausted retransmissions.
    pub reaped: u64,
    /// Peers `poll` should pass over, set for the duration of one
    /// `poll_skipping` call. A peer whose hardware address the caller cannot
    /// resolve must not hold up every other connection's output.
    skip_peers: [Option<Ipv4>; MAX_CONNECTIONS],
    /// The next ephemeral port to try.
    next_port: u16,
    /// Connections opened with `connect`.
    pub opened: u64,
}

fn seq_le(a: u32, b: u32) -> bool {
    (b.wrapping_sub(a) as i32) >= 0
}

fn seq_lt(a: u32, b: u32) -> bool {
    (b.wrapping_sub(a) as i32) > 0
}

impl Tcp {
    pub const fn new() -> Self {
        Self {
            listen_ports: [None; MAX_LISTEN_PORTS],
            conns: [const { Connection::closed() }; MAX_CONNECTIONS],
            next_iss: 0x1000,
            now: 0,
            reply: None,
            segments_in: 0,
            segments_out: 0,
            retransmits: 0,
            resets_sent: 0,
            accepted: 0,
            refused: 0,
            reaped: 0,
            skip_peers: [None; MAX_CONNECTIONS],
            next_port: EPHEMERAL_FIRST,
            opened: 0,
        }
    }

    /// Open a connection to `peer:peer_port` (NET-030): a SYN goes out on
    /// the next `poll`, and the connection is `Established` once the
    /// SYN|ACK arrives -- `receive` reports it then. `None` when this
    /// machine already has `MAX_OUTBOUND` connections of its own open, or
    /// the table is full.
    pub fn connect(&mut self, peer: Ipv4, peer_port: u16) -> Option<usize> {
        let outbound = self
            .conns
            .iter()
            .filter(|c| c.state != State::Closed && c.outbound)
            .count();
        if outbound >= MAX_OUTBOUND {
            return None;
        }
        let index = self.conns.iter().position(|c| c.state == State::Closed)?;
        let local_port = self.ephemeral_port(peer, peer_port)?;
        let iss = self.next_iss;
        self.next_iss = self
            .next_iss
            .wrapping_add(64_000)
            .wrapping_add(self.now as u32);
        let conn = &mut self.conns[index];
        *conn = Connection::closed();
        conn.state = State::SynSent;
        conn.outbound = true;
        conn.syn_unsent = true;
        conn.peer = peer;
        conn.peer_port = peer_port;
        conn.local_port = local_port;
        conn.snd_una = iss;
        conn.snd_nxt = iss.wrapping_add(1);
        conn.last_send_tick = self.now;
        conn.last_activity = self.now;
        self.opened += 1;
        Some(index)
    }

    /// A local port no connection to this peer and no listener uses.
    fn ephemeral_port(&mut self, peer: Ipv4, peer_port: u16) -> Option<u16> {
        let span = u16::MAX - EPHEMERAL_FIRST + 1;
        for _ in 0..span {
            let port = self.next_port;
            self.next_port = if port == u16::MAX {
                EPHEMERAL_FIRST
            } else {
                port + 1
            };
            if !self.is_listening(port) && self.find(peer, peer_port, port).is_none() {
                return Some(port);
            }
        }
        None
    }

    /// Accept connections on `port`. Returns false when there is no free
    /// slot, so a caller cannot silently fail to listen: an unbound port
    /// resets every client that reaches it.
    pub fn listen(&mut self, port: u16) -> bool {
        if self.listen_ports.contains(&Some(port)) {
            return true;
        }
        match self.listen_ports.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some(port);
                true
            }
            None => false,
        }
    }

    pub fn is_listening(&self, port: u16) -> bool {
        self.listen_ports.contains(&Some(port))
    }

    /// The tick the caller last set. Services need it to run deadlines of
    /// their own -- TCP's reaper only asks whether a segment arrived, which
    /// says nothing about whether a request is making progress.
    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn connection(&self, index: usize) -> Option<&Connection> {
        self.conns.get(index).filter(|c| c.state != State::Closed)
    }

    pub fn connections(&self) -> impl Iterator<Item = (usize, &Connection)> {
        self.conns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.state != State::Closed)
    }

    fn find(&self, peer: Ipv4, peer_port: u16, local_port: u16) -> Option<usize> {
        self.conns.iter().position(|c| {
            c.state != State::Closed
                && c.peer == peer
                && c.peer_port == peer_port
                && c.local_port == local_port
        })
    }

    /// Advance time (retransmission timers use it).
    pub fn set_now(&mut self, now: u64) {
        self.now = now;
        self.reap();
    }

    /// Close connections that have gone quiet, so the table cannot be held
    /// shut by peers that connect and never speak again.
    fn reap(&mut self) {
        let now = self.now;
        let mut reaped = 0;
        for conn in self.conns.iter_mut() {
            let limit = match conn.state {
                State::Closed => continue,
                State::SynReceived | State::SynSent => SYN_TIMEOUT_TICKS,
                State::Established => IDLE_TIMEOUT_TICKS,
                // CloseWait means the peer is gone and nothing further will
                // ever arrive; only our own side is still open. Holding a
                // scarce slot for the full idle timeout on a peer that has
                // already left is indefensible, and it is the backstop for
                // any service that forgets to close its half.
                State::CloseWait => CLOSING_TIMEOUT_TICKS,
                _ => CLOSING_TIMEOUT_TICKS,
            };
            if now.saturating_sub(conn.last_activity) >= limit {
                conn.state = State::Closed;
                reaped += 1;
            }
        }
        self.reaped += reaped;
    }

    /// Feed a received segment. Returns the connection that gained
    /// readable data or changed state, if any; any immediate reply is
    /// available through `take_reply`.
    pub fn receive(&mut self, seg: Segment<'_>) -> Option<usize> {
        self.segments_in += 1;
        match self.find(seg.src, seg.src_port, seg.dst_port) {
            Some(index) => {
                self.conns[index].last_activity = self.now;
                self.receive_on(index, seg)
            }
            None => {
                let opening = seg.flags & TCP_SYN != 0 && seg.flags & TCP_ACK == 0;
                if opening && self.is_listening(seg.dst_port) {
                    if let Some(index) = self.accept(seg) {
                        return Some(index);
                    }
                    // No room. Refuse with a reset so the client fails fast
                    // rather than waiting out its own connect timeout.
                    self.refused += 1;
                }
                if seg.flags & TCP_RST == 0 {
                    // Nothing listening here: reset the sender.
                    let (seq, ack, flags) = if seg.flags & TCP_ACK != 0 {
                        (seg.ack, 0, TCP_RST)
                    } else {
                        (
                            0,
                            seg.seq
                                .wrapping_add(seg.payload.len() as u32)
                                .wrapping_add(u32::from(seg.flags & TCP_SYN != 0)),
                            TCP_RST | TCP_ACK,
                        )
                    };
                    self.reply = Some(Outgoing {
                        peer: seg.src,
                        peer_port: seg.src_port,
                        local_port: seg.dst_port,
                        seq,
                        ack,
                        flags,
                        window: 0,
                        payload: (0, 0),
                        mss: None,
                    });
                    self.resets_sent += 1;
                }
                None
            }
        }
    }

    fn accept(&mut self, seg: Segment<'_>) -> Option<usize> {
        // A single listen port may not consume the whole table.
        let on_this_port = self
            .conns
            .iter()
            .filter(|c| c.state != State::Closed && c.local_port == seg.dst_port)
            .count();
        if on_this_port >= MAX_PER_PORT {
            return None;
        }
        let index = self.conns.iter().position(|c| c.state == State::Closed)?;
        let iss = self.next_iss;
        self.next_iss = self
            .next_iss
            .wrapping_add(64_000)
            .wrapping_add(self.now as u32);
        let conn = &mut self.conns[index];
        *conn = Connection::closed();
        conn.state = State::SynReceived;
        conn.peer = seg.src;
        conn.peer_port = seg.src_port;
        conn.local_port = seg.dst_port;
        conn.rcv_nxt = seg.seq.wrapping_add(1);
        conn.snd_una = iss;
        conn.snd_nxt = iss.wrapping_add(1);
        conn.snd_wnd = seg.window;
        conn.last_send_tick = self.now;
        conn.last_activity = self.now;
        self.reply = Some(Outgoing {
            peer: seg.src,
            peer_port: seg.src_port,
            local_port: seg.dst_port,
            seq: iss,
            ack: conn.rcv_nxt,
            flags: TCP_SYN | TCP_ACK,
            window: conn.window(),
            payload: (0, 0),
            mss: Some(MSS),
        });
        self.accepted += 1;
        Some(index)
    }

    fn syn_of(conn: &Connection) -> Outgoing {
        Outgoing {
            peer: conn.peer,
            peer_port: conn.peer_port,
            local_port: conn.local_port,
            seq: conn.snd_una,
            ack: 0,
            flags: TCP_SYN,
            window: conn.window(),
            payload: (0, 0),
            mss: Some(MSS),
        }
    }

    fn ack_of(conn: &Connection) -> Outgoing {
        Outgoing {
            peer: conn.peer,
            peer_port: conn.peer_port,
            local_port: conn.local_port,
            seq: conn.snd_nxt,
            ack: conn.rcv_nxt,
            flags: TCP_ACK,
            window: conn.window(),
            payload: (0, 0),
            mss: None,
        }
    }

    fn receive_on(&mut self, index: usize, seg: Segment<'_>) -> Option<usize> {
        let conn = &mut self.conns[index];
        // Our SYN is out: only its answer counts (RFC 9293 3.10.7.3).
        if conn.state == State::SynSent {
            let acks_syn = seg.flags & TCP_ACK != 0 && seg.ack == conn.snd_nxt;
            if seg.flags & TCP_RST != 0 {
                // Refused -- but only a reset that answers our SYN; any
                // other is a guess at the port and is ignored.
                if acks_syn {
                    conn.state = State::Closed;
                    return Some(index);
                }
                return None;
            }
            if seg.flags & TCP_SYN != 0 && acks_syn {
                conn.rcv_nxt = seg.seq.wrapping_add(1);
                conn.snd_una = seg.ack;
                conn.snd_wnd = seg.window;
                conn.state = State::Established;
                conn.retries = 0;
                self.reply = Some(Self::ack_of(conn));
                return Some(index);
            }
            return None;
        }
        if seg.flags & TCP_RST != 0 {
            conn.state = State::Closed;
            return Some(index);
        }
        let mut changed = false;
        // Acknowledgements.
        if seg.flags & TCP_ACK != 0 {
            if conn.state == State::SynReceived && seg.ack == conn.snd_nxt {
                conn.state = State::Established;
                conn.snd_una = seg.ack;
                changed = true;
            } else if seq_lt(conn.snd_una, seg.ack) && seq_le(seg.ack, conn.snd_nxt) {
                let newly = seg.ack.wrapping_sub(conn.snd_una) as usize;
                // A FIN we sent occupies one sequence number beyond the data.
                let data_acked = newly.min(conn.tx_unacked);
                conn.tx.copy_within(data_acked..conn.tx_len, 0);
                conn.tx_len -= data_acked;
                conn.tx_unacked -= data_acked;
                conn.snd_una = seg.ack;
                conn.retries = 0;
                if conn.fin_sent && seg.ack == conn.snd_nxt {
                    conn.state = match conn.state {
                        State::LastAck => State::Closed,
                        State::FinWait1 => State::FinWait2,
                        other => other,
                    };
                    changed = true;
                }
            }
            conn.snd_wnd = seg.window;
        }
        if conn.state == State::Closed || conn.state == State::SynReceived {
            return changed.then_some(index);
        }
        // Data and FIN, in order only.
        let mut need_ack = false;
        if !seg.payload.is_empty() {
            if seg.seq == conn.rcv_nxt && !conn.peer_closed {
                let room = BUFFER_BYTES - conn.rx_len;
                let take = seg.payload.len().min(room);
                conn.rx[conn.rx_len..conn.rx_len + take].copy_from_slice(&seg.payload[..take]);
                conn.rx_len += take;
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(take as u32);
                changed |= take > 0;
            }
            need_ack = true;
        }
        if seg.flags & TCP_FIN != 0 {
            let fin_seq = seg.seq.wrapping_add(seg.payload.len() as u32);
            if fin_seq == conn.rcv_nxt && !conn.peer_closed {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                conn.peer_closed = true;
                conn.state = match conn.state {
                    State::Established => State::CloseWait,
                    State::FinWait1 | State::FinWait2 => State::Closed,
                    other => other,
                };
                changed = true;
            }
            need_ack = true;
        }
        if need_ack {
            self.reply = Some(Self::ack_of(conn));
        }
        changed.then_some(index)
    }

    /// The immediate reply produced by the last `receive`, if any.
    pub fn take_reply(&mut self) -> Option<Outgoing> {
        self.reply.take()
    }

    /// The peer of the immediate reply, if one is pending.
    pub fn reply_peer(&self) -> Option<Ipv4> {
        self.reply.as_ref().map(|reply| reply.peer)
    }

    /// Peers of every connection with a segment due, into `out`. Returns how
    /// many were written.
    ///
    /// The caller resolves them itself and may be able to serve a later one
    /// when the first is unreachable, which is what stops one unresolvable
    /// peer holding up every other connection's output.
    pub fn peers_with_work(&self, out: &mut [Ipv4]) -> usize {
        let now = self.now;
        let mut count = 0;
        for conn in self.conns.iter() {
            if count == out.len() {
                break;
            }
            if Self::conn_has_work(conn, now) {
                out[count] = conn.peer;
                count += 1;
            }
        }
        count
    }

    /// As `poll`, but ignoring connections to any peer in `skip`.
    ///
    /// `poll` returns the first connection with work, so a peer whose
    /// hardware address is unknown used to hold up every other connection:
    /// nothing was polled at all, no retransmission timer advanced, and
    /// data, ACKs and FINs for every other port waited behind it -- for the
    /// 120-second idle timeout, if the stuck peer was an established
    /// connection whose ARP entry had been evicted.
    pub fn poll_skipping(&mut self, skip: &[Ipv4]) -> Option<Outgoing> {
        self.skip_peers = [None; MAX_CONNECTIONS];
        for (slot, peer) in self.skip_peers.iter_mut().zip(skip.iter()) {
            *slot = Some(*peer);
        }
        let out = self.poll();
        self.skip_peers = [None; MAX_CONNECTIONS];
        out
    }

    /// Who the next segment would go to, without producing it.
    ///
    /// The caller needs this to resolve the next hop *before* consuming
    /// anything: `take_reply` removes the reply and `poll` counts a
    /// retransmission, so discovering afterwards that the hardware address
    /// is unknown threw the segment away with nothing to resend it.
    pub fn next_peer(&self) -> Option<Ipv4> {
        if let Some(reply) = self.reply.as_ref() {
            return Some(reply.peer);
        }
        let now = self.now;
        self.conns
            .iter()
            .find(|conn| Self::conn_has_work(conn, now))
            .map(|conn| conn.peer)
    }

    /// Read one line (through its newline) if a complete line is buffered,
    /// or the whole buffer when it is full without a newline.
    pub fn read_line(&mut self, index: usize, out: &mut [u8]) -> Option<usize> {
        let conn = &self.conns[index];
        let end = match conn.rx[..conn.rx_len].iter().position(|&b| b == b'\n') {
            Some(pos) => pos + 1,
            None if conn.rx_len == BUFFER_BYTES => conn.rx_len,
            None => return None,
        };
        let n = end.min(out.len());
        Some(self.read(index, &mut out[..n]))
    }

    /// Read up to `out.len()` bytes of received data.
    pub fn read(&mut self, index: usize, out: &mut [u8]) -> usize {
        let conn = &mut self.conns[index];
        let before = conn.window();
        let n = conn.rx_len.min(out.len());
        out[..n].copy_from_slice(&conn.rx[..n]);
        conn.rx.copy_within(n..conn.rx_len, 0);
        conn.rx_len -= n;
        // The window was too small for a full segment and now is not: say
        // so. Nothing else would -- an ACK only goes out when data arrives,
        // and a peer facing a closed window sends none.
        if (before as usize) < MSS as usize && conn.window() as usize >= MSS as usize {
            conn.window_update = true;
        }
        n
    }

    /// Queue bytes for sending; returns how many fitted.
    pub fn write(&mut self, index: usize, data: &[u8]) -> usize {
        let conn = &mut self.conns[index];
        if !matches!(conn.state, State::Established | State::CloseWait) || conn.fin_pending {
            return 0;
        }
        let n = data.len().min(BUFFER_BYTES - conn.tx_len);
        conn.tx[conn.tx_len..conn.tx_len + n].copy_from_slice(&data[..n]);
        conn.tx_len += n;
        n
    }

    /// Close our side once all queued data has been sent.
    pub fn close(&mut self, index: usize) {
        let conn = &mut self.conns[index];
        if matches!(conn.state, State::Established | State::CloseWait) {
            conn.fin_pending = true;
        } else if matches!(conn.state, State::SynReceived | State::SynSent) {
            conn.state = State::Closed;
        }
    }

    /// Drop a connection immediately (no FIN; a later peer segment gets RST).
    pub fn abort(&mut self, index: usize) {
        self.conns[index].state = State::Closed;
    }

    /// Next segment to transmit for data, FIN, or retransmission; `None`
    /// when nothing is due. Call repeatedly until it returns `None`.
    pub fn poll(&mut self) -> Option<Outgoing> {
        let skip = self.skip_peers;
        for conn in self.conns.iter_mut() {
            if conn.state == State::Closed {
                continue;
            }
            if skip.contains(&Some(conn.peer)) {
                continue;
            }
            // Our own SYN, the first time (NET-030).
            if conn.syn_unsent {
                conn.syn_unsent = false;
                conn.last_send_tick = self.now;
                self.segments_out += 1;
                return Some(Self::syn_of(conn));
            }
            // An unacknowledged SYN|ACK is in flight too: without this a lost
            // handshake reply is never resent and the slot is held forever.
            let syn_pending = conn.state == State::SynReceived;
            let syn_sent = conn.state == State::SynSent;
            let in_flight = syn_pending
                || syn_sent
                || conn.tx_unacked > 0
                || (conn.fin_sent && conn.snd_una != conn.snd_nxt);
            // Retransmit what is in flight after the timeout.
            if in_flight && self.now.saturating_sub(conn.last_send_tick) >= RTO_TICKS {
                if conn.retries >= MAX_RETRIES {
                    conn.state = State::Closed;
                    self.reaped += 1;
                    continue;
                }
                conn.retries += 1;
                conn.last_send_tick = self.now;
                self.retransmits += 1;
                self.segments_out += 1;
                if syn_sent {
                    return Some(Self::syn_of(conn));
                }
                if syn_pending {
                    return Some(Outgoing {
                        peer: conn.peer,
                        peer_port: conn.peer_port,
                        local_port: conn.local_port,
                        seq: conn.snd_una,
                        ack: conn.rcv_nxt,
                        flags: TCP_SYN | TCP_ACK,
                        window: conn.window(),
                        payload: (0, 0),
                        mss: Some(MSS),
                    });
                }
                let fin = conn.fin_sent && conn.tx_unacked == conn.tx_len;
                return Some(Outgoing {
                    peer: conn.peer,
                    peer_port: conn.peer_port,
                    local_port: conn.local_port,
                    seq: conn.snd_una,
                    ack: conn.rcv_nxt,
                    flags: TCP_ACK | if fin { TCP_FIN } else { TCP_PSH },
                    window: conn.window(),
                    payload: (0, conn.tx_unacked),
                    mss: None,
                });
            }
            if in_flight {
                continue;
            }
            // New data, one segment at a time, within the peer's window.
            let unsent = conn.tx_len - conn.tx_unacked;
            if unsent > 0 && matches!(conn.state, State::Established | State::CloseWait) {
                let len = unsent.min(MSS as usize).min(conn.snd_wnd as usize);
                if len == 0 {
                    continue;
                }
                let seq = conn.snd_nxt;
                conn.snd_nxt = conn.snd_nxt.wrapping_add(len as u32);
                conn.tx_unacked = len;
                conn.last_send_tick = self.now;
                conn.retries = 0;
                self.segments_out += 1;
                return Some(Outgoing {
                    peer: conn.peer,
                    peer_port: conn.peer_port,
                    local_port: conn.local_port,
                    seq,
                    ack: conn.rcv_nxt,
                    flags: TCP_ACK | TCP_PSH,
                    window: conn.window(),
                    payload: (0, len),
                    mss: None,
                });
            }
            // The window reopened after a read (see `read`).
            if conn.window_update {
                conn.window_update = false;
                self.segments_out += 1;
                return Some(Self::ack_of(conn));
            }
            // FIN once everything is acknowledged.
            if conn.fin_pending && !conn.fin_sent && unsent == 0 {
                conn.fin_sent = true;
                conn.state = match conn.state {
                    State::CloseWait => State::LastAck,
                    _ => State::FinWait1,
                };
                let seq = conn.snd_nxt;
                conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
                conn.last_send_tick = self.now;
                conn.retries = 0;
                self.segments_out += 1;
                return Some(Outgoing {
                    peer: conn.peer,
                    peer_port: conn.peer_port,
                    local_port: conn.local_port,
                    seq,
                    ack: conn.rcv_nxt,
                    flags: TCP_ACK | TCP_FIN,
                    window: conn.window(),
                    payload: (0, 0),
                    mss: None,
                });
            }
        }
        None
    }

    /// Whether this connection has a segment due at `now`.
    ///
    /// Mirrors the decisions in `poll` without taking any of them, so a
    /// caller can learn where the next segment is going before committing to
    /// producing it.
    fn conn_has_work(conn: &Connection, now: u64) -> bool {
        if conn.state == State::Closed {
            return false;
        }
        if conn.syn_unsent {
            return true;
        }
        let syn_pending = matches!(conn.state, State::SynReceived | State::SynSent);
        let in_flight =
            syn_pending || conn.tx_unacked > 0 || (conn.fin_sent && conn.snd_una != conn.snd_nxt);
        if in_flight {
            return now.saturating_sub(conn.last_send_tick) >= RTO_TICKS;
        }
        let unsent = conn.tx_len - conn.tx_unacked;
        if unsent > 0 && matches!(conn.state, State::Established | State::CloseWait) {
            return unsent.min(MSS as usize).min(conn.snd_wnd as usize) > 0;
        }
        conn.window_update || (conn.fin_pending && !conn.fin_sent && unsent == 0)
    }

    /// Payload bytes for an `Outgoing` produced by `poll`.
    pub fn payload(&self, index: usize, payload: (usize, usize)) -> &[u8] {
        &self.conns[index].tx[payload.0..payload.0 + payload.1]
    }

    /// Index of the connection an `Outgoing` belongs to.
    pub fn index_of(&self, out: &Outgoing) -> Option<usize> {
        self.find(out.peer, out.peer_port, out.local_port)
    }
}

impl Default for Tcp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: Ipv4 = [10, 0, 2, 2];
    const PORT: u16 = 7779;

    fn seg(seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Segment<'_> {
        Segment {
            src: PEER,
            src_port: 40000,
            dst_port: PORT,
            seq,
            ack,
            flags,
            window: 65535,
            payload,
        }
    }

    /// Like `seg`, but from a chosen source port, so successive clients are
    /// distinct connections rather than the same one reopening.
    fn seg_from(src_port: u16, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Segment<'_> {
        Segment {
            src: PEER,
            src_port,
            dst_port: PORT,
            seq,
            ack,
            flags,
            window: 65535,
            payload,
        }
    }

    fn handshake_from(tcp: &mut Tcp, src_port: u16) -> (usize, u32) {
        tcp.listen(PORT);
        let index = tcp
            .receive(seg_from(src_port, 1000, 0, TCP_SYN, &[]))
            .expect("the table must have room for this client");
        let synack = tcp.take_reply().unwrap();
        let iss = synack.seq;
        assert_eq!(
            tcp.receive(seg_from(src_port, 1001, iss + 1, TCP_ACK, &[])),
            Some(index)
        );
        (index, iss + 1)
    }

    fn handshake(tcp: &mut Tcp) -> (usize, u32) {
        tcp.listen(PORT);
        let index = tcp.receive(seg(1000, 0, TCP_SYN, &[])).unwrap();
        let synack = tcp.take_reply().unwrap();
        assert_eq!(synack.flags, TCP_SYN | TCP_ACK);
        assert_eq!(synack.ack, 1001);
        assert_eq!(synack.mss, Some(MSS));
        let iss = synack.seq;
        assert_eq!(tcp.receive(seg(1001, iss + 1, TCP_ACK, &[])), Some(index));
        assert_eq!(tcp.connection(index).unwrap().state, State::Established);
        (index, iss + 1)
    }

    #[test]
    fn passive_open_data_echo_and_passive_close() {
        let mut tcp = Tcp::new();
        let (index, snd) = handshake(&mut tcp);
        assert_eq!(tcp.accepted, 1);

        // Peer sends a line; we ACK and can read it.
        assert_eq!(
            tcp.receive(seg(1001, snd, TCP_ACK | TCP_PSH, b"hi\n")),
            Some(index)
        );
        let ack = tcp.take_reply().unwrap();
        assert_eq!(ack.flags, TCP_ACK);
        assert_eq!(ack.ack, 1004);
        let mut buf = [0u8; 16];
        assert_eq!(tcp.read(index, &mut buf), 3);
        assert_eq!(&buf[..3], b"hi\n");
        assert_eq!(tcp.read(index, &mut buf), 0);

        // We reply; one segment goes out and is acknowledged.
        assert_eq!(tcp.write(index, b"HI\n"), 3);
        let out = tcp.poll().unwrap();
        assert_eq!(out.seq, snd);
        assert_eq!(out.flags, TCP_ACK | TCP_PSH);
        assert_eq!(tcp.payload(index, out.payload), b"HI\n");
        assert!(tcp.poll().is_none(), "one segment in flight at a time");
        assert_eq!(tcp.receive(seg(1004, snd + 3, TCP_ACK, &[])), None);
        assert!(tcp.connection(index).unwrap().send_buffer().is_empty());

        // Peer closes; we ACK, see peer_closed, close, send FIN, get ACK.
        assert_eq!(
            tcp.receive(seg(1004, snd + 3, TCP_ACK | TCP_FIN, &[])),
            Some(index)
        );
        assert_eq!(tcp.take_reply().unwrap().ack, 1005);
        assert_eq!(tcp.connection(index).unwrap().state, State::CloseWait);
        assert!(tcp.connection(index).unwrap().peer_closed());
        tcp.close(index);
        let fin = tcp.poll().unwrap();
        assert_eq!(fin.flags, TCP_ACK | TCP_FIN);
        assert_eq!(fin.seq, snd + 3);
        assert_eq!(tcp.connection(index).unwrap().state, State::LastAck);
        assert_eq!(tcp.receive(seg(1005, snd + 4, TCP_ACK, &[])), Some(index));
        assert!(
            tcp.connection(index).is_none(),
            "closed connections are freed"
        );
    }

    #[test]
    fn active_close_from_our_side() {
        let mut tcp = Tcp::new();
        let (index, snd) = handshake(&mut tcp);
        tcp.close(index);
        let fin = tcp.poll().unwrap();
        assert_eq!(fin.flags, TCP_ACK | TCP_FIN);
        assert_eq!(tcp.connection(index).unwrap().state, State::FinWait1);
        assert_eq!(tcp.receive(seg(1001, snd + 1, TCP_ACK, &[])), Some(index));
        assert_eq!(tcp.connection(index).unwrap().state, State::FinWait2);
        assert_eq!(
            tcp.receive(seg(1001, snd + 1, TCP_ACK | TCP_FIN, &[])),
            Some(index)
        );
        assert_eq!(tcp.take_reply().unwrap().ack, 1002);
        assert!(tcp.connection(index).is_none());
    }

    #[test]
    fn out_of_order_is_dropped_and_reacked() {
        let mut tcp = Tcp::new();
        let (index, snd) = handshake(&mut tcp);
        assert_eq!(
            tcp.receive(seg(1005, snd, TCP_ACK | TCP_PSH, b"late")),
            None
        );
        let ack = tcp.take_reply().unwrap();
        assert_eq!(ack.ack, 1001, "duplicate ACK for the expected sequence");
        assert_eq!(tcp.connection(index).unwrap().readable(), 0);
        // A retransmission of the missing piece is accepted.
        assert_eq!(
            tcp.receive(seg(1001, snd, TCP_ACK | TCP_PSH, b"abcd")),
            Some(index)
        );
        assert_eq!(tcp.take_reply().unwrap().ack, 1005);
    }

    #[test]
    fn retransmits_after_timeout_and_gives_up() {
        let mut tcp = Tcp::new();
        let (index, snd) = handshake(&mut tcp);
        tcp.write(index, b"data");
        let first = tcp.poll().unwrap();
        assert_eq!(first.seq, snd);
        tcp.set_now(RTO_TICKS - 1);
        assert!(tcp.poll().is_none());
        for retry in 1..=MAX_RETRIES as u64 {
            tcp.set_now(RTO_TICKS * (retry + 1));
            let again = tcp.poll().unwrap();
            assert_eq!(
                again.seq, snd,
                "retransmission {retry} resends from snd_una"
            );
            assert_eq!(tcp.payload(index, again.payload), b"data");
        }
        assert_eq!(tcp.retransmits, MAX_RETRIES as u64);
        tcp.set_now(RTO_TICKS * (MAX_RETRIES as u64 + 3));
        assert!(tcp.poll().is_none());
        assert!(
            tcp.connection(index).is_none(),
            "abandoned after MAX_RETRIES"
        );
    }

    #[test]
    fn peer_window_limits_sends_and_reset_closes() {
        let mut tcp = Tcp::new();
        let (index, snd) = handshake(&mut tcp);
        // Zero window: nothing may be sent.
        let mut zero = seg(1001, snd, TCP_ACK, &[]);
        zero.window = 0;
        tcp.receive(zero);
        tcp.write(index, b"blocked");
        assert!(tcp.poll().is_none());
        // Window opens to 3 bytes: a 3-byte segment goes out.
        let mut small = seg(1001, snd, TCP_ACK, &[]);
        small.window = 3;
        tcp.receive(small);
        let out = tcp.poll().unwrap();
        assert_eq!(out.payload.1, 3);
        // RST closes it.
        assert_eq!(tcp.receive(seg(1001, snd, TCP_RST, &[])), Some(index));
        assert!(tcp.connection(index).is_none());
    }

    #[test]
    fn unknown_segments_get_reset_and_table_is_bounded() {
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        // Data to a port nobody listens on: RST|ACK.
        let mut stray = seg(500, 0, TCP_ACK | TCP_PSH, b"x");
        stray.dst_port = 9;
        assert_eq!(tcp.receive(stray), None);
        let rst = tcp.take_reply().unwrap();
        assert_eq!(rst.flags, TCP_RST);
        assert_eq!(rst.seq, 0);
        assert_eq!(tcp.resets_sent, 1);
        // Fill this port's share; the next SYN is refused with a reset
        // rather than silently ignored, so the client fails fast.
        for i in 0..MAX_PER_PORT as u16 {
            let mut syn = seg(1, 0, TCP_SYN, &[]);
            syn.src_port = 50000 + i;
            assert!(tcp.receive(syn).is_some(), "SYN {i} should be accepted");
            tcp.take_reply();
        }
        let mut syn = seg(1, 0, TCP_SYN, &[]);
        syn.src_port = 60000;
        assert_eq!(tcp.receive(syn), None);
        let refusal = tcp
            .take_reply()
            .expect("a full port must refuse, not black-hole");
        assert_ne!(refusal.flags & TCP_RST, 0);
        assert_eq!(tcp.refused, 1);
        assert_eq!(tcp.connections().count(), MAX_PER_PORT);
    }

    #[test]
    fn a_saturated_port_cannot_starve_the_other() {
        // The finding this guards: four idle connections to the
        // unauthenticated echo port took the signed command port offline
        // until reboot, because both shared one unreserved table.
        const ECHO: u16 = 7779;
        const COMMAND: u16 = 7780;
        let mut tcp = Tcp::new();
        tcp.listen(ECHO);
        tcp.listen(COMMAND);

        for i in 0..MAX_PER_PORT as u16 + 3 {
            let mut syn = seg(1, 0, TCP_SYN, &[]);
            syn.src_port = 40000 + i;
            syn.dst_port = ECHO;
            tcp.receive(syn);
            tcp.take_reply();
        }
        let on_echo = tcp
            .connections()
            .filter(|(_, c)| c.local_port == ECHO)
            .count();
        assert_eq!(
            on_echo, MAX_PER_PORT,
            "the echo port is capped at its share"
        );

        let mut syn = seg(1, 0, TCP_SYN, &[]);
        syn.src_port = 55555;
        syn.dst_port = COMMAND;
        let index = tcp
            .receive(syn)
            .expect("the command port must still accept while echo is saturated");
        let reply = tcp.take_reply().expect("SYN|ACK");
        assert_eq!(reply.flags, TCP_SYN | TCP_ACK);
        assert_eq!(tcp.connection(index).unwrap().local_port, COMMAND);
    }

    #[test]
    fn half_open_connections_are_retransmitted_then_abandoned() {
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        let index = tcp.receive(seg(1000, 0, TCP_SYN, &[])).unwrap();
        assert_eq!(tcp.take_reply().unwrap().flags, TCP_SYN | TCP_ACK);
        // The client never answers. The SYN|ACK must be resent, not dropped.
        for attempt in 1..=MAX_RETRIES as u64 {
            tcp.set_now(RTO_TICKS * attempt);
            let again = tcp.poll().expect("SYN|ACK should be retransmitted");
            assert_eq!(again.flags, TCP_SYN | TCP_ACK, "retransmit {attempt}");
        }
        // Then the slot comes back rather than being held forever.
        tcp.set_now(RTO_TICKS * (MAX_RETRIES as u64 + 2));
        assert!(tcp.poll().is_none());
        assert!(
            tcp.connection(index).is_none(),
            "an unanswered handshake must not hold a slot"
        );
        assert!(tcp.reaped >= 1);
    }

    #[test]
    fn a_peer_that_hung_up_does_not_hold_a_slot_for_the_idle_timeout() {
        // CloseWait used to share the 120-second idle timeout with a live
        // connection, even though the peer has already left and nothing more
        // can ever arrive on it. With eight slots for every port, six
        // ordinary HTTP clients that each made one request and hung up --
        // which is what every HTTP client does -- took the whole machine off
        // the network for two minutes.
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        let (index, snd) = handshake(&mut tcp);

        // The peer sends FIN and never speaks again. We ACK it and, like a
        // service that forgets to close its half, do nothing further.
        assert_eq!(
            tcp.receive(seg(1001, snd, TCP_ACK | TCP_FIN, &[])),
            Some(index)
        );
        tcp.take_reply();
        assert_eq!(tcp.connection(index).unwrap().state, State::CloseWait);

        tcp.set_now(CLOSING_TIMEOUT_TICKS - 1);
        assert!(
            tcp.connection(index).is_some(),
            "the slot is released on a timer, not immediately"
        );
        tcp.set_now(CLOSING_TIMEOUT_TICKS);
        assert!(
            tcp.connection(index).is_none(),
            "a peer that hung up must not hold a slot for the full idle timeout"
        );
        const {
            assert!(
                CLOSING_TIMEOUT_TICKS < IDLE_TIMEOUT_TICKS,
                "the point of this test is that the two differ"
            );
        }
    }

    #[test]
    fn six_clients_that_hang_up_in_turn_do_not_exhaust_the_port() {
        // The end-to-end shape: sequential, entirely well-behaved clients.
        // None of them overlaps another, so one slot would be enough if the
        // table let go of peers that had left.
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        for round in 0..MAX_PER_PORT + 4 {
            let now = round as u64 * CLOSING_TIMEOUT_TICKS;
            tcp.set_now(now);
            let port = 2000 + round as u16;
            let (index, snd) = handshake_from(&mut tcp, port);
            assert!(
                tcp.connection(index).is_some(),
                "client {round} could not be accepted; the table is still \
                 holding slots for clients that already hung up"
            );
            tcp.receive(seg_from(port, 1001, snd, TCP_ACK | TCP_FIN, &[]));
            tcp.take_reply();
        }
    }

    #[test]
    fn idle_connections_are_reaped_and_the_slot_returns() {
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        let (index, _snd) = handshake(&mut tcp);
        assert_eq!(tcp.connection(index).unwrap().state, State::Established);

        // Just short of the limit it survives.
        tcp.set_now(IDLE_TIMEOUT_TICKS - 1);
        assert!(tcp.connection(index).is_some(), "not yet idle enough");

        tcp.set_now(IDLE_TIMEOUT_TICKS);
        assert!(tcp.connection(index).is_none(), "an idle peer is reaped");
        assert_eq!(tcp.reaped, 1);

        // And traffic keeps a connection alive across the same span.
        let (index, snd) = handshake(&mut tcp);
        for step in 1..=4u64 {
            tcp.set_now(step * IDLE_TIMEOUT_TICKS / 2);
            tcp.receive(seg(1001, snd, TCP_ACK | TCP_PSH, b"x"));
            tcp.take_reply();
            let mut sink = [0u8; 8];
            tcp.read(index, &mut sink);
        }
        assert!(
            tcp.connection(index).is_some(),
            "a talking peer must never be reaped"
        );
    }

    #[test]
    fn read_line_waits_for_newline_and_two_ports_listen() {
        let mut tcp = Tcp::new();
        for (index, port) in (7779..7779 + MAX_LISTEN_PORTS as u16).enumerate() {
            assert!(tcp.listen(port), "slot {index} should be free");
            assert!(tcp.is_listening(port));
        }
        let overflow = 7779 + MAX_LISTEN_PORTS as u16;
        assert!(!tcp.listen(overflow), "listen must report a full table");
        assert!(!tcp.is_listening(overflow));
        let (index, snd) = handshake(&mut tcp);
        tcp.receive(seg(1001, snd, TCP_ACK | TCP_PSH, b"par"));
        let mut buf = [0u8; 32];
        assert_eq!(tcp.read_line(index, &mut buf), None);
        tcp.receive(seg(1004, snd, TCP_ACK | TCP_PSH, b"tial\nnext"));
        assert_eq!(tcp.read_line(index, &mut buf), Some(8));
        assert_eq!(&buf[..8], b"partial\n");
        assert_eq!(tcp.read_line(index, &mut buf), None);
        assert_eq!(tcp.connection(index).unwrap().readable(), 4);
    }

    #[test]
    fn peek_shows_buffered_bytes_without_consuming_them() {
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        let (index, snd) = handshake(&mut tcp);
        tcp.receive(seg(1001, snd, TCP_ACK | TCP_PSH, b"GET / HTTP/1.1\r\n"));
        let conn = tcp.connection(index).unwrap();
        assert_eq!(conn.peek(), b"GET / HTTP/1.1\r\n");
        assert_eq!(conn.readable(), 16);
        // Peeking twice is stable, and a later read still sees everything.
        assert_eq!(tcp.connection(index).unwrap().peek().len(), 16);
        let mut out = [0u8; 32];
        assert_eq!(tcp.read(index, &mut out), 16);
        assert_eq!(&out[..16], b"GET / HTTP/1.1\r\n");
        assert!(tcp.connection(index).unwrap().peek().is_empty());
    }

    #[test]
    fn writable_tracks_the_send_buffer() {
        let mut tcp = Tcp::new();
        tcp.listen(PORT);
        let (index, _snd) = handshake(&mut tcp);
        assert_eq!(tcp.connection(index).unwrap().writable(), BUFFER_BYTES);
        let wrote = tcp.write(index, &[0u8; 100]);
        assert_eq!(wrote, 100);
        assert_eq!(
            tcp.connection(index).unwrap().writable(),
            BUFFER_BYTES - 100
        );
        // A write larger than the room left is accepted only in part, and
        // `writable` is what says how much will fit.
        let room = tcp.connection(index).unwrap().writable();
        assert_eq!(tcp.write(index, &[1u8; BUFFER_BYTES]), room);
        assert_eq!(tcp.connection(index).unwrap().writable(), 0);
    }

    // ---- active open (NET-030) ----

    const SERVER: Ipv4 = [10, 0, 2, 2];

    fn from_server(local_port: u16, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Segment<'_> {
        Segment {
            src: SERVER,
            src_port: 80,
            dst_port: local_port,
            seq,
            ack,
            flags,
            window: 4096,
            payload,
        }
    }

    /// Connect, and answer the SYN: the connection index, its port and our ISS.
    fn connected(tcp: &mut Tcp) -> (usize, u16, u32) {
        let conn = tcp.connect(SERVER, 80).expect("room to connect");
        let syn = tcp.poll().expect("the SYN goes out");
        assert_eq!(syn.flags, TCP_SYN);
        assert_eq!(syn.mss, Some(MSS));
        let port = syn.local_port;
        assert!(port >= EPHEMERAL_FIRST);
        assert_eq!(
            tcp.receive(from_server(
                port,
                9000,
                syn.seq.wrapping_add(1),
                TCP_SYN | TCP_ACK,
                &[]
            )),
            Some(conn)
        );
        assert!(tcp.connection(conn).unwrap().is_established());
        let ack = tcp.take_reply().expect("the handshake's last ACK");
        assert_eq!((ack.flags, ack.ack), (TCP_ACK, 9001));
        (conn, port, syn.seq)
    }

    #[test]
    fn connect_handshakes_sends_receives_and_closes() {
        let mut tcp = Tcp::new();
        let (conn, port, iss) = connected(&mut tcp);
        assert_eq!(tcp.write(conn, b"GET / HTTP/1.1\r\n\r\n"), 18);
        let data = tcp.poll().expect("the request");
        assert_eq!(data.seq, iss.wrapping_add(1));
        assert_eq!(tcp.payload(conn, data.payload), b"GET / HTTP/1.1\r\n\r\n");
        // The answer, and the server hangs up.
        tcp.receive(from_server(
            port,
            9001,
            iss.wrapping_add(19),
            TCP_ACK | TCP_PSH,
            b"HTTP/1.1 200 OK\r\n",
        ));
        tcp.receive(from_server(
            port,
            9018,
            iss.wrapping_add(19),
            TCP_ACK | TCP_FIN,
            &[],
        ));
        let mut buf = [0u8; 64];
        let n = tcp.read(conn, &mut buf);
        assert_eq!(&buf[..n], b"HTTP/1.1 200 OK\r\n");
        assert!(tcp.connection(conn).unwrap().peer_closed());
        tcp.close(conn);
        let _ = tcp.take_reply();
        let fin = tcp.poll().expect("our FIN");
        assert!(fin.flags & TCP_FIN != 0);
        tcp.receive(from_server(
            port,
            9019,
            fin.seq.wrapping_add(1),
            TCP_ACK,
            &[],
        ));
        assert!(tcp.connection(conn).is_none(), "closed");
    }

    #[test]
    fn a_refused_connect_closes_and_a_stray_reset_does_not() {
        let mut tcp = Tcp::new();
        let conn = tcp.connect(SERVER, 80).unwrap();
        let syn = tcp.poll().unwrap();
        // A reset that does not answer our SYN is a guess: ignored.
        assert_eq!(
            tcp.receive(from_server(
                syn.local_port,
                0,
                12345,
                TCP_RST | TCP_ACK,
                &[]
            )),
            None
        );
        assert!(tcp.connection(conn).is_some());
        // The real refusal.
        tcp.receive(from_server(
            syn.local_port,
            0,
            syn.seq.wrapping_add(1),
            TCP_RST | TCP_ACK,
            &[],
        ));
        assert!(tcp.connection(conn).is_none());
    }

    #[test]
    fn a_lost_syn_is_resent_and_given_up_on() {
        let mut tcp = Tcp::new();
        let conn = tcp.connect(SERVER, 80).unwrap();
        tcp.poll().unwrap();
        assert_eq!(tcp.poll(), None, "not yet");
        for round in 1..=MAX_RETRIES as u64 {
            tcp.set_now(round * RTO_TICKS);
            let again = tcp.poll().expect("the SYN again");
            assert_eq!(again.flags, TCP_SYN);
        }
        tcp.set_now((MAX_RETRIES as u64 + 1) * RTO_TICKS);
        assert_eq!(tcp.poll(), None);
        assert!(tcp.connection(conn).is_none(), "given up");
    }

    #[test]
    fn outbound_connections_are_bounded_and_get_their_own_ports() {
        let mut tcp = Tcp::new();
        let a = tcp.connect(SERVER, 80).unwrap();
        let b = tcp.connect(SERVER, 80).unwrap();
        assert_ne!(
            tcp.connection(a).unwrap().local_port,
            tcp.connection(b).unwrap().local_port
        );
        assert_eq!(tcp.connect(SERVER, 80), None, "MAX_OUTBOUND");
        // The servers keep their room.
        tcp.listen(PORT);
        let mut accepted = 0;
        for i in 0..MAX_PER_PORT as u16 {
            if tcp
                .receive(seg_from(40000 + i, 1, 0, TCP_SYN, &[]))
                .is_some()
            {
                accepted += 1;
            }
        }
        assert_eq!(accepted, MAX_PER_PORT);
    }

    #[test]
    fn reading_a_full_buffer_tells_the_peer_the_window_is_open() {
        let mut tcp = Tcp::new();
        let (conn, port, iss) = connected(&mut tcp);
        let chunk = [b'x'; 1024];
        let mut seq = 9001u32;
        for _ in 0..BUFFER_BYTES / 1024 {
            tcp.receive(from_server(port, seq, iss.wrapping_add(1), TCP_ACK, &chunk));
            seq = seq.wrapping_add(1024);
        }
        let full = tcp.take_reply().unwrap();
        assert_eq!(full.window, 0, "the buffer is full");
        assert_eq!(tcp.poll(), None);
        let mut out = [0u8; BUFFER_BYTES];
        assert_eq!(tcp.read(conn, &mut out), BUFFER_BYTES);
        let update = tcp.poll().expect("a window update");
        assert_eq!(update.flags, TCP_ACK);
        assert_eq!(update.window as usize, BUFFER_BYTES);
        assert_eq!(update.ack, seq);
        assert_eq!(tcp.poll(), None, "once");
    }
}
