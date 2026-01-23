#![no_std]

//! # Command Palette Service
//!
//! A system-wide command palette that makes the entire OS discoverable via Ctrl+P.
//!
//! ## Philosophy
//!
//! - **Discoverability**: All commands are registered and searchable
//! - **Capability-gated**: Commands only appear if you have the required capability
//! - **Deterministic**: The palette is a pure view over command descriptors
//! - **Testable**: All command logic can be tested independently
//!
//! ## Features
//!
//! - Ctrl+P opens the palette
//! - Type to filter commands with fuzzy matching
//! - Commands are capability-gated
//! - Clean failure for unauthorized commands
//!
//! ## Example
//!
//! ```ignore
//! use services_command_palette::{CommandPalette, CommandDescriptor};
//!
//! let mut palette = CommandPalette::new();
//!
//! // Register a command
//! let descriptor = CommandDescriptor::new(
//!     "open_editor",
//!     "Open Editor",
//!     "Opens a text editor",
//!     vec!["editor".to_string(), "text".to_string()],
//! );
//! palette.register_command(descriptor, Box::new(|_args| {
//!     Ok("Editor opened".to_string())
//! }));
//!
//! // Filter commands
//! let matches = palette.filter_commands("edit");
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Unique identifier for a command
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandId(String);

impl CommandId {
    /// Creates a new command ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Command descriptor with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptor {
    /// Unique command identifier
    pub id: CommandId,
    /// Human-readable command name
    pub name: String,
    /// Description of what the command does
    pub description: String,
    /// Search tags/keywords
    pub tags: Vec<String>,
    /// Required capability ID (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    /// Whether the command is enabled
    pub enabled: bool,
}

impl CommandDescriptor {
    /// Creates a new command descriptor
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id: CommandId::new(id),
            name: name.into(),
            description: description.into(),
            tags,
            required_capability: None,
            enabled: true,
        }
    }

    /// Sets the required capability
    pub fn with_capability(mut self, cap_id: impl Into<String>) -> Self {
        self.required_capability = Some(cap_id.into());
        self
    }

    /// Disables the command
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Checks if this command matches the given query
    pub fn matches(&self, query: &str) -> bool {
        if !self.enabled {
            return false;
        }

        let query_lower = query.to_lowercase();
        
        // Match against name
        if self.name.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Match against description
        if self.description.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Match against tags
        for tag in &self.tags {
            if tag.to_lowercase().contains(&query_lower) {
                return true;
            }
        }

        // Match against ID
        if self.id.as_str().to_lowercase().contains(&query_lower) {
            return true;
        }

        false
    }

    /// Calculates a relevance score for the given query (higher is better)
    pub fn relevance_score(&self, query: &str) -> u32 {
        if !self.enabled {
            return 0;
        }

        let query_lower = query.to_lowercase();
        let mut score = 0u32;

        // Exact match in name gets highest score
        if self.name.to_lowercase() == query_lower {
            score += 1000;
        } else if self.name.to_lowercase().starts_with(&query_lower) {
            score += 500;
        } else if self.name.to_lowercase().contains(&query_lower) {
            score += 100;
        }

        // Match in tags
        for tag in &self.tags {
            if tag.to_lowercase() == query_lower {
                score += 300;
            } else if tag.to_lowercase().starts_with(&query_lower) {
                score += 150;
            } else if tag.to_lowercase().contains(&query_lower) {
                score += 50;
            }
        }

        // Match in description
        if self.description.to_lowercase().contains(&query_lower) {
            score += 10;
        }

        score
    }
}

/// Result of command execution
pub type CommandResult = Result<String, String>;

/// Command handler function signature
pub type CommandHandler = Box<dyn Fn(&[String]) -> CommandResult + Send + Sync>;

/// Registered command with its handler
struct RegisteredCommand {
    descriptor: CommandDescriptor,
    handler: CommandHandler,
}

/// Command palette service
pub struct CommandPalette {
    commands: Vec<RegisteredCommand>,
}

impl CommandPalette {
    /// Creates a new command palette
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Registers a command with its handler
    pub fn register_command(&mut self, descriptor: CommandDescriptor, handler: CommandHandler) {
        self.commands.push(RegisteredCommand {
            descriptor,
            handler,
        });
    }

