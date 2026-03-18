//! GUI host and compositor on view surfaces.

use serde::{Deserialize, Serialize};
use view_types::{ViewContent, ViewFrame, ViewKind};

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopWindow {
    pub frame: ViewFrame,
    pub rect: SurfaceRect,
    pub z_index: usize,
    pub focused: bool,
}

impl DesktopWindow {
    pub fn new(frame: ViewFrame, rect: SurfaceRect) -> Self {
        Self {
            frame,
            rect,
            z_index: 0,
            focused: false,
        }
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
    frame.title
        .clone()
        .unwrap_or_else(|| match frame.kind {
            ViewKind::TextBuffer => "TextBuffer".to_string(),
            ViewKind::StatusLine => "StatusLine".to_string(),
            ViewKind::Panel => "Panel".to_string(),
        })
}

fn draw_window(canvas: &mut [Vec<char>], window: &DesktopWindow) {
    if window.rect.width == 0 || window.rect.height == 0 || canvas.is_empty() || canvas[0].is_empty()
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
        let label = format!(" {} ", window_title(&window.frame));
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
}
