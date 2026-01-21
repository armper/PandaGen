//! # Process Information and Control
//!
//! This module provides user-facing commands for process management (ps, kill).

use crate::{LifecycleState, RestartPolicy, ServiceHandle};
use core_types::{ServiceId, TaskId};
use std::collections::HashMap;
use std::fmt;

/// Information about a running process/service for display
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Service ID
    pub service_id: ServiceId,
    /// Task ID
    pub task_id: TaskId,
    /// Service name
    pub name: String,
    /// Current state
    pub state: LifecycleState,
    /// Status summary (includes crash reason, restart count)
    pub status: String,
    /// Restart policy
    pub restart_policy: RestartPolicy,
}

impl ProcessInfo {
    /// Creates process info from service descriptor and handle
    pub fn new(
        service_id: ServiceId,
        name: String,
        handle: &ServiceHandle,
        restart_policy: RestartPolicy,
    ) -> Self {
        Self {
            service_id,
            task_id: handle.task_id,
            name,
            state: handle.state,
            status: handle.status_summary(),
            restart_policy,
        }
    }
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<36} {:<20} {:<15} {}",
            format!("{}", self.service_id),
            self.name,
            self.state.as_str(),
            self.status
        )
    }
}

/// Process listing manager (ps command implementation)
pub struct ProcessList {
    /// Map of service ID to process info
    processes: HashMap<ServiceId, ProcessInfo>,
}

impl ProcessList {
    /// Creates a new process list
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
        }
    }

    /// Adds a process to the list
    pub fn add(&mut self, service_id: ServiceId, info: ProcessInfo) {
        self.processes.insert(service_id, info);
    }

    /// Removes a process from the list
    pub fn remove(&mut self, service_id: &ServiceId) -> Option<ProcessInfo> {
        self.processes.remove(service_id)
    }

    /// Updates process info
    pub fn update(&mut self, service_id: ServiceId, info: ProcessInfo) {
        self.processes.insert(service_id, info);
    }

    /// Lists all processes
    pub fn list_all(&self) -> Vec<&ProcessInfo> {
        let mut processes: Vec<_> = self.processes.values().collect();
        processes.sort_by(|a, b| a.name.cmp(&b.name));
        processes
    }

    /// Lists processes by state
    pub fn list_by_state(&self, state: LifecycleState) -> Vec<&ProcessInfo> {
        let mut processes: Vec<_> = self
            .processes
            .values()
            .filter(|p| p.state == state)
            .collect();
        processes.sort_by(|a, b| a.name.cmp(&b.name));
        processes
    }

    /// Gets process info by service ID
    pub fn get(&self, service_id: &ServiceId) -> Option<&ProcessInfo> {
        self.processes.get(service_id)
    }

    /// Gets process info by name
    pub fn get_by_name(&self, name: &str) -> Option<&ProcessInfo> {
        self.processes.values().find(|p| p.name == name)
    }

    /// Formats the process list as a table
    pub fn format_table(&self) -> String {
        let mut output = String::new();
        output.push_str("SERVICE ID                           NAME                 STATE           STATUS\n");
        output.push_str("─".repeat(100).as_str());
        output.push('\n');

        for info in self.list_all() {
            output.push_str(&format!("{}\n", info));
        }

        output
    }
}

impl Default for ProcessList {
    fn default() -> Self {
        Self::new()
    }
}

/// Kill signal types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSignal {
    /// Graceful shutdown (SIGTERM equivalent)
    Terminate,
    /// Force kill (SIGKILL equivalent)
    Kill,
    /// Interrupt (SIGINT equivalent)
    Interrupt,
}

impl KillSignal {
    /// Returns the signal name
    pub fn name(&self) -> &'static str {
        match self {
            KillSignal::Terminate => "TERM",
            KillSignal::Kill => "KILL",
            KillSignal::Interrupt => "INT",
        }
    }

    /// Checks if this is a forceful kill
    pub fn is_forceful(&self) -> bool {
        matches!(self, KillSignal::Kill)
    }
}

/// Result of a kill operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillResult {
    /// Process was successfully signaled
    Success { service_id: ServiceId, signal: KillSignal },
    /// Process not found
    NotFound { service_id: ServiceId },
    /// Process already stopped
    AlreadyStopped { service_id: ServiceId },
    /// Kill failed with reason
    Failed { service_id: ServiceId, reason: String },
}

