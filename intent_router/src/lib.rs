//! # Intent Router
//!
//! This crate implements typed, structured command routing.
//!
//! ## Philosophy
//!
//! Unlike shell commands (stringly-typed, path-based), intents are:
//! - Typed and structured
//! - Routable to handlers
//! - Versioned for compatibility

use core_types::ServiceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for an intent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IntentId(Uuid);

impl IntentId {
    /// Creates a new random intent ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for IntentId {
    fn default() -> Self {
        Self::new()
    }
}

/// An intent represents a request to perform an action
///
/// Unlike shell commands ("ls -la /tmp"), intents are structured:
/// - Type: what kind of action
/// - Parameters: typed data, not strings
/// - Handler: which service handles this
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Unique identifier
    pub id: IntentId,
    /// Intent type (e.g., "storage.read", "ui.display")
    pub intent_type: String,
    /// Version of the intent schema
    pub version: (u32, u32),
    /// Structured parameters
    pub parameters: Vec<(String, String)>,
}

impl Intent {
    /// Creates a new intent
    pub fn new(intent_type: String, version: (u32, u32)) -> Self {
        Self {
            id: IntentId::new(),
            intent_type,
            version,
            parameters: Vec::new(),
        }
    }

    /// Adds a parameter to the intent
    pub fn with_parameter(mut self, key: String, value: String) -> Self {
        self.parameters.push((key, value));
        self
    }
}

/// Router for intents
///
/// This maps intent types to service handlers.
pub struct IntentRouter {
    /// Mapping from intent types to services
    routes: Vec<(String, ServiceId)>,
}

impl IntentRouter {
    /// Creates a new intent router
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Registers a handler for an intent type
    pub fn register(&mut self, intent_type: String, handler: ServiceId) {
        self.routes.push((intent_type, handler));
    }

    /// Routes an intent to a handler
    pub fn route(&self, intent_type: &str) -> Option<ServiceId> {
        self.routes
            .iter()
            .find(|(t, _)| t == intent_type)
            .map(|(_, s)| *s)
    }
}

impl Default for IntentRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_creation() {
        let intent = Intent::new("test.action".to_string(), (1, 0));
        assert_eq!(intent.intent_type, "test.action");
        assert_eq!(intent.version, (1, 0));
        assert!(intent.parameters.is_empty());
    }

    #[test]
    fn test_intent_with_parameters() {
        let intent = Intent::new("test.action".to_string(), (1, 0))
            .with_parameter("key1".to_string(), "value1".to_string())
            .with_parameter("key2".to_string(), "value2".to_string());

        assert_eq!(intent.parameters.len(), 2);
        assert_eq!(intent.parameters[0].0, "key1");
    }

    #[test]
    fn test_intent_router() {
        let mut router = IntentRouter::new();
        let service_id = ServiceId::new();

        router.register("test.action".to_string(), service_id);

        let routed = router.route("test.action");
        assert_eq!(routed, Some(service_id));

        let not_found = router.route("other.action");
        assert_eq!(not_found, None);
    }

    #[test]
    fn test_intent_id_uniqueness() {
        let id1 = IntentId::new();
        let id2 = IntentId::new();
        assert_ne!(id1, id2);
    }
}
