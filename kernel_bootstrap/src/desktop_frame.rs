//! Desktop frame builder for graphics display mode (GFX-020, GFX-031).
//!
//! This module is the bare-metal bridge from workspace *state* to a composed
//! desktop *surface*. It deliberately works on a plain data model
//! (`DesktopModel`) instead of the live `WorkspaceSession`, so the mapping
//! from output lines, prompt, editor viewport, palette, launcher, and notices
//! to shell windows is testable on the host without a kernel.
//!
//! Composition and rasterization are delegated to `services_gui_host`: the
//! shell (`compose_shell`) decides geometry and roles, the compositor paints.
//! The renderer owns one persistent RGBA target sized to the framebuffer so
//! no per-frame surface allocation is needed (the kernel heap is a bump
//! allocator).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use graphics_rasterizer::{RgbaBuffer, RgbaColor};
use services_gui_host::{
    compose_shell, shell_layout, Compositor, DesktopCursor, DesktopWindow, LauncherItem,
    ShellModel, ShellNotice, ShellRects, ShellViewIds, SurfaceRect, SurfaceSize,
    RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH,
};
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

/// Palette overlay content for the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaletteModel {
    pub header: String,
    pub query: String,
    pub results: Vec<String>,
    pub selection: usize,
}

/// Editor viewport content for the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditorModel {
    pub title: String,
    pub lines: Vec<String>,
    /// Cursor as (line, column) within `lines`.
    pub cursor: Option<(usize, usize)>,
    pub status: String,
}

/// Everything the desktop needs to know about the workspace for one frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DesktopModel {
    /// Scrollback lines, oldest first.
    pub output_lines: Vec<String>,
    /// Prompt line including prefix and the command being typed.
    pub prompt: String,
    /// Column of the cursor within `prompt`.
    pub prompt_cursor: usize,
    /// Left status text (mode, hints).
    pub status: String,
    /// Right-aligned status text (ticks, indicators).
    pub status_right: String,
    /// Title for the workspace window chrome.
    pub main_title: String,
    pub editor: Option<EditorModel>,
    pub palette: Option<PaletteModel>,
    /// Pointer position in surface pixels; `None` hides the cursor.
    pub pointer: Option<(usize, usize)>,
    /// Whether the text caret is drawn this frame (blink phase).
    pub caret_visible: bool,
    /// Launcher entries, in display order.
    pub launcher: Vec<LauncherItem>,
    /// Transient notices, newest first.
    pub notices: Vec<ShellNotice>,
}

/// Desktop layout in cell units derived from the pixel surface.
///
/// Thin wrapper over the shell layout so kernel code keeps one name for
/// "where things are".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopLayout {
    pub cells: SurfaceSize,
    pub shell: ShellRects,
}

/// Chrome row plus bottom border, per `services_gui_host` window rendering.
const WINDOW_CHROME_ROWS: usize = 2;

impl DesktopLayout {
    /// Compute the cell layout for a pixel surface.
    pub fn for_pixels(width: usize, height: usize) -> Self {
        let cols = width / RASTER_CELL_WIDTH;
        let rows = height / RASTER_CELL_HEIGHT;
        Self {
            cells: SurfaceSize::new(cols, rows),
            shell: shell_layout(cols, rows),
        }
    }

    /// Workspace (main content) rectangle.
    pub fn main(&self) -> SurfaceRect {
        self.shell.workspace
    }

    /// Number of content lines the main window can show.
    pub fn main_content_rows(&self) -> usize {
        self.shell
            .workspace
            .height
            .saturating_sub(WINDOW_CHROME_ROWS)
    }

    /// Number of launcher entries that fit.
    pub fn launcher_rows(&self) -> usize {
        self.shell
            .launcher
            .height
            .saturating_sub(WINDOW_CHROME_ROWS)
    }
}

/// Stable view identities for the desktop's windows.
///
/// Focus, hover, and capture are tracked by `ViewId`, so the same window must
/// keep the same id from frame to frame even though the window list is
/// rebuilt on every render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopViewIds {
    pub shell: ShellViewIds,
}

impl DesktopViewIds {
    pub fn new() -> Self {
        Self {
            shell: ShellViewIds::new(),
        }
    }

    pub const fn main(&self) -> ViewId {
        self.shell.workspace
    }

    pub const fn launcher(&self) -> ViewId {
        self.shell.launcher
    }

    pub const fn palette(&self) -> ViewId {
        self.shell.palette
    }
}

