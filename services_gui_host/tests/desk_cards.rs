//! The desk look (GFX-050): cards, the dock and the top bar, checked on
//! pixels and on hit regions rather than by eye.

use graphics_rasterizer::{RasterRect, RgbaBuffer, RgbaColor};
use services_gui_host::{
    Compositor, DesktopTab, DesktopWindow, HitRegion, Theme, WindowStyle, CARD_HEADER_HEIGHT,
    CARD_LINE_HEIGHT, CARD_PADDING, CARD_RADIUS, DOCK_TILE,
};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

const W: usize = 640;
const H: usize = 400;

fn frame(title: &str, lines: &[&str]) -> ViewFrame {
    let mut frame = ViewFrame::new(
        ViewId::new(),
        ViewKind::TextBuffer,
        1,
        ViewContent::text_buffer(lines.iter().map(|l| l.to_string()).collect()),
        0,
    );
    frame.title = Some(title.to_string());
    frame
}

fn render(windows: Vec<DesktopWindow>) -> RgbaBuffer {
    let compositor = Compositor::new();
    let mut target = RgbaBuffer::new(W, H, RgbaColor::new(0, 0, 0, 255));
    compositor.render_desktop_to_target(&mut target, windows);
    target
}

fn px(target: &RgbaBuffer, x: usize, y: usize) -> RgbaColor {
    target.pixel(x, y).expect("inside the surface")
}

#[test]
fn a_card_has_rounded_corners_a_ring_and_a_header() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let card = DesktopWindow::card(frame("Notepad", &["hello"]), bounds).focused();
    let target = render(vec![card]);

    // The very corner is outside the rounded shape: background shows.
    assert_eq!(
        px(&target, 100, 80),
        theme.background,
        "corner is not rounded"
    );
    // Along the top edge, past the radius, the focus ring is the accent.
    assert_eq!(
        px(&target, 100 + CARD_RADIUS + 4, 80),
        theme.accent,
        "no focus ring"
    );
    // Inside the header, away from any text, the surface shows (no flooded
    // title bar).
    assert_eq!(
        px(&target, 300, 80 + CARD_HEADER_HEIGHT / 2),
        theme.surface,
        "the header is colour-flooded"
    );
    // The header separator is a hairline.
    assert_eq!(
        px(&target, 300, 80 + CARD_HEADER_HEIGHT),
        theme.hairline,
        "no header separator"
    );
    // The shadow lifts the card: two pixels beyond the bottom-right, inside
    // the offset shape, is the shadow colour.
    assert_eq!(
        px(&target, 100 + 200, 80 + 240 + 1),
        theme.shadow,
        "no lift under the card"
    );
}

#[test]
fn an_unfocused_card_has_a_hairline_ring_not_the_accent() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(40, 40, 300, 200);
    let target = render(vec![DesktopWindow::card(frame("Notepad", &[]), bounds)]);
    assert_eq!(px(&target, 40 + CARD_RADIUS + 4, 40), theme.hairline);
    assert_ne!(px(&target, 40 + CARD_RADIUS + 4, 40), theme.accent);
}

#[test]
fn card_hit_regions_name_the_header_the_close_glyph_and_the_line() {
    let bounds = RasterRect::new(100, 80, 400, 240);
    let card = DesktopWindow::card(frame("Notepad", &["one", "two", "three"]), bounds);
    let compositor = Compositor::new();
    let windows = vec![card.clone()];

    let header = compositor
        .hit_test(&windows, 200, 80 + CARD_HEADER_HEIGHT / 2)
        .expect("over the card");
    assert_eq!(header.region, HitRegion::Header);

    let close = card.close_rect().expect("a closable card has a close rect");
    let hit = compositor
        .hit_test(
            &windows,
            close.x + close.width / 2,
            close.y + close.height / 2,
        )
        .expect("over the close glyph");
    assert_eq!(hit.region, HitRegion::Close);

    // The third line, at the card's 20px pitch rather than the classic 18.
    let (ox, oy, pitch) = card.card_text_origin();
    assert_eq!(pitch, CARD_LINE_HEIGHT);
    let hit = compositor
        .hit_test(&windows, ox + 8 * 2 + 1, oy + pitch * 2 + 3)
        .expect("over the content");
    assert_eq!(
        hit.region,
        HitRegion::Content { line: 2, column: 2 },
        "content hit did not map through the card's pitch and padding"
    );

    // Outside the card is the desk.
    assert!(compositor.hit_test(&windows, 10, 10).is_none());
}

