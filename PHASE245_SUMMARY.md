# Phase 245: Surface Memory Budget

## Summary

This phase implements `GFX-047` from the graphics roadmap.

- `services_gui_host::memory::SurfaceBudget`: labelled reservations against a byte limit, `reserve` returning `BudgetError::Exceeded { label, requested, available }` or `Duplicate`, `release`, `used`/`available`, a refusal counter, and size helpers for RGBA and strided native surfaces. Nothing is allocated by the budget; callers allocate only after a successful reservation.
- Kernel: the budget is 75% of the heap (24 MiB of 32 MiB). The text shadow reserves its framebuffer size at boot; the desktop RGBA target reserves before a switch to graphics (or at boot for `display=graphics`). A refused reservation logs the typed error, raises an ERROR notice, and keeps text mode. The `mem` command now prints heap used, free, total, and allocation count.

## Measurement

Serial from a scripted QEMU session:

```
gfx budget: 24576 KiB of 32768 KiB heap
heap: used=4496 KiB ... allocations=265        (text mode after boot)
heap: used=8698 KiB ... allocations=2779       (after switching to graphics)
heap: used=9278 KiB ... allocations=13195      (after ten pointer moves)
```

The desktop target accounts for the 4 MiB step. The last step is the finding: ten pointer moves cost about 580 KiB and 10,400 allocations, because the desktop model, window list, and strings are rebuilt every frame and the bump allocator never frees. At that rate the heap is exhausted after a few hundred frames of mouse motion.

## Rationale

Budgeting makes the large buffers explicit and turns an allocation failure into a policy decision the shell can explain. The measurement it enabled is the more important outcome: it moves "the allocator never frees" from a known limitation to a quantified one with a clear next step.

Validated with `cargo test -p services_gui_host -p kernel_bootstrap`, the bare-metal build, and the QEMU session above.
