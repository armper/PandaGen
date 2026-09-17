# Phase 248: Display Path Telemetry

## Summary

This phase implements `GFX-049` from the graphics roadmap and completes Epic 10.

- `services_gui_host::telemetry::GfxTelemetry`: frames rendered, presents, rejected presents, present latency total/max (ticks) with a slow-present count (more than one tick), animation wakes, pointer events; `GfxSnapshot` adds pacer counters (presents, deferred, coalesced, forced), memory pressure and transitions, heap used/free/total, budget used/limit, and the snapshot tick, with `lines()` for display.
- Kernel: presents are timed at the single present point (both presenters now return success), frames are counted for both the desktop and the text shadow, animation wakes and pointer events are counted where they happen, and a snapshot is published to the workspace every loop iteration. `gfx stats` prints it; `gfx reset` clears the counters.

## Measurement

Scripted QEMU session after switching to graphics and moving the pointer:

```
gfx: frames=41 presents=41 rejected=0 slow=0 avg_present=0.58 ticks max=1 ticks
pacer: presents=41 deferred=0 coalesced=0 forced=0 anim_wakes=10 pointer_events=4
memory: pressure=Normal transitions=0 heap 8323/32768 KiB used, budget 8000/24576 KiB, tick=1564
```

Every render became exactly one present with no deferrals, presents complete within a tick, and the ten animation wakes are the caret blink over the session.

## Rationale

Every mechanism in the display path already kept counters; what was missing was one place that reads them and a command that shows them. Counting at the present point and the render sites, rather than inside the compositor, keeps the compositor pure and makes the numbers the loop's own account of what it did.

Validated with `cargo test -p services_gui_host -p kernel_bootstrap`, the bare-metal build, and the QEMU session above.
