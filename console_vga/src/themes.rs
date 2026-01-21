//! # Themes Module
//!
//! Color theming system for VGA console.
//!
//! ## Philosophy
//!
//! - **Semantic colors**: Error, success, info, warning have consistent colors
//! - **Predefined themes**: Dark, light, and custom themes
//! - **Editor integration**: Themes apply to editor and CLI
//! - **Persistent storage**: Theme config survives reboots

use crate::{Style, VgaColor};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Color roles for semantic coloring
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorRole {
    /// Normal text
    Normal,
    /// Bold/emphasized text
    Bold,
    /// Error messages
    Error,
    /// Success messages
    Success,
    /// Info messages
    Info,
    /// Warning messages
    Warning,
    /// Background
    Background,
    /// Editor cursor
    Cursor,
    /// Selected text
    Selection,
    /// Line numbers (editor)
    LineNumber,
    /// Status line
    StatusLine,
}

/// Color pair: foreground and background
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorPair {
    pub fg: VgaColor,
    pub bg: VgaColor,
}

impl ColorPair {
    /// Create a new color pair
    pub fn new(fg: VgaColor, bg: VgaColor) -> Self {
        Self { fg, bg }
    }

    /// Convert to VGA attribute byte
    pub fn to_attr(&self) -> u8 {
        VgaColor::make_attr(self.fg, self.bg)
    }
}

/// Theme definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// Theme name
    pub name: String,
    /// Color assignments for roles
    colors: HashMap<ColorRole, ColorPair>,
}

impl Theme {
    /// Create a new empty theme
    pub fn new(name: String) -> Self {
        Self {
            name,
            colors: HashMap::new(),
        }
    }

    /// Set a color for a role
    pub fn set_color(&mut self, role: ColorRole, pair: ColorPair) {
        self.colors.insert(role, pair);
    }

    /// Get a color for a role
    pub fn get_color(&self, role: ColorRole) -> Option<ColorPair> {
        self.colors.get(&role).copied()
    }

    /// Get a color for a role, with fallback to default
    pub fn get_color_or_default(&self, role: ColorRole) -> ColorPair {
        self.get_color(role).unwrap_or_else(|| {
            // Default fallback: light gray on black
            ColorPair::new(VgaColor::LightGray, VgaColor::Black)
        })
    }

    /// Convert a style to a color pair using the theme
    pub fn style_to_pair(&self, style: Style) -> ColorPair {
        match style {
            Style::Normal => self.get_color_or_default(ColorRole::Normal),
            Style::Bold => self.get_color_or_default(ColorRole::Bold),
            Style::Error => self.get_color_or_default(ColorRole::Error),
            Style::Success => self.get_color_or_default(ColorRole::Success),
            Style::Info => self.get_color_or_default(ColorRole::Info),
        }
    }

