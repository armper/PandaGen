# Phase 432: random numbers the machine can trust (SEC-030)

## What changed

The kernel had no random number generator. What needed one made do:
passphrase salts hashed the time-stamp counter and a count; DNS query ids
were the clock's low bits; TCP's initial sequence numbers counted up from
0x1000 by 64000 a connection. A guessable DNS id lets anyone on the path
answer a lookup first; a guessable sequence number lets them reset or
inject into a connection. And TLS, next, needs keys.

### The `entropy` crate (host-tested)

- `Pool`: samples folded into a running SHA-256 state, so the pool is never
  worse than its best input; `take` hands out a digest and moves on, so
  no output is given twice.
- `HmacDrbg`: NIST SP 800-90A HMAC_DRBG over SHA-256 (the kernel's own
  `remote_ipc::sha256`), with the standard's reseed counter and request
  bound. It reproduces NIST's CAVP test vector exactly.
- `Rng`: a DRBG seeded from the pool and reseeded from it every 64 fresh
  samples.

### In the kernel (`random.rs`)

- **Seeded at boot**, before the filesystem, DHCP or anything that needs a
  number: RDSEED or RDRAND when CPUID says the CPU has one, and always
  4096 readings of the time-stamp counter around work whose length
  depends on the last reading. The boot log says which
  (`entropy: seeded from ...`; QEMU's default CPU has neither
  instruction, so there it is timing alone).
- **Stirred as the machine runs**: every key, mouse byte and network frame
  puts its moment into the pool, without ever waiting for the generator.
- **Used**: passphrase salts, DNS query ids, and TCP initial sequence
  numbers (`net_stack::tcp::Tcp::set_iss_source`) now come from it.
- `random [bytes]` in the Terminal shows bytes and the generator's
  counts.

## Tests

- `entropy`: the NIST vector; reseeds and seeds change what follows;
  bounded requests; the pool depends on every sample and never repeats;
  the RNG reseeds from fresh samples and fills any length.
- `net_stack`: initial sequence numbers come from the source given, for
  connections made and accepted.
- Gauntlet: the Terminal shape asserts the boot's `entropy: seeded from`
  and `random`'s answer; the network shapes pass with random DNS ids and
  sequence numbers.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo xtask gauntlet`: exit 0.
