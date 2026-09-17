# Phase 277: Per-Caller Keys For Remote Commands

## Summary

Closes the last hardening item for remote IPC (`docs/next_steps.md` item 4): callers no longer share the master secret.

- `remote_ipc::derive_caller_key(master, caller)` = HMAC-SHA256(master, `"caller:" || name`). The kernel keeps only the master (`MasterKey`, a `KeySource` that derives on demand); each client holds a `CallerKey` (name plus 32-byte key) that an operator hands out, and can verify replies with nothing else.
- Both wire formats now carry the caller name and sign it: the UDP envelope gains a `caller` field, and the TCP line becomes `<nonce> <caller> <tag> <command>`. `envelope_from_bytes`/`line::verify` take a `KeySource`, look up the claimed caller's key, and return the caller with the message; a wrong or renamed caller fails the tag. Replies are signed with the requesting caller's key.
- Kernel: `remote_callers=a,b` on the command line restricts which names are admitted (empty means any name with a valid key); rejections are logged as `remote: caller "x" not allowed`.
- xtask: `PANDAGEN_REMOTE_CALLER` (default `xtask`) and `PANDAGEN_REMOTE_KEY` (base64) select the identity; without a key it is derived from `PANDAGEN_REMOTE_TOKEN`. `cargo xtask remote-key <caller>` prints the environment an operator gives to a caller.

## Verification

- `cargo test --workspace` green (remote_ipc tests cover derivation, caller binding in both formats, renamed/tampered lines, and client-side reply verification).
- QEMU as caller `ops` (derived key): UDP `cpus` and TCP `cpus` answered, TCP call under a wrong master rejected `unauthorized`, replay refused, `halt` refused. With `PANDAGEN_REMOTE_TOKEN=not-the-master` but the right `PANDAGEN_REMOTE_KEY`, UDP and TCP calls are still accepted: the caller needs only its own key.

## Limits

Revoking one caller means restricting `remote_callers=` at boot (or rotating the master); there is no runtime key revocation yet.
