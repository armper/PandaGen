//! Settings persistence layer
//!
//! This module handles loading and saving settings overrides to storage.
//! All operations are deterministic, capability-scoped, and safe against corruption.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::{SettingKey, SettingValue, UserId};
use serde::{Deserialize, Serialize};

/// Serializable container for settings overrides
/// Uses BTreeMap for stable ordering
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsOverridesData {
    /// Version of the settings format (for future migrations)
    pub version: u32,
    /// User-specific overrides (stable ordering via BTreeMap)
    pub user_overrides: BTreeMap<UserId, BTreeMap<String, SettingValue>>,
}

impl SettingsOverridesData {
    /// Current version of the settings format
    pub const CURRENT_VERSION: u32 = 1;

    /// Creates a new empty settings data
    pub fn new() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            user_overrides: BTreeMap::new(),
        }
    }

    /// Creates settings data from user overrides
    pub fn from_overrides(
        user_overrides: &BTreeMap<UserId, BTreeMap<SettingKey, SettingValue>>,
    ) -> Self {
        // Convert SettingKey to String for serialization (stable ordering maintained)
        let mut data = Self::new();
        for (user_id, settings) in user_overrides {
            let mut user_settings = BTreeMap::new();
            for (key, value) in settings {
                user_settings.insert(key.as_str().to_string(), value.clone());
            }
            data.user_overrides.insert(user_id.clone(), user_settings);
        }
        data
    }

    /// Converts settings data to user overrides
    pub fn to_overrides(&self) -> BTreeMap<UserId, BTreeMap<SettingKey, SettingValue>> {
        let mut overrides = BTreeMap::new();
        for (user_id, settings) in &self.user_overrides {
            let mut user_settings = BTreeMap::new();
            for (key, value) in settings {
                user_settings.insert(SettingKey::new(key.as_str()), value.clone());
            }
            overrides.insert(user_id.clone(), user_settings);
        }
        overrides
    }
}

impl Default for SettingsOverridesData {
    fn default() -> Self {
        Self::new()
    }
}

/// Result type for persistence operations
pub type PersistenceResult<T> = Result<T, PersistenceError>;

/// Errors that can occur during persistence operations
#[derive(Debug, Clone, PartialEq)]
pub enum PersistenceError {
    /// Failed to serialize settings
    SerializationFailed(String),
    /// Failed to deserialize settings
    DeserializationFailed(String),
    /// Unsupported settings version
    UnsupportedVersion(u32),
}

impl core::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PersistenceError::SerializationFailed(msg) => {
                write!(f, "Failed to serialize settings: {}", msg)
            }
            PersistenceError::DeserializationFailed(msg) => {
                write!(f, "Failed to deserialize settings: {}", msg)
            }
            PersistenceError::UnsupportedVersion(version) => {
                write!(f, "Unsupported settings version: {}", version)
            }
        }
    }
}

/// Serializes settings overrides to JSON bytes
pub fn serialize_overrides(data: &SettingsOverridesData) -> PersistenceResult<Vec<u8>> {
    serde_json::to_vec_pretty(data)
        .map_err(|e| PersistenceError::SerializationFailed(e.to_string()))
}

/// Deserializes settings overrides from JSON bytes
pub fn deserialize_overrides(bytes: &[u8]) -> PersistenceResult<SettingsOverridesData> {
    let data: SettingsOverridesData = serde_json::from_slice(bytes)
        .map_err(|e| PersistenceError::DeserializationFailed(e.to_string()))?;

    // Check version compatibility
    if data.version != SettingsOverridesData::CURRENT_VERSION {
        return Err(PersistenceError::UnsupportedVersion(data.version));
    }

    Ok(data)
}

