//! # Host Control Commands
//!
//! Provides a minimal command surface for workspace operations during live run.
//!
//! ## Command Set
//!
//! - `open editor [path]` - Launch editor component
//! - `open cli` - Launch CLI console component
//! - `list` - List all components
//! - `focus <id>` - Focus a specific component by ID
//! - `next` - Switch to next component
//! - `prev` - Switch to previous component
//! - `close <id>` - Close a component
//! - `quit` - Exit the host
//!
//! ## Philosophy
//!
//! - No pipes, no scripting, no shell features
//! - Commands only orchestrate components
//! - Components do the actual work

use services_workspace_manager::ComponentId;
use thiserror::Error;

/// Host command error types
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostCommandError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Invalid component ID: {0}")]
    InvalidComponentId(String),

    #[error("Missing argument: {0}")]
    MissingArgument(String),

    #[error("Unknown command: {0}")]
    UnknownCommand(String),
}

/// Host commands
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCommand {
    /// Open editor component (optional path)
    OpenEditor { path: Option<String> },

    /// Open CLI console component
    OpenCli,

    /// List all components
    List,

    /// Focus a specific component
    Focus { component_id: ComponentId },

    /// Switch to next component
    Next,

    /// Switch to previous component
    Previous,

    /// Close a component
    Close { component_id: ComponentId },

    /// Quit the host
    Quit,
}

/// Host command parser
pub struct HostCommandParser;

impl HostCommandParser {
    /// Parses a command string
    pub fn parse(input: &str) -> Result<HostCommand, HostCommandError> {
        let input = input.trim();

        if input.is_empty() {
            return Err(HostCommandError::InvalidCommand(
                "Empty command".to_string(),
            ));
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "open" => Self::parse_open(&parts[1..]),
            "list" => Ok(HostCommand::List),
            "focus" => Self::parse_focus(&parts[1..]),
            "next" => Ok(HostCommand::Next),
            "prev" | "previous" => Ok(HostCommand::Previous),
            "close" => Self::parse_close(&parts[1..]),
            "quit" | "exit" => Ok(HostCommand::Quit),
            _ => Err(HostCommandError::UnknownCommand(cmd)),
        }
    }

    /// Parses the "open" command
    fn parse_open(args: &[&str]) -> Result<HostCommand, HostCommandError> {
        if args.is_empty() {
            return Err(HostCommandError::MissingArgument(
                "component type (editor, cli)".to_string(),
            ));
        }

        let component_type = args[0].to_lowercase();

        match component_type.as_str() {
            "editor" => {
                let path = if args.len() > 1 {
                    Some(args[1..].join(" "))
                } else {
                    None
                };
                Ok(HostCommand::OpenEditor { path })
            }
            "cli" | "console" => Ok(HostCommand::OpenCli),
            _ => Err(HostCommandError::InvalidCommand(format!(
                "Unknown component type: {}",
                component_type
            ))),
        }
    }

    /// Parses the "focus" command
    fn parse_focus(args: &[&str]) -> Result<HostCommand, HostCommandError> {
        if args.is_empty() {
            return Err(HostCommandError::MissingArgument(
                "component ID".to_string(),
            ));
        }

        let id_str = args[0];
        let component_id = Self::parse_component_id(id_str)?;

        Ok(HostCommand::Focus { component_id })
    }

    /// Parses the "close" command
    fn parse_close(args: &[&str]) -> Result<HostCommand, HostCommandError> {
        if args.is_empty() {
            return Err(HostCommandError::MissingArgument(
                "component ID".to_string(),
            ));
        }

        let id_str = args[0];
        let component_id = Self::parse_component_id(id_str)?;

        Ok(HostCommand::Close { component_id })
    }