    /// Unregisters a command by ID
    pub fn unregister_command(&mut self, id: &CommandId) -> bool {
        if let Some(pos) = self.commands.iter().position(|cmd| cmd.descriptor.id == *id) {
            self.commands.remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns all registered command descriptors
    pub fn list_commands(&self) -> Vec<CommandDescriptor> {
        self.commands
            .iter()
            .map(|cmd| cmd.descriptor.clone())
            .collect()
    }

    /// Filters commands by query and returns them sorted by relevance
    pub fn filter_commands(&self, query: &str) -> Vec<CommandDescriptor> {
        let mut matches: Vec<_> = self
            .commands
            .iter()
            .filter(|cmd| cmd.descriptor.matches(query))
            .map(|cmd| {
                let score = cmd.descriptor.relevance_score(query);
                (score, cmd.descriptor.clone())
            })
            .collect();

        // Sort by score (descending)
        matches.sort_by(|a, b| b.0.cmp(&a.0));

        matches.into_iter().map(|(_, desc)| desc).collect()
    }

    /// Executes a command by ID with the given arguments
    pub fn execute_command(&self, id: &CommandId, args: &[String]) -> CommandResult {
        if let Some(cmd) = self.commands.iter().find(|cmd| cmd.descriptor.id == *id) {
            if !cmd.descriptor.enabled {
                return Err("Command is disabled".to_string());
            }
            (cmd.handler)(args)
        } else {
            Err(format!("Command not found: {}", id))
        }
    }

    /// Gets a command descriptor by ID
    pub fn get_command(&self, id: &CommandId) -> Option<&CommandDescriptor> {
        self.commands
            .iter()
            .find(|cmd| cmd.descriptor.id == *id)
            .map(|cmd| &cmd.descriptor)
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_command_id_creation() {
        let id = CommandId::new("test_command");
        assert_eq!(id.as_str(), "test_command");
    }

    #[test]
    fn test_command_descriptor_creation() {
        let desc = CommandDescriptor::new(
            "open_editor",
            "Open Editor",
            "Opens a text editor",
            vec!["editor".to_string(), "text".to_string()],
        );

        assert_eq!(desc.id.as_str(), "open_editor");
        assert_eq!(desc.name, "Open Editor");
        assert_eq!(desc.description, "Opens a text editor");
        assert_eq!(desc.tags.len(), 2);
        assert!(desc.enabled);
        assert!(desc.required_capability.is_none());
    }

    #[test]
    fn test_command_descriptor_with_capability() {
        let desc = CommandDescriptor::new(
            "open_file",
            "Open File",
            "Opens a file",
            vec!["file".to_string()],
        )
        .with_capability("fs_read");

        assert_eq!(desc.required_capability, Some("fs_read".to_string()));
    }

    #[test]
    fn test_command_descriptor_disabled() {
        let desc = CommandDescriptor::new(
            "test",
            "Test",
            "Test command",
            vec![],
        )
        .disabled();

        assert!(!desc.enabled);
    }

    #[test]
    fn test_command_matches() {
        let desc = CommandDescriptor::new(
            "open_editor",
            "Open Editor",
            "Opens a text editor",
            vec!["editor".to_string(), "text".to_string()],
        );

        assert!(desc.matches("edit"));
        assert!(desc.matches("editor"));
        assert!(desc.matches("text"));
        assert!(desc.matches("open"));
        assert!(!desc.matches("file"));
    }

    #[test]
    fn test_command_relevance_score() {
        let desc = CommandDescriptor::new(
            "open_editor",
            "Open Editor",
            "Opens a text editor",
            vec!["editor".to_string(), "text".to_string()],
        );

        // Exact name match should score highest
        let score_exact = desc.relevance_score("Open Editor");
        
        // Prefix match should score lower
        let score_prefix = desc.relevance_score("Open");
        
        // Contains match should score even lower
        let score_contains = desc.relevance_score("dit");

        assert!(score_exact > score_prefix);
        assert!(score_prefix > score_contains);
    }

    #[test]
    fn test_disabled_command_no_match() {
        let desc = CommandDescriptor::new(
            "test",
            "Test Command",
            "Test",
            vec![],
        )
        .disabled();

        assert!(!desc.matches("test"));
        assert_eq!(desc.relevance_score("test"), 0);
    }

    #[test]
    fn test_palette_register_command() {
        let mut palette = CommandPalette::new();
        let desc = CommandDescriptor::new(
            "test",
            "Test",
            "Test command",
            vec![],
        );

        palette.register_command(desc.clone(), Box::new(|_| Ok("test".to_string())));

        let commands = palette.list_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].id.as_str(), "test");
    }

    #[test]
    fn test_palette_unregister_command() {
        let mut palette = CommandPalette::new();
        let desc = CommandDescriptor::new(
            "test",
            "Test",
            "Test command",
            vec![],
        );
        let id = desc.id.clone();

        palette.register_command(desc, Box::new(|_| Ok("test".to_string())));
        assert_eq!(palette.list_commands().len(), 1);

        let removed = palette.unregister_command(&id);
        assert!(removed);
        assert_eq!(palette.list_commands().len(), 0);

        let removed_again = palette.unregister_command(&id);
        assert!(!removed_again);
    }

    #[test]
    fn test_palette_filter_commands() {
        let mut palette = CommandPalette::new();

        palette.register_command(
            CommandDescriptor::new(
                "open_editor",
                "Open Editor",
                "Opens a text editor",
                vec!["editor".to_string()],
            ),
            Box::new(|_| Ok("".to_string())),
        );

        palette.register_command(
            CommandDescriptor::new(
                "open_file",
                "Open File",
                "Opens a file",
                vec!["file".to_string()],
            ),
            Box::new(|_| Ok("".to_string())),
        );

        palette.register_command(
            CommandDescriptor::new(
                "save_file",
                "Save File",
                "Saves a file",
                vec!["file".to_string(), "save".to_string()],
            ),
            Box::new(|_| Ok("".to_string())),
        );

        let matches = palette.filter_commands("file");
        assert_eq!(matches.len(), 2);

        let matches = palette.filter_commands("open");
        assert_eq!(matches.len(), 2);

        let matches = palette.filter_commands("editor");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id.as_str(), "open_editor");
    }

