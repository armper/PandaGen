# Phase 433: HTTPS -- the machine's requests over TLS (NET-040)

## What changed

`fetch https://...` in the Terminal, and `https://` addresses and
redirects in the Web card, now work -- over the real internet, with
certificates checked. The Web card reads `https://rust-lang.org/`
(through two redirects) and `https://example.com`.

### The `net_tls` crate

A thin, sans-IO wrapper over rustls's unbuffered client: TCP bytes in,
TCP bytes out, plaintext in and out; no socket, clock or random source of
its own, so the kernel's non-blocking fetch drives it one network pass at
a time and it runs under `cargo test`.

- **Nothing cryptographic is written here.** rustls 0.23 does TLS 1.3 and
  1.2; RustCrypto's pure-Rust primitives (through `rustls-rustcrypto`)
  do the maths; `rustls-webpki` checks certificates against Mozilla's
  roots (`webpki-roots`), with the real-time clock for validity dates (an
  unreadable clock fails every certificate rather than trusting any).
- **Randomness** reaches rustls through `getrandom`'s custom hook, which
  `set_entropy_source` points at the kernel's generator (Phase 432).
- **The test CA**: the one extra root, so the gauntlet can serve HTTPS
  from the host. Its certificate has critical name constraints -- it may
  vouch only for `pandagen.test` names and for `10.0.2.2`, QEMU's address
  for the host -- so it can sign nothing on the internet. Its private key
  was never kept; only the one server certificate it signed and that
  certificate's key are in `net_tls/testdata`.

### In the kernel

- A fetch knows its scheme (`net_stack::http::Url::secure`, port 443 by
  default). Once the connection is up an `https` fetch opens a TLS
  session, which holds the request until the handshake is done; the
  network pass feeds it TCP's bytes, takes its plaintext as the page and
  gives its records back to TCP. A TLS failure (a certificate that does
  not check out, a protocol error) ends the fetch and says why.
- The configuration -- Mozilla's roots, parsed -- is made at the first
  `https` request and kept.
- The kernel's target has no SSE, and RustCrypto's SIMD paths crash
  LLVM's type legalizer when built for it (found by a probe build first);
  `.cargo/config.toml` selects their portable software paths for the
  kernel target. The kernel grew from 5.3 to 6.9 MB, which moved the
  smallest machine it boots on: at 16 MiB of RAM what is left no longer
  holds the 2 MiB heap, and the kernel says so and stops. The gauntlet's
  "small machine" shape is 18 MiB now.

### Found on the way

- **The HTML reader panicked the kernel** on a text run that began with a
  multi-byte character (rust-lang.org's language menu has `日本語`): it
  sliced one byte past the run's start. It now steps a whole character,
  and a new test throws three thousand generated pages -- cut-off tags,
  entities, quotes, comments, wide characters -- at the reader, the
  wrapper and URL resolution, which must not panic.

### The Web card and the harness

- Redirects to `https://` are followed like any other; the "no TLS"
  messages are gone.
- `qemu-script --https-serve <port>` serves the test pages over TLS with
  the test CA's certificate.

## Tests

- `net_tls` (host): a request and its answer through a checked handshake
  with a rustls server; a certificate for another name is refused; the
  test CA cannot vouch for `example.com` (refused for the name
  constraint, asserted); an expired or unknown clock fails the
  certificate; a host that is no name is refused up front.
- `net_stack`: `https` URLs parse with their port and scheme.
- `web`: a chain of redirects ends on the page; https redirects are
  followed; wide characters at a text's start; no generated page panics.
- New gauntlet shape "HTTPS: the Terminal and the Web card, over TLS":
  the Terminal fetches 3.4 KiB over TLS, and the Web card loads a page
  and follows its link over TLS.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo xtask gauntlet`: exit 0.
