//! # Keybindings Module
//!
//! Configurable keybindings for PandaGen workspace.
//!
//! ## Philosophy
//!
//! - **Vim-like remapping**: Flexible key remapping similar to vim
//! - **Per-profile bindings**: Different profiles can have different keybindings
//! - **Persistent storage**: Keybindings stored in JSON configuration
//! - **Explicit mappings**: No hidden or implicit keybindings

use input_types::{KeyCode, KeyEvent, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Action that can be triggered by a keybinding
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Switch to next tile
    SwitchTile,
    /// Focus top tile
    FocusTop,
    /// Focus bottom tile
    FocusBottom,
    /// Save current document
    Save,
    /// Quit application
    Quit,
    /// Enter command mode
    CommandMode,
    /// Custom action with name
    Custom(String),
}

impl Action {
    /// Get the action name
    pub fn name(&self) -> &str {
        match self {
            Action::SwitchTile => "switch_tile",
            Action::FocusTop => "focus_top",
            Action::FocusBottom => "focus_bottom",
            Action::Save => "save",
            Action::Quit => "quit",
            Action::CommandMode => "command_mode",
            Action::Custom(name) => name,
        }
    }

    /// Parse action from name
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "switch_tile" => Some(Action::SwitchTile),
            "focus_top" => Some(Action::FocusTop),
            "focus_bottom" => Some(Action::FocusBottom),
            "save" => Some(Action::Save),
            "quit" => Some(Action::Quit),
            "command_mode" => Some(Action::CommandMode),
            _ => Some(Action::Custom(name.to_string())),
        }
    }
}

/// Key combination for binding
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyCombo {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

impl KeyCombo {
    /// Create a new key combination
    pub fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        Self { code, modifiers }
    }

    /// Create from a key event
    pub fn from_event(event: &KeyEvent) -> Self {
        Self {
            code: event.code,
            modifiers: event.modifiers,
        }
    }

    /// Check if this combo matches a key event
    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.code == event.code && self.modifiers == event.modifiers
    }
}

/// Keybinding profile
#[derive(Debug, Clone)]
pub struct KeyBindingProfile {
    /// Profile name
    pub name: String,
    /// Bindings map: key combo -> action
    bindings: HashMap<KeyCombo, Action>,
}

impl Serialize for KeyBindingProfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        // Convert bindings to a Vec for serialization
        let bindings_vec: Vec<(KeyCombo, Action)> = self
            .bindings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let mut state = serializer.serialize_struct("KeyBindingProfile", 2)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("bindings", &bindings_vec)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for KeyBindingProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            name: String,
            bindings: Vec<(KeyCombo, Action)>,
        }

        let helper = Helper::deserialize(deserializer)?;
        let bindings = helper.bindings.into_iter().collect();

        Ok(KeyBindingProfile {
            name: helper.name,
            bindings,
        })
    }
}

impl KeyBindingProfile {
    /// Create a new empty profile
    pub fn new(name: String) -> Self {
        Self {
            name,
            bindings: HashMap::new(),
        }
    }

    /// Add a keybinding
    pub fn bind(&mut self, combo: KeyCombo, action: Action) {
        self.bindings.insert(combo, action);
    }

    /// Remove a keybinding
    pub fn unbind(&mut self, combo: &KeyCombo) -> Option<Action> {
        self.bindings.remove(combo)
    }

    /// Get the action for a key event
    pub fn get_action(&self, event: &KeyEvent) -> Option<&Action> {
        let combo = KeyCombo::from_event(event);
        self.bindings.get(&combo)
    }

    /// Get all bindings
    pub fn bindings(&self) -> &HashMap<KeyCombo, Action> {
        &self.bindings
    }

    /// Check if a key combo is bound
    pub fn is_bound(&self, combo: &KeyCombo) -> bool {
        self.bindings.contains_key(combo)
    }

    /// Clear all bindings
    pub fn clear(&mut self) {
        self.bindings.clear();
    }

