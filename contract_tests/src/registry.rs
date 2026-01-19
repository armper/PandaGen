//! Service Registry contract tests
//!
//! These tests define the stable contract for the Registry service.

use crate::test_helpers::*;
use core_types::ServiceId;
use ipc::{ChannelId, SchemaVersion};
use serde::{Deserialize, Serialize};

// ===== Registry Contract Version =====
const REGISTRY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// ===== Action Identifiers =====
const ACTION_REGISTER: &str = "registry.register";
const ACTION_LOOKUP: &str = "registry.lookup";
const ACTION_UNREGISTER: &str = "registry.unregister";
const ACTION_LIST: &str = "registry.list";

// ===== Canonical Payload Structures =====

/// Register service request payload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterRequest {
    pub service_id: ServiceId,
    pub channel: ChannelId,
}

/// Lookup service request payload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LookupRequest {
    pub service_id: ServiceId,
}

/// Lookup service response payload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LookupResponse {
    pub channel: ChannelId,
}

/// Unregister service request payload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnregisterRequest {
    pub service_id: ServiceId,
}

/// List services response payload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListResponse {
    pub services: Vec<ServiceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceEntry {
    pub service_id: ServiceId,
    pub channel: ChannelId,
}

// ===== Contract Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_contract() {
        let service_id = ServiceId::new();
        let request = RegisterRequest {
            service_id,
            channel: ChannelId::new(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_REGISTER,
            REGISTRY_SCHEMA_VERSION,
            &request,
        );

        // Verify contract stability
        verify_envelope_contract(&envelope, ACTION_REGISTER, REGISTRY_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        // Verify payload can be deserialized
        let deserialized: RegisterRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_lookup_contract() {
        let service_id = ServiceId::new();
        let request = LookupRequest { service_id };

        let envelope = create_test_envelope(
            service_id,
            ACTION_LOOKUP,
            REGISTRY_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_LOOKUP, REGISTRY_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: LookupRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_lookup_response_contract() {
        let service_id = ServiceId::new();
        let response = LookupResponse {
            channel: ChannelId::new(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_LOOKUP,
            REGISTRY_SCHEMA_VERSION,
            &response,
        );

        let deserialized: LookupResponse = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, response);
    }

    #[test]
    fn test_unregister_contract() {
        let service_id = ServiceId::new();
        let request = UnregisterRequest { service_id };

        let envelope = create_test_envelope(
            service_id,
            ACTION_UNREGISTER,
            REGISTRY_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_UNREGISTER, REGISTRY_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: UnregisterRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_list_contract() {
        let service_id = ServiceId::new();
        let response = ListResponse {
            services: vec![
                ServiceEntry {
                    service_id: ServiceId::new(),
                    channel: ChannelId::new(),
                },
                ServiceEntry {
                    service_id: ServiceId::new(),
                    channel: ChannelId::new(),
                },
            ],
        };

        let envelope =
            create_test_envelope(service_id, ACTION_LIST, REGISTRY_SCHEMA_VERSION, &response);

        verify_envelope_contract(&envelope, ACTION_LIST, REGISTRY_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: ListResponse = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized.services.len(), response.services.len());
    }

    #[test]
    fn test_action_identifiers_are_stable() {
        // These constants MUST NOT CHANGE without intentional version bump
        assert_eq!(ACTION_REGISTER, "registry.register");
        assert_eq!(ACTION_LOOKUP, "registry.lookup");
        assert_eq!(ACTION_UNREGISTER, "registry.unregister");
        assert_eq!(ACTION_LIST, "registry.list");
    }

    #[test]
    fn test_schema_version_is_stable() {
        // Schema version MUST NOT CHANGE without intentional evolution
        assert_eq!(REGISTRY_SCHEMA_VERSION.major, 1);
        assert_eq!(REGISTRY_SCHEMA_VERSION.minor, 0);
    }
}
