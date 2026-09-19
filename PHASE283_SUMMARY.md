# Phase 283: HTTP, So That curl Can Be The Judge

## Why

The gauntlet needs a critic that cannot be argued with. Every test so far was
one I wrote, which means every test encodes what I already believed. `curl` is
unmodified, has never heard of this kernel, and fails loudly on anything it
does not consider valid HTTP: a Content-Length that disagrees with the body, a
missing blank line, a connection that closes mid-response.

## What was built

`net_stack::http` — server-side HTTP/1.1 request parsing and response
headers. No allocation; the caller owns every buffer. Strict about framing,
incurious about headers it does not need. It accepts bare LF as well as CRLF,
distinguishes "the head has not arrived yet" from "this is not HTTP", honours
`Connection:` over the version default, and reports the exact head length so
a pipelined body is not eaten. Eleven tests, including a real captured curl
request and a loop asserting that every truncation of a request parses as
incomplete rather than complete.

`kernel_bootstrap::bare_metal_net` serves it on TCP 8080:

- `/` a live status page: uptime, CPUs online, heap, frames rendered and
  presented, storage backend.
- `/health` a two-byte text response.
- `/bytes/N` up to 1 MiB of a recognisable position-dependent pattern, so a
  client can distinguish truncation from corruption.

A body larger than the 2 KiB TCP send buffer is streamed: the handler writes
headers, records what is left, and `pump_http` tops the buffer up on every
service pass as it drains.

`net_stack::tcp` gains `peek` (inspect the buffer without consuming it, since
HTTP's message boundary is a blank line rather than a newline) and `writable`.

## The bug this found immediately

The first `curl` attempt got `Connection reset by peer`. `Tcp` had exactly two
listen slots and was now being asked for three, so `listen` silently dropped
the third and port 8080 was never bound — every client got a reset from the
"nothing is listening here" path.

`listen` now returns a bool so a caller cannot silently fail to listen, the
table holds four ports, and the test asserts both the capacity and the
refusal. That defect had been latent since Phase 273 and was found within
seconds of pointing a real client at the machine, which is the whole argument
for this phase.

## Verification

`gauntlet:http_curl` drives real `curl` through six checks: the status page
with Content-Length verified against the actual body length, `/health`, a
clean 404, a 200,000-byte streamed body compared byte for byte against the
expected pattern, two requests over one kept-alive connection, and `HEAD`.

`cargo test --workspace` green; `net_stack` now 38 tests.
