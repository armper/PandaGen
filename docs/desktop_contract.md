# Desktop Output Contract

This document records the canonical graphical desktop contract that the
graphics roadmap (`docs/path_to_graphics.md`, stories `GFX-001` to
`GFX-005`) asked for. Everything here is implemented and tested; the
document names the pieces and the decisions so they are not re-derived.

## Decision: retained windows, immediate content (GFX-002)

The contract is a hybrid:

- **Retained scene at the window level.** A frame is a `DesktopScene`
  (`services_gui_host`): a surface size in cells, a list of `DesktopWindow`s
  with roles, z-index, focus, chrome, tabs, and highlight, an optional
  `DesktopCursor`, an optional `Theme`, and an optional damage rectangle.
  Windows are identified by `ViewId`, which is what makes deltas, focus, and
  capture stable across frames.
- **Immediate content inside a window.** A window's `ViewFrame` carries a
  `ViewContent`: text lines, a status line, a panel, or `Graphics { ops }`, a
  list of `DrawOp`s executed in the view's own pixel space and clipped to the
  window's content area by the host. Views never learn where they are on
  screen.

Retained windows give the shell stable identity, hit testing, and cheap
deltas; immediate ops give apps drawing freedom without a scene graph the
kernel would have to keep alive.

## Surface, layers, damage, revision, timing (GFX-001)

| Concern | Where it lives |
| --- | --- |
| Surface dimensions | `DesktopScene.size` in cells; pixel size is cells times `RASTER_CELL_WIDTH` x `RASTER_CELL_HEIGHT` (8 x 18 with the desktop font). The kernel presenter (`DesktopSurface`) requires the RGBA frame to match the framebuffer exactly. |
| Layers | `DesktopWindowRole` maps to `DesktopWindowLayer` (workspace, overlay, palette, notification, modal, system); `composition_order` sorts by layer, then z-index, then view id, and painting and hit testing share that order. |
| Damage regions | `RasterRect` in pixels. Produced by `diff_scenes` (per changed window, caret-only changes damage only the caret cells) and by the kernel's text shadow tracker; consumed by `render_scene` and by the framebuffer presenter's damage copy. Incremental repaint equals full render inside the damage (property tested). |
| Frame revision | `RemoteDesktopFrame.revision` and `RemoteSnapshotFrame.revision` share one monotonic counter in `RemoteUiHost`. |
| Presentation timing | PIT ticks at 100 Hz. `FramePacer` allows at most one present per policy interval and coalesces dirty marks; `AnimationClock` schedules the next redraw tick; `GfxTelemetry` records present latency in ticks. |

## Graphical view content (GFX-003)

`view_types::ViewContent::Graphics { ops: Vec<DrawOp> }` with:

- `DrawOp::Fill`, `Border`, `RoundedFill`, `RoundedBorder`, `Line`, `Text`
- `PixelRect` (view-space pixels), `Color` (RGBA), `TextStyle` (compact font,
  muted)

The compositor executes ops through a `ContainerTarget` whose origin is the
window's text origin, so graphics and text views align, and whose clip is
the content area intersected with the current damage. Text renderers and
the workspace manager summarise graphics content as `[graphics: n ops]`.

## Colour, brush, border, and text-style model (GFX-005)

- `Color` is RGBA with no palette or terminal semantics; the compositor's
  `Theme` supplies named tokens (background, surface, borders, title fills,
  text, muted text, caret, selection, tabs, pointer) that a light or dark
  theme swaps as data.
- Brushes are solid fills; borders are thickness plus colour; rounded shapes
  take a pixel radius. Alpha blends source-over through `blit_image`/`blend_over`.
- Text style is a small closed set (`compact`, `muted`) that bitmap fonts can
  honour; `None` colour means the theme's text colour.

## Serialisation and replay (GFX-004)

`ViewContent`, `DrawOp`, `DesktopScene`, `SceneUpdate` (keyframe or delta),
and the remote frames are `serde` types with round-trip tests. A recorded
update stream replayed through `SceneReplay` reproduces every scene and,
rendered by the software backend, every pixel of the live session.
