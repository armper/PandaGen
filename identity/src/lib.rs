//! # Identity
//!
//! This crate provides execution identity primitives for PandaGen.
//!
//! ## Philosophy
//!
//! - **Identity is explicit and contextual, not global**
//! - **Authority comes from capabilities, not names**
//! - **Identity does NOT grant authority by itself**
//! - **Testability first; no hidden global state**
//!
//! ## Core Concepts
//!
//! - `ExecutionId`: Unique identifier for a running task/service
//! - `IdentityKind`: Type of execution (System, Service, Component, PipelineStage)
//! - `IdentityMetadata`: Parent, creator, creation time, trust domain
//! - `TrustDomain`: Isolation boundary for authority delegation
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - POSIX users, groups, or permissions
//! - Authentication or cryptography
//! - Access control lists (ACLs)
//! - Global policy engine

use core_types::TaskId;
use resources::{ResourceBudget, ResourceUsage};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for an execution context
///
/// Every running task/service has an ExecutionId that tracks its identity
/// for supervision and audit purposes. ExecutionIds are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionId(Uuid);

impl ExecutionId {
    /// Creates a new unique execution ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns the inner UUID value
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exec:{}", self.0)
    }
}

/// Type of execution context
///
/// Identity kind determines the role and capabilities of an execution.
/// This is structural information, not authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdentityKind {
    /// Core system component (kernel, scheduler)
    System,
    /// User-space service (storage, logger, registry)
    Service,
    /// Application component
    Component,
    /// Pipeline stage execution
    PipelineStage,
}

impl fmt::Display for IdentityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityKind::System => write!(f, "System"),
            IdentityKind::Service => write!(f, "Service"),
            IdentityKind::Component => write!(f, "Component"),
            IdentityKind::PipelineStage => write!(f, "PipelineStage"),
        }
    }
}

/// Trust domain tag
///
/// Trust domains define boundaries for capability delegation.
/// Delegation within a domain is implicit. Delegation across domains
/// requires explicit permission.
///
/// This is NOT a security enforcement mechanism yet - it's structural
/// information for supervision and audit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrustDomain(String);

impl TrustDomain {
    /// Creates a new trust domain with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Core system trust domain
    pub fn core() -> Self {
        Self("core".to_string())
    }

    /// User application trust domain
    pub fn user() -> Self {
        Self("user".to_string())
    }

    /// Sandboxed execution trust domain
    pub fn sandbox() -> Self {
        Self("sandbox".to_string())
    }

    /// Returns the domain name
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrustDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata associated with an execution identity
///
/// Identity metadata is immutable after creation. It provides lineage
/// information for supervision and audit.
///
/// **Phase 11 Addition**: Optional resource budget attachment for quota enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityMetadata {
    /// Unique execution identifier
    pub execution_id: ExecutionId,
    /// Type of execution
    pub kind: IdentityKind,
    /// Associated task ID (if any)
    pub task_id: Option<TaskId>,
    /// Parent execution (supervisor)
    pub parent_id: Option<ExecutionId>,
    /// Creator execution (who spawned this)
    pub creator_id: Option<ExecutionId>,
    /// Time of creation (nanoseconds since epoch)
    pub created_at_nanos: u64,
    /// Trust domain
    pub trust_domain: TrustDomain,
    /// Human-readable name (for debugging/logging)
    pub name: String,
    /// Optional resource budget (Phase 11)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<ResourceBudget>,
    /// Current resource usage (Phase 11)
    /// Note: Defaults to zero on deserialization. This is intentional because
    /// usage is runtime state that should not be persisted. If identity is
    /// serialized and deserialized, usage should reset to zero.
    #[serde(default)]
    pub usage: ResourceUsage,
}

impl IdentityMetadata {
    /// Creates a new identity metadata
    pub fn new(
        kind: IdentityKind,
        trust_domain: TrustDomain,
        name: impl Into<String>,
        created_at_nanos: u64,
    ) -> Self {
        Self {
            execution_id: ExecutionId::new(),
            kind,
            task_id: None,
            parent_id: None,
            creator_id: None,
            created_at_nanos,
            trust_domain,
            name: name.into(),
            budget: None,
            usage: ResourceUsage::zero(),
        }
    }

    /// Sets the task ID (builder pattern)
    pub fn with_task_id(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }

