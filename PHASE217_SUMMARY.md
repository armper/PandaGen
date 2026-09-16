# Phase 217: Optimized Kernel Profile, Shadow Presentation On, Scripted QEMU

## Summary

Three connected changes that came out of validating Phases 214 to 216 on the real kernel image in QEMU.

**Optimized kernel image.** Root `Cargo.toml` gains `[profile.kernel]` (inherits release, opt-level 2, line tables only). `cargo xtask iso` builds the kernel with `--profile kernel` and stages it from `target/x86_64-unknown-none/kernel/`. Host tests still use `dev`.

**Shadow presentation enabled.** `FB_SHADOW_ENABLED` in `kernel_bootstrap/src/main.rs` is now `true`. The text workspace renders into an off-screen shadow, and the paced, damage-limited presenter (`GFX-017` to `GFX-019`) is the only writer to the hardware framebuffer on that path. Three imports and one tick parameter that only existed under `debug_assertions` were adjusted so the release-profile kernel builds without new warnings.

**Scripted headless QEMU.** `cargo xtask qemu-script` boots the ISO with no display, drives the QEMU monitor over a Unix socket, injects `sendkey` sequences with `sleep:` and `shot:` directives, writes PPM screendumps and the serial log, and fails on missing `--expect-serial` text, a kernel panic, or a rejected framebuffer present.

## Rationale

Enabling the shadow path exposed that a full-frame present took 3.2 seconds. Boot-time measurements on serial showed the cause was not VRAM: clearing 4 MiB of heap and clearing 4 MiB of framebuffer both took about 670 ms, and a `memcpy`-based present took 2.9 s. The kernel and the `-Zbuild-std` core/alloc/compiler_builtins were all compiled at opt-level 0. With the `kernel` profile every one of those measurements dropped to at most one 10 ms tick.

The shadow toggle had been left off, most likely because of this cost. With pacing, damage limiting, and an optimized image, the shadow path is now the cheaper and architecturally correct default: the hardware buffer is written once per tick at most and only inside the damage box.

The scripted QEMU command exists because none of the above could be trusted from `cargo test` alone. It turns the QEMU window into something the test loop can drive, screenshot, and assert on.

## Tests

Validated with:

- `cargo test -p kernel_bootstrap` (72 lib + 66 bin tests)
- `cargo build -p kernel_bootstrap --profile kernel --target x86_64-unknown-none -Zbuild-std=core,alloc` (no new warnings)
- `cargo xtask iso`
- scripted QEMU sessions with screendumps verified by eye: boot banner, `help` output after scroll, `open editor readme.md`, insert-mode typing, `:q` refused on a dirty buffer, `:q!` back to the workspace with full redraw, `ls`, `Ctrl+P` palette open and `Esc` close. Serial logs showed zero rejected presents and no panics.
- `cargo xtask qemu-script --keys "h,e,l,p,ret,sleep:0.5,shot:help" --expect-serial "WS > help"` passes.
