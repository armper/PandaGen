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

use crate::{window_pixel_rect, DesktopCursor, DesktopScene, DesktopWindow, SurfaceSize, Theme};

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
                damage = union(damage, window_pixel_rect(old.rect));
                damage = union(damage, window_pixel_rect(window.rect));
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
            }
        };
        let update = if need_keyframe {
            self.since_keyframe = 0;
            SceneUpdate::Keyframe(scene.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DesktopWindowRole, SurfaceRect, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH};
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
        // Damage covers the side window, the note, main, and both cursors.
        assert_eq!(damage.x, 0);
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
}
