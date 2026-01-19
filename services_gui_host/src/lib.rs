//! GUI host and compositor on view surfaces.

use serde::{Deserialize, Serialize};
use view_types::{ViewContent, ViewFrame};

/// Composited surface frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceFrame {
    pub content: String,
    pub frame_count: usize,
    pub timestamp_ns: u64,
}

/// Simple compositor that merges view frames into a surface.
pub struct Compositor;

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
            output.push_str("\n");
        }

        SurfaceFrame {
            content: output,
            frame_count: frames.len(),
            timestamp_ns: frames
                .iter()
                .map(|frame| frame.timestamp_ns)
                .max()
                .unwrap_or(0),
        }
    }
}

fn render_content(content: &ViewContent) -> String {
    match content {
        ViewContent::TextBuffer { lines } => lines.join("\n"),
        ViewContent::StatusLine { text } => text.clone(),
        ViewContent::Panel { metadata } => format!("panel: {}", metadata),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use view_types::{ViewId, ViewKind};

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
        assert_eq!(surface.frame_count, 2);
        assert!(surface.content.contains("Editor"));
        assert!(surface.content.contains("ready"));
        assert_eq!(surface.timestamp_ns, 12);
    }
}
