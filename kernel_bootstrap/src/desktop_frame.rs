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
use graphics_rasterizer::RgbaColor;
use services_gui_host::{
    compose_shell, shell_layout, Compositor, DesktopCursor, DesktopScene, DesktopWindow,
    HostedSurface, LauncherItem, RenderBackend, ShellModel, ShellNotice, ShellRects, ShellViewIds,
    SoftwareBackend, SurfaceRect, SurfaceSize, Theme, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH,
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
    /// Document line number of `lines[0]` (0-based).
    pub first_line: usize,
    /// Total document lines, for gutter width and the status strip.
    pub line_count: usize,
    pub dirty: bool,
}

/// Width in cells of the line-number gutter for `line_count` lines
/// (digits plus one space), at least 3 digits wide.
pub fn gutter_width(line_count: usize) -> usize {
    let mut digits = 1;
    let mut n = line_count.max(1);
    while n >= 10 {
        n /= 10;
        digits += 1;
    }
    digits.max(3) + 1
}

/// Prefix `text` with a right-aligned line number in a gutter of `width` cells.
pub fn gutter_line(number: usize, width: usize, text: &str) -> String {
    let digits = alloc::format!("{}", number);
    let mut line = String::new();
    for _ in digits.len()..width.saturating_sub(1) {
        line.push(' ');
    }
    line.push_str(&digits);
    line.push(' ');
    line.push_str(text);
    line
}

/// File picker content for the desktop (GFX-037).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PickerModel {
    /// Directory breadcrumb shown on the first line.
    pub breadcrumb: String,
    pub entries: Vec<String>,
    pub selection: usize,
}

/// Picker content line of the first entry (line 0 is the breadcrumb).
pub const PICKER_ENTRIES_FIRST_LINE: usize = 1;

/// Map a picker content line to an entry index, if it is one.
pub fn picker_entry_at_line(line: usize) -> Option<usize> {
    line.checked_sub(PICKER_ENTRIES_FIRST_LINE)
}

/// Pipeline status surface content (GFX-039).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PipelineModel {
    /// One line per stage, already formatted.
    pub lines: Vec<String>,
    /// Index of the running stage, if any.
    pub running: Option<usize>,
    pub done: usize,
    pub total: usize,
    pub failed: bool,
    pub finished: bool,
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
    /// File picker shown in the workspace window when no editor is open.
    pub picker: Option<PickerModel>,
    /// Lines the scrollback view is scrolled up from the tail.
    pub scrollback_offset: usize,
    /// Pipeline run shown in the workspace window (no editor or picker).
    pub pipeline: Option<PipelineModel>,
    /// A custom hosted component's surface (GFX-040); takes the workspace
    /// window when present and no editor or picker is open.
    pub hosted: Option<HostedSurface>,
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
/// Palette content line of the first result (line 0 is the search row).
pub const PALETTE_RESULTS_FIRST_LINE: usize = 1;

