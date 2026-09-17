//! Property tests for compositor invariants (GFX-046).
//!
//! Dependency-free: a small xorshift generator drives random scenes so the
//! tests stay deterministic and fast. Each property runs a few hundred
//! random cases.

use graphics_rasterizer::{RasterRect, RenderTarget, RgbaBuffer};
use services_gui_host::{
    diff_scenes, Compositor, DesktopCursor, DesktopScene, DesktopWindow, DesktopWindowRole,
    SurfaceRect, SurfaceSize, Theme, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH,
};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

const ROLES: [DesktopWindowRole; 6] = [
    DesktopWindowRole::Main,
    DesktopWindowRole::Status,
    DesktopWindowRole::Overlay,
    DesktopWindowRole::Palette,
    DesktopWindowRole::Notification,
    DesktopWindowRole::Modal,
];

fn random_window(rng: &mut Rng, size: SurfaceSize, id: ViewId) -> DesktopWindow {
    let w = 2 + rng.below(size.width.saturating_sub(2));
    let h = 1 + rng.below(size.height.saturating_sub(1));
    let x = rng.below(size.width.saturating_sub(w) + 1);
    let y = rng.below(size.height.saturating_sub(h) + 1);
    let lines: Vec<String> = (0..rng.below(6))
        .map(|i| format!("line {i} {}", rng.below(1000)))
        .collect();
    let mut frame = ViewFrame::new(
        id,
        ViewKind::TextBuffer,
        1,
        ViewContent::text_buffer(lines),
        0,
    )
    .with_title(format!("W{}", rng.below(100)));
    if rng.chance(50) {
        frame = frame.with_cursor(CursorPosition::new(rng.below(5), rng.below(10)));
    }
    let mut window = DesktopWindow::new(frame, SurfaceRect::new(x, y, w, h))
        .with_role(ROLES[rng.below(ROLES.len())])
        .with_z_index(rng.below(4))
        .with_highlight(if rng.chance(30) {
            Some(rng.below(4))
        } else {
            None
        });
    if rng.chance(30) {
        window = window.without_chrome();
    }
    if rng.chance(30) {
        window = window.focused();
    }
    window
}

fn random_scene(rng: &mut Rng, ids: &[ViewId]) -> DesktopScene {
    let size = SurfaceSize::new(8 + rng.below(20), 4 + rng.below(12));
    let count = rng.below(ids.len() + 1);
    let windows = ids[..count]
        .iter()
        .map(|id| random_window(rng, size, *id))
        .collect();
    let (pw, ph) = (
        size.width * RASTER_CELL_WIDTH,
        size.height * RASTER_CELL_HEIGHT,
    );
    let cursor = if rng.chance(70) {
        Some(DesktopCursor::new(rng.below(pw + 10), rng.below(ph + 10)))
    } else {
        None
    };
    DesktopScene::new(size, windows).with_cursor(cursor)
}

fn shuffle<T>(rng: &mut Rng, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.below(i + 1);
        items.swap(i, j);
    }
}

fn pixel_rect(rect: SurfaceRect) -> RasterRect {
    RasterRect::new(
        rect.x * RASTER_CELL_WIDTH,
        rect.y * RASTER_CELL_HEIGHT,
        rect.width * RASTER_CELL_WIDTH,
        rect.height * RASTER_CELL_HEIGHT,
    )
}

#[test]
fn property_render_is_independent_of_window_list_order() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let ids: Vec<ViewId> = (0..6).map(|_| ViewId::new()).collect();
    let compositor = Compositor::new();
    for _ in 0..200 {
        let scene = random_scene(&mut rng, &ids);
        let reference = compositor.render_scene_rgba(&scene);
        let mut shuffled = scene.clone();
        shuffle(&mut rng, &mut shuffled.windows);
        let again = compositor.render_scene_rgba(&shuffled);
        assert_eq!(reference.pixels, again.pixels);
    }
}

#[test]
fn property_pixels_outside_every_window_and_cursor_are_background() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let ids: Vec<ViewId> = (0..5).map(|_| ViewId::new()).collect();
    let compositor = Compositor::new();
    for _ in 0..150 {
        let scene = random_scene(&mut rng, &ids);
        let surface = compositor.render_scene_rgba(&scene);
        let cursor_bounds = scene.cursor.map(|c| c.bounds());
        for _ in 0..200 {
            let x = rng.below(surface.width);
            let y = rng.below(surface.height);
            let covered = scene
                .windows
                .iter()
                .any(|w| pixel_rect(w.rect).contains(x, y))
                || cursor_bounds.is_some_and(|b| b.contains(x, y));
            if !covered {
                assert_eq!(
                    surface.pixel(x, y),
                    Some(Theme::DEFAULT.background),
                    "({x},{y}) in {:?}",
                    scene.size
                );
            }
        }
    }
}

