//! # Service Contract Schemas
//!
//! **These tests cannot fail when a service changes.** Read that before
//! treating anything here as coverage.
//!
//! The crate depends on `core_types`, `ipc`, `serde` and `serde_json` -- and
//! on none of the services whose contracts it names. Every payload type in
//! these modules (`CreateObjectRequest`, `RouteIntentRequest`, and the rest)
//! is declared here, in this crate, and the tests serialize those local
//! declarations and deserialize them back. They assert that this crate
//! agrees with itself, which it always will.
//!
//! The action identifiers have the same problem from the other end:
//! `"storage.create_object"` and its siblings appear nowhere else in the
//! workspace. No service dispatches on them, because the services are called
//! through their Rust APIs rather than through message envelopes. There is
//! no wire contract for these to be the golden copy of.
//!
//! So what this crate is, honestly, is a written-down proposal for a
//! message interface that does not exist yet, plus round-trip tests that
//! confirm the proposal is serializable. That is worth keeping -- it is the
//! clearest statement of the intended shape anywhere in the tree -- but it
//! is not a drift detector, and the previous version of this comment
//! ("Contract tests fail when interfaces change") said it was.
//!
//! To make it one: have a service depend on these types and dispatch on
//! these action strings, then the tests start meaning something. Until then,
//! changing `services_storage` will not turn anything here red.

pub mod intent_router;
pub mod process_manager;
pub mod registry;
pub mod storage;

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
