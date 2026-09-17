# Phase 241: Compact Scene Transport

## Summary

This phase implements `GFX-042` from the graphics roadmap.

**`services_gui_host::transport`**
- `SceneUpdate::{Keyframe(DesktopScene), Delta(SceneDelta)}`, serde-tagged.
- `SceneDelta { size, changed, removed, cursor, theme, damage }`: windows are identified by view id, so only new or changed windows are carried; the cursor and theme are carried only when they changed; `damage` is the union of every pixel rectangle the delta touches (old and new rects of changed windows, removed windows, both cursor sprites, or the whole surface on a theme change).
- `diff_scenes` and `apply_delta`, order-independent over the window list.
- `SceneEncoder` emits a keyframe for the first frame, on a surface size change, when the keyframe interval is reached, or after `reset` (a viewer joined), and deltas otherwise. `SceneDecoder` applies updates and rejects a delta with no base (`DecodeError::MissingKeyframe`).

**`services_remote_ui_host`**: `RemoteDesktopFrame` now carries a `SceneUpdate`; the host owns an encoder (keyframe every 60 updates by default) and exposes `request_keyframe`.

## Rationale

A caret blink is one cursor field, not a whole desktop. Diffing by window identity, rather than by position in the list, is what lets the shell rebuild its window list every frame while the wire only carries what changed. Damage is computed by the encoder so a receiver can repaint minimally without re-deriving it, and forced keyframes bound how much history a late joiner needs. The transport is pure over plain data, so it is deterministic under test.

## Tests

- delta round trip: changed, removed, and added windows, cursor change, damage extent; empty delta on identical scenes
- a caret-only delta serialises to less than a quarter of the keyframe
- encoder/decoder stay in sync through deltas, theme change with full-surface damage, interval keyframe, size-change keyframe, reset keyframe, and rejection of a baseless delta
- remote host: second frame is a delta, decoder reconstructs the scene, `request_keyframe` forces a keyframe

Validated with `cargo test -p services_gui_host -p services_remote_ui_host`, `cargo build -p services_gui_host --no-default-features`, and `cargo test -p kernel_bootstrap`.
