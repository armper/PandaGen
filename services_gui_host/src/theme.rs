//! Shell theme tokens (GFX-035).
//!
//! Every colour the compositor paints comes from a `Theme`, so the desktop
//! has one coherent visual language and a different look is a different
//! value, not a code change. Tokens are named for their role in the UI, not
//! for their hue, which is what lets a light theme reuse the same painter.
//!
//! Colours are plain RGBA; there is no dependency on ANSI or terminal
//! attributes anywhere in this model.

use graphics_rasterizer::RgbaColor;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    /// Desktop behind all windows.
    pub background: RgbaColor,
    /// Window content area.
    pub surface: RgbaColor,
    pub border_focused: RgbaColor,
    pub border_unfocused: RgbaColor,
    /// Title bar fill for the focused window.
    pub title_focused: RgbaColor,
    /// Title bar fill for unfocused windows.
    pub title_unfocused: RgbaColor,
    /// Title bar fill for notification cards.
    pub title_notice: RgbaColor,
    /// Title bar fill for the palette.
    pub title_palette: RgbaColor,
    /// Primary text.
    pub text: RgbaColor,
    /// De-emphasised text (unfocused titles, inactive tabs).
    pub text_muted: RgbaColor,
    /// Text caret.
    pub caret: RgbaColor,
    /// Active tab body; matches `surface` so the tab joins its content.
    pub tab_active: RgbaColor,
    /// Inactive tab body.
    pub tab_inactive: RgbaColor,
    pub pointer_fill: RgbaColor,
    pub pointer_outline: RgbaColor,
}

impl Theme {
    /// The default dark slate theme.
    pub const DEFAULT: Theme = Theme {
        background: RgbaColor::new(12, 18, 28, 255),
        surface: RgbaColor::new(28, 34, 48, 255),
        border_focused: RgbaColor::new(52, 211, 153, 255),
        border_unfocused: RgbaColor::new(107, 114, 128, 255),
        title_focused: RgbaColor::new(34, 58, 70, 255),
        title_unfocused: RgbaColor::new(38, 44, 60, 255),
        title_notice: RgbaColor::new(78, 58, 30, 255),
        title_palette: RgbaColor::new(40, 66, 78, 255),
        text: RgbaColor::new(226, 232, 240, 255),
        text_muted: RgbaColor::new(148, 163, 184, 255),
        caret: RgbaColor::new(251, 146, 60, 255),
        tab_active: RgbaColor::new(28, 34, 48, 255),
        tab_inactive: RgbaColor::new(52, 60, 78, 255),
        pointer_fill: RgbaColor::new(245, 245, 245, 255),
        pointer_outline: RgbaColor::new(10, 10, 10, 255),
    };

    /// A light variant, mostly to prove the painter is theme-agnostic.
    pub const LIGHT: Theme = Theme {
        background: RgbaColor::new(226, 230, 236, 255),
        surface: RgbaColor::new(250, 250, 252, 255),
        border_focused: RgbaColor::new(16, 122, 90, 255),
        border_unfocused: RgbaColor::new(160, 168, 180, 255),
        title_focused: RgbaColor::new(196, 232, 220, 255),
        title_unfocused: RgbaColor::new(228, 232, 238, 255),
        title_notice: RgbaColor::new(250, 226, 180, 255),
        title_palette: RgbaColor::new(204, 228, 236, 255),
        text: RgbaColor::new(24, 30, 40, 255),
        text_muted: RgbaColor::new(96, 106, 122, 255),
        caret: RgbaColor::new(210, 90, 20, 255),
        tab_active: RgbaColor::new(250, 250, 252, 255),
        tab_inactive: RgbaColor::new(214, 220, 228, 255),
        pointer_fill: RgbaColor::new(20, 20, 20, 255),
        pointer_outline: RgbaColor::new(250, 250, 250, 255),
    };
}

impl Default for Theme {
    fn default() -> Self {
        Self::DEFAULT
    }
}
