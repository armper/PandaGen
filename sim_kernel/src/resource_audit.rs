//! Resource consumption audit log
//!
//! Phase 12: Tracks resource consumption and exhaustion events for testing.
//!
//! This module provides test-visible auditing of resource operations:
//! - Consumption events (message send/receive, CPU ticks, storage ops, pipeline stages)
//! - Exhaustion events (when budget limits are reached)
//! - Cancellation due to exhaustion
//!
//! Audit logs are deterministic and queryable in tests but do not affect
//! correctness or enforcement logic.

use identity::ExecutionId;
use kernel_api::Instant;
use serde::{Deserialize, Serialize};

/// Resource consumption event
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceEvent {
    /// Message consumption (send or receive)
    MessageConsumed {
        execution_id: ExecutionId,
        operation: MessageOperation,
        before: u64,
        after: u64,
    },

    /// CPU ticks consumed
    CpuConsumed {
        execution_id: ExecutionId,
        amount: u64,
        before: u64,
        after: u64,
    },

    /// Storage operation consumed
    StorageOpConsumed {
        execution_id: ExecutionId,
        operation: StorageOperation,
        before: u64,
        after: u64,
    },

    /// Pipeline stage consumed
    PipelineStageConsumed {
        execution_id: ExecutionId,
        stage_name: String,
        before: u64,
        after: u64,
    },

    /// Resource budget exhausted
    BudgetExhausted {
        execution_id: ExecutionId,
        resource_type: String,
        limit: u64,
        attempted_usage: u64,
        operation: String,
    },

    /// Cancellation triggered by exhaustion
    CancelledDueToExhaustion {
        execution_id: ExecutionId,
        resource_type: String,
    },
}

/// Type of message operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageOperation {
    Send,
    Receive,
}

/// Type of storage operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageOperation {
    Write,
    Commit,
}

/// Audit entry with timestamp
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceAuditEntry {
    pub timestamp: Instant,
    pub event: ResourceEvent,
}

/// Resource audit log
///
/// Test-visible log of all resource consumption and exhaustion events.
pub struct ResourceAuditLog {
    entries: Vec<ResourceAuditEntry>,
}

impl ResourceAuditLog {
    /// Creates a new empty audit log
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Records a resource event
    pub fn record_event(&mut self, timestamp: Instant, event: ResourceEvent) {
        self.entries.push(ResourceAuditEntry { timestamp, event });
    }

    /// Returns the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Checks if the log is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns all entries
    pub fn get_entries(&self) -> &[ResourceAuditEntry] {
        &self.entries
    }

    /// Checks if any entry matches a predicate
    pub fn has_event<F>(&self, predicate: F) -> bool
    where
        F: Fn(&ResourceEvent) -> bool,
    {
        self.entries.iter().any(|entry| predicate(&entry.event))
    }

    /// Counts events matching a predicate
    pub fn count_events<F>(&self, predicate: F) -> usize
    where
        F: Fn(&ResourceEvent) -> bool,
    {
        self.entries
            .iter()
            .filter(|entry| predicate(&entry.event))
            .count()
    }

    /// Finds entries for a specific execution ID
    pub fn entries_for_execution(&self, execution_id: ExecutionId) -> Vec<&ResourceAuditEntry> {
        self.entries
            .iter()
            .filter(|entry| match &entry.event {
                ResourceEvent::MessageConsumed { execution_id: eid, .. }
                | ResourceEvent::CpuConsumed { execution_id: eid, .. }
                | ResourceEvent::StorageOpConsumed { execution_id: eid, .. }
                | ResourceEvent::PipelineStageConsumed { execution_id: eid, .. }
                | ResourceEvent::BudgetExhausted { execution_id: eid, .. }
                | ResourceEvent::CancelledDueToExhaustion { execution_id: eid, .. } => {
                    eid == &execution_id
                }
            })
            .collect()
    }

    /// Clears all entries
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for ResourceAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_creation() {
        let log = ResourceAuditLog::new();
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }

    #[test]
    fn test_record_message_consumed() {
        let mut log = ResourceAuditLog::new();
        let exec_id = ExecutionId::new();
        let timestamp = Instant::from_nanos(1000);

        log.record_event(
            timestamp,
            ResourceEvent::MessageConsumed {
                execution_id: exec_id,
                operation: MessageOperation::Send,
                before: 0,
                after: 1,
            },
        );

        assert_eq!(log.len(), 1);
        assert!(log.has_event(|e| matches!(e, ResourceEvent::MessageConsumed { .. })));
    }

    #[test]
    fn test_record_budget_exhausted() {
        let mut log = ResourceAuditLog::new();
        let exec_id = ExecutionId::new();
        let timestamp = Instant::from_nanos(1000);

        log.record_event(
            timestamp,
            ResourceEvent::BudgetExhausted {
                execution_id: exec_id,
                resource_type: "MessageCount".to_string(),
                limit: 10,
                attempted_usage: 11,
                operation: "send_message".to_string(),
            },
        );

        assert_eq!(log.len(), 1);
        assert!(log.has_event(|e| matches!(e, ResourceEvent::BudgetExhausted { .. })));
    }

    #[test]
    fn test_count_events() {
        let mut log = ResourceAuditLog::new();
        let exec_id = ExecutionId::new();
        let timestamp = Instant::from_nanos(1000);

        log.record_event(
            timestamp,
            ResourceEvent::MessageConsumed {
                execution_id: exec_id,
                operation: MessageOperation::Send,
                before: 0,
                after: 1,
            },
        );

        log.record_event(
            timestamp,
            ResourceEvent::MessageConsumed {
                execution_id: exec_id,
                operation: MessageOperation::Send,
                before: 1,
                after: 2,
            },
        );

        log.record_event(
            timestamp,
            ResourceEvent::CpuConsumed {
                execution_id: exec_id,
                amount: 10,
                before: 0,
                after: 10,
            },
        );

        assert_eq!(
            log.count_events(|e| matches!(e, ResourceEvent::MessageConsumed { .. })),
            2
        );
        assert_eq!(
            log.count_events(|e| matches!(e, ResourceEvent::CpuConsumed { .. })),
            1
        );
    }

    #[test]
    fn test_entries_for_execution() {
        let mut log = ResourceAuditLog::new();
        let exec_id1 = ExecutionId::new();
        let exec_id2 = ExecutionId::new();
        let timestamp = Instant::from_nanos(1000);

        log.record_event(
            timestamp,
            ResourceEvent::MessageConsumed {
                execution_id: exec_id1,
                operation: MessageOperation::Send,
                before: 0,
                after: 1,
            },
        );

        log.record_event(
            timestamp,
            ResourceEvent::CpuConsumed {
                execution_id: exec_id2,
                amount: 10,
                before: 0,
                after: 10,
            },
        );

        let entries_1 = log.entries_for_execution(exec_id1);
        let entries_2 = log.entries_for_execution(exec_id2);

        assert_eq!(entries_1.len(), 1);
        assert_eq!(entries_2.len(), 1);
    }
}
