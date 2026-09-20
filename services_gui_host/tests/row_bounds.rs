//! `draw_window` must cost what the canvas costs, not what the window claims.
//!
//! `SurfaceRect` is four `usize`s and `DesktopWindow` derives `Deserialize`.
//! A `DesktopScene` travels over the wire inside a keyframe, so a viewer
//! rendering a decoded scene walks whatever the producer put in the height.

use services_gui_host::{Compositor, DesktopWindow, SurfaceRect, SurfaceSize};
use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

fn window(rect: SurfaceRect) -> DesktopWindow {
    let frame = ViewFrame::new(
        ViewId::new(),
        ViewKind::TextBuffer,
        1,
        ViewContent::text_buffer(vec!["x".to_string()]),
        0,
    );
    DesktopWindow::new(frame, rect)
}

#[test]
fn compose_desktop_cost_is_bounded_by_the_canvas() {
    let compositor = Compositor::new();
    for rect in [
        SurfaceRect::new(0, 0, 2, 200_000_000),
        SurfaceRect::new(0, 0, 200_000_000, 2),
        SurfaceRect::new(0, 0, usize::MAX, usize::MAX),
    ] {
        let started = std::time::Instant::now();
        let _ = compositor.compose_desktop(SurfaceSize::new(80, 24), vec![window(rect)]);
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "draw_window walked {rect:?} instead of the canvas: {elapsed:?}"
        );
    }
}

/// The clamp is exact: a window that fits is drawn exactly as before.
#[test]
fn a_window_that_fits_is_drawn_unchanged() {
    let compositor = Compositor::new();
    let frame = compositor.compose_desktop(
        SurfaceSize::new(40, 12),
        vec![window(SurfaceRect::new(2, 1, 20, 8))],
    );
    let rendered = frame.rows.join("\n");
    assert!(
        rendered.contains('+'),
        "the window's border is missing: {rendered}"
    );
    let border_rows = frame.rows.iter().filter(|row| row.contains('+')).count();
    assert!(
        border_rows >= 2,
        "a window 8 rows tall must have a top and a bottom border, got {border_rows}"
    );
}
