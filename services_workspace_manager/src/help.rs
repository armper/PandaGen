//! # Help System
//!
//! Tiered help system for workspace commands

use crate::command_surface::{
    component_id_usage_pattern, help_usage_pattern, non_launch_prompt_suggestion_by_id,
    parse_help_topic, COMPONENT_ID_COMMAND_SPECS, HELPER_COMMAND_SPECS, HELP_TOPIC_SPECS,
    LAUNCH_COMMAND_SPECS, NON_LAUNCH_PALETTE_SPECS,
};
use core::fmt;
use serde::{Deserialize, Serialize};

/// Help category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        let token = s.trim();
        if token.is_empty() {
            parse_help_topic(None)
        } else {
            parse_help_topic(Some(token))
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
        let mut content = String::from("PandaGen OS Workspace\n\nAvailable help topics:\n");
        for topic in HELP_TOPIC_SPECS {
            if let Some(primary_alias) = topic.aliases.first() {
                content.push_str(&format!(
                    "- {}  — {}\n",
                    format!("help {}", primary_alias),
                    topic_description(topic.topic)
                ));
            }
        }
        content.push_str(&format!(
            "- {}  — Show help by topic\n",
            help_usage_pattern()
        ));
        content.push_str("\nTip: Press Ctrl+P to find commands faster");
        content
    }

    fn workspace_help(&self) -> String {
        let mut lines = Vec::new();

        // Launch commands are sourced from shared launch grammar/metadata.
        for spec in LAUNCH_COMMAND_SPECS {
            let pattern = if let Some(usage) = spec.required_usage {
                usage.strip_prefix("Usage: ").unwrap_or(usage).to_string()
            } else if spec.token == "editor" {
                "open editor <path>".to_string()
            } else {
                format!("open {}", spec.token)
            };
            lines.push(format_help_line(&pattern, spec.palette.description));
        }

        // Helper command aliases are sourced from shared helper grammar.
        for helper in HELPER_COMMAND_SPECS {
            let aliases = helper
                .aliases
                .iter()
                .map(|parts| parts.join(" "))
                .collect::<Vec<String>>()
                .join(" | ");
            lines.push(format_help_line(&aliases, helper.palette.description));
        }

        // Workspace command descriptors are sourced from shared palette specs.
        for spec in NON_LAUNCH_PALETTE_SPECS.iter().filter(|spec| {
            spec.category == "Workspace" && matches!(spec.id, "list" | "focus_next" | "focus_prev")
        }) {
            if let Some(suggestion) = non_launch_prompt_suggestion_by_id(spec.id) {
                lines.push(format_help_line(&suggestion.pattern, spec.description));
            }
        }

        // Component-id command grammar is sourced from shared command rules.
        for spec in COMPONENT_ID_COMMAND_SPECS {
            let pattern = component_id_usage_pattern(spec.token).unwrap_or(spec.usage);
            let description = NON_LAUNCH_PALETTE_SPECS
                .iter()
                .find(|entry| entry.id == spec.token)
                .map(|entry| entry.description)
                .unwrap_or("Target a component");
            lines.push(format_help_line(pattern, description));
        }

        lines.push(format_help_line(
            &help_usage_pattern(),
            "Show help by topic",
        ));

        let mut content = String::from("Workspace Commands\n\n");
        for line in lines {
            content.push_str(&line);
            content.push('\n');
        }
        content.push_str("\nTip: Press Ctrl+P to find commands faster");
        content
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
        let mut lines = Vec::new();
        for spec in NON_LAUNCH_PALETTE_SPECS
            .iter()
            .filter(|spec| spec.category == "System" && !matches!(spec.id, "help_system"))
        {
            let pattern = spec
                .prompt_pattern
                .map(|pattern| pattern.trim_end())
                .unwrap_or(spec.id);
            lines.push(format_help_line(pattern, spec.description));
        }
        lines.push(format_help_line(
            &help_usage_pattern(),
            "Show help by topic",
        ));

        let mut content = String::from("System Commands\n\n");
        for line in lines {
            content.push_str(&line);
            content.push('\n');
        }
        content.push_str("\nTip: Press Ctrl+P to find commands faster");
        content
    }
}

fn format_help_line(pattern: &str, description: &str) -> String {
    format!("{:<34} — {}", pattern, description)
}

fn topic_description(topic: HelpCategory) -> &'static str {
    match topic {
        HelpCategory::Overview => "Workspace overview",
        HelpCategory::Workspace => "Workspace management commands",
        HelpCategory::Editor => "Editor commands and operations",
        HelpCategory::Keys => "Keyboard shortcuts reference",
        HelpCategory::System => "System control commands",
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
        assert!(content.contains("help workspace"));
        assert!(content.contains("help [workspace|editor|keys|system]"));
        assert!(content.contains("Tip: Press Ctrl+P"));
    }

    #[test]
    fn test_help_workspace_has_commands() {
        let content = HelpCategory::Workspace.content();
        assert!(content.contains("open editor"));
        assert!(content.contains("list"));
        assert!(content.contains("open custom <entry>"));
        assert!(content.contains("recent | recent files | open recent"));
        assert!(content.contains("help [workspace|editor|keys|system]"));
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
        assert!(content.contains("boot profile show"));
        assert!(content.contains("boot profile set"));
        assert!(content.contains("boot profile save"));
        assert!(!content.contains("halt"));
        assert!(!content.contains("reboot"));
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