/// Map a palette content line to a result index, if it is one.
pub fn palette_result_at_line(line: usize) -> Option<usize> {
    line.checked_sub(PALETTE_RESULTS_FIRST_LINE)
}

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
            // Graphical editor view (GFX-036): line-number gutter, and the
            // caret shifted past it.
            let width = gutter_width(editor.line_count);
            let lines: Vec<String> = editor
                .lines
                .iter()
                .take(content_rows)
                .enumerate()
                .map(|(i, text)| {
                    let number = editor.first_line + i + 1;
                    if number <= editor.line_count.max(1) {
                        gutter_line(number, width, text)
                    } else {
                        // Past the end of the document: blank gutter, no text.
                        let mut blank = String::new();
                        for _ in 0..width {
                            blank.push(' ');
                        }
                        blank
                    }
                })
                .collect();
            let mut frame = ViewFrame::new(
                id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            if let Some((line, column)) = editor.cursor {
                frame = frame.with_cursor(CursorPosition::new(line, column + width));
            }
            let mut title = editor.title.clone();
            if editor.dirty {
                title.push_str(" [+]");
            }
            (frame, title)
        }
        None if model.hosted.is_some() && model.picker.is_none() => {
            let hosted = model.hosted.as_ref().expect("checked");
            let mut frame = ViewFrame::new(
                id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(hosted.lines.iter().take(content_rows).cloned().collect()),
                0,
            );
            if let Some((line, column)) = hosted.caret {
                frame = frame.with_cursor(CursorPosition::new(line, column));
            }
            (frame, hosted.title.clone())
        }
        None if model.pipeline.is_some() && model.picker.is_none() => {
            // Pipeline status surface: the trace on top, prompt pinned below.
            let pipeline = model.pipeline.as_ref().expect("checked");
            let mut lines: Vec<String> = pipeline
                .lines
                .iter()
                .take(content_rows.saturating_sub(2))
                .cloned()
                .collect();
            lines.push(String::new());
            let prompt_line = lines.len();
            lines.push(model.prompt.clone());
            let frame = ViewFrame::new(id, ViewKind::Panel, 0, ViewContent::text_buffer(lines), 0)
                .with_cursor(CursorPosition::new(prompt_line, model.prompt_cursor));
            let title = if pipeline.finished {
                if pipeline.failed {
                    String::from("Pipeline (failed)")
                } else {
                    String::from("Pipeline (done)")
                }
            } else {
                String::from("Pipeline (running)")
            };
            (frame, title)
        }
        None if model.picker.is_some() => {
            let picker = model.picker.as_ref().expect("checked");
            let mut lines = Vec::with_capacity(picker.entries.len() + 1);
            let mut crumb = String::from("ROOT / ");
            crumb.push_str(&picker.breadcrumb);
            lines.push(crumb);
            lines.extend(
                picker
                    .entries
                    .iter()
                    .take(content_rows.saturating_sub(1))
                    .cloned(),
            );
            let frame = ViewFrame::new(id, ViewKind::Panel, 0, ViewContent::text_buffer(lines), 0);
            (frame, String::from("Files"))
        }
        None => {
            // Text-native host surface (GFX-038): the tail of the scrollback,
            // optionally scrolled up, with the prompt pinned below it.
            let visible_output = content_rows.saturating_sub(1);
            let max_offset = model.output_lines.len().saturating_sub(visible_output);
            let offset = model.scrollback_offset.min(max_offset);
            let skip = max_offset - offset;
            let mut lines: Vec<String> = model
                .output_lines
                .iter()
                .skip(skip)
                .take(visible_output)
                .cloned()
                .collect();
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
            let mut title = model.main_title.clone();
            if offset > 0 {
                title.push_str(&alloc::format!(" (scrolled {} lines)", offset));
            }
            (frame, title)
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
        .map(|editor| {
            let (line, col) = editor
                .cursor
                .map(|(l, c)| (editor.first_line + l + 1, c + 1))
                .unwrap_or((0, 0));
            alloc::format!(
                "{}  {}{}  Ln {}, Col {}  ({} lines)",
                editor.status,
                editor.title,
                if editor.dirty { " [+]" } else { "" },
                line,
                col,
                editor.line_count
            )
        })
        .unwrap_or_else(|| model.status.clone());
    let status_left = match (&model.editor, &model.picker, &model.pipeline) {
        (None, None, _) if model.hosted.is_some() => {
            model.hosted.as_ref().expect("checked").status.clone()
        }
        (None, Some(picker), _) => alloc::format!(
            "Files: {} entries  Up/Down select  Enter open  Esc close",
            picker.entries.len()
        ),
        (None, None, Some(pipeline)) => alloc::format!(
            "Pipeline: {}/{} stages done{}  (pipeline clear to dismiss)",
            pipeline.done,
            pipeline.total,
            if pipeline.failed { ", failed" } else { "" }
        ),
        _ => status_left,
    };
    let workspace_highlight = match (&model.editor, &model.picker, &model.pipeline) {
        (Some(editor), _, _) => editor.cursor.map(|(line, _)| line),
        (None, None, _) if model.hosted.is_some() => {
            model.hosted.as_ref().expect("checked").highlight
        }
        (None, Some(picker), _) if !picker.entries.is_empty() => {
            Some(PICKER_ENTRIES_FIRST_LINE + picker.selection)
        }
        (None, None, Some(pipeline)) => pipeline.running,
        _ => None,
    };

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
        workspace_highlight,
        palette,
        palette_title: model
            .palette
            .as_ref()
            .map(|p| p.header.clone())
            .unwrap_or_default(),
        // Line 0 is the search row; results start at line 1.
        palette_selection: model
            .palette
            .as_ref()
            .filter(|p| !p.results.is_empty())
            .map(|p| PALETTE_RESULTS_FIRST_LINE + p.selection),
    };
    compose_shell(&shell, &layout.shell, &ids.shell)
}

