//! Editor modes

#[cfg(feature = "serde_support")]
use serde::{Deserialize, Serialize};

/// Editor mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
pub enum EditorMode {
    /// Normal mode (navigation and commands)
    Normal,
    /// Insert mode (text entry)
    Insert,
    /// Command mode (ex commands like :q, :w)
    Command,
    /// Search mode (search prompt)
    Search,
}

impl EditorMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Command => "COMMAND",
            EditorMode::Search => "SEARCH",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_strings() {
        assert_eq!(EditorMode::Normal.as_str(), "NORMAL");
        assert_eq!(EditorMode::Insert.as_str(), "INSERT");
        assert_eq!(EditorMode::Command.as_str(), "COMMAND");
        assert_eq!(EditorMode::Search.as_str(), "SEARCH");
    }
}
