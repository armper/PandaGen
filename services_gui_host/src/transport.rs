//! Compact transport for graphical updates (GFX-042).
//!
//! A remote session should not pay for a full scene when only the caret
//! blinked. `SceneEncoder` turns successive `DesktopScene`s into
//! `SceneUpdate`s: a `Keyframe` carrying the whole scene, or a `Delta`
//! carrying only the windows that changed (by view id), removed ids, and
//! cursor/theme changes, plus the damage rectangle those changes imply.
//! `SceneDecoder` applies updates in order and rejects a delta it has no
//! base for. Encoder and decoder are pure, so replay is deterministic.

use alloc::vec::Vec;
use graphics_rasterizer::RasterRect;
use serde::{Deserialize, Serialize};
use view_types::ViewId;

use crate::{
    caret_pixel_rect, window_pixel_rect, DesktopCursor, DesktopScene, DesktopWindow, SurfaceSize,
    Theme,
};

/// Changes between two scenes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SceneDelta {
    pub size: SurfaceSize,
    /// Windows that are new or changed; replace by view id.
    pub changed: Vec<DesktopWindow>,
    /// Windows no longer present.
    pub removed: Vec<ViewId>,
    /// `Some(new cursor)` when the cursor changed (including to hidden).
    #[serde(default)]
    pub cursor: Option<Option<DesktopCursor>>,
    /// `Some(theme)` when the theme changed.
    #[serde(default)]
    pub theme: Option<Option<Theme>>,
    /// Union of pixel rectangles touched by this delta.
    #[serde(default)]
    pub damage: Option<RasterRect>,
}

impl SceneDelta {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
            && self.removed.is_empty()
            && self.cursor.is_none()
            && self.theme.is_none()
    }
}

/// One transport unit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneUpdate {
    Keyframe(DesktopScene),
    Delta(SceneDelta),
}

/// Damage for a window that changed. A caret-only change (same window with
/// only `frame.cursor` different) damages just the two caret rectangles,
/// which is what makes a blinking caret cheap on the wire and on screen.
fn changed_window_damage(old: &DesktopWindow, new: &DesktopWindow) -> RasterRect {
    let mut same_but_caret = new.clone();
    same_but_caret.frame.cursor = old.frame.cursor;
    if &same_but_caret == old {
        let mut acc = None;
        for cursor in [old.frame.cursor, new.frame.cursor].into_iter().flatten() {
            if let Some(rect) = caret_pixel_rect(new, cursor) {
                acc = union(acc, rect);
            }
        }
        if let Some(rect) = acc {
            return rect;
        }
    }
    union(
        Some(window_pixel_rect(old.rect)),
        window_pixel_rect(new.rect),
    )
    .expect("union of two rects")
}

/// Extend `acc` by `rect`.
fn union(acc: Option<RasterRect>, rect: RasterRect) -> Option<RasterRect> {
    Some(match acc {
        None => rect,
        Some(a) => {
            let x = a.x.min(rect.x);
            let y = a.y.min(rect.y);
            let right = a.right().max(rect.right());
            let bottom = a.bottom().max(rect.bottom());
            RasterRect::new(x, y, right - x, bottom - y)
        }
    })
}

