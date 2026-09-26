//! The desk look (GFX-050): cards, the dock and the top bar, checked on
//! pixels and on hit regions rather than by eye.

use graphics_rasterizer::{RasterRect, RgbaBuffer, RgbaColor};
use services_gui_host::{
    Compositor, DesktopTab, DesktopWindow, HitRegion, LineStyle, Theme, Wallpaper, WindowStyle,
    CARD_HEADER_HEIGHT, CARD_LINE_HEIGHT, CARD_PADDING, CARD_RADIUS, DOCK_TILE,
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

/// Within `slack` of `want` in every channel: glass (GFX-098) is its own
/// colour laid over what is under it, so it is near, not equal.
fn near(got: RgbaColor, want: RgbaColor, slack: u8) -> bool {
    got.r.abs_diff(want.r) <= slack
        && got.g.abs_diff(want.g) <= slack
        && got.b.abs_diff(want.b) <= slack
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
    // The shadow lifts the card (GFX-099): just under it the background
    // is darkened toward the shadow colour, and it fades with distance.
    let bg = px(&target, 20, 20);
    let near_edge = px(&target, 100 + 100, 80 + 240 + 2);
    let far = px(&target, 100 + 100, 80 + 240 + 17);
    assert!(
        near_edge.r < bg.r || near_edge.g < bg.g || near_edge.b < bg.b,
        "no shadow: {near_edge:?} on {bg:?}"
    );
    assert!(
        near_edge.b <= far.b,
        "the shadow does not fade: {near_edge:?} then {far:?}"
    );
    assert_eq!(
        px(&target, 100 + 100, 80 + 240 + 40),
        bg,
        "the shadow spreads too far"
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

/// The close cross is drawn (GFX-104): solid on its diagonals, soft beside
/// them, and nothing between its arms or past their ends.
#[test]
fn the_close_cross_is_two_smooth_diagonals() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let card = DesktopWindow::card(frame("Notepad", &["one"]), bounds);
    let close = card.close_rect().unwrap();
    let target = render(vec![card]);
    let at = |dx: usize, dy: usize| px(&target, close.x + dx, close.y + dy);
    let header = at(1, 1);
    assert_ne!(header, theme.text_muted);
    // The centre and an arm's end are solid.
    assert_eq!(at(8, 8), theme.text_muted);
    assert_eq!(at(3, 3), theme.text_muted);
    assert_eq!(at(12, 3), theme.text_muted);
    // Beside a diagonal: partly inked.
    let soft = at(5, 4);
    assert!(soft != theme.text_muted && soft != header, "{soft:?}");
    // Between the arms and past their ends: the header.
    assert_eq!(at(8, 3), header);
    assert_eq!(at(2, 2), header);
}

/// The palette's header is a search field (GFX-113): a magnifier where
/// another card's title starts, and the query after it.
#[test]
fn the_palettes_header_has_a_magnifier() {
    use services_gui_host::{DesktopWindowRole, CARD_PADDING, SEARCH_GLYPH_W};
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let palette =
        DesktopWindow::card(frame("memo_", &["one"]), bounds).with_role(DesktopWindowRole::Palette);
    let plain = DesktopWindow::card(frame("memo_", &["one"]), bounds);
    let with = render(vec![palette]);
    let without = render(vec![plain]);
    let x = 100 + CARD_PADDING + 4;
    let title_y = 80 + (CARD_HEADER_HEIGHT - 16) / 2;
    // The ring's left edge, halfway down it.
    assert_eq!(px(&with, x, title_y + 2 + 5), theme.text_muted);
    assert_ne!(px(&without, x, title_y + 2 + 5), theme.text_muted);
    // The query starts after the magnifier: where the plain title's first
    // letter is, the palette has nothing of it.
    let inked = |t: &RgbaBuffer, x0: usize| {
        (x0..x0 + 8).any(|x| (title_y..title_y + 16).any(|y| px(t, x, y) != px(t, 300, y)))
    };
    // (The label starts with a space: its first letter is a cell in.)
    assert!(inked(&without, x + 8));
    assert!(inked(&with, x + SEARCH_GLYPH_W + 8));
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

/// A styled line takes its tone's colour, a bold one strikes twice, an
/// underlined one has a rule under it; the grid does not move (GFX-073).
#[test]
fn line_styles_change_tone_weight_and_rule_but_not_the_grid() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let card = DesktopWindow::card(frame("Notepad", &["# Title", "plain", "> aside"]), bounds)
        .focused()
        .with_line_styles(vec![(0, LineStyle::HEADING), (2, LineStyle::QUIET)]);
    let (ox, oy, pitch) = card.card_text_origin();
    let target = render(vec![card]);
    let glyph_h = 16;
    let text_y = oy + (pitch - glyph_h) / 2;
    // Somewhere in "# Title" there is an accent pixel and no plain-text pixel.
    let row0: Vec<RgbaColor> = (ox..ox + 7 * 8)
        .map(|x| px(&target, x, text_y + 8))
        .collect();
    assert!(
        row0.contains(&theme.accent),
        "heading is not accent-coloured"
    );
    assert!(!row0.contains(&theme.text), "heading has plain-text pixels");
    // The rule: an accent line right under the glyphs, across the text.
    assert_eq!(
        px(&target, ox + 3, text_y + glyph_h + 1),
        theme.accent,
        "no underline"
    );
    assert_eq!(
        px(&target, ox + 7 * 8 + 4, text_y + glyph_h + 1),
        theme.surface,
        "underline too long"
    );
    // Bold: the glyph column one pixel right of a stem is also lit. 'l' in
    // "Title" (index 3) has a vertical stem; find a lit pixel and check
    // its right neighbour.
    let stem_x = (ox + 3 * 8..ox + 4 * 8)
        .find(|x| px(&target, *x, text_y + 8) == theme.accent)
        .expect("a lit pixel in 'l'");
    assert_eq!(
        px(&target, stem_x + 1, text_y + 8),
        theme.accent,
        "not bold"
    );
    // Line 1 is plain text on the grid's next row; line 2 is muted.
    let row1: Vec<RgbaColor> = (ox..ox + 5 * 8)
        .map(|x| px(&target, x, text_y + pitch + 8))
        .collect();
    assert!(row1.contains(&theme.text));
    let row2: Vec<RgbaColor> = (ox..ox + 7 * 8)
        .map(|x| px(&target, x, text_y + 2 * pitch + 8))
        .collect();
    assert!(row2.contains(&theme.text_muted) && !row2.contains(&theme.text));
}

/// A dock tile with an icon draws it, two pixels a bit, in place of the
/// monogram (GFX-084); a tile without one still shows its letters.
#[test]
fn a_dock_tile_draws_its_icon_in_place_of_the_monogram() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(200, 300, 240, 84);
    // A frame: the top row and the left column lit, nothing else.
    let mut icon = [0u16; 16];
    icon[0] = 0xFFFF;
    for row in icon.iter_mut().skip(1) {
        *row = 0x8000;
    }
    let dock = DesktopWindow::new(
        frame("", &[]),
        services_gui_host::SurfaceRect::new(0, 0, 0, 0),
    )
    .with_style(WindowStyle::Dock)
    .with_pixel_rect(bounds)
    .with_tabs(vec![
        DesktopTab::new("Np", false).with_icon(Some(icon)),
        DesktopTab::new("Fi", false),
    ]);
    let target = render(vec![dock]);
    // Two tiles (GFX-114): row 2*64 + 6 = 134 wide, starts at
    // 200 + (240-134)/2 = 253; in the 84px capsule the tiles sit at
    // y = 300 + (84-72)/2 = 306, room for the light under them.
    let (tx, ty) = (253, 306);
    let (ox, oy) = (tx + (DOCK_TILE - 32) / 2, ty + (DOCK_TILE - 32) / 2);
    assert_eq!(px(&target, ox, oy), theme.text, "top-left bit");
    assert_eq!(
        px(&target, ox + 31, oy + 1),
        theme.text,
        "top row, two pixels tall"
    );
    assert_eq!(px(&target, ox + 1, oy + 31), theme.text, "left column");
    assert_eq!(
        px(&target, ox + 16, oy + 16),
        theme.tab_inactive,
        "inside is the tile"
    );
    // The second tile has no icon: its monogram puts text pixels in the
    // middle of the tile.
    let middle: Vec<RgbaColor> = (tx + 70 + 20..tx + 70 + 44)
        .flat_map(|x| (ty + 20..ty + 44).map(move |y| (x, y)))
        .map(|(x, y)| px(&target, x, y))
        .collect();
    assert!(middle.contains(&theme.text));
}

/// A text card can carry an overlay (GFX-086): an icon drawn beside a
/// row, over the content, while the text is still there.
#[test]
fn a_text_card_draws_its_overlay_over_the_lines() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let mut bits = [0u16; 16];
    bits[0] = 0xFFFF;
    let card = DesktopWindow::card(frame("Files", &["   memo", "   other"]), bounds)
        .focused()
        .with_overlay(vec![view_types::DrawOp::Icon {
            x: 2,
            y: 2,
            scale: 1,
            bits,
            color: Some(view_types::Color::rgb(52, 211, 153)),
        }]);
    let (ox, oy, _) = card.card_text_origin();
    let target = render(vec![card]);
    // The icon's top row, sixteen pixels of accent, two in from the origin.
    for x in ox + 2..ox + 18 {
        assert_eq!(px(&target, x, oy + 2), theme.accent, "x={x}");
    }
    assert_eq!(px(&target, ox + 2, oy + 3), theme.surface, "one bit tall");
    // The text is still drawn after the indent.
    let row: Vec<RgbaColor> = (ox + 24..ox + 24 + 4 * 8)
        .map(|x| px(&target, x, oy + 10))
        .collect();
    assert!(row.contains(&theme.text));
}

