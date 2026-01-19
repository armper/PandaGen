//! Intent Router service contract tests
//!
//! These tests define the stable contract for the IntentRouter service.

use crate::test_helpers::*;
use core_types::ServiceId;
use ipc::SchemaVersion;
use serde::{Deserialize, Serialize};

// ===== IntentRouter Contract Version =====
const INTENT_ROUTER_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// ===== Action Identifiers =====
const ACTION_ROUTE_INTENT: &str = "intent_router.route_intent";
const ACTION_REGISTER_HANDLER: &str = "intent_router.register_handler";
const ACTION_UNREGISTER_HANDLER: &str = "intent_router.unregister_handler";
const ACTION_LIST_HANDLERS: &str = "intent_router.list_handlers";

// ===== Canonical Payload Structures =====

/// Route intent request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteIntentRequest {
    pub intent_type: String,
    pub parameters: Vec<(String, String)>,
}

/// Route intent response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteIntentResponse {
    pub handler_service: ServiceId,
}

/// Register handler request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterHandlerRequest {
    pub intent_type: String,
    pub handler_service: ServiceId,
    #[serde(default)]
    pub priority: u32,
}

/// Unregister handler request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnregisterHandlerRequest {
    pub intent_type: String,
    pub handler_service: ServiceId,
}

/// List handlers request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListHandlersRequest {
    #[serde(default)]
    pub intent_type: Option<String>,
}

/// List handlers response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListHandlersResponse {
    pub handlers: Vec<HandlerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandlerInfo {
    pub intent_type: String,
    pub handler_service: ServiceId,
    pub priority: u32,
}

// ===== Contract Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_intent_contract() {
        let service_id = ServiceId::new();
        let request = RouteIntentRequest {
            intent_type: "open_file".to_string(),
            parameters: vec![("path".to_string(), "/test/file.txt".to_string())],
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_ROUTE_INTENT,
            INTENT_ROUTER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_ROUTE_INTENT, INTENT_ROUTER_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: RouteIntentRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_register_handler_contract() {
        let service_id = ServiceId::new();
        let request = RegisterHandlerRequest {
            intent_type: "open_file".to_string(),
            handler_service: ServiceId::new(),
            priority: 10,
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_REGISTER_HANDLER,
            INTENT_ROUTER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(
            &envelope,
            ACTION_REGISTER_HANDLER,
            INTENT_ROUTER_SCHEMA_VERSION,
        );
        verify_major_version(&envelope, 1);

        let deserialized: RegisterHandlerRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_unregister_handler_contract() {
        let service_id = ServiceId::new();
        let request = UnregisterHandlerRequest {
            intent_type: "open_file".to_string(),
            handler_service: ServiceId::new(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_UNREGISTER_HANDLER,
            INTENT_ROUTER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(
            &envelope,
            ACTION_UNREGISTER_HANDLER,
            INTENT_ROUTER_SCHEMA_VERSION,
        );
        verify_major_version(&envelope, 1);

        let deserialized: UnregisterHandlerRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_list_handlers_contract() {
        let service_id = ServiceId::new();
        let request = ListHandlersRequest {
            intent_type: Some("open_file".to_string()),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_LIST_HANDLERS,
            INTENT_ROUTER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_LIST_HANDLERS, INTENT_ROUTER_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: ListHandlersRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_list_handlers_response_contract() {
        let service_id = ServiceId::new();
        let response = ListHandlersResponse {
            handlers: vec![HandlerInfo {
                intent_type: "open_file".to_string(),
                handler_service: ServiceId::new(),
                priority: 10,
            }],
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_LIST_HANDLERS,
            INTENT_ROUTER_SCHEMA_VERSION,
            &response,
        );

        let deserialized: ListHandlersResponse = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized.handlers.len(), response.handlers.len());
    }

    #[test]
    fn test_optional_parameters_backward_compatibility() {
        // Test that priority field with default is backward compatible
        let json_without_priority = r#"{"intent_type":"test","handler_service":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let parsed: RegisterHandlerRequest = serde_json::from_str(json_without_priority).unwrap();
        assert_eq!(parsed.priority, 0); // Default value

        // Test that intent_type None is backward compatible
        let json_empty = r#"{}"#;
        let parsed: ListHandlersRequest = serde_json::from_str(json_empty).unwrap();
        assert_eq!(parsed.intent_type, None);
    }

    #[test]
    fn test_action_identifiers_are_stable() {
        assert_eq!(ACTION_ROUTE_INTENT, "intent_router.route_intent");
        assert_eq!(ACTION_REGISTER_HANDLER, "intent_router.register_handler");
        assert_eq!(
            ACTION_UNREGISTER_HANDLER,
            "intent_router.unregister_handler"
        );
        assert_eq!(ACTION_LIST_HANDLERS, "intent_router.list_handlers");
    }

    #[test]
    fn test_schema_version_is_stable() {
        assert_eq!(INTENT_ROUTER_SCHEMA_VERSION.major, 1);
        assert_eq!(INTENT_ROUTER_SCHEMA_VERSION.minor, 0);
    }
}
