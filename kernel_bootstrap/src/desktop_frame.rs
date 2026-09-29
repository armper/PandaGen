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
use graphics_rasterizer::RasterRect;
use graphics_rasterizer::RgbaColor;
use services_gui_host::{
    compose_shell, shell_layout, Compositor, DesktopCursor, DesktopScene, DesktopWindow,
    HostedSurface, LauncherItem, RenderBackend, ShellModel, ShellNotice, ShellRects, ShellViewIds,
    SoftwareBackend, SurfaceRect, SurfaceSize, Theme, WindowStyle, RASTER_CELL_HEIGHT,
    RASTER_CELL_WIDTH,
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

/// What a frame must repaint (GFX-120).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// Nothing changed: the last frame stands.
    None,
    /// Only these rectangles, which do not overlap.
    Rects(Vec<RasterRect>),
    /// Everything: the first frame, a new theme, wallpaper or size, or a
    /// veil over the whole desk.
    Full,
}

/// How far past a window's bounds its drawing reaches: the soft shadow
/// under a card or the dock (spread and drop), with a pixel to spare.
const SHADOW_REACH: usize = {
    let card = services_gui_host::CARD_SHADOW_SPREAD + services_gui_host::CARD_SHADOW_DROP;
    let dock =
        (services_gui_host::DOCK_SHADOW_SPREAD + services_gui_host::DOCK_SHADOW_DROP) as usize;
    (if card > dock { card } else { dock }) + 2
};

/// Pixels of margin round a caret's damage.
const CARET_PAD: usize = 4;
/// Most separate rectangles a frame repaints; past that they are merged.
const MAX_RECTS: usize = 8;

fn bounding(a: RasterRect, b: RasterRect) -> RasterRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    RasterRect::new(x, y, right - x, bottom - y)
}

/// Rectangles to repaint: added one at a time, overlapping ones merged.
#[derive(Default)]
struct Region(Vec<RasterRect>);

impl Region {
    fn add(&mut self, rect: RasterRect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let mut rect = rect;
        // Merge with everything it overlaps, until nothing does.
        loop {
            match self.0.iter().position(|r| overlaps(*r, rect)) {
                Some(i) => rect = bounding(self.0.swap_remove(i), rect),
                None => break,
            }
        }
        self.0.push(rect);
        if self.0.len() > MAX_RECTS {
            let all = self.0.drain(..).reduce(bounding).expect("some");
            self.0.push(all);
        }
    }
}

/// A window's reach: its bounds and what it draws past them -- a card's
/// shadow; the dock's shadow and, above it, the risen icon and its label.
fn reach(window: &DesktopWindow) -> RasterRect {
    use services_gui_host::{
        CARD_SHADOW_DROP, CARD_SHADOW_SPREAD, DOCK_REACH, DOCK_SHADOW_DROP, DOCK_SHADOW_SPREAD,
    };
    let b = window.bounds();
    // (left and right, up, down), a pixel to spare each: exactly what
    // the painter draws past the bounds.
    let (side, up, down) = match window.style {
        // The top bar paints inside its bounds only.
        WindowStyle::TopBar => return b,
        WindowStyle::Card => (
            CARD_SHADOW_SPREAD + 1,
            CARD_SHADOW_SPREAD + 1,
            CARD_SHADOW_SPREAD + CARD_SHADOW_DROP + 1,
        ),
        WindowStyle::Dock => (
            DOCK_SHADOW_SPREAD as usize + 1,
            DOCK_REACH + 1,
            (DOCK_SHADOW_SPREAD + DOCK_SHADOW_DROP) as usize + 1,
        ),
        _ => (SHADOW_REACH, SHADOW_REACH, SHADOW_REACH),
    };
    RasterRect::new(
        b.x.saturating_sub(side),
        b.y.saturating_sub(up),
        b.width + 2 * side,
        b.height + up + down,
    )
}