/// Pictures (GFX-094): a dock tile with a picture is drawn as the picture,
/// blended by its alpha, and a card's overlay can draw one by number; a
/// theme without pictures draws nothing for either.
#[test]
fn pictures_are_blended_by_their_alpha_in_the_dock_and_on_cards() {
    // A 40x40 picture: opaque red, but its first column is half-clear.
    static RGBA: [u8; 40 * 40 * 4] = {
        let mut px = [0u8; 40 * 40 * 4];
        let mut i = 0;
        while i < 40 * 40 {
            px[i * 4] = 200;
            px[i * 4 + 3] = if i % 40 == 0 { 128 } else { 255 };
            i += 1;
        }
        px
    };
    static PICTURES: [services_gui_host::Picture; 1] = [services_gui_host::Picture {
        width: 40,
        height: 40,
        rgba: &RGBA,
    }];
    let theme = Theme::DEFAULT.with_pictures(&PICTURES);
    let bounds = RasterRect::new(200, 300, 240, 84);
    let dock = DesktopWindow::new(
        frame("", &[]),
        services_gui_host::SurfaceRect::new(0, 0, 0, 0),
    )
    .with_style(WindowStyle::Dock)
    .with_pixel_rect(bounds)
    .with_tabs(vec![DesktopTab::new("Np", false).with_picture(Some(0))]);
    let card = DesktopWindow::card(frame("Card", &[]), RasterRect::new(300, 60, 300, 200))
        .with_overlay(vec![view_types::DrawOp::Picture {
            x: 10,
            y: 10,
            id: 0,
        }]);
    let (ox, oy, _) = card.card_text_origin();
    let compositor = Compositor::with_theme(theme);
    let mut target = RgbaBuffer::new(W, H, RgbaColor::new(0, 0, 0, 255));
    compositor.render_desktop_to_target(&mut target, vec![dock.clone(), card.clone()]);
    // One tile, centred: the 64px tile at 288, 306; the 40px picture in
    // its middle at 300, 318.
    assert_eq!(px(&target, 320, 338), RgbaColor::new(200, 0, 0, 255));
    let edge = px(&target, 300, 338);
    assert!(edge.r > 90 && edge.r < 130, "half blended: {edge:?}");
    assert_eq!(
        px(&target, ox + 30, oy + 30),
        RgbaColor::new(200, 0, 0, 255)
    );
    // Without the table, neither draws: the tile is gone too, so the
    // pill shows through where the picture would be.
    let plain = render(vec![dock, card]);
    assert_ne!(px(&plain, 320, 338), RgbaColor::new(200, 0, 0, 255));
}

