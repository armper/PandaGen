//! GUI host and compositor on view surfaces.

use serde::{Deserialize, Serialize};
use services_workspace_manager::{SplitAxis, WorkspaceRenderSnapshot, WorkspaceTileRenderSnapshot};
use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

const DESKTOP_BACKGROUND: char = '.';
const CURSOR_GLYPH: char = '@';

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
            tabs: Vec::new(),
            z_index: 0,
            focused: false,
        }
    }

    pub fn with_role(mut self, role: DesktopWindowRole) -> Self {
        self.role = role;
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
        windows.sort_by_key(|window| (window.z_index, window.frame.view_id.as_uuid()));

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
    use view_types::{CursorPosition, ViewId, ViewKind};

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
    fn test_workspace_snapshot_maps_vertical_tiles_to_desktop_windows() {
        use services_workspace_manager::{
            ComponentId, WorkspaceLayoutSnapshot, WorkspaceTileLayoutSnapshot,
        };

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
}
