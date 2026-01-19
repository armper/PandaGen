//! Storage object schema identity types
//!
//! These types enable disciplined evolution of storage object schemas.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifier for a storage object schema type
///
/// Unlike ad-hoc structure inference, this explicitly names the schema.
/// Examples: "user-profile", "audit-event", "config-v2"
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectSchemaId(String);

impl ObjectSchemaId {
    /// Creates a new object schema ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the schema ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectSchemaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Schema({})", self.0)
    }
}

impl From<String> for ObjectSchemaId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ObjectSchemaId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Version number for a storage object schema
///
/// Unlike IPC SchemaVersion (which has major.minor), this is a simple
/// monotonic version number for storage objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObjectSchemaVersion(u32);

impl ObjectSchemaVersion {
    /// Creates a new schema version
    pub const fn new(version: u32) -> Self {
        Self(version)
    }

    /// Returns the version number
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Returns the next version
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for ObjectSchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl Default for ObjectSchemaVersion {
    fn default() -> Self {
        Self(1)
    }
}

/// Migration path from one schema version to another
///
/// Optional metadata to track how an object was migrated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationLineage {
    /// Original schema version
    pub from_version: ObjectSchemaVersion,
    /// Target schema version
    pub to_version: ObjectSchemaVersion,
    /// Optional timestamp when migration occurred
    pub migrated_at: Option<u64>,
}

impl MigrationLineage {
    /// Creates a new migration lineage
    pub fn new(from: ObjectSchemaVersion, to: ObjectSchemaVersion) -> Self {
        Self {
            from_version: from,
            to_version: to,
            migrated_at: None,
        }
    }

    /// Sets the migration timestamp
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.migrated_at = Some(timestamp);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_schema_id_creation() {
        let schema1 = ObjectSchemaId::new("user-profile");
        let schema2 = ObjectSchemaId::new("user-profile".to_string());
        assert_eq!(schema1, schema2);
        assert_eq!(schema1.as_str(), "user-profile");
    }

    #[test]
    fn test_object_schema_id_from() {
        let schema1: ObjectSchemaId = "test-schema".into();
        let schema2: ObjectSchemaId = "test-schema".to_string().into();
        assert_eq!(schema1, schema2);
    }

    #[test]
    fn test_object_schema_id_display() {
        let schema = ObjectSchemaId::new("audit-event");
        assert_eq!(format!("{}", schema), "Schema(audit-event)");
    }

    #[test]
    fn test_object_schema_version_creation() {
        let v1 = ObjectSchemaVersion::new(1);
        let v2 = ObjectSchemaVersion::new(2);
        assert_eq!(v1.as_u32(), 1);
        assert_eq!(v2.as_u32(), 2);
    }

    #[test]
    fn test_object_schema_version_ordering() {
        let v1 = ObjectSchemaVersion::new(1);
        let v2 = ObjectSchemaVersion::new(2);
        let v3 = ObjectSchemaVersion::new(3);

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v1 < v3);
        assert_eq!(v1, v1);
    }

    #[test]
    fn test_object_schema_version_next() {
        let v1 = ObjectSchemaVersion::new(1);
        let v2 = v1.next();
        let v3 = v2.next();

        assert_eq!(v2.as_u32(), 2);
        assert_eq!(v3.as_u32(), 3);
    }

    #[test]
    fn test_object_schema_version_default() {
        let default_version = ObjectSchemaVersion::default();
        assert_eq!(default_version.as_u32(), 1);
    }

    #[test]
    fn test_object_schema_version_display() {
        let v = ObjectSchemaVersion::new(42);
        assert_eq!(format!("{}", v), "v42");
    }

    #[test]
    fn test_migration_lineage_creation() {
        let lineage =
            MigrationLineage::new(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(2));

        assert_eq!(lineage.from_version, ObjectSchemaVersion::new(1));
        assert_eq!(lineage.to_version, ObjectSchemaVersion::new(2));
        assert_eq!(lineage.migrated_at, None);
    }

    #[test]
    fn test_migration_lineage_with_timestamp() {
        let lineage =
            MigrationLineage::new(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(2))
                .with_timestamp(1234567890);

        assert_eq!(lineage.migrated_at, Some(1234567890));
    }

    #[test]
    fn test_serialization() {
        let schema_id = ObjectSchemaId::new("test-schema");
        let version = ObjectSchemaVersion::new(5);
        let lineage =
            MigrationLineage::new(ObjectSchemaVersion::new(1), ObjectSchemaVersion::new(5));

        // Test that these types can be serialized/deserialized
        let schema_json = serde_json::to_string(&schema_id).unwrap();
        let version_json = serde_json::to_string(&version).unwrap();
        let lineage_json = serde_json::to_string(&lineage).unwrap();

        let schema_parsed: ObjectSchemaId = serde_json::from_str(&schema_json).unwrap();
        let version_parsed: ObjectSchemaVersion = serde_json::from_str(&version_json).unwrap();
        let lineage_parsed: MigrationLineage = serde_json::from_str(&lineage_json).unwrap();

        assert_eq!(schema_id, schema_parsed);
        assert_eq!(version, version_parsed);
        assert_eq!(lineage, lineage_parsed);
    }
}