/// A veil (GFX-096) darkens what is under it and draws its overlay on
/// top; a card under it is still seen, dimmer.
#[test]
fn a_veil_darkens_what_is_under_it() {
    let theme = Theme::DEFAULT;
    let card =
        DesktopWindow::card(frame("Card", &["x"]), RasterRect::new(100, 80, 300, 200)).focused();
    let veil = DesktopWindow::new(
        frame("", &[]),
        services_gui_host::SurfaceRect::new(0, 0, 0, 0),
    )
    .with_style(WindowStyle::Veil)
    .with_pixel_rect(RasterRect::new(0, 0, W, H))
    .with_z_index(1_000)
    .with_overlay(vec![view_types::DrawOp::Fill {
        rect: view_types::PixelRect {
            x: 10,
            y: 10,
            width: 4,
            height: 4,
        },
        color: view_types::Color::rgb(255, 255, 255),
    }]);
    let bare = render(vec![card.clone()]);
    let veiled = render(vec![card, veil]);
    let (a, b) = (px(&bare, 250, 200), px(&veiled, 250, 200));
    assert_eq!(a, theme.surface);
    assert!(b.r < a.r && b.g < a.g && b.b < a.b, "{a:?} -> {b:?}");
    assert!(b.g > theme.shadow.g, "still seen through: {b:?}");
    assert_eq!(px(&veiled, 11, 11), RgbaColor::new(255, 255, 255, 255));
}