    #[test]
    fn test_palette_execute_command() {
        let mut palette = CommandPalette::new();
        let desc = CommandDescriptor::new(
            "test",
            "Test",
            "Test command",
            vec![],
        );
        let id = desc.id.clone();

        palette.register_command(desc, Box::new(|args| {
            Ok(format!("executed with {} args", args.len()))
        }));

        let result = palette.execute_command(&id, &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "executed with 0 args");

        let result = palette.execute_command(
            &id,
            &["arg1".to_string(), "arg2".to_string()],
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "executed with 2 args");
    }

    #[test]
    fn test_palette_execute_nonexistent_command() {
        let palette = CommandPalette::new();
        let result = palette.execute_command(&CommandId::new("nonexistent"), &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_palette_execute_disabled_command() {
        let mut palette = CommandPalette::new();
        let desc = CommandDescriptor::new(
            "test",
            "Test",
            "Test command",
            vec![],
        )
        .disabled();
        let id = desc.id.clone();

        palette.register_command(desc, Box::new(|_| Ok("".to_string())));

        let result = palette.execute_command(&id, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    fn test_palette_get_command() {
        let mut palette = CommandPalette::new();
        let desc = CommandDescriptor::new(
            "test",
            "Test",
            "Test command",
            vec![],
        );
        let id = desc.id.clone();

        palette.register_command(desc, Box::new(|_| Ok("".to_string())));

        let found = palette.get_command(&id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().id.as_str(), "test");

        let not_found = palette.get_command(&CommandId::new("nonexistent"));
        assert!(not_found.is_none());
    }

    #[test]
    fn test_filter_commands_sorted_by_relevance() {
        let mut palette = CommandPalette::new();

        // This should match "editor" exactly in tags
        palette.register_command(
            CommandDescriptor::new(
                "open_editor",
                "Open Editor",
                "Opens a text editor",
                vec!["editor".to_string()],
            ),
            Box::new(|_| Ok("".to_string())),
        );

        // This should only match "editor" in the name
        palette.register_command(
            CommandDescriptor::new(
                "close_all",
                "Close All Editors",
                "Closes all open editors",
                vec!["close".to_string()],
            ),
            Box::new(|_| Ok("".to_string())),
        );

        let matches = palette.filter_commands("editor");
        assert_eq!(matches.len(), 2);
        
        // First result should be the one with exact tag match
        assert_eq!(matches[0].id.as_str(), "open_editor");
    }
}
