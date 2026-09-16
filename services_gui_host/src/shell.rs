//! Minimal graphical shell surface (GFX-031).
//!
//! The shell is the frame around applications: a status bar along the top,
//! a launcher strip down the left, transient notices in the top-right of the
//! workspace area, the palette centred over everything, and the workspace
//! itself filling the rest. It is described by a plain `ShellModel` and laid
//! out with the layout vocabulary, so a shell frame is deterministic data:
//! the same model and size always compose the same windows.
//!
//! The shell owns no application state. Callers hand it the workspace
//! `ViewFrame` and the strings to show; it decides geometry and window roles.

use alloc::string::String;
use alloc::vec::Vec;
use graphics_rasterizer::RasterRect;
use serde::{Deserialize, Serialize};
use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

use crate::layout::{Anchor, Insets, LayoutId, LayoutNode, Length};
use crate::{DesktopWindow, DesktopWindowRole, SurfaceRect};

/// Cells of height for the status bar (one content line plus borders).
pub const STATUS_BAR_ROWS: usize = 2;
/// Cells of width for the launcher strip.
pub const LAUNCHER_COLS: usize = 18;
/// Cells of width for a notice card.
pub const NOTICE_COLS: usize = 36;
/// Cells of height for one notice card (chrome row plus one line plus border).
pub const NOTICE_ROWS: usize = 3;
/// Maximum notices shown at once; older ones are dropped by the caller.
pub const MAX_NOTICES: usize = 4;

/// One launcher entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LauncherItem {
    pub label: String,
    /// Command identifier the shell reports back when the item is activated.
    pub command: String,
    /// Highlighted (e.g. the component currently open).
    pub active: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum NoticeLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl NoticeLevel {
    pub const fn label(self) -> &'static str {
        match self {
            NoticeLevel::Info => "info",
            NoticeLevel::Success => "ok",
            NoticeLevel::Warning => "warn",
            NoticeLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShellNotice {
    pub level: NoticeLevel,
    pub text: String,
}

/// Everything the shell shows besides the workspace content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShellModel {
    /// Left-aligned status text (mode, hints).
    pub status_left: String,
    /// Right-aligned status text (clock, ticks, indicators).
    pub status_right: String,
    pub launcher: Vec<LauncherItem>,
    /// Newest first; only the first `MAX_NOTICES` are shown.
    pub notices: Vec<ShellNotice>,
    /// Workspace content and title (None shows an empty workspace surface).
    pub workspace: Option<ViewFrame>,
    pub workspace_title: String,
    /// Palette content, when open.
    pub palette: Option<ViewFrame>,
    pub palette_title: String,
    /// Palette content line to highlight (the selected result).
    pub palette_selection: Option<usize>,
}

/// Stable identities for shell windows across frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellViewIds {
    pub status: ViewId,
    pub launcher: ViewId,
    pub workspace: ViewId,
    pub palette: ViewId,
    pub notices: [ViewId; MAX_NOTICES],
}

impl ShellViewIds {
    pub fn new() -> Self {
        Self {
            status: ViewId::new(),
            launcher: ViewId::new(),
            workspace: ViewId::new(),
            palette: ViewId::new(),
            notices: [ViewId::new(), ViewId::new(), ViewId::new(), ViewId::new()],
        }
    }
}

impl Default for ShellViewIds {
    fn default() -> Self {
        Self::new()
    }
}

/// Solved shell geometry in cell units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRects {
    pub cols: usize,
    pub rows: usize,
    pub status: SurfaceRect,
    pub launcher: SurfaceRect,
    pub workspace: SurfaceRect,
    pub palette: SurfaceRect,
    /// Notice cards, top-right of the workspace, newest first.
    pub notices: [SurfaceRect; MAX_NOTICES],
}

const STATUS_ID: LayoutId = LayoutId(1);
const LAUNCHER_ID: LayoutId = LayoutId(2);
const WORKSPACE_ID: LayoutId = LayoutId(3);
const PALETTE_ID: LayoutId = LayoutId(4);
const NOTICE_BASE_ID: u32 = 10;

