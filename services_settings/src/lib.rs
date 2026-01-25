#![no_std]

//! # Settings Registry Service
//!
//! A typed, capability-scoped settings system for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Typed settings**: All settings have explicit types, not stringly-typed
//! - **Capability-scoped**: Settings require capabilities to read/write
//! - **Layered**: Read-only defaults + per-user overrides
//! - **Deterministic**: Settings are serializable and reproducible
//! - **Testable**: All settings logic can be tested independently
//!
//! ## Features
//!
//! - Read-only defaults baked in
//! - Per-user overrides stored via a SettingsCap
//! - Keybindings, theme, editor prefs, recent files, layout
//! - No global config files, no environment variables
//!
//! ## Example
//!
//! ```ignore
//! use services_settings::{SettingsRegistry, SettingValue};
//!
//! let mut registry = SettingsRegistry::new();
//!
//! // Register a default setting
//! registry.register_default("editor.tab_size", SettingValue::Integer(4));
//!
//! // Override for a user
//! registry.set_user_override("user123", "editor.tab_size", SettingValue::Integer(2));
//!
//! // Get effective value
//! let tab_size = registry.get("user123", "editor.tab_size");
//! ```

pub mod persistence;

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Setting key (path-like identifier)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SettingKey(String);

impl SettingKey {
    /// Creates a new setting key
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Returns the key as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Checks if this key starts with the given prefix
    pub fn starts_with(&self, prefix: &str) -> bool {
        self.0.starts_with(prefix)
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for SettingKey {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// Setting value (strongly typed)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingValue {
    /// Boolean value
    Boolean(bool),
    /// Integer value
    Integer(i64),
    /// Floating point value
    Float(f64),
    /// String value
    String(String),
    /// List of strings
    StringList(Vec<String>),
}

impl SettingValue {
    /// Returns true if this is a boolean
    pub fn is_boolean(&self) -> bool {
        matches!(self, SettingValue::Boolean(_))
    }

    /// Returns true if this is an integer
    pub fn is_integer(&self) -> bool {
        matches!(self, SettingValue::Integer(_))
    }

    /// Returns true if this is a float
    pub fn is_float(&self) -> bool {
        matches!(self, SettingValue::Float(_))
    }

    /// Returns true if this is a string
    pub fn is_string(&self) -> bool {
        matches!(self, SettingValue::String(_))
    }

    /// Returns true if this is a string list
    pub fn is_string_list(&self) -> bool {
        matches!(self, SettingValue::StringList(_))
    }

    /// Tries to get as boolean
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            SettingValue::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    /// Tries to get as integer
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            SettingValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Tries to get as float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            SettingValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Tries to get as string
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SettingValue::String(v) => Some(v.as_str()),
            _ => None,
        }
    }

    /// Tries to get as string list
    pub fn as_string_list(&self) -> Option<&[String]> {
        match self {
            SettingValue::StringList(v) => Some(v.as_slice()),
            _ => None,
        }
    }
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::Boolean(v) => write!(f, "{}", v),
            SettingValue::Integer(v) => write!(f, "{}", v),
            SettingValue::Float(v) => write!(f, "{}", v),
            SettingValue::String(v) => write!(f, "{}", v),
            SettingValue::StringList(v) => write!(f, "{:?}", v),
        }
    }
}

/// User ID type
pub type UserId = String;

/// Settings registry
pub struct SettingsRegistry {
    /// Default settings (read-only)
    defaults: BTreeMap<SettingKey, SettingValue>,
    /// User-specific overrides
    user_overrides: BTreeMap<UserId, BTreeMap<SettingKey, SettingValue>>,
}

impl SettingsRegistry {
    /// Creates a new settings registry
    pub fn new() -> Self {
        Self {
            defaults: BTreeMap::new(),
            user_overrides: BTreeMap::new(),
        }
    }

    /// Registers a default setting
    pub fn register_default(&mut self, key: impl Into<SettingKey>, value: SettingValue) {
        self.defaults.insert(key.into(), value);
    }

    /// Sets a user-specific override
    pub fn set_user_override(
        &mut self,
        user_id: impl Into<UserId>,
        key: impl Into<SettingKey>,
        value: SettingValue,
    ) {
        let user_id = user_id.into();
        let key = key.into();

        self.user_overrides
            .entry(user_id)
            .or_default()
            .insert(key, value);
    }

