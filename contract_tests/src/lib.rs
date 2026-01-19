//! # Service Contract Tests
//!
//! This crate provides "golden" tests for service contracts to ensure
//! they don't drift accidentally over time.
//!
//! ## Philosophy
//!
//! - **Explicit over implicit**: Service contracts are written as code
//! - **Testability first**: Contract tests fail when interfaces change
//! - **Mechanism not policy**: Define what must be stable, not how to use it
//!
//! ## Structure
//!
//! Each service has a module with contract tests that verify:
//! - Message envelope structure
//! - Action identifiers
//! - Schema versions
//! - Payload field contracts

pub mod registry;
pub mod storage;
pub mod process_manager;
pub mod intent_router;

/// Common test helpers for contract validation
pub mod test_helpers {
    use core_types::ServiceId;
    use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
    use serde::Serialize;

    /// Creates a test message envelope with expected fields
    pub fn create_test_envelope<T: Serialize>(
        destination: ServiceId,
        action: &str,
        version: SchemaVersion,
        payload: &T,
    ) -> MessageEnvelope {
        let payload_bytes = MessagePayload::new(payload).expect("Failed to serialize payload");
        MessageEnvelope::new(destination, action.to_string(), version, payload_bytes)
    }

    /// Verifies an envelope has the expected action and version
    pub fn verify_envelope_contract(
        envelope: &MessageEnvelope,
        expected_action: &str,
        expected_version: SchemaVersion,
    ) {
        assert_eq!(
            envelope.action, expected_action,
            "Action identifier changed: expected '{}', got '{}'",
            expected_action, envelope.action
        );
        assert_eq!(
            envelope.schema_version, expected_version,
            "Schema version changed: expected {}, got {}",
            expected_version, envelope.schema_version
        );
    }

    /// Verifies schema version stays within major version
    pub fn verify_major_version(envelope: &MessageEnvelope, expected_major: u32) {
        assert_eq!(
            envelope.schema_version.major, expected_major,
            "Major version changed (breaking change): expected {}, got {}",
            expected_major, envelope.schema_version.major
        );
    }
}
