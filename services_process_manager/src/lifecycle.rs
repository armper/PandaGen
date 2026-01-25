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

    /// Returns a human-readable status string
    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleState::Starting => "Starting",
            LifecycleState::Running => "Running",
            LifecycleState::Stopping => "Stopping",
            LifecycleState::Stopped => "Stopped",
            LifecycleState::Failed => "Failed",
            LifecycleState::Restarting => "Restarting",
        }
    }
}

/// Reason why a service crashed or failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashReason {
    /// Service panicked with a message
    Panic(String),
    /// Service exited with non-zero code
    ExitCode(i32),
    /// Service was killed by signal
    Signal(String),
    /// Service exceeded resource limit
    ResourceLimit(String),
    /// Service timed out
    Timeout,
    /// Service had an unhandled error
    Error(String),
}

impl CrashReason {
    /// Returns a human-readable crash description
    pub fn description(&self) -> String {
        match self {
            CrashReason::Panic(msg) => format!("Panic: {}", msg),
            CrashReason::ExitCode(code) => format!("Exit code: {}", code),
            CrashReason::Signal(sig) => format!("Killed by signal: {}", sig),
            CrashReason::ResourceLimit(limit) => format!("Resource limit exceeded: {}", limit),
            CrashReason::Timeout => "Timeout".to_string(),
            CrashReason::Error(err) => format!("Error: {}", err),
        }
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
    /// Optional crash reason (if state is Failed)
    pub crash_reason: Option<CrashReason>,
    /// Number of restart attempts
    pub restart_count: u32,
}

impl ServiceHandle {
    /// Creates a new service handle
    pub fn new(task_id: TaskId, state: LifecycleState) -> Self {
        Self {
            task_id,
            state,
            crash_reason: None,
            restart_count: 0,
        }
    }

    /// Updates the service state
    pub fn set_state(&mut self, state: LifecycleState) {
        self.state = state;
    }

    /// Sets the crash reason (typically when transitioning to Failed state)
    pub fn set_crash_reason(&mut self, reason: CrashReason) {
        self.crash_reason = Some(reason);
        self.state = LifecycleState::Failed;
    }

    /// Increments the restart counter
    pub fn increment_restart_count(&mut self) {
        self.restart_count += 1;
    }

    /// Gets a human-readable status summary
    pub fn status_summary(&self) -> String {
        let mut summary = self.state.as_str().to_string();

        if let Some(ref reason) = self.crash_reason {
            summary.push_str(&format!(" ({})", reason.description()));
        }

        if self.restart_count > 0 {
            summary.push_str(&format!(" [restarts: {}]", self.restart_count));
        }

        summary
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

    #[test]
    fn test_crash_reason_description() {
        let panic_reason = CrashReason::Panic("Out of memory".to_string());
        assert_eq!(panic_reason.description(), "Panic: Out of memory");

        let exit_reason = CrashReason::ExitCode(1);
        assert_eq!(exit_reason.description(), "Exit code: 1");

        let timeout_reason = CrashReason::Timeout;
        assert_eq!(timeout_reason.description(), "Timeout");
    }

    #[test]
    fn test_service_handle_crash_reason() {
        let task_id = TaskId::new();
        let mut handle = ServiceHandle::new(task_id, LifecycleState::Running);

        let crash = CrashReason::Panic("Test panic".to_string());
        handle.set_crash_reason(crash.clone());

        assert_eq!(handle.state, LifecycleState::Failed);
        assert_eq!(handle.crash_reason, Some(crash));
    }

    #[test]
    fn test_service_handle_restart_count() {
        let task_id = TaskId::new();
        let mut handle = ServiceHandle::new(task_id, LifecycleState::Starting);

        assert_eq!(handle.restart_count, 0);
        handle.increment_restart_count();
        assert_eq!(handle.restart_count, 1);
        handle.increment_restart_count();
        assert_eq!(handle.restart_count, 2);
    }

    #[test]
    fn test_service_handle_status_summary() {
        let task_id = TaskId::new();
        let mut handle = ServiceHandle::new(task_id, LifecycleState::Running);

        assert_eq!(handle.status_summary(), "Running");

        handle.set_crash_reason(CrashReason::ExitCode(1));
        assert!(handle.status_summary().contains("Failed"));
        assert!(handle.status_summary().contains("Exit code: 1"));

        handle.increment_restart_count();
        assert!(handle.status_summary().contains("[restarts: 1]"));
    }

    #[test]
    fn test_lifecycle_state_as_str() {
        assert_eq!(LifecycleState::Starting.as_str(), "Starting");
        assert_eq!(LifecycleState::Running.as_str(), "Running");
        assert_eq!(LifecycleState::Failed.as_str(), "Failed");
    }
}
