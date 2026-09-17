# Phase 240: Remote Graphical Desktop Scenes

## Summary

This phase implements `GFX-041` from the graphics roadmap.

**`services_gui_host::DesktopScene`**: `{ size (cells), windows, cursor, theme, damage }`, fully serialisable, with `Compositor::render_scene(target, &scene)` (the scene's theme overrides the local one) and `render_scene_rgba`. Rendering a scene equals rendering its parts directly, which is asserted by a test.

**`services_remote_ui_host`**: `RemoteDesktopFrame { revision, timestamp_ns, scene }`, `RemoteUiHost::push_desktop`, and a `SnapshotSink::send_desktop` hook with a default that drops the frame, so existing text-only sinks keep working. `IpcSnapshotSink` sends desktop frames under the `ui.desktop` action with their own schema version; `JsonLineSink` writes tagged records (`{"kind":"snapshot"|"desktop",...}`); `InMemorySink` collects both. Text and desktop frames share one revision counter so a mixed stream is totally ordered.

## Rationale

The desktop is data at every level now (shell model, hosted surfaces, windows, theme), so the remote transport carries that data rather than pixels: it is smaller, it survives any viewer resolution or theme, and a viewer running the same compositor paints exactly what the sender saw. Keeping desktop frames on a separate action means a viewer that only understands text snapshots can ignore them, and the shared revision stream is what a replay (`GFX-043`) needs to interleave both kinds faithfully.

## Tests

- `services_gui_host`: scene JSON round trip with all fields; `render_scene_rgba` equals direct rendering with the same theme and cursor; optional fields default when absent.
- `services_remote_ui_host`: desktop frames continue the revision stream and reach every sink; the IPC sink uses the desktop action and the payload decodes back to the scene; the JSON-line sink tags both frame kinds and round-trips.

Validated with `cargo test -p services_gui_host -p services_remote_ui_host`.
