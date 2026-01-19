//! Message types and envelope structure

use core_types::{ServiceId, TaskId};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Creates a new random message ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a message ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Msg({})", self.0)
    }
}

/// Schema version for message payload
///
/// This enables backward-compatible evolution of message formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// Major version (breaking changes)
    pub major: u32,
    /// Minor version (backward-compatible additions)
    pub minor: u32,
}

impl SchemaVersion {
    /// Creates a new schema version
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Checks if this version is compatible with another
    ///
    /// Compatibility rules:
    /// - Same major version = compatible
    /// - Different major version = incompatible
    pub fn is_compatible_with(&self, other: &SchemaVersion) -> bool {
        self.major == other.major
    }

    /// Checks if this version is older than another
    pub fn is_older_than(&self, other: &SchemaVersion) -> bool {
        self.major < other.major || (self.major == other.major && self.minor < other.minor)
    }

    /// Checks if this version is newer than another
    pub fn is_newer_than(&self, other: &SchemaVersion) -> bool {
        self.major > other.major || (self.major == other.major && self.minor > other.minor)
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}", self.major, self.minor)
    }
}

/// Compatibility result for version checking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Versions are compatible
    Compatible,
    /// Sender version is too old, upgrade required
    UpgradeRequired,
    /// Version is not supported (too new or too old outside window)
    Unsupported,
}

/// Version policy for a service
///
/// Defines which schema versions a service accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionPolicy {
    /// Current version the service implements
    current: SchemaVersion,
    /// Minimum major version supported (for N-1 compatibility)
    min_major: u32,
}

impl VersionPolicy {
    /// Creates a version policy for the current version
    pub const fn current(major: u32, minor: u32) -> Self {
        Self {
            current: SchemaVersion::new(major, minor),
            min_major: major,
        }
    }

    /// Sets the minimum supported major version
    ///
    /// Example: If current is v3.0 and min_major is 2,
    /// the policy accepts v2.x and v3.x, rejects v1.x and v4.x
    pub const fn with_min_major(mut self, min_major: u32) -> Self {
        self.min_major = min_major;
        self
    }

    /// Checks if an incoming schema version is compatible
    pub fn check_compatibility(&self, incoming: &SchemaVersion) -> Compatibility {
        // Reject versions newer than current
        if incoming.major > self.current.major {
            return Compatibility::Unsupported;
        }

        // Reject versions older than minimum
        if incoming.major < self.min_major {
            return Compatibility::UpgradeRequired;
        }

        // Within supported range
        Compatibility::Compatible
    }

    /// Returns the current version
    pub fn current_version(&self) -> SchemaVersion {
        self.current
    }

    /// Returns the minimum supported version
    pub fn min_version(&self) -> SchemaVersion {
        SchemaVersion::new(self.min_major, 0)
    }
}

/// Error when schema versions don't match
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaMismatchError {
    /// Sender is using too old a version and must upgrade
    UpgradeRequired {
        service: ServiceId,
        expected_min: SchemaVersion,
        received: SchemaVersion,
    },
    /// Version is not supported (too new or outside window)
    Unsupported {
        service: ServiceId,
        supported_range: (SchemaVersion, SchemaVersion),
        received: SchemaVersion,
    },
}

impl fmt::Display for SchemaMismatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaMismatchError::UpgradeRequired {
                service,
                expected_min,
                received,
            } => write!(
                f,
                "Schema version too old for service {}: received {}, expected at least {}. Please upgrade sender.",
                service, received, expected_min
            ),
            SchemaMismatchError::Unsupported {
                service,
                supported_range,
                received,
            } => write!(
                f,
                "Schema version not supported by service {}: received {}, supported range {}-{}",
                service, received, supported_range.0, supported_range.1
            ),
        }
    }
}

impl std::error::Error for SchemaMismatchError {}

impl SchemaMismatchError {
    /// Creates an upgrade required error
    pub fn upgrade_required(
        service: ServiceId,
        expected_min: SchemaVersion,
        received: SchemaVersion,
    ) -> Self {
        Self::UpgradeRequired {
            service,
            expected_min,
            received,
        }
    }

    /// Creates an unsupported version error
    pub fn unsupported(
        service: ServiceId,
        supported_range: (SchemaVersion, SchemaVersion),
        received: SchemaVersion,
    ) -> Self {
        Self::Unsupported {
            service,
            supported_range,
            received,
        }
    }
}

