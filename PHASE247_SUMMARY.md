# Phase 247: Clean Degradation Under Memory Pressure

## Summary

This phase implements `GFX-048` from the graphics roadmap.

**`services_gui_host::degrade`**
- `PressureThresholds::classify(free, total)`: `Normal`, `Low` (under 15% free), or `Critical` (under 5% free or under a 256 KiB frame reserve; also when the heap is absent).
- `Degradation::for_pressure`: a fixed, monotonic shedding order: low drops notices and hides the pointer sprite; critical additionally falls back to the text console.
- `PressureMonitor`: samples with hysteresis (a level must persist for N samples to be adopted or left) except that critical is adopted on the first sample.

**Kernel**
- The loop samples heap stats every iteration. On critical while in graphics mode it raises an ERROR notice, switches to text mode (which needs no per-frame allocation), and forces a full text repaint. On low it re-renders the desktop with notices and pointer removed.
- `heap stress <KiB>` holds allocations (in 1 MiB pieces, via `try_reserve_exact` so a refusal is a message rather than an abort) and `heap release` frees them, so the policy can be driven on the real image.
- A missing framebuffer was already handled (VGA fallback, graphics refused with a message); the budget from Phase 245 covers oversized surfaces.

## Verification

- `cargo test -p services_gui_host`: classification by ratio and reserve, degradation order, hysteresis and immediate critical, recovery.
- QEMU (`cargo xtask qemu-script`), serial: `heap stress 21000` left 3443 KiB free and logged `memory pressure: Low`; the screendump had no notice cards and no pointer sprite. `heap stress 3000` left 443 KiB free and logged `Critical`; the screendump shows the text console background. `heap release` freed 24000 KiB, logged `Normal`, and `mem` showed the heap back at its 8324 KiB baseline. No allocation errors or panics.

## Rationale

Degrading is a policy decision, so it lives in the GUI host as pure data over heap numbers, and the kernel only applies it. Shedding decorations before the desktop, and the desktop before anything else, keeps the user's work visible for as long as possible; immediate adoption of critical avoids waiting for confirmation while a frame could fail. Returning to graphics after recovery is deliberately left to the user.
