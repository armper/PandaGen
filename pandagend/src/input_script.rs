//! # Input Script Parser
//!
//! Provides a simple scripted input format for deterministic testing and demos.
//!
//! ## Format
//!
//! Scripts are line-based, with each line representing one input action:
//! - Key names: `Enter`, `Escape`, `Backspace`, `Tab`, `Space`
//! - Arrow keys: `Up`, `Down`, `Left`, `Right`
//! - Alphanumeric: `a`, `A`, `0-9` (single characters)
//! - Modifiers: `Ctrl+c`, `Alt+x`, `Shift+a`
//! - Text strings: `"Hello World"` (expanded to individual key presses)
//! - Comments: `# This is a comment`
//! - Delays: `wait 100ms` (for timing control)
//!
//! ## Example
//!
//! ```text
//! # Open editor and type text
//! i                    # Enter insert mode
//! "Hello Panda"        # Type text
//! Escape               # Exit insert mode
//! :wq                  # Save and quit
//! Enter
//! ```

use input_types::{InputEvent, KeyCode, KeyEvent, Modifiers};
use std::collections::VecDeque;
use thiserror::Error;

/// Input script error types
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InputScriptError {
    #[error("Invalid key name: {0}")]
    InvalidKeyName(String),

    #[error("Invalid modifier: {0}")]
    InvalidModifier(String),

    #[error("Parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },

    #[error("Empty script")]
    EmptyScript,

    #[error("Invalid delay format: {0}")]
    InvalidDelay(String),
}

/// A single scripted input action
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedInput {
    /// A single key press
    Key(KeyCode, Modifiers),
    /// Wait for a duration (in milliseconds)
    Wait(u64),
}

/// Input script
///
/// Parses and provides scripted input events for deterministic testing.
#[derive(Debug, Clone)]
pub struct InputScript {
    inputs: VecDeque<ScriptedInput>,
}

impl InputScript {
    /// Creates a new empty input script
    pub fn new() -> Self {
        Self {
            inputs: VecDeque::new(),
        }
    }

    /// Parses a script from text
    pub fn from_text(text: &str) -> Result<Self, InputScriptError> {
        let mut inputs = VecDeque::new();

        for (line_num, line) in text.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse the line
            let parsed = Self::parse_line(line, line_num + 1)?;
            inputs.extend(parsed);
        }

        if inputs.is_empty() {
            return Err(InputScriptError::EmptyScript);
        }

