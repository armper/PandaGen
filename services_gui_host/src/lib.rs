//! GUI host and compositor on view surfaces.
//!
//! The compositor core is `no_std` + `alloc` so the bare-metal kernel can
//! compose the same desktop the host tests validate. The workspace-manager
//! adapter is behind the `workspace` feature (on by default).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use graphics_rasterizer::{
    RasterRect, RenderTarget, RgbaBuffer, RgbaColor, ScissorTarget, DESKTOP_FONT, SMOOTH_FONT,
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "workspace")]
use services_workspace_manager::{SplitAxis, WorkspaceRenderSnapshot, WorkspaceTileRenderSnapshot};
use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

pub mod animation;
pub mod backend;
pub mod bench;
pub mod degrade;
pub mod host;
pub mod input_routing;
pub mod layout;
pub mod memory;
pub mod scene;
pub mod shell;
pub mod telemetry;
pub mod theme;
pub mod transport;
pub use animation::{AnimationClock, Blink, Easing, Transition};
pub use backend::{
    BackendCapabilities, BackendError, CompositionStage, RenderBackend, SoftwareBackend,
};
pub use degrade::{Degradation, MemoryPressure, PressureMonitor, PressureThresholds};
pub use host::{HostEvent, HostResponse, HostedComponent, HostedSurface, ListComponent};
pub use input_routing::{CaptureState, Delivery, DesktopInputRouter};
pub use layout::{Anchor, Axis, Insets, LayoutId, LayoutNode, Length};
pub use memory::{BudgetError, Reservation, SurfaceBudget};
pub use scene::ScrollRegion;
pub use shell::{
    compose_shell, shell_layout, LauncherItem, NoticeLevel, ShellModel, ShellNotice, ShellRects,
    ShellViewIds,
};
pub use telemetry::{GfxSnapshot, GfxTelemetry};
pub use theme::{Picture, Theme};
pub use transport::{
    apply_delta, diff_scenes, DecodeError, SceneDecoder, SceneDelta, SceneEncoder, SceneReplay,
    SceneUpdate,
};

const DESKTOP_BACKGROUND: char = '.';
const CURSOR_GLYPH: char = '@';
/// Pixel width of one desktop text cell (window rects are in cell units).
pub const RASTER_CELL_WIDTH: usize = DESKTOP_FONT.advance_x();
/// Pixel height of one desktop text cell, including line spacing.
pub const RASTER_CELL_HEIGHT: usize = DESKTOP_FONT.glyph_height() + 2;
const RASTER_BORDER_THICKNESS: usize = 1;

const DESKTOP_BACKGROUND_COLOR: RgbaColor = Theme::DEFAULT.background;
const POINTER_FILL_COLOR: RgbaColor = Theme::DEFAULT.pointer_fill;
const POINTER_OUTLINE_COLOR: RgbaColor = Theme::DEFAULT.pointer_outline;
const WINDOW_FILL_COLOR: RgbaColor = Theme::DEFAULT.surface;
const FOCUSED_BORDER_COLOR: RgbaColor = Theme::DEFAULT.border_focused;
const UNFOCUSED_BORDER_COLOR: RgbaColor = Theme::DEFAULT.border_unfocused;
const TEXT_COLOR: RgbaColor = Theme::DEFAULT.text;
const CURSOR_COLOR: RgbaColor = Theme::DEFAULT.caret;
/// Corner radius of tab boxes in the title strip.
const TAB_RADIUS: usize = 3;

/// Dimensions of a composited surface.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceSize {
    pub width: usize,
    pub height: usize,
}

impl SurfaceSize {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

/// Rectangular placement for a desktop window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl SurfaceRect {
    pub fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Window descriptor for desktop composition.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum DesktopWindowRole {
    /// Primary application or workspace content.
    #[default]
    Main,
    /// Status strip or compact informational surface.
    Status,
    /// Non-modal overlay attached to the current workspace.
    Overlay,
    /// Command or launcher palette that floats above workspace content.
    Palette,
    /// Transient system notification surface.
    Notification,
    /// Blocking modal surface that captures interaction priority.
    Modal,
    /// Shell launcher strip: non-focusable, sits with workspace surfaces.
    Launcher,
}

/// Canonical desktop layer ordering policy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum DesktopWindowLayer {
    /// Regular workspace-managed windows and status surfaces.
    #[default]
    Workspace,
    /// Non-modal overlays attached to the current workspace.
    Overlay,
    /// Command palettes and launchers that float above overlays.
    Palette,
    /// Transient toast or alert surfaces.
    Notification,
    /// Blocking modal surfaces with the highest interactive priority.
    Modal,
    /// Reserved top layer for future system-owned surfaces.
    System,
}

impl DesktopWindowLayer {
    fn for_role(role: DesktopWindowRole) -> Self {
        match role {
            DesktopWindowRole::Main | DesktopWindowRole::Status | DesktopWindowRole::Launcher => {
                Self::Workspace
            }
            DesktopWindowRole::Overlay => Self::Overlay,
            DesktopWindowRole::Palette => Self::Palette,
            DesktopWindowRole::Notification => Self::Notification,
            DesktopWindowRole::Modal => Self::Modal,
        }
    }

    /// Composition order: lower keys are painted first (further back).
    pub fn sort_key(self) -> usize {
        match self {
            Self::Workspace => 0,
            Self::Overlay => 1,
            Self::Palette => 2,
            Self::Notification => 3,
            Self::Modal => 4,
            Self::System => 5,
        }
    }
}

/// Visible tab metadata for a desktop window chrome strip.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopTab {
    pub label: String,
    pub active: bool,
    /// The pointer is over this tab (a dock tile lights up under it).
    #[serde(default)]
    pub hovered: bool,
    /// The app has a window tucked into the dock (GFX-053): the running dot
    /// is drawn as a ring, so the tile says "something is here, waiting".
    #[serde(default)]
    pub tucked: bool,
    /// A 16x16 one-bit icon (GFX-084), drawn two pixels a bit in place of
    /// the monogram when present. Row 0 is the top; bit 15 is the left.
    #[serde(default)]
    pub icon: Option<[u16; 16]>,
    /// A picture from the theme's table (GFX-094), drawn in place of the
    /// tile, the icon and the monogram when the theme has it.
    #[serde(default)]
    pub picture: Option<u32>,
}

impl DesktopTab {
    pub fn new(label: impl Into<String>, active: bool) -> Self {
        Self {
            label: label.into(),
            active,
            hovered: false,
            tucked: false,
            icon: None,
            picture: None,
        }
    }

    pub fn with_picture(mut self, picture: Option<u32>) -> Self {
        self.picture = picture;
        self
    }

    pub fn with_icon(mut self, icon: Option<[u16; 16]>) -> Self {
        self.icon = icon;
        self
    }
}

/// The side of a dock icon as drawn: sixteen bits, two pixels each.
pub const DOCK_ICON: usize = 32;

/// Window descriptor for desktop composition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopWindow {
    pub frame: ViewFrame,
    pub rect: SurfaceRect,
    #[serde(default)]
    pub role: DesktopWindowRole,
    #[serde(default)]
    pub layer: DesktopWindowLayer,
    #[serde(default)]
    pub tabs: Vec<DesktopTab>,
    pub z_index: usize,
    pub focused: bool,
    /// Whether the title/tab strip row is drawn. Shell strips (status bar,
    /// launcher) turn it off so content starts at the border.
    #[serde(default = "default_chrome")]
    pub chrome: bool,
    /// Content line drawn with the selection fill (palette selection,
    /// active launcher entry).
    #[serde(default)]
    pub highlight_line: Option<usize>,
    /// Selected text, as `(content line, first column, end column)` spans
    /// in character cells, painted with the selection fill behind the text
    /// (GFX-054). A span may run past the line's end to mark a selected
    /// line break.
    #[serde(default)]
    pub selection_spans: Vec<(usize, usize, usize)>,
    /// How particular content lines are drawn (GFX-073): weight, tone,
    /// underline. One font, so weight is a one-pixel double strike and
    /// size stays the grid's; hierarchy comes from tone and rule.
    #[serde(default)]
    pub line_styles: Vec<(usize, LineStyle)>,
    /// Draw operations laid over the content after the text (GFX-086), in
    /// the content area's pixel space: icons beside rows, a badge in a
    /// corner. This is how a text card gets pictures without giving up
    /// its lines.
    #[serde(default)]
    pub overlay: Vec<view_types::DrawOp>,
    /// How the window is painted and hit-tested (GFX-050).
    #[serde(default)]
    pub style: WindowStyle,
    /// Pixel geometry, overriding the cell `rect` when set.
    ///
    /// The desk positions windows by pixel -- a card dragged by its header
    /// lands where the pointer left it, not on the nearest cell. Everything
    /// that needs a window's bounds goes through [`DesktopWindow::bounds`],
    /// which is what lets both kinds coexist in one scene.
    #[serde(default)]
    pub pixel_rect: Option<RasterRect>,
    /// Whether a card draws its close glyph and answers `HitRegion::Close`.
    #[serde(default)]
    pub closable: bool,
    /// Muted text drawn in a card's footer strip (a notepad's `Ln 3, Col 12`).
    #[serde(default)]
    pub footer: Option<String>,
    /// Action chips in a card's header, right of the title (GFX-056):
    /// small labelled pills, hit as `HitRegion::Action { index }`. The
    /// card's own commands live here, so nothing needs a menu bar.
    #[serde(default)]
    pub actions: Vec<String>,
}

/// Height of a header action chip.
pub const CARD_CHIP_HEIGHT: usize = 16;
/// Horizontal padding inside a chip, each side.
pub const CARD_CHIP_PAD: usize = 6;
/// Gap between chips.
pub const CARD_CHIP_GAP: usize = 6;

/// Which of the theme's text colours a styled line takes (GFX-073).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum LineTone {
    #[default]
    Text,
    Accent,
    Muted,
}

/// How one content line is drawn (GFX-073). Bold is a second strike one
/// pixel right, the only weight one 8x16 font has; the line keeps the
/// grid's height so nothing below it moves.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LineStyle {
    pub bold: bool,
    pub tone: LineTone,
    pub underline: bool,
}

impl LineStyle {
    pub const HEADING: LineStyle = LineStyle {
        bold: true,
        tone: LineTone::Accent,
        underline: true,
    };
    pub const SUBHEADING: LineStyle = LineStyle {
        bold: true,
        tone: LineTone::Accent,
        underline: false,
    };
    pub const STRONG: LineStyle = LineStyle {
        bold: true,
        tone: LineTone::Text,
        underline: false,
    };
    pub const QUIET: LineStyle = LineStyle {
        bold: false,
        tone: LineTone::Muted,
        underline: false,
    };
}

/// How a window is painted (GFX-050).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WindowStyle {
    /// The tiled workspace look: cell grid, one-cell title strip, tabs.
    #[default]
    Classic,
    /// A desk card: flat surface, hairline, rounded corners, a slim header
    /// with a title and one close glyph, an optional footer strip.
    Card,
    /// The desk's bottom dock: a centred pill of rounded tiles, one per
    /// `tabs` entry, whose label is a monogram and whose `active` flag draws
    /// the running dot.
    Dock,
    /// The desk's top bar: the window title at the left, the first content
    /// line right-aligned.
    TopBar,
    /// A veil over everything (GFX-096): what is under it, darkened, and
    /// the overlay drawn on top. The rest screen is one.
    Veil,
}

fn default_chrome() -> bool {
    true
}

/// Card geometry (GFX-050). Pixels, not cells.
pub const CARD_HEADER_HEIGHT: usize = 24;
pub const CARD_PADDING: usize = 8;
pub const CARD_LINE_HEIGHT: usize = 20;
pub const CARD_RADIUS: usize = 6;
pub const CARD_FOOTER_HEIGHT: usize = 20;
/// The close glyph's hit box, inset from the header's right edge.
pub const CARD_CLOSE_SIZE: usize = 16;
/// How far the shadow extends past a card, right and down. Damage for a
/// moved card must grow by this much or it leaves a trail.
pub const CARD_LIFT: usize = 2;
/// Dock tiles.
pub const DOCK_TILE: usize = 40;
pub const DOCK_TILE_GAP: usize = 8;
pub const DOCK_RADIUS: usize = 12;
pub const DOCK_TILE_RADIUS: usize = 8;

impl DesktopWindow {
    pub fn new(frame: ViewFrame, rect: SurfaceRect) -> Self {
        Self {
            frame,
            rect,
            role: DesktopWindowRole::Main,
            layer: DesktopWindowLayer::Workspace,
            tabs: Vec::new(),
            z_index: 0,
            focused: false,
            chrome: true,
            highlight_line: None,
            selection_spans: Vec::new(),
            line_styles: Vec::new(),
            overlay: Vec::new(),
            style: WindowStyle::Classic,
            pixel_rect: None,
            closable: false,
            footer: None,
            actions: Vec::new(),
        }
    }

    /// A card at a pixel rectangle (GFX-050).
    pub fn card(frame: ViewFrame, bounds: RasterRect) -> Self {
        let mut window = Self::new(frame, SurfaceRect::new(0, 0, 0, 0));
        window.style = WindowStyle::Card;
        window.pixel_rect = Some(bounds);
        window.closable = true;
        window
    }

    pub fn with_style(mut self, style: WindowStyle) -> Self {
        self.style = style;
        self
    }

    /// Give a card header action chips (GFX-056).
    pub fn with_actions(mut self, actions: Vec<String>) -> Self {
        self.actions = actions;
        self
    }

    /// Where each header chip sits, in `actions` order, laid out from the
    /// close glyph leftwards. Chips that would run into the title are
    /// dropped from the end, so a narrow card keeps its first actions.
    pub fn action_rects(&self) -> Vec<Option<RasterRect>> {
        let Some(header) = self.header_rect() else {
            return self.actions.iter().map(|_| None).collect();
        };
        let right = match self.close_rect() {
            Some(close) => close.x.saturating_sub(CARD_CHIP_GAP),
            None => header.right().saturating_sub(CARD_PADDING),
        };
        // Leave the title at least twelve cells.
        let min_x = header.x + CARD_PADDING + 4 + 12 * RASTER_CELL_WIDTH;
        let y = header.y + (CARD_HEADER_HEIGHT.saturating_sub(CARD_CHIP_HEIGHT)) / 2;
        let widths: Vec<usize> = self
            .actions
            .iter()
            .map(|a| a.chars().count() * RASTER_CELL_WIDTH + CARD_CHIP_PAD * 2)
            .collect();
        let mut keep = widths.len();
        let mut total = 0;
        while keep > 0 {
            total = widths[..keep].iter().sum::<usize>() + CARD_CHIP_GAP * (keep - 1);
            if right >= total && right - total >= min_x {
                break;
            }
            keep -= 1;
        }
        let mut x = if keep == 0 { 0 } else { right - total };
        widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                if index >= keep {
                    return None;
                }
                let rect = RasterRect::new(x, y, *width, CARD_CHIP_HEIGHT);
                x += width + CARD_CHIP_GAP;
                Some(rect)
            })
            .collect()
    }

    pub fn with_pixel_rect(mut self, bounds: RasterRect) -> Self {
        self.pixel_rect = Some(bounds);
        self
    }

    pub fn with_footer(mut self, footer: Option<String>) -> Self {
        self.footer = footer;
        self
    }

    /// The window's pixel bounds, whichever way its geometry was given.
    pub fn bounds(&self) -> RasterRect {
        self.pixel_rect.unwrap_or_else(|| pixel_rect(self.rect))
    }

    /// Where a card's header sits, or `None` for other styles.
    pub fn header_rect(&self) -> Option<RasterRect> {
        (self.style == WindowStyle::Card).then(|| {
            let bounds = self.bounds();
            RasterRect::new(
                bounds.x,
                bounds.y,
                bounds.width,
                CARD_HEADER_HEIGHT.min(bounds.height),
            )
        })
    }

    /// Where a card's close glyph sits, or `None` when there is none.
    pub fn close_rect(&self) -> Option<RasterRect> {
        if !self.closable {
            return None;
        }
        let header = self.header_rect()?;
        let inset = (CARD_HEADER_HEIGHT.saturating_sub(CARD_CLOSE_SIZE)) / 2;
        let x = header.right().checked_sub(CARD_CLOSE_SIZE + inset)?;
        Some(RasterRect::new(
            x,
            header.y + inset,
            CARD_CLOSE_SIZE,
            CARD_CLOSE_SIZE,
        ))
    }

    /// Where a card's resize grip sits, or `None` for other styles.
    pub fn grip_rect(&self) -> Option<RasterRect> {
        (self.style == WindowStyle::Card).then(|| {
            let bounds = self.bounds();
            RasterRect::new(
                bounds.right().saturating_sub(CARD_GRIP_SIZE),
                bounds.bottom().saturating_sub(CARD_GRIP_SIZE),
                CARD_GRIP_SIZE,
                CARD_GRIP_SIZE,
            )
        })
    }

    /// The text origin and pitch of a card's content: `(x, y, line_height)`.
    pub fn card_text_origin(&self) -> (usize, usize, usize) {
        let bounds = self.bounds();
        (
            bounds.x + CARD_PADDING,
            bounds.y + CARD_HEADER_HEIGHT + CARD_PADDING,
            CARD_LINE_HEIGHT,
        )
    }

    pub fn with_role(mut self, role: DesktopWindowRole) -> Self {
        self.role = role;
        self.layer = DesktopWindowLayer::for_role(role);
        self
    }

    pub fn with_layer(mut self, layer: DesktopWindowLayer) -> Self {
        self.layer = layer;
        self
    }

    pub fn with_tabs(mut self, tabs: Vec<DesktopTab>) -> Self {
        self.tabs = tabs;
        self
    }

    pub fn with_z_index(mut self, z_index: usize) -> Self {
        self.z_index = z_index;
        self
    }

    pub fn focused(mut self) -> Self {
        self.focused = true;
        self
    }

    /// Fill content line `line` with the selection colour.
    pub fn with_highlight(mut self, line: Option<usize>) -> Self {
        self.highlight_line = line;
        self
    }

    /// Paint these `(line, start, end)` character spans with the selection
    /// fill.
    pub fn with_selection(mut self, spans: Vec<(usize, usize, usize)>) -> Self {
        self.selection_spans = spans;
        self
    }

    /// Style particular content lines (GFX-073).
    pub fn with_line_styles(mut self, styles: Vec<(usize, LineStyle)>) -> Self {
        self.line_styles = styles;
        self
    }

    /// Lay draw operations over the content, after the text (GFX-086).
    pub fn with_overlay(mut self, ops: Vec<view_types::DrawOp>) -> Self {
        self.overlay = ops;
        self
    }

    /// Draw no title row; content begins right inside the border.
    pub fn without_chrome(mut self) -> Self {
        self.chrome = false;
        self
    }

    /// Content lines this window can show at its cell height.
    pub fn content_rows(&self) -> usize {
        match self.style {
            WindowStyle::Card => {
                let bounds = self.bounds();
                let footer = if self.footer.is_some() {
                    CARD_FOOTER_HEIGHT
                } else {
                    0
                };
                bounds
                    .height
                    .saturating_sub(CARD_HEADER_HEIGHT + CARD_PADDING * 2 + footer)
                    / CARD_LINE_HEIGHT
            }
            WindowStyle::Dock | WindowStyle::TopBar | WindowStyle::Veil => 0,
            WindowStyle::Classic if self.chrome => self.rect.height.saturating_sub(2),
            WindowStyle::Classic => self.rect.height,
        }
    }
}