    /// Parses a component ID from string
    ///
    /// Accepts:
    /// - Full UUID: "550e8400-e29b-41d4-a716-446655440000"
    /// - Short format: "comp:550e8400-..."
    /// - Just UUID part after "comp:"
    fn parse_component_id(s: &str) -> Result<ComponentId, HostCommandError> {
        let s = s.trim();

        // Strip "comp:" prefix if present
        let uuid_str = s.strip_prefix("comp:").unwrap_or(s);

        // Parse UUID
        let uuid = uuid::Uuid::parse_str(uuid_str)
            .map_err(|_| HostCommandError::InvalidComponentId(s.to_string()))?;

        Ok(ComponentId::from_uuid(uuid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_open_editor_no_path() {
        let cmd = HostCommandParser::parse("open editor").unwrap();
        assert_eq!(cmd, HostCommand::OpenEditor { path: None });
    }

    #[test]
    fn test_parse_open_editor_with_path() {
        let cmd = HostCommandParser::parse("open editor /path/to/file.txt").unwrap();
        assert_eq!(
            cmd,
            HostCommand::OpenEditor {
                path: Some("/path/to/file.txt".to_string())
            }
        );
    }

    #[test]
    fn test_parse_open_editor_with_path_spaces() {
        let cmd = HostCommandParser::parse("open editor /path with spaces/file.txt").unwrap();
        assert_eq!(
            cmd,
            HostCommand::OpenEditor {
                path: Some("/path with spaces/file.txt".to_string())
            }
        );
    }

    #[test]
    fn test_parse_open_cli() {
        let cmd = HostCommandParser::parse("open cli").unwrap();
        assert_eq!(cmd, HostCommand::OpenCli);
    }

    #[test]
    fn test_parse_open_console() {
        let cmd = HostCommandParser::parse("open console").unwrap();
        assert_eq!(cmd, HostCommand::OpenCli);
    }

    #[test]
    fn test_parse_list() {
        let cmd = HostCommandParser::parse("list").unwrap();
        assert_eq!(cmd, HostCommand::List);
    }

    #[test]
    fn test_parse_focus() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let cmd = HostCommandParser::parse(&format!("focus {}", uuid_str)).unwrap();

        if let HostCommand::Focus { component_id } = cmd {
            assert_eq!(
                component_id.as_uuid().to_string(),
                "550e8400-e29b-41d4-a716-446655440000"
            );
        } else {
            panic!("Expected Focus command");
        }
    }

    #[test]
    fn test_parse_focus_with_prefix() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let cmd = HostCommandParser::parse(&format!("focus comp:{}", uuid_str)).unwrap();

        if let HostCommand::Focus { component_id } = cmd {
            assert_eq!(
                component_id.as_uuid().to_string(),
                "550e8400-e29b-41d4-a716-446655440000"
            );
        } else {
            panic!("Expected Focus command");
        }
    }

    #[test]
    fn test_parse_next() {
        let cmd = HostCommandParser::parse("next").unwrap();
        assert_eq!(cmd, HostCommand::Next);
    }

    #[test]
    fn test_parse_prev() {
        let cmd = HostCommandParser::parse("prev").unwrap();
        assert_eq!(cmd, HostCommand::Previous);
    }

    #[test]
    fn test_parse_previous() {
        let cmd = HostCommandParser::parse("previous").unwrap();
        assert_eq!(cmd, HostCommand::Previous);
    }

    #[test]
    fn test_parse_close() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let cmd = HostCommandParser::parse(&format!("close {}", uuid_str)).unwrap();

        if let HostCommand::Close { component_id } = cmd {
            assert_eq!(
                component_id.as_uuid().to_string(),
                "550e8400-e29b-41d4-a716-446655440000"
            );
        } else {
            panic!("Expected Close command");
        }
    }

    #[test]
    fn test_parse_quit() {
        let cmd = HostCommandParser::parse("quit").unwrap();
        assert_eq!(cmd, HostCommand::Quit);
    }

    #[test]
    fn test_parse_exit() {
        let cmd = HostCommandParser::parse("exit").unwrap();
        assert_eq!(cmd, HostCommand::Quit);
    }

    #[test]
    fn test_parse_empty_command() {
        let result = HostCommandParser::parse("");
        assert!(matches!(result, Err(HostCommandError::InvalidCommand(_))));
    }

    #[test]
    fn test_parse_unknown_command() {
        let result = HostCommandParser::parse("unknown");
        assert!(matches!(result, Err(HostCommandError::UnknownCommand(_))));
    }

    #[test]
    fn test_parse_open_missing_type() {
        let result = HostCommandParser::parse("open");
        assert!(matches!(result, Err(HostCommandError::MissingArgument(_))));
    }

    #[test]
    fn test_parse_open_invalid_type() {
        let result = HostCommandParser::parse("open invalid");
        assert!(matches!(result, Err(HostCommandError::InvalidCommand(_))));
    }

    #[test]
    fn test_parse_focus_missing_id() {
        let result = HostCommandParser::parse("focus");
        assert!(matches!(result, Err(HostCommandError::MissingArgument(_))));
    }

    #[test]
    fn test_parse_focus_invalid_id() {
        let result = HostCommandParser::parse("focus invalid-uuid");
        assert!(matches!(
            result,
            Err(HostCommandError::InvalidComponentId(_))
        ));
    }

    #[test]
    fn test_parse_close_missing_id() {
        let result = HostCommandParser::parse("close");
        assert!(matches!(result, Err(HostCommandError::MissingArgument(_))));
    }

    #[test]
    fn test_parse_case_insensitive() {
        assert_eq!(
            HostCommandParser::parse("OPEN EDITOR").unwrap(),
            HostCommand::OpenEditor { path: None }
        );
        assert_eq!(HostCommandParser::parse("LIST").unwrap(), HostCommand::List);
        assert_eq!(HostCommandParser::parse("NEXT").unwrap(), HostCommand::Next);
        assert_eq!(HostCommandParser::parse("QUIT").unwrap(), HostCommand::Quit);
    }

    #[test]
    fn test_parse_whitespace_handling() {
        assert_eq!(
            HostCommandParser::parse("  open   editor  ").unwrap(),
            HostCommand::OpenEditor { path: None }
        );
        assert_eq!(
            HostCommandParser::parse("\tlist\t").unwrap(),
            HostCommand::List
        );
    }
}
