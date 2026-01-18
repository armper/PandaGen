//! # Service Registry
//!
//! This crate implements capability-based service discovery.
//!
//! ## Philosophy
//!
//! Unlike traditional service discovery (DNS, path-based, ports),
//! services are registered and looked up using capabilities.

use core_types::ServiceId;
use ipc::ChannelId;
use std::collections::HashMap;

/// Error types for registry operations
#[derive(Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// Service already registered
    AlreadyRegistered(ServiceId),
    /// Service not found
    NotFound(ServiceId),
}

/// Service registry
///
/// This maintains a mapping from service IDs to communication channels.
/// Unlike traditional service registries (which use paths or ports),
/// this uses strongly-typed identifiers and capabilities.
pub struct ServiceRegistry {
    /// Registered services
    services: HashMap<ServiceId, ChannelId>,
}

impl ServiceRegistry {
    /// Creates a new service registry
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    /// Registers a service
    pub fn register(
        &mut self,
        service_id: ServiceId,
        channel: ChannelId,
    ) -> Result<(), RegistryError> {
        if self.services.contains_key(&service_id) {
            return Err(RegistryError::AlreadyRegistered(service_id));
        }
        self.services.insert(service_id, channel);
        Ok(())
    }

    /// Looks up a service
    pub fn lookup(&self, service_id: ServiceId) -> Result<ChannelId, RegistryError> {
        self.services
            .get(&service_id)
            .copied()
            .ok_or(RegistryError::NotFound(service_id))
    }

    /// Unregisters a service
    pub fn unregister(&mut self, service_id: ServiceId) -> Result<(), RegistryError> {
        self.services
            .remove(&service_id)
            .ok_or(RegistryError::NotFound(service_id))?;
        Ok(())
    }

    /// Returns the number of registered services
    pub fn count(&self) -> usize {
        self.services.len()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = ServiceRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_service_registration() {
        let mut registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let channel_id = ChannelId::new();

        registry.register(service_id, channel_id).unwrap();
        assert_eq!(registry.count(), 1);

        let looked_up = registry.lookup(service_id).unwrap();
        assert_eq!(looked_up, channel_id);
    }

    #[test]
    fn test_duplicate_registration() {
        let mut registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let channel1 = ChannelId::new();
        let channel2 = ChannelId::new();

        registry.register(service_id, channel1).unwrap();
        let result = registry.register(service_id, channel2);
        assert_eq!(result, Err(RegistryError::AlreadyRegistered(service_id)));
    }

    #[test]
    fn test_service_not_found() {
        let registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let result = registry.lookup(service_id);
        assert_eq!(result, Err(RegistryError::NotFound(service_id)));
    }

    #[test]
    fn test_service_unregistration() {
        let mut registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let channel_id = ChannelId::new();

        registry.register(service_id, channel_id).unwrap();
        assert_eq!(registry.count(), 1);

        registry.unregister(service_id).unwrap();
        assert_eq!(registry.count(), 0);

        let result = registry.lookup(service_id);
        assert_eq!(result, Err(RegistryError::NotFound(service_id)));
    }

    #[test]
    fn test_unregister_not_found() {
        let mut registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let result = registry.unregister(service_id);
        assert_eq!(result, Err(RegistryError::NotFound(service_id)));
    }
}
