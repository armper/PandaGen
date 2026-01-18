//! Resilience Test Utilities
//!
//! This crate provides shared utilities for resilience and integration tests.
//!
//! ## Test Philosophy
//!
//! - **Safety under faults**: System must maintain invariants even when components crash
//! - **Deterministic failures**: All faults are reproducible via FaultPlan
//! - **No capability leaks**: Capabilities cannot be used after revocation/crash
//! - **Consistency**: Storage and registry maintain consistency through failures

use kernel_api::{KernelApi, TaskDescriptor};
use services_registry::ServiceRegistry;
use sim_kernel::SimulatedKernel;

/// Bootstrap helper for tests
///
/// Creates a kernel and registry with basic services initialized.
/// This is a simplified version of the full system bootstrap for testing.
pub fn test_bootstrap() -> (SimulatedKernel, ServiceRegistry) {
    let kernel = SimulatedKernel::new();
    let registry = ServiceRegistry::new();

    (kernel, registry)
}

/// Creates a simple test service
///
/// Spawns a task that can be used for testing service lifecycle.
pub fn spawn_test_service(
    kernel: &mut SimulatedKernel,
    name: &str,
) -> Result<core_types::TaskId, kernel_api::KernelError> {
    let descriptor = TaskDescriptor::new(name.to_string());
    let handle = kernel.spawn_task(descriptor)?;
    Ok(handle.task_id)
}
