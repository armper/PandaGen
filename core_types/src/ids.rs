//! Unique identifiers for system entities

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a service
///
/// Services are long-lived system components that provide functionality
/// to other parts of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceId(Uuid);

impl ServiceId {
    /// Creates a new random service ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a service ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ServiceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Service({})", self.0)
    }
}

/// Unique identifier for a task
///
/// Tasks are individual units of execution. They may be short-lived or long-lived.
/// Unlike traditional processes, tasks are constructed explicitly with their
/// required capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(Uuid);

impl TaskId {
    /// Creates a new random task ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a task ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Task({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_id_creation() {
        let id1 = ServiceId::new();
        let id2 = ServiceId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_service_id_from_uuid() {
        let uuid = Uuid::new_v4();
        let id = ServiceId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn test_task_id_creation() {
        let id1 = TaskId::new();
        let id2 = TaskId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_task_id_from_uuid() {
        let uuid = Uuid::new_v4();
        let id = TaskId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn test_service_id_display() {
        let id = ServiceId::new();
        let display = format!("{}", id);
        assert!(display.starts_with("Service("));
    }

    #[test]
    fn test_task_id_display() {
        let id = TaskId::new();
        let display = format!("{}", id);
        assert!(display.starts_with("Task("));
    }
}