    /// Create a dark theme (default)
    pub fn dark() -> Self {
        let mut theme = Self::new("dark".to_string());

        theme.set_color(
            ColorRole::Normal,
            ColorPair::new(VgaColor::LightGray, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Bold,
            ColorPair::new(VgaColor::White, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Error,
            ColorPair::new(VgaColor::LightRed, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Success,
            ColorPair::new(VgaColor::LightGreen, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Info,
            ColorPair::new(VgaColor::LightCyan, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Warning,
            ColorPair::new(VgaColor::Yellow, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Background,
            ColorPair::new(VgaColor::LightGray, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Cursor,
            ColorPair::new(VgaColor::Black, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Selection,
            ColorPair::new(VgaColor::Black, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::LineNumber,
            ColorPair::new(VgaColor::DarkGray, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::StatusLine,
            ColorPair::new(VgaColor::Black, VgaColor::LightGray),
        );

        theme
    }

    /// Create a light theme
    pub fn light() -> Self {
        let mut theme = Self::new("light".to_string());

        theme.set_color(
            ColorRole::Normal,
            ColorPair::new(VgaColor::Black, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Bold,
            ColorPair::new(VgaColor::Black, VgaColor::White),
        );
        theme.set_color(
            ColorRole::Error,
            ColorPair::new(VgaColor::Red, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Success,
            ColorPair::new(VgaColor::Green, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Info,
            ColorPair::new(VgaColor::Blue, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Warning,
            ColorPair::new(VgaColor::Brown, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Background,
            ColorPair::new(VgaColor::Black, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::Cursor,
            ColorPair::new(VgaColor::White, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Selection,
            ColorPair::new(VgaColor::White, VgaColor::DarkGray),
        );
        theme.set_color(
            ColorRole::LineNumber,
            ColorPair::new(VgaColor::DarkGray, VgaColor::LightGray),
        );
        theme.set_color(
            ColorRole::StatusLine,
            ColorPair::new(VgaColor::White, VgaColor::Blue),
        );

        theme
    }

    /// Create a high-contrast theme
    pub fn high_contrast() -> Self {
        let mut theme = Self::new("high_contrast".to_string());

        theme.set_color(
            ColorRole::Normal,
            ColorPair::new(VgaColor::White, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Bold,
            ColorPair::new(VgaColor::Yellow, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Error,
            ColorPair::new(VgaColor::White, VgaColor::Red),
        );
        theme.set_color(
            ColorRole::Success,
            ColorPair::new(VgaColor::Black, VgaColor::Green),
        );
        theme.set_color(
            ColorRole::Info,
            ColorPair::new(VgaColor::White, VgaColor::Blue),
        );
        theme.set_color(
            ColorRole::Warning,
            ColorPair::new(VgaColor::Black, VgaColor::Yellow),
        );
        theme.set_color(
            ColorRole::Background,
            ColorPair::new(VgaColor::White, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::Cursor,
            ColorPair::new(VgaColor::Black, VgaColor::White),
        );
        theme.set_color(
            ColorRole::Selection,
            ColorPair::new(VgaColor::Black, VgaColor::Yellow),
        );
        theme.set_color(
            ColorRole::LineNumber,
            ColorPair::new(VgaColor::Yellow, VgaColor::Black),
        );
        theme.set_color(
            ColorRole::StatusLine,
            ColorPair::new(VgaColor::Black, VgaColor::White),
        );

        theme
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Theme manager
pub struct ThemeManager {
    /// Active theme
    active_theme: Theme,
    /// Available themes
    themes: HashMap<String, Theme>,
}

impl ThemeManager {
    /// Create a new theme manager with default themes
    pub fn new() -> Self {
        let dark = Theme::dark();
        let light = Theme::light();
        let high_contrast = Theme::high_contrast();

        let mut themes = HashMap::new();
        themes.insert("dark".to_string(), dark.clone());
        themes.insert("light".to_string(), light);
        themes.insert("high_contrast".to_string(), high_contrast);

        Self {
            active_theme: dark,
            themes,
        }
    }

    /// Get the active theme
    pub fn active_theme(&self) -> &Theme {
        &self.active_theme
    }

    /// Set the active theme by name
    pub fn set_theme(&mut self, name: &str) -> Result<(), String> {
        if let Some(theme) = self.themes.get(name) {
            self.active_theme = theme.clone();
            Ok(())
        } else {
            Err(format!("Theme '{}' not found", name))
        }
    }

    /// Add a custom theme
    pub fn add_theme(&mut self, theme: Theme) {
        self.themes.insert(theme.name.clone(), theme);
    }

    /// Remove a theme
    pub fn remove_theme(&mut self, name: &str) -> Result<(), String> {
        if name == "dark" || name == "light" || name == "high_contrast" {
            return Err("Cannot remove built-in theme".to_string());
        }
        if self.active_theme.name == name {
            return Err("Cannot remove active theme".to_string());
        }
        self.themes
            .remove(name)
            .ok_or_else(|| format!("Theme '{}' not found", name))?;
        Ok(())
    }

    /// Get a theme by name
    pub fn get_theme(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name)
    }

    /// List all theme names
    pub fn theme_names(&self) -> Vec<String> {
        self.themes.keys().cloned().collect()
    }

    /// Get color for a role using the active theme
    pub fn get_color(&self, role: ColorRole) -> ColorPair {
        self.active_theme.get_color_or_default(role)
    }

    /// Convert a style to VGA attribute using the active theme
    pub fn style_to_attr(&self, style: Style) -> u8 {
        self.active_theme.style_to_pair(style).to_attr()
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let data = serde_json::json!({
            "active_theme": self.active_theme.name,
            "themes": self.themes,
        });
        serde_json::to_string_pretty(&data)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, String> {
        let data: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;

        let active_name = data["active_theme"]
            .as_str()
            .ok_or("Missing active_theme")?;

        let themes_data = data["themes"].as_object().ok_or("Missing themes")?;

        let mut themes = HashMap::new();
        for (name, theme_data) in themes_data {
            let theme: Theme = serde_json::from_value(theme_data.clone())
                .map_err(|e| format!("Failed to parse theme '{}': {}", name, e))?;
            themes.insert(name.clone(), theme);
        }

        let active_theme = themes
            .get(active_name)
            .ok_or_else(|| format!("Active theme '{}' not found", active_name))?
            .clone();

        Ok(Self {
            active_theme,
            themes,
        })
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_pair_creation() {
        let pair = ColorPair::new(VgaColor::White, VgaColor::Black);
        assert_eq!(pair.fg, VgaColor::White);
        assert_eq!(pair.bg, VgaColor::Black);
    }

    #[test]
    fn test_color_pair_to_attr() {
        let pair = ColorPair::new(VgaColor::White, VgaColor::Black);
        let attr = pair.to_attr();
        assert_eq!(attr, VgaColor::make_attr(VgaColor::White, VgaColor::Black));
    }

    #[test]
    fn test_theme_creation() {
        let theme = Theme::new("test".to_string());
        assert_eq!(theme.name, "test");
    }

    #[test]
    fn test_theme_set_get_color() {
        let mut theme = Theme::new("test".to_string());
        let pair = ColorPair::new(VgaColor::White, VgaColor::Black);

        theme.set_color(ColorRole::Normal, pair);
        assert_eq!(theme.get_color(ColorRole::Normal), Some(pair));
        assert_eq!(theme.get_color(ColorRole::Error), None);
    }

    #[test]
    fn test_theme_get_color_or_default() {
        let theme = Theme::new("test".to_string());
        let pair = theme.get_color_or_default(ColorRole::Normal);
        assert_eq!(pair.fg, VgaColor::LightGray);
        assert_eq!(pair.bg, VgaColor::Black);
    }

    #[test]
    fn test_theme_style_to_pair() {
        let theme = Theme::dark();
        let pair = theme.style_to_pair(Style::Error);
        assert_eq!(pair.fg, VgaColor::LightRed);
        assert_eq!(pair.bg, VgaColor::Black);
    }

    #[test]
    fn test_dark_theme() {
        let theme = Theme::dark();
        assert_eq!(theme.name, "dark");

        let normal = theme.get_color(ColorRole::Normal).unwrap();
        assert_eq!(normal.fg, VgaColor::LightGray);
        assert_eq!(normal.bg, VgaColor::Black);

        let error = theme.get_color(ColorRole::Error).unwrap();
        assert_eq!(error.fg, VgaColor::LightRed);
    }

    #[test]
    fn test_light_theme() {
        let theme = Theme::light();
        assert_eq!(theme.name, "light");

        let normal = theme.get_color(ColorRole::Normal).unwrap();
        assert_eq!(normal.fg, VgaColor::Black);
        assert_eq!(normal.bg, VgaColor::LightGray);
    }

    #[test]
    fn test_high_contrast_theme() {
        let theme = Theme::high_contrast();
        assert_eq!(theme.name, "high_contrast");

        let normal = theme.get_color(ColorRole::Normal).unwrap();
        assert_eq!(normal.fg, VgaColor::White);
        assert_eq!(normal.bg, VgaColor::Black);
    }

    #[test]
    fn test_theme_serialization() {
        let theme = Theme::dark();
        let json = theme.to_json().unwrap();
        let deserialized = Theme::from_json(&json).unwrap();

        assert_eq!(theme.name, deserialized.name);
        assert_eq!(
            theme.get_color(ColorRole::Normal),
            deserialized.get_color(ColorRole::Normal)
        );
    }

    #[test]
    fn test_theme_manager_creation() {
        let manager = ThemeManager::new();
        assert_eq!(manager.active_theme().name, "dark");
        assert!(manager.theme_names().contains(&"dark".to_string()));
        assert!(manager.theme_names().contains(&"light".to_string()));
        assert!(manager.theme_names().contains(&"high_contrast".to_string()));
    }

    #[test]
    fn test_theme_manager_set_theme() {
        let mut manager = ThemeManager::new();
        assert_eq!(manager.active_theme().name, "dark");

        manager.set_theme("light").unwrap();
        assert_eq!(manager.active_theme().name, "light");

        let result = manager.set_theme("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_theme_manager_add_theme() {
        let mut manager = ThemeManager::new();
        let theme = Theme::new("custom".to_string());

        manager.add_theme(theme);
        assert!(manager.theme_names().contains(&"custom".to_string()));
    }

    #[test]
    fn test_theme_manager_remove_theme() {
        let mut manager = ThemeManager::new();
        let theme = Theme::new("custom".to_string());
        manager.add_theme(theme);

        manager.remove_theme("custom").unwrap();
        assert!(!manager.theme_names().contains(&"custom".to_string()));

        // Cannot remove built-in
        let result = manager.remove_theme("dark");
        assert!(result.is_err());

        // Cannot remove active
        manager.set_theme("light").unwrap();
        let result = manager.remove_theme("light");
        assert!(result.is_err());
    }

    #[test]
    fn test_theme_manager_get_color() {
        let manager = ThemeManager::new();
        let pair = manager.get_color(ColorRole::Error);
        assert_eq!(pair.fg, VgaColor::LightRed);
        assert_eq!(pair.bg, VgaColor::Black);
    }

    #[test]
    fn test_theme_manager_style_to_attr() {
        let manager = ThemeManager::new();
        let attr = manager.style_to_attr(Style::Success);
        assert_eq!(
            attr,
            VgaColor::make_attr(VgaColor::LightGreen, VgaColor::Black)
        );
    }

    #[test]
    fn test_theme_manager_serialization() {
        let manager = ThemeManager::new();
        let json = manager.to_json().unwrap();
        let deserialized = ThemeManager::from_json(&json).unwrap();

        assert_eq!(
            manager.active_theme().name,
            deserialized.active_theme().name
        );
        assert_eq!(
            manager.theme_names().len(),
            deserialized.theme_names().len()
        );
    }
}
