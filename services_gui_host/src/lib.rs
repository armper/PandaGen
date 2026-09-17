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
    RasterRect, RenderTarget, RgbaBuffer, RgbaColor, ScissorTarget, DESKTOP_FONT,
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
pub use theme::Theme;
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
}

impl DesktopTab {
    pub fn new(label: impl Into<String>, active: bool) -> Self {
        Self {
            label: label.into(),
            active,
        }
    }
}

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
}

fn default_chrome() -> bool {
    true
}

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
        }
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

    /// Draw no title row; content begins right inside the border.
    pub fn without_chrome(mut self) -> Self {
        self.chrome = false;
        self
    }

    /// Content lines this window can show at its cell height.
    pub fn content_rows(&self) -> usize {
        if self.chrome {
            self.rect.height.saturating_sub(2)
        } else {
            self.rect.height
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
        }
    }

    /// Compositor painting with `theme` (GFX-035).
    pub const fn with_theme(theme: Theme) -> Self {
        Self { theme }
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

        if let Some(rect) = damage_rect {
            target.fill_rect(rect, self.theme.background);
        } else {
            target.clear(self.theme.background);
        }

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
}

impl DesktopScene {
    pub fn new(size: SurfaceSize, windows: Vec<DesktopWindow>) -> Self {
        Self {
            size,
            windows,
            cursor: None,
            theme: None,
            damage: None,
        }
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
        };
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
    let rect = pixel_rect(window.rect);
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
            let rect = pixel_rect(window.rect);
            if !rect.contains(x, y) {
                continue;
            }
            let local_x = x - rect.x;
            let local_y = y - rect.y;

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
                    &DESKTOP_FONT
                };
                canvas.draw_text_with_font(*x as usize, *y as usize, text, font, color);
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

    for dy in 0..rect.height {
        let y = rect.y + dy;
        if y >= canvas.len() {
            continue;
        }

        for dx in 0..rect.width {
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
    let rect = pixel_rect(window.rect);
    if rect.width == 0 || rect.height == 0 {
        return false;
    }

    let clipped_rect = damage_rect
        .map(|damage| rect.intersect(damage))
        .unwrap_or(Some(rect));
    let Some(clipped_rect) = clipped_rect else {
        return false;
    };

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
    if separator_y < rect.y + rect.height {
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
    let bottom = rect.y + rect.height;
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
    let bottom = rect.y + rect.height;
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
        let glyph = graphics_rasterizer::ascii_8x16_glyph('I');
        let (dy, row) = glyph.iter().enumerate().find(|(_, r)| **r != 0).unwrap();
        let dx = (0..8).find(|dx| (row >> (7 - dx)) & 1 == 1).unwrap();
        assert_eq!(surface.pixel(ox + dx, oy + 40 + dy), Some(TEXT_COLOR));
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
