//! Service lifecycle management

use core_types::TaskId;

/// Lifecycle states for a service
///
/// Unlike Unix processes (running/stopped), we have explicit states
/// that reflect the service's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// Service is starting up
    Starting,
    /// Service is running normally
    Running,
    /// Service is shutting down gracefully
    Stopping,
    /// Service has stopped
    Stopped,
    /// Service has failed
    Failed,
    /// Service is waiting to restart
    Restarting,
}

impl LifecycleState {
    /// Checks if the service is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, LifecycleState::Stopped | LifecycleState::Failed)
    }

    /// Checks if the service is active
    pub fn is_active(&self) -> bool {
        matches!(self, LifecycleState::Starting | LifecycleState::Running)
    }
}

/// Handle to a managed service
///
/// This provides control over a service's lifecycle.
#[derive(Debug, Clone)]
pub struct ServiceHandle {
    /// The task running the service
    pub task_id: TaskId,
    /// Current lifecycle state
    pub state: LifecycleState,
}

impl ServiceHandle {
    /// Creates a new service handle
    pub fn new(task_id: TaskId, state: LifecycleState) -> Self {
        Self { task_id, state }
    }

    /// Updates the service state
    pub fn set_state(&mut self, state: LifecycleState) {
        self.state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_states() {
        assert!(LifecycleState::Stopped.is_terminal());
        assert!(LifecycleState::Failed.is_terminal());
        assert!(!LifecycleState::Running.is_terminal());

        assert!(LifecycleState::Running.is_active());
        assert!(LifecycleState::Starting.is_active());
        assert!(!LifecycleState::Stopped.is_active());
    }

    #[test]
    fn test_service_handle_creation() {
        let task_id = TaskId::new();
        let handle = ServiceHandle::new(task_id, LifecycleState::Starting);

        assert_eq!(handle.task_id, task_id);
        assert_eq!(handle.state, LifecycleState::Starting);
    }

    #[test]
    fn test_service_handle_state_change() {
        let task_id = TaskId::new();
        let mut handle = ServiceHandle::new(task_id, LifecycleState::Starting);

        handle.set_state(LifecycleState::Running);
        assert_eq!(handle.state, LifecycleState::Running);

        handle.set_state(LifecycleState::Stopping);
        assert_eq!(handle.state, LifecycleState::Stopping);
    }
}