#[test]
fn pixel_bounds_win_over_the_cell_rect() {
    let bounds = RasterRect::new(123, 77, 210, 150);
    let card = DesktopWindow::card(frame("A", &[]), bounds);
    assert_eq!(card.bounds(), bounds);
    let compositor = Compositor::new();
    let windows = vec![card];
    assert!(compositor.hit_test(&windows, 124, 78).is_some());
    assert!(compositor.hit_test(&windows, 122, 78).is_none());
    assert!(compositor.hit_test(&windows, 123 + 210, 78).is_none());
}

#[test]
fn the_caret_and_a_footer_are_drawn_where_the_geometry_says() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let mut frame = frame("Notepad", &["ab", "cd"]);
    frame.cursor = Some(CursorPosition::new(1, 2));
    let card = DesktopWindow::card(frame, bounds).with_footer(Some("Ln 2, Col 3".to_string()));
    let (ox, oy, pitch) = card.card_text_origin();
    let target = render(vec![card.clone()]);

    // Caret: a 2px accent bar at column 2 of line 1.
    assert_eq!(
        px(&target, ox + 8 * 2, oy + pitch + 4),
        theme.accent,
        "no caret"
    );
    // Footer hairline sits 20px above the bottom edge.
    let footer_top = bounds.bottom() - 20;
    assert_eq!(
        px(&target, 300, footer_top),
        theme.hairline,
        "no footer separator"
    );
    // And the content row count leaves room for it.
    assert_eq!(
        card.content_rows(),
        (240 - CARD_HEADER_HEIGHT - CARD_PADDING * 2 - 20) / CARD_LINE_HEIGHT
    );
}

#[test]
fn the_dock_paints_a_tile_per_app_and_names_the_tile_that_was_hit() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(200, 340, 240, 56);
    let dock = DesktopWindow::new(
        frame("", &[]),
        services_gui_host::SurfaceRect::new(0, 0, 0, 0),
    )
    .with_style(WindowStyle::Dock)
    .with_pixel_rect(bounds)
    .with_tabs(vec![
        DesktopTab::new("Np", true),
        DesktopTab::new("Fi", false),
        DesktopTab::new("Tm", false),
    ]);
    let target = render(vec![dock.clone()]);

    // The pill is the raised surface.
    assert_eq!(px(&target, 204, 368), theme.surface_raised);

    let compositor = Compositor::new();
    let windows = vec![dock];
    // Three tiles, centred: total row = 3*40 + 2*8 = 136; starts at 200 + (240-136)/2 = 252.
    let first_x = 252 + DOCK_TILE / 2;
    let y = 340 + 56 / 2;
    let hit = compositor.hit_test(&windows, first_x, y).unwrap();
    assert_eq!(hit.region, HitRegion::DockTile { index: 0 });
    let hit = compositor.hit_test(&windows, first_x + 48 * 2, y).unwrap();
    assert_eq!(hit.region, HitRegion::DockTile { index: 2 });
    // Between tiles is the pill, not a tile.
    let hit = compositor
        .hit_test(&windows, 252 + DOCK_TILE + 4, y)
        .unwrap();
    assert_eq!(hit.region, HitRegion::Border);
    // A tile is drawn as the inactive tab colour, and the running dot under
    // the first one is the accent.
    assert_eq!(px(&target, first_x, y), theme.tab_inactive);
    assert_eq!(
        px(&target, first_x, 340 + (56 - DOCK_TILE) / 2 + DOCK_TILE + 3),
        theme.accent
    );
}

#[test]
fn the_top_bar_is_a_raised_strip_with_a_hairline() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(0, 0, W, 28);
    let bar = DesktopWindow::new(
        frame("Notepad", &["t=12"]),
        services_gui_host::SurfaceRect::new(0, 0, 0, 0),
    )
    .with_style(WindowStyle::TopBar)
    .with_pixel_rect(bounds);
    let target = render(vec![bar]);
    assert_eq!(px(&target, 320, 4), theme.surface_raised);
    assert_eq!(px(&target, 320, 27), theme.hairline);
}

/// A classic window is untouched by any of this: same pixels as before.
#[test]
fn classic_windows_still_paint_the_classic_way() {
    let theme = Theme::DEFAULT;
    let window = DesktopWindow::new(
        frame("Workspace", &["hi"]),
        services_gui_host::SurfaceRect::new(2, 2, 20, 8),
    );
    assert_eq!(window.style, WindowStyle::Classic);
    let target = render(vec![window]);
    // Classic border is a hard 1px frame in the unfocused border colour at
    // the top-left corner -- no rounding.
    assert_eq!(px(&target, 16, 36), theme.border_unfocused);
}