/// A wallpaper smaller than the surface is enlarged smoothly (GFX-097):
/// between a black and a white source pixel there is grey, not a step.
#[test]
fn a_small_wallpaper_is_enlarged_smoothly() {
    static PALETTE: [u8; 6] = [0, 0, 0, 255, 255, 255];
    static INDICES: [u8; 2] = [0, 1];
    let wall = services_gui_host::Wallpaper {
        width: 2,
        height: 1,
        palette: &PALETTE,
        indices: &INDICES,
    };
    let row: Vec<u8> = (0..8).map(|x| wall.sample(x, 0, 8, 4).r).collect();
    assert_eq!(row[0], 0, "{row:?}");
    assert_eq!(row[7], 255, "{row:?}");
    assert!(row[3] > 40 && row[3] < 215, "a middle grey: {row:?}");
    assert!(
        row.windows(2).all(|w| w[0] <= w[1]),
        "no steps back: {row:?}"
    );
    // At its own size a wallpaper is drawn as it is.
    assert_eq!(wall.sample(1, 0, 2, 1).r, 255);
}

/// A wallpaper is sampled to the surface, behind the cards, and a damage
/// repaint reads the same pixels (GFX-066).
#[test]
fn a_wallpaper_is_sampled_to_the_surface_behind_the_cards() {
    // A 2x2 picture: red, green / blue, white.
    static PALETTE: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    static INDICES: [u8; 4] = [0, 1, 2, 3];
    let picture = Wallpaper {
        width: 2,
        height: 2,
        palette: &PALETTE,
        indices: &INDICES,
    };
    let compositor = Compositor::new().with_wallpaper(Some(picture));
    let card = DesktopWindow::card(
        frame("Notepad", &["hi"]),
        RasterRect::new(100, 80, 200, 100),
    );
    let mut target = RgbaBuffer::new(W, H, RgbaColor::new(0, 0, 0, 255));
    compositor.render_desktop_to_target(&mut target, vec![card.clone()]);
    assert_eq!(px(&target, 1, 1), RgbaColor::new(255, 0, 0, 255));
    assert_eq!(px(&target, W - 1, 1), RgbaColor::new(0, 255, 0, 255));
    assert_eq!(px(&target, 1, H - 1), RgbaColor::new(0, 0, 255, 255));
    assert_eq!(
        px(&target, W - 1, H - 1),
        RgbaColor::new(255, 255, 255, 255)
    );
    // The card is on top of it.
    assert_eq!(px(&target, 200, 130), Theme::DEFAULT.surface);
    // A damage repaint of the bottom-right quarter shows the same pixels.
    compositor.render_desktop_to_target_with_damage(
        &mut target,
        vec![card],
        Some(RasterRect::new(W / 2, H / 2, W / 2, H / 2)),
    );
    assert_eq!(
        px(&target, W - 1, H - 1),
        RgbaColor::new(255, 255, 255, 255)
    );
    // (Well inside the white quadrant: a 2x2 picture enlarged is blended
    // across its middle, GFX-097.)
    assert_eq!(
        px(&target, W * 3 / 4 + 1, H * 3 / 4 + 1),
        RgbaColor::new(255, 255, 255, 255)
    );
    // The scene carries it too.
    let scene = services_gui_host::DesktopScene::new(
        services_gui_host::SurfaceSize {
            width: W / 8,
            height: H / 18,
        },
        vec![],
    )
    .with_wallpaper(Some(picture));
    let mut plain = Compositor::new();
    plain = plain.with_wallpaper(None);
    let mut target2 = RgbaBuffer::new(W, H, RgbaColor::new(0, 0, 0, 255));
    plain.render_scene_full(&mut target2, &scene);
    assert_eq!(px(&target2, 1, 1), RgbaColor::new(255, 0, 0, 255));
}