impl fmt::Display for KillResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KillResult::Success { service_id, signal } => {
                write!(f, "Sent {} to service {}", signal.name(), service_id)
            }
            KillResult::NotFound { service_id } => {
                write!(f, "Service {} not found", service_id)
            }
            KillResult::AlreadyStopped { service_id } => {
                write!(f, "Service {} is already stopped", service_id)
            }
            KillResult::Failed { service_id, reason } => {
                write!(f, "Failed to kill service {}: {}", service_id, reason)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_info_creation() {
        let service_id = ServiceId::new();
        let task_id = TaskId::new();
        let handle = ServiceHandle::new(task_id, LifecycleState::Running);
        
        let info = ProcessInfo::new(
            service_id,
            "test-service".to_string(),
            &handle,
            RestartPolicy::Always,
        );

        assert_eq!(info.name, "test-service");
        assert_eq!(info.state, LifecycleState::Running);
        assert_eq!(info.restart_policy, RestartPolicy::Always);
    }

    #[test]
    fn test_process_list_add_remove() {
        let mut list = ProcessList::new();
        let service_id = ServiceId::new();
        let task_id = TaskId::new();
        let handle = ServiceHandle::new(task_id, LifecycleState::Running);
        
        let info = ProcessInfo::new(
            service_id,
            "test".to_string(),
            &handle,
            RestartPolicy::Never,
        );

        list.add(service_id, info.clone());
        assert_eq!(list.list_all().len(), 1);

        let removed = list.remove(&service_id);
        assert!(removed.is_some());
        assert_eq!(list.list_all().len(), 0);
    }

    #[test]
    fn test_process_list_by_state() {
        let mut list = ProcessList::new();
        
        for i in 0..5 {
            let service_id = ServiceId::new();
            let task_id = TaskId::new();
            let state = if i % 2 == 0 {
                LifecycleState::Running
            } else {
                LifecycleState::Stopped
            };
            let handle = ServiceHandle::new(task_id, state);
            
            let info = ProcessInfo::new(
                service_id,
                format!("service-{}", i),
                &handle,
                RestartPolicy::Always,
            );
            list.add(service_id, info);
        }

        let running = list.list_by_state(LifecycleState::Running);
        let stopped = list.list_by_state(LifecycleState::Stopped);
        
        assert_eq!(running.len(), 3); // 0, 2, 4
        assert_eq!(stopped.len(), 2); // 1, 3
    }

    #[test]
    fn test_process_list_get_by_name() {
        let mut list = ProcessList::new();
        let service_id = ServiceId::new();
        let task_id = TaskId::new();
        let handle = ServiceHandle::new(task_id, LifecycleState::Running);
        
        let info = ProcessInfo::new(
            service_id,
            "my-service".to_string(),
            &handle,
            RestartPolicy::OnFailure,
        );

        list.add(service_id, info);

        let found = list.get_by_name("my-service");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "my-service");

        let not_found = list.get_by_name("other-service");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_kill_signal() {
        assert_eq!(KillSignal::Terminate.name(), "TERM");
        assert_eq!(KillSignal::Kill.name(), "KILL");
        assert_eq!(KillSignal::Interrupt.name(), "INT");

        assert!(!KillSignal::Terminate.is_forceful());
        assert!(KillSignal::Kill.is_forceful());
        assert!(!KillSignal::Interrupt.is_forceful());
    }

    #[test]
    fn test_kill_result_display() {
        let service_id = ServiceId::new();
        
        let success = KillResult::Success {
            service_id,
            signal: KillSignal::Terminate,
        };
        assert!(format!("{}", success).contains("Sent TERM"));

        let not_found = KillResult::NotFound { service_id };
        assert!(format!("{}", not_found).contains("not found"));

        let already_stopped = KillResult::AlreadyStopped { service_id };
        assert!(format!("{}", already_stopped).contains("already stopped"));
    }

    #[test]
    fn test_process_list_format_table() {
        let mut list = ProcessList::new();
        let service_id = ServiceId::new();
        let task_id = TaskId::new();
        let handle = ServiceHandle::new(task_id, LifecycleState::Running);
        
        let info = ProcessInfo::new(
            service_id,
            "test".to_string(),
            &handle,
            RestartPolicy::Always,
        );

        list.add(service_id, info);

        let table = list.format_table();
        assert!(table.contains("SERVICE ID"));
        assert!(table.contains("NAME"));
        assert!(table.contains("STATE"));
        assert!(table.contains("test"));
    }
}
