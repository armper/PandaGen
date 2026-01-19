//! Storage service contract tests
//!
//! These tests define the stable contract for the Storage service.

use ipc::SchemaVersion;
use serde::{Deserialize, Serialize};

// ===== Storage Contract Version =====
#[allow(dead_code)]
const STORAGE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// ===== Action Identifiers =====
#[allow(dead_code)]
const ACTION_CREATE_OBJECT: &str = "storage.create_object";
#[allow(dead_code)]
const ACTION_READ_OBJECT: &str = "storage.read_object";
#[allow(dead_code)]
const ACTION_WRITE_OBJECT: &str = "storage.write_object";
#[allow(dead_code)]
const ACTION_DELETE_OBJECT: &str = "storage.delete_object";
#[allow(dead_code)]
const ACTION_LIST_VERSIONS: &str = "storage.list_versions";

// ===== Canonical Payload Structures =====

/// Object kind enum (stable contract)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ObjectKind {
    Blob,
    Log,
    Map,
}

/// Create object request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateObjectRequest {
    pub kind: ObjectKind,
    #[serde(default)]
    pub metadata: Vec<(String, String)>,
}

/// Create object response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateObjectResponse {
    pub object_id: String, // Simplified for contract test
    pub version_id: String,
}

/// Read object request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadObjectRequest {
    pub object_id: String,
    #[serde(default)]
    pub version_id: Option<String>,
}

/// Read object response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadObjectResponse {
    pub object_id: String,
    pub version_id: String,
    pub kind: ObjectKind,
    pub data: Vec<u8>,
}

/// Write object request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WriteObjectRequest {
    pub object_id: String,
    pub data: Vec<u8>,
}

/// Write object response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WriteObjectResponse {
    pub version_id: String,
}

/// Delete object request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeleteObjectRequest {
    pub object_id: String,
}

/// List versions request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListVersionsRequest {
    pub object_id: String,
}

/// List versions response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListVersionsResponse {
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionInfo {
    pub version_id: String,
    pub timestamp: u64,
}

// ===== Contract Tests =====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use core_types::ServiceId;

    #[test]
    fn test_create_object_contract() {
        let service_id = ServiceId::new();
        let request = CreateObjectRequest {
            kind: ObjectKind::Blob,
            metadata: vec![("key".to_string(), "value".to_string())],
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_CREATE_OBJECT,
            STORAGE_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_CREATE_OBJECT, STORAGE_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: CreateObjectRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_read_object_contract() {
        let service_id = ServiceId::new();
        let request = ReadObjectRequest {
            object_id: "test-object".to_string(),
            version_id: Some("v1".to_string()),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_READ_OBJECT,
            STORAGE_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_READ_OBJECT, STORAGE_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: ReadObjectRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_write_object_contract() {
        let service_id = ServiceId::new();
        let request = WriteObjectRequest {
            object_id: "test-object".to_string(),
            data: vec![1, 2, 3, 4],
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_WRITE_OBJECT,
            STORAGE_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_WRITE_OBJECT, STORAGE_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: WriteObjectRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_delete_object_contract() {
        let service_id = ServiceId::new();
        let request = DeleteObjectRequest {
            object_id: "test-object".to_string(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_DELETE_OBJECT,
            STORAGE_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_DELETE_OBJECT, STORAGE_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: DeleteObjectRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_list_versions_contract() {
        let service_id = ServiceId::new();
        let request = ListVersionsRequest {
            object_id: "test-object".to_string(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_LIST_VERSIONS,
            STORAGE_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_LIST_VERSIONS, STORAGE_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: ListVersionsRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_object_kind_enum_is_stable() {
        // These variants MUST NOT CHANGE without version bump
        let blob = ObjectKind::Blob;
        let log = ObjectKind::Log;
        let map = ObjectKind::Map;

        // Verify serialization is stable
        assert_eq!(serde_json::to_string(&blob).unwrap(), r#""Blob""#);
        assert_eq!(serde_json::to_string(&log).unwrap(), r#""Log""#);
        assert_eq!(serde_json::to_string(&map).unwrap(), r#""Map""#);
    }

    #[test]
    fn test_action_identifiers_are_stable() {
        assert_eq!(ACTION_CREATE_OBJECT, "storage.create_object");
        assert_eq!(ACTION_READ_OBJECT, "storage.read_object");
        assert_eq!(ACTION_WRITE_OBJECT, "storage.write_object");
        assert_eq!(ACTION_DELETE_OBJECT, "storage.delete_object");
        assert_eq!(ACTION_LIST_VERSIONS, "storage.list_versions");
    }

    #[test]
    fn test_schema_version_is_stable() {
        assert_eq!(STORAGE_SCHEMA_VERSION.major, 1);
        assert_eq!(STORAGE_SCHEMA_VERSION.minor, 0);
    }
}
