//! Process manager runtime with supervision.

use crate::{LifecycleState, RestartPolicy, ServiceDescriptor, ServiceHandle};
use core_types::ServiceId;
use identity::{ExitNotification, ExitReason};
use kernel_api::{KernelApi, KernelError, TaskDescriptor};
use std::collections::HashMap;
use thiserror::Error;

/// Source of exit notifications for supervision.
pub trait ExitNotificationSource {
    fn drain_exit_notifications(&mut self) -> Vec<ExitNotification>;
}

#[derive(Debug, Error)]
pub enum ProcessManagerError {
    #[error("Service already registered: {0}")]
    AlreadyRegistered(ServiceId),

    #[error("Service not found: {0}")]
    ServiceNotFound(ServiceId),

    #[error("Kernel error: {0}")]
    Kernel(String),
}

impl From<KernelError> for ProcessManagerError {
    fn from(err: KernelError) -> Self {
        ProcessManagerError::Kernel(err.to_string())
    }
}

#[derive(Debug, Clone)]
struct ManagedService {
    descriptor: ServiceDescriptor,
    handle: ServiceHandle,
    restart_attempts: u32,
}

/// Process manager with supervision and restart policy enforcement.
pub struct ProcessManager {
    services: HashMap<ServiceId, ManagedService>,
    task_to_service: HashMap<core_types::TaskId, ServiceId>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            task_to_service: HashMap::new(),
        }
    }

    /// Registers and starts a service.
    pub fn start_service<K: KernelApi>(
        &mut self,
        kernel: &mut K,
        descriptor: ServiceDescriptor,
    ) -> Result<ServiceHandle, ProcessManagerError> {
        if self.services.contains_key(&descriptor.service_id) {
            return Err(ProcessManagerError::AlreadyRegistered(
                descriptor.service_id,
            ));
        }

        let task_desc = TaskDescriptor {
            name: descriptor.name.clone(),
            capabilities: descriptor.capabilities.clone(),
        };
        let handle = kernel.spawn_task(task_desc)?;
        let service_handle = ServiceHandle::new(handle.task_id, LifecycleState::Running);

        self.task_to_service
            .insert(handle.task_id, descriptor.service_id);

        self.services.insert(
            descriptor.service_id,
            ManagedService {
                descriptor,
                handle: service_handle.clone(),
                restart_attempts: 0,
            },
        );

        Ok(service_handle)
    }

    /// Returns a service handle by ID.
    pub fn service_handle(&self, service_id: ServiceId) -> Option<&ServiceHandle> {
        self.services.get(&service_id).map(|svc| &svc.handle)
    }

    /// Processes exit notifications and applies restart policy.
    pub fn handle_exits<K: KernelApi, S: ExitNotificationSource>(
        &mut self,
        kernel: &mut K,
        source: &mut S,
    ) -> Result<(), ProcessManagerError> {
        for notification in source.drain_exit_notifications() {
            let Some(task_id) = notification.task_id else {
                continue;
            };
            let Some(service_id) = self.task_to_service.get(&task_id).copied() else {
                continue;
            };

            let Some(service) = self.services.get_mut(&service_id) else {
                continue;
            };

            service.handle.set_state(match notification.reason {
                ExitReason::Normal => LifecycleState::Stopped,
                ExitReason::Failure { .. } => LifecycleState::Failed,
                ExitReason::Cancelled { .. } => LifecycleState::Failed,
                ExitReason::Timeout => LifecycleState::Failed,
            });

            if Self::should_restart(service, &notification.reason) {
                let restarted = Self::restart_service(kernel, service)?;
                if restarted {
                    self.task_to_service
                        .insert(service.handle.task_id, service_id);
                }
            }
        }

        Ok(())
    }

    fn should_restart(service: &ManagedService, reason: &ExitReason) -> bool {
        match service.descriptor.restart_policy {
            RestartPolicy::Never => false,
            RestartPolicy::Always => true,
            RestartPolicy::OnFailure => !matches!(reason, ExitReason::Normal),
            RestartPolicy::ExponentialBackoff { max_attempts } => {
                service.restart_attempts < max_attempts
            }
        }
    }

    fn restart_service<K: KernelApi>(
        kernel: &mut K,
        service: &mut ManagedService,
    ) -> Result<bool, ProcessManagerError> {
        service.restart_attempts = service.restart_attempts.saturating_add(1);
        service.handle.set_state(LifecycleState::Restarting);

        let task_desc = TaskDescriptor {
            name: service.descriptor.name.clone(),
            capabilities: service.descriptor.capabilities.clone(),
        };
        let handle = kernel.spawn_task(task_desc)?;
        service.handle = ServiceHandle::new(handle.task_id, LifecycleState::Running);
        Ok(true)
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity::{ExitNotification, ExitReason};
    use sim_kernel::SimulatedKernel;

    struct TestExitSource {
        notifications: Vec<ExitNotification>,
    }

    impl TestExitSource {
        fn new(notifications: Vec<ExitNotification>) -> Self {
            Self { notifications }
        }
    }

    impl ExitNotificationSource for TestExitSource {
        fn drain_exit_notifications(&mut self) -> Vec<ExitNotification> {
            std::mem::take(&mut self.notifications)
        }
    }

    #[test]
    fn test_restart_on_failure_policy() {
        let mut kernel = SimulatedKernel::new();
        let mut manager = ProcessManager::new();

        let descriptor = ServiceDescriptor::new("svc".to_string(), RestartPolicy::OnFailure);
        let handle = manager
            .start_service(&mut kernel, descriptor.clone())
            .unwrap();

        let notification = ExitNotification {
            execution_id: identity::ExecutionId::new(),
            task_id: Some(handle.task_id),
            reason: ExitReason::Failure {
                error: "boom".to_string(),
            },
            terminated_at_nanos: 0,
        };

        let mut source = TestExitSource::new(vec![notification]);
        manager.handle_exits(&mut kernel, &mut source).unwrap();

        let updated = manager.service_handle(descriptor.service_id).unwrap();
        assert_eq!(updated.state, LifecycleState::Running);
        assert_ne!(updated.task_id, handle.task_id);
    }

    #[test]
    fn test_no_restart_on_normal_exit() {
        let mut kernel = SimulatedKernel::new();
        let mut manager = ProcessManager::new();

        let descriptor = ServiceDescriptor::new("svc".to_string(), RestartPolicy::OnFailure);
        let handle = manager
            .start_service(&mut kernel, descriptor.clone())
            .unwrap();

        let notification = ExitNotification {
            execution_id: identity::ExecutionId::new(),
            task_id: Some(handle.task_id),
            reason: ExitReason::Normal,
            terminated_at_nanos: 0,
        };

        let mut source = TestExitSource::new(vec![notification]);
        manager.handle_exits(&mut kernel, &mut source).unwrap();

        let updated = manager.service_handle(descriptor.service_id).unwrap();
        assert_eq!(updated.state, LifecycleState::Stopped);
        assert_eq!(updated.task_id, handle.task_id);
    }
}
