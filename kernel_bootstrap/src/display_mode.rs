//! Display mode selection (GFX-020).
//!
//! PandaGen can drive the framebuffer in two ways:
//!
//! - `TextConsole`: the mature text workspace draws glyph cells into a shadow
//!   framebuffer that is presented through the damage-aware presenter.
//! - `GraphicsDesktop`: workspace state is mapped into desktop windows,
//!   composed by `services_gui_host`, rasterized to RGBA, and presented as a
//!   full desktop surface.
//!
//! The mode is a boot-time choice (kernel command line `display=...`) that
//! can also be changed at runtime with the `display` workspace command. Both
//! paths share the same presenter and pacer, so switching is a policy change
//! in the loop rather than a different hardware path.

/// Which renderer owns the framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Text workspace on the shadow framebuffer.
    TextConsole,
    /// Composited graphical desktop.
    GraphicsDesktop,
}

impl DisplayMode {
    /// Mode used when neither the command line nor the user chose one.
    pub const DEFAULT: DisplayMode = DisplayMode::TextConsole;

    /// Command-line key whose value selects the mode (`display=graphics`).
    pub const CMDLINE_KEY: &'static str = "display";

    /// Parse a user- or cmdline-supplied mode name.
    pub fn parse(name: &str) -> Option<DisplayMode> {
        match name.trim() {
            "text" | "console" | "tty" => Some(DisplayMode::TextConsole),
            "graphics" | "gfx" | "desktop" | "gui" => Some(DisplayMode::GraphicsDesktop),
            _ => None,
        }
    }

    /// Extract `display=<mode>` from a whitespace-separated kernel command line.
    ///
    /// The last occurrence wins so an appended override takes effect. Unknown
    /// values are ignored rather than treated as errors: a typo on the boot
    /// line should never prevent the system from coming up in the default.
    pub fn from_cmdline(cmdline: &[u8]) -> Option<DisplayMode> {
        let text = core::str::from_utf8(cmdline).ok()?;
        let mut selected = None;
        for token in text.split_ascii_whitespace() {
            if let Some(value) = token.strip_prefix(Self::CMDLINE_KEY) {
                if let Some(value) = value.strip_prefix('=') {
                    if let Some(mode) = Self::parse(value) {
                        selected = Some(mode);
                    }
                }
            }
        }
        selected
    }

    pub const fn label(self) -> &'static str {
        match self {
            DisplayMode::TextConsole => "text",
            DisplayMode::GraphicsDesktop => "graphics",
        }
    }

    pub const fn is_graphics(self) -> bool {
        matches!(self, DisplayMode::GraphicsDesktop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_accepts_aliases() {
        assert_eq!(DisplayMode::parse("text"), Some(DisplayMode::TextConsole));
        assert_eq!(DisplayMode::parse(" tty "), Some(DisplayMode::TextConsole));
        assert_eq!(
            DisplayMode::parse("graphics"),
            Some(DisplayMode::GraphicsDesktop)
        );
        assert_eq!(
            DisplayMode::parse("gui"),
            Some(DisplayMode::GraphicsDesktop)
        );
        assert_eq!(DisplayMode::parse("vga"), None);
        assert_eq!(DisplayMode::parse(""), None);
    }

    #[test]
    fn test_cmdline_selects_last_valid_value() {
        assert_eq!(DisplayMode::from_cmdline(b""), None);
        assert_eq!(DisplayMode::from_cmdline(b"quiet loglevel=3"), None);
        assert_eq!(
            DisplayMode::from_cmdline(b"quiet display=graphics"),
            Some(DisplayMode::GraphicsDesktop)
        );
        assert_eq!(
            DisplayMode::from_cmdline(b"display=graphics display=text"),
            Some(DisplayMode::TextConsole)
        );
        assert_eq!(
            DisplayMode::from_cmdline(b"display=bogus display=gfx display=nope"),
            Some(DisplayMode::GraphicsDesktop)
        );
        // `displayfoo=graphics` is not the key.
        assert_eq!(DisplayMode::from_cmdline(b"displayfoo=graphics"), None);
        assert_eq!(DisplayMode::from_cmdline(&[0xFF, 0xFE]), None);
    }

    #[test]
    fn test_labels_round_trip() {
        for mode in [DisplayMode::TextConsole, DisplayMode::GraphicsDesktop] {
            assert_eq!(DisplayMode::parse(mode.label()), Some(mode));
        }
        assert!(DisplayMode::GraphicsDesktop.is_graphics());
        assert!(!DisplayMode::DEFAULT.is_graphics());
    }
}
