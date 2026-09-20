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
    /// Highlighted (selected) content row.
    pub selection: RgbaColor,
    /// Active tab body; matches `surface` so the tab joins its content.
    pub tab_active: RgbaColor,
    /// Inactive tab body.
    pub tab_inactive: RgbaColor,
    pub pointer_fill: RgbaColor,
    pub pointer_outline: RgbaColor,
    /// Desk tokens (GFX-050). One accent, used only for focus, the caret and
    /// the dock's running dot; a raised surface for the shell strips; and a
    /// hairline for card edges. Hierarchy on this rasterizer comes from
    /// colour and space, not size -- there is one 8x16 font -- so these are
    /// deliberately few.
    #[serde(default = "Theme::default_accent")]
    pub accent: RgbaColor,
    #[serde(default = "Theme::default_surface_raised")]
    pub surface_raised: RgbaColor,
    #[serde(default = "Theme::default_hairline")]
    pub hairline: RgbaColor,
    /// Drawn under a card, offset by two pixels, to lift it off the desk.
    /// Fills overwrite rather than blend on this rasterizer, so it is a
    /// solid slightly-darker shape rather than a soft shadow.
    #[serde(default = "Theme::default_shadow")]
    pub shadow: RgbaColor,
    /// The desk fades from `background` at the top to this at the bottom.
    /// Equal to `background` means a flat fill. Fills overwrite on this
    /// rasterizer, so the gradient is painted a row at a time.
    #[serde(default = "Theme::default_background_bottom")]
    pub background_bottom: RgbaColor,
}

impl Theme {
    const fn default_accent() -> RgbaColor {
        Theme::DEFAULT.accent
    }
    const fn default_surface_raised() -> RgbaColor {
        Theme::DEFAULT.surface_raised
    }
    const fn default_hairline() -> RgbaColor {
        Theme::DEFAULT.hairline
    }
    const fn default_shadow() -> RgbaColor {
        Theme::DEFAULT.shadow
    }
    const fn default_background_bottom() -> RgbaColor {
        Theme::DEFAULT.background_bottom
    }
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
        selection: RgbaColor::new(44, 82, 96, 255),
        tab_active: RgbaColor::new(28, 34, 48, 255),
        tab_inactive: RgbaColor::new(52, 60, 78, 255),
        pointer_fill: RgbaColor::new(245, 245, 245, 255),
        pointer_outline: RgbaColor::new(10, 10, 10, 255),
        accent: RgbaColor::new(52, 211, 153, 255),
        surface_raised: RgbaColor::new(22, 28, 40, 255),
        hairline: RgbaColor::new(48, 56, 72, 255),
        shadow: RgbaColor::new(8, 12, 20, 255),
        // Flat by default: the classic graphics mode, the remote viewer's
        // golden fixtures and every existing pixel test expect one colour.
        background_bottom: RgbaColor::new(12, 18, 28, 255),
    };

    /// The desk's theme (GFX-052): the default palette with a soft vertical
    /// gradient behind the cards. Applied per scene, so nothing that is not
    /// the desk changes.
    pub const DESK: Theme = Theme {
        background_bottom: RgbaColor::new(22, 26, 44, 255),
        ..Theme::DEFAULT
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
        selection: RgbaColor::new(200, 224, 240, 255),
        tab_active: RgbaColor::new(250, 250, 252, 255),
        tab_inactive: RgbaColor::new(214, 220, 228, 255),
        pointer_fill: RgbaColor::new(20, 20, 20, 255),
        pointer_outline: RgbaColor::new(250, 250, 250, 255),
        accent: RgbaColor::new(16, 122, 90, 255),
        surface_raised: RgbaColor::new(238, 241, 246, 255),
        hairline: RgbaColor::new(200, 206, 216, 255),
        shadow: RgbaColor::new(196, 202, 212, 255),
        background_bottom: RgbaColor::new(226, 230, 236, 255),
    };
}

impl Default for Theme {
    fn default() -> Self {
        Self::DEFAULT
    }
}
