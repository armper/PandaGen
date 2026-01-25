//! # Workspace Status and State Tracking
//!
//! This module provides deterministic state tracking for the workspace UI:
//! - Status strip content
//! - Recent history (files, commands, errors)
//! - Command suggestions for guided prompts
//! - Validation indicators

use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use alloc::collections::VecDeque;
#[cfg(feature = "std")]
use std::collections::VecDeque;

/// Maximum number of items to keep in history
const MAX_HISTORY_SIZE: usize = 20;

/// Workspace status information for the status strip
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStatus {
    /// Active editor filename (if any)
    pub active_editor: Option<String>,
    /// Whether the active editor has unsaved changes
    pub has_unsaved_changes: bool,
    /// Filesystem availability status
    pub fs_status: FsStatus,
    /// Number of active jobs
    pub active_jobs: usize,
    /// Last action result (ephemeral toast)
    pub last_action: Option<String>,
}

impl WorkspaceStatus {
    /// Creates a new workspace status
    pub fn new() -> Self {
        Self {
            active_editor: None,
            has_unsaved_changes: false,
            fs_status: FsStatus::Ok,
            active_jobs: 0,
            last_action: None,
        }
    }

    /// Formats the status strip as a single line
    /// Examples:
    /// - "Workspace — Editor: hi.txt | Unsaved | FS: OK | Jobs: 2"
    /// - "Workspace — No editors | Idle"
    /// - "Workspace — Editor: main.rs | Saved | Jobs: 0"
    pub fn format_status_strip(&self) -> String {
        let mut parts = vec!["Workspace".to_string()];

        // Editor status
        if let Some(ref filename) = self.active_editor {
            let save_status = if self.has_unsaved_changes {
                "Unsaved"
            } else {
                "Saved"
            };
            parts.push(format!("Editor: {} | {}", filename, save_status));
        } else {
            parts.push("No editors".to_string());
        }

        // Job status
        if self.active_jobs > 0 {
            parts.push(format!("Jobs: {}", self.active_jobs));
        } else {
            parts.push("Idle".to_string());
        }

        // FS status (only show if not OK)
        match self.fs_status {
            FsStatus::Ok => {}
            FsStatus::ReadOnly => parts.push("FS: Read-only".to_string()),
            FsStatus::Unavailable => parts.push("FS: Unavailable".to_string()),
        }

        parts.join(" — ")
    }

    /// Formats the status strip with optional last action on the right
    pub fn format_status_strip_with_action(&self) -> String {
        let base = self.format_status_strip();
        if let Some(ref action) = self.last_action {
            format!("{}    [ {} ]", base, action)
        } else {
            base
        }
    }

    /// Clears the ephemeral last action
    pub fn clear_last_action(&mut self) {
        self.last_action = None;
    }

    /// Sets the last action result
    pub fn set_last_action(&mut self, action: impl Into<String>) {
        self.last_action = Some(action.into());
    }
}

impl Default for WorkspaceStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Filesystem status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsStatus {
    /// Filesystem is available and writable
    Ok,
    /// Filesystem is read-only
    ReadOnly,
    /// Filesystem is unavailable
    Unavailable,
}

/// Recent history tracker with bounded FIFO queues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentHistory {
    /// Recently opened files
    recent_files: VecDeque<String>,
    /// Recently executed commands
    recent_commands: VecDeque<String>,
    /// Recent errors
    recent_errors: VecDeque<String>,
}

impl RecentHistory {
    /// Creates a new empty history
    pub fn new() -> Self {
        Self {
            recent_files: VecDeque::new(),
            recent_commands: VecDeque::new(),
            recent_errors: VecDeque::new(),
        }
    }

    /// Adds a file to recent files (FIFO, max size bounded)
    pub fn add_file(&mut self, file: String) {
        // Remove if already exists (move to front)
        self.recent_files.retain(|f| f != &file);
        
        // Add to front
        self.recent_files.push_front(file);
        
        // Trim to max size
        while self.recent_files.len() > MAX_HISTORY_SIZE {
            self.recent_files.pop_back();
        }
    }