/// The parts of a card that changed, if nothing but its header (title,
/// chips), its content (lines, selection, caret, graphics) or its footer
/// did: each is drawn clipped to its own strip, so only those strips need
/// repainting -- what typing, a ticking clock or a program's new view
/// changes (GFX-120). `None` when the card moved, was raised, gained focus
/// or anything else that changes it whole.
fn card_parts_changed(old: &DesktopWindow, new: &DesktopWindow) -> Option<Vec<RasterRect>> {
    use services_gui_host::{CARD_FOOTER_HEIGHT, CARD_HEADER_HEIGHT, CARD_PADDING};
    if new.style != WindowStyle::Card {
        return None;
    }
    let mut same = new.clone();
    same.frame.content = old.frame.content.clone();
    same.frame.cursor = old.frame.cursor;
    same.frame.revision = old.frame.revision;
    same.frame.timestamp_ns = old.frame.timestamp_ns;
    same.highlight_line = old.highlight_line;
    same.selection_spans = old.selection_spans.clone();
    same.line_styles = old.line_styles.clone();
    same.overlay = old.overlay.clone();
    same.frame.title = old.frame.title.clone();
    same.actions = old.actions.clone();
    same.footer = old.footer.clone();
    if &same != old {
        return None;
    }
    let b = new.bounds();
    let mut parts = Vec::new();
    let content_changed = new.frame.content != old.frame.content
        || new.frame.cursor != old.frame.cursor
        || new.highlight_line != old.highlight_line
        || new.selection_spans != old.selection_spans
        || new.line_styles != old.line_styles
        || new.overlay != old.overlay;
    if content_changed {
        let footer = if new.footer.is_some() {
            CARD_FOOTER_HEIGHT
        } else {
            0
        };
        let top = b.y + CARD_HEADER_HEIGHT + CARD_PADDING;
        let bottom = b.bottom().saturating_sub(CARD_PADDING + footer);
        if top < bottom && b.width > CARD_PADDING * 2 {
            // A pixel's margin, as for the caret.
            parts.push(RasterRect::new(
                b.x + CARD_PADDING - 1,
                top - 1,
                b.width - CARD_PADDING * 2 + 2,
                bottom - top + 2,
            ));
        }
    }
    if new.frame.title != old.frame.title || new.actions != old.actions {
        // The header, and the hairline under it.
        parts.push(RasterRect::new(b.x, b.y, b.width, CARD_HEADER_HEIGHT + 2));
    }
    if new.footer != old.footer {
        let h = CARD_FOOTER_HEIGHT + CARD_PADDING;
        parts.push(RasterRect::new(
            b.x,
            b.bottom().saturating_sub(h),
            b.width,
            h,
        ));
    }
    Some(parts)
}

/// Where damage makes a glass window repaint itself whole: what its blur
/// reads -- its bounds and the blur's margin round them.
fn glass_reads(window: &DesktopWindow) -> RasterRect {
    let b = window.bounds();
    let m = 2 * services_gui_host::DOCK_BLUR + 1;
    RasterRect::new(
        b.x.saturating_sub(m),
        b.y.saturating_sub(m),
        b.width + 2 * m,
        b.height + 2 * m,
    )
}

