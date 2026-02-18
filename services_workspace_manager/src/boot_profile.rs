//! # Boot Profiles
//!
//! This module manages boot profiles - what PandaGen does on startup.
//! Boot straight into vi, start in workspace mode, or run as a kiosk.

use serde::{Deserialize, Serialize};
use services_storage::{JournaledStorage, ObjectId, TransactionError, TransactionalStorage};
use uuid::Uuid;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

const BOOT_PROFILE_OBJECT_UUID: u128 = 0x6b2a_f5a8_0dc3_4da0_82f8_f3d3_b20f_4ec3;

/// Boot profile types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BootProfile {
    /// Boot into workspace manager (default)
    ///
    /// User gets command prompt, can launch services/editors
    #[default]
    Workspace,

    /// Boot straight into vi editor
    ///
    /// Like a feral 1993 UNIX box - power on, edit, save, reboot
    Editor,

    /// Boot into kiosk mode
    ///
    /// Single-app mode, no shell access, locked down
    Kiosk,
}

impl BootProfile {
    /// Returns human-readable profile name
    pub fn name(&self) -> &'static str {
        match self {
            BootProfile::Workspace => "Workspace",
            BootProfile::Editor => "Editor",
            BootProfile::Kiosk => "Kiosk",
        }
    }

    /// Returns profile description
    pub fn description(&self) -> &'static str {
        match self {
            BootProfile::Workspace => {
                "Interactive workspace with command prompt and service management"
            }
            BootProfile::Editor => "Boot straight into vi editor (power-on editing)",
            BootProfile::Kiosk => "Single-app kiosk mode (locked down, no shell)",
        }
    }

    /// Parses profile from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "workspace" => Some(BootProfile::Workspace),
            "editor" => Some(BootProfile::Editor),
            "kiosk" => Some(BootProfile::Kiosk),
            _ => None,
        }
    }
}

/// Boot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootConfig {
    /// Active boot profile
    pub profile: BootProfile,

    /// Auto-start services (service names)
    pub auto_start: Vec<String>,

    /// Initial file to open (for Editor profile)
    pub editor_file: Option<String>,

    /// Kiosk app to run (for Kiosk profile)
    pub kiosk_app: Option<String>,

    /// Additional configuration (key-value pairs)
    pub extra: HashMap<String, String>,
}

impl BootConfig {
    /// Creates a new boot config with given profile
    pub fn new(profile: BootProfile) -> Self {
        Self {
            profile,
            auto_start: Vec::new(),
            editor_file: None,
            kiosk_app: None,
            extra: HashMap::new(),
        }
    }

    /// Creates workspace profile config
    pub fn workspace() -> Self {
        Self::new(BootProfile::Workspace)
            .with_auto_start(vec!["logger".to_string(), "storage".to_string()])
    }

    /// Creates editor profile config
    pub fn editor() -> Self {
        Self::new(BootProfile::Editor).with_editor_file("/tmp/scratch.txt".to_string())
    }

    /// Creates kiosk profile config
    pub fn kiosk() -> Self {
        Self::new(BootProfile::Kiosk).with_kiosk_app("demo-app".to_string())
    }

    /// Adds auto-start services
    pub fn with_auto_start(mut self, services: Vec<String>) -> Self {
        self.auto_start = services;
        self
    }

    /// Sets editor file
    pub fn with_editor_file(mut self, file: String) -> Self {
        self.editor_file = Some(file);
        self
    }

    /// Sets kiosk app
    pub fn with_kiosk_app(mut self, app: String) -> Self {
        self.kiosk_app = Some(app);
        self
    }

    /// Adds extra configuration
    pub fn with_extra(mut self, key: String, value: String) -> Self {
        self.extra.insert(key, value);
        self
    }

    /// Serializes to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Default for BootConfig {
    fn default() -> Self {
        Self::workspace()
    }
}

/// Boot profile manager
pub struct BootProfileManager {
    /// Current configuration
    config: BootConfig,

    /// Whether config has been loaded from storage
    loaded: bool,
}

impl BootProfileManager {
    /// Creates a new boot profile manager
    pub fn new() -> Self {
        Self {
            config: BootConfig::default(),
            loaded: false,
        }
    }

