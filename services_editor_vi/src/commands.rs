//! Command parsing and execution

use thiserror::Error;

/// Command parsing error
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error("Unknown command: {0}")]
    UnknownCommand(String),

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    #[error("Cannot quit: unsaved changes (use :q! to force)")]
    UnsavedChanges,
}

/// Editor command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Write (save) the buffer
    Write,
    /// Quit the editor
    Quit,
    /// Force quit (discard changes)
    ForceQuit,
    /// Write and quit
    WriteQuit,
}

/// Command parser
pub struct CommandParser;

impl CommandParser {
    /// Parse a command string (without the leading ':')
    pub fn parse(cmd: &str) -> Result<Command, CommandError> {
        let trimmed = cmd.trim();

        match trimmed {
            "w" | "write" => Ok(Command::Write),
            "q" | "quit" => Ok(Command::Quit),
            "q!" | "quit!" => Ok(Command::ForceQuit),
            "wq" | "x" => Ok(Command::WriteQuit),
            "" => Err(CommandError::InvalidSyntax("Empty command".to_string())),
            _ => Err(CommandError::UnknownCommand(trimmed.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_write() {
        assert_eq!(CommandParser::parse("w"), Ok(Command::Write));
        assert_eq!(CommandParser::parse("write"), Ok(Command::Write));
        assert_eq!(CommandParser::parse(" w "), Ok(Command::Write));
    }

    #[test]
    fn test_parse_quit() {
        assert_eq!(CommandParser::parse("q"), Ok(Command::Quit));
        assert_eq!(CommandParser::parse("quit"), Ok(Command::Quit));
    }

    #[test]
    fn test_parse_force_quit() {
        assert_eq!(CommandParser::parse("q!"), Ok(Command::ForceQuit));
        assert_eq!(CommandParser::parse("quit!"), Ok(Command::ForceQuit));
    }

    #[test]
    fn test_parse_write_quit() {
        assert_eq!(CommandParser::parse("wq"), Ok(Command::WriteQuit));
        assert_eq!(CommandParser::parse("x"), Ok(Command::WriteQuit));
    }

    #[test]
    fn test_parse_empty_command() {
        assert_eq!(
            CommandParser::parse(""),
            Err(CommandError::InvalidSyntax("Empty command".to_string()))
        );
    }

    #[test]
    fn test_parse_unknown_command() {
        assert_eq!(
            CommandParser::parse("unknown"),
            Err(CommandError::UnknownCommand("unknown".to_string()))
        );
    }
}