fn overlaps(a: RasterRect, b: RasterRect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// What must be repainted to go from `prev` to `next` (GFX-120).
///
/// A window that changed repaints where it was and where it is, with its
/// shadow; one whose only change is the caret repaints just the caret
/// (the blink that used to repaint the whole screen twice a second). The
/// dock is glass -- it blurs what is behind it -- so damage that touches
/// it repaints all of it. A different theme, wallpaper or size, or a veil
/// anywhere, repaints everything. Separate changes stay separate
/// rectangles: a card and the clock are not the screen between them.
pub fn frame_damage(prev: Option<&DesktopScene>, next: &DesktopScene) -> Damage {
    let Some(prev) = prev else {
        return Damage::Full;
    };
    if prev.size != next.size || prev.theme != next.theme || prev.wallpaper != next.wallpaper {
        return Damage::Full;
    }
    let veil = |s: &DesktopScene| s.windows.iter().any(|w| w.style == WindowStyle::Veil);
    if veil(prev) || veil(next) {
        return Damage::Full;
    }
    let mut region = Region::default();
    for window in &next.windows {
        match prev
            .windows
            .iter()
            .find(|w| w.frame.view_id == window.frame.view_id)
        {
            Some(old) if old == window => {}
            Some(old) => {
                let mut caret_only = window.clone();
                caret_only.frame.cursor = old.frame.cursor;
                let carets = if &caret_only == old {
                    [old.frame.cursor, window.frame.cursor]
                        .into_iter()
                        .flatten()
                        .map(|c| services_gui_host::caret_pixel_rect(window, c))
                        .collect::<Option<Vec<_>>>()
                } else {
                    None
                };
                let parts = if carets.is_none() {
                    card_parts_changed(old, window)
                } else {
                    None
                };
                match carets {
                    None if parts.is_some() => {
                        for part in parts.expect("checked") {
                            region.add(part);
                        }
                    }
                    // A few pixels round it: the painted caret reaches a
                    // little past the rectangle it is said to be in.
                    Some(rects) => {
                        for r in rects {
                            region.add(RasterRect::new(
                                r.x.saturating_sub(CARET_PAD),
                                r.y.saturating_sub(CARET_PAD),
                                r.width + 2 * CARET_PAD,
                                r.height + 2 * CARET_PAD,
                            ));
                        }
                    }
                    None => {
                        region.add(reach(old));
                        region.add(reach(window));
                    }
                }
            }
            None => region.add(reach(window)),
        }
    }
    for old in &prev.windows {
        if !next
            .windows
            .iter()
            .any(|w| w.frame.view_id == old.frame.view_id)
        {
            region.add(reach(old));
        }
    }
    if prev.cursor != next.cursor {
        for cursor in [prev.cursor, next.cursor].into_iter().flatten() {
            region.add(cursor.bounds());
        }
    }
    // Glass: the dock and the top bar blur what is behind all of them,
    // so damage that touches either repaints all of it -- a part
    // repainted alone would blur its old, already-frosted neighbours in.
    for _ in 0..2 {
        for window in &next.windows {
            if matches!(window.style, WindowStyle::Dock | WindowStyle::TopBar) {
                let reads = glass_reads(window);
                if region.0.iter().any(|r| overlaps(*r, reads)) {
                    region.add(reach(window));
                    region.add(reads);
                }
            }
        }
    }
    let (w, h) = next.pixel_size();
    let screen = RasterRect::new(0, 0, w, h);
    let rects: Vec<RasterRect> = region
        .0
        .into_iter()
        .filter_map(|r| r.intersect(screen))
        .collect();
    let area: usize = rects.iter().map(|r| r.width * r.height).sum();
    if rects.is_empty() {
        Damage::None
    } else if area >= w * h {
        Damage::Full
    } else {
        Damage::Rects(rects)
    }
}

/// Owns the RGBA target and composes desktop frames into it.
pub struct DesktopFrameRenderer {
    /// The renderer backend (GFX-050); software is authoritative.
    backend: SoftwareBackend,
    layout: DesktopLayout,
    ids: DesktopViewIds,
    width: usize,
    height: usize,
    /// The scene the surface shows (GFX-120), for the next frame's damage.
    last: Option<DesktopScene>,
    /// The last frame's repaint: pixels repainted.
    last_repainted: u64,
}

/// Background painted before the first frame; only visible if a scene is
/// smaller than the surface.
#[allow(dead_code)]
const CLEAR_COLOR: RgbaColor = RgbaColor::new(0, 0, 0, 255);

impl DesktopFrameRenderer {
    /// Allocate a renderer for a `width` x `height` pixel framebuffer, or
    /// `None` if the backend will not take a surface that size.
    ///
    /// This used to `expect`, and the kernel aborts on panic. Every other
    /// decision on this path is careful -- a graphics mode the budget cannot
    /// afford is refused and falls back to text with a notice -- so an
    /// assertion here was the one place that care was abandoned.
    pub fn try_new(width: usize, height: usize) -> Option<Self> {
        let backend = SoftwareBackend::new(width, height, Theme::DEFAULT).ok()?;
        Some(Self::from_parts(backend, width, height))
    }

    /// Allocate a renderer, panicking if the surface is too large. Tests
    /// only; the boot path uses `try_new` and falls back to text.
    #[cfg(test)]
    pub fn new(width: usize, height: usize) -> Self {
        Self::try_new(width, height).expect("test surface is within the backend limit")
    }

    fn from_parts(backend: SoftwareBackend, width: usize, height: usize) -> Self {
        Self {
            backend,
            layout: DesktopLayout::for_pixels(width, height),
            ids: DesktopViewIds::new(),
            width,
            height,
            last: None,
            last_repainted: 0,
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

    /// Compose an already-built window list under `theme` (the desk paints
    /// with `Theme::DESK`; the classic mode with the backend's own).
    pub fn render_windows_with_theme(
        &mut self,
        windows: Vec<DesktopWindow>,
        pointer: Option<(usize, usize)>,
        theme: Theme,
    ) -> usize {
        self.render_windows_with_look(windows, pointer, theme, None)
    }

    /// As `render_windows_with_theme`, with a wallpaper behind everything
    /// (GFX-066); `None` paints the theme's gradient.
    pub fn render_windows_with_look(
        &mut self,
        windows: Vec<DesktopWindow>,
        pointer: Option<(usize, usize)>,
        theme: Theme,
        wallpaper: Option<services_gui_host::Wallpaper>,
    ) -> usize {
        let scene = self
            .scene(windows, pointer)
            .with_theme(theme)
            .with_wallpaper(wallpaper);
        self.render_scene(scene)
    }

    /// Repaint only what changed since the last frame (GFX-120).
    fn render_scene(&mut self, scene: DesktopScene) -> usize {
        let all = (self.width * self.height) as u64;
        let damage = frame_damage(self.last.as_ref(), &scene);
        let painted = match damage {
            Damage::None => {
                self.last_repainted = 0;
                return 0;
            }
            Damage::Rects(rects) => {
                self.last_repainted = rects.iter().map(|r| (r.width * r.height) as u64).sum();
                let mut painted = 0;
                let mut result = Ok(());
                for rect in rects {
                    match self.backend.render_damage(&scene, rect) {
                        Ok(stats) => painted = painted.max(stats.painted_windows),
                        Err(e) => {
                            result = Err(e);
                            break;
                        }
                    }
                }
                result.map(|()| painted)
            }
            Damage::Full => {
                self.last_repainted = all;
                self.backend
                    .render(&scene)
                    .map(|stats| stats.painted_windows)
            }
        };
        self.last = Some(scene);
        match painted {
            Ok(windows) => windows,
            Err(_) => {
                // Not what the surface shows: start again next frame.
                self.last = None;
                0
            }
        }
    }

    /// Forget the last frame, so the next repaints everything.
    pub fn invalidate(&mut self) {
        self.last = None;
    }

    /// Compose an already-built window list (lets a caller apply focus first).
    pub fn render_windows(
        &mut self,
        windows: Vec<DesktopWindow>,
        pointer: Option<(usize, usize)>,
    ) -> usize {
        let scene = self.scene(windows, pointer);
        self.render_scene(scene)
    }

    /// Compose `model` into the RGBA target. Returns the number of windows painted.
    pub fn render(&mut self, model: &DesktopModel) -> usize {
        let windows = self.windows(model);
        self.render_windows(windows, model.pointer)
    }

    /// The last frame's repaint: pixels repainted, and the frame's pixels
    /// in all (GFX-120).
    pub fn last_repaint(&self) -> (u64, u64) {
        let all = (self.width() * self.height()) as u64;
        (self.last_repainted, all)
    }

    /// Tightly packed RGBA8888 pixels, `width * height * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        self.backend.pixels()
    }
}

#[cfg(test)]
mod damage_tests {
    use super::*;
    use crate::desk::{Desk, DeskApp};

    const W: usize = 1024;
    const H: usize = 640;

    /// Render `desk` as it is now through `renderer` (damage-limited), and
    /// through a fresh renderer in full; the pixels must be the same.
    fn same_as_full(
        renderer: &mut DesktopFrameRenderer,
        desk: &mut Desk,
        caret: bool,
        pointer: Option<(usize, usize)>,
        what: &str,
    ) -> u64 {
        let windows = desk.windows("12:00", caret, None);
        renderer.render_windows_with_look(windows.clone(), pointer, desk.theme(), desk.wallpaper());
        let mut fresh = DesktopFrameRenderer::new(W, H);
        fresh.render_windows_with_look(windows, pointer, desk.theme(), desk.wallpaper());
        let (a, b) = (renderer.pixels(), fresh.pixels());
        if a != b {
            let first = a.iter().zip(b).position(|(x, y)| x != y).unwrap() / 4;
            panic!("{what}: differs first at ({}, {})", first % W, first / W);
        }
        renderer.last_repaint().0
    }

    #[test]
    fn a_damage_limited_frame_is_the_full_frame_pixel_for_pixel() {
        let mut desk = Desk::new(W, H);
        let mut renderer = DesktopFrameRenderer::new(W, H);
        let all = (W * H) as u64;
        let notepad = desk.launch(DeskApp::Notepad);
        for byte in b"hello" {
            desk.handle_key(*byte);
        }
        assert_eq!(
            same_as_full(&mut renderer, &mut desk, true, None, "first"),
            all
        );
        // Nothing changed: nothing repainted.
        assert_eq!(
            same_as_full(&mut renderer, &mut desk, true, None, "again"),
            0
        );
        // The caret blinks: a sliver, not the screen.
        let blink = same_as_full(&mut renderer, &mut desk, false, None, "blink");
        assert!(blink > 0 && blink < all / 100, "the caret alone: {blink}");
        same_as_full(&mut renderer, &mut desk, true, None, "blink back");
        // Typing: the card, not the screen.
        desk.handle_key(b'!');
        let typed = same_as_full(&mut renderer, &mut desk, true, None, "typed");
        assert!(typed < all / 2, "one card: {typed}");
        // The pointer moves.
        same_as_full(&mut renderer, &mut desk, true, Some((300, 300)), "pointer");
        same_as_full(
            &mut renderer,
            &mut desk,
            true,
            Some((320, 310)),
            "pointer moved",
        );
        // A second card opens over the first, then the first is raised,
        // then the second closes -- shadows and all.
        let calc = desk.launch(DeskApp::Calculator);
        same_as_full(&mut renderer, &mut desk, true, None, "opened");
        desk.raise(notepad);
        same_as_full(&mut renderer, &mut desk, true, None, "raised");
        desk.close(calc);
        same_as_full(&mut renderer, &mut desk, true, None, "closed");
        // A theme change is everything.
        renderer.invalidate();
        assert_eq!(
            same_as_full(&mut renderer, &mut desk, true, None, "invalidated"),
            all
        );
    }

    #[test]
    fn damage_that_touches_glass_takes_all_of_it() {
        let desk_scene = |renderer: &DesktopFrameRenderer, desk: &mut Desk| {
            renderer
                .scene(desk.windows("12:00", true, None), None)
                .with_theme(desk.theme())
        };
        let mut desk = Desk::new(W, H);
        let renderer = DesktopFrameRenderer::new(W, H);
        let before = desk_scene(&renderer, &mut desk);
        let after = desk_scene(&renderer, &mut desk);
        assert_eq!(frame_damage(Some(&before), &after), Damage::None);
        assert_eq!(frame_damage(None, &after), Damage::Full);
        // The clock changes: the bar, whole.
        let later = renderer
            .scene(desk.windows("12:01", true, None), None)
            .with_theme(desk.theme());
        let bar = later
            .windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap()
            .bounds();
        match frame_damage(Some(&after), &later) {
            Damage::Rects(rects) => {
                assert!(
                    rects
                        .iter()
                        .any(|d| d.x <= bar.x && d.right() >= bar.right()),
                    "{rects:?} vs {bar:?}"
                );
            }
            other => panic!("{other:?}"),
        }
    }
}

#[cfg(test)]
mod surface_limit_tests {
    use super::*;

    #[test]
    fn a_surface_the_backend_will_not_take_is_refused_not_asserted() {
        // `new` used to `expect`, and the kernel aborts on panic. Every other
        // decision on this path refuses and falls back to text with a notice;
        // this was the one place that care was abandoned.
        assert!(
            DesktopFrameRenderer::try_new(4000, 3000).is_none(),
            "a surface past the backend limit must be refused"
        );
        assert!(
            DesktopFrameRenderer::try_new(1280, 800).is_some(),
            "and an ordinary one must still be built"
        );
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
