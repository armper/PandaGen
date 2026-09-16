# Phase 218: Text/Graphics Display Mode Switching (Milestone A)

## Summary

This phase implements `GFX-020` from the graphics roadmap and completes Milestone A: a graphical desktop is visible in QEMU.

**Compositor on bare metal.** `graphics_rasterizer` and `services_gui_host` are now `no_std` + `alloc`. The workspace-manager adapter in `services_gui_host` moved behind a default-on `workspace` feature, so the kernel depends on the compositor core with `default-features = false`. `RASTER_CELL_WIDTH`, `RASTER_CELL_HEIGHT`, and `DesktopWindowLayer::sort_key` became public so callers can lay out windows in cell units and test z-order.

**`kernel_bootstrap/src/display_mode.rs`.** `DisplayMode::{TextConsole, GraphicsDesktop}` with alias parsing and `from_cmdline` for `display=...` on the kernel command line (last valid value wins, garbage ignored).

**`kernel_bootstrap/src/desktop_frame.rs`.** A pure `DesktopModel` (scrollback, prompt, cursor, status, optional editor viewport, optional palette) is mapped by `build_desktop_windows` into Main, Status, and Palette windows on a `DesktopLayout` derived from the pixel size. `DesktopFrameRenderer` owns one persistent RGBA target sized to the framebuffer and composes frames into it, so no per-frame surface allocation happens on the bump-allocated kernel heap.

**Runtime wiring.**
- Limine `ExecutableCmdlineRequest` is requested; `BootInfo` carries the parsed display mode.
- New workspace command `display [text | graphics | status]`, with graphics refused when no framebuffer exists.
- The loop keeps one present point. In graphics mode a dirty frame builds the model, renders it, and marks the pacer dirty; the present converts RGBA through `present_desktop_surface`. In text mode the shadow damage path is unchanged. Switching either way forces a full repaint and invalidates the shadow damage so no stale pixels survive.
- `boot/limine.conf` gains a second menu entry that boots straight into graphics.
- Kernel heap raised from 8 MiB to 32 MiB: the text shadow and the desktop target are about 4 MiB each at 1280x800 and the allocator never frees.

## Rationale

The roadmap's target stack is services publish views, the workspace decides layout, the GUI host composes, a rasterizer paints, the kernel presents. This phase is the first time all five stages run inside the kernel image on the real framebuffer. Keeping the desktop builder on a plain data model rather than the live session means the window mapping is unit-tested on the host, and the same compositor validated by golden tests paints the QEMU screen.

Making the mode a runtime switch rather than a build flag lets both renderers be compared on the same boot, which is how the font gap below was found.

## Known Gap

The rasterizer's `DESKTOP_FONT` covers uppercase letters, digits, and little punctuation. Lowercase text renders uppercase and `'`, `>`, `|` render as `?`. Tracked as `GFX-053` and named the next story.

## Tests

Added:

- `display_mode::tests`: alias parsing, cmdline last-wins and garbage handling, label round trip.
- `desktop_frame::tests`: layout fits the surface and degrades on tiny sizes, main/status window mapping with prompt cursor, scrollback clipped to visible rows, palette overlay focus and z-order, editor content and status, renderer paints the exact framebuffer size and repaints deterministically.
- `render_stats` tests now serialize on a mutex; they share global counters and had a latent parallel-run race.

Validated with:

- `cargo test -p kernel_bootstrap` (81 lib + 75 bin)
- `cargo test -p graphics_rasterizer -p services_gui_host`, and both built with `--no-default-features`
- `cargo build -p kernel_bootstrap --profile kernel --target x86_64-unknown-none -Zbuild-std=core,alloc`
- `cargo xtask qemu-script` session: `display graphics`, `help`, `Ctrl+P` palette, `open editor readme.md`, insert-mode typing, `:q!`, `display text`, with `--expect-serial` on both switch messages and screendumps reviewed for every state
- `cargo test --all`