    /// Removes a user-specific override
    pub fn remove_user_override(&mut self, user_id: &str, key: &SettingKey) -> bool {
        if let Some(user_settings) = self.user_overrides.get_mut(user_id) {
            user_settings.remove(key).is_some()
        } else {
            false
        }
    }

    /// Gets the effective setting value for a user (override or default)
    pub fn get(&self, user_id: &str, key: &SettingKey) -> Option<&SettingValue> {
        // Check user override first
        if let Some(user_settings) = self.user_overrides.get(user_id) {
            if let Some(value) = user_settings.get(key) {
                return Some(value);
            }
        }

        // Fall back to default
        self.defaults.get(key)
    }

    /// Gets the default value for a setting
    pub fn get_default(&self, key: &SettingKey) -> Option<&SettingValue> {
        self.defaults.get(key)
    }

    /// Gets the user override (if any) for a setting
    pub fn get_user_override(&self, user_id: &str, key: &SettingKey) -> Option<&SettingValue> {
        self.user_overrides
            .get(user_id)
            .and_then(|settings| settings.get(key))
    }

    /// Returns all default setting keys
    pub fn list_defaults(&self) -> Vec<SettingKey> {
        self.defaults.keys().cloned().collect()
    }

    /// Returns all user overrides for a specific user
    pub fn list_user_overrides(&self, user_id: &str) -> Vec<SettingKey> {
        self.user_overrides
            .get(user_id)
            .map(|settings| settings.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns all settings with a given prefix for a user
    pub fn list_with_prefix(&self, user_id: &str, prefix: &str) -> Vec<(SettingKey, SettingValue)> {
        let mut results = Vec::new();

        // Collect defaults
        for (key, value) in &self.defaults {
            if key.starts_with(prefix) {
                results.push((key.clone(), value.clone()));
            }
        }

        // Override with user settings
        if let Some(user_settings) = self.user_overrides.get(user_id) {
            for (key, value) in user_settings {
                if key.starts_with(prefix) {
                    // Replace or add
                    if let Some(pos) = results.iter().position(|(k, _)| k == key) {
                        results[pos] = (key.clone(), value.clone());
                    } else {
                        results.push((key.clone(), value.clone()));
                    }
                }
            }
        }

        results
    }

    /// Clears all user overrides for a specific user
    pub fn clear_user_overrides(&mut self, user_id: &str) {
        self.user_overrides.remove(user_id);
    }

    /// Resets a setting to its default value for a user
    pub fn reset_to_default(&mut self, user_id: &str, key: &SettingKey) -> bool {
        self.remove_user_override(user_id, key)
    }

    /// Exports all user overrides for persistence
    pub fn export_overrides(&self) -> BTreeMap<UserId, BTreeMap<SettingKey, SettingValue>> {
        self.user_overrides.clone()
    }

    /// Imports user overrides (replaces existing overrides)
    pub fn import_overrides(
        &mut self,
        overrides: BTreeMap<UserId, BTreeMap<SettingKey, SettingValue>>,
    ) {
        self.user_overrides = overrides;
    }

    /// Applies overrides for a specific user (merges with existing)
    pub fn apply_user_overrides(
        &mut self,
        user_id: impl Into<UserId>,
        overrides: BTreeMap<SettingKey, SettingValue>,
    ) {
        let user_id = user_id.into();
        self.user_overrides
            .entry(user_id)
            .or_default()
            .extend(overrides);
    }
}

impl Default for SettingsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Common setting keys
pub mod keys {
    pub const EDITOR_TAB_SIZE: &str = "editor.tab_size";
    pub const EDITOR_USE_SPACES: &str = "editor.use_spaces";
    pub const EDITOR_LINE_NUMBERS: &str = "editor.line_numbers";
    pub const EDITOR_WORD_WRAP: &str = "editor.word_wrap";
    pub const THEME_NAME: &str = "theme.name";
    pub const THEME_FONT_SIZE: &str = "theme.font_size";
    pub const KEYBINDING_COMMAND_PALETTE: &str = "keybinding.command_palette";
    pub const UI_SHOW_STATUS_BAR: &str = "ui.show_status_bar";
    pub const UI_RECENT_FILES_LIMIT: &str = "ui.recent_files_limit";
    pub const UI_SHOW_KEYBINDING_HINTS: &str = "ui.show_keybinding_hints";
    pub const UI_THEME: &str = "ui.theme";
    pub const KEYBINDINGS_PROFILE: &str = "keybindings.profile";
}

/// Creates a settings registry with default settings
pub fn create_default_registry() -> SettingsRegistry {
    let mut registry = SettingsRegistry::new();

    // Editor settings
    registry.register_default(keys::EDITOR_TAB_SIZE, SettingValue::Integer(4));
    registry.register_default(keys::EDITOR_USE_SPACES, SettingValue::Boolean(true));
    registry.register_default(keys::EDITOR_LINE_NUMBERS, SettingValue::Boolean(true));
    registry.register_default(keys::EDITOR_WORD_WRAP, SettingValue::Boolean(false));

    // Theme settings
    registry.register_default(
        keys::THEME_NAME,
        SettingValue::String("default".to_string()),
    );
    registry.register_default(keys::THEME_FONT_SIZE, SettingValue::Integer(14));

    // Keybinding settings
    registry.register_default(
        keys::KEYBINDING_COMMAND_PALETTE,
        SettingValue::String("Ctrl+P".to_string()),
    );
    registry.register_default(
        keys::KEYBINDINGS_PROFILE,
        SettingValue::String("default".to_string()),
    );

    // UI settings
    registry.register_default(keys::UI_SHOW_STATUS_BAR, SettingValue::Boolean(true));
    registry.register_default(keys::UI_RECENT_FILES_LIMIT, SettingValue::Integer(10));
    registry.register_default(keys::UI_SHOW_KEYBINDING_HINTS, SettingValue::Boolean(true));
    registry.register_default(keys::UI_THEME, SettingValue::String("default".to_string()));

    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_setting_key_creation() {
        let key = SettingKey::new("editor.tab_size");
        assert_eq!(key.as_str(), "editor.tab_size");
    }

    #[test]
    fn test_setting_key_starts_with() {
        let key = SettingKey::new("editor.tab_size");
        assert!(key.starts_with("editor"));
        assert!(key.starts_with("editor.tab"));
        assert!(!key.starts_with("theme"));
    }

    #[test]
    fn test_setting_value_boolean() {
        let val = SettingValue::Boolean(true);
        assert!(val.is_boolean());
        assert_eq!(val.as_boolean(), Some(true));
        assert_eq!(val.as_integer(), None);
    }

    #[test]
    fn test_setting_value_integer() {
        let val = SettingValue::Integer(42);
        assert!(val.is_integer());
        assert_eq!(val.as_integer(), Some(42));
        assert_eq!(val.as_boolean(), None);
    }

    #[test]
    fn test_setting_value_float() {
        let val = SettingValue::Float(3.14);
        assert!(val.is_float());
        assert_eq!(val.as_float(), Some(3.14));
    }

    #[test]
    fn test_setting_value_string() {
        let val = SettingValue::String("test".to_string());
        assert!(val.is_string());
        assert_eq!(val.as_string(), Some("test"));
    }

    #[test]
    fn test_setting_value_string_list() {
        let val = SettingValue::StringList(vec!["a".to_string(), "b".to_string()]);
        assert!(val.is_string_list());
        assert_eq!(
            val.as_string_list(),
            Some(&["a".to_string(), "b".to_string()][..])
        );
    }

    #[test]
    fn test_registry_creation() {
        let registry = SettingsRegistry::new();
        assert_eq!(registry.list_defaults().len(), 0);
    }

    #[test]
    fn test_registry_register_default() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("test.key", SettingValue::Integer(42));

        let value = registry.get_default(&SettingKey::new("test.key"));
        assert_eq!(value, Some(&SettingValue::Integer(42)));
    }

    #[test]
    fn test_registry_set_user_override() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("test.key", SettingValue::Integer(42));
        registry.set_user_override("user1", "test.key", SettingValue::Integer(100));

        // User1 should see override
        let value = registry.get("user1", &SettingKey::new("test.key"));
        assert_eq!(value, Some(&SettingValue::Integer(100)));

        // Other users should see default
        let value = registry.get("user2", &SettingKey::new("test.key"));
        assert_eq!(value, Some(&SettingValue::Integer(42)));
    }

