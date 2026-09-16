# Phase 230: Animation Timing Hooks And Caret Blink

## Summary

This phase implements `GFX-030` from the graphics roadmap.

`services_gui_host::animation`:
- `Easing::{Linear, EaseIn, EaseOut, EaseInOut, Step}` over fixed-point progress (0..=1000), integer-only and monotonic.
- `Transition { start_tick, duration_ticks, from, to, easing }` with `value_at(tick)` (exact endpoints, rounded midpoints), `progress_at`, `is_finished_at`, and `next_change_after`.
- `Blink { period_ticks, phase_tick }` with `is_on_at`, `next_flip_after`, and `restart_at`.
- `AnimationClock`: keeps the earliest scheduled wake; `poll(now)` fires once when due. This is the bridge between time-indexed state and the event-driven loop.

Kernel: `DesktopModel.caret_visible` drives whether the main window frame carries a cursor. The loop owns a `Blink` (100 ticks, 0.5 s on / 0.5 s off) and an `AnimationClock`; each graphics frame samples the blink and schedules the next flip, the clock marks the desktop dirty when a flip is due, and any keyboard input restarts the blink so the caret is visible right after typing.

## Rationale

The roadmap asks for animation "without requiring a game-engine model." PandaGen renders on state change and presents on a paced tick, so animation has to be expressed as state sampled at a tick plus a next-wake time the loop can schedule, not a per-frame callback. `AnimationClock` gives the loop exactly one question to ask per iteration. The blinking caret is the smallest real consumer and exercises the whole path: sample, schedule, wake, redraw, present.

## Tests

- easing endpoints, clamping, monotonicity, and characteristic midpoints
- transition endpoints, midpoint rounding, reverse and negative spans, zero duration, eased midpoint, next-change scheduling
- blink phase for even, odd, and zero periods, next flip, restart
- clock keeps the earliest wake and fires once; a simulated loop redraws once per half period
- kernel: caret visibility removes or keeps the main window cursor

Verified in QEMU: six screendumps a quarter second apart in graphics mode show the caret present in two and absent in four, matching a 0.5 s on/off cycle.