    /// Loads configuration (from storage in real impl)
    pub fn load(&mut self, storage_handle: Option<&mut JournaledStorage>) -> Result<(), String> {
        let Some(storage) = storage_handle else {
            self.config = BootConfig::default();
            self.loaded = true;
            return Ok(());
        };

        let object_id = Self::boot_profile_object_id();
        let mut tx = storage
            .begin_transaction()
            .map_err(|e| format!("Failed to begin boot profile load transaction: {}", e))?;

        let bytes = match storage.read_data(&tx, object_id) {
            Ok(bytes) => bytes,
            Err(TransactionError::ObjectNotFound(_)) => {
                let _ = storage.rollback(&mut tx);
                self.config = BootConfig::default();
                self.loaded = true;
                return Ok(());
            }
            Err(err) => {
                let _ = storage.rollback(&mut tx);
                return Err(format!("Failed to read boot profile from storage: {}", err));
            }
        };
        let _ = storage.rollback(&mut tx);

        self.config = String::from_utf8(bytes)
            .ok()
            .and_then(|json| BootConfig::from_json(&json).ok())
            .unwrap_or_default();
        self.loaded = true;
        Ok(())
    }

    /// Saves configuration (to storage in real impl)
    pub fn save(&self, storage_handle: Option<&mut JournaledStorage>) -> Result<(), String> {
        let Some(storage) = storage_handle else {
            return Ok(());
        };

        let object_id = Self::boot_profile_object_id();
        let json = self
            .config
            .to_json()
            .map_err(|e| format!("Failed to serialize boot profile: {}", e))?;

        let mut tx = storage
            .begin_transaction()
            .map_err(|e| format!("Failed to begin boot profile save transaction: {}", e))?;
        storage
            .write(&mut tx, object_id, json.as_bytes())
            .map_err(|e| format!("Failed to write boot profile to storage: {}", e))?;
        storage
            .commit(&mut tx)
            .map_err(|e| format!("Failed to commit boot profile save transaction: {}", e))?;

        Ok(())
    }

    /// Gets current boot configuration
    pub fn config(&self) -> &BootConfig {
        &self.config
    }

    /// Updates boot configuration
    pub fn set_config(&mut self, config: BootConfig) {
        self.config = config;
    }

    /// Changes boot profile
    pub fn set_profile(&mut self, profile: BootProfile) {
        self.config.profile = profile;
    }

    /// Checks if config was loaded from storage
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Gets the boot profile
    pub fn profile(&self) -> BootProfile {
        self.config.profile
    }

    fn boot_profile_object_id() -> ObjectId {
        ObjectId::from_uuid(Uuid::from_u128(BOOT_PROFILE_OBJECT_UUID))
    }
}

