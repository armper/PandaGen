# Phase 242: Deterministic Graphical Replay

## Summary

This phase implements `GFX-043` and `GFX-044` from the graphics roadmap.

- `services_gui_host::transport::SceneReplay`: a cursor over a recorded `SceneUpdate` stream with `step` and `replay_all`, built on `SceneDecoder`.
- `Compositor::render_scene` now means "incremental repaint of a persistent target, limited to the scene's damage", and the new `render_scene_full` (used by `render_scene_rgba`) paints everything, ignoring the damage hint. The previous behaviour painted only the damaged region into a fresh buffer, which the new replay test caught as a pixel mismatch on the first delta frame.

## Tests

- `transport`: a six-frame session (caret move, notice added, theme change, notice removed, pointer move) is encoded with a small keyframe interval so both keyframes and deltas occur, serialised to JSON and back, replayed, and checked three ways per frame: scene equality (as window sets plus cursor and theme), pixel identity of full renders, and pixel identity of an incremental viewer that repaints a persistent buffer by damage only. Replay is repeatable and stepping ends cleanly.
- `services_remote_ui_host`: a session pushed through the JSON-line sink with an interleaved text snapshot yields desktop frames with revisions 1 to 5 whose replay reproduces every scene.

## Rationale

Text snapshots were replayable because they were data; graphical sessions are now data too, so replay is the same discipline. The incremental check is the important one: it proves the transport's damage rectangles are sufficient, not only that keyframes are correct, which is what a real viewer relies on.

Validated with `cargo test -p services_gui_host -p services_remote_ui_host` and `cargo build -p services_gui_host --no-default-features`.