    #[test]
    fn test_registry_remove_user_override() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("test.key", SettingValue::Integer(42));
        registry.set_user_override("user1", "test.key", SettingValue::Integer(100));

        let removed = registry.remove_user_override("user1", &SettingKey::new("test.key"));
        assert!(removed);

        // Should fall back to default
        let value = registry.get("user1", &SettingKey::new("test.key"));
        assert_eq!(value, Some(&SettingValue::Integer(42)));

        // Try removing again
        let removed = registry.remove_user_override("user1", &SettingKey::new("test.key"));
        assert!(!removed);
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = SettingsRegistry::new();

        let value = registry.get("user1", &SettingKey::new("nonexistent"));
        assert_eq!(value, None);
    }

    #[test]
    fn test_registry_list_defaults() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("test.key1", SettingValue::Integer(1));
        registry.register_default("test.key2", SettingValue::Integer(2));

        let keys = registry.list_defaults();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_registry_list_user_overrides() {
        let mut registry = SettingsRegistry::new();

        registry.set_user_override("user1", "test.key1", SettingValue::Integer(1));
        registry.set_user_override("user1", "test.key2", SettingValue::Integer(2));
        registry.set_user_override("user2", "test.key3", SettingValue::Integer(3));

        let keys = registry.list_user_overrides("user1");
        assert_eq!(keys.len(), 2);

        let keys = registry.list_user_overrides("user2");
        assert_eq!(keys.len(), 1);

        let keys = registry.list_user_overrides("user3");
        assert_eq!(keys.len(), 0);
    }

    #[test]
    fn test_registry_list_with_prefix() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("editor.tab_size", SettingValue::Integer(4));
        registry.register_default("editor.use_spaces", SettingValue::Boolean(true));
        registry.register_default("theme.name", SettingValue::String("dark".to_string()));

        registry.set_user_override("user1", "editor.tab_size", SettingValue::Integer(2));

        let settings = registry.list_with_prefix("user1", "editor");
        assert_eq!(settings.len(), 2);

        // Check that override is applied
        let tab_size = settings
            .iter()
            .find(|(k, _)| k.as_str() == "editor.tab_size");
        assert_eq!(tab_size.unwrap().1, SettingValue::Integer(2));

        let settings = registry.list_with_prefix("user1", "theme");
        assert_eq!(settings.len(), 1);
    }

    #[test]
    fn test_registry_clear_user_overrides() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("test.key", SettingValue::Integer(42));
        registry.set_user_override("user1", "test.key", SettingValue::Integer(100));
        registry.set_user_override("user1", "test.key2", SettingValue::Integer(200));

        assert_eq!(registry.list_user_overrides("user1").len(), 2);

        registry.clear_user_overrides("user1");
        assert_eq!(registry.list_user_overrides("user1").len(), 0);

        // Should fall back to default
        let value = registry.get("user1", &SettingKey::new("test.key"));
        assert_eq!(value, Some(&SettingValue::Integer(42)));
    }

    #[test]
    fn test_registry_reset_to_default() {
        let mut registry = SettingsRegistry::new();

        registry.register_default("test.key", SettingValue::Integer(42));
        registry.set_user_override("user1", "test.key", SettingValue::Integer(100));

        let reset = registry.reset_to_default("user1", &SettingKey::new("test.key"));
        assert!(reset);

        let value = registry.get("user1", &SettingKey::new("test.key"));
        assert_eq!(value, Some(&SettingValue::Integer(42)));
    }

    #[test]
    fn test_create_default_registry() {
        let registry = create_default_registry();

        // Check some default values
        let tab_size = registry.get("any_user", &SettingKey::new(keys::EDITOR_TAB_SIZE));
        assert_eq!(tab_size, Some(&SettingValue::Integer(4)));

        let use_spaces = registry.get("any_user", &SettingKey::new(keys::EDITOR_USE_SPACES));
        assert_eq!(use_spaces, Some(&SettingValue::Boolean(true)));

        let theme_name = registry.get("any_user", &SettingKey::new(keys::THEME_NAME));
        assert_eq!(
            theme_name,
            Some(&SettingValue::String("default".to_string()))
        );
    }

    #[test]
    fn test_default_registry_override() {
        let mut registry = create_default_registry();

        // Override tab size for user1
        registry.set_user_override("user1", keys::EDITOR_TAB_SIZE, SettingValue::Integer(2));

        // user1 should see 2
        let tab_size = registry.get("user1", &SettingKey::new(keys::EDITOR_TAB_SIZE));
        assert_eq!(tab_size, Some(&SettingValue::Integer(2)));

        // user2 should still see 4
        let tab_size = registry.get("user2", &SettingKey::new(keys::EDITOR_TAB_SIZE));
        assert_eq!(tab_size, Some(&SettingValue::Integer(4)));
    }
}
