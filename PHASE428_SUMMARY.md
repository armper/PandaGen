# Phase 428: resolve and fetch from the Terminal (NET-033)

## What changed

The machine can now reach out. Phase 427 gave `net_stack` a TCP client,
a DNS client and an HTTP client; this phase puts them behind two
Terminal words:

- **`resolve <name> [server[:port]]`** looks a name up through the
  resolver DHCP named (QEMU's 10.0.2.3 when none), or the one given.
- **`fetch http://host[:port]/path`** (or `fetch host/path`) resolves the
  host, connects, sends the request and shows the response: the status,
  the length, a redirect's target, and the first forty lines of a text
  body ("... N more lines" after), or "(not text: N bytes)". `https` is
  refused plainly: there is no TLS here yet. `net fetch` and
  `net resolve` say the same.

### How it runs

A fetch does not block anything. It is a small state machine in the
network stack (`Fetch`: resolving, connecting, receiving) that the main
loop's network pass moves along -- the query out and resent every second
three times, the SYN, the request once the connection is up, the body
read as it comes (which reopens the window, so pages past the 2 KiB
buffer arrive), the end at the close or the declared length. The
Terminal's prompt stays free meanwhile, and the lines are handed to it as
they are ready. Everything is bounded: one request at a time, 10 s all
told, 64 KiB kept (the rest counted), and at most two connections of the
machine's own so its servers keep their room.

- DNS replies are taken only from the resolver asked, and only with the
  query's id; anything else is ignored and the wait goes on.
- A connection the machine opened is never handed to a server: the
  network pass used to give every connection's data to the echo service
  unless it arrived on a known server port.
- The command service's 512-byte answers are not used: a page does not
  fit, and the Terminal is the one asking.

### The harness

`qemu-script --http-serve <port>` runs a web server on the host's
loopback (the guest reaches it as 10.0.2.2) serving a hundred numbered
lines; `--dns-serve <port>` runs a resolver that answers every name with
10.0.2.2. The new gauntlet shape resolves `panda.test` through it and
fetches the page, expecting all 3492 bytes and line 40 -- so the window
updates of Phase 427 are exercised on a real boot, and nothing depends on
the host's own network.

By hand, with the host online: `fetch example.com` resolves through QEMU's
resolver and shows the page (`HTTP 200 OK, 559 bytes`).

## Tests

- `test_fetch_and_resolve_lines_become_requests` (the Terminal's words).
- Gauntlet shape "the Terminal: resolve a name, fetch a page".
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo xtask gauntlet`: exit 0.
