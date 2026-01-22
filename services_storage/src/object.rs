//! Object types and identifiers

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::permissions::Ownership;

/// Unique identifier for a storage object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(Uuid);

impl ObjectId {
    /// Creates a new random object ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates an object ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ObjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Object({})", self.0)
    }
}

/// Unique identifier for a specific version of an object
///
/// Unlike traditional filesystems where "versions" are external (git, etc.),
/// every object modification creates a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VersionId(Uuid);

impl VersionId {
    /// Creates a new random version ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a version ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for VersionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Version({})", self.0)
    }
}

/// Different kinds of storage objects
///
/// Unlike a traditional filesystem (where everything is bytes),
/// we distinguish different object types for better semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
    /// Immutable blob of bytes (like S3 object)
    ///
    /// Use for: binary data, images, archives
    Blob,

    /// Append-only log (like Kafka topic)
    ///
    /// Use for: event streams, audit logs, time-series
    Log,

    /// Key-value map (like Redis hash)
    ///
    /// Use for: structured data, configuration, indexes
    Map,
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectKind::Blob => write!(f, "Blob"),
            ObjectKind::Log => write!(f, "Log"),
            ObjectKind::Map => write!(f, "Map"),
        }
    }
}

/// A storage object (scaffold only)
///
/// In a real system, this would contain the actual data or a handle to it.
#[derive(Debug, Clone)]
pub struct Object {
    /// Unique identifier
    pub id: ObjectId,
    /// Current version
    pub version: VersionId,
    /// Object kind
    pub kind: ObjectKind,
    /// Schema identifier (what type of data this object contains)
    pub schema_id: Option<core_types::ObjectSchemaId>,
    /// Schema version (which version of the schema)
    pub schema_version: Option<core_types::ObjectSchemaVersion>,
    /// Optional migration lineage
    pub migration_lineage: Option<core_types::MigrationLineage>,
    /// Metadata (tags, timestamps, etc.)
    pub metadata: Vec<(String, String)>,
    /// Ownership information
    pub ownership: Option<Ownership>,
}

impl Object {
    /// Creates a new object
    pub fn new(kind: ObjectKind) -> Self {
        Self {
            id: ObjectId::new(),
            version: VersionId::new(),
            kind,
            schema_id: None,
            schema_version: None,
            migration_lineage: None,
            metadata: Vec::new(),
            ownership: None,
        }
    }

    /// Sets the schema identity for this object
    pub fn with_schema(
        mut self,
        schema_id: core_types::ObjectSchemaId,
        schema_version: core_types::ObjectSchemaVersion,
    ) -> Self {
        self.schema_id = Some(schema_id);
        self.schema_version = Some(schema_version);
        self
    }

    /// Sets migration lineage for this object
    pub fn with_migration_lineage(mut self, lineage: core_types::MigrationLineage) -> Self {
        self.migration_lineage = Some(lineage);
        self
    }

    /// Adds metadata to the object
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.push((key, value));
        self
    }

    /// Sets ownership for this object
    pub fn with_ownership(mut self, ownership: Ownership) -> Self {
        self.ownership = Some(ownership);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    #[test]
    fn test_object_id_creation() {
        let id1 = ObjectId::new();
        let id2 = ObjectId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_version_id_creation() {
        let v1 = VersionId::new();
        let v2 = VersionId::new();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_object_kinds() {
        assert_eq!(format!("{}", ObjectKind::Blob), "Blob");
        assert_eq!(format!("{}", ObjectKind::Log), "Log");
        assert_eq!(format!("{}", ObjectKind::Map), "Map");
    }

    #[test]
    fn test_object_creation() {
        let obj = Object::new(ObjectKind::Blob);
        assert_eq!(obj.kind, ObjectKind::Blob);
        assert!(obj.metadata.is_empty());
    }

    #[test]
    fn test_object_with_metadata() {
        let obj = Object::new(ObjectKind::Blob)
            .with_metadata("author".to_string(), "alice".to_string())
            .with_metadata("size".to_string(), "1024".to_string());

        assert_eq!(obj.metadata.len(), 2);
        assert_eq!(obj.metadata[0].0, "author");
        assert_eq!(obj.metadata[0].1, "alice");
    }
}
