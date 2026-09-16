# Phase 215: Add Frame Pacing And Present Policy

## Summary

This phase implements `GFX-018` from the graphics roadmap.

New module `kernel_bootstrap/src/present_policy.rs`:

- `PresentPolicy` holds the minimum tick interval between hardware presents. `DEFAULT_BARE_METAL` is one PIT tick (100 Hz ceiling); `IMMEDIATE` disables pacing.
- `FramePacer` is pure state over a monotonic tick counter. Render passes call `mark_dirty()`; the loop calls `poll(now)` and gets `Idle`, `Defer { due_tick }`, or `Present`. `force(now)` bypasses pacing for mode switches.
- `PacerStats` counts presents, forced presents, coalesced dirty marks, and deferred polls.

`kernel_bootstrap/src/main.rs` wiring:

- the two render-site shadow presents became `mark_dirty()` calls
- the loop now has a single present point at the top of each iteration, gated by the pacer
- idle pausing is skipped while a present is pending so a deferred present lands on its due tick

## Rationale

Before this phase a present happened as a side effect of every render pass. Under key repeat or a burst of output lines, that meant one full-frame hardware copy per pass with no upper bound.

Separating render from present gives the desktop path the shape a compositor needs: rendering is as frequent as state changes, presenting is bounded by an explicit policy, and the two are connected only by a dirty flag. The pacer is tick-based rather than wall-clock-based so it is deterministic under test and works on bare metal with only the PIT.

A counter that goes backwards is treated as "due" so a broken clock degrades to present-when-dirty instead of freezing the display.

## Tests

Added in `present_policy::tests`:

- idle pacer never presents
- first present is immediate
- dirty marks inside the interval coalesce into one present
- present rate is bounded under continuous dirtying
- immediate policy presents on every dirty poll
- force presents inside the interval and restarts it
- clock going backwards does not freeze the display
- interval saturates near `u64::MAX`

Validated with:

- `cargo fmt --all`
- `cargo test -p kernel_bootstrap`
- `cargo build -p kernel_bootstrap --target x86_64-unknown-none -Zbuild-std=core,alloc` (no new warnings)