/// Composited surface frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceFrame {
    pub width: usize,
    pub height: usize,
    pub rows: Vec<String>,
    pub content: String,
    pub frame_count: usize,
    pub timestamp_ns: u64,
}

impl SurfaceFrame {
    fn from_rows(rows: Vec<String>, frame_count: usize, timestamp_ns: u64) -> Self {
        let width = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        let height = rows.len();
        let content = rows.join("\n");

        Self {
            width,
            height,
            rows,
            content,
            frame_count,
            timestamp_ns,
        }
    }
}

/// Rasterized desktop frame in RGBA pixel space.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RasterSurfaceFrame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub frame_count: usize,
    pub timestamp_ns: u64,
}

impl RasterSurfaceFrame {
    fn new(buffer: RgbaBuffer, frame_count: usize, timestamp_ns: u64) -> Self {
        Self {
            width: buffer.width(),
            height: buffer.height(),
            pixels: buffer.as_bytes().to_vec(),
            frame_count,
            timestamp_ns,
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<RgbaColor> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y * self.width + x) * 4;
        Some(RgbaColor::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RasterRenderStats {
    pub frame_count: usize,
    pub timestamp_ns: u64,
    #[serde(default)]
    pub painted_windows: usize,
    #[serde(default)]
    pub damage_rect: Option<RasterRect>,
}

/// Simple compositor that merges view frames into a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compositor {
    theme: Theme,
    /// Painted behind everything instead of the theme's gradient (GFX-066).
    wallpaper: Option<Wallpaper>,
}

/// A picture for the desk (GFX-066): 256 colours, one byte a pixel plus a
/// palette, sampled nearest-neighbour to the surface, so one image serves
/// any screen. Static because the kernel builds it in; a loaded picture
/// would be a leaked `Vec` behind the same reference. Indexed rather than
/// RGB because the first RGB build made the kernel 11 MB and a 16 MiB
/// machine no longer booted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wallpaper {
    pub width: usize,
    pub height: usize,
    /// 256 RGB triples.
    pub palette: &'static [u8],
    /// `width * height` palette indices, row-major.
    pub indices: &'static [u8],
}

impl Wallpaper {
    /// The colour at surface pixel `(x, y)` on a `surface_w` x `surface_h`
    /// surface.
    pub fn sample(&self, x: usize, y: usize, surface_w: usize, surface_h: usize) -> RgbaColor {
        if self.width == 0 || self.height == 0 || surface_w == 0 || surface_h == 0 {
            return RgbaColor::new(0, 0, 0, 255);
        }
        if self.width < surface_w || self.height < surface_h {
            return self.sample_smooth(x, y, surface_w, surface_h);
        }
        let sx = (x * self.width / surface_w).min(self.width - 1);
        let sy = (y * self.height / surface_h).min(self.height - 1);
        self.colour_at(sx, sy)
    }

    fn colour_at(&self, sx: usize, sy: usize) -> RgbaColor {
        let index = self.indices.get(sy * self.width + sx).copied().unwrap_or(0) as usize;
        match self.palette.get(index * 3..index * 3 + 3) {
            Some(px) => RgbaColor::new(px[0], px[1], px[2], 255),
            None => RgbaColor::new(0, 0, 0, 255),
        }
    }

    /// Enlarging (GFX-097): blend the four source pixels around the
    /// surface pixel's centre by distance, so a half-size wallpaper is
    /// soft rather than blocky. Fixed point, 1/256 of a source pixel.
    fn sample_smooth(&self, x: usize, y: usize, surface_w: usize, surface_h: usize) -> RgbaColor {
        let fx = ((2 * x + 1) * self.width * 128 / surface_w).saturating_sub(128);
        let fy = ((2 * y + 1) * self.height * 128 / surface_h).saturating_sub(128);
        let (x0, y0) = (
            (fx >> 8).min(self.width - 1),
            (fy >> 8).min(self.height - 1),
        );
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (ax, ay) = ((fx & 255) as u32, (fy & 255) as u32);
        let (a, b) = (self.colour_at(x0, y0), self.colour_at(x1, y0));
        let (c, d) = (self.colour_at(x0, y1), self.colour_at(x1, y1));
        let mix = |p: u8, q: u8, r: u8, s: u8| -> u8 {
            let top = p as u32 * (256 - ax) + q as u32 * ax;
            let bottom = r as u32 * (256 - ax) + s as u32 * ax;
            ((top * (256 - ay) + bottom * ay) >> 16) as u8
        };
        RgbaColor::new(
            mix(a.r, b.r, c.r, d.r),
            mix(a.g, b.g, c.g, d.g),
            mix(a.b, b.b, c.b, d.b),
            255,
        )
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            theme: Theme::DEFAULT,
            wallpaper: None,
        }
    }

    /// Compositor painting with `theme` (GFX-035).
    pub const fn with_theme(theme: Theme) -> Self {
        Self {
            theme,
            wallpaper: None,
        }
    }

    /// Paint `wallpaper` behind everything (GFX-066); `None` is the
    /// theme's gradient.
    pub const fn with_wallpaper(mut self, wallpaper: Option<Wallpaper>) -> Self {
        self.wallpaper = wallpaper;
        self
    }

    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn compose(&self, mut frames: Vec<ViewFrame>) -> SurfaceFrame {
        frames.sort_by_key(|frame| frame.view_id.as_uuid());

        let mut output = String::new();
        for frame in frames.iter() {
            let title = frame
                .title
                .clone()
                .unwrap_or_else(|| format!("{:?}", frame.kind));
            output.push_str(&format!("[{}]\n", title));
            output.push_str(&render_content(&frame.content));
            output.push('\n');
        }

        let rows = output
            .trim_end_matches('\n')
            .lines()
            .map(|line| line.to_string())
            .collect();

        SurfaceFrame::from_rows(
            rows,
            frames.len(),
            frames
                .iter()
                .map(|frame| frame.timestamp_ns)
                .max()
                .unwrap_or(0),
        )
    }

    /// Compose a deterministic desktop surface from positioned windows.
    ///
    /// This models a clean-slate desktop surface directly, rather than routing
    /// through terminal-era abstractions. The result is still text-serializable
    /// so composition logic remains fully testable under `cargo test`.
    pub fn compose_desktop(
        &self,
        size: SurfaceSize,
        mut windows: Vec<DesktopWindow>,
    ) -> SurfaceFrame {
        let mut canvas = vec![vec![DESKTOP_BACKGROUND; size.width]; size.height];
        windows.sort_by_key(|window| {
            (
                window.layer.sort_key(),
                window.z_index,
                window.frame.view_id.as_uuid(),
            )
        });

        for window in &windows {
            draw_window(&mut canvas, window);
        }

        let rows = canvas
            .into_iter()
            .map(|row| row.into_iter().collect::<String>())
            .collect::<Vec<_>>();
        let timestamp_ns = windows
            .iter()
            .map(|window| window.frame.timestamp_ns)
            .max()
            .unwrap_or(0);

        SurfaceFrame::from_rows(rows, windows.len(), timestamp_ns)
    }

    /// Compose a desktop into an RGBA pixel buffer using the software rasterizer.
    pub fn compose_desktop_rgba(
        &self,
        size: SurfaceSize,
        windows: Vec<DesktopWindow>,
    ) -> RasterSurfaceFrame {
        let mut buffer = RgbaBuffer::new(
            size.width.saturating_mul(RASTER_CELL_WIDTH),
            size.height.saturating_mul(RASTER_CELL_HEIGHT),
            self.theme.background,
        );
        let stats = self.render_desktop_to_target(&mut buffer, windows);
        RasterSurfaceFrame::new(buffer, stats.frame_count, stats.timestamp_ns)
    }

    /// Render a desktop into any pixel target that implements the raster contract.
    pub fn render_desktop_to_target(
        &self,
        target: &mut impl RenderTarget,
        windows: Vec<DesktopWindow>,
    ) -> RasterRenderStats {
        self.render_desktop_to_target_with_damage(target, windows, None)
    }

    /// Render only the damaged region of a desktop into a pixel target.
    pub fn render_desktop_to_target_with_damage(
        &self,
        target: &mut impl RenderTarget,
        windows: Vec<DesktopWindow>,
        damage_rect: Option<RasterRect>,
    ) -> RasterRenderStats {
        self.render_desktop_to_target_with_cursor(target, windows, damage_rect, None)
    }

    /// Render windows and then the pointer cursor above them.
    ///
    /// The cursor is painted last and unclipped by window damage: when the
    /// damaged region does not include the sprite, the cursor is still drawn
    /// so a repaint under a stationary pointer never erases it.
    pub fn render_desktop_to_target_with_cursor(
        &self,
        target: &mut impl RenderTarget,
        mut windows: Vec<DesktopWindow>,
        damage_rect: Option<RasterRect>,
        cursor: Option<DesktopCursor>,
    ) -> RasterRenderStats {
        let target_bounds = RasterRect::new(0, 0, target.width(), target.height());
        let damage_rect = damage_rect.and_then(|rect| rect.intersect(target_bounds));

        // The desk background is a vertical gradient (GFX-052), so both the
        // full clear and a damage repaint go through one row-wise fill, or
        // a repainted rectangle would show a flat patch of the top colour.
        let whole = RasterRect::new(0, 0, target.width(), target.height());
        fill_background(
            target,
            damage_rect.unwrap_or(whole),
            whole.height,
            &self.theme,
            self.wallpaper,
        );

        windows.sort_by_key(composition_sort_key);

        let mut painted_windows = 0;
        for window in &windows {
            if raster_window(target, window, damage_rect, &self.theme) {
                painted_windows += 1;
            }
        }

        if let Some(cursor) = cursor {
            raster_cursor(target, &cursor, &self.theme);
        }

        RasterRenderStats {
            frame_count: windows.len(),
            timestamp_ns: windows
                .iter()
                .map(|window| window.frame.timestamp_ns)
                .max()
                .unwrap_or(0),
            painted_windows,
            damage_rect,
        }
    }
}

/// Pointer cursor shape.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Arrow,
}

/// Pointer cursor composed above every window (GFX-025).
///
/// The cursor is its own surface layer rather than a glyph in a text grid:
/// it has a pixel position, a hotspot, and is painted last so it is never
/// occluded by a window. `visible == false` composes nothing, which is how a
/// desktop hides the pointer during keyboard-only interaction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopCursor {
    pub x: usize,
    pub y: usize,
    pub shape: CursorShape,
    pub visible: bool,
}

impl DesktopCursor {
    pub const fn new(x: usize, y: usize) -> Self {
        Self {
            x,
            y,
            shape: CursorShape::Arrow,
            visible: true,
        }
    }

    pub const fn hidden() -> Self {
        Self {
            x: 0,
            y: 0,
            shape: CursorShape::Arrow,
            visible: false,
        }
    }

    /// Pixel bounds the cursor sprite covers (for damage tracking).
    pub fn bounds(&self) -> RasterRect {
        let (w, h) = cursor_sprite_size(self.shape);
        RasterRect::new(self.x, self.y, w, h)
    }
}

/// Arrow sprite, 12x19. `X` outline, `o` fill, `.` transparent. The hotspot
/// is the top-left pixel.
const ARROW_SPRITE: [&str; 19] = [
    "X...........",
    "XX..........",
    "XoX.........",
    "XooX........",
    "XoooX.......",
    "XooooX......",
    "XoooooX.....",
    "XooooooX....",
    "XoooooooX...",
    "XooooooooX..",
    "XoooooooooX.",
    "XooooooXXXXX",
    "XoooXooX....",
    "XooX.XooX...",
    "XoX..XooX...",
    "XX....XooX..",
    "X.....XooX..",
    ".......XX...",
    "............",
];

fn cursor_sprite_size(shape: CursorShape) -> (usize, usize) {
    match shape {
        CursorShape::Arrow => (ARROW_SPRITE[0].len(), ARROW_SPRITE.len()),
    }
}

/// Paint the cursor sprite; pixels outside the target are skipped by the target.
fn raster_cursor(target: &mut impl RenderTarget, cursor: &DesktopCursor, theme: &Theme) {
    if !cursor.visible {
        return;
    }
    let rows: &[&str] = match cursor.shape {
        CursorShape::Arrow => &ARROW_SPRITE,
    };
    for (dy, row) in rows.iter().enumerate() {
        for (dx, cell) in row.bytes().enumerate() {
            let color = match cell {
                b'X' => theme.pointer_outline,
                b'o' => theme.pointer_fill,
                _ => continue,
            };
            let x = cursor.x.saturating_add(dx);
            let y = cursor.y.saturating_add(dy);
            if x < target.width() && y < target.height() {
                target.write_pixel(x, y, color);
            }
        }
    }
}

/// A complete, serialisable description of one desktop frame (GFX-041).
///
/// This is what travels to a remote viewer instead of pixels: the window
/// list (in cell units), the cursor, the theme, and the optional damage
/// rectangle. Rendering it through `Compositor::render_scene` on any host
/// reproduces the same pixels, which is what makes remote sessions and
/// replays deterministic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopScene {
    /// Surface size in cells.
    pub size: SurfaceSize,
    pub windows: Vec<DesktopWindow>,
    #[serde(default)]
    pub cursor: Option<DesktopCursor>,
    #[serde(default)]
    pub theme: Option<Theme>,
    /// Region that changed since the previous scene, if known.
    #[serde(default)]
    pub damage: Option<RasterRect>,
    /// The picture behind everything (GFX-066). The machine's, not the
    /// remote viewer's: it is not serialised.
    #[serde(skip)]
    pub wallpaper: Option<Wallpaper>,
}

impl DesktopScene {
    pub fn new(size: SurfaceSize, windows: Vec<DesktopWindow>) -> Self {
        Self {
            size,
            windows,
            cursor: None,
            theme: None,
            damage: None,
            wallpaper: None,
        }
    }

    pub fn with_wallpaper(mut self, wallpaper: Option<Wallpaper>) -> Self {
        self.wallpaper = wallpaper;
        self
    }

