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

/// A picture the compositor draws by number (GFX-094): straight-alpha
/// RGBA, `width * height * 4` bytes, row-major. Built into the kernel, so
/// the table is `'static` and travels with the theme at no cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    pub width: u16,
    pub height: u16,
    pub rgba: &'static [u8],
}

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
    /// The pictures `DrawOp::Picture` and a dock tile's picture name by
    /// index (GFX-094). Empty unless the host has pictures to give: then
    /// nothing that asks for one draws, and everything else is as it was.
    #[serde(skip)]
    pub pictures: &'static [Picture],
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
        pictures: &[],
    };

    /// The desk's theme (GFX-052): the default palette with a soft vertical
    /// gradient behind the cards. Applied per scene, so nothing that is not
    /// the desk changes.
    pub const DESK: Theme = Theme {
        background_bottom: RgbaColor::new(22, 26, 44, 255),
        ..Theme::DEFAULT
    };

    /// The desk's presets (GFX-059), by the name the Look app shows.
    pub const PRESETS: [(&'static str, Theme); 5] = [
        ("Dusk", Theme::DESK),
        ("Daylight", Theme::LIGHT),
        ("Ember", Theme::EMBER),
        ("Forest", Theme::FOREST),
        ("Mono", Theme::MONO),
    ];

    /// Accent colours a preset can take (GFX-059): focus ring, caret,
    /// the dock's running dot.
    pub const ACCENTS: [(&'static str, RgbaColor); 4] = [
        ("Mint", RgbaColor::new(52, 211, 153, 255)),
        ("Sky", RgbaColor::new(96, 165, 250, 255)),
        ("Amber", RgbaColor::new(251, 191, 36, 255)),
        ("Rose", RgbaColor::new(244, 114, 182, 255)),
    ];

    /// A preset by name, case-insensitively.
    pub fn named(name: &str) -> Option<Theme> {
        Theme::PRESETS
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| *t)
    }

    /// An accent by name, case-insensitively.
    pub fn accent_named(name: &str) -> Option<RgbaColor> {
        Theme::ACCENTS
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, c)| *c)
    }

    /// This theme with another accent: the ring, the caret's colour is
    /// left alone (it is orange on purpose, to be found).
    /// This theme with a table of pictures to draw by number (GFX-094).
    pub const fn with_pictures(mut self, pictures: &'static [Picture]) -> Theme {
        self.pictures = pictures;
        self
    }

    pub const fn with_accent(mut self, accent: RgbaColor) -> Theme {
        self.accent = accent;
        self.border_focused = accent;
        self
    }

    /// Warm dark: embers under a night sky.
    pub const EMBER: Theme = Theme {
        background: RgbaColor::new(26, 16, 18, 255),
        background_bottom: RgbaColor::new(44, 22, 26, 255),
        surface: RgbaColor::new(40, 28, 30, 255),
        surface_raised: RgbaColor::new(32, 22, 24, 255),
        hairline: RgbaColor::new(70, 50, 52, 255),
        shadow: RgbaColor::new(16, 10, 12, 255),
        text: RgbaColor::new(244, 232, 226, 255),
        text_muted: RgbaColor::new(176, 150, 142, 255),
        selection: RgbaColor::new(98, 58, 52, 255),
        tab_active: RgbaColor::new(40, 28, 30, 255),
        tab_inactive: RgbaColor::new(64, 44, 46, 255),
        title_focused: RgbaColor::new(70, 40, 36, 255),
        title_unfocused: RgbaColor::new(50, 36, 38, 255),
        title_notice: RgbaColor::new(90, 62, 30, 255),
        title_palette: RgbaColor::new(78, 46, 44, 255),
        border_unfocused: RgbaColor::new(120, 96, 92, 255),
        ..Theme::DEFAULT
    };

    /// Deep green: a desk in the shade.
    pub const FOREST: Theme = Theme {
        background: RgbaColor::new(12, 24, 20, 255),
        background_bottom: RgbaColor::new(18, 38, 30, 255),
        surface: RgbaColor::new(24, 40, 34, 255),
        surface_raised: RgbaColor::new(18, 32, 27, 255),
        hairline: RgbaColor::new(44, 68, 58, 255),
        shadow: RgbaColor::new(8, 16, 13, 255),
        text: RgbaColor::new(226, 240, 232, 255),
        text_muted: RgbaColor::new(140, 170, 154, 255),
        selection: RgbaColor::new(40, 84, 66, 255),
        tab_active: RgbaColor::new(24, 40, 34, 255),
        tab_inactive: RgbaColor::new(40, 62, 52, 255),
        title_focused: RgbaColor::new(30, 66, 52, 255),
        title_unfocused: RgbaColor::new(30, 48, 40, 255),
        title_notice: RgbaColor::new(78, 66, 30, 255),
        title_palette: RgbaColor::new(34, 72, 58, 255),
        border_unfocused: RgbaColor::new(96, 124, 110, 255),
        ..Theme::DEFAULT
    };

    /// Greys only, for the accent to do all the pointing.
    pub const MONO: Theme = Theme {
        background: RgbaColor::new(18, 18, 20, 255),
        background_bottom: RgbaColor::new(30, 30, 34, 255),
        surface: RgbaColor::new(34, 34, 38, 255),
        surface_raised: RgbaColor::new(26, 26, 30, 255),
        hairline: RgbaColor::new(60, 60, 66, 255),
        shadow: RgbaColor::new(10, 10, 12, 255),
        text: RgbaColor::new(236, 236, 240, 255),
        text_muted: RgbaColor::new(150, 150, 160, 255),
        selection: RgbaColor::new(70, 70, 80, 255),
        tab_active: RgbaColor::new(34, 34, 38, 255),
        tab_inactive: RgbaColor::new(54, 54, 60, 255),
        title_focused: RgbaColor::new(58, 58, 64, 255),
        title_unfocused: RgbaColor::new(44, 44, 48, 255),
        title_notice: RgbaColor::new(80, 70, 40, 255),
        title_palette: RgbaColor::new(60, 60, 68, 255),
        border_unfocused: RgbaColor::new(110, 110, 120, 255),
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
        pictures: &[],
    };
}

impl Default for Theme {
    fn default() -> Self {
        Self::DEFAULT
    }
}