/// Message envelope containing routing and metadata
///
/// This is the outer wrapper for all messages. The actual payload
/// is type-erased to allow generic message handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Unique identifier for this message
    pub id: MessageId,
    /// Destination service
    pub destination: ServiceId,
    /// Source task (optional, for responses)
    pub source: Option<TaskId>,
    /// Action or method to invoke
    pub action: String,
    /// Schema version of the payload
    pub schema_version: SchemaVersion,
    /// Correlation ID for request/response matching
    pub correlation_id: Option<MessageId>,
    /// Serialized payload (type-erased)
    pub payload: MessagePayload,
}

impl MessageEnvelope {
    /// Creates a new message envelope
    pub fn new(
        destination: ServiceId,
        action: String,
        schema_version: SchemaVersion,
        payload: MessagePayload,
    ) -> Self {
        Self {
            id: MessageId::new(),
            destination,
            source: None,
            action,
            schema_version,
            correlation_id: None,
            payload,
        }
    }

    /// Sets the source task
    pub fn with_source(mut self, source: TaskId) -> Self {
        self.source = Some(source);
        self
    }

    /// Sets the correlation ID (for responses)
    pub fn with_correlation(mut self, correlation_id: MessageId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Checks if this is a response to another message
    pub fn is_response(&self) -> bool {
        self.correlation_id.is_some()
    }
}

/// Type-erased message payload
///
/// In a real system, this would use a more sophisticated serialization
/// mechanism (e.g., Cap'n Proto, protobuf). For now, we use JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    /// Serialized data (JSON for now)
    data: Vec<u8>,
}

impl MessagePayload {
    /// Creates a new payload from serializable data
    pub fn new<T: Serialize>(data: &T) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_vec(data)?;
        Ok(Self { data: json })
    }

    /// Deserializes the payload into a specific type
    pub fn deserialize<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.data)
    }

    /// Returns the raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

/// A complete message with typed payload
///
/// This is a convenience wrapper that combines the envelope with a typed payload.
#[derive(Debug)]
pub struct Message<T> {
    /// The message envelope
    pub envelope: MessageEnvelope,
    /// The typed payload
    pub payload: T,
}

impl<T: Serialize> Message<T> {
    /// Creates a new message
    pub fn new(
        destination: ServiceId,
        action: String,
        schema_version: SchemaVersion,
        payload: T,
    ) -> Result<Self, serde_json::Error> {
        let payload_bytes = MessagePayload::new(&payload)?;
        let envelope = MessageEnvelope::new(destination, action, schema_version, payload_bytes);
        Ok(Self { envelope, payload })
    }