    /// Get the number of bindings
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// Create a default profile with common bindings
    pub fn default_profile() -> Self {
        let mut profile = Self::new("default".to_string());

        // Alt+Tab to switch tiles
        profile.bind(
            KeyCombo::new(KeyCode::Tab, Modifiers::ALT),
            Action::SwitchTile,
        );

        // Ctrl+S to save
        profile.bind(KeyCombo::new(KeyCode::S, Modifiers::CTRL), Action::Save);

        // Ctrl+Q to quit
        profile.bind(KeyCombo::new(KeyCode::Q, Modifiers::CTRL), Action::Quit);

        // Ctrl+1 to focus top
        profile.bind(
            KeyCombo::new(KeyCode::Num1, Modifiers::CTRL),
            Action::FocusTop,
        );

        // Ctrl+2 to focus bottom
        profile.bind(
            KeyCombo::new(KeyCode::Num2, Modifiers::CTRL),
            Action::FocusBottom,
        );

        /*
        // Escape for command mode - REMOVED to allow editor to use Escape
        profile.bind(
            KeyCombo::new(KeyCode::Escape, Modifiers::NONE),
            Action::CommandMode,
        );
        */

        profile
    }

    /// Create a vim-style profile
    pub fn vim_profile() -> Self {
        let mut profile = Self::new("vim".to_string());

        // Ctrl+W followed by w to switch windows
        profile.bind(
            KeyCombo::new(KeyCode::Tab, Modifiers::CTRL),
            Action::SwitchTile,
        );

        // :w to save (in command mode, handled separately)
        profile.bind(KeyCombo::new(KeyCode::S, Modifiers::CTRL), Action::Save);

        // :q to quit (in command mode, handled separately)
        profile.bind(KeyCombo::new(KeyCode::Q, Modifiers::CTRL), Action::Quit);

        // Escape for command mode
        profile.bind(
            KeyCombo::new(KeyCode::Escape, Modifiers::NONE),
            Action::CommandMode,
        );

        profile
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

/// Keybinding manager
pub struct KeyBindingManager {
    /// Active profile
    active_profile: KeyBindingProfile,
    /// Available profiles
    profiles: HashMap<String, KeyBindingProfile>,
}

impl KeyBindingManager {
    /// Create a new keybinding manager with default profile
    pub fn new() -> Self {
        let default = KeyBindingProfile::default_profile();
        let vim = KeyBindingProfile::vim_profile();

        let mut profiles = HashMap::new();
        profiles.insert("default".to_string(), default.clone());
        profiles.insert("vim".to_string(), vim);

        Self {
            active_profile: default,
            profiles,
        }
    }

    /// Get the active profile
    pub fn active_profile(&self) -> &KeyBindingProfile {
        &self.active_profile
    }

    /// Get the active profile mutably
    pub fn active_profile_mut(&mut self) -> &mut KeyBindingProfile {
        &mut self.active_profile
    }

    /// Switch to a different profile
    pub fn set_profile(&mut self, name: &str) -> Result<(), String> {
        if let Some(profile) = self.profiles.get(name) {
            self.active_profile = profile.clone();
            Ok(())
        } else {
            Err(format!("Profile '{}' not found", name))
        }
    }

    /// Add a new profile
    pub fn add_profile(&mut self, profile: KeyBindingProfile) {
        self.profiles.insert(profile.name.clone(), profile);
    }

    /// Remove a profile
    pub fn remove_profile(&mut self, name: &str) -> Result<(), String> {
        if name == "default" {
            return Err("Cannot remove default profile".to_string());
        }
        if self.active_profile.name == name {
            return Err("Cannot remove active profile".to_string());
        }
        self.profiles
            .remove(name)
            .ok_or_else(|| format!("Profile '{}' not found", name))?;
        Ok(())
    }

    /// Get a profile by name
    pub fn get_profile(&self, name: &str) -> Option<&KeyBindingProfile> {
        self.profiles.get(name)
    }

    /// List all profile names
    pub fn profile_names(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }

    /// Get the action for a key event using the active profile
    pub fn get_action(&self, event: &KeyEvent) -> Option<&Action> {
        self.active_profile.get_action(event)
    }

    /// Serialize all profiles to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let data = serde_json::json!({
            "active_profile": self.active_profile.name,
            "profiles": self.profiles,
        });
        serde_json::to_string_pretty(&data)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, String> {
        let data: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;

        let active_name = data["active_profile"]
            .as_str()
            .ok_or("Missing active_profile")?;

        let profiles_data = data["profiles"].as_object().ok_or("Missing profiles")?;

        let mut profiles = HashMap::new();
        for (name, profile_data) in profiles_data {
            let profile: KeyBindingProfile = serde_json::from_value(profile_data.clone())
                .map_err(|e| format!("Failed to parse profile '{}': {}", name, e))?;
            profiles.insert(name.clone(), profile);
        }

        let active_profile = profiles
            .get(active_name)
            .ok_or_else(|| format!("Active profile '{}' not found", active_name))?
            .clone();

        Ok(Self {
            active_profile,
            profiles,
        })
    }
}