impl Default for BootProfileManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use services_storage::{JournaledStorage, TransactionalStorage};

    #[test]
    fn test_boot_profile_name() {
        assert_eq!(BootProfile::Workspace.name(), "Workspace");
        assert_eq!(BootProfile::Editor.name(), "Editor");
        assert_eq!(BootProfile::Kiosk.name(), "Kiosk");
    }

    #[test]
    fn test_boot_profile_from_str() {
        assert_eq!(
            BootProfile::parse("workspace"),
            Some(BootProfile::Workspace)
        );
        assert_eq!(BootProfile::parse("editor"), Some(BootProfile::Editor));
        assert_eq!(BootProfile::parse("kiosk"), Some(BootProfile::Kiosk));
        assert_eq!(
            BootProfile::parse("WORKSPACE"),
            Some(BootProfile::Workspace)
        );
        assert_eq!(BootProfile::parse("invalid"), None);
    }

    #[test]
    fn test_boot_profile_default() {
        assert_eq!(BootProfile::default(), BootProfile::Workspace);
    }

    #[test]
    fn test_boot_config_creation() {
        let config = BootConfig::new(BootProfile::Editor);
        assert_eq!(config.profile, BootProfile::Editor);
        assert!(config.auto_start.is_empty());
        assert!(config.editor_file.is_none());
        assert!(config.kiosk_app.is_none());
    }

    #[test]
    fn test_boot_config_workspace() {
        let config = BootConfig::workspace();
        assert_eq!(config.profile, BootProfile::Workspace);
        assert!(!config.auto_start.is_empty());
        assert!(config.auto_start.contains(&"logger".to_string()));
    }

    #[test]
    fn test_boot_config_editor() {
        let config = BootConfig::editor();
        assert_eq!(config.profile, BootProfile::Editor);
        assert!(config.editor_file.is_some());
    }

    #[test]
    fn test_boot_config_kiosk() {
        let config = BootConfig::kiosk();
        assert_eq!(config.profile, BootProfile::Kiosk);
        assert!(config.kiosk_app.is_some());
    }

    #[test]
    fn test_boot_config_with_auto_start() {
        let config = BootConfig::new(BootProfile::Workspace)
            .with_auto_start(vec!["service1".to_string(), "service2".to_string()]);

        assert_eq!(config.auto_start.len(), 2);
        assert_eq!(config.auto_start[0], "service1");
    }

    #[test]
    fn test_boot_config_with_editor_file() {
        let config =
            BootConfig::new(BootProfile::Editor).with_editor_file("/path/to/file.txt".to_string());

        assert_eq!(config.editor_file, Some("/path/to/file.txt".to_string()));
    }

    #[test]
    fn test_boot_config_with_kiosk_app() {
        let config = BootConfig::new(BootProfile::Kiosk).with_kiosk_app("my-app".to_string());

        assert_eq!(config.kiosk_app, Some("my-app".to_string()));
    }

    #[test]
    fn test_boot_config_with_extra() {
        let config = BootConfig::new(BootProfile::Workspace)
            .with_extra("key1".to_string(), "value1".to_string())
            .with_extra("key2".to_string(), "value2".to_string());

        assert_eq!(config.extra.len(), 2);
        assert_eq!(config.extra.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_boot_config_serialization() {
        let config = BootConfig::workspace();
        let json = config.to_json().unwrap();

        assert!(json.contains("Workspace"));
        assert!(json.contains("logger"));

        let deserialized = BootConfig::from_json(&json).unwrap();
        assert_eq!(deserialized.profile, BootProfile::Workspace);
        assert!(!deserialized.auto_start.is_empty());
    }

    #[test]
    fn test_boot_profile_manager_creation() {
        let manager = BootProfileManager::new();
        assert_eq!(manager.profile(), BootProfile::Workspace);
        assert!(!manager.is_loaded());
    }

    #[test]
    fn test_boot_profile_manager_load() {
        let mut manager = BootProfileManager::new();
        assert!(manager.load(None).is_ok());
        assert!(manager.is_loaded());
    }

    #[test]
    fn test_boot_profile_manager_save_and_load_roundtrip() {
        let mut storage = JournaledStorage::new();
        let config = BootConfig::new(BootProfile::Kiosk)
            .with_kiosk_app("browser".to_string())
            .with_auto_start(vec!["network".to_string(), "input".to_string()])
            .with_extra("theme".to_string(), "amber".to_string());

        let mut manager = BootProfileManager::new();
        manager.set_config(config);
        manager.save(Some(&mut storage)).unwrap();

        let mut loaded = BootProfileManager::new();
        loaded.load(Some(&mut storage)).unwrap();

        assert!(loaded.is_loaded());
        assert_eq!(loaded.profile(), BootProfile::Kiosk);
        assert_eq!(loaded.config().kiosk_app.as_deref(), Some("browser"));
        assert_eq!(
            loaded.config().auto_start,
            vec!["network".to_string(), "input".to_string()]
        );
        assert_eq!(
            loaded.config().extra.get("theme"),
            Some(&"amber".to_string())
        );
    }

    #[test]
    fn test_boot_profile_manager_load_corrupt_data_falls_back_to_default() {
        let mut storage = JournaledStorage::new();
        let mut tx = storage.begin_transaction().unwrap();
        storage
            .write(
                &mut tx,
                BootProfileManager::boot_profile_object_id(),
                b"{ this is not valid json ",
            )
            .unwrap();
        storage.commit(&mut tx).unwrap();

        let mut manager = BootProfileManager::new();
        manager.set_profile(BootProfile::Kiosk);
        manager.load(Some(&mut storage)).unwrap();

        assert!(manager.is_loaded());
        assert_eq!(manager.profile(), BootProfile::Workspace);
    }

    #[test]
    fn test_boot_profile_manager_set_config() {
        let mut manager = BootProfileManager::new();
        let config = BootConfig::editor();

        manager.set_config(config);
        assert_eq!(manager.profile(), BootProfile::Editor);
    }

    #[test]
    fn test_boot_profile_manager_set_profile() {
        let mut manager = BootProfileManager::new();
        manager.set_profile(BootProfile::Kiosk);

        assert_eq!(manager.profile(), BootProfile::Kiosk);
    }
}
