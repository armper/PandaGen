//! Policy Audit Log
//!
//! Records policy decisions for test verification and debugging.
//! This is NOT for production observability - it's for proving policy correctness.

use kernel_api::Instant;
use policy::{PolicyDecision, PolicyEvent};
use serde::{Deserialize, Serialize};

/// Policy decision audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAuditEvent {
    /// When the decision was made (simulated time)
    pub timestamp: Instant,
    /// Which policy event was evaluated
    pub event: PolicyEvent,
    /// Name of the policy engine that made this decision
    pub policy_name: String,
    /// The decision that was made
    pub decision: PolicyDecision,
    /// Optional context information
    pub context_summary: String,
}

/// Policy audit log for testing
///
/// Records all policy decisions made during execution.
/// Used to verify policy behavior in tests.
#[derive(Debug, Clone)]
pub struct PolicyAuditLog {
    events: Vec<PolicyAuditEvent>,
}

impl PolicyAuditLog {
    /// Creates a new empty audit log
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Records a policy decision
    pub fn record_decision(
        &mut self,
        timestamp: Instant,
        event: PolicyEvent,
        policy_name: String,
        decision: PolicyDecision,
        context_summary: String,
    ) {
        self.events.push(PolicyAuditEvent {
            timestamp,
            event,
            policy_name,
            decision,
            context_summary,
        });
    }

    /// Returns all recorded events
    pub fn events(&self) -> &[PolicyAuditEvent] {
        &self.events
    }

    /// Clears all recorded events
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Returns events matching a predicate
    pub fn find_events<F>(&self, predicate: F) -> Vec<&PolicyAuditEvent>
    where
        F: Fn(&PolicyAuditEvent) -> bool,
    {
        self.events.iter().filter(|e| predicate(e)).collect()
    }

    /// Checks if any event matches a predicate
    pub fn has_event<F>(&self, predicate: F) -> bool
    where
        F: Fn(&PolicyAuditEvent) -> bool,
    {
        self.events.iter().any(predicate)
    }

    /// Counts events matching a predicate
    pub fn count_events<F>(&self, predicate: F) -> usize
    where
        F: Fn(&PolicyAuditEvent) -> bool,
    {
        self.events.iter().filter(|e| predicate(e)).count()
    }
}

impl Default for PolicyAuditLog {
    fn default() -> Self {
        Self::new()
    }
}