        Ok(Self { inputs })
    }

    /// Parses a single line of script
    fn parse_line(line: &str, line_num: usize) -> Result<Vec<ScriptedInput>, InputScriptError> {
        let line = line.trim();

        // Handle wait command
        if line.starts_with("wait ") {
            let duration_str = line.strip_prefix("wait ").unwrap().trim();
            let millis =
                Self::parse_duration(duration_str).map_err(|e| InputScriptError::ParseError {
                    line: line_num,
                    message: e.to_string(),
                })?;
            return Ok(vec![ScriptedInput::Wait(millis)]);
        }

        // Handle quoted strings
        if line.starts_with('"') && line.ends_with('"') {
            let text = &line[1..line.len() - 1];
            return Ok(text
                .chars()
                .map(|c| ScriptedInput::Key(Self::char_to_keycode(c), Modifiers::none()))
                .collect());
        }

        // Handle single key or key with modifiers
        let (modifiers, key_name) = Self::parse_modifiers(line)?;
        let keycode = Self::parse_keycode(key_name).map_err(|e| InputScriptError::ParseError {
            line: line_num,
            message: e.to_string(),
        })?;

        Ok(vec![ScriptedInput::Key(keycode, modifiers)])
    }

    /// Parses modifiers from a key string (e.g., "Ctrl+c" → (Modifiers::CTRL, "c"))
    fn parse_modifiers(input: &str) -> Result<(Modifiers, &str), InputScriptError> {
        let mut modifiers = Modifiers::none();
        let parts: Vec<&str> = input.split('+').collect();

        if parts.len() == 1 {
            return Ok((modifiers, input));
        }

        // All but last part are modifiers
        for modifier_str in &parts[..parts.len() - 1] {
            modifiers = match modifier_str.trim().to_lowercase().as_str() {
                "ctrl" | "control" => modifiers.with(Modifiers::CTRL),
                "alt" => modifiers.with(Modifiers::ALT),
                "shift" => modifiers.with(Modifiers::SHIFT),
                "super" | "meta" => modifiers.with(Modifiers::META),
                other => return Err(InputScriptError::InvalidModifier(other.to_string())),
            };
        }

        Ok((modifiers, parts[parts.len() - 1].trim()))
    }

    /// Parses a key name to KeyCode
    fn parse_keycode(name: &str) -> Result<KeyCode, InputScriptError> {
        match name.to_lowercase().as_str() {
            // Special keys
            "enter" | "return" => Ok(KeyCode::Enter),
            "escape" | "esc" => Ok(KeyCode::Escape),
            "backspace" | "back" => Ok(KeyCode::Backspace),
            "tab" => Ok(KeyCode::Tab),
            "space" => Ok(KeyCode::Space),

            // Arrow keys
            "up" | "arrowup" => Ok(KeyCode::Up),
            "down" | "arrowdown" => Ok(KeyCode::Down),
            "left" | "arrowleft" => Ok(KeyCode::Left),
            "right" | "arrowright" => Ok(KeyCode::Right),

            // Function keys
            "f1" => Ok(KeyCode::F1),
            "f2" => Ok(KeyCode::F2),
            "f3" => Ok(KeyCode::F3),
            "f4" => Ok(KeyCode::F4),
            "f5" => Ok(KeyCode::F5),
            "f6" => Ok(KeyCode::F6),
            "f7" => Ok(KeyCode::F7),
            "f8" => Ok(KeyCode::F8),
            "f9" => Ok(KeyCode::F9),
            "f10" => Ok(KeyCode::F10),
            "f11" => Ok(KeyCode::F11),
            "f12" => Ok(KeyCode::F12),

            // Single character
            _ if name.len() == 1 => {
                let c = name.chars().next().unwrap();
                Ok(Self::char_to_keycode(c))
            }

            // Unknown key
            _ => Err(InputScriptError::InvalidKeyName(name.to_string())),
        }
    }

    /// Converts a character to a KeyCode
    fn char_to_keycode(c: char) -> KeyCode {
        match c {
            'a'..='z' | 'A'..='Z' => {
                let upper = c.to_ascii_uppercase();
                match upper {
                    'A' => KeyCode::A,
                    'B' => KeyCode::B,
                    'C' => KeyCode::C,
                    'D' => KeyCode::D,
                    'E' => KeyCode::E,
                    'F' => KeyCode::F,
                    'G' => KeyCode::G,
                    'H' => KeyCode::H,
                    'I' => KeyCode::I,
                    'J' => KeyCode::J,
                    'K' => KeyCode::K,
                    'L' => KeyCode::L,
                    'M' => KeyCode::M,
                    'N' => KeyCode::N,
                    'O' => KeyCode::O,
                    'P' => KeyCode::P,
                    'Q' => KeyCode::Q,
                    'R' => KeyCode::R,
                    'S' => KeyCode::S,
                    'T' => KeyCode::T,
                    'U' => KeyCode::U,
                    'V' => KeyCode::V,
                    'W' => KeyCode::W,
                    'X' => KeyCode::X,
                    'Y' => KeyCode::Y,
                    'Z' => KeyCode::Z,
                    _ => unreachable!(),
                }
            }
            '0'..='9' => match c {
                '0' => KeyCode::Num0,
                '1' => KeyCode::Num1,
                '2' => KeyCode::Num2,
                '3' => KeyCode::Num3,
                '4' => KeyCode::Num4,
                '5' => KeyCode::Num5,
                '6' => KeyCode::Num6,
                '7' => KeyCode::Num7,
                '8' => KeyCode::Num8,
                '9' => KeyCode::Num9,
                _ => unreachable!(),
            },
            ' ' => KeyCode::Space,
            ':' => KeyCode::Semicolon, // Colon requires shift
            '/' => KeyCode::Slash,
            '.' => KeyCode::Period,
            ',' => KeyCode::Comma,
            _ => KeyCode::Unknown,
        }
    }

    /// Parses a duration string (e.g., "100ms", "1s")
    fn parse_duration(s: &str) -> Result<u64, InputScriptError> {
        let s = s.trim().to_lowercase();

        if let Some(ms_str) = s.strip_suffix("ms") {
            ms_str
                .trim()
                .parse::<u64>()
                .map_err(|_| InputScriptError::InvalidDelay(s.to_string()))
        } else if let Some(s_str) = s.strip_suffix('s') {
            s_str
                .trim()
                .parse::<u64>()
                .map(|s| s * 1000)
                .map_err(|_| InputScriptError::InvalidDelay(s.to_string()))
        } else {
            Err(InputScriptError::InvalidDelay(s.to_string()))
        }
    }

    /// Returns the next input event, if any
    pub fn next_input(&mut self) -> Option<ScriptedInput> {
        self.inputs.pop_front()
    }

    /// Returns true if the script has more inputs
    pub fn has_more(&self) -> bool {
        !self.inputs.is_empty()
    }

    /// Returns the number of remaining inputs
    pub fn remaining(&self) -> usize {
        self.inputs.len()
    }

    /// Converts a scripted input to an InputEvent
    pub fn to_input_event(input: &ScriptedInput) -> Option<InputEvent> {
        match input {
            ScriptedInput::Key(code, modifiers) => {
                Some(InputEvent::Key(KeyEvent::pressed(*code, *modifiers)))
            }
            ScriptedInput::Wait(_) => None, // Wait is handled by host
        }
    }
}

