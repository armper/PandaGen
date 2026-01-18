//! Service descriptor and restart policy

use core_types::{Cap, ServiceId};
use serde::{Deserialize, Serialize};

/// Restart policy for a service
///
/// Unlike Unix where restart behavior is often in shell scripts,
/// we make it explicit and type-safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPolicy {
    /// Never restart the service
    Never,
    /// Always restart the service (regardless of exit status)
    Always,
    /// Restart only on failure
    OnFailure,
    /// Restart with exponential backoff
    ExponentialBackoff { max_attempts: u32 },
}

/// Descriptor for a service
///
/// This specifies what to run and how to manage it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDescriptor {
    /// Unique service identifier
    pub service_id: ServiceId,
    /// Human-readable name
    pub name: String,
    /// Restart policy
    pub restart_policy: RestartPolicy,
    /// Initial capabilities granted to the service
    pub capabilities: Vec<Cap<()>>,
    /// Service dependencies (other services this depends on)
    pub dependencies: Vec<ServiceId>,
}

impl ServiceDescriptor {
    /// Creates a new service descriptor
    pub fn new(name: String, restart_policy: RestartPolicy) -> Self {
        Self {
            service_id: ServiceId::new(),
            name,
            restart_policy,
            capabilities: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// Adds a capability to the service
    pub fn with_capability(mut self, cap: Cap<()>) -> Self {
        self.capabilities.push(cap);
        self
    }

    /// Adds a dependency to the service
    pub fn with_dependency(mut self, dep: ServiceId) -> Self {
        self.dependencies.push(dep);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restart_policy_equality() {
        assert_eq!(RestartPolicy::Never, RestartPolicy::Never);
        assert_ne!(RestartPolicy::Never, RestartPolicy::Always);
    }

    #[test]
    fn test_service_descriptor_creation() {
        let desc = ServiceDescriptor::new("test_service".to_string(), RestartPolicy::Always);
        assert_eq!(desc.name, "test_service");
        assert_eq!(desc.restart_policy, RestartPolicy::Always);
        assert!(desc.capabilities.is_empty());
        assert!(desc.dependencies.is_empty());
    }

    #[test]
    fn test_service_descriptor_with_dependency() {
        let dep_id = ServiceId::new();
        let desc = ServiceDescriptor::new("test".to_string(), RestartPolicy::Never)
            .with_dependency(dep_id);

        assert_eq!(desc.dependencies.len(), 1);
        assert_eq!(desc.dependencies[0], dep_id);
    }

    #[test]
    fn test_exponential_backoff_policy() {
        let policy = RestartPolicy::ExponentialBackoff { max_attempts: 5 };
        assert_eq!(
            policy,
            RestartPolicy::ExponentialBackoff { max_attempts: 5 }
        );
    }
}
