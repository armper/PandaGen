//! # Service Registry
//!
//! This crate implements capability-based service discovery.
//!
//! ## Philosophy
//!
//! Unlike traditional service discovery (DNS, path-based, ports),
//! services are registered and looked up using capabilities.

use core_types::ServiceId;
use ipc::{ChannelId, SchemaVersion};
use std::collections::HashMap;

/// Error types for registry operations
#[derive(Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// Service already registered
    AlreadyRegistered(ServiceId),
    /// Service name already registered
    NameAlreadyRegistered(String),
    /// Service not found
    NotFound(ServiceId),
    /// Service name not found
    NameNotFound(String),
}

/// Descriptor for a registered service
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDescriptor {
    pub id: ServiceId,
    pub name: String,
    pub channel: ChannelId,
    pub schema_version: SchemaVersion,
}

/// Service registry
///
/// This maintains a mapping from service IDs to communication channels.
/// Unlike traditional service registries (which use paths or ports),
/// this uses strongly-typed identifiers and capabilities.
pub struct ServiceRegistry {
    /// Registered services
    services: HashMap<ServiceId, ChannelId>,
    /// Optional descriptors by ID
    descriptors: HashMap<ServiceId, ServiceDescriptor>,
    /// Name lookup table
    names: HashMap<String, ServiceId>,
}

impl ServiceRegistry {
    /// Creates a new service registry
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            descriptors: HashMap::new(),
            names: HashMap::new(),
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

    /// Registers a service with a descriptor (name + schema version).
    pub fn register_descriptor(
        &mut self,
        descriptor: ServiceDescriptor,
    ) -> Result<(), RegistryError> {
        if self.services.contains_key(&descriptor.id) {
            return Err(RegistryError::AlreadyRegistered(descriptor.id));
        }
        if self.names.contains_key(&descriptor.name) {
            return Err(RegistryError::NameAlreadyRegistered(descriptor.name));
        }
        self.services.insert(descriptor.id, descriptor.channel);
        self.names.insert(descriptor.name.clone(), descriptor.id);
        self.descriptors.insert(descriptor.id, descriptor);
        Ok(())
    }

    /// Registers a service by name.
    pub fn register_named(
        &mut self,
        name: String,
        service_id: ServiceId,
        channel: ChannelId,
        schema_version: SchemaVersion,
    ) -> Result<(), RegistryError> {
        let descriptor = ServiceDescriptor {
            id: service_id,
            name,
            channel,
            schema_version,
        };
        self.register_descriptor(descriptor)
    }

    /// Looks up a service
    pub fn lookup(&self, service_id: ServiceId) -> Result<ChannelId, RegistryError> {
        self.services
            .get(&service_id)
            .copied()
            .ok_or(RegistryError::NotFound(service_id))
    }

    /// Looks up a service by name.
    pub fn lookup_by_name(&self, name: &str) -> Result<ChannelId, RegistryError> {
        let id = self
            .names
            .get(name)
            .copied()
            .ok_or_else(|| RegistryError::NameNotFound(name.to_string()))?;
        self.lookup(id)
    }

    /// Returns a descriptor for a service, if registered with metadata.
    pub fn descriptor(&self, service_id: ServiceId) -> Option<&ServiceDescriptor> {
        self.descriptors.get(&service_id)
    }

    /// Lists all descriptors (metadata-aware registrations only).
    pub fn descriptors(&self) -> Vec<ServiceDescriptor> {
        self.descriptors.values().cloned().collect()
    }

    /// Unregisters a service
    pub fn unregister(&mut self, service_id: ServiceId) -> Result<(), RegistryError> {
        self.services
            .remove(&service_id)
            .ok_or(RegistryError::NotFound(service_id))?;
        if let Some(descriptor) = self.descriptors.remove(&service_id) {
            self.names.remove(&descriptor.name);
        }
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
    fn test_register_descriptor_and_lookup_by_name() {
        let mut registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let channel_id = ChannelId::new();
        let schema_version = SchemaVersion::new(1, 0);

        registry
            .register_named("input".to_string(), service_id, channel_id, schema_version)
            .unwrap();

        let looked_up = registry.lookup_by_name("input").unwrap();
        assert_eq!(looked_up, channel_id);

        let descriptor = registry.descriptor(service_id).unwrap();
        assert_eq!(descriptor.name, "input");
        assert_eq!(descriptor.schema_version, schema_version);
    }

    #[test]
    fn test_register_descriptor_duplicate_name() {
        let mut registry = ServiceRegistry::new();
        let service_id1 = ServiceId::new();
        let service_id2 = ServiceId::new();
        let channel_id1 = ChannelId::new();
        let channel_id2 = ChannelId::new();
        let schema_version = SchemaVersion::new(1, 0);

        registry
            .register_named(
                "logger".to_string(),
                service_id1,
                channel_id1,
                schema_version,
            )
            .unwrap();

        let result = registry.register_named(
            "logger".to_string(),
            service_id2,
            channel_id2,
            schema_version,
        );

        assert_eq!(
            result,
            Err(RegistryError::NameAlreadyRegistered("logger".to_string()))
        );
    }

    #[test]
    fn test_unregister_not_found() {
        let mut registry = ServiceRegistry::new();
        let service_id = ServiceId::new();
        let result = registry.unregister(service_id);
        assert_eq!(result, Err(RegistryError::NotFound(service_id)));
    }
}
