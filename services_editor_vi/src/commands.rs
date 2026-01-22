//! Command parsing and execution

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

/// Command parsing error
#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    UnknownCommand(String),
    InvalidSyntax(String),
    UnsavedChanges,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::UnknownCommand(cmd) => write!(f, "Unknown command: {}", cmd),
            CommandError::InvalidSyntax(msg) => write!(f, "Invalid syntax: {}", msg),
            CommandError::UnsavedChanges => {
                write!(f, "Cannot quit: unsaved changes (use :q! to force)")
            }
        }
    }
}

/// Editor command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Write (save) the buffer
    Write,
    /// Write to a specific path (Save As)
    WriteAs { path: String },
    /// Edit/open a file
    Edit { path: String, force: bool },
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

        // Handle empty command
        if trimmed.is_empty() {
            return Err(CommandError::InvalidSyntax("Empty command".to_string()));
        }

        // Split command and arguments
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let command = parts[0];

        match command {
            "w" | "write" => {
                if parts.len() > 1 {
                    // :w <path> - Save As
                    let path = parts[1..].join(" ");
                    Ok(Command::WriteAs { path })
                } else {
                    // :w - Save to current file
                    Ok(Command::Write)
                }
            }
            "e" | "edit" => {
                if parts.len() > 1 {
                    let path = parts[1..].join(" ");
                    Ok(Command::Edit { path, force: false })
                } else {
                    Err(CommandError::InvalidSyntax(
                        "Usage: :e <path>".to_string(),
                    ))
                }
            }
            "e!" | "edit!" => {
                if parts.len() > 1 {
                    let path = parts[1..].join(" ");
                    Ok(Command::Edit { path, force: true })
                } else {
                    Err(CommandError::InvalidSyntax(
                        "Usage: :e! <path>".to_string(),
                    ))
                }
            }
            "q" | "quit" => Ok(Command::Quit),
            "q!" | "quit!" => Ok(Command::ForceQuit),
            "wq" | "x" => Ok(Command::WriteQuit),
            _ => Err(CommandError::UnknownCommand(command.to_string())),
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
    fn test_parse_write_as() {
        assert_eq!(
            CommandParser::parse("w test.txt"),
            Ok(Command::WriteAs {
                path: "test.txt".to_string()
            })
        );
        assert_eq!(
            CommandParser::parse("write myfile.txt"),
            Ok(Command::WriteAs {
                path: "myfile.txt".to_string()
            })
        );
        assert_eq!(
            CommandParser::parse("w path/to/file.txt"),
            Ok(Command::WriteAs {
                path: "path/to/file.txt".to_string()
            })
        );
    }

    #[test]
    fn test_parse_edit() {
        assert_eq!(
            CommandParser::parse("e notes.txt"),
            Ok(Command::Edit {
                path: "notes.txt".to_string(),
                force: false
            })
        );
        assert_eq!(
            CommandParser::parse("edit path/to/file.txt"),
            Ok(Command::Edit {
                path: "path/to/file.txt".to_string(),
                force: false
            })
        );
    }

    #[test]
    fn test_parse_edit_force() {
        assert_eq!(
            CommandParser::parse("e! notes.txt"),
            Ok(Command::Edit {
                path: "notes.txt".to_string(),
                force: true
            })
        );
        assert_eq!(
            CommandParser::parse("edit! path/to/file.txt"),
            Ok(Command::Edit {
                path: "path/to/file.txt".to_string(),
                force: true
            })
        );
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