#[test]
fn property_hit_test_reports_the_topmost_window_in_paint_order() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let ids: Vec<ViewId> = (0..6).map(|_| ViewId::new()).collect();
    let compositor = Compositor::new();
    for _ in 0..200 {
        let scene = random_scene(&mut rng, &ids);
        let order = services_gui_host::composition_order(&scene.windows);
        let (pw, ph) = scene.pixel_size();
        for _ in 0..50 {
            let x = rng.below(pw);
            let y = rng.below(ph);
            let expected = order
                .iter()
                .rev()
                .find(|&&i| pixel_rect(scene.windows[i].rect).contains(x, y))
                .copied();
            let hit = compositor
                .hit_test(&scene.windows, x, y)
                .map(|h| h.window_index);
            assert_eq!(hit, expected);
        }
    }
}

#[test]
fn property_damage_repaint_matches_full_render_inside_the_damage_rect() {
    let mut rng = Rng(0x0F0F_1234_ABCD_9999);
    let ids: Vec<ViewId> = (0..5).map(|_| ViewId::new()).collect();
    let compositor = Compositor::new();
    for _ in 0..150 {
        let old = random_scene(&mut rng, &ids);
        let mut new = random_scene(&mut rng, &ids);
        new.size = old.size;
        let (pw, ph) = new.pixel_size();
        let full = compositor.render_scene_rgba(&new);

        // Start from the old frame, repaint an arbitrary rectangle of the new one.
        let mut buffer = RgbaBuffer::new(pw, ph, Theme::DEFAULT.background);
        compositor.render_scene_full(&mut buffer, &old);
        let dw = 1 + rng.below(pw);
        let dh = 1 + rng.below(ph);
        let damage = RasterRect::new(rng.below(pw), rng.below(ph), dw, dh);
        let mut damaged = new.clone();
        damaged.damage = Some(damage);
        compositor.render_scene(&mut buffer, &damaged);

        for y in damage.y..(damage.y + damage.height).min(ph) {
            for x in damage.x..(damage.x + damage.width).min(pw) {
                // The cursor is always painted in full, so it may differ
                // outside the rect but never inside it.
                assert_eq!(buffer.pixel(x, y), full.pixel(x, y), "({x},{y}) {damage:?}");
            }
        }
    }
}

#[test]
fn property_delta_damage_is_sufficient_for_incremental_repaint() {
    let mut rng = Rng(0x5555_AAAA_3333_CCCC);
    let ids: Vec<ViewId> = (0..5).map(|_| ViewId::new()).collect();
    let compositor = Compositor::new();
    for _ in 0..150 {
        let old = random_scene(&mut rng, &ids);
        let mut new = random_scene(&mut rng, &ids);
        new.size = old.size;
        // Sometimes only nudge the old scene, to exercise caret-only deltas.
        if rng.chance(40) {
            new = old.clone();
            if let Some(w) = new.windows.first_mut() {
                w.frame.cursor = Some(CursorPosition::new(rng.below(3), rng.below(6)));
            }
            if rng.chance(50) {
                new.cursor = Some(DesktopCursor::new(rng.below(50), rng.below(50)));
            }
        }
        let delta = diff_scenes(&old, &new);
        let mut applied = services_gui_host::apply_delta(&old, &delta);
        let (pw, ph) = new.pixel_size();
        let mut buffer = RgbaBuffer::new(pw, ph, Theme::DEFAULT.background);
        compositor.render_scene_full(&mut buffer, &old);
        compositor.render_scene(&mut buffer, &applied);
        let full = compositor.render_scene_rgba(&new);
        if buffer.as_bytes() != full.pixels.as_slice() {
            let mut first = None;
            'outer: for y in 0..ph {
                for x in 0..pw {
                    if buffer.pixel(x, y) != full.pixel(x, y) {
                        first = Some((x, y, buffer.pixel(x, y), full.pixel(x, y)));
                        break 'outer;
                    }
                }
            }
            let inside = first.and_then(|(x, y, _, _)| delta.damage.map(|d| d.contains(x, y)));
            let dump: Vec<String> = old
                .windows
                .iter()
                .map(|w| {
                    format!(
                        "id={:?} rect={:?} role={:?} z={} focused={} chrome={} cursor={:?} lines={} hl={:?}",
                        w.frame.view_id, w.rect, w.role, w.z_index, w.focused, w.chrome, w.frame.cursor,
                        match &w.frame.content { ViewContent::TextBuffer { lines } => lines.len(), _ => 0 },
                        w.highlight_line
                    )
                })
                .collect();
            panic!(
                "first diff {first:?} inside_damage={inside:?} size={:?} old_cursor={:?} new_cursor={:?}\nold windows:\n{}\ndelta={delta:?}",
                old.size,
                old.cursor,
                new.cursor,
                dump.join("\n")
            );
        }
        applied.damage = None;
        assert_eq!(applied.cursor, new.cursor);
    }
}