    /// Adds a command to recent commands
    pub fn add_command(&mut self, command: String) {
        // Remove if already exists
        self.recent_commands.retain(|c| c != &command);
        
        // Add to front
        self.recent_commands.push_front(command);
        
        // Trim to max size
        while self.recent_commands.len() > MAX_HISTORY_SIZE {
            self.recent_commands.pop_back();
        }
    }

    /// Adds an error to recent errors
    pub fn add_error(&mut self, error: String) {
        // Don't remove duplicates for errors
        self.recent_errors.push_front(error);
        
        // Trim to max size (keep last N errors)
        while self.recent_errors.len() > MAX_HISTORY_SIZE {
            self.recent_errors.pop_back();
        }
    }

    /// Gets recent files (most recent first)
    pub fn get_recent_files(&self) -> Vec<String> {
        self.recent_files.iter().cloned().collect()
    }

    /// Gets recent commands (most recent first)
    pub fn get_recent_commands(&self) -> Vec<String> {
        self.recent_commands.iter().cloned().collect()
    }

    /// Gets recent errors (most recent first)
    pub fn get_recent_errors(&self) -> Vec<String> {
        self.recent_errors.iter().cloned().collect()
    }

    /// Clears all history
    pub fn clear(&mut self) {
        self.recent_files.clear();
        self.recent_commands.clear();
        self.recent_errors.clear();
    }
}

impl Default for RecentHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Command suggestion for guided prompts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSuggestion {
    /// Command pattern (e.g., "open editor <path>")
    pub pattern: String,
    /// Description of what the command does
    pub description: String,
}

impl CommandSuggestion {
    /// Creates a new command suggestion
    pub fn new(pattern: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            description: description.into(),
        }
    }

    /// Formats the suggestion for display
    /// Example: "  open editor <path>   — Open file in editor"
    pub fn format(&self) -> String {
        format!("  {}   — {}", self.pattern, self.description)
    }
}

/// Generate command suggestions based on partial input
/// Returns deterministically ordered suggestions
pub fn generate_suggestions(input: &str) -> Vec<CommandSuggestion> {
    let input_lower = input.trim().to_lowercase();
    
    // Empty input - show common commands
    if input_lower.is_empty() {
        return vec![
            CommandSuggestion::new("open editor <path>", "Open file in editor"),
            CommandSuggestion::new("list", "List all components"),
            CommandSuggestion::new("help", "Show help overview"),
            CommandSuggestion::new("recent", "Show recent files"),
        ];
    }
    
    // Match command prefixes
    let mut suggestions = Vec::new();
    
    // "open" commands
    if "open".starts_with(&input_lower) || input_lower.starts_with("op") {
        suggestions.push(CommandSuggestion::new("open editor <path>", "Open file in editor"));
        suggestions.push(CommandSuggestion::new("open recent", "Show recent files"));
    }
    
    // "help" commands
    if "help".starts_with(&input_lower) || input_lower.starts_with("he") || input_lower == "?" {
        suggestions.push(CommandSuggestion::new("help", "Overview"));
        suggestions.push(CommandSuggestion::new("help editor", "Editor commands"));
        suggestions.push(CommandSuggestion::new("help keys", "Keyboard shortcuts"));
        suggestions.push(CommandSuggestion::new("help workspace", "Workspace commands"));
        suggestions.push(CommandSuggestion::new("help system", "System commands"));
    }
    
    // "list" commands
    if "list".starts_with(&input_lower) || input_lower.starts_with("li") {
        suggestions.push(CommandSuggestion::new("list", "List all components"));
    }
    
    // "recent" commands
    if "recent".starts_with(&input_lower) || input_lower.starts_with("rec") {
        suggestions.push(CommandSuggestion::new("recent", "Show recent files"));
    }
    
    // "close" commands
    if "close".starts_with(&input_lower) || input_lower.starts_with("cl") {
        suggestions.push(CommandSuggestion::new("close <id>", "Close a component"));
    }
    
    // "next" / "prev" navigation
    if "next".starts_with(&input_lower) || input_lower.starts_with("ne") {
        suggestions.push(CommandSuggestion::new("next", "Focus next component"));
    }
    if "prev".starts_with(&input_lower) || "previous".starts_with(&input_lower) || input_lower.starts_with("pr") {
        suggestions.push(CommandSuggestion::new("prev", "Focus previous component"));
    }
    
    suggestions
}

