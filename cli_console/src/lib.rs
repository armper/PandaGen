//! # CLI Console (Demo)
//!
//! This is a simple demonstration of wiring services together.
//! It is NOT a shell and NOT intended for POSIX compatibility.

pub mod commands;
pub mod interactive;

use kernel_api::{KernelApi, TaskDescriptor};
use services_registry::ServiceRegistry;
use sim_kernel::SimulatedKernel;

/// Bootstrap function
///
/// This wires together the simulated kernel and core services.
/// It returns handles/capabilities that can be used to interact with the system.
///
/// ## Design
///
/// Unlike traditional OS bootstrap (which often involves magic and implicit state),
/// this is explicit and returns typed capabilities.
pub fn bootstrap() -> (SimulatedKernel, ServiceRegistry) {
    let kernel = SimulatedKernel::new();
    let registry = ServiceRegistry::new();

    // In a real system, we would:
    // 1. Spawn core services (logger, storage, etc.)
    // 2. Register them in the registry
    // 3. Return capabilities to interact with them

    (kernel, registry)
}

/// Demo function showing how to use the system
pub fn demo() {
    let (mut kernel, _registry) = bootstrap();

    // Spawn a task
    let descriptor = TaskDescriptor::new("demo_task".to_string());
    let _handle = kernel.spawn_task(descriptor).expect("Failed to spawn task");

    println!("Demo completed successfully");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap() {
        let (kernel, registry) = bootstrap();
        assert_eq!(kernel.task_count(), 0);
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_demo() {
        // Just verify it runs without panicking
        demo();
    }
}