impl Default for InputScript {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_key() {
        let script = InputScript::from_text("a").unwrap();
        assert_eq!(script.remaining(), 1);

        let mut script = script;
        let input = script.next_input().unwrap();
        assert_eq!(input, ScriptedInput::Key(KeyCode::A, Modifiers::none()));
    }

    #[test]
    fn test_parse_special_keys() {
        let script = InputScript::from_text("Enter\nEscape\nBackspace").unwrap();
        assert_eq!(script.remaining(), 3);

        let mut script = script;
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::Enter, Modifiers::none())
        );
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::Escape, Modifiers::none())
        );
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::Backspace, Modifiers::none())
        );
    }

    #[test]
    fn test_parse_modifiers() {
        let script = InputScript::from_text("Ctrl+c\nAlt+x\nShift+a").unwrap();
        assert_eq!(script.remaining(), 3);

        let mut script = script;
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::C, Modifiers::CTRL)
        );
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::X, Modifiers::ALT)
        );
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::A, Modifiers::SHIFT)
        );
    }

    #[test]
    fn test_parse_quoted_string() {
        let script = InputScript::from_text(r#""Hi""#).unwrap();
        assert_eq!(script.remaining(), 2);

        let mut script = script;
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::H, Modifiers::none())
        );
        assert_eq!(
            script.next_input().unwrap(),
            ScriptedInput::Key(KeyCode::I, Modifiers::none())
        );
    }

    #[test]
    fn test_parse_wait() {
        let script = InputScript::from_text("wait 100ms\nwait 2s").unwrap();
        assert_eq!(script.remaining(), 2);

        let mut script = script;
        assert_eq!(script.next_input().unwrap(), ScriptedInput::Wait(100));
        assert_eq!(script.next_input().unwrap(), ScriptedInput::Wait(2000));
    }

    #[test]
    fn test_parse_comments() {
        let script = InputScript::from_text("# Comment\na\n# Another comment\nb").unwrap();
        assert_eq!(script.remaining(), 2);
    }

    #[test]
    fn test_parse_empty_lines() {
        let script = InputScript::from_text("a\n\nb\n\n\nc").unwrap();
        assert_eq!(script.remaining(), 3);
    }

    #[test]
    fn test_empty_script_error() {
        let result = InputScript::from_text("");
        assert!(matches!(result, Err(InputScriptError::EmptyScript)));
    }

    #[test]
    fn test_empty_script_with_comments() {
        let result = InputScript::from_text("# Just comments\n# Nothing else");
        assert!(matches!(result, Err(InputScriptError::EmptyScript)));
    }

    #[test]
    fn test_invalid_key_name() {
        let result = InputScript::from_text("InvalidKeyName");
        assert!(matches!(result, Err(InputScriptError::ParseError { .. })));
    }

    #[test]
    fn test_invalid_modifier() {
        let result = InputScript::from_text("Invalid+a");
        assert!(matches!(result, Err(InputScriptError::InvalidModifier(_))));
    }

    #[test]
    fn test_invalid_delay() {
        let result = InputScript::from_text("wait abc");
        assert!(matches!(result, Err(InputScriptError::ParseError { .. })));
    }

    #[test]
    fn test_complex_script() {
        let script = InputScript::from_text(
            r#"
            # Open editor
            i
            "Hello Panda"
            Escape
            ":wq"
            Enter
        "#,
        )
        .unwrap();

        // i + 11 chars (Hello Panda) + Escape + 3 chars (:wq) + Enter = 17
        assert_eq!(script.remaining(), 17);
    }

    #[test]
    fn test_to_input_event() {
        let key_input = ScriptedInput::Key(KeyCode::A, Modifiers::none());
        let event = InputScript::to_input_event(&key_input).unwrap();
        assert!(matches!(event, InputEvent::Key(_)));

        let wait_input = ScriptedInput::Wait(100);
        let event = InputScript::to_input_event(&wait_input);
        assert!(event.is_none());
    }
}
