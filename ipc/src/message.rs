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
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}", self.major, self.minor)
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
}