/// Header chips sit between the title and the close glyph, in order, and
/// answer as `HitRegion::Action` (GFX-056). A card too narrow for all of
/// them keeps the first.
#[test]
fn header_action_chips_are_laid_out_in_order_and_hit_by_index() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let card = DesktopWindow::card(frame("Files", &["a.txt"]), bounds)
        .focused()
        .with_actions(vec!["New".into(), "Open".into(), "Delete".into()]);
    let rects = card.action_rects();
    let placed: Vec<RasterRect> = rects.iter().flatten().copied().collect();
    assert_eq!(placed.len(), 3, "{rects:?}");
    assert!(
        placed[0].x < placed[1].x && placed[1].x < placed[2].x,
        "not in order"
    );
    let close = card.close_rect().unwrap();
    assert!(placed[2].right() < close.x, "chips overlap the close glyph");
    assert_eq!(placed[0].width, 3 * 8 + 12);

    let compositor = Compositor::new();
    let windows = vec![card.clone()];
    let hit = compositor
        .hit_test(&windows, placed[1].x + 2, placed[1].y + 2)
        .unwrap();
    assert_eq!(hit.region, HitRegion::Action { index: 1 });
    // Between chips is still the header.
    let hit = compositor
        .hit_test(&windows, placed[1].x - 3, placed[1].y + 2)
        .unwrap();
    assert_eq!(hit.region, HitRegion::Header);

    // Painted: the chip's fill inside, the surface just outside it.
    let target = render(windows);
    assert_eq!(
        px(&target, placed[0].x + 2, placed[0].y + 2),
        theme.surface_raised
    );
    assert_eq!(px(&target, placed[0].x - 2, placed[0].y + 2), theme.surface);

    // A narrow card keeps "New" and drops from the end.
    let narrow = DesktopWindow::card(frame("Files", &[]), RasterRect::new(0, 0, 200, 100))
        .with_actions(vec!["New".into(), "Open".into(), "Delete".into()]);
    let rects = narrow.action_rects();
    assert!(rects[0].is_some(), "{rects:?}");
    assert!(rects[2].is_none(), "{rects:?}");
}

/// Selected text sits on the selection fill, the rest on the surface, and
/// a span past a line's end marks its line break (GFX-054).
#[test]
fn selection_spans_paint_behind_the_selected_cells_only() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(100, 80, 400, 240);
    let card = DesktopWindow::card(frame("Notepad", &["hello world", "ab", ""]), bounds)
        .focused()
        .with_selection(vec![(0, 6, 12), (1, 0, 3), (2, 0, 1)]);
    let (origin_x, origin_y, pitch) = card.card_text_origin();
    let target = render(vec![card]);
    let cell = services_gui_host::RASTER_CELL_WIDTH;
    let mid = |line: usize| origin_y + line * pitch + 1;
    // Line 0: "hello " is on the surface, "world" and one cell beyond on the fill.
    assert_eq!(px(&target, origin_x + 2 * cell + 1, mid(0)), theme.surface);
    assert_eq!(
        px(&target, origin_x + 6 * cell + 1, mid(0)),
        theme.selection
    );
    assert_eq!(
        px(&target, origin_x + 11 * cell + 1, mid(0)),
        theme.selection
    );
    assert_eq!(px(&target, origin_x + 12 * cell + 1, mid(0)), theme.surface);
    // Line 1 and the empty line 2 carry their marks; line 1 past its span is surface.
    assert_eq!(px(&target, origin_x + 1, mid(1)), theme.selection);
    assert_eq!(px(&target, origin_x + 3 * cell + 1, mid(1)), theme.surface);
    assert_eq!(px(&target, origin_x + 1, mid(2)), theme.selection);
}

