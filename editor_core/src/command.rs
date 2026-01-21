//! Command parsing and execution

use alloc::string::String;

/// Parsed command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Quit editor
    Quit { force: bool },
    /// Write to current file
    Write,
    /// Write to specified path
    WriteAs(String),
    /// Write and quit
    WriteQuit,
    /// Unknown command
    Unknown(String),
}

/// Command outcome
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Request to quit
    RequestQuit { forced: bool },
    /// Request to save
    RequestSave,
    /// Request to save as
    RequestSaveAs(String),
    /// Request to save and quit
    RequestSaveAndQuit,
    /// Show error message
    Error(String),
}

/// Parse command string (without leading ':')
pub fn parse_command(cmd_str: &str) -> Command {
    let trimmed = cmd_str.trim();
    
    match trimmed {
        "q" => Command::Quit { force: false },
        "q!" => Command::Quit { force: true },
        "w" => Command::Write,
        "wq" => Command::WriteQuit,
        _ if trimmed.starts_with("w ") => {
            let path = trimmed[2..].trim();
            Command::WriteAs(path.into())
        }
        _ => Command::Unknown(trimmed.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quit() {
        assert_eq!(parse_command("q"), Command::Quit { force: false });
        assert_eq!(parse_command("q!"), Command::Quit { force: true });
    }

    #[test]
    fn test_parse_write() {
        assert_eq!(parse_command("w"), Command::Write);
        assert_eq!(parse_command("wq"), Command::WriteQuit);
    }

    #[test]
    fn test_parse_write_as() {
        match parse_command("w test.txt") {
            Command::WriteAs(path) => assert_eq!(path, "test.txt"),
            _ => panic!("Expected WriteAs"),
        }
    }

    #[test]
    fn test_parse_unknown() {
        match parse_command("unknown") {
            Command::Unknown(s) => assert_eq!(s, "unknown"),
            _ => panic!("Expected Unknown"),
        }
    }
}