/// Owns the RGBA target and composes desktop frames into it.
pub struct DesktopFrameRenderer {
    /// The renderer backend (GFX-050); software is authoritative.
    backend: SoftwareBackend,
    layout: DesktopLayout,
    ids: DesktopViewIds,
    width: usize,
    height: usize,
}

/// Background painted before the first frame; only visible if a scene is
/// smaller than the surface.
#[allow(dead_code)]
const CLEAR_COLOR: RgbaColor = RgbaColor::new(0, 0, 0, 255);

impl DesktopFrameRenderer {
    /// Allocate a renderer for a `width` x `height` pixel framebuffer.
    pub fn new(width: usize, height: usize) -> Self {
        let backend = SoftwareBackend::new(width, height, Theme::DEFAULT)
            .expect("framebuffer surfaces are within the software backend limit");
        Self {
            backend,
            layout: DesktopLayout::for_pixels(width, height),
            ids: DesktopViewIds::new(),
            width,
            height,
        }
    }

    pub fn compositor(&self) -> &Compositor {
        self.backend.compositor()
    }

    pub fn backend(&self) -> &dyn RenderBackend {
        &self.backend
    }

    pub const fn view_ids(&self) -> &DesktopViewIds {
        &self.ids
    }

    pub const fn layout(&self) -> &DesktopLayout {
        &self.layout
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn frames_rendered(&self) -> u64 {
        self.backend.frames_rendered()
    }

    /// Cell size of the desktop scene this renderer produces.
    pub fn scene_size(&self) -> SurfaceSize {
        self.layout.cells
    }

    /// Describe a frame as a scene: what the backend renders and what the
    /// remote UI host can ship.
    pub fn scene(
        &self,
        windows: Vec<DesktopWindow>,
        pointer: Option<(usize, usize)>,
    ) -> DesktopScene {
        DesktopScene::new(self.layout.cells, windows)
            .with_cursor(pointer.map(|(x, y)| DesktopCursor::new(x, y)))
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
        let scene = self.scene(windows, pointer);
        match self.backend.render(&scene) {
            Ok(stats) => stats.painted_windows,
            // The scene is built from this renderer's own layout, so a size
            // mismatch cannot happen; treat it as "nothing painted".
            Err(_) => 0,
        }
    }

    /// Compose `model` into the RGBA target. Returns the number of windows painted.
    pub fn render(&mut self, model: &DesktopModel) -> usize {
        let windows = self.windows(model);
        self.render_windows(windows, model.pointer)
    }

    /// Tightly packed RGBA8888 pixels, `width * height * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        self.backend.pixels()
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
            picker: None,
            scrollback_offset: 0,
            pipeline: None,
            hosted: None,
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

    fn pixel_at(renderer: &DesktopFrameRenderer, x: usize, y: usize) -> RgbaColor {
        let offset = (y * renderer.width() + x) * 4;
        let p = &renderer.pixels()[offset..offset + 4];
        RgbaColor::new(p[0], p[1], p[2], p[3])
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
        model.notices = vec![ShellNotice::new(NoticeLevel::Info, "Switched to graphics")];
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
        assert_eq!(
            palette.highlight_line,
            Some(2),
            "selection 1 is content line 2"
        );
        assert_eq!(palette_result_at_line(2), Some(1));
        assert_eq!(palette_result_at_line(0), None);
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
            cursor: Some((1, 2)),
            status: "-- NORMAL --".to_string(),
            first_line: 9,
            line_count: 42,
            dirty: true,
        });
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        assert_eq!(main.frame.title.as_deref(), Some("readme.md [+]"));
        // Gutter is 3 digits + space = 4 cells; caret shifts past it.
        assert_eq!(main.frame.cursor, Some(CursorPosition::new(1, 6)));
        assert_eq!(main.highlight_line, Some(1), "current line highlighted");
        let ViewContent::TextBuffer { lines } = &main.frame.content else {
            panic!("main must be a text buffer");
        };
        assert_eq!(lines[0], " 10 # PandaGen");
        assert_eq!(lines[1], " 11 ");