    pub fn with_cursor(mut self, cursor: Option<DesktopCursor>) -> Self {
        self.cursor = cursor;
        self
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn with_damage(mut self, damage: Option<RasterRect>) -> Self {
        self.damage = damage;
        self
    }

    /// Pixel size of the surface this scene describes.
    pub fn pixel_size(&self) -> (usize, usize) {
        (
            self.size.width.saturating_mul(RASTER_CELL_WIDTH),
            self.size.height.saturating_mul(RASTER_CELL_HEIGHT),
        )
    }
}

impl Compositor {
    /// Repaint a scene into a persistent `target` that already shows the
    /// previous scene, touching only `scene.damage` when it is set. The
    /// scene's theme, when present, overrides the compositor's own so a
    /// remote viewer paints what the sender saw.
    pub fn render_scene(
        &self,
        target: &mut impl RenderTarget,
        scene: &DesktopScene,
    ) -> RasterRenderStats {
        self.render_scene_with_damage(target, scene, scene.damage)
    }

    /// Paint a scene in full into `target`, ignoring any damage hint.
    pub fn render_scene_full(
        &self,
        target: &mut impl RenderTarget,
        scene: &DesktopScene,
    ) -> RasterRenderStats {
        self.render_scene_with_damage(target, scene, None)
    }

    fn render_scene_with_damage(
        &self,
        target: &mut impl RenderTarget,
        scene: &DesktopScene,
        damage: Option<RasterRect>,
    ) -> RasterRenderStats {
        let painter = match scene.theme {
            Some(theme) => Compositor::with_theme(theme),
            None => *self,
        }
        .with_wallpaper(scene.wallpaper.or(self.wallpaper));
        painter.render_desktop_to_target_with_cursor(
            target,
            scene.windows.clone(),
            damage,
            scene.cursor,
        )
    }

    /// Render a scene in full into a fresh RGBA surface of its pixel size.
    pub fn render_scene_rgba(&self, scene: &DesktopScene) -> RasterSurfaceFrame {
        let (width, height) = scene.pixel_size();
        let theme = scene.theme.unwrap_or(self.theme);
        let mut buffer = RgbaBuffer::new(width, height, theme.background);
        let stats = self.render_scene_full(&mut buffer, scene);
        RasterSurfaceFrame::new(buffer, stats.frame_count, stats.timestamp_ns)
    }
}

/// Which part of a window a pixel lands on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HitRegion {
    /// The one-pixel frame around the window.
    Border,
    /// The title/tab strip row.
    Chrome,
    /// The content area, with the text cell under the pointer.
    Content { line: usize, column: usize },
    /// A card's header: press-and-drag moves the window (GFX-050).
    Header,
    /// A card's close glyph.
    Close,
    /// A dock tile, by index into `tabs`.
    DockTile { index: usize },
    /// A card's bottom-right corner: press-and-drag resizes (GFX-052).
    Resize,
    /// A header action chip, by index into `actions` (GFX-056).
    Action { index: usize },
}

/// Result of hit testing a desktop position.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct HitTarget {
    /// Index into the window slice that was tested.
    pub window_index: usize,
    pub view_id: ViewId,
    pub role: DesktopWindowRole,
    pub region: HitRegion,
    /// Pixel offset from the window's top-left corner.
    pub local_x: usize,
    pub local_y: usize,
}

/// Sort key shared by painting and hit testing: layer policy first, then
/// z-index, then view id for determinism. Later entries paint on top.
fn composition_sort_key(window: &DesktopWindow) -> (usize, usize, [u8; 16]) {
    (
        window.layer.sort_key(),
        window.z_index,
        *window.frame.view_id.as_uuid().as_bytes(),
    )
}

/// Window indices from bottom-most to top-most.
pub fn composition_order(windows: &[DesktopWindow]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..windows.len()).collect();
    order.sort_by_key(|&index| composition_sort_key(&windows[index]));
    order
}

/// Pixel rectangle of a window placed in cell units.
pub fn window_pixel_rect(rect: SurfaceRect) -> RasterRect {
    pixel_rect(rect)
}

/// Pixel rectangle the caret sprite occupies for `cursor` in `window`, or
/// `None` when the caret falls outside the content area. Matches the caret
/// drawn by the painter exactly, so damage can be limited to it.
pub fn caret_pixel_rect(
    window: &DesktopWindow,
    cursor: view_types::CursorPosition,
) -> Option<RasterRect> {
    if window.style == WindowStyle::Card {
        let (x, y, pitch) = window.card_text_origin();
        let caret = RasterRect::new(
            x + cursor.column * RASTER_CELL_WIDTH,
            y + cursor.line * pitch,
            2,
            pitch.saturating_sub(4),
        );
        return card_content_rect(window).and_then(|content| caret.intersect(content));
    }
    let rect = window.bounds();
    let content_top = if window.chrome {
        rect.y + RASTER_CELL_HEIGHT
    } else {
        rect.y
    };
    let caret = RasterRect::new(
        rect.x + 2 + cursor.column * RASTER_CELL_WIDTH,
        content_top + 1 + cursor.line * RASTER_CELL_HEIGHT,
        4,
        RASTER_CELL_HEIGHT.saturating_sub(2),
    );
    window_content_rect_for(rect, window.chrome).and_then(|content| caret.intersect(content))
}

impl Compositor {
    /// Find the top-most window under desktop pixel `(x, y)`.
    ///
    /// Uses the exact ordering the painter uses, so whatever the user sees on
    /// top is what receives the hit. Returns `None` over bare desktop.
    pub fn hit_test(&self, windows: &[DesktopWindow], x: usize, y: usize) -> Option<HitTarget> {
        for index in composition_order(windows).into_iter().rev() {
            let window = &windows[index];
            let rect = window.bounds();
            if !rect.contains(x, y) {
                continue;
            }
            let local_x = x - rect.x;
            let local_y = y - rect.y;

            if let Some(region) = hit_region_for_style(window, x, y) {
                return Some(HitTarget {
                    window_index: index,
                    view_id: window.frame.view_id,
                    role: window.role,
                    region,
                    local_x,
                    local_y,
                });
            }

            let region = if let Some(content) = window_content_rect_for(rect, window.chrome)
                .filter(|content| content.contains(x, y))
            {
                HitRegion::Content {
                    line: (y - content.y) / RASTER_CELL_HEIGHT,
                    column: (x - content.x) / RASTER_CELL_WIDTH,
                }
            } else if window.chrome && window_chrome_rect(rect).contains(x, y) {
                HitRegion::Chrome
            } else {
                HitRegion::Border
            };

            return Some(HitTarget {
                window_index: index,
                view_id: window.frame.view_id,
                role: window.role,
                region,
                local_x,
                local_y,
            });
        }
        None
    }
}

#[cfg(feature = "workspace")]
impl Compositor {
    /// Map a workspace snapshot into tiled desktop windows.
    ///
    /// This is the first bridge from workspace-managed split/tab state into
    /// the desktop compositor. It intentionally maps only visible tile content
    /// for now; shell overlays such as breadcrumbs and notifications can be
    /// layered on later without changing the tile/window contract.
    pub fn desktop_windows_from_workspace_snapshot(
        &self,
        size: SurfaceSize,
        snapshot: &WorkspaceRenderSnapshot,
    ) -> Vec<DesktopWindow> {
        if !snapshot.tiles.is_empty() {
            return workspace_tile_windows(size, snapshot);
        }

        snapshot
            .main_view
            .as_ref()
            .cloned()
            .map(|frame| {
                DesktopWindow::new(frame, SurfaceRect::new(0, 0, size.width, size.height)).focused()
            })
            .into_iter()
            .collect()
    }

    /// Compose a workspace snapshot directly into a desktop surface.
    pub fn compose_workspace_snapshot(
        &self,
        size: SurfaceSize,
        snapshot: &WorkspaceRenderSnapshot,
    ) -> SurfaceFrame {
        self.compose_desktop(
            size,
            self.desktop_windows_from_workspace_snapshot(size, snapshot),
        )
    }

    /// Compose a workspace snapshot directly into an RGBA pixel surface.
    pub fn compose_workspace_snapshot_rgba(
        &self,
        size: SurfaceSize,
        snapshot: &WorkspaceRenderSnapshot,
    ) -> RasterSurfaceFrame {
        self.compose_desktop_rgba(
            size,
            self.desktop_windows_from_workspace_snapshot(size, snapshot),
        )
    }
}

fn render_content(content: &ViewContent) -> String {
    match content {
        ViewContent::TextBuffer { lines } => lines.join("\n"),
        ViewContent::StatusLine { text } => text.clone(),
        ViewContent::Panel { metadata } => format!("panel: {}", metadata),
        ViewContent::Graphics { ops } => format!("graphics: {} ops", ops.len()),
    }
}

fn render_content_lines(content: &ViewContent) -> Vec<String> {
    match content {
        ViewContent::TextBuffer { lines } => {
            if lines.is_empty() {
                vec![String::new()]
            } else {
                lines.clone()
            }
        }
        ViewContent::StatusLine { text } => vec![text.clone()],
        ViewContent::Panel { metadata } => vec![format!("panel: {}", metadata)],
        // Graphics have no text lines; they are drawn by `raster_graphics`.
        ViewContent::Graphics { .. } => Vec::new(),
    }
}

/// Execute a view's draw operations inside `content_rect`, in the view's
/// own pixel space (GFX-003/005). Everything is clipped by the container.
fn raster_graphics(
    target: &mut impl RenderTarget,
    content_rect: RasterRect,
    ops: &[view_types::DrawOp],
    theme: &Theme,
) {
    use graphics_rasterizer::{ContainerTarget, COMPACT_FONT};
    use view_types::DrawOp;
    let to_color = |c: view_types::Color| RgbaColor::new(c.r, c.g, c.b, c.a);
    let to_rect = |r: view_types::PixelRect| {
        RasterRect::new(
            r.x as usize,
            r.y as usize,
            r.width as usize,
            r.height as usize,
        )
    };
    let mut canvas = ContainerTarget::new(target, content_rect);
    for op in ops {
        match op {
            DrawOp::Fill { rect, color } => canvas.fill_rect(to_rect(*rect), to_color(*color)),
            DrawOp::Border {
                rect,
                thickness,
                color,
            } => canvas.draw_border(to_rect(*rect), *thickness as usize, to_color(*color)),
            DrawOp::RoundedFill {
                rect,
                radius,
                color,
            } => canvas.fill_rounded_rect(to_rect(*rect), *radius as usize, to_color(*color)),
            DrawOp::RoundedBorder {
                rect,
                radius,
                thickness,
                color,
            } => canvas.draw_rounded_border(
                to_rect(*rect),
                *radius as usize,
                *thickness as usize,
                to_color(*color),
            ),
            DrawOp::Line {
                x0,
                y0,
                x1,
                y1,
                color,
            } => canvas.draw_line(
                *x0 as i64,
                *y0 as i64,
                *x1 as i64,
                *y1 as i64,
                to_color(*color),
            ),
            DrawOp::Picture { x, y, id } => {
                if let Some(picture) = theme.pictures.get(*id as usize) {
                    draw_picture(&mut canvas, *x as usize, *y as usize, picture);
                }
            }
            DrawOp::Icon {
                x,
                y,
                scale,
                bits,
                color,
            } => {
                let color = color.map(to_color).unwrap_or(theme.text);
                let scale = (*scale).max(1) as usize;
                for (row, row_bits) in bits.iter().enumerate() {
                    for col in 0..16 {
                        if row_bits & (1 << (15 - col)) != 0 {
                            canvas.fill_rect(
                                RasterRect::new(
                                    *x as usize + col * scale,
                                    *y as usize + row * scale,
                                    scale,
                                    scale,
                                ),
                                color,
                            );
                        }
                    }
                }
            }
            DrawOp::Text {
                x,
                y,
                text,
                color,
                style,
            } => {
                let color = match color {
                    Some(c) => to_color(*c),
                    None if style.muted => theme.text_muted,
                    None => theme.text,
                };
                let font = if style.compact {
                    &COMPACT_FONT
                } else {
                    &SMOOTH_FONT
                };
                if style.scale > 1 {
                    canvas.draw_text_scaled(
                        *x as usize,
                        *y as usize,
                        text,
                        font,
                        style.scale as usize,
                        color,
                    );
                } else {
                    canvas.draw_text_with_font(*x as usize, *y as usize, text, font, color);
                }
            }
        }
    }
}

fn window_title(frame: &ViewFrame) -> String {
    frame.title.clone().unwrap_or_else(|| match frame.kind {
        ViewKind::TextBuffer => "TextBuffer".to_string(),
        ViewKind::StatusLine => "StatusLine".to_string(),
        ViewKind::Panel => "Panel".to_string(),
    })
}

fn window_chrome_label(window: &DesktopWindow) -> String {
    if !window.tabs.is_empty() {
        format!(" {} ", render_tab_strip(&window.tabs))
    } else {
        format!(" {} ", window_title(&window.frame))
    }
}