/// Attempts to load settings from bytes, falling back to defaults on error
pub fn load_overrides_safe(bytes: &[u8]) -> SettingsOverridesData {
    deserialize_overrides(bytes).unwrap_or_else(|_| SettingsOverridesData::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_data_creation() {
        let data = SettingsOverridesData::new();
        assert_eq!(data.version, SettingsOverridesData::CURRENT_VERSION);
        assert_eq!(data.user_overrides.len(), 0);
    }

    #[test]
    fn test_from_overrides() {
        let mut user_overrides = BTreeMap::new();
        let mut user1_settings = BTreeMap::new();
        user1_settings.insert(
            SettingKey::new("editor.tab_size"),
            SettingValue::Integer(2),
        );
        user_overrides.insert("user1".to_string(), user1_settings);

        let data = SettingsOverridesData::from_overrides(&user_overrides);
        assert_eq!(data.user_overrides.len(), 1);
        assert!(data.user_overrides.contains_key("user1"));
        assert_eq!(
            data.user_overrides.get("user1").unwrap().get("editor.tab_size"),
            Some(&SettingValue::Integer(2))
        );
    }

    #[test]
    fn test_to_overrides() {
        let mut data = SettingsOverridesData::new();
        let mut user1_settings = BTreeMap::new();
        user1_settings.insert("editor.tab_size".to_string(), SettingValue::Integer(2));
        data.user_overrides.insert("user1".to_string(), user1_settings);

        let overrides = data.to_overrides();
        assert_eq!(overrides.len(), 1);
        assert!(overrides.contains_key("user1"));
        assert_eq!(
            overrides
                .get("user1")
                .unwrap()
                .get(&SettingKey::new("editor.tab_size")),
            Some(&SettingValue::Integer(2))
        );
    }

    #[test]
    fn test_roundtrip_conversion() {
        let mut user_overrides = BTreeMap::new();
        let mut user1_settings = BTreeMap::new();
        user1_settings.insert(
            SettingKey::new("editor.tab_size"),
            SettingValue::Integer(2),
        );
        user1_settings.insert(
            SettingKey::new("editor.use_spaces"),
            SettingValue::Boolean(false),
        );
        user_overrides.insert("user1".to_string(), user1_settings);

        let data = SettingsOverridesData::from_overrides(&user_overrides);
        let roundtrip = data.to_overrides();

        assert_eq!(user_overrides, roundtrip);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut data = SettingsOverridesData::new();
        let mut user1_settings = BTreeMap::new();
        user1_settings.insert("editor.tab_size".to_string(), SettingValue::Integer(2));
        data.user_overrides.insert("user1".to_string(), user1_settings);

        let bytes = serialize_overrides(&data).unwrap();
        let deserialized = deserialize_overrides(&bytes).unwrap();

        assert_eq!(data, deserialized);
    }

    #[test]
    fn test_deterministic_serialization() {
        // Create data with multiple keys (BTreeMap ensures stable order)
        let mut data = SettingsOverridesData::new();
        let mut user1_settings = BTreeMap::new();
        user1_settings.insert("zzz.last".to_string(), SettingValue::Integer(1));
        user1_settings.insert("aaa.first".to_string(), SettingValue::Integer(2));
        user1_settings.insert("mmm.middle".to_string(), SettingValue::Integer(3));
        data.user_overrides.insert("user1".to_string(), user1_settings);

        // Serialize twice
        let bytes1 = serialize_overrides(&data).unwrap();
        let bytes2 = serialize_overrides(&data).unwrap();

        // Should be identical
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_deserialize_invalid_json() {
        let invalid_json = b"{ invalid json }";
        let result = deserialize_overrides(invalid_json);
        assert!(result.is_err());
        match result {
            Err(PersistenceError::DeserializationFailed(_)) => {}
            _ => panic!("Expected DeserializationFailed error"),
        }
    }

    #[test]
    fn test_deserialize_unsupported_version() {
        let json = r#"{
            "version": 999,
            "user_overrides": {}
        }"#;
        let result = deserialize_overrides(json.as_bytes());
        assert!(result.is_err());
        match result {
            Err(PersistenceError::UnsupportedVersion(999)) => {}
            _ => panic!("Expected UnsupportedVersion error"),
        }
    }

    #[test]
    fn test_load_overrides_safe_with_valid_data() {
        let mut data = SettingsOverridesData::new();
        let mut user1_settings = BTreeMap::new();
        user1_settings.insert("test.key".to_string(), SettingValue::Integer(42));
        data.user_overrides.insert("user1".to_string(), user1_settings);

        let bytes = serialize_overrides(&data).unwrap();
        let loaded = load_overrides_safe(&bytes);

        assert_eq!(loaded, data);
    }

    #[test]
    fn test_load_overrides_safe_with_invalid_data() {
        let invalid_json = b"{ invalid json }";
        let loaded = load_overrides_safe(invalid_json);

        // Should fall back to empty defaults
        assert_eq!(loaded, SettingsOverridesData::new());
    }

    #[test]
    fn test_stable_key_ordering_in_json() {
        let mut data = SettingsOverridesData::new();
        let mut settings = BTreeMap::new();
        settings.insert("z_key".to_string(), SettingValue::Integer(1));
        settings.insert("a_key".to_string(), SettingValue::Integer(2));
        settings.insert("m_key".to_string(), SettingValue::Integer(3));
        data.user_overrides.insert("user1".to_string(), settings);

        let bytes = serialize_overrides(&data).unwrap();
        let json_str = core::str::from_utf8(&bytes).unwrap();

        // Keys should appear in alphabetical order in the JSON
        let a_pos = json_str.find("a_key").unwrap();
        let m_pos = json_str.find("m_key").unwrap();
        let z_pos = json_str.find("z_key").unwrap();

        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }
}