        let ViewContent::StatusLine { text } =
            &find(&windows, DesktopWindowRole::Status).frame.content
        else {
            panic!("status must be a status line");
        };
        assert!(
            text.starts_with("-- NORMAL --  readme.md [+]  Ln 11, Col 3  (42 lines)"),
            "{text}"
        );
        assert_eq!(gutter_width(9), 4);
        assert_eq!(gutter_width(12345), 6);
        assert_eq!(gutter_line(7, 4, "x"), "  7 x");

        // Rows past the end of a short document get a blank gutter.
        let mut short = sample_model();
        short.editor = Some(EditorModel {
            title: "a.txt".to_string(),
            lines: vec!["one".to_string(), "".to_string(), "".to_string()],
            cursor: Some((0, 0)),
            status: "-- NORMAL --".to_string(),
            first_line: 0,
            line_count: 1,
            dirty: false,
        });
        let windows = build_desktop_windows(&layout, &short, &DesktopViewIds::new());
        let ViewContent::TextBuffer { lines } =
            &find(&windows, DesktopWindowRole::Main).frame.content
        else {
            panic!()
        };
        assert_eq!(lines[0], "  1 one");
        assert_eq!(lines[1], "    ");
        assert_eq!(
            find(&windows, DesktopWindowRole::Main)
                .frame
                .title
                .as_deref(),
            Some("a.txt")
        );
    }

    #[test]
    fn test_scrollback_offset_shows_older_lines_and_marks_title() {
        let layout = DesktopLayout::for_pixels(RASTER_CELL_WIDTH * 60, RASTER_CELL_HEIGHT * 12);
        let rows = layout.main_content_rows();
        let visible = rows - 1;
        let mut model = sample_model();
        model.output_lines = (0..50).map(|i| i.to_string()).collect();
        model.scrollback_offset = 3;
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        let ViewContent::TextBuffer { lines } = &main.frame.content else {
            panic!()
        };
        assert_eq!(lines.len(), rows);
        assert_eq!(lines[0], (50 - visible - 3).to_string());
        assert_eq!(lines[visible - 1], (50 - 1 - 3).to_string());
        assert_eq!(lines.last().unwrap(), "WS > ls", "prompt stays pinned");
        assert_eq!(
            main.frame.title.as_deref(),
            Some("Workspace (scrolled 3 lines)")
        );

        // Offsets past the top clamp.
        model.scrollback_offset = 1000;
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let ViewContent::TextBuffer { lines } =
            &find(&windows, DesktopWindowRole::Main).frame.content
        else {
            panic!()
        };
        assert_eq!(lines[0], "0");
    }

    #[test]
    fn test_hosted_surface_takes_the_workspace_window() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let mut model = sample_model();
        model.hosted = Some(
            HostedSurface::new("About")
                .with_lines(vec!["PandaGen".to_string(), "clean-slate".to_string()])
                .with_highlight(Some(1))
                .with_caret(0, 3)
                .with_status("Esc closes"),
        );
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        assert_eq!(main.frame.title.as_deref(), Some("About"));
        assert_eq!(main.highlight_line, Some(1));
        assert_eq!(main.frame.cursor, Some(CursorPosition::new(0, 3)));
        let ViewContent::TextBuffer { lines } = &main.frame.content else {
            panic!()
        };
        assert_eq!(lines, &vec!["PandaGen", "clean-slate"]);
        let ViewContent::StatusLine { text } =
            &find(&windows, DesktopWindowRole::Status).frame.content
        else {
            panic!()
        };
        assert!(text.starts_with("Esc closes"), "{text}");

        // An open editor still wins over a hosted component.
        model.editor = Some(EditorModel {
            title: "x".to_string(),
            lines: vec![],
            cursor: None,
            status: "-- NORMAL --".to_string(),
            first_line: 0,
            line_count: 0,
            dirty: false,
        });
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        assert_eq!(
            find(&windows, DesktopWindowRole::Main)
                .frame
                .title
                .as_deref(),
            Some("x")
        );
    }

    #[test]
    fn test_pipeline_model_renders_trace_with_running_stage_highlighted() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let mut model = sample_model();
        model.pipeline = Some(PipelineModel {
            lines: vec![
                "[ ok ] help  1 ticks  commands: help".to_string(),
                "[ >> ] ticks".to_string(),
                "[ .. ] mem".to_string(),
            ],
            running: Some(1),
            done: 1,
            total: 3,
            failed: false,
            finished: false,
        });
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        assert_eq!(main.frame.title.as_deref(), Some("Pipeline (running)"));
        assert_eq!(main.highlight_line, Some(1));
        let ViewContent::TextBuffer { lines } = &main.frame.content else {
            panic!()
        };
        assert_eq!(lines[1], "[ >> ] ticks");
        assert_eq!(lines.last().unwrap(), "WS > ls", "prompt pinned");
        assert_eq!(main.frame.cursor, Some(CursorPosition::new(4, 7)));
        let ViewContent::StatusLine { text } =
            &find(&windows, DesktopWindowRole::Status).frame.content
        else {
            panic!()
        };
        assert!(text.starts_with("Pipeline: 1/3 stages done"), "{text}");

        let mut failed = model.clone();
        let p = failed.pipeline.as_mut().unwrap();
        p.running = None;
        p.failed = true;
        p.finished = true;
        let windows = build_desktop_windows(&layout, &failed, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        assert_eq!(main.frame.title.as_deref(), Some("Pipeline (failed)"));
        assert_eq!(main.highlight_line, None);
    }

    #[test]
    fn test_picker_model_renders_breadcrumb_entries_and_selection() {
        let layout = DesktopLayout::for_pixels(1280, 800);
        let mut model = sample_model();
        model.picker = Some(PickerModel {
            breadcrumb: String::new(),
            entries: vec!["readme.md".to_string(), "test.txt".to_string()],
            selection: 1,
        });
        let windows = build_desktop_windows(&layout, &model, &DesktopViewIds::new());
        let main = find(&windows, DesktopWindowRole::Main);
        assert_eq!(main.frame.title.as_deref(), Some("Files"));
        assert_eq!(main.frame.cursor, None, "picker shows no caret");
        assert_eq!(main.highlight_line, Some(2));
        let ViewContent::TextBuffer { lines } = &main.frame.content else {
            panic!()
        };
        assert_eq!(lines, &vec!["ROOT / ", "readme.md", "test.txt"]);
        let ViewContent::StatusLine { text } =
            &find(&windows, DesktopWindowRole::Status).frame.content
        else {
            panic!()
        };
        assert!(text.starts_with("Files: 2 entries"), "{text}");
        assert_eq!(picker_entry_at_line(2), Some(1));
        assert_eq!(picker_entry_at_line(0), None);
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
        let bg = pixel_at(&renderer, 0, 0);
        assert_ne!(bg, CLEAR_COLOR);
        let main = renderer.layout.main();
        let inside_main = pixel_at(
            &renderer,
            (main.x + 1) * RASTER_CELL_WIDTH + 1,
            (main.y + 1) * RASTER_CELL_HEIGHT + 1,
        );
        assert_ne!(inside_main, bg);

        // A pointer paints the cursor sprite on top of the desktop.
        let mut with_pointer = sample_model();
        with_pointer.pointer = Some((300, 400));
        renderer.render(&with_pointer);
        let hotspot = pixel_at(&renderer, 300, 400);
        assert_eq!(hotspot, RgbaColor::new(10, 10, 10, 255));

        // Re-rendering with a palette repaints the whole target deterministically.
        let mut with_palette = sample_model();
        with_palette.palette = Some(PaletteModel::default());
        assert_eq!(renderer.render(&with_palette), 4);
        assert_eq!(renderer.pixels().len(), width * height * 4);
    }
}
