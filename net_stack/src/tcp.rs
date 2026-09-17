//! Minimal TCP for a server that talks to a handful of peers at a time.
//!
//! Passive open only, in-order receive (out-of-order segments are dropped
//! and re-acknowledged), one outstanding send segment with a fixed
//! retransmission timeout, and both close directions. Enough for line
//! protocols from standard tools; not a general-purpose stack.

use crate::wire::{TCP_ACK, TCP_FIN, TCP_PSH, TCP_RST, TCP_SYN};
use crate::Ipv4;

/// Connections tracked at once.
pub const MAX_CONNECTIONS: usize = 4;
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
    retries: u8,
    /// FIN queued by the application (after all data).
    fin_pending: bool,
    fin_sent: bool,
    /// Peer sent a FIN that the application has not seen yet.
    peer_closed: bool,
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
            retries: 0,
            fin_pending: false,
            fin_sent: false,
            peer_closed: false,
        }
    }

    fn window(&self) -> u16 {
        (BUFFER_BYTES - self.rx_len).min(u16::MAX as usize) as u16
    }

    /// Bytes the application may read.
    pub fn readable(&self) -> usize {
        self.rx_len
    }

    pub fn peer_closed(&self) -> bool {
        self.peer_closed
    }

    pub fn send_buffer(&self) -> &[u8] {
        &self.tx[..self.tx_len]
    }
}

pub struct Tcp {
    listen_port: Option<u16>,
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
            listen_port: None,
            conns: [const { Connection::closed() }; MAX_CONNECTIONS],
            next_iss: 0x1000,
            now: 0,
            reply: None,
            segments_in: 0,
            segments_out: 0,
            retransmits: 0,
            resets_sent: 0,
            accepted: 0,
        }
    }

    pub fn listen(&mut self, port: u16) {
        self.listen_port = Some(port);
    }

    pub fn listen_port(&self) -> Option<u16> {
        self.listen_port
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
    }

    /// Feed a received segment. Returns the connection that gained
    /// readable data or changed state, if any; any immediate reply is
    /// available through `take_reply`.
    pub fn receive(&mut self, seg: Segment<'_>) -> Option<usize> {
        self.segments_in += 1;
        match self.find(seg.src, seg.src_port, seg.dst_port) {
            Some(index) => self.receive_on(index, seg),
            None => {
                if seg.flags & TCP_SYN != 0
                    && seg.flags & TCP_ACK == 0
                    && Some(seg.dst_port) == self.listen_port
                {
                    self.accept(seg)
                } else if seg.flags & TCP_RST == 0 {
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
                    None
                } else {
                    None
                }
            }
        }
    }

    fn accept(&mut self, seg: Segment<'_>) -> Option<usize> {
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

    /// Read up to `out.len()` bytes of received data.
    pub fn read(&mut self, index: usize, out: &mut [u8]) -> usize {
        let conn = &mut self.conns[index];
        let n = conn.rx_len.min(out.len());
        out[..n].copy_from_slice(&conn.rx[..n]);
        conn.rx.copy_within(n..conn.rx_len, 0);
        conn.rx_len -= n;
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
        } else if conn.state == State::SynReceived {
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
        for conn in self.conns.iter_mut() {
            if conn.state == State::Closed {
                continue;
            }
            let in_flight = conn.tx_unacked > 0 || (conn.fin_sent && conn.snd_una != conn.snd_nxt);
            // Retransmit what is in flight after the timeout.
            if in_flight && self.now.saturating_sub(conn.last_send_tick) >= RTO_TICKS {
                if conn.retries >= MAX_RETRIES {
                    conn.state = State::Closed;
                    continue;
                }
                conn.retries += 1;
                conn.last_send_tick = self.now;
                self.retransmits += 1;
                self.segments_out += 1;
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
        // Fill the table; the next SYN is ignored.
        for i in 0..MAX_CONNECTIONS as u16 {
            let mut syn = seg(1, 0, TCP_SYN, &[]);
            syn.src_port = 50000 + i;
            assert!(tcp.receive(syn).is_some());
            tcp.take_reply();
        }
        let mut syn = seg(1, 0, TCP_SYN, &[]);
        syn.src_port = 60000;
        assert_eq!(tcp.receive(syn), None);
        assert!(tcp.take_reply().is_none());
        assert_eq!(tcp.connections().count(), MAX_CONNECTIONS);
    }
}
