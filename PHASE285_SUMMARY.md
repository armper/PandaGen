# Phase 285: The Shipped Image Authenticated With A Published Constant

Gauntlet round 2, security critic. Its headline finding was configuration
rather than cryptography, and it was the most serious thing on the board.

## The finding

`boot/limine.conf` set neither `remote_token=` nor `remote_callers=`. So the
image built from this repository:

- kept `REMOTE_TOKEN` at `remote_ipc::DEFAULT_REMOTE_TOKEN`, the string
  `"pandagen-dev"`, a `pub const` in the open source tree;
- kept `REMOTE_CALLERS` empty, which `allows()` short-circuits to "any name".

Every protection built on top — per-caller key derivation, the replay window,
the read-only allowlist — rests on that secret. A secret anyone can read is
not one. Anyone who had the repository could drive any machine running an
image built from it.

The critic was also clear about what is *not* wrong, which is worth
recording: the HMAC covers every field with NUL separators so there is no
length-ambiguity gap, the caller name is inside the tag so renaming is
rejected, `tags_equal` is constant time, replies are bound to their request
and re-signed, and compromising one caller's derived key does not reveal the
master or any other caller's key. The construction is sound. It was being
handed a public key.

## The fix

Each build gets its own secret. `cargo xtask iso` draws 128 bits from
`/dev/urandom`, substitutes it into the boot config it stages into the image,
and leaves it in `dist/remote-token`, which is already outside version
control. The client reads the same file, so `cargo xtask remote` and the
gauntlet scripts keep working with no ceremony.

The kernel now refuses to open its remote ports at all without a secret. Not
"falls back to the default": the UDP remote port and the TCP command port are
never bound, and the boot log says so. A value that is still the build-time
placeholder, or that equals the published development constant, counts as
absent — so an image built by hand, bypassing xtask, is closed rather than
open with a guessable key.

## And `boot` left the allowlist

`boot` was a remote-callable command that prints the kernel's physical and
virtual load addresses and the HHDM offset: precisely what turns a memory bug
into an exploit. It is off the list. `mem` and `heap` stay, since allocator
totals are diagnostic rather than an address disclosure.

## Verification

`gauntlet/remote_auth.py`, run against a real boot:

- the secret this image was built with is accepted;
- a command signed with the constant published in this repository is refused;
- correctly signed requests for `halt`, `boot`, and `write` are all refused.

Replay rejection and the rest of the allowlist still behave. The full gauntlet
suite and `cargo test --workspace` are green.

## Still open

The replay window is 256 entries and an attacker able to generate valid
traffic can flush it before replaying something older; nonces are not ordered
or time-bound. `remote_callers=` still defaults to accepting any caller name,
which is defensible now that deriving a key requires the per-build master,
but it means every caller has identical authority.