/// Prompt validation state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptValidation {
    /// Valid prefix but incomplete command
    ValidPrefix,
    /// Valid complete command
    ValidComplete,
    /// Invalid command
    Invalid,
}

impl PromptValidation {
    /// Returns the prompt indicator for this validation state
    /// - ValidPrefix → normal prompt (>)
    /// - ValidComplete → success indicator ($)
    /// - Invalid → error indicator (?)
    pub fn prompt_indicator(&self) -> &'static str {
        match self {
            PromptValidation::ValidPrefix => ">",
            PromptValidation::ValidComplete => "$",
            PromptValidation::Invalid => "?",
        }
    }
}

/// Actionable error with suggested actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionableError {
    /// Error message
    pub message: String,
    /// Suggested actions (e.g., "Retry", "Help")
    pub actions: Vec<String>,
}

impl ActionableError {
    /// Creates a new actionable error
    pub fn new(message: impl Into<String>, actions: Vec<String>) -> Self {
        Self {
            message: message.into(),
            actions,
        }
    }

    /// Creates an error with "Retry" and "Help" actions
    pub fn with_retry_help(message: impl Into<String>) -> Self {
        Self::new(message, vec!["Retry".to_string(), "Help".to_string()])
    }

    /// Creates an error with only "Help" action
    pub fn with_help(message: impl Into<String>) -> Self {
        Self::new(message, vec!["Help".to_string()])
    }

    /// Formats the error with actions
    /// Example: "Filesystem unavailable — Retry | Help"
    pub fn format(&self) -> String {
        format!("{} — {}", self.message, self.actions.join(" | "))
    }
}

/// Context breadcrumbs for status strip
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBreadcrumbs {
    /// Breadcrumb parts from root to current context
    parts: Vec<String>,
}

impl ContextBreadcrumbs {
    /// Creates a new breadcrumb trail
    pub fn new() -> Self {
        Self {
            parts: vec!["PANDA".to_string(), "ROOT".to_string()],
        }
    }

    /// Adds a context level
    pub fn push(&mut self, part: String) {
        self.parts.push(part);
    }

    /// Removes the last context level
    pub fn pop(&mut self) {
        if self.parts.len() > 2 {
            self.parts.pop();
        }
    }

    /// Clears to root
    pub fn clear(&mut self) {
        self.parts.truncate(2);
    }

    /// Formats breadcrumbs with separators
    /// Example: "PANDA > ROOT > EDITOR(main.rs) > INSERT"
    pub fn format(&self) -> String {
        self.parts.join(" > ")
    }

    /// Sets breadcrumbs from parts
    pub fn set_parts(&mut self, parts: Vec<String>) {
        self.parts = parts;
    }
}

impl Default for ContextBreadcrumbs {
    fn default() -> Self {
        Self::new()
    }
}

