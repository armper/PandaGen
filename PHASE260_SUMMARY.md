# Phase 260: Authenticated Remote IPC

## Summary

Hardens the Phase 259 remote command port. Every datagram now carries an HMAC-SHA256 tag over a shared token; the kernel silently drops anything that does not verify, so the port no longer answers strangers.

- `remote_ipc::sha256`: dependency-free SHA-256 (`digest`), HMAC-SHA256 (`hmac`, RFC 2104), and constant-time `tags_equal`. Tested against the FIPS "abc" / 56-byte / 64-byte vectors, the empty string, and RFC 4231 HMAC test case 2.
- The wire envelope gains a base64 `tag` = HMAC(token, id || 0 || action || 0 || payload). `envelope_to_bytes(message, token)` signs; `envelope_from_bytes(bytes, token)` verifies and returns `Unauthorized` on mismatch. `DEFAULT_REMOTE_TOKEN` (`pandagen-dev`) is the development secret.
- `kernel_bootstrap`: `REMOTE_TOKEN` (up to 64 bytes) defaults to the dev token and can be replaced with `remote_token=<value>` on the Limine command line (logged as `remote: token set from command line`, never echoing the value). Verification failures are logged as `remote: bad envelope (Authorization denied)`, counted as denied, and get no reply.
- `cargo xtask remote` signs with `PANDAGEN_REMOTE_TOKEN` (or the default); replies are verified too. `qemu-script`'s `remote:` step accepts a third `;<token>` field, and a socket timeout is reported as the stable `no reply (timeout)` since that is the intended symptom of a bad token.

## Verification

- `cargo test --workspace` green (remote_ipc 5 tests).
- QEMU: `cpus` with the default token answers; `cpus` with `wrong-token` gets no reply and serial shows `remote: bad envelope (Authorization denied)`; `halt` is still refused as `command not allowed`; `mem` answers afterwards, so a rejected call leaves the server ready.

## Not Yet

No replay protection (a captured signed datagram could be resent to re-run the same read-only command) and no per-caller keys. A nonce or timestamp inside the signed input would close the replay window.
