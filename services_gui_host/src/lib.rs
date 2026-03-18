//! GUI host and compositor on view surfaces.

use graphics_rasterizer::{
    RasterRect, RenderTarget, RgbaBuffer, RgbaColor, ScissorTarget, DESKTOP_FONT,
};
use serde::{Deserialize, Serialize};
use services_workspace_manager::{SplitAxis, WorkspaceRenderSnapshot, WorkspaceTileRenderSnapshot};
use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

const DESKTOP_BACKGROUND: char = '.';
const CURSOR_GLYPH: char = '@';
const RASTER_CELL_WIDTH: usize = DESKTOP_FONT.advance_x();
const RASTER_CELL_HEIGHT: usize = DESKTOP_FONT.glyph_height() + 2;
const RASTER_BORDER_THICKNESS: usize = 1;

const DESKTOP_BACKGROUND_COLOR: RgbaColor = RgbaColor::new(12, 18, 28, 255);
const WINDOW_FILL_COLOR: RgbaColor = RgbaColor::new(28, 34, 48, 255);
const FOCUSED_BORDER_COLOR: RgbaColor = RgbaColor::new(52, 211, 153, 255);
const UNFOCUSED_BORDER_COLOR: RgbaColor = RgbaColor::new(107, 114, 128, 255);
const TEXT_COLOR: RgbaColor = RgbaColor::new(226, 232, 240, 255);
const CURSOR_COLOR: RgbaColor = RgbaColor::new(251, 146, 60, 255);

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
            DesktopWindowRole::Main | DesktopWindowRole::Status => Self::Workspace,
            DesktopWindowRole::Overlay => Self::Overlay,
            DesktopWindowRole::Palette => Self::Palette,
            DesktopWindowRole::Notification => Self::Notification,
            DesktopWindowRole::Modal => Self::Modal,
        }
    }

    fn sort_key(self) -> usize {
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
pub struct Compositor;

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor {
    pub fn new() -> Self {
        Self
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
            DESKTOP_BACKGROUND_COLOR,
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
        mut windows: Vec<DesktopWindow>,
        damage_rect: Option<RasterRect>,
    ) -> RasterRenderStats {
        let target_bounds = RasterRect::new(0, 0, target.width(), target.height());
        let damage_rect = damage_rect.and_then(|rect| rect.intersect(target_bounds));

        if let Some(rect) = damage_rect {
            target.fill_rect(rect, DESKTOP_BACKGROUND_COLOR);
        } else {
            target.clear(DESKTOP_BACKGROUND_COLOR);
        }

        windows.sort_by_key(|window| {
            (
                window.layer.sort_key(),
                window.z_index,
                window.frame.view_id.as_uuid(),
            )
        });

        let mut painted_windows = 0;
        for window in &windows {
            if raster_window(target, window, damage_rect) {
                painted_windows += 1;
            }
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

    if rect.width > 2 {
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

    {
        let mut window_target = ScissorTarget::new(target, clipped_rect);
        window_target.fill_rect(rect, WINDOW_FILL_COLOR);
        window_target.draw_border(
            rect,
            RASTER_BORDER_THICKNESS,
            if window.focused {
                FOCUSED_BORDER_COLOR
            } else {
                UNFOCUSED_BORDER_COLOR
            },
        );
    }

    let chrome_label = window_chrome_label(window);
    if let Some(chrome_rect) = window_chrome_rect(rect).intersect(clipped_rect) {
        let mut chrome_target = ScissorTarget::new(target, chrome_rect);
        chrome_target.draw_text_with_font(
            rect.x + 2,
            rect.y + 2,
            &chrome_label,
            &DESKTOP_FONT,
            TEXT_COLOR,
        );
    }

    if let Some(content_rect) = window_content_rect(rect) {
        if let Some(content_clip) = content_rect.intersect(clipped_rect) {
            let target_height = target.height();
            let mut content_target = ScissorTarget::new(target, content_clip);
            let line_origin_y = rect.y + RASTER_CELL_HEIGHT + 2;
            for (line_index, line) in render_content_lines(&window.frame.content)
                .into_iter()
                .take(window.rect.height.saturating_sub(2))
                .enumerate()
            {
                let y = line_origin_y + line_index * RASTER_CELL_HEIGHT;
                if y >= target_height {
                    break;
                }
                content_target.draw_text_with_font(rect.x + 2, y, &line, &DESKTOP_FONT, TEXT_COLOR);
            }

            if let Some(cursor) = window.frame.cursor {
                let cursor_x = rect.x + RASTER_CELL_WIDTH + 2 + cursor.column * RASTER_CELL_WIDTH;
                let cursor_y = rect.y + RASTER_CELL_HEIGHT + 1 + cursor.line * RASTER_CELL_HEIGHT;
                content_target.fill_rect(
                    RasterRect::new(cursor_x, cursor_y, 4, RASTER_CELL_HEIGHT.saturating_sub(2)),
                    CURSOR_COLOR,
                );
            }
        }
    }

    true
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

fn partition_rects_vertical(size: SurfaceSize, count: usize) -> Vec<SurfaceRect> {
    let slices = partition_extent(size.width, count);
    slices
        .into_iter()
        .map(|(x, width)| SurfaceRect::new(x, 0, width, size.height))
        .collect()
}

fn partition_rects_horizontal(size: SurfaceSize, count: usize) -> Vec<SurfaceRect> {
    let slices = partition_extent(size.height, count);
    slices
        .into_iter()
        .map(|(y, height)| SurfaceRect::new(0, y, size.width, height))
        .collect()
}

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
                    Some(_) => '?',
                    None => '!',
                };
                row.push(ch);
            }
            rows.push(row);
        }

        rows.join("\n")
    }

    fn assert_raster_golden(surface: &RasterSurfaceFrame, expected: &str) {
        let actual = raster_surface_to_golden(surface);
        let expected = expected.trim_end();
        assert!(
            actual == expected,
            "golden raster mismatch\n--- actual ---\n{actual}\n--- expected ---\n{expected}"
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

        assert_eq!(surface.width, 72);
        assert_eq!(surface.height, 50);
        assert_eq!(surface.frame_count, 1);
        assert_eq!(surface.timestamp_ns, 33);
        assert_eq!(surface.pixel(0, 0), Some(DESKTOP_BACKGROUND_COLOR));
        assert_eq!(surface.pixel(9, 10), Some(FOCUSED_BORDER_COLOR));
        assert_eq!(surface.pixel(16, 20), Some(WINDOW_FILL_COLOR));
        assert_eq!(surface.pixel(15, 22), Some(TEXT_COLOR));
        assert_eq!(surface.pixel(20, 21), Some(CURSOR_COLOR));
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

        assert_eq!(surface.pixel(9, 10), Some(FOCUSED_BORDER_COLOR));
        assert_eq!(surface.pixel(18, 20), Some(UNFOCUSED_BORDER_COLOR));
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

        let mut bytes = vec![0; 72 * 50 * 4];
        let mut target =
            LinearFramebufferTarget::new(72, 50, 72, LinearPixelFormat::Rgb32, &mut bytes);

        let stats = compositor.render_desktop_to_target(
            &mut target,
            vec![DesktopWindow::new(editor, SurfaceRect::new(1, 1, 4, 3)).focused()],
        );

        assert_eq!(stats.frame_count, 1);
        assert_eq!(stats.timestamp_ns, 44);
        assert_eq!(target.pixel(0, 0), Some(DESKTOP_BACKGROUND_COLOR));
        assert_eq!(target.pixel(9, 10), Some(FOCUSED_BORDER_COLOR));
        assert_eq!(target.pixel(15, 22), Some(TEXT_COLOR));
        assert_eq!(target.pixel(20, 21), Some(CURSOR_COLOR));
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

        let mut target = RgbaBuffer::new(108, 50, RgbaColor::new(0, 0, 0, 0));
        compositor.render_desktop_to_target(
            &mut target,
            vec![
                DesktopWindow::new(left.clone(), SurfaceRect::new(1, 1, 4, 3)).focused(),
                DesktopWindow::new(right.clone(), SurfaceRect::new(6, 1, 4, 3)),
            ],
        );
        let preserved_pixel = target.pixel(56, 21);

        let damage_rect = RasterRect::new(18, 20, 8, 8);
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
        assert_eq!(target.pixel(20, 21), Some(CURSOR_COLOR));
        assert_eq!(target.pixel(56, 21), preserved_pixel);
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

        assert_eq!(surface.pixel(19, 12), Some(WINDOW_FILL_COLOR));
        assert_eq!(surface.pixel(20, 12), Some(TEXT_COLOR));
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