    /// Converts this message into an envelope (consuming the payload)
    pub fn into_envelope(self) -> MessageEnvelope {
        self.envelope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestPayload {
        value: i32,
    }

    #[test]
    fn test_message_id_creation() {
        let id1 = MessageId::new();
        let id2 = MessageId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_schema_version_compatibility() {
        let v1_0 = SchemaVersion::new(1, 0);
        let v1_1 = SchemaVersion::new(1, 1);
        let v2_0 = SchemaVersion::new(2, 0);

        assert!(v1_0.is_compatible_with(&v1_1));
        assert!(v1_1.is_compatible_with(&v1_0));
        assert!(!v1_0.is_compatible_with(&v2_0));
        assert!(!v2_0.is_compatible_with(&v1_0));
    }

    #[test]
    fn test_message_payload_serialization() {
        let payload = TestPayload { value: 42 };
        let msg_payload = MessagePayload::new(&payload).unwrap();

        let deserialized: TestPayload = msg_payload.deserialize().unwrap();
        assert_eq!(deserialized, payload);
    }

    #[test]
    fn test_message_envelope_creation() {
        let dest = ServiceId::new();
        let payload = TestPayload { value: 42 };
        let msg_payload = MessagePayload::new(&payload).unwrap();

        let envelope = MessageEnvelope::new(
            dest,
            "test_action".to_string(),
            SchemaVersion::new(1, 0),
            msg_payload,
        );

        assert_eq!(envelope.destination, dest);
        assert_eq!(envelope.action, "test_action");
        assert!(!envelope.is_response());
    }

    #[test]
    fn test_message_envelope_with_correlation() {
        let dest = ServiceId::new();
        let payload = TestPayload { value: 42 };
        let msg_payload = MessagePayload::new(&payload).unwrap();
        let original_id = MessageId::new();

        let envelope = MessageEnvelope::new(
            dest,
            "response_action".to_string(),
            SchemaVersion::new(1, 0),
            msg_payload,
        )
        .with_correlation(original_id);

        assert!(envelope.is_response());
        assert_eq!(envelope.correlation_id, Some(original_id));
    }

    #[test]
    fn test_typed_message_creation() {
        let dest = ServiceId::new();
        let payload = TestPayload { value: 42 };

        let message =
            Message::new(dest, "test".to_string(), SchemaVersion::new(1, 0), payload).unwrap();

        assert_eq!(message.envelope.destination, dest);
        assert_eq!(message.payload.value, 42);
    }

    #[test]
    fn test_message_with_source() {
        let dest = ServiceId::new();
        let source = TaskId::new();
        let payload = TestPayload { value: 42 };
        let msg_payload = MessagePayload::new(&payload).unwrap();

        let envelope = MessageEnvelope::new(
            dest,
            "test".to_string(),
            SchemaVersion::new(1, 0),
            msg_payload,
        )
        .with_source(source);

        assert_eq!(envelope.source, Some(source));
    }

    // ===== SchemaVersion comparison tests =====

    #[test]
    fn test_schema_version_older_than() {
        let v1_0 = SchemaVersion::new(1, 0);
        let v1_1 = SchemaVersion::new(1, 1);
        let v2_0 = SchemaVersion::new(2, 0);

        assert!(v1_0.is_older_than(&v1_1));
        assert!(v1_0.is_older_than(&v2_0));
        assert!(v1_1.is_older_than(&v2_0));
        assert!(!v1_1.is_older_than(&v1_0));
        assert!(!v2_0.is_older_than(&v1_0));
        assert!(!v1_0.is_older_than(&v1_0));
    }

    #[test]
    fn test_schema_version_newer_than() {
        let v1_0 = SchemaVersion::new(1, 0);
        let v1_1 = SchemaVersion::new(1, 1);
        let v2_0 = SchemaVersion::new(2, 0);

        assert!(v1_1.is_newer_than(&v1_0));
        assert!(v2_0.is_newer_than(&v1_0));
        assert!(v2_0.is_newer_than(&v1_1));
        assert!(!v1_0.is_newer_than(&v1_1));
        assert!(!v1_0.is_newer_than(&v2_0));
        assert!(!v1_0.is_newer_than(&v1_0));
    }

    // ===== Compatibility enum tests =====

    #[test]
    fn test_compatibility_enum() {
        assert_eq!(Compatibility::Compatible, Compatibility::Compatible);
        assert_ne!(Compatibility::Compatible, Compatibility::UpgradeRequired);
        assert_ne!(Compatibility::Compatible, Compatibility::Unsupported);
    }

    // ===== VersionPolicy tests =====

    #[test]
    fn test_version_policy_current_only() {
        let policy = VersionPolicy::current(2, 5);

        // Same major version is compatible
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(2, 0)),
            Compatibility::Compatible
        );
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(2, 5)),
            Compatibility::Compatible
        );
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(2, 10)),
            Compatibility::Compatible
        );

        // Older major version requires upgrade
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(1, 0)),
            Compatibility::UpgradeRequired
        );

        // Newer major version is unsupported
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(3, 0)),
            Compatibility::Unsupported
        );
    }

    #[test]
    fn test_version_policy_with_minimum() {
        let policy = VersionPolicy::current(3, 0).with_min_major(2);

        // v3.x is compatible
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(3, 0)),
            Compatibility::Compatible
        );
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(3, 5)),
            Compatibility::Compatible
        );

        // v2.x is compatible (within N-1 window)
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(2, 0)),
            Compatibility::Compatible
        );
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(2, 99)),
            Compatibility::Compatible
        );

        // v1.x requires upgrade (too old)
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(1, 0)),
            Compatibility::UpgradeRequired
        );
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(1, 999)),
            Compatibility::UpgradeRequired
        );

        // v4.x is unsupported (too new)
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(4, 0)),
            Compatibility::Unsupported
        );
    }

    #[test]
    fn test_version_policy_boundary_conditions() {
        let policy = VersionPolicy::current(10, 5).with_min_major(8);

        // Boundary: exactly at minimum
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(8, 0)),
            Compatibility::Compatible
        );

        // Boundary: just below minimum
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(7, 999)),
            Compatibility::UpgradeRequired
        );

        // Boundary: exactly at current
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(10, 5)),
            Compatibility::Compatible
        );

        // Boundary: just above current
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(11, 0)),
            Compatibility::Unsupported
        );

        // Extreme: version 0
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(0, 0)),
            Compatibility::UpgradeRequired
        );

        // Extreme: very high version
        assert_eq!(
            policy.check_compatibility(&SchemaVersion::new(999, 0)),
            Compatibility::Unsupported
        );
    }

    #[test]
    fn test_version_policy_getters() {
        let policy = VersionPolicy::current(5, 3).with_min_major(4);

        assert_eq!(policy.current_version(), SchemaVersion::new(5, 3));
        assert_eq!(policy.min_version(), SchemaVersion::new(4, 0));
    }

    // ===== SchemaMismatchError tests =====

    #[test]
    fn test_schema_mismatch_error_upgrade_required() {
        let service = ServiceId::new();
        let error = SchemaMismatchError::upgrade_required(
            service,
            SchemaVersion::new(2, 0),
            SchemaVersion::new(1, 5),
        );

        match error {
            SchemaMismatchError::UpgradeRequired {
                service: s,
                expected_min,
                received,
            } => {
                assert_eq!(s, service);
                assert_eq!(expected_min, SchemaVersion::new(2, 0));
                assert_eq!(received, SchemaVersion::new(1, 5));
            }
            _ => panic!("Expected UpgradeRequired variant"),
        }
    }

    #[test]
    fn test_schema_mismatch_error_unsupported() {
        let service = ServiceId::new();
        let error = SchemaMismatchError::unsupported(
            service,
            (SchemaVersion::new(2, 0), SchemaVersion::new(3, 5)),
            SchemaVersion::new(4, 0),
        );

        match error {
            SchemaMismatchError::Unsupported {
                service: s,
                supported_range,
                received,
            } => {
                assert_eq!(s, service);
                assert_eq!(supported_range.0, SchemaVersion::new(2, 0));
                assert_eq!(supported_range.1, SchemaVersion::new(3, 5));
                assert_eq!(received, SchemaVersion::new(4, 0));
            }
            _ => panic!("Expected Unsupported variant"),
        }
    }

    #[test]
    fn test_schema_mismatch_error_display() {
        let service = ServiceId::new();

        let error1 = SchemaMismatchError::upgrade_required(
            service,
            SchemaVersion::new(2, 0),
            SchemaVersion::new(1, 5),
        );
        let msg1 = format!("{}", error1);
        assert!(msg1.contains("too old"));
        assert!(msg1.contains("v1.5"));
        assert!(msg1.contains("v2.0"));
        assert!(msg1.contains("upgrade"));

        let error2 = SchemaMismatchError::unsupported(
            service,
            (SchemaVersion::new(2, 0), SchemaVersion::new(3, 5)),
            SchemaVersion::new(4, 0),
        );
        let msg2 = format!("{}", error2);
        assert!(msg2.contains("not supported"));
        assert!(msg2.contains("v4.0"));
        assert!(msg2.contains("v2.0"));
        assert!(msg2.contains("v3.5"));
    }

    #[test]
    fn test_schema_mismatch_error_equality() {
        let service1 = ServiceId::new();
        let service2 = ServiceId::new();

        let error1 = SchemaMismatchError::upgrade_required(
            service1,
            SchemaVersion::new(2, 0),
            SchemaVersion::new(1, 5),
        );
        let error2 = SchemaMismatchError::upgrade_required(
            service1,
            SchemaVersion::new(2, 0),
            SchemaVersion::new(1, 5),
        );
        let error3 = SchemaMismatchError::upgrade_required(
            service2,
            SchemaVersion::new(2, 0),
            SchemaVersion::new(1, 5),
        );

        assert_eq!(error1, error2);
        assert_ne!(error1, error3);
    }
}
