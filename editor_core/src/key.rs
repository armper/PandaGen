//! Platform-independent key representation

#[cfg(feature = "serde_support")]
use serde::{Deserialize, Serialize};

/// Platform-independent key event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
pub enum Key {
    // Printable ASCII
    Char(char),
    
    // Navigation
    Left,
    Right,
    Up,
    Down,
    
    // Special keys
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
    Space,
    
    // Letters (for commands in normal mode)
    H,
    J,
    K,
    L,
    I,
    A,
    X,
    D,
    U,
    N,
    
    // Commands
    Colon,
    Slash,
    
    // Modifiers (for Ctrl+R etc)
    CtrlR,
}

impl Key {
    /// Convert ASCII byte to Key (for PS/2 keyboard translation)
    pub fn from_ascii(byte: u8) -> Option<Self> {
        match byte {
            0x1B => Some(Key::Escape),
            0x08 | 0x7F => Some(Key::Backspace),
            b'\r' | b'\n' => Some(Key::Enter),
            b'\t' => Some(Key::Tab),
            b' ' => Some(Key::Space),
            b':' => Some(Key::Colon),
            b'/' => Some(Key::Slash),
            b'h' => Some(Key::H),
            b'j' => Some(Key::J),
            b'k' => Some(Key::K),
            b'l' => Some(Key::L),
            b'i' => Some(Key::I),
            b'a' => Some(Key::A),
            b'x' => Some(Key::X),
            b'd' => Some(Key::D),
            b'u' => Some(Key::U),
            b'n' => Some(Key::N),
            ch if (0x20..0x7F).contains(&ch) => Some(Key::Char(ch as char)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_ascii() {
        assert_eq!(Key::from_ascii(b'h'), Some(Key::H));
        assert_eq!(Key::from_ascii(b'i'), Some(Key::I));
        assert_eq!(Key::from_ascii(b' '), Some(Key::Space));
        assert_eq!(Key::from_ascii(b':'), Some(Key::Colon));
        assert_eq!(Key::from_ascii(0x1B), Some(Key::Escape));
        assert_eq!(Key::from_ascii(b'a'), Some(Key::A));
        assert_eq!(Key::from_ascii(b'Z'), Some(Key::Char('Z')));
    }
}
