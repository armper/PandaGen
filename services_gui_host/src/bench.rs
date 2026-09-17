//! Compositor work benchmarks (GFX-045).
//!
//! These scenarios measure *work*, not time: pixels written, windows
//! painted, and damage area for representative desktops. Work counts are
//! deterministic, so they run under `cargo test` as regression bounds; the
//! `compositor_bench` example wraps the same scenarios in wall-clock timing
//! for humans.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use graphics_rasterizer::{CountingTarget, RasterRect, RgbaBuffer};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

use crate::{
    diff_scenes, Compositor, DesktopCursor, DesktopScene, DesktopWindow, DesktopWindowRole,
    SurfaceRect, SurfaceSize, Theme, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH,
};

/// Result of one scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchReport {
    pub name: String,
    /// Pixels written by the compositor for this frame.
    pub pixels_written: u64,
    pub windows_painted: usize,
    /// Damage area repainted (surface area for a full compose).
    pub damage_area: usize,
    pub surface_area: usize,
}

impl BenchReport {
    /// Fraction of the surface repainted, in tenths of a percent.
    pub fn repaint_permille(&self) -> u64 {
        if self.surface_area == 0 {
            0
        } else {
            self.pixels_written * 1000 / self.surface_area as u64
        }
    }
}

/// Surface used by every scenario: a 1280x800 desktop in cells.
pub const BENCH_SIZE: SurfaceSize = SurfaceSize::new_const(160, 44);

impl SurfaceSize {
    pub const fn new_const(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

fn text_window(id: ViewId, title: &str, rect: SurfaceRect, lines: usize) -> DesktopWindow {
    let content: Vec<String> = (0..lines)
        .map(|i| alloc::format!("{title} line {i} with some text to rasterize"))
        .collect();
    DesktopWindow::new(
        ViewFrame::new(
            id,
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(content),
            0,
        )
        .with_title(title)
        .with_cursor(CursorPosition::new(0, 3)),
        rect,
    )
}

/// `count` overlapping windows cascaded across the surface.
pub fn many_windows_scene(count: usize) -> DesktopScene {
    let windows = (0..count)
        .map(|i| {
            let x = (i * 3) % BENCH_SIZE.width.saturating_sub(40);
            let y = (i * 2) % BENCH_SIZE.height.saturating_sub(12);
            text_window(
                ViewId::new(),
                &alloc::format!("W{i}"),
                SurfaceRect::new(x, y, 40, 12),
                10,
            )
            .with_z_index(i)
        })
        .collect();
    DesktopScene::new(BENCH_SIZE, windows).with_cursor(Some(DesktopCursor::new(600, 400)))
}

/// A `cols` x `rows` grid of non-overlapping tiles filling the surface.
pub fn tile_grid_scene(cols: usize, rows: usize) -> DesktopScene {
    let tile_w = BENCH_SIZE.width / cols.max(1);
    let tile_h = BENCH_SIZE.height / rows.max(1);
    let mut windows = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            let mut window = text_window(
                ViewId::new(),
                &alloc::format!("T{r}{c}"),
                SurfaceRect::new(c * tile_w, r * tile_h, tile_w, tile_h),
                tile_h.saturating_sub(2),
            );
            window.focused = r == 0 && c == 0;
            windows.push(window);
        }
    }
    DesktopScene::new(BENCH_SIZE, windows)
}

fn measure(
    name: &str,
    compositor: &Compositor,
    scene: &DesktopScene,
    damage: Option<RasterRect>,
) -> BenchReport {
    let (w, h) = scene.pixel_size();
    let mut buffer = RgbaBuffer::new(w, h, Theme::DEFAULT.background);
    let mut counting = CountingTarget::new(&mut buffer);
    let stats = compositor.render_desktop_to_target_with_cursor(
        &mut counting,
        scene.windows.clone(),
        damage,
        scene.cursor,
    );
    BenchReport {
        name: name.to_string(),
        pixels_written: counting.writes(),
        windows_painted: stats.painted_windows,
        damage_area: damage.map(|d| d.width * d.height).unwrap_or(w * h),
        surface_area: w * h,
    }
}

/// Run every scenario once and return the reports.
pub fn run_all() -> Vec<BenchReport> {
    let compositor = Compositor::new();
    let mut reports = Vec::new();

    reports.push(measure(
        "full: 8 windows",
        &compositor,
        &many_windows_scene(8),
        None,
    ));
    reports.push(measure(
        "full: 64 windows",
        &compositor,
        &many_windows_scene(64),
        None,
    ));
    reports.push(measure(
        "full: 8x6 tile grid",
        &compositor,
        &tile_grid_scene(8, 6),
        None,
    ));

    // Cursor move: repaint only the union of old and new cursor bounds.
    let before = many_windows_scene(8);
    let mut after = before.clone();
    after.cursor = Some(DesktopCursor::new(612, 409));
    let delta = diff_scenes(&before, &after);
    reports.push(measure(
        "cursor move (damage)",
        &compositor,
        &after,
        delta.damage,
    ));

    // Caret blink: one window's cursor toggles.
    let mut blink = before.clone();
    blink.windows[0].frame.cursor = None;
    let delta = diff_scenes(&before, &blink);
    reports.push(measure(
        "caret blink (damage)",
        &compositor,
        &blink,
        delta.damage,
    ));

    // Overlay toggle: palette appears over the desktop.
    let mut with_palette = before.clone();
    with_palette.windows.push(
        DesktopWindow::new(
            ViewFrame::new(
                ViewId::new(),
                ViewKind::Panel,
                1,
                ViewContent::text_buffer((0..10).map(|i| alloc::format!("command {i}")).collect()),
                0,
            )
            .with_title("Commands"),
            SurfaceRect::new(40, 11, 80, 22),
        )
        .with_role(DesktopWindowRole::Palette)
        .focused(),
    );
    let delta = diff_scenes(&before, &with_palette);
    reports.push(measure(
        "palette open (damage)",
        &compositor,
        &with_palette,
        delta.damage,
    ));

    reports
}

/// Cell size in pixels, for callers converting work to cells.
pub const fn cell_area() -> usize {
    RASTER_CELL_WIDTH * RASTER_CELL_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenarios_report_bounded_work() {
        let reports = run_all();
        let by_name = |name: &str| {
            reports
                .iter()
                .find(|r| r.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        let full8 = by_name("full: 8 windows");
        let full64 = by_name("full: 64 windows");
        let grid = by_name("full: 8x6 tile grid");
        let cursor = by_name("cursor move (damage)");
        let blink = by_name("caret blink (damage)");
        let palette = by_name("palette open (damage)");

        // Full composes paint every window and at least the whole surface once.
        assert_eq!(full8.windows_painted, 8);
        assert_eq!(full64.windows_painted, 64);
        assert_eq!(grid.windows_painted, 48);
        assert!(full8.pixels_written >= full8.surface_area as u64);
        // Overdraw from 64 cascaded windows stays under 8x the surface.
        assert!(
            full64.pixels_written < 8 * full64.surface_area as u64,
            "{full64:?}"
        );

        // Damage-limited frames touch a tiny fraction of the surface.
        assert!(cursor.repaint_permille() < 10, "{cursor:?}");
        assert!(blink.repaint_permille() < 10, "{blink:?}");
        assert!(cursor.windows_painted <= 2);
        // The palette repaints its own area plus the overdraw of windows
        // beneath it, well under the full desktop.
        assert!(palette.repaint_permille() < 800, "{palette:?}");
        assert!(palette.damage_area < palette.surface_area / 2);

        // Work is deterministic across runs.
        assert_eq!(run_all(), reports);
    }
}