#[test]
fn the_dock_paints_a_tile_per_app_and_names_the_tile_that_was_hit() {
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(200, 300, 240, 84);
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

    // The capsule is glass (GFX-114): frosted, so lighter than the desk
    // around it; outside its rounded end, the desk.
    let glass = px(&target, 222, 342);
    let desk = px(&target, 100, 342);
    let sum = |c: RgbaColor| c.r as u32 + c.g as u32 + c.b as u32;
    assert!(sum(glass) > sum(desk), "{glass:?} over {desk:?}");
    assert_eq!(px(&target, 201, 301), desk, "the capsule's corner is round");

    let compositor = Compositor::new();
    let windows = vec![dock];
    // Three tiles, centred: total row = 3*64 + 2*6 = 204; starts at 200 + (240-204)/2 = 218.
    let first_x = 218 + DOCK_TILE / 2;
    let y = 306 + DOCK_TILE / 2;
    let hit = compositor.hit_test(&windows, first_x, y).unwrap();
    assert_eq!(hit.region, HitRegion::DockTile { index: 0 });
    let hit = compositor.hit_test(&windows, first_x + 70 * 2, y).unwrap();
    assert_eq!(hit.region, HitRegion::DockTile { index: 2 });
    // Between tiles is the pill, not a tile.
    let hit = compositor
        .hit_test(&windows, 218 + DOCK_TILE + 3, y)
        .unwrap();
    assert_eq!(hit.region, HitRegion::Border);
    // A tile is drawn as the inactive tab colour, and the running light
    // under the first one is the accent.
    assert_eq!(px(&target, first_x, y), theme.tab_inactive);
    assert_eq!(px(&target, first_x, 306 + DOCK_TILE + 4), theme.accent);
}

/// The dock's extras (GFX-114): a divider before a tile that asks for
/// one, a puck and the tile's name over the dock under the pointer, and a
/// longer light for the focused app.
#[test]
fn the_dock_divides_names_the_hovered_tile_and_lengthens_the_focused_light() {
    use services_gui_host::{dock_row_width, DOCK_DIVIDER, DOCK_TILE_GAP};
    let theme = Theme::DEFAULT;
    let bounds = RasterRect::new(120, 300, 400, 84);
    let mut hovered = DesktopTab::new("Fi", true);
    hovered.hovered = true;
    hovered.name = "Files".to_string();
    let mut focused = DesktopTab::new("Np", true);
    focused.focused = true;
    let mut apps = DesktopTab::new("Ap", false);
    apps.divider_before = true;
    let tabs = vec![focused, hovered, apps];
    assert_eq!(
        dock_row_width(&tabs),
        3 * 64 + 2 * DOCK_TILE_GAP + DOCK_DIVIDER
    );
    let dock = DesktopWindow::new(
        frame("", &[]),
        services_gui_host::SurfaceRect::new(0, 0, 0, 0),
    )
    .with_style(WindowStyle::Dock)
    .with_pixel_rect(bounds)
    .with_tabs(tabs);
    let target = render(vec![dock.clone()]);
    let start = 120 + (400 - dock_row_width(&dock.tabs)) / 2;
    // The third tile starts past the divider; the divider is a line of
    // light in the middle of its room.
    let third = start + 2 * (64 + DOCK_TILE_GAP) + DOCK_DIVIDER;
    let hit = Compositor::new()
        .hit_test(&[dock.clone()], third + 4, 330)
        .unwrap();
    assert_eq!(hit.region, HitRegion::DockTile { index: 2 });
    let line_x = third - (DOCK_TILE_GAP + DOCK_DIVIDER) / 2;
    let sum = |c: RgbaColor| c.r as u32 + c.g as u32 + c.b as u32;
    assert!(sum(px(&target, line_x, 342)) > sum(px(&target, line_x - 3, 342)));
    // The name is over the dock: something is drawn above the capsule,
    // over the hovered tile, where the desk is otherwise plain.
    let label_centre = start + 64 + DOCK_TILE_GAP + 32;
    let desk = px(&target, 20, 280);
    assert!((270..292).any(|y| px(&target, label_centre, y) != desk));
    assert!((270..292).all(|y| px(&target, 20, y) == desk));
    // The focused light is longer than a running one's.
    let lit = |x0: usize| {
        (x0..x0 + 64)
            .filter(|x| px(&target, *x, 306 + DOCK_TILE + 4) == theme.accent)
            .count()
    };
    assert!(
        lit(start) > lit(start + 64 + DOCK_TILE_GAP),
        "focused is longer"
    );
    assert!(lit(start + 64 + DOCK_TILE_GAP) > 0);
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
    // Frosted glass (GFX-114): the tint over the blurred desk, so near
    // the raised surface but not it.
    let glass = px(&target, 320, 4);
    assert!(near(glass, theme.surface_raised, 24), "{glass:?}");
    assert_ne!(glass, theme.background);
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