impl Default for DesktopViewIds {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the workspace (main) view frame and its title from the model.
fn main_frame(layout: &DesktopLayout, model: &DesktopModel, id: ViewId) -> (ViewFrame, String) {
    let content_rows = layout.main_content_rows();
    let (mut frame, title) = match &model.editor {
        Some(editor) => {
            let mut frame = ViewFrame::new(
                id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(editor.lines.iter().take(content_rows).cloned().collect()),
                0,
            );
            if let Some((line, column)) = editor.cursor {
                frame = frame.with_cursor(CursorPosition::new(line, column));
            }
            (frame, editor.title.clone())
        }
        None => {
            let visible_output = content_rows.saturating_sub(1);
            let skip = model.output_lines.len().saturating_sub(visible_output);
            let mut lines: Vec<String> = model.output_lines.iter().skip(skip).cloned().collect();
            let prompt_line = lines.len();
            lines.push(model.prompt.clone());
            let frame = ViewFrame::new(
                id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(lines),
                0,
            )
            .with_cursor(CursorPosition::new(prompt_line, model.prompt_cursor));
            (frame, model.main_title.clone())
        }
    };
    if !model.caret_visible {
        frame.cursor = None;
    }
    (frame, title)
}

/// Map a desktop model into shell windows.
///
/// Window roles carry z-order policy (`DesktopWindowLayer::for_role`), so the
/// palette always composes above the workspace, launcher, and status.
pub fn build_desktop_windows(
    layout: &DesktopLayout,
    model: &DesktopModel,
    ids: &DesktopViewIds,
) -> Vec<DesktopWindow> {
    let (workspace, workspace_title) = main_frame(layout, model, ids.main());

    let status_left = model
        .editor
        .as_ref()
        .map(|editor| editor.status.clone())
        .unwrap_or_else(|| model.status.clone());

    let palette = model.palette.as_ref().map(|palette| {
        let mut lines = Vec::with_capacity(palette.results.len() + 1);
        let mut query = String::from("Search: ");
        query.push_str(&palette.query);
        lines.push(query);
        for (index, result) in palette.results.iter().enumerate() {
            let mut line = String::from(if index == palette.selection {
                "> "
            } else {
                "  "
            });
            line.push_str(result);
            lines.push(line);
        }
        ViewFrame::new(
            ids.palette(),
            ViewKind::Panel,
            0,
            ViewContent::text_buffer(lines),
            0,
        )
    });

    let shell = ShellModel {
        status_left,
        status_right: model.status_right.clone(),
        launcher: model
            .launcher
            .iter()
            .take(layout.launcher_rows())
            .cloned()
            .collect(),
        notices: model.notices.clone(),
        workspace: Some(workspace),
        workspace_title,
        palette,
        palette_title: model
            .palette
            .as_ref()
            .map(|p| p.header.clone())
            .unwrap_or_default(),
    };
    compose_shell(&shell, &layout.shell, &ids.shell)
}

/// Owns the RGBA target and composes desktop frames into it.
pub struct DesktopFrameRenderer {
    target: RgbaBuffer,
    layout: DesktopLayout,
    compositor: Compositor,
    ids: DesktopViewIds,
    frames: u64,
}

const CLEAR_COLOR: RgbaColor = RgbaColor::new(0, 0, 0, 255);

impl DesktopFrameRenderer {
    /// Allocate a renderer for a `width` x `height` pixel framebuffer.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            target: RgbaBuffer::new(width, height, CLEAR_COLOR),
            layout: DesktopLayout::for_pixels(width, height),
            compositor: Compositor::new(),
            ids: DesktopViewIds::new(),
            frames: 0,
        }
    }

    pub const fn compositor(&self) -> &Compositor {
        &self.compositor
    }

    pub const fn view_ids(&self) -> &DesktopViewIds {
        &self.ids
    }

    pub const fn layout(&self) -> &DesktopLayout {
        &self.layout
    }

    pub const fn width(&self) -> usize {
        self.target.width()
    }

    pub const fn height(&self) -> usize {
        self.target.height()
    }

    pub const fn frames_rendered(&self) -> u64 {
        self.frames
    }

    /// Build the window list for `model` with this renderer's stable ids.
    pub fn windows(&self, model: &DesktopModel) -> Vec<DesktopWindow> {
        build_desktop_windows(&self.layout, model, &self.ids)
    }

    /// Compose an already-built window list (lets a caller apply focus first).
    pub fn render_windows(
        &mut self,
        windows: Vec<DesktopWindow>,
        pointer: Option<(usize, usize)>,
    ) -> usize {
        let cursor = pointer.map(|(x, y)| DesktopCursor::new(x, y));
        let stats = self.compositor.render_desktop_to_target_with_cursor(
            &mut self.target,
            windows,
            None,
            cursor,
        );
        self.frames += 1;
        stats.painted_windows
    }

    /// Compose `model` into the RGBA target. Returns the number of windows painted.
    pub fn render(&mut self, model: &DesktopModel) -> usize {
        let windows = self.windows(model);
        self.render_windows(windows, model.pointer)
    }

    /// Tightly packed RGBA8888 pixels, `width * height * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        self.target.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use services_gui_host::{DesktopWindowRole, NoticeLevel};

    fn sample_model() -> DesktopModel {
        DesktopModel {
            output_lines: vec!["PandaGen Workspace".to_string(), "hello".to_string()],
            prompt: "WS > ls".to_string(),
            prompt_cursor: 7,
            status: "WS: Ctrl+P Commands".to_string(),
            status_right: "t=7".to_string(),
            main_title: "Workspace".to_string(),
            editor: None,
            palette: None,
            pointer: None,
            caret_visible: true,
            launcher: vec![
                LauncherItem {
                    label: "Open Editor".to_string(),
                    command: "open_editor".to_string(),
                    active: false,
                },
                LauncherItem {
                    label: "Show Help".to_string(),
                    command: "help".to_string(),
                    active: false,
                },
            ],
            notices: vec![],
        }
    }

    fn find(windows: &[DesktopWindow], role: DesktopWindowRole) -> &DesktopWindow {
        windows
            .iter()
            .find(|w| w.role == role)
            .unwrap_or_else(|| panic!("no window with role {role:?}"))
    }

    #[test]
    fn test_layout_fits_inside_surface() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        assert_eq!(
            layout.cells,
            SurfaceSize::new(1280 / RASTER_CELL_WIDTH, 800 / RASTER_CELL_HEIGHT)
        );
        let main = layout.main();
        assert!(main.x + main.width <= layout.cells.width);
        assert!(main.y + main.height <= layout.cells.height);
        assert_eq!(main.y, layout.shell.status.height);
        assert_eq!(main.x, layout.shell.launcher.width);
        assert!(layout.main_content_rows() > 10);
        assert!(layout.launcher_rows() > 5);

        // Tiny surfaces degrade to empty rects instead of underflowing.
        let tiny = DesktopLayout::for_pixels(RASTER_CELL_WIDTH - 1, RASTER_CELL_HEIGHT - 1);
        assert_eq!(tiny.cells, SurfaceSize::new(0, 0));
        assert_eq!(tiny.main().width, 0);
        assert_eq!(tiny.main_content_rows(), 0);
    }

    #[test]
    fn test_workspace_model_builds_shell_windows() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let ids = DesktopViewIds::new();
        let windows = build_desktop_windows(&layout, &sample_model(), &ids);
        assert_eq!(windows.len(), 3, "workspace, status, launcher");

        let main = find(&windows, DesktopWindowRole::Main);
        assert!(main.focused);
        assert_eq!(main.frame.view_id, ids.main());
        assert_eq!(main.frame.title.as_deref(), Some("Workspace"));
        match &main.frame.content {
            ViewContent::TextBuffer { lines } => {
                assert_eq!(lines.last().map(String::as_str), Some("WS > ls"));
                assert_eq!(lines.len(), 3);
            }
            other => panic!("unexpected content {other:?}"),
        }
        assert_eq!(main.frame.cursor, Some(CursorPosition::new(2, 7)));

        let status = find(&windows, DesktopWindowRole::Status);
        assert!(!status.focused);
        assert!(!status.chrome);
        match &status.frame.content {
            ViewContent::StatusLine { text } => {
                assert!(text.starts_with("WS: Ctrl+P Commands"));
                assert!(text.ends_with("t=7"));
            }
            other => panic!("unexpected content {other:?}"),
        }

        let launcher = find(&windows, DesktopWindowRole::Launcher);
        assert_eq!(launcher.frame.view_id, ids.launcher());
        match &launcher.frame.content {
            ViewContent::TextBuffer { lines } => {
                assert_eq!(
                    lines,
                    &vec!["  Open Editor".to_string(), "  Show Help".to_string()]
                );
            }
            other => panic!("unexpected content {other:?}"),
        }
    }

    #[test]
    fn test_output_tail_is_clipped_to_visible_rows() {
        let layout = DesktopLayout::for_pixels(RASTER_CELL_WIDTH * 60, RASTER_CELL_HEIGHT * 12);
        let rows = layout.main_content_rows();
        let mut model = sample_model();
        model.output_lines = (0..50).map(|i| i.to_string()).collect();
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        let ViewContent::TextBuffer { lines } = &main.frame.content else {
            panic!("main must be a text buffer");
        };
        assert_eq!(lines.len(), rows);
        assert_eq!(lines[0], (50 - (rows - 1)).to_string());
        assert_eq!(lines.last().unwrap(), "WS > ls");
        assert_eq!(main.frame.cursor.unwrap().line, rows - 1);
    }

    #[test]
    fn test_palette_and_notices_add_windows_above_workspace() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let mut model = sample_model();
        model.palette = Some(PaletteModel {
            header: "Commands".to_string(),
            query: "op".to_string(),
            results: vec!["Open Editor".to_string(), "Open CLI".to_string()],
            selection: 1,
        });
        model.notices = vec![ShellNotice {
            level: NoticeLevel::Info,
            text: "Switched to graphics".to_string(),
        }];
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        assert_eq!(windows.len(), 5);
        let main = find(&windows, DesktopWindowRole::Main);
        assert!(!main.focused);
        let palette = find(&windows, DesktopWindowRole::Palette);
        assert!(palette.focused);
        assert_eq!(palette.frame.title.as_deref(), Some("Commands"));
        let ViewContent::TextBuffer { lines } = &palette.frame.content else {
            panic!("palette must be a text buffer");
        };
        assert_eq!(lines, &vec!["Search: op", "  Open Editor", "> Open CLI"]);
        assert!(palette.layer.sort_key() > main.layer.sort_key());
        let notice = find(&windows, DesktopWindowRole::Notification);
        assert_eq!(notice.frame.title.as_deref(), Some("INFO"));
        assert!(notice.layer.sort_key() > main.layer.sort_key());
    }

    #[test]
    fn test_editor_model_replaces_main_content_and_status() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let mut model = sample_model();
        model.editor = Some(EditorModel {
            title: "readme.md".to_string(),
            lines: vec!["# PandaGen".to_string(), "".to_string()],
            cursor: Some((0, 2)),
            status: "-- NORMAL --".to_string(),
        });
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        assert_eq!(main.frame.title.as_deref(), Some("readme.md"));
        assert_eq!(main.frame.cursor, Some(CursorPosition::new(0, 2)));
        let ViewContent::StatusLine { text } =
            &find(&windows, DesktopWindowRole::Status).frame.content
        else {
            panic!("status must be a status line");
        };
        assert!(text.starts_with("-- NORMAL --"));
    }

    #[test]
    fn test_caret_visibility_controls_cursor_in_main_window() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let mut model = sample_model();
        model.caret_visible = false;
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        assert_eq!(find(&windows, DesktopWindowRole::Main).frame.cursor, None);
        model.caret_visible = true;
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        assert!(find(&windows, DesktopWindowRole::Main)
            .frame
            .cursor
            .is_some());
    }

    #[test]
    fn test_renderer_keeps_view_ids_stable_across_frames() {
        let renderer = DesktopFrameRenderer::new(RASTER_CELL_WIDTH * 80, RASTER_CELL_HEIGHT * 30);
        let first = renderer.windows(&sample_model());
        let mut with_palette = sample_model();
        with_palette.palette = Some(PaletteModel::default());
        let second = renderer.windows(&with_palette);
        assert_eq!(first[0].frame.view_id, second[0].frame.view_id);
        assert_eq!(first[1].frame.view_id, second[1].frame.view_id);
        assert_eq!(
            second.last().unwrap().frame.view_id,
            renderer.view_ids().palette()
        );
        assert_ne!(renderer.view_ids().main(), renderer.view_ids().launcher());
    }

    #[test]
    fn test_renderer_paints_exact_framebuffer_size() {
        let (width, height) = (RASTER_CELL_WIDTH * 80, RASTER_CELL_HEIGHT * 30);
        let mut renderer = DesktopFrameRenderer::new(width, height);
        assert_eq!(renderer.pixels().len(), width * height * 4);
        let painted = renderer.render(&sample_model());
        assert_eq!(painted, 3);
        assert_eq!(renderer.frames_rendered(), 1);

        // Background is painted (not the clear colour) and window fill differs.
        let bg = renderer.target.pixel(0, 0).unwrap();
        assert_ne!(bg, CLEAR_COLOR);
        let main = renderer.layout.main();
        let inside_main = renderer
            .target
            .pixel(
                (main.x + 1) * RASTER_CELL_WIDTH + 1,
                (main.y + 1) * RASTER_CELL_HEIGHT + 1,
            )
            .unwrap();
        assert_ne!(inside_main, bg);

        // A pointer paints the cursor sprite on top of the desktop.
        let mut with_pointer = sample_model();
        with_pointer.pointer = Some((300, 400));
        renderer.render(&with_pointer);
        let hotspot = renderer.target.pixel(300, 400).unwrap();
        assert_eq!(hotspot, RgbaColor::new(10, 10, 10, 255));

        // Re-rendering with a palette repaints the whole target deterministically.
        let mut with_palette = sample_model();
        with_palette.palette = Some(PaletteModel::default());
        assert_eq!(renderer.render(&with_palette), 4);
        assert_eq!(renderer.pixels().len(), width * height * 4);
    }
}