/// Compute the delta from `prev` to `next`. Window order within the scene
/// does not matter; identity is the view id.
pub fn diff_scenes(prev: &DesktopScene, next: &DesktopScene) -> SceneDelta {
    let mut damage = None;
    let mut changed = Vec::new();
    for window in &next.windows {
        match prev
            .windows
            .iter()
            .find(|w| w.frame.view_id == window.frame.view_id)
        {
            Some(old) if old == window => {}
            Some(old) => {
                damage = union(damage, changed_window_damage(old, window));
                changed.push(window.clone());
            }
            None => {
                damage = union(damage, window_pixel_rect(window.rect));
                changed.push(window.clone());
            }
        }
    }
    let mut removed = Vec::new();
    for old in &prev.windows {
        if !next
            .windows
            .iter()
            .any(|w| w.frame.view_id == old.frame.view_id)
        {
            damage = union(damage, window_pixel_rect(old.rect));
            removed.push(old.frame.view_id);
        }
    }
    let cursor = if prev.cursor != next.cursor {
        if let Some(c) = prev.cursor {
            damage = union(damage, c.bounds());
        }
        if let Some(c) = next.cursor {
            damage = union(damage, c.bounds());
        }
        Some(next.cursor)
    } else {
        None
    };
    let theme = if prev.theme != next.theme {
        // A theme change repaints everything.
        let (w, h) = next.pixel_size();
        damage = union(damage, RasterRect::new(0, 0, w, h));
        Some(next.theme)
    } else {
        None
    };
    SceneDelta {
        size: next.size,
        changed,
        removed,
        cursor,
        theme,
        damage,
    }
}

/// Whether any two windows in `scene` share a view id.
///
/// A delta identifies windows by view id -- `changed` updates the first
/// match and `removed` drops every match -- so a scene with a repeated id
/// cannot be described by one. Encoding it anyway silently produced a
/// *different* scene at the viewer, and since every later delta builds on
/// that, the stream never recovers.
fn has_duplicate_view_ids(scene: &DesktopScene) -> bool {
    // `ViewId` is not `Ord`, and the window count is small, so compare the
    // uuids directly rather than add an ordering to the wire type for this.
    let mut seen = alloc::vec::Vec::new();
    for window in &scene.windows {
        let id = window.frame.view_id.as_uuid();
        if seen.contains(&id) {
            return true;
        }
        seen.push(id);
    }
    false
}

/// Apply `delta` to `base`, producing the next scene. Changed windows keep
/// their position in the list when they already exist, so ordering stays
/// stable for equal z-keys.
pub fn apply_delta(base: &DesktopScene, delta: &SceneDelta) -> DesktopScene {
    let mut windows: Vec<DesktopWindow> = base
        .windows
        .iter()
        .filter(|w| !delta.removed.contains(&w.frame.view_id))
        .cloned()
        .collect();
    for window in &delta.changed {
        match windows
            .iter_mut()
            .find(|w| w.frame.view_id == window.frame.view_id)
        {
            Some(slot) => *slot = window.clone(),
            None => windows.push(window.clone()),
        }
    }
    DesktopScene {
        size: delta.size,
        windows,
        cursor: delta.cursor.unwrap_or(base.cursor),
        theme: delta.theme.unwrap_or(base.theme),
        damage: delta.damage,
    }
}

/// Sender side: emits keyframes when it must and deltas otherwise.
#[derive(Debug, Clone, Default)]
pub struct SceneEncoder {
    last: Option<DesktopScene>,
    since_keyframe: u32,
    /// Emit a keyframe at least every this many updates (0 = only when needed).
    pub keyframe_interval: u32,
}

impl SceneEncoder {
    pub fn new(keyframe_interval: u32) -> Self {
        Self {
            last: None,
            since_keyframe: 0,
            keyframe_interval,
        }
    }

    /// Encode `scene` relative to what was sent before.
    pub fn encode(&mut self, scene: &DesktopScene) -> SceneUpdate {
        let need_keyframe = match &self.last {
            None => true,
            Some(last) => {
                last.size != scene.size
                    || (self.keyframe_interval > 0 && self.since_keyframe >= self.keyframe_interval)
                    // A repeated view id cannot be expressed as a delta, so
                    // send the scene itself rather than something the viewer
                    // will decode into a different picture and then build
                    // every later frame on.
                    || has_duplicate_view_ids(last)
                    || has_duplicate_view_ids(scene)
            }
        };
        let update = if need_keyframe {
            self.since_keyframe = 0;
            // A keyframe replaces the viewer's whole scene, so its damage is
            // the whole surface. It used to carry `scene.damage` -- the
            // producer's "what changed since the previous frame", which is
            // meaningless to a viewer that has no previous frame. A viewer
            // honouring it would repaint one small rectangle of an otherwise
            // blank or stale surface and leave the rest wrong until
            // something else happened to damage it.
            let (w, h) = scene.pixel_size();
            let mut scene = scene.clone();
            scene.damage = Some(RasterRect::new(0, 0, w, h));
            SceneUpdate::Keyframe(scene)
        } else {
            self.since_keyframe += 1;
            SceneUpdate::Delta(diff_scenes(self.last.as_ref().expect("checked"), scene))
        };
        self.last = Some(scene.clone());
        update
    }

