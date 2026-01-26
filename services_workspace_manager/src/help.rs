//! # Help System
//!
//! Tiered help system for workspace commands

use core::fmt;

/// Help category
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpCategory {
    /// Overview help
    Overview,
    /// Workspace commands
    Workspace,
    /// Editor commands
    Editor,
    /// Keyboard shortcuts
    Keys,
    /// System commands
    System,
}

impl HelpCategory {
    /// Parse help category from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "overview" | "" => Some(HelpCategory::Overview),
            "workspace" => Some(HelpCategory::Workspace),
            "editor" => Some(HelpCategory::Editor),
            "keys" | "keyboard" | "shortcuts" => Some(HelpCategory::Keys),
            "system" => Some(HelpCategory::System),
            _ => None,
        }
    }

    /// Get the help content for this category
    pub fn content(&self) -> String {
        match self {
            HelpCategory::Overview => self.overview_help(),
            HelpCategory::Workspace => self.workspace_help(),
            HelpCategory::Editor => self.editor_help(),
            HelpCategory::Keys => self.keys_help(),
            HelpCategory::System => self.system_help(),
        }
    }

    fn overview_help(&self) -> String {
        "PandaGen OS Workspace\n\
             \n\
             Available help topics:\n\
             - help workspace  — Workspace management commands\n\
             - help editor     — Editor commands and operations\n\
             - help keys       — Keyboard shortcuts reference\n\
             - help system     — System control commands\n\
             \n\
             Tip: Press Ctrl+P to find commands faster"
            .to_string()
    }

    fn workspace_help(&self) -> String {
        "Workspace Commands\n\
             \n\
             open editor <path>   — Open file in editor\n\
             list                 — List all components\n\
             next / prev          — Switch focus between components\n\
             close <id>           — Close a component\n\
             recent               — Show recent files\n\
             \n\
             Tip: Press Ctrl+P to find commands faster"
            .to_string()
    }

    fn editor_help(&self) -> String {
        "Editor Commands\n\
             \n\
             The editor is vi-like with familiar keybindings:\n\
             - i        — Enter insert mode\n\
             - ESC      — Return to normal mode\n\
             - :w       — Save file\n\
             - :q       — Quit editor\n\
             - h,j,k,l  — Navigate left, down, up, right\n\
             \n\
             Tip: Press Ctrl+P to find commands faster"
            .to_string()
    }

    fn keys_help(&self) -> String {
        "Keyboard Shortcuts\n\
             \n\
             Global shortcuts:\n\
             - Ctrl+P       — Open command palette\n\
             - Alt+Tab      — Switch between components\n\
             - Ctrl+S       — Save current file\n\
             - Ctrl+Q       — Quit workspace\n\
             \n\
             Focus navigation:\n\
             - Ctrl+1       — Focus top component\n\
             - Ctrl+2       — Focus bottom component\n\
             \n\
             Tip: Press Ctrl+P to find commands faster"
            .to_string()
    }

    fn system_help(&self) -> String {
        "System Commands\n\
             \n\
             System control:\n\
             - halt         — Shut down system\n\
             - reboot       — Restart system\n\
             - mem          — Show memory usage\n\
             - ticks        — Show scheduler ticks\n\
             \n\
             Tip: Press Ctrl+P to find commands faster"
            .to_string()
    }
}

impl fmt::Display for HelpCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HelpCategory::Overview => write!(f, "overview"),
            HelpCategory::Workspace => write!(f, "workspace"),
            HelpCategory::Editor => write!(f, "editor"),
            HelpCategory::Keys => write!(f, "keys"),
            HelpCategory::System => write!(f, "system"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_category_from_str() {
        assert_eq!(HelpCategory::parse(""), Some(HelpCategory::Overview));
        assert_eq!(
            HelpCategory::parse("overview"),
            Some(HelpCategory::Overview)
        );
        assert_eq!(
            HelpCategory::parse("workspace"),
            Some(HelpCategory::Workspace)
        );
        assert_eq!(HelpCategory::parse("editor"), Some(HelpCategory::Editor));
        assert_eq!(HelpCategory::parse("keys"), Some(HelpCategory::Keys));
        assert_eq!(HelpCategory::parse("keyboard"), Some(HelpCategory::Keys));
        assert_eq!(HelpCategory::parse("system"), Some(HelpCategory::System));
        assert_eq!(HelpCategory::parse("invalid"), None);
    }

    #[test]
    fn test_help_overview_has_tip() {
        let content = HelpCategory::Overview.content();
        assert!(content.contains("Tip: Press Ctrl+P"));
    }

    #[test]
    fn test_help_workspace_has_commands() {
        let content = HelpCategory::Workspace.content();
        assert!(content.contains("open editor"));
        assert!(content.contains("list"));
        assert!(content.contains("Tip: Press Ctrl+P"));
    }

    #[test]
    fn test_help_editor_has_vi_keys() {
        let content = HelpCategory::Editor.content();
        assert!(content.contains("vi-like"));
        assert!(content.contains("ESC"));
        assert!(content.contains("Tip: Press Ctrl+P"));
    }

    #[test]
    fn test_help_keys_has_shortcuts() {
        let content = HelpCategory::Keys.content();
        assert!(content.contains("Ctrl+P"));
        assert!(content.contains("Alt+Tab"));
        assert!(content.contains("Tip: Press Ctrl+P"));
    }

    #[test]
    fn test_help_system_has_commands() {
        let content = HelpCategory::System.content();
        assert!(content.contains("halt"));
        assert!(content.contains("reboot"));
        assert!(content.contains("Tip: Press Ctrl+P"));
    }

    #[test]
    fn test_help_category_display() {
        assert_eq!(HelpCategory::Overview.to_string(), "overview");
        assert_eq!(HelpCategory::Workspace.to_string(), "workspace");
        assert_eq!(HelpCategory::Editor.to_string(), "editor");
        assert_eq!(HelpCategory::Keys.to_string(), "keys");
        assert_eq!(HelpCategory::System.to_string(), "system");
    }
}