fn render_tab_strip(tabs: &[DesktopTab]) -> String {
    tabs.iter()
        .map(|tab| {
            if tab.active {
                format!("[{}]", tab.label)
            } else {
                format!("({})", tab.label)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn draw_window(canvas: &mut [Vec<char>], window: &DesktopWindow) {
    if window.rect.width == 0
        || window.rect.height == 0
        || canvas.is_empty()
        || canvas[0].is_empty()
    {
        return;
    }

    let border = if window.focused { '#' } else { '+' };
    let rect = window.rect;

    // Bounded by the canvas, not by the rectangle. `SurfaceRect` is four
    // `usize`s and `DesktopWindow` derives `Deserialize`: a `DesktopScene`
    // arrives over the wire inside a keyframe, so a viewer rendering a
    // decoded scene walked whatever height the producer claimed. 200 million
    // rows on a 24-row canvas measured at 1.57 s, and `usize::MAX` does not
    // finish at all -- every one of those iterations hit the `continue`
    // below and drew nothing. This is the same clamp Phase 331 put in
    // `graphics_rasterizer`'s rounded shapes; this is the text compositor,
    // which is its sibling and did not get it.
    //
    // Exact rather than conservative: the rows and columns dropped are the
    // ones the bounds checks already skipped, and `is_bottom`/`is_right`
    // still compare against the rectangle's own extent, so the border is
    // drawn in the same cells.
    let rows = canvas.len().saturating_sub(rect.y).min(rect.height);
    for dy in 0..rows {
        let y = rect.y + dy;
        if y >= canvas.len() {
            continue;
        }

        let cols = canvas[y].len().saturating_sub(rect.x).min(rect.width);
        for dx in 0..cols {
            let x = rect.x + dx;
            if x >= canvas[y].len() {
                continue;
            }

            let is_top = dy == 0;
            let is_bottom = dy + 1 == rect.height;
            let is_left = dx == 0;
            let is_right = dx + 1 == rect.width;

            if is_top || is_bottom || is_left || is_right {
                canvas[y][x] = border;
            } else {
                canvas[y][x] = ' ';
            }
        }
    }

    if window.chrome && rect.width > 2 {
        let label = window_chrome_label(window);
        for (offset, ch) in label.chars().take(rect.width - 2).enumerate() {
            put_char(canvas, rect.x + 1 + offset, rect.y, ch);
        }
    }

    let inner_width = rect.width.saturating_sub(2);
    let inner_height = rect.height.saturating_sub(2);
    if inner_width == 0 || inner_height == 0 {
        return;
    }

    for (line_index, line) in render_content_lines(&window.frame.content)
        .into_iter()
        .take(inner_height)
        .enumerate()
    {
        let y = rect.y + 1 + line_index;
        if y >= canvas.len() {
            break;
        }

        for (column, ch) in line.chars().take(inner_width).enumerate() {
            put_char(canvas, rect.x + 1 + column, y, ch);
        }
    }

    if let Some(cursor) = window.frame.cursor {
        if cursor.line < inner_height && cursor.column < inner_width {
            put_char(
                canvas,
                rect.x + 1 + cursor.column,
                rect.y + 1 + cursor.line,
                CURSOR_GLYPH,
            );
        }
    }
}

fn raster_window(
    target: &mut impl RenderTarget,
    window: &DesktopWindow,
    damage_rect: Option<RasterRect>,
    theme: &Theme,
) -> bool {
    // Cut the window down to the surface before anything is computed from
    // it. `DesktopWindow` derives `Deserialize`, so `rect` arrives over the
    // wire and `pixel_rect` only saturates the multiply -- a `usize::MAX`
    // height stays `usize::MAX`, and the twenty `rect.x + ..` and
    // `rect.y + ..` expressions below each had to survive it on their own.
    // They did not: `RasterRect::bottom()` overflowed, which panics in debug
    // and in release wraps to *below* `y`, so `contains` answered false for
    // every pixel and the window rendered as nothing with no error at all.
    let rect = window.bounds().clamped_to(target.width(), target.height());
    if rect.width == 0 || rect.height == 0 {
        return false;
    }

    let clipped_rect = damage_rect
        .map(|damage| rect.intersect(damage))
        .unwrap_or(Some(rect));
    let Some(clipped_rect) = clipped_rect else {
        return false;
    };

    match window.style {
        WindowStyle::Card => return raster_card(target, window, rect, clipped_rect, theme),
        WindowStyle::Dock => return raster_dock(target, window, rect, clipped_rect, theme),
        WindowStyle::TopBar => return raster_top_bar(target, window, rect, clipped_rect, theme),
        WindowStyle::Veil => return raster_veil(target, window, rect, clipped_rect, theme),
        WindowStyle::Classic => {}
    }

    let border_color = if window.focused {
        theme.border_focused
    } else {
        theme.border_unfocused
    };
    {
        let mut window_target = ScissorTarget::new(target, clipped_rect);
        window_target.fill_rect(rect, theme.surface);
        window_target.draw_border(rect, RASTER_BORDER_THICKNESS, border_color);
    }

    if window.chrome {
        raster_title_bar(target, window, rect, clipped_rect, border_color, theme);
    }

    // Content text starts one cell down when a chrome row is present.
    let content_top = if window.chrome {
        rect.y + RASTER_CELL_HEIGHT
    } else {
        rect.y
    };
    if let Some(content_rect) = window_content_rect_for(rect, window.chrome) {
        if let Some(content_clip) = content_rect.intersect(clipped_rect) {
            let target_height = target.height();
            let mut content_target = ScissorTarget::new(target, content_clip);
            let line_origin_y = content_top + 2;
            if let Some(highlight) = window.highlight_line.filter(|l| *l < window.content_rows()) {
                content_target.fill_rect(
                    RasterRect::new(
                        content_rect.x,
                        content_top + 1 + highlight * RASTER_CELL_HEIGHT,
                        content_rect.width,
                        RASTER_CELL_HEIGHT,
                    ),
                    theme.selection,
                );
            }
            if let ViewContent::Graphics { ops } = &window.frame.content {
                // Ops draw in content space: origin at the text origin so
                // graphics and text views align.
                let origin = RasterRect::new(
                    rect.x + 2,
                    content_top + 2,
                    content_clip.right().saturating_sub(rect.x + 2),
                    content_clip.bottom().saturating_sub(content_top + 2),
                );
                if let Some(canvas_rect) = origin.intersect(content_clip) {
                    raster_graphics(&mut content_target, canvas_rect, ops, theme);
                }
            }
            for (line_index, line) in render_content_lines(&window.frame.content)
                .into_iter()
                .take(window.content_rows())
                .enumerate()
            {
                let y = line_origin_y + line_index * RASTER_CELL_HEIGHT;
                if y >= target_height {
                    break;
                }
                content_target.draw_text_with_font(rect.x + 2, y, &line, &DESKTOP_FONT, theme.text);
            }

            if let Some(cursor) = window.frame.cursor {
                // Same origin as the text run above so the caret sits under
                // the glyph it refers to.
                let cursor_x = rect.x + 2 + cursor.column * RASTER_CELL_WIDTH;
                let cursor_y = content_top + 1 + cursor.line * RASTER_CELL_HEIGHT;
                content_target.fill_rect(
                    RasterRect::new(cursor_x, cursor_y, 4, RASTER_CELL_HEIGHT.saturating_sub(2)),
                    theme.caret,
                );
            }
        }
    }

    true
}

/// Paint `rect` with the desk background: `background` at row 0 fading to
/// `background_bottom` at the last row of a `surface_height`-tall surface.
/// A theme whose two colours are equal gets one plain fill.
fn fill_background(
    target: &mut impl RenderTarget,
    rect: RasterRect,
    surface_height: usize,
    theme: &Theme,
    wallpaper: Option<Wallpaper>,
) {
    // A wallpaper (GFX-066) is sampled per pixel, so a damage repaint of
    // any rectangle reads the same pixels the full frame did.
    if let Some(picture) = wallpaper {
        let surface_w = target.width();
        for y in rect.y..rect.bottom().min(surface_height) {
            for x in rect.x..rect.right().min(surface_w) {
                target.write_pixel(x, y, picture.sample(x, y, surface_w, surface_height));
            }
        }
        return;
    }
    let (top, bottom) = (theme.background, theme.background_bottom);
    if top == bottom || surface_height <= 1 {
        target.fill_rect(rect, top);
        return;
    }
    let span = (surface_height - 1) as u32;
    let lerp = |a: u8, b: u8, y: usize| -> u8 {
        let y = y.min(surface_height - 1) as u32;
        ((a as u32 * (span - y) + b as u32 * y) / span) as u8
    };
    for y in rect.y..rect.bottom().min(surface_height) {
        let color = RgbaColor::new(
            lerp(top.r, bottom.r, y),
            lerp(top.g, bottom.g, y),
            lerp(top.b, bottom.b, y),
            255,
        );
        target.fill_rect(RasterRect::new(rect.x, y, rect.width, 1), color);
    }
}

/// A card's resize grip: the bottom-right corner (GFX-052).
pub const CARD_GRIP_SIZE: usize = 14;

/// A card's content rectangle: inside the padding, below the header, above
/// the footer (GFX-050).
fn card_content_rect(window: &DesktopWindow) -> Option<RasterRect> {
    let bounds = window.bounds();
    let footer = if window.footer.is_some() {
        CARD_FOOTER_HEIGHT
    } else {
        0
    };
    let top = bounds.y + CARD_HEADER_HEIGHT + CARD_PADDING;
    let bottom = bounds.bottom().saturating_sub(CARD_PADDING + footer);
    if top >= bottom || bounds.width <= CARD_PADDING * 2 {
        return None;
    }
    Some(RasterRect::new(
        bounds.x + CARD_PADDING,
        top,
        bounds.width - CARD_PADDING * 2,
        bottom - top,
    ))
}

/// A dock tile's rectangle, or `None` past the end of `tabs`.
fn dock_tile_rect(window: &DesktopWindow, index: usize) -> Option<RasterRect> {
    if index >= window.tabs.len() {
        return None;
    }
    let bounds = window.bounds();
    let count = window.tabs.len();
    let row_width = count * DOCK_TILE + count.saturating_sub(1) * DOCK_TILE_GAP;
    let start_x = bounds.x + bounds.width.saturating_sub(row_width) / 2;
    let y = bounds.y + bounds.height.saturating_sub(DOCK_TILE) / 2;
    Some(RasterRect::new(
        start_x + index * (DOCK_TILE + DOCK_TILE_GAP),
        y,
        DOCK_TILE,
        DOCK_TILE,
    ))
}

/// Hit regions for the desk styles; `None` hands the classic geometry the
/// decision.
fn hit_region_for_style(window: &DesktopWindow, x: usize, y: usize) -> Option<HitRegion> {
    match window.style {
        WindowStyle::Classic => None,
        WindowStyle::Card => {
            if window.close_rect().is_some_and(|r| r.contains(x, y)) {
                return Some(HitRegion::Close);
            }
            if let Some(index) = window
                .action_rects()
                .iter()
                .position(|r| r.is_some_and(|r| r.contains(x, y)))
            {
                return Some(HitRegion::Action { index });
            }
            if window.grip_rect().is_some_and(|r| r.contains(x, y)) {
                return Some(HitRegion::Resize);
            }
            if window.header_rect().is_some_and(|r| r.contains(x, y)) {
                return Some(HitRegion::Header);
            }
            if card_content_rect(window).is_some_and(|c| c.contains(x, y)) {
                let (origin_x, origin_y, pitch) = window.card_text_origin();
                return Some(HitRegion::Content {
                    line: y.saturating_sub(origin_y) / pitch,
                    column: x.saturating_sub(origin_x) / RASTER_CELL_WIDTH,
                });
            }
            Some(HitRegion::Border)
        }
        WindowStyle::Dock => {
            let index = (0..window.tabs.len())
                .find(|&i| dock_tile_rect(window, i).is_some_and(|r| r.contains(x, y)));
            Some(match index {
                Some(index) => HitRegion::DockTile { index },
                None => HitRegion::Border,
            })
        }
        // The bar's text cells, so the desk can tell what was clicked
        // (GFX-067): the space strip, the notices, or the rest.
        WindowStyle::TopBar | WindowStyle::Veil => Some(HitRegion::Content {
            line: 0,
            column: x.saturating_sub(window.bounds().x) / RASTER_CELL_WIDTH.max(1),
        }),
    }
}

/// Paint a desk card (GFX-050): shadow, body, hairline or accent ring,
/// header with title and close glyph, content lines with caret, footer.
fn raster_card(
    target: &mut impl RenderTarget,
    window: &DesktopWindow,
    rect: RasterRect,
    clipped_rect: RasterRect,
    theme: &Theme,
) -> bool {
    // The lift extends two pixels past the card's bounds, so the clip has
    // to as well -- a scissor cut to the bounds alone clipped the shadow
    // away entirely, and the first pixel test caught it.
    let lift = RasterRect::new(
        clipped_rect.x,
        clipped_rect.y,
        clipped_rect.width + CARD_LIFT,
        clipped_rect.height + CARD_LIFT,
    )
    .clamped_to(target.width(), target.height());
    let mut painter = ScissorTarget::new(target, lift);

    // Lift: a solid darker shape two pixels down and right. Fills overwrite
    // on this rasterizer, so this is the honest version of a shadow.
    let shadow = RasterRect::new(
        rect.x + CARD_LIFT,
        rect.y + CARD_LIFT,
        rect.width,
        rect.height,
    );
    painter.fill_rounded_rect(shadow, CARD_RADIUS, theme.shadow);
    painter.fill_rounded_rect(rect, CARD_RADIUS, theme.surface);

    // Focus is a ring, not a flooded title bar.
    let (ring, thickness) = if window.focused {
        (theme.accent, 2)
    } else {
        (theme.hairline, 1)
    };
    painter.draw_rounded_border(rect, CARD_RADIUS, thickness, ring);

    // Header: title at the left, a hairline under it, the close glyph at
    // the right. The header is the same surface as the body -- calm, not a
    // colour-flooded strip.
    let title_color = if window.focused {
        theme.text
    } else {
        theme.text_muted
    };
    let title_y = rect.y + (CARD_HEADER_HEIGHT.saturating_sub(DESKTOP_FONT.glyph_height())) / 2;
    let close = window.close_rect();
    let chips = window.action_rects();
    let chips_left = chips.iter().flatten().map(|r| r.x).min();
    let title_room = chips_left
        .or(close.map(|c| c.x))
        .map(|x| x.saturating_sub(rect.x + CARD_PADDING + 4 + CARD_CHIP_GAP))
        .unwrap_or(rect.width.saturating_sub(CARD_PADDING * 2));
    let title = window_chrome_label(window);
    let title = fit_text(&title, title_room / RASTER_CELL_WIDTH.max(1));
    painter.draw_text_with_font(
        rect.x + CARD_PADDING + 4,
        title_y,
        &title,
        &SMOOTH_FONT,
        title_color,
    );
    let header_bottom = rect.y + CARD_HEADER_HEIGHT;
    if header_bottom < rect.bottom() {
        painter.draw_hline(
            rect.x + thickness,
            header_bottom,
            rect.width.saturating_sub(thickness * 2),
            theme.hairline,
        );
    }
    // Header action chips: raised pills with a hairline, muted text.
    for (label, chip) in window.actions.iter().zip(chips.iter()) {
        let Some(chip) = chip else { continue };
        painter.fill_rounded_rect(*chip, 4, theme.surface_raised);
        painter.draw_rounded_border(*chip, 4, 1, theme.hairline);
        let ty = chip.y + (chip.height.saturating_sub(DESKTOP_FONT.glyph_height())) / 2;
        painter.draw_text_with_font(
            chip.x + CARD_CHIP_PAD,
            ty,
            label,
            &SMOOTH_FONT,
            if window.focused {
                theme.text
            } else {
                theme.text_muted
            },
        );
    }
    if let Some(close) = close {
        // An 'x' in the one font there is, centred in its hit box.
        let gx = close.x + (CARD_CLOSE_SIZE.saturating_sub(RASTER_CELL_WIDTH)) / 2;
        let gy = close.y + (CARD_CLOSE_SIZE.saturating_sub(DESKTOP_FONT.glyph_height())) / 2;
        painter.draw_text_with_font(gx, gy, "x", &SMOOTH_FONT, theme.text_muted);
    }

    // Content.
    if let Some(content) = card_content_rect(window) {
        if let Some(content_clip) = content.intersect(clipped_rect) {
            let (origin_x, origin_y, pitch) = window.card_text_origin();
            let rows = window.content_rows();
            let mut content_painter = ScissorTarget::new(&mut painter, content_clip);
            if let Some(highlight) = window.highlight_line.filter(|l| *l < rows) {
                content_painter.fill_rect(
                    RasterRect::new(
                        content.x,
                        origin_y + highlight * pitch,
                        content.width,
                        pitch,
                    ),
                    theme.selection,
                );
            }
            for &(line, start, end) in &window.selection_spans {
                if line >= rows || end <= start {
                    continue;
                }
                content_painter.fill_rect(
                    RasterRect::new(
                        origin_x + start * RASTER_CELL_WIDTH,
                        origin_y + line * pitch,
                        (end - start) * RASTER_CELL_WIDTH,
                        pitch,
                    ),
                    theme.selection,
                );
            }
            if let ViewContent::Graphics { ops } = &window.frame.content {
                raster_graphics(&mut content_painter, content_clip, ops, theme);
            }
            let text_y_offset = (pitch.saturating_sub(DESKTOP_FONT.glyph_height())) / 2;
            for (line_index, line) in render_content_lines(&window.frame.content)
                .into_iter()
                .take(rows)
                .enumerate()
            {
                let style = window
                    .line_styles
                    .iter()
                    .find(|(l, _)| *l == line_index)
                    .map(|(_, s)| *s)
                    .unwrap_or_default();
                let color = match style.tone {
                    LineTone::Text => theme.text,
                    LineTone::Accent => theme.accent,
                    LineTone::Muted => theme.text_muted,
                };
                let y = origin_y + line_index * pitch + text_y_offset;
                content_painter.draw_text_with_font(origin_x, y, &line, &SMOOTH_FONT, color);
                if style.bold {
                    // A second strike one pixel right: the weight one font
                    // can give (GFX-073).
                    content_painter.draw_text_with_font(
                        origin_x + 1,
                        y,
                        &line,
                        &SMOOTH_FONT,
                        color,
                    );
                }
                if style.underline {
                    let width = line.chars().count() * RASTER_CELL_WIDTH + 1;
                    content_painter.draw_hline(
                        origin_x,
                        y + DESKTOP_FONT.glyph_height() + 1,
                        width,
                        color,
                    );
                }
            }
            if !window.overlay.is_empty() {
                raster_graphics(&mut content_painter, content_clip, &window.overlay, theme);
            }
            if let Some(cursor) = window.frame.cursor {
                content_painter.fill_rect(
                    RasterRect::new(
                        origin_x + cursor.column * RASTER_CELL_WIDTH,
                        origin_y + cursor.line * pitch + 2,
                        2,
                        pitch.saturating_sub(4),
                    ),
                    theme.accent,
                );
            }
        }
    }

    // Resize grip: two short diagonals in the bottom-right corner, in the
    // hairline colour so it reads as texture rather than as a control.
    if let Some(grip) = window.grip_rect() {
        let (cx, cy) = (
            grip.right().saturating_sub(4),
            grip.bottom().saturating_sub(4),
        );
        for step in [0usize, 4] {
            painter.draw_line(
                (cx.saturating_sub(8 - step)) as i64,
                cy as i64,
                cx as i64,
                (cy.saturating_sub(8 - step)) as i64,
                theme.hairline,
            );
        }
    }

    // Footer: muted, above the bottom edge, under a hairline.
    if let Some(footer) = &window.footer {
        let top = rect.bottom().saturating_sub(CARD_FOOTER_HEIGHT);
        if top > rect.y + CARD_HEADER_HEIGHT {
            painter.draw_hline(
                rect.x + thickness,
                top,
                rect.width.saturating_sub(thickness * 2),
                theme.hairline,
            );
            let text_y = top + (CARD_FOOTER_HEIGHT.saturating_sub(DESKTOP_FONT.glyph_height())) / 2;
            let room = rect.width.saturating_sub(CARD_PADDING * 2) / RASTER_CELL_WIDTH.max(1);
            painter.draw_text_with_font(
                rect.x + CARD_PADDING,
                text_y,
                &fit_text(footer, room),
                &SMOOTH_FONT,
                theme.text_muted,
            );
        }
    }
    true
}

/// Paint the dock (GFX-050): a raised pill holding one rounded tile per
/// `tabs` entry, the label as a monogram, a dot under the running ones.
fn raster_dock(
    target: &mut impl RenderTarget,
    window: &DesktopWindow,
    rect: RasterRect,
    clipped_rect: RasterRect,
    theme: &Theme,
) -> bool {
    let mut painter = ScissorTarget::new(target, clipped_rect);
    painter.fill_rounded_rect(rect, DOCK_RADIUS, theme.surface_raised);
    painter.draw_rounded_border(rect, DOCK_RADIUS, 1, theme.hairline);
    for (index, tab) in window.tabs.iter().enumerate() {
        let Some(tile) = dock_tile_rect(window, index) else {
            break;
        };
        let fill = if tab.hovered {
            theme.selection
        } else {
            theme.tab_inactive
        };
        // A picture is the tile (GFX-094): drawn alone, with a halo
        // behind it under the pointer.
        let picture = tab
            .picture
            .and_then(|id| theme.pictures.get(id as usize).copied());
        if let Some(picture) = picture {
            if tab.hovered {
                let halo = RasterRect::new(
                    tile.x.saturating_sub(3),
                    tile.y.saturating_sub(3),
                    tile.width + 6,
                    tile.height + 6,
                );
                painter.fill_rounded_rect(halo, DOCK_TILE_RADIUS + 3, theme.selection);
            }
            let px = tile.x + DOCK_TILE.saturating_sub(picture.width as usize) / 2;
            let py = tile.y + DOCK_TILE.saturating_sub(picture.height as usize) / 2;
            draw_picture(&mut painter, px, py, &picture);
        } else {
            painter.fill_rounded_rect(tile, DOCK_TILE_RADIUS, fill);
            if let Some(icon) = tab.icon {
                // The icon, two pixels a bit, centred (GFX-084).
                let ox = tile.x + (DOCK_TILE.saturating_sub(DOCK_ICON)) / 2;
                let oy = tile.y + (DOCK_TILE.saturating_sub(DOCK_ICON)) / 2;
                for (row, bits) in icon.iter().enumerate() {
                    for col in 0..16 {
                        if bits & (1 << (15 - col)) != 0 {
                            painter.fill_rect(
                                RasterRect::new(ox + col * 2, oy + row * 2, 2, 2),
                                theme.text,
                            );
                        }
                    }
                }
            } else {
                let monogram: String = tab.label.chars().take(2).collect();
                let text_w = monogram.chars().count() * RASTER_CELL_WIDTH;
                painter.draw_text_with_font(
                    tile.x + (DOCK_TILE.saturating_sub(text_w)) / 2,
                    tile.y + (DOCK_TILE.saturating_sub(DESKTOP_FONT.glyph_height())) / 2,
                    &monogram,
                    &SMOOTH_FONT,
                    theme.text,
                );
            }
        }
        if tab.active {
            let dot = RasterRect::new(tile.x + DOCK_TILE / 2 - 3, tile.bottom() + 2, 6, 6);
            if tab.tucked {
                painter.draw_rounded_border(dot, 3, 1, theme.accent);
            } else {
                painter.fill_rounded_rect(dot, 3, theme.accent);
            }
        }
    }
    true
}

/// Paint the top bar (GFX-050): raised strip, hairline under it, the title
/// at the left and the first content line at the right.
fn raster_top_bar(
    target: &mut impl RenderTarget,
    window: &DesktopWindow,
    rect: RasterRect,
    clipped_rect: RasterRect,
    theme: &Theme,
) -> bool {
    let mut painter = ScissorTarget::new(target, clipped_rect);
    painter.fill_rect(rect, theme.surface_raised);
    if rect.height > 0 {
        painter.draw_hline(rect.x, rect.bottom() - 1, rect.width, theme.hairline);
    }
    let text_y = rect.y + (rect.height.saturating_sub(DESKTOP_FONT.glyph_height())) / 2;
    let left = window.frame.title.clone().unwrap_or_default();
    // The title sits right of the desk's mark (GFX-094), which the desk
    // draws in the bar's overlay.
    painter.draw_text_with_font(
        rect.x + TOP_BAR_TITLE_X,
        text_y,
        &left,
        &SMOOTH_FONT,
        theme.text,
    );
    let lines = render_content_lines(&window.frame.content);
    if let Some(right) = lines.first() {
        let width = right.chars().count() * RASTER_CELL_WIDTH;
        let x = rect.right().saturating_sub(width + 12);
        painter.draw_text_with_font(x, text_y, right, &SMOOTH_FONT, theme.text_muted);
    }
    // A second content line is centred (GFX-067): the space strip. Its
    // cells are what `top_bar_centre_column` reports, so a click lands on
    // the character the eye sees.
    if let Some(centre) = lines.get(1) {
        let column = top_bar_centre_column(rect.width, centre.chars().count());
        painter.draw_text_with_font(
            rect.x + column * RASTER_CELL_WIDTH,
            text_y,
            centre,
            &SMOOTH_FONT,
            theme.text,
        );
    }
    // The tray's pictures (GFX-088): a badge, a meter, over the text,
    // in the bar's own pixels.
    if !window.overlay.is_empty() {
        raster_graphics(&mut painter, rect, &window.overlay, theme);
    }
    true
}

/// How dark a veil makes what is under it (GFX-096), out of 255.
pub const VEIL_ALPHA: u8 = 176;

/// Paint a veil (GFX-096): the theme's shadow colour laid over what is
/// already there at `VEIL_ALPHA`, then the overlay.
fn raster_veil(
    target: &mut impl RenderTarget,
    window: &DesktopWindow,
    rect: RasterRect,
    clipped_rect: RasterRect,
    theme: &Theme,
) -> bool {
    let shade = RgbaColor::new(theme.shadow.r, theme.shadow.g, theme.shadow.b, VEIL_ALPHA);
    for y in clipped_rect.y..clipped_rect.bottom().min(target.height()) {
        for x in clipped_rect.x..clipped_rect.right().min(target.width()) {
            let under = target.pixel(x, y).unwrap_or(theme.background);
            target.write_pixel(x, y, graphics_rasterizer::blend_over(under, shade));
        }
    }
    if !window.overlay.is_empty() {
        let mut painter = ScissorTarget::new(target, clipped_rect);
        raster_graphics(&mut painter, rect, &window.overlay, theme);
    }
    true
}

/// Where the top bar's title starts (GFX-094): after a 20px mark at x=12.
pub const TOP_BAR_TITLE_X: usize = 40;

/// Draw `picture` with its top-left at `(x, y)`, each pixel blended over
/// what is there by its alpha (GFX-094).
pub fn draw_picture(
    target: &mut (impl RenderTarget + ?Sized),
    x: usize,
    y: usize,
    picture: &Picture,
) {
    let (w, h) = (picture.width as usize, picture.height as usize);
    if picture.rgba.len() < w * h * 4 {
        return;
    }
    for row in 0..h {
        let ty = y + row;
        if ty >= target.height() {
            break;
        }
        for col in 0..w {
            let tx = x + col;
            if tx >= target.width() {
                break;
            }
            let at = (row * w + col) * 4;
            let a = picture.rgba[at + 3];
            if a == 0 {
                continue;
            }
            let src = RgbaColor::new(
                picture.rgba[at],
                picture.rgba[at + 1],
                picture.rgba[at + 2],
                a,
            );
            let color = if a == 255 {
                src
            } else {
                let under = target.pixel(tx, ty).unwrap_or(RgbaColor::new(0, 0, 0, 255));
                graphics_rasterizer::blend_over(under, src)
            };
            target.write_pixel(tx, ty, color);
        }
    }
}

/// The first cell of a centred top-bar text `chars` wide on a bar
/// `bar_width` pixels wide.
pub fn top_bar_centre_column(bar_width: usize, chars: usize) -> usize {
    (bar_width / RASTER_CELL_WIDTH).saturating_sub(chars) / 2
}

/// `text` cut to `max_chars`, with a trailing ellipsis mark when cut.
fn fit_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    if max_chars > 0 {
        out.push('~');
    }
    out
}

/// Title bar fill for a window: role first, then focus.
fn title_fill(window: &DesktopWindow, theme: &Theme) -> RgbaColor {
    match window.role {
        DesktopWindowRole::Notification => theme.title_notice,
        DesktopWindowRole::Palette => theme.title_palette,
        _ if window.focused => theme.title_focused,
        _ => theme.title_unfocused,
    }
}

/// Graphical title bar (GFX-032): a tinted strip under the top border, a
/// separator line in the border colour (the focus ring continues through it),
/// and either the title text or a strip of rounded tab boxes.
fn raster_title_bar(
    target: &mut impl RenderTarget,
    window: &DesktopWindow,
    rect: RasterRect,
    clipped_rect: RasterRect,
    border_color: RgbaColor,
    theme: &Theme,
) {
    // Separator between the title bar and the content; for the focused
    // window this carries the accent so the ring reads as one shape. It is
    // painted first and on its own clip because it lies just below the
    // chrome rectangle: a damage region that misses the chrome row must
    // still restore it.
    let separator_y = rect.y + RASTER_CELL_HEIGHT;
    if separator_y < rect.y.saturating_add(rect.height) {
        let mut line_target = ScissorTarget::new(target, clipped_rect);
        line_target.draw_hline(
            rect.x + RASTER_BORDER_THICKNESS,
            separator_y,
            rect.width.saturating_sub(RASTER_BORDER_THICKNESS * 2),
            border_color,
        );
    }

    let Some(chrome_rect) = window_chrome_rect(rect).intersect(clipped_rect) else {
        return;
    };
    let title_text = if window.focused {
        theme.text
    } else {
        theme.text_muted
    };
    let mut chrome_target = ScissorTarget::new(target, chrome_rect);
    chrome_target.fill_rect(window_chrome_rect(rect), title_fill(window, theme));

    if window.tabs.is_empty() {
        let title = window_chrome_label(window);
        chrome_target.draw_text_with_font(
            rect.x + 2,
            rect.y + 2,
            &title,
            &DESKTOP_FONT,
            title_text,
        );
    } else {
        let tab_top = rect.y + RASTER_BORDER_THICKNESS + 1;
        let tab_height = RASTER_CELL_HEIGHT.saturating_sub(RASTER_BORDER_THICKNESS + 1);
        let mut x = rect.x + 2;
        for tab in &window.tabs {
            let width = (tab.label.chars().count() + 2) * RASTER_CELL_WIDTH;
            let body = RasterRect::new(x, tab_top, width, tab_height);
            let (fill, text) = if tab.active {
                (theme.tab_active, theme.text)
            } else {
                (theme.tab_inactive, theme.text_muted)
            };
            chrome_target.fill_rounded_rect(body, TAB_RADIUS, fill);
            chrome_target.draw_text_with_font(
                x + RASTER_CELL_WIDTH,
                rect.y + 2,
                &tab.label,
                &DESKTOP_FONT,
                text,
            );
            x += width + 2;
        }
    }
}

fn pixel_rect(rect: SurfaceRect) -> RasterRect {
    RasterRect::new(
        rect.x.saturating_mul(RASTER_CELL_WIDTH),
        rect.y.saturating_mul(RASTER_CELL_HEIGHT),
        rect.width.saturating_mul(RASTER_CELL_WIDTH),
        rect.height.saturating_mul(RASTER_CELL_HEIGHT),
    )
}

fn window_chrome_rect(rect: RasterRect) -> RasterRect {
    RasterRect::new(
        rect.x + RASTER_BORDER_THICKNESS,
        rect.y + RASTER_BORDER_THICKNESS,
        rect.width.saturating_sub(RASTER_BORDER_THICKNESS * 2),
        RASTER_CELL_HEIGHT.saturating_sub(RASTER_BORDER_THICKNESS),
    )
}

fn window_content_rect_for(rect: RasterRect, chrome: bool) -> Option<RasterRect> {
    if chrome {
        return window_content_rect(rect);
    }
    let y = rect.y + RASTER_BORDER_THICKNESS;
    let bottom = rect.y.saturating_add(rect.height);
    if y >= bottom {
        return None;
    }
    Some(RasterRect::new(
        rect.x + RASTER_BORDER_THICKNESS,
        y,
        rect.width.saturating_sub(RASTER_BORDER_THICKNESS * 2),
        bottom - y - RASTER_BORDER_THICKNESS,
    ))
}

fn window_content_rect(rect: RasterRect) -> Option<RasterRect> {
    let y = rect.y + RASTER_CELL_HEIGHT + RASTER_BORDER_THICKNESS;
    let bottom = rect.y.saturating_add(rect.height);
    if y >= bottom {
        return None;
    }

    Some(RasterRect::new(
        rect.x + RASTER_BORDER_THICKNESS,
        y,
        rect.width.saturating_sub(RASTER_BORDER_THICKNESS * 2),
        bottom - y - RASTER_BORDER_THICKNESS,
    ))
}

fn put_char(canvas: &mut [Vec<char>], x: usize, y: usize, ch: char) {
    if let Some(row) = canvas.get_mut(y) {
        if let Some(cell) = row.get_mut(x) {
            *cell = ch;
        }
    }
}

#[cfg(feature = "workspace")]
fn workspace_tile_windows(
    size: SurfaceSize,
    snapshot: &WorkspaceRenderSnapshot,
) -> Vec<DesktopWindow> {
    let mut tiles = snapshot.tiles.iter().collect::<Vec<_>>();
    tiles.sort_by_key(|tile| tile.tile_index);

    let tile_count = tiles.len();
    if tile_count == 0 {
        return Vec::new();
    }

    let rects = match snapshot.layout.split_axis {
        Some(SplitAxis::Vertical) => partition_rects_vertical(size, tile_count),
        _ => partition_rects_horizontal(size, tile_count),
    };

    tiles
        .into_iter()
        .zip(rects)
        .map(|(tile, rect)| {
            let (frame, role, tabs) = tile_window_frame(tile, tile.tile_index);
            let mut window = DesktopWindow::new(frame, rect)
                .with_role(role)
                .with_tabs(tabs)
                .with_z_index(tile.tile_index);
            if tile.is_focused {
                window = window.focused();
            }
            window
        })
        .collect()
}

#[cfg(feature = "workspace")]
fn tile_window_frame(
    tile: &WorkspaceTileRenderSnapshot,
    tile_index: usize,
) -> (ViewFrame, DesktopWindowRole, Vec<DesktopTab>) {
    let (mut frame, role) = if let Some(frame) = tile.main_view.clone() {
        (frame, DesktopWindowRole::Main)
    } else if let Some(frame) = tile.status_view.clone() {
        (frame, DesktopWindowRole::Status)
    } else {
        (
            ViewFrame::new(
                ViewId::new(),
                ViewKind::Panel,
                1,
                ViewContent::panel("[empty tile]"),
                0,
            ),
            DesktopWindowRole::Main,
        )
    };

    if frame.title.is_none() {
        let title = if tile.tabs.len() > 1 {
            format!("Tile {} [{} tabs]", tile_index + 1, tile.tabs.len())
        } else {
            format!("Tile {}", tile_index + 1)
        };
        frame = frame.with_title(title);
    }

    let tabs = tile_window_tabs(tile, &frame);

    (frame, role, tabs)
}

#[cfg(feature = "workspace")]
fn tile_window_tabs(tile: &WorkspaceTileRenderSnapshot, frame: &ViewFrame) -> Vec<DesktopTab> {
    if tile.tabs.is_empty() {
        return Vec::new();
    }

    let active_component = tile.active_component;
    let active_label = frame.title.clone().unwrap_or_else(|| "Active".to_string());

    tile.tabs
        .iter()
        .enumerate()
        .map(|(index, component_id)| {
            let active = Some(*component_id) == active_component;
            let label = if active {
                active_label.clone()
            } else {
                format!("Tab {}", index + 1)
            };
            DesktopTab::new(label, active)
        })
        .collect()
}

#[cfg(feature = "workspace")]
fn partition_rects_vertical(size: SurfaceSize, count: usize) -> Vec<SurfaceRect> {
    let slices = partition_extent(size.width, count);
    slices
        .into_iter()
        .map(|(x, width)| SurfaceRect::new(x, 0, width, size.height))
        .collect()
}

#[cfg(feature = "workspace")]
fn partition_rects_horizontal(size: SurfaceSize, count: usize) -> Vec<SurfaceRect> {
    let slices = partition_extent(size.height, count);
    slices
        .into_iter()
        .map(|(y, height)| SurfaceRect::new(0, y, size.width, height))
        .collect()
}

#[cfg(feature = "workspace")]
fn partition_extent(total: usize, count: usize) -> Vec<(usize, usize)> {
    if count == 0 {
        return Vec::new();
    }

    let base = total / count;
    let remainder = total % count;
    let mut offset = 0;
    let mut slices = Vec::with_capacity(count);
    for index in 0..count {
        let len = base + usize::from(index < remainder);
        slices.push((offset, len));
        offset += len;
    }
    slices
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphics_rasterizer::{LinearFramebufferTarget, LinearPixelFormat};
    use services_workspace_manager::{
        ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
    };
    use view_types::{CursorPosition, ViewId, ViewKind};

    fn raster_surface_to_golden(surface: &RasterSurfaceFrame) -> String {
        let mut rows = Vec::with_capacity(surface.height);
        for y in 0..surface.height {
            let mut row = String::with_capacity(surface.width);
            for x in 0..surface.width {
                let ch = match surface.pixel(x, y) {
                    Some(color) if color == DESKTOP_BACKGROUND_COLOR => '.',
                    Some(color) if color == WINDOW_FILL_COLOR => 'f',
                    Some(color) if color == FOCUSED_BORDER_COLOR => '#',
                    Some(color) if color == UNFOCUSED_BORDER_COLOR => '+',
                    Some(color) if color == TEXT_COLOR => 't',
                    Some(color) if color == CURSOR_COLOR => '@',
                    Some(color) if color == Theme::DEFAULT.title_focused => 'T',
                    Some(color) if color == Theme::DEFAULT.title_unfocused => 'u',
                    Some(color) if color == Theme::DEFAULT.title_notice => 'n',
                    Some(color) if color == Theme::DEFAULT.title_palette => 'p',
                    Some(color) if color == Theme::DEFAULT.text_muted => 'm',
                    Some(color) if color == Theme::DEFAULT.tab_inactive => 'i',
                    Some(color) if color == Theme::DEFAULT.selection => 'h',
                    Some(_) => '?',
                    None => '!',
                };
                row.push(ch);
            }
            rows.push(row);
        }

        rows.join("\n")
    }

    /// Compare against a checked-in fixture. Set `PANDAGEN_UPDATE_GOLDEN=1` to
    /// rewrite the fixture from the current output instead (review the diff).
    fn assert_raster_golden(surface: &RasterSurfaceFrame, fixture: &str, expected: &str) {
        let actual = raster_surface_to_golden(surface);
        if std::env::var_os("PANDAGEN_UPDATE_GOLDEN").is_some() {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/golden")
                .join(fixture);
            std::fs::write(&path, format!("{actual}\n")).expect("write golden fixture");
            return;
        }
        let expected = expected.trim_end();
        assert!(
            actual == expected,
            "golden raster mismatch (set PANDAGEN_UPDATE_GOLDEN=1 to regenerate)\n--- actual ---\n{actual}\n--- expected ---\n{expected}"
        );
    }

    fn sample_workspace_snapshot_for_golden() -> WorkspaceRenderSnapshot {
        let left_component = ComponentId::new();
        let right_component = ComponentId::new();
        let left = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            2,
            ViewContent::text_buffer(vec!["left".to_string(), "cursor".to_string()]),
            20,
        )
        .with_title("Editor")
        .with_cursor(CursorPosition::new(1, 2));
        let right = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            5,
            ViewContent::panel("notify"),
            50,
        )
        .with_title("Panel");

        WorkspaceRenderSnapshot {
            focused_component: Some(left_component),
            main_view: Some(left.clone()),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: Some(SplitAxis::Vertical),
                focused_tile: 0,
                tiles: vec![
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 0,
                        is_focused: true,
                        active_component: Some(left_component),
                        tabs: vec![left_component, right_component],
                    },
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 1,
                        is_focused: false,
                        active_component: Some(right_component),
                        tabs: vec![right_component],
                    },
                ],
            },
            tiles: vec![
                WorkspaceTileRenderSnapshot {
                    tile_index: 0,
                    is_focused: true,
                    active_component: Some(left_component),
                    tabs: vec![left_component, right_component],
                    main_view: Some(left),
                    status_view: None,
                },
                WorkspaceTileRenderSnapshot {
                    tile_index: 1,
                    is_focused: false,
                    active_component: Some(right_component),
                    tabs: vec![right_component],
                    main_view: Some(right),
                    status_view: None,
                },
            ],
            component_count: 2,
            running_count: 2,
            status_strip: "Graphics".to_string(),
            breadcrumbs: "PANDA/desktop".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        }
    }

    #[test]
    fn test_compositor_renders_frames() {
        let compositor = Compositor::new();
        let frame1 = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["hello".to_string()]),
            10,
        )
        .with_title("Editor");

        let frame2 = ViewFrame::new(
            ViewId::new(),
            ViewKind::StatusLine,
            1,
            ViewContent::status_line("ready"),
            12,
        )
        .with_title("Status");

        let surface = compositor.compose(vec![frame1, frame2]);
        assert_eq!(surface.width, 8);
        assert_eq!(surface.height, 4);
        assert_eq!(surface.frame_count, 2);
        assert!(surface.content.contains("Editor"));
        assert!(surface.content.contains("ready"));
        assert_eq!(surface.timestamp_ns, 12);
    }

    #[test]
    fn test_compose_desktop_renders_window_chrome_and_cursor() {
        let compositor = Compositor::new();
        let editor = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            3,
            ViewContent::text_buffer(vec!["hello".to_string(), "world".to_string()]),
            33,
        )
        .with_title("Editor")
        .with_cursor(CursorPosition::new(1, 2));

        let surface = compositor.compose_desktop(
            SurfaceSize::new(16, 8),
            vec![DesktopWindow::new(editor, SurfaceRect::new(1, 1, 10, 5))
                .with_z_index(1)
                .focused()],
        );

        assert_eq!(surface.width, 16);
        assert_eq!(surface.height, 8);
        assert_eq!(surface.frame_count, 1);
        assert_eq!(surface.timestamp_ns, 33);
        assert_eq!(surface.rows[0], "................");
        assert_eq!(surface.rows[1], ".# Editor #.....");
        assert_eq!(surface.rows[2], ".#hello   #.....");
        assert_eq!(surface.rows[3], ".#wo@ld   #.....");
        assert_eq!(surface.rows[4], ".#        #.....");
        assert_eq!(surface.rows[5], ".##########.....");
    }

    #[test]
    fn test_compose_desktop_honors_window_z_order() {
        let compositor = Compositor::new();
        let back = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["back".to_string()]),
            10,
        )
        .with_title("Back");
        let front = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            2,
            ViewContent::text_buffer(vec!["front".to_string()]),
            20,
        )
        .with_title("Top");

        let surface = compositor.compose_desktop(
            SurfaceSize::new(14, 7),
            vec![
                DesktopWindow::new(back, SurfaceRect::new(0, 1, 8, 4)).with_z_index(0),
                DesktopWindow::new(front, SurfaceRect::new(4, 2, 7, 4))
                    .with_z_index(5)
                    .focused(),
            ],
        );

        assert_eq!(surface.timestamp_ns, 20);
        assert_eq!(surface.rows[2], "+bac# Top #...");
        assert_eq!(surface.rows[3], "+   #front#...");
    }

    #[test]
    fn test_compose_desktop_clips_windows_at_surface_edge() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::StatusLine,
            4,
            ViewContent::status_line("status-ready"),
            44,
        )
        .with_title("Status");

        let surface = compositor.compose_desktop(
            SurfaceSize::new(12, 6),
            vec![DesktopWindow::new(frame, SurfaceRect::new(8, 3, 8, 4)).with_z_index(2)],
        );

        assert_eq!(surface.rows[3], "........+ St");
        assert_eq!(surface.rows[4], "........+sta");
        assert_eq!(surface.rows[5], "........+   ");
        assert!(surface.rows.iter().all(|row| row.len() == 12));
    }

    #[test]
    fn test_compose_desktop_rgba_renders_border_background_and_cursor() {
        let compositor = Compositor::new();
        let editor = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            3,
            ViewContent::text_buffer(vec!["A".to_string()]),
            33,
        )
        .with_title("Editor")
        .with_cursor(CursorPosition::new(0, 0));

        let surface = compositor.compose_desktop_rgba(
            SurfaceSize::new(8, 5),
            vec![DesktopWindow::new(editor, SurfaceRect::new(1, 1, 4, 3))
                .with_z_index(1)
                .focused()],
        );

        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        assert_eq!(surface.width, 8 * w);
        assert_eq!(surface.height, 5 * h);
        assert_eq!(surface.frame_count, 1);
        assert_eq!(surface.timestamp_ns, 33);
        assert_eq!(surface.pixel(0, 0), Some(DESKTOP_BACKGROUND_COLOR));
        // Window rect in pixels: (w, h) .. (5w, 4h).
        assert_eq!(surface.pixel(w, h), Some(FOCUSED_BORDER_COLOR));
        // Right side of the content area holds no text or cursor.
        let content_y = h + h + 2;
        assert_eq!(
            surface.pixel(5 * w - 3, content_y + 8),
            Some(WINDOW_FILL_COLOR)
        );
        // Text run starts at rect.x + 2: some pixel of 'A' is lit there.
        let text_lit = (0..w).any(|dx| {
            (0..DESKTOP_FONT.glyph_height())
                .any(|dy| surface.pixel(w + 2 + dx, content_y + dy) == Some(TEXT_COLOR))
        });
        assert!(text_lit, "content text must be painted at the text origin");
        // Cursor at column 0 shares that origin (4 px wide caret).
        assert_eq!(surface.pixel(w + 3, content_y + 4), Some(CURSOR_COLOR));
        assert_eq!(
            surface.pixel(w + 2 + w + 3, content_y + 4),
            Some(WINDOW_FILL_COLOR)
        );
    }

    #[test]
    fn test_workspace_snapshot_maps_vertical_tiles_to_desktop_windows() {
        let compositor = Compositor::new();
        let left_component = ComponentId::new();
        let right_component = ComponentId::new();
        let left = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            2,
            ViewContent::text_buffer(vec!["left".to_string()]),
            20,
        )
        .with_title("Editor");
        let right = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            5,
            ViewContent::text_buffer(vec!["right".to_string()]),
            50,
        )
        .with_title("CLI");

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: Some(right_component),
            main_view: Some(right.clone()),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: Some(SplitAxis::Vertical),
                focused_tile: 1,
                tiles: vec![
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 0,
                        is_focused: false,
                        active_component: Some(left_component),
                        tabs: vec![left_component],
                    },
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 1,
                        is_focused: true,
                        active_component: Some(right_component),
                        tabs: vec![right_component],
                    },
                ],
            },
            tiles: vec![
                WorkspaceTileRenderSnapshot {
                    tile_index: 0,
                    is_focused: false,
                    active_component: Some(left_component),
                    tabs: vec![left_component],
                    main_view: Some(left),
                    status_view: None,
                },
                WorkspaceTileRenderSnapshot {
                    tile_index: 1,
                    is_focused: true,
                    active_component: Some(right_component),
                    tabs: vec![right_component],
                    main_view: Some(right),
                    status_view: None,
                },
            ],
            component_count: 2,
            running_count: 2,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let windows =
            compositor.desktop_windows_from_workspace_snapshot(SurfaceSize::new(20, 8), &snapshot);

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].rect, SurfaceRect::new(0, 0, 10, 8));
        assert_eq!(windows[1].rect, SurfaceRect::new(10, 0, 10, 8));
        assert!(!windows[0].focused);
        assert!(windows[1].focused);
        assert_eq!(windows[0].frame.title.as_deref(), Some("Editor"));
        assert_eq!(windows[1].frame.title.as_deref(), Some("CLI"));
    }

    #[test]
    fn test_workspace_snapshot_maps_single_tile_to_full_surface_window() {
        let compositor = Compositor::new();
        let focused = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            7,
            ViewContent::text_buffer(vec!["solo".to_string()]),
            70,
        )
        .with_title("Solo");

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: None,
            main_view: Some(focused),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: Default::default(),
            tiles: Vec::new(),
            component_count: 1,
            running_count: 1,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let windows =
            compositor.desktop_windows_from_workspace_snapshot(SurfaceSize::new(18, 6), &snapshot);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].rect, SurfaceRect::new(0, 0, 18, 6));
        assert!(windows[0].focused);
        assert_eq!(windows[0].frame.title.as_deref(), Some("Solo"));
    }

    #[test]
    fn test_workspace_snapshot_orders_tiles_by_tile_index() {
        use services_workspace_manager::{
            ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
        };

        let compositor = Compositor::new();
        let first_component = ComponentId::new();
        let second_component = ComponentId::new();
        let first = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["first".to_string()]),
            10,
        )
        .with_title("First");
        let second = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            2,
            ViewContent::text_buffer(vec!["second".to_string()]),
            20,
        )
        .with_title("Second");

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: Some(first_component),
            main_view: Some(first.clone()),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: Some(SplitAxis::Vertical),
                focused_tile: 0,
                tiles: vec![
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 0,
                        is_focused: true,
                        active_component: Some(first_component),
                        tabs: vec![first_component],
                    },
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 1,
                        is_focused: false,
                        active_component: Some(second_component),
                        tabs: vec![second_component],
                    },
                ],
            },
            tiles: vec![
                WorkspaceTileRenderSnapshot {
                    tile_index: 1,
                    is_focused: false,
                    active_component: Some(second_component),
                    tabs: vec![second_component],
                    main_view: Some(second),
                    status_view: None,
                },
                WorkspaceTileRenderSnapshot {
                    tile_index: 0,
                    is_focused: true,
                    active_component: Some(first_component),
                    tabs: vec![first_component],
                    main_view: Some(first),
                    status_view: None,
                },
            ],
            component_count: 2,
            running_count: 2,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let windows =
            compositor.desktop_windows_from_workspace_snapshot(SurfaceSize::new(20, 8), &snapshot);

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].frame.title.as_deref(), Some("First"));
        assert_eq!(windows[0].rect, SurfaceRect::new(0, 0, 10, 8));
        assert_eq!(windows[1].frame.title.as_deref(), Some("Second"));
        assert_eq!(windows[1].rect, SurfaceRect::new(10, 0, 10, 8));
    }

    #[test]
    fn test_desktop_window_defaults_to_main_role() {
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("default"),
            10,
        );

        let window = DesktopWindow::new(frame, SurfaceRect::new(0, 0, 4, 3));

        assert_eq!(window.role, DesktopWindowRole::Main);
    }

    #[test]
    fn test_workspace_snapshot_uses_status_role_when_tile_has_only_status_view() {
        use services_workspace_manager::{
            ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
        };

        let compositor = Compositor::new();
        let component = ComponentId::new();
        let status = ViewFrame::new(
            ViewId::new(),
            ViewKind::StatusLine,
            3,
            ViewContent::status_line("status"),
            30,
        );

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: Some(component),
            main_view: None,
            status_view: Some(status.clone()),
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: Some(SplitAxis::Vertical),
                focused_tile: 0,
                tiles: vec![WorkspaceTileLayoutSnapshot {
                    tile_index: 0,
                    is_focused: true,
                    active_component: Some(component),
                    tabs: vec![component],
                }],
            },
            tiles: vec![WorkspaceTileRenderSnapshot {
                tile_index: 0,
                is_focused: true,
                active_component: Some(component),
                tabs: vec![component],
                main_view: None,
                status_view: Some(status),
            }],
            component_count: 1,
            running_count: 1,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let windows =
            compositor.desktop_windows_from_workspace_snapshot(SurfaceSize::new(20, 4), &snapshot);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].role, DesktopWindowRole::Status);
        assert!(windows[0].focused);
    }

    #[test]
    fn test_workspace_snapshot_maps_tile_tabs_into_window_metadata() {
        use services_workspace_manager::{
            ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
        };

        let compositor = Compositor::new();
        let active_component = ComponentId::new();
        let inactive_component = ComponentId::new();
        let editor = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            4,
            ViewContent::text_buffer(vec!["hello".to_string()]),
            40,
        )
        .with_title("Editor");

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: Some(active_component),
            main_view: Some(editor.clone()),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: None,
                focused_tile: 0,
                tiles: vec![WorkspaceTileLayoutSnapshot {
                    tile_index: 0,
                    is_focused: true,
                    active_component: Some(active_component),
                    tabs: vec![active_component, inactive_component],
                }],
            },
            tiles: vec![WorkspaceTileRenderSnapshot {
                tile_index: 0,
                is_focused: true,
                active_component: Some(active_component),
                tabs: vec![active_component, inactive_component],
                main_view: Some(editor),
                status_view: None,
            }],
            component_count: 2,
            running_count: 2,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let windows =
            compositor.desktop_windows_from_workspace_snapshot(SurfaceSize::new(24, 6), &snapshot);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].tabs.len(), 2);
        assert_eq!(windows[0].tabs[0].label, "Editor");
        assert!(windows[0].tabs[0].active);
        assert_eq!(windows[0].tabs[1].label, "Tab 2");
        assert!(!windows[0].tabs[1].active);
    }

    #[test]
    fn test_compose_workspace_snapshot_renders_tab_strip_for_multi_tab_tile() {
        use services_workspace_manager::{
            ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
        };

        let compositor = Compositor::new();
        let active_component = ComponentId::new();
        let inactive_component = ComponentId::new();
        let editor = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            4,
            ViewContent::text_buffer(vec!["hello".to_string()]),
            40,
        )
        .with_title("Editor");

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: Some(active_component),
            main_view: Some(editor.clone()),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: None,
                focused_tile: 0,
                tiles: vec![WorkspaceTileLayoutSnapshot {
                    tile_index: 0,
                    is_focused: true,
                    active_component: Some(active_component),
                    tabs: vec![active_component, inactive_component],
                }],
            },
            tiles: vec![WorkspaceTileRenderSnapshot {
                tile_index: 0,
                is_focused: true,
                active_component: Some(active_component),
                tabs: vec![active_component, inactive_component],
                main_view: Some(editor),
                status_view: None,
            }],
            component_count: 2,
            running_count: 2,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let surface = compositor.compose_workspace_snapshot(SurfaceSize::new(20, 5), &snapshot);

        assert_eq!(surface.rows[0], "# [Editor] (Tab 2) #");
        assert_eq!(surface.rows[1], "#hello             #");
    }

    #[test]
    fn test_notification_role_assigns_notification_layer() {
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("notice"),
            10,
        );

        let window = DesktopWindow::new(frame, SurfaceRect::new(0, 0, 6, 4))
            .with_role(DesktopWindowRole::Notification);

        assert_eq!(window.layer, DesktopWindowLayer::Notification);
    }

    #[test]
    fn test_compose_desktop_layer_policy_beats_raw_z_index() {
        let compositor = Compositor::new();
        let workspace = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["workspace".to_string()]),
            10,
        )
        .with_title("Main");
        let notification = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("toast"),
            20,
        )
        .with_title("Notify");

        let surface = compositor.compose_desktop(
            SurfaceSize::new(18, 7),
            vec![
                DesktopWindow::new(workspace, SurfaceRect::new(0, 1, 12, 5)).with_z_index(99),
                DesktopWindow::new(notification, SurfaceRect::new(4, 2, 10, 4))
                    .with_role(DesktopWindowRole::Notification)
                    .with_z_index(0),
            ],
        );

        assert_eq!(surface.rows[2], "+wor+ Notify +....");
        assert_eq!(surface.rows[3], "+   +panel: t+....");
    }

    #[test]
    fn test_compose_desktop_rgba_layer_policy_beats_raw_z_index() {
        let compositor = Compositor::new();
        let workspace = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["A".to_string()]),
            10,
        )
        .with_title("Main");
        let modal = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("M"),
            20,
        )
        .with_title("Modal");

        let surface = compositor.compose_desktop_rgba(
            SurfaceSize::new(12, 6),
            vec![
                DesktopWindow::new(workspace, SurfaceRect::new(1, 1, 6, 4))
                    .with_z_index(99)
                    .focused(),
                DesktopWindow::new(modal, SurfaceRect::new(2, 2, 4, 3))
                    .with_role(DesktopWindowRole::Modal)
                    .with_z_index(0),
            ],
        );

        assert_eq!(
            surface.pixel(RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT),
            Some(FOCUSED_BORDER_COLOR)
        );
        assert_eq!(
            surface.pixel(2 * RASTER_CELL_WIDTH, 2 * RASTER_CELL_HEIGHT),
            Some(UNFOCUSED_BORDER_COLOR)
        );
    }

    #[test]
    fn test_render_desktop_to_linear_framebuffer_target() {
        let compositor = Compositor::new();
        let editor = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            4,
            ViewContent::text_buffer(vec!["A".to_string()]),
            44,
        )
        .with_title("Main")
        .with_cursor(CursorPosition::new(0, 0));

        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let (width, height) = (8 * w, 5 * h);
        let mut bytes = vec![0; width * height * 4];
        let mut target = LinearFramebufferTarget::new(
            width,
            height,
            width,
            LinearPixelFormat::Rgb32,
            &mut bytes,
        );

        let stats = compositor.render_desktop_to_target(
            &mut target,
            vec![DesktopWindow::new(editor, SurfaceRect::new(1, 1, 4, 3)).focused()],
        );

        assert_eq!(stats.frame_count, 1);
        assert_eq!(stats.timestamp_ns, 44);
        assert_eq!(target.pixel(0, 0), Some(DESKTOP_BACKGROUND_COLOR));
        assert_eq!(target.pixel(w, h), Some(FOCUSED_BORDER_COLOR));
        let content_y = 2 * h + 2;
        assert_eq!(target.pixel(w + 3, content_y + 4), Some(CURSOR_COLOR));
        let text_lit = (0..w).any(|dx| {
            (0..DESKTOP_FONT.glyph_height())
                .any(|dy| target.pixel(w + 2 + dx, content_y + dy) == Some(TEXT_COLOR))
        });
        assert!(text_lit);
    }

    #[test]
    fn test_render_desktop_to_target_with_damage_only_repaints_intersecting_windows() {
        let compositor = Compositor::new();
        let left = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            4,
            ViewContent::text_buffer(vec!["A".to_string()]),
            44,
        )
        .with_title("Left");
        let right = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            2,
            ViewContent::panel("side"),
            40,
        )
        .with_title("Right");

        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let mut target = RgbaBuffer::new(12 * w, 5 * h, RgbaColor::new(0, 0, 0, 0));
        compositor.render_desktop_to_target(
            &mut target,
            vec![
                DesktopWindow::new(left.clone(), SurfaceRect::new(1, 1, 4, 3)).focused(),
                DesktopWindow::new(right.clone(), SurfaceRect::new(6, 1, 4, 3)),
            ],
        );
        // A point inside the right window's content area.
        let probe = (7 * w + 4, 2 * h + 10);
        let preserved_pixel = target.pixel(probe.0, probe.1);
        assert_eq!(preserved_pixel, Some(WINDOW_FILL_COLOR));

        // Damage covers only the left window's caret cell.
        let content_y = 2 * h + 2;
        let damage_rect = RasterRect::new(w + 2, content_y, w, h);
        let stats = compositor.render_desktop_to_target_with_damage(
            &mut target,
            vec![
                DesktopWindow::new(
                    left.with_cursor(CursorPosition::new(0, 0)),
                    SurfaceRect::new(1, 1, 4, 3),
                )
                .focused(),
                DesktopWindow::new(right, SurfaceRect::new(6, 1, 4, 3)),
            ],
            Some(damage_rect),
        );

        assert_eq!(stats.frame_count, 2);
        assert_eq!(stats.painted_windows, 1);
        assert_eq!(stats.damage_rect, Some(damage_rect));
        assert_eq!(target.pixel(w + 3, content_y + 4), Some(CURSOR_COLOR));
        assert_eq!(target.pixel(probe.0, probe.1), preserved_pixel);
    }

    #[test]
    fn test_cursor_composes_above_windows_and_clips_at_edges() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec![]),
            0,
        );
        let windows = vec![DesktopWindow::new(frame, SurfaceRect::new(0, 0, 8, 5)).focused()];
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let mut target = RgbaBuffer::new(8 * w, 5 * h, RgbaColor::new(0, 0, 0, 0));

        // Cursor in the middle of the window: hotspot pixel is outline, the
        // fill shows a few pixels in, and the window fill is untouched elsewhere.
        let cursor = DesktopCursor::new(3 * w, 2 * h + 4);
        compositor.render_desktop_to_target_with_cursor(
            &mut target,
            windows.clone(),
            None,
            Some(cursor),
        );
        assert_eq!(
            target.pixel(cursor.x, cursor.y),
            Some(POINTER_OUTLINE_COLOR)
        );
        assert_eq!(
            target.pixel(cursor.x + 2, cursor.y + 5),
            Some(POINTER_FILL_COLOR)
        );
        assert_eq!(
            target.pixel(cursor.x + 11, cursor.y),
            Some(WINDOW_FILL_COLOR)
        );
        assert_eq!(cursor.bounds(), RasterRect::new(cursor.x, cursor.y, 12, 19));

        // Hidden cursor paints nothing.
        compositor.render_desktop_to_target_with_cursor(
            &mut target,
            windows.clone(),
            None,
            Some(DesktopCursor::hidden()),
        );
        assert_eq!(target.pixel(cursor.x, cursor.y), Some(WINDOW_FILL_COLOR));

        // Cursor hanging off the bottom-right corner is clipped, not a panic.
        let edge = DesktopCursor::new(8 * w - 3, 5 * h - 2);
        compositor.render_desktop_to_target_with_cursor(&mut target, windows, None, Some(edge));
        assert_eq!(target.pixel(edge.x, edge.y), Some(POINTER_OUTLINE_COLOR));
        assert_eq!(
            target.pixel(edge.x + 1, edge.y + 1),
            Some(POINTER_OUTLINE_COLOR)
        );
    }

    #[test]
    fn test_cursor_survives_damage_repaint_elsewhere() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec![]),
            0,
        );
        let windows = vec![DesktopWindow::new(frame, SurfaceRect::new(0, 0, 8, 5)).focused()];
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let mut target = RgbaBuffer::new(8 * w, 5 * h, RgbaColor::new(0, 0, 0, 0));
        let cursor = DesktopCursor::new(2 * w, 2 * h);
        compositor.render_desktop_to_target_with_cursor(
            &mut target,
            windows.clone(),
            None,
            Some(cursor),
        );
        // Repaint a region far from the cursor; the cursor must still be there.
        compositor.render_desktop_to_target_with_cursor(
            &mut target,
            windows,
            Some(RasterRect::new(5 * w, 3 * h, w, h)),
            Some(cursor),
        );
        assert_eq!(
            target.pixel(cursor.x, cursor.y),
            Some(POINTER_OUTLINE_COLOR)
        );
    }

    #[test]
    fn test_desktop_scene_round_trips_and_renders_identically() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["hello".to_string()]),
            5,
        )
        .with_title("Main")
        .with_cursor(CursorPosition::new(0, 2));
        let windows = vec![
            DesktopWindow::new(frame, SurfaceRect::new(1, 1, 12, 5))
                .with_highlight(Some(0))
                .focused(),
            DesktopWindow::new(
                ViewFrame::new(
                    ViewId::new(),
                    ViewKind::Panel,
                    1,
                    ViewContent::panel("n"),
                    6,
                )
                .with_title("Note"),
                SurfaceRect::new(8, 0, 8, 3),
            )
            .with_role(DesktopWindowRole::Notification),
        ];
        let scene = DesktopScene::new(SurfaceSize::new(16, 7), windows.clone())
            .with_cursor(Some(DesktopCursor::new(20, 30)))
            .with_theme(Theme::LIGHT);

        // JSON round trip preserves every field.
        let json = serde_json::to_string(&scene).unwrap();
        let back: DesktopScene = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scene);

        // Rendering the scene equals rendering its parts with the same theme.
        let via_scene = compositor.render_scene_rgba(&back);
        let light = Compositor::with_theme(Theme::LIGHT);
        let (w, h) = scene.pixel_size();
        let mut direct = RgbaBuffer::new(w, h, Theme::LIGHT.background);
        light.render_desktop_to_target_with_cursor(
            &mut direct,
            windows,
            None,
            Some(DesktopCursor::new(20, 30)),
        );
        assert_eq!(via_scene.pixels, direct.as_bytes());
        assert_eq!(via_scene.frame_count, 2);
        assert_eq!(via_scene.timestamp_ns, 6);

        // Optional fields default when absent on the wire.
        let minimal: DesktopScene =
            serde_json::from_str(r#"{"size":{"width":4,"height":2},"windows":[]}"#).unwrap();
        assert_eq!(minimal.cursor, None);
        assert_eq!(minimal.theme, None);
        assert_eq!(
            minimal.pixel_size(),
            (4 * RASTER_CELL_WIDTH, 2 * RASTER_CELL_HEIGHT)
        );
    }

    #[test]
    fn test_highlight_line_fills_one_content_row() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::text_buffer(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
            0,
        );
        let window =
            DesktopWindow::new(frame, SurfaceRect::new(0, 0, 10, 6)).with_highlight(Some(1));
        let surface = compositor.compose_desktop_rgba(SurfaceSize::new(10, 6), vec![window]);
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        // Row 1 band spans the content width; rows 0 and 2 are plain surface.
        let band_y = h + 1 + h + h / 2;
        assert_eq!(surface.pixel(6 * w, band_y), Some(Theme::DEFAULT.selection));
        assert_eq!(surface.pixel(1, band_y), Some(Theme::DEFAULT.selection));
        assert_eq!(surface.pixel(6 * w, band_y - h), Some(WINDOW_FILL_COLOR));
        assert_eq!(surface.pixel(6 * w, band_y + h), Some(WINDOW_FILL_COLOR));
        // Text still paints over the band.
        let glyph = graphics_rasterizer::ascii_8x16_glyph('b');
        let (dy, row) = glyph.iter().enumerate().find(|(_, r)| **r != 0).unwrap();
        let dx = (0..8).find(|dx| (row >> (7 - dx)) & 1 == 1).unwrap();
        assert_eq!(surface.pixel(2 + dx, h + 2 + h + dy), Some(TEXT_COLOR));

        // Out-of-range highlight paints nothing.
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::text_buffer(vec![]),
            0,
        );
        let window =
            DesktopWindow::new(frame, SurfaceRect::new(0, 0, 10, 3)).with_highlight(Some(9));
        let surface = compositor.compose_desktop_rgba(SurfaceSize::new(10, 3), vec![window]);
        for y in 0..3 * h {
            for x in 0..10 * w {
                assert_ne!(surface.pixel(x, y), Some(Theme::DEFAULT.selection));
            }
        }
    }

    #[test]
    fn test_graphics_content_draws_ops_clipped_to_the_content_area() {
        use view_types::{Color, DrawOp, PixelRect, TextStyle};
        let compositor = Compositor::new();
        let ops = vec![
            DrawOp::Fill {
                rect: PixelRect::new(0, 0, 20, 10),
                color: Color::rgb(200, 0, 0),
            },
            // Far beyond the window: must be clipped away.
            DrawOp::Fill {
                rect: PixelRect::new(1000, 1000, 50, 50),
                color: Color::rgb(0, 200, 0),
            },
            DrawOp::Line {
                x0: 0,
                y0: 30,
                x1: 400,
                y1: 30,
                color: Color::rgb(0, 0, 200),
            },
            DrawOp::Text {
                x: 0,
                y: 40,
                text: "I".to_string(),
                color: None,
                style: TextStyle::default(),
            },
        ];
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::graphics(ops),
            0,
        )
        .with_title("Chart");
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let window = DesktopWindow::new(frame, SurfaceRect::new(1, 1, 12, 6)).focused();
        let surface = compositor.compose_desktop_rgba(SurfaceSize::new(16, 9), vec![window]);

        // Content origin: rect.x + 2, content_top + 2 with content_top = rect.y + cell height.
        let ox = w + 2;
        let oy = 2 * h + 2;
        assert_eq!(surface.pixel(ox, oy), Some(RgbaColor::new(200, 0, 0, 255)));
        assert_eq!(
            surface.pixel(ox + 19, oy + 9),
            Some(RgbaColor::new(200, 0, 0, 255))
        );
        assert_eq!(surface.pixel(ox + 20, oy + 10), Some(WINDOW_FILL_COLOR));
        // Line runs to the window's inner edge and no further.
        assert_eq!(
            surface.pixel(ox + 50, oy + 30),
            Some(RgbaColor::new(0, 0, 200, 255))
        );
        let inner_right = (1 + 12) * w - 1;
        assert_eq!(
            surface.pixel(inner_right - 1, oy + 30),
            Some(RgbaColor::new(0, 0, 200, 255))
        );
        assert_ne!(
            surface.pixel(inner_right, oy + 30),
            Some(RgbaColor::new(0, 0, 200, 255))
        );
        // Nothing green anywhere (clipped op), text painted in theme text colour.
        for y in 0..9 * h {
            for x in 0..16 * w {
                assert_ne!(surface.pixel(x, y), Some(RgbaColor::new(0, 200, 0, 255)));
            }
        }
        // Text is drawn smoothly (GFX-095): a corner may be soft, but
        // the stem of the 'I' is in the full text colour.
        let glyph = graphics_rasterizer::ascii_8x16_glyph('I');
        let inked = glyph.iter().enumerate().any(|(dy, row)| {
            (0..8).any(|dx| {
                (row >> (7 - dx)) & 1 == 1
                    && surface.pixel(ox + dx, oy + 40 + dy) == Some(TEXT_COLOR)
            })
        });
        assert!(inked, "no fully inked pixel in the 'I'");
        // Hit testing still reports content cells for graphics windows.
        let hit = compositor
            .hit_test(
                &[DesktopWindow::new(
                    ViewFrame::new(
                        ViewId::new(),
                        ViewKind::Panel,
                        1,
                        ViewContent::graphics(vec![]),
                        0,
                    ),
                    SurfaceRect::new(1, 1, 12, 6),
                )],
                ox + 3,
                oy + 3,
            )
            .unwrap();
        assert_eq!(hit.region, HitRegion::Content { line: 0, column: 0 });
    }

    #[test]
    fn test_title_bar_tint_follows_focus_and_role() {
        let compositor = Compositor::new();
        let make = |title: &str| {
            ViewFrame::new(
                ViewId::new(),
                ViewKind::TextBuffer,
                1,
                ViewContent::text_buffer(vec![]),
                0,
            )
            .with_title(title)
        };
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let windows = vec![
            DesktopWindow::new(make("A"), SurfaceRect::new(0, 0, 8, 4)).focused(),
            DesktopWindow::new(make("B"), SurfaceRect::new(8, 0, 8, 4)),
            DesktopWindow::new(make("N"), SurfaceRect::new(0, 4, 8, 3))
                .with_role(DesktopWindowRole::Notification),
            DesktopWindow::new(make("P"), SurfaceRect::new(8, 4, 8, 3))
                .with_role(DesktopWindowRole::Palette),
        ];
        let surface = compositor.compose_desktop_rgba(SurfaceSize::new(16, 7), windows);

        // A pixel in the title strip past the label text.
        let probe_x = 6 * w;
        assert_eq!(
            surface.pixel(probe_x, 4),
            Some(Theme::DEFAULT.title_focused)
        );
        assert_eq!(
            surface.pixel(8 * w + probe_x, 4),
            Some(Theme::DEFAULT.title_unfocused)
        );
        assert_eq!(
            surface.pixel(probe_x, 4 * h + 4),
            Some(Theme::DEFAULT.title_notice)
        );
        assert_eq!(
            surface.pixel(8 * w + probe_x, 4 * h + 4),
            Some(Theme::DEFAULT.title_palette)
        );

        // Separator under the title bar carries the border colour.
        assert_eq!(surface.pixel(probe_x, h), Some(FOCUSED_BORDER_COLOR));
        assert_eq!(
            surface.pixel(8 * w + probe_x, h),
            Some(UNFOCUSED_BORDER_COLOR)
        );
        // Content below is plain surface.
        assert_eq!(surface.pixel(probe_x, h + 5), Some(WINDOW_FILL_COLOR));

        // Unfocused title text is muted.
        let glyph = graphics_rasterizer::ascii_8x16_glyph('B');
        let (dy, row) = glyph
            .iter()
            .enumerate()
            .find(|(_, row)| **row != 0)
            .unwrap();
        let dx = (0..8).find(|dx| (row >> (7 - dx)) & 1 == 1).unwrap();
        // Label is padded with one space, so the glyph starts one cell in.
        assert_eq!(
            surface.pixel(8 * w + 2 + w + dx, 2 + dy),
            Some(Theme::DEFAULT.text_muted)
        );
    }

    #[test]
    fn test_tab_strip_draws_active_and_inactive_tab_boxes() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec![]),
            0,
        );
        let window = DesktopWindow::new(frame, SurfaceRect::new(0, 0, 20, 4))
            .with_tabs(vec![
                DesktopTab::new("one", true),
                DesktopTab::new("two", false),
            ])
            .focused();
        let surface = compositor.compose_desktop_rgba(SurfaceSize::new(20, 4), vec![window]);
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);

        // First tab box: x 2.. 2+5w, padding cell before the label is fill.
        let first_pad = (2 + w / 2, 2 + h / 2);
        assert_eq!(
            surface.pixel(first_pad.0, first_pad.1),
            Some(Theme::DEFAULT.tab_active)
        );
        // Second tab starts after the first box plus a 2px gap.
        let second_x = 2 + 5 * w + 2;
        assert_eq!(
            surface.pixel(second_x + w / 2, 2 + h / 2),
            Some(Theme::DEFAULT.tab_inactive)
        );
        // Rounded corner of the inactive tab shows the title fill behind it.
        assert_eq!(
            surface.pixel(second_x, 2),
            Some(Theme::DEFAULT.title_focused)
        );
        // Past both tabs the strip is title fill.
        assert_eq!(
            surface.pixel(second_x + 5 * w + 4, 2 + h / 2),
            Some(Theme::DEFAULT.title_focused)
        );
    }

    #[test]
    fn test_theme_override_changes_every_painted_token() {
        let compositor = Compositor::with_theme(Theme::LIGHT);
        assert_eq!(compositor.theme(), &Theme::LIGHT);
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["x".to_string()]),
            0,
        )
        .with_cursor(CursorPosition::new(0, 0));
        let window = DesktopWindow::new(frame, SurfaceRect::new(1, 1, 8, 4)).focused();
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let mut target = RgbaBuffer::new(10 * w, 6 * h, RgbaColor::new(0, 0, 0, 0));
        compositor.render_desktop_to_target_with_cursor(
            &mut target,
            vec![window],
            None,
            Some(DesktopCursor::new(0, 5 * h)),
        );
        assert_eq!(target.pixel(0, 0), Some(Theme::LIGHT.background));
        assert_eq!(target.pixel(w, h), Some(Theme::LIGHT.border_focused));
        assert_eq!(target.pixel(6 * w, h + 4), Some(Theme::LIGHT.title_focused));
        assert_eq!(target.pixel(w + 3, 2 * h + 6), Some(Theme::LIGHT.caret));
        assert_eq!(target.pixel(8 * w, 3 * h), Some(Theme::LIGHT.surface));
        assert_eq!(target.pixel(0, 5 * h), Some(Theme::LIGHT.pointer_outline));
    }

    #[test]
    fn test_hit_test_reports_regions_and_content_cells() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["abc".to_string()]),
            0,
        );
        let view_id = frame.view_id;
        let windows = vec![DesktopWindow::new(frame, SurfaceRect::new(1, 1, 6, 4)).focused()];
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);

        // Bare desktop.
        assert_eq!(compositor.hit_test(&windows, 0, 0), None);
        assert_eq!(compositor.hit_test(&windows, 7 * w, 5 * h), None);

        // Top-left pixel is border.
        let border = compositor.hit_test(&windows, w, h).unwrap();
        assert_eq!(border.region, HitRegion::Border);
        assert_eq!(border.view_id, view_id);
        assert_eq!((border.local_x, border.local_y), (0, 0));
        assert_eq!(border.window_index, 0);
        assert_eq!(border.role, DesktopWindowRole::Main);

        // Inside the title row.
        let chrome = compositor.hit_test(&windows, w + 5, h + 3).unwrap();
        assert_eq!(chrome.region, HitRegion::Chrome);

        // Content row 0, column 2: content starts one cell below the top plus
        // the border, one border pixel in from the left.
        let content_x = w + RASTER_BORDER_THICKNESS + 2 * w + 1;
        let content_y = h + h + RASTER_BORDER_THICKNESS + 1;
        let content = compositor.hit_test(&windows, content_x, content_y).unwrap();
        assert_eq!(content.region, HitRegion::Content { line: 0, column: 2 });
        let content = compositor
            .hit_test(&windows, content_x, content_y + h)
            .unwrap();
        assert_eq!(content.region, HitRegion::Content { line: 1, column: 2 });

        // Bottom border pixel.
        let bottom = compositor.hit_test(&windows, w + 5, 5 * h - 1).unwrap();
        assert_eq!(bottom.region, HitRegion::Border);
    }

    #[test]
    fn test_hit_test_follows_paint_order() {
        let compositor = Compositor::new();
        let make = |title: &str| {
            ViewFrame::new(
                ViewId::new(),
                ViewKind::TextBuffer,
                1,
                ViewContent::text_buffer(vec![]),
                0,
            )
            .with_title(title)
        };
        let windows = vec![
            // Full-screen Main at the bottom of its layer.
            DesktopWindow::new(make("main"), SurfaceRect::new(0, 0, 10, 10))
                .with_z_index(0)
                .focused(),
            DesktopWindow::new(make("palette"), SurfaceRect::new(2, 2, 4, 4))
                .with_role(DesktopWindowRole::Palette),
            // Two overlapping Main windows: higher z-index wins.
            DesktopWindow::new(make("low"), SurfaceRect::new(6, 6, 4, 4)).with_z_index(1),
            DesktopWindow::new(make("high"), SurfaceRect::new(7, 7, 3, 3)).with_z_index(2),
        ];
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);

        let hit = compositor.hit_test(&windows, 3 * w, 3 * h).unwrap();
        assert_eq!(hit.window_index, 1);
        assert_eq!(hit.role, DesktopWindowRole::Palette);

        let hit = compositor.hit_test(&windows, 8 * w, 8 * h).unwrap();
        assert_eq!(hit.window_index, 3);

        let hit = compositor.hit_test(&windows, 6 * w + 2, 6 * h + 2).unwrap();
        assert_eq!(hit.window_index, 2);

        let hit = compositor.hit_test(&windows, 1, 1).unwrap();
        assert_eq!(hit.window_index, 0);

        assert_eq!(composition_order(&windows), vec![0, 2, 3, 1]);

        // Layer policy beats z-index: a Main with a huge z-index stays under a Palette.
        let mut boosted = windows.clone();
        boosted[0].z_index = 1000;
        let hit = compositor.hit_test(&boosted, 3 * w, 3 * h).unwrap();
        assert_eq!(hit.role, DesktopWindowRole::Palette);
        assert_eq!(composition_order(&boosted), vec![2, 3, 0, 1]);
    }

    #[test]
    fn test_hit_test_on_windows_too_small_for_content() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::StatusLine,
            1,
            ViewContent::status_line("s"),
            0,
        );
        // Height 1 cell: chrome row only, no content rect.
        let windows = vec![DesktopWindow::new(frame, SurfaceRect::new(0, 0, 4, 1))];
        let hit = compositor.hit_test(&windows, 3, 3).unwrap();
        assert_eq!(hit.region, HitRegion::Chrome);
        assert_eq!(
            compositor.hit_test(&windows, 0, 0).unwrap().region,
            HitRegion::Border
        );
    }

    #[test]
    fn test_compose_desktop_rgba_matches_golden_fixture() {
        let compositor = Compositor::new();
        let editor = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            4,
            ViewContent::text_buffer(vec!["AB".to_string(), "CD".to_string()]),
            44,
        )
        .with_title("Main")
        .with_cursor(CursorPosition::new(1, 1));
        let modal = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            2,
            ViewContent::panel("ok"),
            60,
        )
        .with_title("Modal");

        let surface = compositor.compose_desktop_rgba(
            SurfaceSize::new(8, 5),
            vec![
                DesktopWindow::new(editor, SurfaceRect::new(1, 1, 4, 3)).focused(),
                DesktopWindow::new(modal, SurfaceRect::new(3, 1, 3, 3))
                    .with_role(DesktopWindowRole::Modal),
            ],
        );

        assert_raster_golden(
            &surface,
            "desktop_rgba_surface.golden",
            include_str!("../tests/golden/desktop_rgba_surface.golden"),
        );
    }

    #[test]
    fn test_compose_workspace_snapshot_rgba_matches_golden_fixture() {
        let compositor = Compositor::new();
        let snapshot = sample_workspace_snapshot_for_golden();

        let surface =
            compositor.compose_workspace_snapshot_rgba(SurfaceSize::new(12, 5), &snapshot);

        assert_raster_golden(
            &surface,
            "workspace_snapshot_rgba_surface.golden",
            include_str!("../tests/golden/workspace_snapshot_rgba_surface.golden"),
        );
    }

    #[test]
    fn test_compose_desktop_rgba_clips_long_content_to_window_bounds() {
        let compositor = Compositor::new();
        let frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            4,
            ViewContent::text_buffer(vec!["WWWWWWWW".to_string()]),
            44,
        )
        .with_title("Wide");

        let surface = compositor.compose_desktop_rgba(
            SurfaceSize::new(8, 5),
            vec![DesktopWindow::new(frame, SurfaceRect::new(1, 1, 3, 3)).focused()],
        );

        for y in 22..(22 + DESKTOP_FONT.glyph_height()) {
            for x in 37..46 {
                assert_eq!(
                    surface.pixel(x, y),
                    Some(DESKTOP_BACKGROUND_COLOR),
                    "content overflow at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn test_compose_desktop_rgba_uses_desktop_font_spacing_for_title() {
        let compositor = Compositor::new();
        let title = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            2,
            ViewContent::text_buffer(vec!["body".to_string()]),
            22,
        )
        .with_title("II");

        let surface = compositor.compose_desktop_rgba(
            SurfaceSize::new(8, 5),
            vec![DesktopWindow::new(title, SurfaceRect::new(1, 1, 4, 3)).focused()],
        );

        // Chrome label " II " is drawn at (rect.x + 2, rect.y + 2) with the
        // desktop font; the second 'I' is exactly one advance to the right.
        let (w, h) = (RASTER_CELL_WIDTH, RASTER_CELL_HEIGHT);
        let origin = (w + 2 + DESKTOP_FONT.advance_x(), h + 2);
        let glyph = graphics_rasterizer::ascii_8x16_glyph('I');
        let mut checked = 0;
        for (dy, row) in glyph.iter().enumerate() {
            for dx in 0..8 {
                if (row >> (7 - dx)) & 1 == 1 {
                    assert_eq!(
                        surface.pixel(origin.0 + dx, origin.1 + dy),
                        Some(TEXT_COLOR)
                    );
                    assert_eq!(
                        surface.pixel(origin.0 + DESKTOP_FONT.advance_x() + dx, origin.1 + dy),
                        Some(TEXT_COLOR)
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn test_workspace_snapshot_maps_horizontal_tiles_to_desktop_windows() {
        use services_workspace_manager::{
            ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
        };

        let compositor = Compositor::new();
        let top_component = ComponentId::new();
        let bottom_component = ComponentId::new();
        let top = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            2,
            ViewContent::text_buffer(vec!["top".to_string()]),
            20,
        )
        .with_title("Top");
        let bottom = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            5,
            ViewContent::text_buffer(vec!["bottom".to_string()]),
            50,
        )
        .with_title("Bottom");

        let snapshot = WorkspaceRenderSnapshot {
            focused_component: Some(top_component),
            main_view: Some(top.clone()),
            status_view: None,
            composed_main_view: None,
            composed_status_view: None,
            layout: WorkspaceLayoutSnapshot {
                split_axis: Some(SplitAxis::Horizontal),
                focused_tile: 0,
                tiles: vec![
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 0,
                        is_focused: true,
                        active_component: Some(top_component),
                        tabs: vec![top_component],
                    },
                    WorkspaceTileLayoutSnapshot {
                        tile_index: 1,
                        is_focused: false,
                        active_component: Some(bottom_component),
                        tabs: vec![bottom_component],
                    },
                ],
            },
            tiles: vec![
                WorkspaceTileRenderSnapshot {
                    tile_index: 0,
                    is_focused: true,
                    active_component: Some(top_component),
                    tabs: vec![top_component],
                    main_view: Some(top),
                    status_view: None,
                },
                WorkspaceTileRenderSnapshot {
                    tile_index: 1,
                    is_focused: false,
                    active_component: Some(bottom_component),
                    tabs: vec![bottom_component],
                    main_view: Some(bottom),
                    status_view: None,
                },
            ],
            component_count: 2,
            running_count: 2,
            status_strip: "Workspace".to_string(),
            breadcrumbs: "PANDA".to_string(),
            #[cfg(debug_assertions)]
            debug_info: None,
        };

        let windows =
            compositor.desktop_windows_from_workspace_snapshot(SurfaceSize::new(18, 9), &snapshot);

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].rect, SurfaceRect::new(0, 0, 18, 5));
        assert_eq!(windows[1].rect, SurfaceRect::new(0, 5, 18, 4));
        assert!(windows[0].focused);
        assert!(!windows[1].focused);
    }

    #[test]
    fn test_compose_desktop_focus_visuals_distinguish_focused_from_unfocused() {
        let compositor = Compositor::new();
        let focused = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("focused"),
            10,
        )
        .with_title("Focus");
        let unfocused = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("idle"),
            10,
        )
        .with_title("Idle");

        let surface = compositor.compose_desktop(
            SurfaceSize::new(20, 8),
            vec![
                DesktopWindow::new(focused, SurfaceRect::new(0, 0, 10, 4)).focused(),
                DesktopWindow::new(unfocused, SurfaceRect::new(10, 0, 10, 4)),
            ],
        );

        assert_eq!(surface.rows[0], "# Focus ##+ Idle +++");
        assert_eq!(surface.rows[3], "##########++++++++++");
    }

    #[test]
    fn test_compose_desktop_modal_layer_outranks_notification_and_overlay() {
        let compositor = Compositor::new();
        let workspace = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec!["workspace".to_string()]),
            10,
        )
        .with_title("Main");
        let overlay = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("overlay"),
            20,
        )
        .with_title("Overlay");
        let modal = ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            1,
            ViewContent::panel("modal"),
            30,
        )
        .with_title("Modal");

        let surface = compositor.compose_desktop(
            SurfaceSize::new(20, 8),
            vec![
                DesktopWindow::new(workspace, SurfaceRect::new(0, 1, 14, 5)).with_z_index(50),
                DesktopWindow::new(overlay, SurfaceRect::new(3, 2, 12, 4))
                    .with_role(DesktopWindowRole::Overlay)
                    .with_z_index(99),
                DesktopWindow::new(modal, SurfaceRect::new(5, 3, 10, 4))
                    .with_role(DesktopWindowRole::Modal)
                    .with_z_index(0),
            ],
        );

        assert_eq!(surface.rows[3], "+  +p+ Modal ++.....");
        assert!(surface.rows[4].contains("panel: m"));
    }
}
