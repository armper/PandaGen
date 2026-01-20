//! Visual styling for terminal console
//!
//! This module provides visual enhancements for the terminal experience:
//! - Prompt styling (bold, colors)
//! - Error output distinction
//! - Banner/help screens
//! - Clean redraw rules

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// Console style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Normal text
    Normal,
    /// Bold text (for prompts, headers)
    Bold,
    /// Error text (visually distinct)
    Error,
    /// Success text (confirmation messages)
    Success,
    /// Help/info text
    Info,
}

/// Styled text segment
#[derive(Debug, Clone)]
pub struct StyledText {
    pub text: String,
    pub style: Style,
}

impl StyledText {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn normal(text: impl Into<String>) -> Self {
        Self::new(text, Style::Normal)
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self::new(text, Style::Bold)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::new(text, Style::Error)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text, Style::Success)
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::new(text, Style::Info)
    }

    /// Render to plain text (stripping style information)
    pub fn to_plain(&self) -> &str {
        &self.text
    }

    /// Render with visual markers for style
    pub fn to_marked(&self) -> String {
        match self.style {
            Style::Normal => self.text.clone(),
            Style::Bold => alloc::format!("**{}**", self.text),
            Style::Error => alloc::format!("[ERROR] {}", self.text),
            Style::Success => alloc::format!("[OK] {}", self.text),
            Style::Info => alloc::format!("[INFO] {}", self.text),
        }
    }
}

/// Terminal banner
pub struct Banner {
    lines: Vec<String>,
}

impl Banner {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    pub fn add_line(&mut self, line: impl Into<String>) -> &mut Self {
        self.lines.push(line.into());
        self
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Create a default PandaGen banner
    pub fn default_pandagen() -> Self {
        let mut banner = Self::new();
        banner
            .add_line("╔═══════════════════════════════════════╗")
            .add_line("║        PandaGen Operating System       ║")
            .add_line("║                                       ║")
            .add_line("║  Type 'help' for available commands  ║")
            .add_line("╚═══════════════════════════════════════╝");
        banner
    }

    /// Create a help screen
    pub fn help_screen() -> Self {
        let mut banner = Self::new();
        banner
            .add_line("Available Commands:")
            .add_line("  ls              - List files")
            .add_line("  cat <file>      - Display file contents")
            .add_line("  mkdir <dir>     - Create directory")
            .add_line("  write <file>    - Write to file")
            .add_line("  rm <name>       - Remove file/directory")
            .add_line("  stat <name>     - Show file/directory info")
            .add_line("  help            - Show this help")
            .add_line("")
            .add_line("Keyboard Shortcuts:")
            .add_line("  Ctrl+A / Home   - Jump to start of line")
            .add_line("  Ctrl+E / End    - Jump to end of line")
            .add_line("  Ctrl+U          - Delete to start of line")
            .add_line("  Ctrl+K          - Delete to end of line")
            .add_line("  Ctrl+W          - Delete word before cursor")
            .add_line("  Ctrl+C          - Cancel current input")
            .add_line("  Up/Down         - Navigate command history")
            .add_line("  PageUp/PageDown - Scroll output");
        banner
    }
}

impl Default for Banner {
    fn default() -> Self {
        Self::new()
    }
}

/// Redraw manager to prevent flicker
pub struct RedrawManager {
    last_frame: Vec<String>,
    dirty: bool,
}

impl RedrawManager {
    pub fn new() -> Self {
        Self {
            last_frame: Vec::new(),
            dirty: true,
        }
    }

    /// Check if content has changed since last frame
    pub fn has_changed(&mut self, new_frame: &[String]) -> bool {
        if self.dirty {
            return true;
        }

        if self.last_frame.len() != new_frame.len() {
            return true;
        }

        for (old, new) in self.last_frame.iter().zip(new_frame.iter()) {
            if old != new {
                return true;
            }
        }

        false
    }

    /// Update last frame and mark clean
    pub fn update(&mut self, frame: Vec<String>) {
        self.last_frame = frame;
        self.dirty = false;
    }

    /// Mark as dirty (force redraw)
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

impl Default for RedrawManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_styled_text_creation() {
        let text = StyledText::normal("Hello");
        assert_eq!(text.to_plain(), "Hello");
        assert_eq!(text.style, Style::Normal);
    }

    #[test]
    fn test_styled_text_bold() {
        let text = StyledText::bold("Prompt");
        assert_eq!(text.to_plain(), "Prompt");
        assert_eq!(text.to_marked(), "**Prompt**");
    }

    #[test]
    fn test_styled_text_error() {
        let text = StyledText::error("Failed");
        assert_eq!(text.to_plain(), "Failed");
        assert!(text.to_marked().contains("ERROR"));
    }

    #[test]
    fn test_styled_text_success() {
        let text = StyledText::success("Done");
        assert!(text.to_marked().contains("OK"));
    }

    #[test]
    fn test_banner_creation() {
        let mut banner = Banner::new();
        banner.add_line("Line 1");
        banner.add_line("Line 2");

        assert_eq!(banner.lines().len(), 2);
        assert_eq!(banner.lines()[0], "Line 1");
    }

    #[test]
    fn test_default_pandagen_banner() {
        let banner = Banner::default_pandagen();
        assert!(banner.lines().len() > 0);
        assert!(banner.lines()[0].contains("═"));
    }

    #[test]
    fn test_help_screen() {
        let banner = Banner::help_screen();
        assert!(banner.lines().len() > 5);
        let text = banner.lines().join("\n");
        assert!(text.contains("ls"));
        assert!(text.contains("Ctrl"));
    }

    #[test]
    fn test_redraw_manager_initial_dirty() {
        let mut manager = RedrawManager::new();
        let frame = vec!["line1".to_string()];
        assert!(manager.has_changed(&frame));
    }

    #[test]
    fn test_redraw_manager_no_change() {
        let mut manager = RedrawManager::new();
        let frame = vec!["line1".to_string(), "line2".to_string()];

        manager.update(frame.clone());
        assert!(!manager.has_changed(&frame));
    }

    #[test]
    fn test_redraw_manager_detects_change() {
        let mut manager = RedrawManager::new();
        let frame1 = vec!["line1".to_string()];
        let frame2 = vec!["line2".to_string()];

        manager.update(frame1);
        assert!(manager.has_changed(&frame2));
    }

    #[test]
    fn test_redraw_manager_mark_dirty() {
        let mut manager = RedrawManager::new();
        let frame = vec!["line1".to_string()];

        manager.update(frame.clone());
        assert!(!manager.has_changed(&frame));

        manager.mark_dirty();
        assert!(manager.has_changed(&frame));
    }
}
