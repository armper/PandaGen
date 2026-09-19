# Phase 279: TCP Connection Lifecycle — Reservation, Reaping, Refusal

## The finding

`gauntlet/tcp_slots.py`: four clients open connections to the
**unauthenticated** echo port and simply hold them. The signed command port —
the machine's control plane — then stops answering. Permanently, until reboot.

Both the TCP critic and the packet-parser critic found this independently, and
it reproduced on the first run:

```
FAIL: command port accepted a connection but could not serve it: timeout
```

Three separate defects combined:

1. Both listen ports shared one four-slot table with no reservation, so the
   data plane could consume every slot the control plane needed.
2. Nothing ever reaped a connection. `poll` only arms a timer when something
   is in flight, and an established-but-silent connection never is. A peer
   that connects and says nothing holds its slot forever.
3. A full table black-holed new connections: `accept` returned `None` and
   `receive` produced no reply at all, so the client hung until its own
   connect timeout rather than failing fast.

Plus a fourth, found by inspection while fixing: a connection in
`SynReceived` was not "in flight" either, so a lost SYN-ACK was never
retransmitted, and that slot leaked too.

## The fix

- The table grows to eight, and `MAX_PER_PORT` caps any single listen port at
  six. Saturating one port always leaves room for the other.
- Every connection carries `last_activity`, refreshed on each segment either
  way. `set_now` reaps: 8 s for a half-open handshake (a backstop; the
  retransmit budget normally abandons it first), 120 s idle, 10 s for a
  connection stuck in a closing state.
- A refused connection now gets a reset instead of silence.
- `SynReceived` counts as in flight, so the SYN-ACK is retransmitted on the
  RTO and the slot is released after `MAX_RETRIES`.
- `net status` reports `refused=` and `reaped=`.

## Verification

Four new tier-1 tests, where the test is the peer and controls the clock:

- `a_saturated_port_cannot_starve_the_other` — the exact finding, in library form.
- `half_open_connections_are_retransmitted_then_abandoned` — the SYN-ACK is
  resent `MAX_RETRIES` times, then the slot comes back.
- `idle_connections_are_reaped_and_the_slot_returns` — and a connection that
  keeps talking is never reaped, across four times the idle span.
- `unknown_segments_get_reset_and_table_is_bounded`, rewritten: a full port
  refuses with a reset rather than black-holing.

`gauntlet:tcp_slots` fails before, passes after. `cargo test --workspace`
green. No regression in `cmd_pipeline`, `cmd_halfclose`, `tcp_concurrent`,
`tcp_bulk_echo`.

## Honest limits

The idle timeout is a policy, not a protocol feature: a legitimate peer that
stays silent for two minutes is disconnected, where a real server would use
keepalives. With eight slots that is the right trade, but it is a trade.