impl Default for KeyBindingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_name() {
        assert_eq!(Action::SwitchTile.name(), "switch_tile");
        assert_eq!(Action::Save.name(), "save");
        assert_eq!(Action::Quit.name(), "quit");
        assert_eq!(Action::Custom("test".to_string()).name(), "test");
    }

    #[test]
    fn test_action_from_name() {
        assert_eq!(Action::from_name("switch_tile"), Some(Action::SwitchTile));
        assert_eq!(Action::from_name("save"), Some(Action::Save));
        assert_eq!(Action::from_name("quit"), Some(Action::Quit));
        assert_eq!(
            Action::from_name("custom_action"),
            Some(Action::Custom("custom_action".to_string()))
        );
    }

    #[test]
    fn test_key_combo_creation() {
        let combo = KeyCombo::new(KeyCode::A, Modifiers::CTRL);
        assert_eq!(combo.code, KeyCode::A);
        assert_eq!(combo.modifiers, Modifiers::CTRL);
    }

    #[test]
    fn test_key_combo_from_event() {
        let event = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);
        let combo = KeyCombo::from_event(&event);
        assert_eq!(combo.code, KeyCode::A);
        assert_eq!(combo.modifiers, Modifiers::CTRL);
    }

    #[test]
    fn test_key_combo_matches() {
        let combo = KeyCombo::new(KeyCode::A, Modifiers::CTRL);
        let event1 = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);
        let event2 = KeyEvent::pressed(KeyCode::B, Modifiers::CTRL);

        assert!(combo.matches(&event1));
        assert!(!combo.matches(&event2));
    }

    #[test]
    fn test_profile_creation() {
        let profile = KeyBindingProfile::new("test".to_string());
        assert_eq!(profile.name, "test");
        assert_eq!(profile.binding_count(), 0);
    }

    #[test]
    fn test_profile_bind() {
        let mut profile = KeyBindingProfile::new("test".to_string());
        let combo = KeyCombo::new(KeyCode::A, Modifiers::CTRL);

        profile.bind(combo, Action::Save);
        assert_eq!(profile.binding_count(), 1);
        assert!(profile.is_bound(&KeyCombo::new(KeyCode::A, Modifiers::CTRL)));
    }

    #[test]
    fn test_profile_unbind() {
        let mut profile = KeyBindingProfile::new("test".to_string());
        let combo = KeyCombo::new(KeyCode::A, Modifiers::CTRL);

        profile.bind(combo.clone(), Action::Save);
        assert_eq!(profile.binding_count(), 1);

        let action = profile.unbind(&combo);
        assert_eq!(action, Some(Action::Save));
        assert_eq!(profile.binding_count(), 0);
    }

    #[test]
    fn test_profile_get_action() {
        let mut profile = KeyBindingProfile::new("test".to_string());
        let combo = KeyCombo::new(KeyCode::A, Modifiers::CTRL);

        profile.bind(combo, Action::Save);

        let event = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);
        let action = profile.get_action(&event);
        assert_eq!(action, Some(&Action::Save));
    }

    #[test]
    fn test_profile_clear() {
        let mut profile = KeyBindingProfile::new("test".to_string());
        profile.bind(KeyCombo::new(KeyCode::A, Modifiers::CTRL), Action::Save);
        profile.bind(KeyCombo::new(KeyCode::B, Modifiers::CTRL), Action::Quit);

        assert_eq!(profile.binding_count(), 2);
        profile.clear();
        assert_eq!(profile.binding_count(), 0);
    }

    #[test]
    fn test_default_profile() {
        let profile = KeyBindingProfile::default_profile();
        assert_eq!(profile.name, "default");
        assert!(profile.binding_count() > 0);

        // Check some default bindings
        let alt_tab = KeyEvent::pressed(KeyCode::Tab, Modifiers::ALT);
        assert_eq!(profile.get_action(&alt_tab), Some(&Action::SwitchTile));

        let ctrl_s = KeyEvent::pressed(KeyCode::S, Modifiers::CTRL);
        assert_eq!(profile.get_action(&ctrl_s), Some(&Action::Save));
    }

    #[test]
    fn test_vim_profile() {
        let profile = KeyBindingProfile::vim_profile();
        assert_eq!(profile.name, "vim");
        assert!(profile.binding_count() > 0);
    }

    #[test]
    fn test_profile_serialization() {
        let mut profile = KeyBindingProfile::new("test".to_string());
        profile.bind(KeyCombo::new(KeyCode::A, Modifiers::CTRL), Action::Save);

        let json = profile.to_json().unwrap();
        let deserialized = KeyBindingProfile::from_json(&json).unwrap();

        assert_eq!(profile.name, deserialized.name);
        assert_eq!(profile.binding_count(), deserialized.binding_count());
    }

    #[test]
    fn test_manager_creation() {
        let manager = KeyBindingManager::new();
        assert_eq!(manager.active_profile().name, "default");
        assert!(manager.profile_names().contains(&"default".to_string()));
        assert!(manager.profile_names().contains(&"vim".to_string()));
    }

    #[test]
    fn test_manager_set_profile() {
        let mut manager = KeyBindingManager::new();
        assert_eq!(manager.active_profile().name, "default");

        manager.set_profile("vim").unwrap();
        assert_eq!(manager.active_profile().name, "vim");

        let result = manager.set_profile("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_add_profile() {
        let mut manager = KeyBindingManager::new();
        let profile = KeyBindingProfile::new("custom".to_string());

        manager.add_profile(profile);
        assert!(manager.profile_names().contains(&"custom".to_string()));
    }

    #[test]
    fn test_manager_remove_profile() {
        let mut manager = KeyBindingManager::new();
        let profile = KeyBindingProfile::new("custom".to_string());
        manager.add_profile(profile);

        manager.remove_profile("custom").unwrap();
        assert!(!manager.profile_names().contains(&"custom".to_string()));

        // Cannot remove default
        let result = manager.remove_profile("default");
        assert!(result.is_err());

        // Cannot remove active profile
        manager.set_profile("vim").unwrap();
        let result = manager.remove_profile("vim");
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_get_action() {
        let manager = KeyBindingManager::new();
        let event = KeyEvent::pressed(KeyCode::S, Modifiers::CTRL);
        let action = manager.get_action(&event);
        assert_eq!(action, Some(&Action::Save));
    }

    #[test]
    fn test_manager_serialization() {
        let manager = KeyBindingManager::new();
        let json = manager.to_json().unwrap();
        let deserialized = KeyBindingManager::from_json(&json).unwrap();

        assert_eq!(
            manager.active_profile().name,
            deserialized.active_profile().name
        );
        assert_eq!(
            manager.profile_names().len(),
            deserialized.profile_names().len()
        );
    }
}