/// Layout tree for a `cols` x `rows` cell surface.
pub fn shell_tree(cols: usize, rows: usize) -> LayoutNode {
    let notice_stack = LayoutNode::anchored(
        Anchor::TopRight,
        NOTICE_COLS,
        NOTICE_ROWS * MAX_NOTICES,
        LayoutNode::vstack(
            0,
            (0..MAX_NOTICES)
                .map(|i| {
                    (
                        Length::Fixed(NOTICE_ROWS),
                        LayoutNode::Leaf(LayoutId(NOTICE_BASE_ID + i as u32)),
                    )
                })
                .collect(),
        ),
    );
    let workspace_area = LayoutNode::overlay(alloc::vec![
        LayoutNode::Leaf(WORKSPACE_ID),
        LayoutNode::padded(Insets::new(0, 1, 0, 0), notice_stack),
    ]);
    let body = LayoutNode::hstack(
        0,
        alloc::vec![
            (Length::Fixed(LAUNCHER_COLS), LayoutNode::Leaf(LAUNCHER_ID)),
            (Length::Weight(1), workspace_area),
        ],
    );
    LayoutNode::overlay(alloc::vec![
        LayoutNode::vstack(
            0,
            alloc::vec![
                (Length::Fixed(STATUS_BAR_ROWS), LayoutNode::Leaf(STATUS_ID)),
                (Length::Weight(1), body),
            ],
        ),
        LayoutNode::anchored(
            Anchor::Center,
            (cols / 2).max(1),
            (rows / 2).max(1),
            LayoutNode::Leaf(PALETTE_ID),
        ),
    ])
}

/// Solve the shell layout for a cell surface.
pub fn shell_layout(cols: usize, rows: usize) -> ShellRects {
    let solved = shell_tree(cols, rows).solve(RasterRect::new(0, 0, cols, rows));
    let find = |id: LayoutId| {
        solved
            .iter()
            .find(|(leaf, _)| *leaf == id)
            .map(|(_, r)| SurfaceRect::new(r.x, r.y, r.width, r.height))
            .unwrap_or(SurfaceRect::new(0, 0, 0, 0))
    };
    let mut notices = [SurfaceRect::new(0, 0, 0, 0); MAX_NOTICES];
    for (i, slot) in notices.iter_mut().enumerate() {
        *slot = find(LayoutId(NOTICE_BASE_ID + i as u32));
    }
    ShellRects {
        cols,
        rows,
        status: find(STATUS_ID),
        launcher: find(LAUNCHER_ID),
        workspace: find(WORKSPACE_ID),
        palette: find(PALETTE_ID),
        notices,
    }
}

/// Right-align `right` after `left` within `width` cells, truncating `left`
/// first if both do not fit.
fn status_line(left: &str, right: &str, width: usize) -> String {
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    let mut out = String::new();
    if left_len + 1 + right_len <= width {
        out.push_str(left);
        for _ in 0..(width - left_len - right_len) {
            out.push(' ');
        }
        out.push_str(right);
    } else {
        let keep = width.saturating_sub(right_len + 1);
        out.extend(left.chars().take(keep));
        if right_len < width {
            out.push(' ');
            out.push_str(right);
        }
    }
    out
}