    /// Forget the base so the next update is a keyframe (new viewer joined).
    pub fn reset(&mut self) {
        self.last = None;
        self.since_keyframe = 0;
    }
}

/// Receiver side.
#[derive(Debug, Clone, Default)]
pub struct SceneDecoder {
    current: Option<DesktopScene>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// A delta arrived before any keyframe.
    MissingKeyframe,
}

impl SceneDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> Option<&DesktopScene> {
        self.current.as_ref()
    }

    /// Apply one update; returns the resulting scene.
    pub fn apply(&mut self, update: &SceneUpdate) -> Result<&DesktopScene, DecodeError> {
        let next = match update {
            SceneUpdate::Keyframe(scene) => scene.clone(),
            SceneUpdate::Delta(delta) => {
                let base = self.current.as_ref().ok_or(DecodeError::MissingKeyframe)?;
                apply_delta(base, delta)
            }
        };
        self.current = Some(next);
        Ok(self.current.as_ref().expect("just set"))
    }
}

/// Deterministic replay of a recorded update stream (GFX-043).
///
/// Feeding the same updates in the same order always yields the same scenes,
/// and rendering those scenes with the same compositor yields the same
/// pixels as the live session did. `SceneReplay` is a thin cursor over a
/// decoder so a viewer, a test, or a debugger can step frame by frame.
#[derive(Debug, Clone, Default)]
pub struct SceneReplay {
    updates: Vec<SceneUpdate>,
    position: usize,
    decoder: SceneDecoder,
}

