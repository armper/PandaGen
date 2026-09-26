# Phase 427: the network stack can ask as well as answer (NET-030, NET-031, NET-032)

## What changed

`net_stack` could only serve: TCP was "passive open only", DNS existed
only as a DHCP field, and HTTP was a server's request parser. This phase
gives it a client's three pieces, all allocation-free and host-tested;
the kernel uses them in the next phase.

### TCP active open (`tcp.rs`, NET-030)

- `Tcp::connect(peer, port)` opens a connection from an ephemeral port
  (49152 and up, never one a listener or another connection to that peer
  uses). The SYN goes out on the next `poll`, is resent every `RTO_TICKS`
  and given up on after `MAX_RETRIES`, as the server side's SYN|ACK is.
- `SynSent` takes only the answer to our SYN: a SYN|ACK acknowledging it
  establishes the connection (and `receive` reports it); a reset
  acknowledging it is a refusal; any other reset is a guess at the port
  and is ignored (RFC 9293 3.10.7.3).
- At most `MAX_OUTBOUND` (2) connections of the machine's own at once, so
  its requests cannot crowd its servers out of the eight-slot table.
- **Window updates.** When the application reads from a buffer that had
  closed the window below a segment, the connection now sends an ACK
  saying it has room. Nothing did before: an ACK went out only when data
  arrived, and a sender facing a zero window sends none. A server never
  met this -- requests are small -- but any download larger than the
  2 KiB buffer would have stalled.

### DNS (`dns.rs`, NET-031)

- `build_query(id, name)`: an A query with recursion desired; names and
  labels checked for length.
- `parse_answer(id, name, reply)`: the reply must carry our id, be a
  response, and not be truncated; NXDOMAIN and other errors are reported
  as such; names are read through compression pointers (with a bound on
  how many, so a pointer loop ends); a CNAME chain is followed within the
  reply, in whatever order the records come; an A record for some other
  name is not taken.

### HTTP client (`http.rs`, NET-032)

- `parse_url`: `http://host[:port][/path]`, or a bare `host/path`;
  `https` is refused (no TLS here), as are credentials in the URL and
  control characters.
- `write_request`: `GET` with `Host`, `User-Agent: PandaGen` and
  `Connection: close`, so a response without a length ends at the close.
- `parse_response`: status, reason, `Content-Length`, whether the body is
  chunked (the last transfer coding), and `Location`.
- `dechunk`: a complete chunked body, extensions ignored.

## Tests

- TCP: handshake, request, response, the server's FIN and ours; a refusal
  closes and a stray reset does not; a lost SYN is resent and given up
  on; outbound connections are bounded and the servers keep their room;
  reading a full buffer sends one window update.
- DNS: the query's bytes; a compressed answer; someone else's reply and
  our own query echoed back are refused; a CNAME followed with the
  address first; NXDOMAIN, SERVFAIL, no address, another name's address,
  a pointer loop and a truncated record.
- HTTP: URLs as typed; the request's head; response heads with length,
  chunking and a redirect; incomplete and malformed heads; chunked bodies
  complete, incomplete, too big and malformed.
- `cargo xtask gauntlet`: exit 0.
