//! Capability Audit Trail
//!
//! This module provides audit logging for capability operations in test/simulation mode.
//! It tracks all capability lifecycle events for verification in tests.
//!
//! ## Philosophy
//!
//! - Test-only: This is NOT production logging, it's for test verification
//! - Deterministic: Events are recorded in order for reproducible tests
//! - Queryable: Tests can assert on audit trail to verify security properties
//!
//! ## Example
//!
//! ```
//! use sim_kernel::capability_audit::{CapabilityAuditLog, CapabilityAuditEvent};
//! use core_types::{TaskId, CapabilityEvent};
//! use kernel_api::Instant;
//!
//! let mut audit_log = CapabilityAuditLog::new();
//!
//! // Record an event
//! audit_log.record_event(
//!     Instant::from_nanos(1000),
//!     CapabilityEvent::Granted {
//!         cap_id: 42,
//!         grantor: None,
//!         grantee: TaskId::new(),
//!         cap_type: "FileRead".to_string(),
//!     }
//! );
//!
//! // Query events
//! let events = audit_log.get_events();
//! assert_eq!(events.len(), 1);
//! ```

use core_types::CapabilityEvent;
use kernel_api::Instant;

/// A single audit event with timestamp
#[derive(Debug, Clone)]
pub struct CapabilityAuditEvent {
    /// Simulated time when the event occurred
    pub timestamp: Instant,
    /// The capability event that occurred
    pub event: CapabilityEvent,
}

/// Audit log for capability operations
///
/// This maintains a chronological record of all capability events
/// for verification in tests.
#[derive(Debug, Default)]
pub struct CapabilityAuditLog {
    /// Chronological list of events
    events: Vec<CapabilityAuditEvent>,
}

impl CapabilityAuditLog {
    /// Creates a new empty audit log
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Records a capability event at the specified time
    pub fn record_event(&mut self, timestamp: Instant, event: CapabilityEvent) {
        self.events.push(CapabilityAuditEvent { timestamp, event });
    }

    /// Returns all recorded events
    pub fn get_events(&self) -> &[CapabilityAuditEvent] {
        &self.events
    }

    /// Returns events for a specific capability ID
    pub fn get_events_for_cap(&self, cap_id: u64) -> Vec<&CapabilityAuditEvent> {
        self.events
            .iter()
            .filter(|e| match &e.event {
                CapabilityEvent::Granted { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::Delegated { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::CrossDomainDelegation { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::Cloned { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::Dropped { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::InvalidUseAttempt { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::Invalidated { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::Revoked { cap_id: id, .. } => *id == cap_id,
                CapabilityEvent::LeaseExpired { cap_id: id, .. } => *id == cap_id,
            })
            .collect()
    }

    /// Counts events of a specific type
    pub fn count_events<F>(&self, predicate: F) -> usize
    where
        F: Fn(&CapabilityEvent) -> bool,
    {
        self.events.iter().filter(|e| predicate(&e.event)).count()
    }

    /// Checks if any event matches the predicate
    pub fn has_event<F>(&self, predicate: F) -> bool
    where
        F: Fn(&CapabilityEvent) -> bool,
    {
        self.events.iter().any(|e| predicate(&e.event))
    }

    /// Clears all events (useful for test reset)
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Returns the number of recorded events
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Checks if the audit log is empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::TaskId;

    #[test]
    fn test_audit_log_creation() {
        let log = CapabilityAuditLog::new();
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }

    #[test]
    fn test_record_event() {
        let mut log = CapabilityAuditLog::new();
        let task = TaskId::new();

        log.record_event(
            Instant::from_nanos(1000),
            CapabilityEvent::Granted {
                cap_id: 42,
                grantor: None,
                grantee: task,
                cap_type: "FileRead".to_string(),
            },
        );

        assert_eq!(log.len(), 1);
        assert!(!log.is_empty());
    }

    #[test]
    fn test_get_events() {
        let mut log = CapabilityAuditLog::new();
        let task = TaskId::new();

        log.record_event(
            Instant::from_nanos(1000),
            CapabilityEvent::Granted {
                cap_id: 42,
                grantor: None,
                grantee: task,
                cap_type: "FileRead".to_string(),
            },
        );

        let events = log.get_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].timestamp, Instant::from_nanos(1000));
    }

    #[test]
    fn test_get_events_for_cap() {
        let mut log = CapabilityAuditLog::new();
        let task = TaskId::new();

        // Add event for cap 42
        log.record_event(
            Instant::from_nanos(1000),
            CapabilityEvent::Granted {
                cap_id: 42,
                grantor: None,
                grantee: task,
                cap_type: "FileRead".to_string(),
            },
        );

        // Add event for cap 43
        log.record_event(
            Instant::from_nanos(2000),
            CapabilityEvent::Granted {
                cap_id: 43,
                grantor: None,
                grantee: task,
                cap_type: "FileWrite".to_string(),
            },
        );

        let events_42 = log.get_events_for_cap(42);
        assert_eq!(events_42.len(), 1);

        let events_43 = log.get_events_for_cap(43);
        assert_eq!(events_43.len(), 1);
    }

    #[test]
    fn test_count_events() {
        let mut log = CapabilityAuditLog::new();
        let task = TaskId::new();

        log.record_event(
            Instant::from_nanos(1000),
            CapabilityEvent::Granted {
                cap_id: 42,
                grantor: None,
                grantee: task,
                cap_type: "FileRead".to_string(),
            },
        );

        log.record_event(
            Instant::from_nanos(2000),
            CapabilityEvent::Delegated {
                cap_id: 42,
                from_task: task,
                to_task: TaskId::new(),
                cap_type: "FileRead".to_string(),
            },
        );

        let grant_count = log.count_events(|e| matches!(e, CapabilityEvent::Granted { .. }));
        assert_eq!(grant_count, 1);

        let delegate_count = log.count_events(|e| matches!(e, CapabilityEvent::Delegated { .. }));
        assert_eq!(delegate_count, 1);
    }

    #[test]
    fn test_has_event() {
        let mut log = CapabilityAuditLog::new();
        let task = TaskId::new();

        log.record_event(
            Instant::from_nanos(1000),
            CapabilityEvent::Granted {
                cap_id: 42,
                grantor: None,
                grantee: task,
                cap_type: "FileRead".to_string(),
            },
        );

        assert!(log.has_event(|e| matches!(e, CapabilityEvent::Granted { .. })));
        assert!(!log.has_event(|e| matches!(e, CapabilityEvent::Delegated { .. })));
    }

    #[test]
    fn test_clear() {
        let mut log = CapabilityAuditLog::new();
        let task = TaskId::new();

        log.record_event(
            Instant::from_nanos(1000),
            CapabilityEvent::Granted {
                cap_id: 42,
                grantor: None,
                grantee: task,
                cap_type: "FileRead".to_string(),
            },
        );

        assert_eq!(log.len(), 1);

        log.clear();
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }
}