    /// Sets the parent ID (builder pattern)
    pub fn with_parent(mut self, parent_id: ExecutionId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Sets the creator ID (builder pattern)
    pub fn with_creator(mut self, creator_id: ExecutionId) -> Self {
        self.creator_id = Some(creator_id);
        self
    }

    /// Checks if this identity is in the same trust domain as another
    pub fn same_domain(&self, other: &IdentityMetadata) -> bool {
        self.trust_domain == other.trust_domain
    }

    /// Checks if this identity is a child of the given parent
    pub fn is_child_of(&self, parent_id: ExecutionId) -> bool {
        self.parent_id == Some(parent_id)
    }

    /// Sets the resource budget (builder pattern)
    ///
    /// Phase 11: Attaches a resource budget to this identity.
    /// Budget inheritance rules:
    /// - Child budget must be ≤ parent budget (validated at spawn time)
    /// - Budget is scoped to identity lifetime
    pub fn with_budget(mut self, budget: ResourceBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Checks if identity has a budget attached
    pub fn has_budget(&self) -> bool {
        self.budget.is_some()
    }

    /// Validates budget inheritance (child ≤ parent)
    ///
    /// Returns true if this identity's budget is a subset of the parent's budget,
    /// or if either has no budget (no constraint).
    pub fn budget_inherits_from(&self, parent: &IdentityMetadata) -> bool {
        match (&self.budget, &parent.budget) {
            (Some(child_budget), Some(parent_budget)) => child_budget.is_subset_of(parent_budget),
            _ => true, // No constraint if either has no budget
        }
    }
}

/// Exit reason for an execution
///
/// Structured information about why an execution terminated.
/// Used for supervision decisions and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitReason {
    /// Normal successful exit
    Normal,
    /// Failed with error
    Failure { error: String },
    /// Cancelled by supervisor or user
    Cancelled { reason: String },
    /// Timed out
    Timeout,
}

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitReason::Normal => write!(f, "normal exit"),
            ExitReason::Failure { error } => write!(f, "failed: {}", error),
            ExitReason::Cancelled { reason } => write!(f, "cancelled: {}", reason),
            ExitReason::Timeout => write!(f, "timeout"),
        }
    }
}

/// Exit notification
///
/// Sent from kernel to supervisor when a child execution terminates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitNotification {
    /// Execution that terminated
    pub execution_id: ExecutionId,
    /// Task ID (if applicable)
    pub task_id: Option<TaskId>,
    /// Why it terminated
    pub reason: ExitReason,
    /// When it terminated (nanoseconds since epoch)
    pub terminated_at_nanos: u64,
}