impl SceneReplay {
    pub fn new(updates: Vec<SceneUpdate>) -> Self {
        Self {
            updates,
            position: 0,
            decoder: SceneDecoder::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.updates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    pub fn position(&self) -> usize {
        self.position
    }

    /// Apply the next update; `None` at the end of the stream.
    pub fn step(&mut self) -> Option<Result<DesktopScene, DecodeError>> {
        let update = self.updates.get(self.position)?;
        self.position += 1;
        Some(self.decoder.apply(update).map(|scene| scene.clone()))
    }

    /// Replay everything from the start, collecting each reconstructed scene.
    pub fn replay_all(&mut self) -> Result<Vec<DesktopScene>, DecodeError> {
        self.position = 0;
        self.decoder = SceneDecoder::new();
        let mut scenes = Vec::with_capacity(self.updates.len());
        while let Some(result) = self.step() {
            scenes.push(result?);
        }
        Ok(scenes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Compositor, DesktopWindowRole, SurfaceRect, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH,
    };
    use alloc::string::ToString;
    use alloc::vec;
    use view_types::{CursorPosition, ViewContent, ViewFrame, ViewKind};

    fn window(id: ViewId, title: &str, rect: SurfaceRect) -> DesktopWindow {
        DesktopWindow::new(
            ViewFrame::new(
                id,
                ViewKind::TextBuffer,
                1,
                ViewContent::text_buffer(vec![title.to_string()]),
                0,
            )
            .with_title(title),
            rect,
        )
    }

    fn scene(ids: &[ViewId]) -> DesktopScene {
        DesktopScene::new(
            SurfaceSize::new(40, 20),
            vec![
                window(ids[0], "main", SurfaceRect::new(0, 0, 30, 20)).focused(),
                window(ids[1], "side", SurfaceRect::new(30, 0, 10, 20)),
            ],
        )
        .with_cursor(Some(DesktopCursor::new(5, 5)))
    }

    #[test]
    fn test_delta_round_trips_and_only_carries_changes() {
        let ids = [ViewId::new(), ViewId::new(), ViewId::new()];
        let a = scene(&ids);
        // Move the caret in main, drop side, add a notice, move the pointer.
        let mut b = a.clone();
        b.windows[0].frame.cursor = Some(CursorPosition::new(0, 3));
        b.windows.remove(1);
        b.windows.push(
            window(ids[2], "note", SurfaceRect::new(20, 0, 10, 3))
                .with_role(DesktopWindowRole::Notification),
        );
        b.cursor = Some(DesktopCursor::new(100, 100));

        let delta = diff_scenes(&a, &b);
        assert_eq!(delta.changed.len(), 2, "changed main and new note");
        assert_eq!(delta.removed, vec![ids[1]]);
        assert_eq!(delta.cursor, Some(Some(DesktopCursor::new(100, 100))));
        assert_eq!(delta.theme, None);
        let damage = delta.damage.unwrap();
        // Damage covers the side window, the note, the caret cells in main,
        // and both pointer sprites (the old one starts at x = 5).
        assert_eq!(damage.x, 5);
        assert!(damage.right() >= 40 * RASTER_CELL_WIDTH);
        assert!(damage.bottom() >= 20 * RASTER_CELL_HEIGHT);

        let mut applied = apply_delta(&a, &delta);
        applied.damage = None;
        let mut expected = b.clone();
        expected.damage = None;
        // Same windows regardless of list order.
        assert_eq!(applied.windows.len(), expected.windows.len());
        for w in &expected.windows {
            assert!(applied.windows.contains(w));
        }
        assert_eq!(applied.cursor, expected.cursor);
        assert_eq!(applied.size, expected.size);

        // Caret-only change damages just the caret cells, not the window.
        let mut caret_moved = b.clone();
        caret_moved.windows[0].frame.cursor = Some(CursorPosition::new(0, 5));
        let small = diff_scenes(&b, &caret_moved);
        let d = small.damage.unwrap();
        assert!(
            d.width <= 4 + 5 * RASTER_CELL_WIDTH && d.height <= RASTER_CELL_HEIGHT,
            "{d:?}"
        );
        assert_eq!(small.changed.len(), 1);

        // No change: empty delta with no damage.
        let none = diff_scenes(&b, &b);
        assert!(none.is_empty());
        assert_eq!(none.damage, None);
    }

    #[test]
    fn test_caret_blink_delta_is_much_smaller_than_keyframe() {
        let ids = [ViewId::new(), ViewId::new()];
        let a = scene(&ids);
        let mut b = a.clone();
        b.cursor = Some(DesktopCursor::new(6, 5));
        let key = serde_json::to_vec(&SceneUpdate::Keyframe(a.clone())).unwrap();
        let delta = serde_json::to_vec(&SceneUpdate::Delta(diff_scenes(&a, &b))).unwrap();
        assert!(
            delta.len() * 4 < key.len(),
            "delta {} vs keyframe {}",
            delta.len(),
            key.len()
        );
        let back: SceneUpdate = serde_json::from_slice(&delta).unwrap();
        assert_eq!(back, SceneUpdate::Delta(diff_scenes(&a, &b)));
    }

    #[test]
    fn test_encoder_and_decoder_stay_in_sync_with_keyframes_when_needed() {
        let ids = [ViewId::new(), ViewId::new()];
        let mut encoder = SceneEncoder::new(3);
        let mut decoder = SceneDecoder::new();

        let first = scene(&ids);
        let u1 = encoder.encode(&first);
        assert!(
            matches!(u1, SceneUpdate::Keyframe(_)),
            "first is a keyframe"
        );
        assert_eq!(decoder.apply(&u1).unwrap().windows.len(), 2);

        let mut second = first.clone();
        second.cursor = Some(DesktopCursor::new(9, 9));
        let u2 = encoder.encode(&second);
        assert!(matches!(u2, SceneUpdate::Delta(_)));
        let mut got = decoder.apply(&u2).unwrap().clone();
        got.damage = None;
        assert_eq!(got, second);

        // Theme change travels in a delta and repaints everything.
        let third = second.clone().with_theme(Theme::LIGHT);
        let u3 = encoder.encode(&third);
        match &u3 {
            SceneUpdate::Delta(d) => {
                assert_eq!(d.theme, Some(Some(Theme::LIGHT)));
                assert_eq!(
                    d.damage,
                    Some(RasterRect::new(
                        0,
                        0,
                        40 * RASTER_CELL_WIDTH,
                        20 * RASTER_CELL_HEIGHT
                    ))
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(decoder.apply(&u3).unwrap().theme, Some(Theme::LIGHT));

        // Two deltas so far; the third delta is still allowed, then the
        // interval forces a keyframe.
        let u4 = encoder.encode(&third);
        assert!(matches!(u4, SceneUpdate::Delta(_)));
        decoder.apply(&u4).unwrap();
        let u5 = encoder.encode(&third);
        assert!(matches!(u5, SceneUpdate::Keyframe(_)));
        decoder.apply(&u5).unwrap();

        // Size change forces a keyframe.
        let mut resized = third.clone();
        resized.size = SurfaceSize::new(10, 10);
        assert!(matches!(encoder.encode(&resized), SceneUpdate::Keyframe(_)));

        // Reset (new viewer) forces a keyframe; a delta without base is rejected.
        encoder.reset();
        assert!(matches!(encoder.encode(&resized), SceneUpdate::Keyframe(_)));
        let mut fresh = SceneDecoder::new();
        assert_eq!(
            fresh.apply(&SceneUpdate::Delta(diff_scenes(&first, &second))),
            Err(DecodeError::MissingKeyframe)
        );
    }

    #[test]
    fn test_replay_reconstructs_every_scene_and_every_pixel() {
        let ids = [ViewId::new(), ViewId::new(), ViewId::new()];
        // A short "session": caret moves, a notice appears, the theme flips,
        // the notice goes away, the pointer moves.
        let mut live = Vec::new();
        let base = scene(&ids);
        live.push(base.clone());
        let mut s2 = base.clone();
        s2.windows[0].frame.cursor = Some(CursorPosition::new(0, 2));
        live.push(s2.clone());
        let mut s3 = s2.clone();
        s3.windows.push(
            window(ids[2], "note", SurfaceRect::new(25, 0, 15, 3))
                .with_role(DesktopWindowRole::Notification),
        );
        live.push(s3.clone());
        let s4 = s3.clone().with_theme(Theme::LIGHT);
        live.push(s4.clone());
        let mut s5 = s4.clone();
        s5.windows.retain(|w| w.frame.view_id != ids[2]);
        live.push(s5.clone());
        let mut s6 = s5.clone();
        s6.cursor = Some(DesktopCursor::new(50, 60));
        live.push(s6.clone());

        // Record with a small keyframe interval so both kinds appear.
        let mut encoder = SceneEncoder::new(2);
        let updates: Vec<SceneUpdate> = live.iter().map(|s| encoder.encode(s)).collect();
        assert!(updates.iter().any(|u| matches!(u, SceneUpdate::Delta(_))));
        assert!(
            updates
                .iter()
                .filter(|u| matches!(u, SceneUpdate::Keyframe(_)))
                .count()
                >= 2
        );

        // Serialise the stream as a viewer would receive it and replay it.
        let wire: Vec<Vec<u8>> = updates
            .iter()
            .map(|u| serde_json::to_vec(u).unwrap())
            .collect();
        let received: Vec<SceneUpdate> = wire
            .iter()
            .map(|b| serde_json::from_slice(b).unwrap())
            .collect();
        let mut replay = SceneReplay::new(received);
        assert_eq!(replay.len(), live.len());
        let scenes = replay.replay_all().unwrap();
        assert_eq!(scenes.len(), live.len());

        let compositor = Compositor::new();
        // Incremental viewer: one persistent buffer repainted by damage only.
        let (w, h) = base.pixel_size();
        let mut incremental = graphics_rasterizer::RgbaBuffer::new(w, h, Theme::DEFAULT.background);
        for (index, (expected, got)) in live.iter().zip(&scenes).enumerate() {
            compositor.render_scene(&mut incremental, got);
            let full = compositor.render_scene_rgba(expected);
            assert_eq!(
                incremental.as_bytes(),
                full.pixels.as_slice(),
                "incremental repaint diverged at frame {index}"
            );
            let mut got_scene = got.clone();
            got_scene.damage = None;
            let mut expected_scene = expected.clone();
            expected_scene.damage = None;
            // Windows may be reordered by delta application; compare as sets.
            assert_eq!(
                got_scene.windows.len(),
                expected_scene.windows.len(),
                "frame {index}"
            );
            for w in &expected_scene.windows {
                assert!(got_scene.windows.contains(w), "frame {index}");
            }
            assert_eq!(got_scene.cursor, expected_scene.cursor, "frame {index}");
            assert_eq!(got_scene.theme, expected_scene.theme, "frame {index}");
            // And the pixels are identical.
            let live_pixels = compositor.render_scene_rgba(expected);
            let replay_pixels = compositor.render_scene_rgba(got);
            assert_eq!(live_pixels.pixels, replay_pixels.pixels, "frame {index}");
        }

        // Stepping is resumable and stable across a second replay.
        let again = replay.replay_all().unwrap();
        assert_eq!(again, scenes);
        assert_eq!(replay.position(), live.len());
        assert!(replay.step().is_none());
    }
}

#[cfg(test)]
mod duplicate_view_id_tests {
    use super::*;
    use crate::{DesktopWindow, SurfaceRect, SurfaceSize};
    use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

    fn window(view_id: ViewId, line: &str) -> DesktopWindow {
        let frame = ViewFrame::new(
            view_id,
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec![line.to_string()]),
            0,
        );
        DesktopWindow::new(frame, SurfaceRect::new(0, 0, 10, 5))
    }

    fn scene(windows: Vec<DesktopWindow>) -> DesktopScene {
        DesktopScene {
            size: SurfaceSize::new(40, 12),
            windows,
            cursor: None,
            theme: None,
            damage: None,
        }
    }

    /// A scene arriving over the wire is not trusted to have distinct view
    /// ids, and `apply_delta` keys on them: `removed` drops every window with
    /// a matching id while `changed` updates only the first. Whatever the
    /// decoder does with a duplicate, it must be something stable -- a scene
    /// that decodes differently depending on how many copies of an id it
    /// already holds makes every later delta wrong too.
    #[test]
    fn a_duplicate_view_id_does_not_desynchronise_the_stream() {
        let shared = ViewId::new();
        let base = scene(vec![window(shared, "first"), window(shared, "second")]);

        let mut next = base.clone();
        next.windows[0] = window(shared, "updated");

        // Through the real encoder and decoder, which is the path a viewer
        // is on.
        let mut encoder = SceneEncoder::new(0);
        let mut decoder = SceneDecoder::new();
        decoder.apply(&encoder.encode(&base)).unwrap();
        let applied = decoder.apply(&encoder.encode(&next)).unwrap().clone();

        // Whatever the rule is, applying the encoder's own delta to the
        // encoder's own base has to reproduce the scene the encoder was
        // describing. Otherwise the viewer and the producer disagree from
        // here on, and no later delta can repair it.
        assert_eq!(
            applied.windows.len(),
            next.windows.len(),
            "the window count changed: base {:?}, applied {:?}",
            next.windows
                .iter()
                .map(|w| &w.frame.content)
                .collect::<Vec<_>>(),
            applied
                .windows
                .iter()
                .map(|w| &w.frame.content)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            applied
                .windows
                .iter()
                .map(|w| &w.frame.content)
                .collect::<Vec<_>>(),
            next.windows
                .iter()
                .map(|w| &w.frame.content)
                .collect::<Vec<_>>(),
            "the decoded scene is not the scene the delta described"
        );
    }
}