/// Validates a command input and returns its validation state
/// This is render-time validation only - no execution side effects
pub fn validate_command(input: &str) -> PromptValidation {
    let input_trimmed = input.trim();
    
    if input_trimmed.is_empty() {
        return PromptValidation::ValidPrefix;
    }
    
    let parts: Vec<&str> = input_trimmed.split_whitespace().collect();
    if parts.is_empty() {
        return PromptValidation::ValidPrefix;
    }
    
    let cmd = parts[0];
    
    // Check valid commands
    match cmd {
        "open" => {
            // Needs at least component type
            if parts.len() == 1 {
                PromptValidation::ValidPrefix
            } else if parts.len() == 2 {
                // Has component type but no args - check if it's valid
                match parts[1] {
                    "editor" => PromptValidation::ValidPrefix, // Editor needs filename
                    "cli" | "pipeline" => PromptValidation::ValidComplete,
                    _ => PromptValidation::Invalid,
                }
            } else {
                // Has component type and args
                match parts[1] {
                    "editor" | "cli" | "pipeline" => PromptValidation::ValidComplete,
                    _ => PromptValidation::Invalid,
                }
            }
        }
        "list" | "next" | "prev" | "previous" => {
            if parts.len() == 1 {
                PromptValidation::ValidComplete
            } else {
                PromptValidation::Invalid
            }
        }
        "close" | "focus" | "status" => {
            // These need a component ID
            if parts.len() == 1 {
                PromptValidation::ValidPrefix
            } else if parts.len() == 2 {
                // Check if it looks like a component ID
                if parts[1].starts_with("comp:") {
                    PromptValidation::ValidComplete
                } else {
                    PromptValidation::Invalid
                }
            } else {
                PromptValidation::Invalid
            }
        }
        "help" => {
            if parts.len() == 1 {
                PromptValidation::ValidComplete
            } else if parts.len() == 2 {
                // Check if it's a valid help category
                match parts[1] {
                    "workspace" | "editor" | "keys" | "system" => PromptValidation::ValidComplete,
                    _ => PromptValidation::Invalid,
                }
            } else {
                PromptValidation::Invalid
            }
        }
        "recent" => {
            if parts.len() == 1 {
                PromptValidation::ValidComplete
            } else {
                PromptValidation::Invalid
            }
        }
        // Check if input is a valid prefix of any command
        _ => {
            // Valid command prefixes
            let valid_prefixes = [
                ("open", &["op", "ope"] as &[&str]),
                ("list", &["li", "lis"]),
                ("next", &["ne", "nex"]),
                ("prev", &["pr", "pre"]),  // Note: also matches "previous"
                ("close", &["cl", "clo", "clos"]),
                ("focus", &["fo", "foc", "focu"]),
                ("status", &["st", "sta", "stat", "statu"]),
                ("help", &["he", "hel"]),
                ("recent", &["re", "rec", "rece", "recen"]),
            ];
            
            // Check if input matches any valid prefix
            for (_, prefixes) in &valid_prefixes {
                if prefixes.contains(&cmd) {
                    return PromptValidation::ValidPrefix;
                }
            }
            
            PromptValidation::Invalid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_status_no_editor() {
        let status = WorkspaceStatus::new();
        let strip = status.format_status_strip();
        assert!(strip.contains("No editors"));
        assert!(strip.contains("Idle"));
    }

    #[test]
    fn test_workspace_status_with_editor_saved() {
        let mut status = WorkspaceStatus::new();
        status.active_editor = Some("test.txt".to_string());
        status.has_unsaved_changes = false;
        
        let strip = status.format_status_strip();
        assert!(strip.contains("Editor: test.txt"));
        assert!(strip.contains("Saved"));
    }

    #[test]
    fn test_workspace_status_with_unsaved_changes() {
        let mut status = WorkspaceStatus::new();
        status.active_editor = Some("main.rs".to_string());
        status.has_unsaved_changes = true;
        
        let strip = status.format_status_strip();
        assert!(strip.contains("Editor: main.rs"));
        assert!(strip.contains("Unsaved"));
    }

    #[test]
    fn test_workspace_status_with_jobs() {
        let mut status = WorkspaceStatus::new();
        status.active_jobs = 2;
        
        let strip = status.format_status_strip();
        assert!(strip.contains("Jobs: 2"));
    }

    #[test]
    fn test_workspace_status_fs_readonly() {
        let mut status = WorkspaceStatus::new();
        status.fs_status = FsStatus::ReadOnly;
        
        let strip = status.format_status_strip();
        assert!(strip.contains("FS: Read-only"));
    }

    #[test]
    fn test_workspace_status_with_last_action() {
        let mut status = WorkspaceStatus::new();
        status.set_last_action("Wrote 12 lines to disk");
        
        let strip = status.format_status_strip_with_action();
        assert!(strip.contains("[ Wrote 12 lines to disk ]"));
    }

    #[test]
    fn test_workspace_status_clear_last_action() {
        let mut status = WorkspaceStatus::new();
        status.set_last_action("Test");
        status.clear_last_action();
        
        assert!(status.last_action.is_none());
    }

    #[test]
    fn test_recent_history_add_file() {
        let mut history = RecentHistory::new();
        history.add_file("file1.txt".to_string());
        history.add_file("file2.txt".to_string());
        
        let files = history.get_recent_files();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "file2.txt");
        assert_eq!(files[1], "file1.txt");
    }

    #[test]
    fn test_recent_history_deduplication() {
        let mut history = RecentHistory::new();
        history.add_file("file1.txt".to_string());
        history.add_file("file2.txt".to_string());
        history.add_file("file1.txt".to_string()); // Move to front
        
        let files = history.get_recent_files();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "file1.txt");
        assert_eq!(files[1], "file2.txt");
    }

    #[test]
    fn test_recent_history_max_size() {
        let mut history = RecentHistory::new();
        
        // Add more than MAX_HISTORY_SIZE items
        for i in 0..25 {
            history.add_file(format!("file{}.txt", i));
        }
        
        let files = history.get_recent_files();
        assert_eq!(files.len(), MAX_HISTORY_SIZE);
        assert_eq!(files[0], "file24.txt");
    }

    #[test]
    fn test_recent_history_commands() {
        let mut history = RecentHistory::new();
        history.add_command("open editor test.txt".to_string());
        history.add_command("list".to_string());
        
        let commands = history.get_recent_commands();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0], "list");
    }

    #[test]
    fn test_recent_history_errors() {
        let mut history = RecentHistory::new();
        history.add_error("Error 1".to_string());
        history.add_error("Error 2".to_string());
        
        let errors = history.get_recent_errors();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0], "Error 2");
    }

    #[test]
    fn test_recent_history_clear() {
        let mut history = RecentHistory::new();
        history.add_file("file.txt".to_string());
        history.add_command("test".to_string());
        history.add_error("error".to_string());
        
        history.clear();
        
        assert_eq!(history.get_recent_files().len(), 0);
        assert_eq!(history.get_recent_commands().len(), 0);
        assert_eq!(history.get_recent_errors().len(), 0);
    }

    #[test]
    fn test_command_suggestion_format() {
        let suggestion = CommandSuggestion::new("open editor <path>", "Open file in editor");
        let formatted = suggestion.format();
        
        assert!(formatted.contains("open editor <path>"));
        assert!(formatted.contains("Open file in editor"));
    }

    #[test]
    fn test_generate_suggestions_empty_input() {
        let suggestions = generate_suggestions("");
        assert!(suggestions.len() > 0);
        assert!(suggestions.iter().any(|s| s.pattern.contains("open")));
        assert!(suggestions.iter().any(|s| s.pattern.contains("list")));
    }

    #[test]
    fn test_generate_suggestions_open_prefix() {
        let suggestions = generate_suggestions("op");
        assert!(suggestions.iter().any(|s| s.pattern.contains("open editor")));
    }

    #[test]
    fn test_generate_suggestions_help_prefix() {
        let suggestions = generate_suggestions("he");
        assert!(suggestions.iter().any(|s| s.pattern.contains("help")));
        assert!(suggestions.iter().any(|s| s.pattern.contains("help editor")));
    }

    #[test]
    fn test_generate_suggestions_list_prefix() {
        let suggestions = generate_suggestions("li");
        assert!(suggestions.iter().any(|s| s.pattern == "list"));
    }

    #[test]
    fn test_generate_suggestions_deterministic() {
        let suggestions1 = generate_suggestions("he");
        let suggestions2 = generate_suggestions("he");
        
        assert_eq!(suggestions1.len(), suggestions2.len());
        for (s1, s2) in suggestions1.iter().zip(suggestions2.iter()) {
            assert_eq!(s1.pattern, s2.pattern);
        }
    }

    #[test]
    fn test_prompt_validation_indicators() {
        assert_eq!(PromptValidation::ValidPrefix.prompt_indicator(), ">");
        assert_eq!(PromptValidation::ValidComplete.prompt_indicator(), "$");
        assert_eq!(PromptValidation::Invalid.prompt_indicator(), "?");
    }

    #[test]
    fn test_context_breadcrumbs_default() {
        let breadcrumbs = ContextBreadcrumbs::new();
        let formatted = breadcrumbs.format();
        
        assert_eq!(formatted, "PANDA > ROOT");
    }

    #[test]
    fn test_context_breadcrumbs_push_pop() {
        let mut breadcrumbs = ContextBreadcrumbs::new();
        breadcrumbs.push("EDITOR(main.rs)".to_string());
        breadcrumbs.push("INSERT".to_string());
        
        assert_eq!(breadcrumbs.format(), "PANDA > ROOT > EDITOR(main.rs) > INSERT");
        
        breadcrumbs.pop();
        assert_eq!(breadcrumbs.format(), "PANDA > ROOT > EDITOR(main.rs)");
    }

    #[test]
    fn test_context_breadcrumbs_clear() {
        let mut breadcrumbs = ContextBreadcrumbs::new();
        breadcrumbs.push("TEST".to_string());
        breadcrumbs.clear();
        
        assert_eq!(breadcrumbs.format(), "PANDA > ROOT");
    }

    #[test]
    fn test_context_breadcrumbs_cannot_pop_below_root() {
        let mut breadcrumbs = ContextBreadcrumbs::new();
        breadcrumbs.pop();
        breadcrumbs.pop();
        breadcrumbs.pop();
        
        // Should still have at least PANDA > ROOT
        let formatted = breadcrumbs.format();
        assert!(formatted.contains("PANDA"));
        assert!(formatted.contains("ROOT"));
    }

    #[test]
    fn test_validate_command_empty() {
        assert_eq!(validate_command(""), PromptValidation::ValidPrefix);
        assert_eq!(validate_command("   "), PromptValidation::ValidPrefix);
    }

    #[test]
    fn test_validate_command_list() {
        assert_eq!(validate_command("list"), PromptValidation::ValidComplete);
        assert_eq!(validate_command("list extra"), PromptValidation::Invalid);
    }

    #[test]
    fn test_validate_command_open_incomplete() {
        assert_eq!(validate_command("open"), PromptValidation::ValidPrefix);
        assert_eq!(validate_command("open editor"), PromptValidation::ValidPrefix);
    }

    #[test]
    fn test_validate_command_open_complete() {
        assert_eq!(validate_command("open editor test.txt"), PromptValidation::ValidComplete);
        assert_eq!(validate_command("open cli"), PromptValidation::ValidComplete);
    }

    #[test]
    fn test_validate_command_open_invalid() {
        assert_eq!(validate_command("open invalid"), PromptValidation::Invalid);
    }

    #[test]
    fn test_validate_command_help() {
        assert_eq!(validate_command("help"), PromptValidation::ValidComplete);
        assert_eq!(validate_command("help workspace"), PromptValidation::ValidComplete);
        assert_eq!(validate_command("help invalid"), PromptValidation::Invalid);
    }

    #[test]
    fn test_validate_command_close() {
        assert_eq!(validate_command("close"), PromptValidation::ValidPrefix);
        assert_eq!(validate_command("close comp:123"), PromptValidation::ValidComplete);
        assert_eq!(validate_command("close invalid"), PromptValidation::Invalid);
    }

    #[test]
    fn test_validate_command_partial() {
        assert_eq!(validate_command("op"), PromptValidation::ValidPrefix);
        assert_eq!(validate_command("li"), PromptValidation::ValidPrefix);
        assert_eq!(validate_command("he"), PromptValidation::ValidPrefix);
    }

    #[test]
    fn test_validate_command_invalid() {
        assert_eq!(validate_command("invalid"), PromptValidation::Invalid);
        assert_eq!(validate_command("xyz"), PromptValidation::Invalid);
    }

    #[test]
    fn test_actionable_error_format() {
        let error = ActionableError::new(
            "Filesystem unavailable",
            vec!["Retry".to_string(), "Help".to_string()],
        );
        let formatted = error.format();
        assert_eq!(formatted, "Filesystem unavailable — Retry | Help");
    }

    #[test]
    fn test_actionable_error_with_retry_help() {
        let error = ActionableError::with_retry_help("Connection failed");
        assert_eq!(error.actions.len(), 2);
        assert_eq!(error.actions[0], "Retry");
        assert_eq!(error.actions[1], "Help");
    }

    #[test]
    fn test_actionable_error_with_help() {
        let error = ActionableError::with_help("Invalid command");
        assert_eq!(error.actions.len(), 1);
        assert_eq!(error.actions[0], "Help");
    }
}