/// Identity-related errors
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("Identity not found: {0}")]
    NotFound(ExecutionId),

    #[error("Identity already exists: {0}")]
    AlreadyExists(ExecutionId),

    #[error("Parent identity not found: {0}")]
    ParentNotFound(ExecutionId),

    #[error("Supervisor mismatch: {child} is not supervised by {supervisor}")]
    SupervisorMismatch {
        child: ExecutionId,
        supervisor: ExecutionId,
    },

    #[error("Cannot control unrelated identity: {0}")]
    UnrelatedIdentity(ExecutionId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_id_unique() {
        let id1 = ExecutionId::new();
        let id2 = ExecutionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_execution_id_display() {
        let id = ExecutionId::new();
        let display = format!("{}", id);
        assert!(display.starts_with("exec:"));
    }

    #[test]
    fn test_identity_kind() {
        assert_eq!(IdentityKind::System.to_string(), "System");
        assert_eq!(IdentityKind::Service.to_string(), "Service");
        assert_eq!(IdentityKind::Component.to_string(), "Component");
        assert_eq!(IdentityKind::PipelineStage.to_string(), "PipelineStage");
    }

    #[test]
    fn test_trust_domain_predefined() {
        let core = TrustDomain::core();
        let user = TrustDomain::user();
        let sandbox = TrustDomain::sandbox();

        assert_eq!(core.name(), "core");
        assert_eq!(user.name(), "user");
        assert_eq!(sandbox.name(), "sandbox");

        assert_ne!(core, user);
        assert_ne!(user, sandbox);
    }

    #[test]
    fn test_trust_domain_custom() {
        let custom = TrustDomain::new("custom-domain");
        assert_eq!(custom.name(), "custom-domain");
    }

    #[test]
    fn test_identity_metadata_creation() {
        let now = 1000u64;
        let metadata = IdentityMetadata::new(
            IdentityKind::Service,
            TrustDomain::core(),
            "test-service",
            now,
        );

        assert_eq!(metadata.kind, IdentityKind::Service);
        assert_eq!(metadata.trust_domain, TrustDomain::core());
        assert_eq!(metadata.name, "test-service");
        assert_eq!(metadata.created_at_nanos, now);
        assert_eq!(metadata.task_id, None);
        assert_eq!(metadata.parent_id, None);
        assert_eq!(metadata.creator_id, None);
    }

    #[test]
    fn test_identity_metadata_with_builder() {
        let now = 1000u64;
        let task_id = TaskId::new();
        let parent_id = ExecutionId::new();
        let creator_id = ExecutionId::new();

        let metadata = IdentityMetadata::new(
            IdentityKind::Component,
            TrustDomain::user(),
            "test-component",
            now,
        )
        .with_task_id(task_id)
        .with_parent(parent_id)
        .with_creator(creator_id);

        assert_eq!(metadata.task_id, Some(task_id));
        assert_eq!(metadata.parent_id, Some(parent_id));
        assert_eq!(metadata.creator_id, Some(creator_id));
    }

    #[test]
    fn test_identity_metadata_immutability() {
        let now = 1000u64;
        let metadata = IdentityMetadata::new(
            IdentityKind::Service,
            TrustDomain::core(),
            "test-service",
            now,
        );

        // Clone to simulate "modification attempt"
        let metadata2 = metadata.clone();
        assert_eq!(metadata.execution_id, metadata2.execution_id);
        assert_eq!(metadata.created_at_nanos, metadata2.created_at_nanos);
    }

    #[test]
    fn test_same_domain_check() {
        let now = 1000u64;
        let id1 =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service1", now);
        let id2 =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service2", now);
        let id3 = IdentityMetadata::new(
            IdentityKind::Component,
            TrustDomain::user(),
            "component",
            now,
        );

        assert!(id1.same_domain(&id2));
        assert!(!id1.same_domain(&id3));
        assert!(!id2.same_domain(&id3));
    }

    #[test]
    fn test_parent_child_relationship() {
        let now = 1000u64;
        let parent_id = ExecutionId::new();

        let parent =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "parent", now)
                .with_task_id(TaskId::new());

        let child =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::core(), "child", now)
                .with_parent(parent.execution_id);

        assert!(child.is_child_of(parent.execution_id));
        assert!(!parent.is_child_of(child.execution_id));
        assert!(!child.is_child_of(parent_id)); // Different parent
    }

    #[test]
    fn test_exit_reason_display() {
        assert_eq!(ExitReason::Normal.to_string(), "normal exit");
        assert_eq!(
            ExitReason::Failure {
                error: "test error".to_string()
            }
            .to_string(),
            "failed: test error"
        );
        assert_eq!(
            ExitReason::Cancelled {
                reason: "user cancelled".to_string()
            }
            .to_string(),
            "cancelled: user cancelled"
        );
        assert_eq!(ExitReason::Timeout.to_string(), "timeout");
    }

    #[test]
    fn test_exit_notification() {
        let now = 1000u64;
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();

        let notification = ExitNotification {
            execution_id: exec_id,
            task_id: Some(task_id),
            reason: ExitReason::Normal,
            terminated_at_nanos: now,
        };

        assert_eq!(notification.execution_id, exec_id);
        assert_eq!(notification.task_id, Some(task_id));
        assert_eq!(notification.reason, ExitReason::Normal);
        assert_eq!(notification.terminated_at_nanos, now);
    }

    // ============================================================================
    // Phase 11: Resource Budget Tests
    // ============================================================================

    #[test]
    fn test_identity_with_budget() {
        use resources::{CpuTicks, MessageCount, ResourceBudget};

        let now = 1000u64;
        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000))
            .with_message_count(MessageCount::new(50));

        let identity = IdentityMetadata::new(
            IdentityKind::Component,
            TrustDomain::user(),
            "test-component",
            now,
        )
        .with_budget(budget);

        assert!(identity.has_budget());
        assert_eq!(identity.budget, Some(budget));
        assert!(identity.usage.cpu_ticks.is_zero());
    }

    #[test]
    fn test_identity_without_budget() {
        let now = 1000u64;
        let identity =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::user(), "test", now);

        assert!(!identity.has_budget());
        assert_eq!(identity.budget, None);
    }

    #[test]
    fn test_budget_inheritance_valid() {
        use resources::{CpuTicks, ResourceBudget};

        let now = 1000u64;
        let parent_budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(1000));

        let parent =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "parent", now)
                .with_budget(parent_budget);

        // Child with smaller budget - valid
        let child_budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(500));
        let child =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::core(), "child", now)
                .with_parent(parent.execution_id)
                .with_budget(child_budget);

        assert!(child.budget_inherits_from(&parent));
    }

    #[test]
    fn test_budget_inheritance_equal() {
        use resources::{CpuTicks, ResourceBudget};

        let now = 1000u64;
        let budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(1000));

        let parent =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "parent", now)
                .with_budget(budget);

        // Child with equal budget - valid
        let child =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::core(), "child", now)
                .with_parent(parent.execution_id)
                .with_budget(budget);

        assert!(child.budget_inherits_from(&parent));
    }

    #[test]
    fn test_budget_inheritance_violates() {
        use resources::{CpuTicks, ResourceBudget};

        let now = 1000u64;
        let parent_budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(500));

        let parent =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "parent", now)
                .with_budget(parent_budget);

        // Child with larger budget - invalid
        let child_budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(1000));
        let child =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::core(), "child", now)
                .with_parent(parent.execution_id)
                .with_budget(child_budget);

        assert!(!child.budget_inherits_from(&parent));
    }

    #[test]
    fn test_budget_inheritance_no_parent_budget() {
        use resources::{CpuTicks, ResourceBudget};

        let now = 1000u64;
        let parent =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "parent", now);

        // Child has budget, parent doesn't - valid (no constraint)
        let child_budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(1000));
        let child =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::core(), "child", now)
                .with_parent(parent.execution_id)
                .with_budget(child_budget);

        assert!(child.budget_inherits_from(&parent));
    }

    #[test]
    fn test_budget_inheritance_no_child_budget() {
        use resources::{CpuTicks, ResourceBudget};

        let now = 1000u64;
        let parent_budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(1000));

        let parent =
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "parent", now)
                .with_budget(parent_budget);

        // Child has no budget - valid (no constraint)
        let child =
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::core(), "child", now)
                .with_parent(parent.execution_id);

        assert!(child.budget_inherits_from(&parent));
    }
}