/// Compose shell windows for `model`. Order: workspace, status, launcher,
/// notices (newest first), palette. Z-order is decided by roles, not by
/// this order.
pub fn compose_shell(
    model: &ShellModel,
    rects: &ShellRects,
    ids: &ShellViewIds,
) -> Vec<DesktopWindow> {
    let mut windows = Vec::with_capacity(4 + MAX_NOTICES);

    let workspace_frame = model.workspace.clone().unwrap_or_else(|| {
        ViewFrame::new(
            ids.workspace,
            ViewKind::TextBuffer,
            0,
            ViewContent::empty_text_buffer(),
            0,
        )
    });
    let mut workspace_frame = workspace_frame;
    workspace_frame.view_id = ids.workspace;
    workspace_frame.title = Some(model.workspace_title.clone());
    let mut workspace =
        DesktopWindow::new(workspace_frame, rects.workspace).with_role(DesktopWindowRole::Main);
    if model.palette.is_none() {
        workspace = workspace.focused();
    }
    windows.push(workspace);

    let status_width = rects.status.width.saturating_sub(2);
    let status_text = status_line(&model.status_left, &model.status_right, status_width);
    windows.push(
        DesktopWindow::new(
            ViewFrame::new(
                ids.status,
                ViewKind::StatusLine,
                0,
                ViewContent::status_line(status_text),
                0,
            ),
            rects.status,
        )
        .with_role(DesktopWindowRole::Status)
        .without_chrome(),
    );

    let launcher_lines: Vec<String> = model
        .launcher
        .iter()
        .map(|item| {
            let mut line = String::from(if item.active { "> " } else { "  " });
            line.push_str(&item.label);
            line
        })
        .collect();
    let active_launcher = model.launcher.iter().position(|item| item.active);
    windows.push(
        DesktopWindow::new(
            ViewFrame::new(
                ids.launcher,
                ViewKind::Panel,
                0,
                ViewContent::text_buffer(launcher_lines),
                0,
            )
            .with_title("Launch"),
            rects.launcher,
        )
        .with_role(DesktopWindowRole::Launcher)
        .with_highlight(active_launcher),
    );

    for (index, notice) in model.notices.iter().take(MAX_NOTICES).enumerate() {
        let mut title = String::from(notice.level.label());
        title.make_ascii_uppercase();
        windows.push(
            DesktopWindow::new(
                ViewFrame::new(
                    ids.notices[index],
                    ViewKind::Panel,
                    0,
                    ViewContent::text_buffer(alloc::vec![notice.text.clone()]),
                    0,
                )
                .with_title(title),
                rects.notices[index],
            )
            .with_role(DesktopWindowRole::Notification)
            .with_z_index(MAX_NOTICES - index),
        );
    }

    if let Some(palette) = &model.palette {
        let mut frame = palette.clone();
        frame.view_id = ids.palette;
        frame.title = Some(model.palette_title.clone());
        windows.push(
            DesktopWindow::new(frame, rects.palette)
                .with_role(DesktopWindowRole::Palette)
                .with_highlight(model.palette_selection)
                .focused(),
        );
    }

    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_routing::DesktopInputRouter;
    use crate::{Compositor, HitRegion, RASTER_CELL_HEIGHT, RASTER_CELL_WIDTH};
    use alloc::string::ToString;
    use alloc::vec;
    use input_types::{PointerButton, PointerButtons, PointerEvent, PointerPosition};

    fn model() -> ShellModel {
        ShellModel {
            status_left: "[Workspace]".to_string(),
            status_right: "t=42".to_string(),
            launcher: vec![
                LauncherItem {
                    label: "Open Editor".to_string(),
                    command: "open_editor".to_string(),
                    active: false,
                },
                LauncherItem {
                    label: "Show Help".to_string(),
                    command: "help".to_string(),
                    active: true,
                },
            ],
            notices: vec![ShellNotice {
                level: NoticeLevel::Success,
                text: "Switched to graphics".to_string(),
            }],
            workspace: Some(ViewFrame::new(
                ViewId::new(),
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(vec!["WS > ".to_string()]),
                0,
            )),
            workspace_title: "Workspace".to_string(),
            palette: None,
            palette_title: "Commands".to_string(),
            palette_selection: None,
        }
    }

    #[test]
    fn test_layout_partitions_surface_without_overlap() {
        let rects = shell_layout(160, 44);
        assert_eq!(rects.status, SurfaceRect::new(0, 0, 160, STATUS_BAR_ROWS));
        assert_eq!(rects.launcher, SurfaceRect::new(0, 2, LAUNCHER_COLS, 42));
        assert_eq!(
            rects.workspace,
            SurfaceRect::new(LAUNCHER_COLS, 2, 160 - LAUNCHER_COLS, 42)
        );
        // Notices hug the workspace's top-right, one cell in from the edge.
        assert_eq!(
            rects.notices[0],
            SurfaceRect::new(160 - 1 - NOTICE_COLS, 2, NOTICE_COLS, 3)
        );
        assert_eq!(rects.notices[1].y, 5);
        assert_eq!(rects.palette, SurfaceRect::new(40, 11, 80, 22));

        // Tiny surface: nothing panics and rects stay inside.
        let tiny = shell_layout(10, 3);
        assert!(tiny.workspace.x + tiny.workspace.width <= 10);
        assert_eq!(tiny.launcher.width, 10);
    }

    #[test]
    fn test_compose_produces_shell_windows_with_roles_and_stable_ids() {
        let rects = shell_layout(160, 44);
        let ids = ShellViewIds::new();
        let windows = compose_shell(&model(), &rects, &ids);
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0].role, DesktopWindowRole::Main);
        assert!(windows[0].focused);
        assert_eq!(windows[0].frame.view_id, ids.workspace);
        assert_eq!(windows[0].frame.title.as_deref(), Some("Workspace"));
        assert_eq!(windows[1].role, DesktopWindowRole::Status);
        assert!(!windows[1].chrome);
        assert_eq!(windows[2].role, DesktopWindowRole::Launcher);
        assert_eq!(windows[2].frame.view_id, ids.launcher);
        assert_eq!(windows[3].role, DesktopWindowRole::Notification);
        assert_eq!(windows[3].frame.title.as_deref(), Some("OK"));

        match &windows[2].frame.content {
            ViewContent::TextBuffer { lines } => {
                assert_eq!(
                    lines,
                    &vec!["  Open Editor".to_string(), "> Show Help".to_string()]
                );
            }
            other => panic!("{other:?}"),
        }
        match &windows[1].frame.content {
            ViewContent::StatusLine { text } => {
                assert!(text.starts_with("[Workspace]"));
                assert!(text.ends_with("t=42"));
                assert_eq!(text.chars().count(), 158);
            }
            other => panic!("{other:?}"),
        }

        // Same ids on the next frame; palette appears last with its id.
        let mut with_palette = model();
        with_palette.palette = Some(ViewFrame::new(
            ViewId::new(),
            ViewKind::Panel,
            0,
            ViewContent::text_buffer(vec!["Search: ".to_string()]),
            0,
        ));
        with_palette.palette_selection = Some(2);
        let again = compose_shell(&with_palette, &rects, &ids);
        assert_eq!(again.last().unwrap().highlight_line, Some(2));
        assert_eq!(
            windows[2].highlight_line,
            Some(1),
            "active launcher entry highlighted"
        );
        assert_eq!(again[0].frame.view_id, windows[0].frame.view_id);
        assert!(!again[0].focused, "palette takes focus");
        assert_eq!(again.last().unwrap().frame.view_id, ids.palette);
        assert_eq!(again.last().unwrap().role, DesktopWindowRole::Palette);
    }

    #[test]
    fn test_status_line_alignment_and_truncation() {
        assert_eq!(status_line("ab", "cd", 8), "ab    cd");
        assert_eq!(status_line("abcdefgh", "xy", 8), "abcde xy");
        assert_eq!(status_line("abc", "toolongright", 8), "");
        assert_eq!(status_line("", "r", 3), "  r");
    }

    #[test]
    fn test_clicking_a_launcher_line_is_a_content_hit_on_the_launcher() {
        let rects = shell_layout(160, 44);
        let ids = ShellViewIds::new();
        let windows = compose_shell(&model(), &rects, &ids);
        let compositor = Compositor::new();
        // Second launcher line: launcher content starts one cell below its
        // top (chrome row), so line 1 is at cell row 2 + 1 + 1.
        let x = rects.launcher.x * RASTER_CELL_WIDTH + 3;
        let y = (rects.launcher.y + 2) * RASTER_CELL_HEIGHT + 3;
        let hit = compositor.hit_test(&windows, x, y).unwrap();
        assert_eq!(hit.view_id, ids.launcher);
        assert_eq!(hit.role, DesktopWindowRole::Launcher);
        assert_eq!(hit.region, HitRegion::Content { line: 1, column: 0 });

        // Routing a click there captures but does not move keyboard focus.
        let mut router = DesktopInputRouter::new();
        router.set_keyboard_focus(Some(ids.workspace));
        router.route(
            &compositor,
            &windows,
            PointerEvent::button_pressed(
                PointerPosition::new(x as i32, y as i32),
                PointerButton::Primary,
                PointerButtons::NONE,
            ),
        );
        assert_eq!(router.keyboard_focus(), Some(ids.workspace));
        assert_eq!(router.capture().map(|c| c.target), Some(ids.launcher));

        // The chrome-less status bar reports content on its single line.
        let sx = 5 * RASTER_CELL_WIDTH;
        let sy = 1 + 4;
        let hit = compositor.hit_test(&windows, sx, sy).unwrap();
        assert_eq!(hit.role, DesktopWindowRole::Status);
        assert_eq!(hit.region, HitRegion::Content { line: 0, column: 4 });
    }
}
