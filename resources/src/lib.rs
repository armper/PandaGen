//! # Resources
//!
//! This crate provides resource budget primitives for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Resources are finite and must be explicit**
//! - **Budgets are enforced, not advisory**
//! - **Accounting is deterministic and testable**
//! - **Policy may require or limit resources, but does not implement accounting**
//! - **No POSIX concepts** (no ulimits, no nice, no cgroups)
//! - **Simulation-first** (no real hardware yet)
//!
//! ## Core Concepts
//!
//! - Resource types: CpuTicks, MemoryUnits, MessageCount, StorageOps, PipelineStages
//! - `ResourceBudget`: Immutable limits for resources
//! - `ResourceUsage`: Current consumption
//! - `ResourceDelta`: Changes to resource consumption
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - Real scheduling or preemption
//! - Async runtimes
//! - POSIX-style limits (ulimits, nice, cgroups)
//! - Global mutable counters
//!
//! Everything must run under SimKernel, be deterministic, and be auditable.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// CPU ticks (simulated execution steps)
///
/// Represents abstract computational work units, not real CPU cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CpuTicks(pub u64);

impl CpuTicks {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn saturating_add(&self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for CpuTicks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} CPU ticks", self.0)
    }
}

/// Memory units (abstract units, not bytes)
///
/// Represents abstract memory allocation units, not real memory addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MemoryUnits(pub u64);

impl MemoryUnits {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn saturating_add(&self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for MemoryUnits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} memory units", self.0)
    }
}

/// Message count
///
/// Tracks number of messages sent/received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MessageCount(pub u64);

impl MessageCount {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn saturating_add(&self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for MessageCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} messages", self.0)
    }
}

/// Storage operations count
///
/// Tracks number of storage read/write operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StorageOps(pub u64);

impl StorageOps {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn saturating_add(&self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for StorageOps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} storage ops", self.0)
    }
}

/// Pipeline stages count
///
/// Tracks number of pipeline stages executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PipelineStages(pub u64);

impl PipelineStages {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn saturating_add(&self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for PipelineStages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} pipeline stages", self.0)
    }
}

/// Resource budget
///
/// Immutable limits for resource consumption. Once created, cannot be modified
/// except by replacing with a new budget (subject to policy validation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceBudget {
    pub cpu_ticks: Option<CpuTicks>,
    pub memory_units: Option<MemoryUnits>,
    pub message_count: Option<MessageCount>,
    pub storage_ops: Option<StorageOps>,
    pub pipeline_stages: Option<PipelineStages>,
}

impl ResourceBudget {
    /// Creates a new empty budget (no limits)
    pub fn unlimited() -> Self {
        Self {
            cpu_ticks: None,
            memory_units: None,
            message_count: None,
            storage_ops: None,
            pipeline_stages: None,
        }
    }

    /// Creates a new budget with all resources set to zero
    pub fn zero() -> Self {
        Self {
            cpu_ticks: Some(CpuTicks::zero()),
            memory_units: Some(MemoryUnits::zero()),
            message_count: Some(MessageCount::zero()),
            storage_ops: Some(StorageOps::zero()),
            pipeline_stages: Some(PipelineStages::zero()),
        }
    }

    /// Builder: sets CPU ticks limit
    pub fn with_cpu_ticks(mut self, limit: CpuTicks) -> Self {
        self.cpu_ticks = Some(limit);
        self
    }

    /// Builder: sets memory units limit
    pub fn with_memory_units(mut self, limit: MemoryUnits) -> Self {
        self.memory_units = Some(limit);
        self
    }

    /// Builder: sets message count limit
    pub fn with_message_count(mut self, limit: MessageCount) -> Self {
        self.message_count = Some(limit);
        self
    }

    /// Builder: sets storage ops limit
    pub fn with_storage_ops(mut self, limit: StorageOps) -> Self {
        self.storage_ops = Some(limit);
        self
    }

    /// Builder: sets pipeline stages limit
    pub fn with_pipeline_stages(mut self, limit: PipelineStages) -> Self {
        self.pipeline_stages = Some(limit);
        self
    }

    /// Checks if this budget is a subset of (less than or equal to) another budget
    ///
    /// Returns true if all limits in this budget are ≤ corresponding limits in other.
    /// If either budget has None for a resource, that resource is not constrained.
    pub fn is_subset_of(&self, other: &ResourceBudget) -> bool {
        // For each resource, if self has a limit, other must have a limit >= self's limit
        if let Some(self_cpu) = self.cpu_ticks {
            match other.cpu_ticks {
                Some(other_cpu) if self_cpu <= other_cpu => {}
                None => {} // No limit in parent is OK
                _ => return false,
            }
        }

        if let Some(self_mem) = self.memory_units {
            match other.memory_units {
                Some(other_mem) if self_mem <= other_mem => {}
                None => {}
                _ => return false,
            }
        }

        if let Some(self_msg) = self.message_count {
            match other.message_count {
                Some(other_msg) if self_msg <= other_msg => {}
                None => {}
                _ => return false,
            }
        }

        if let Some(self_storage) = self.storage_ops {
            match other.storage_ops {
                Some(other_storage) if self_storage <= other_storage => {}
                None => {}
                _ => return false,
            }
        }

        if let Some(self_stages) = self.pipeline_stages {
            match other.pipeline_stages {
                Some(other_stages) if self_stages <= other_stages => {}
                None => {}
                _ => return false,
            }
        }

        true
    }

    /// Returns the minimum of two budgets (most restrictive)
    pub fn min(&self, other: &ResourceBudget) -> ResourceBudget {
        ResourceBudget {
            cpu_ticks: match (self.cpu_ticks, other.cpu_ticks) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            memory_units: match (self.memory_units, other.memory_units) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            message_count: match (self.message_count, other.message_count) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            storage_ops: match (self.storage_ops, other.storage_ops) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            pipeline_stages: match (self.pipeline_stages, other.pipeline_stages) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
        }
    }
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl fmt::Display for ResourceBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceBudget[")?;
        let mut parts = Vec::new();
        if let Some(cpu) = self.cpu_ticks {
            parts.push(format!("cpu={}", cpu.0));
        }
        if let Some(mem) = self.memory_units {
            parts.push(format!("mem={}", mem.0));
        }
        if let Some(msg) = self.message_count {
            parts.push(format!("msg={}", msg.0));
        }
        if let Some(storage) = self.storage_ops {
            parts.push(format!("storage={}", storage.0));
        }
        if let Some(stages) = self.pipeline_stages {
            parts.push(format!("stages={}", stages.0));
        }
        if parts.is_empty() {
            write!(f, "unlimited")?;
        } else {
            write!(f, "{}", parts.join(", "))?;
        }
        write!(f, "]")
    }
}

/// Resource usage
///
/// Tracks current resource consumption. Mutable, updated as resources are consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub cpu_ticks: CpuTicks,
    pub memory_units: MemoryUnits,
    pub message_count: MessageCount,
    pub storage_ops: StorageOps,
    pub pipeline_stages: PipelineStages,
}

impl ResourceUsage {
    /// Creates a new usage tracker with all values at zero
    pub fn zero() -> Self {
        Self {
            cpu_ticks: CpuTicks::zero(),
            memory_units: MemoryUnits::zero(),
            message_count: MessageCount::zero(),
            storage_ops: StorageOps::zero(),
            pipeline_stages: PipelineStages::zero(),
        }
    }

    /// Consumes CPU ticks
    pub fn consume_cpu_ticks(&mut self, amount: CpuTicks) {
        self.cpu_ticks = self.cpu_ticks.saturating_add(amount);
    }

    /// Consumes memory units
    pub fn consume_memory_units(&mut self, amount: MemoryUnits) {
        self.memory_units = self.memory_units.saturating_add(amount);
    }

    /// Consumes a message
    pub fn consume_message(&mut self) {
        self.message_count = self.message_count.saturating_add(MessageCount::new(1));
    }

    /// Consumes storage operation
    pub fn consume_storage_op(&mut self) {
        self.storage_ops = self.storage_ops.saturating_add(StorageOps::new(1));
    }

    /// Consumes a pipeline stage
    pub fn consume_pipeline_stage(&mut self) {
        self.pipeline_stages = self.pipeline_stages.saturating_add(PipelineStages::new(1));
    }

    /// Checks if usage exceeds budget
    ///
    /// Returns the first resource that exceeds its limit, if any.
    pub fn exceeds(&self, budget: &ResourceBudget) -> Option<ResourceExceeded> {
        if let Some(limit) = budget.cpu_ticks {
            if self.cpu_ticks > limit {
                return Some(ResourceExceeded::CpuTicks {
                    limit,
                    usage: self.cpu_ticks,
                });
            }
        }

        if let Some(limit) = budget.memory_units {
            if self.memory_units > limit {
                return Some(ResourceExceeded::MemoryUnits {
                    limit,
                    usage: self.memory_units,
                });
            }
        }

        if let Some(limit) = budget.message_count {
            if self.message_count > limit {
                return Some(ResourceExceeded::MessageCount {
                    limit,
                    usage: self.message_count,
                });
            }
        }

        if let Some(limit) = budget.storage_ops {
            if self.storage_ops > limit {
                return Some(ResourceExceeded::StorageOps {
                    limit,
                    usage: self.storage_ops,
                });
            }
        }

        if let Some(limit) = budget.pipeline_stages {
            if self.pipeline_stages > limit {
                return Some(ResourceExceeded::PipelineStages {
                    limit,
                    usage: self.pipeline_stages,
                });
            }
        }

        None
    }

    /// Returns remaining budget
    pub fn remaining(&self, budget: &ResourceBudget) -> ResourceBudget {
        ResourceBudget {
            cpu_ticks: budget
                .cpu_ticks
                .map(|limit| limit.saturating_sub(self.cpu_ticks)),
            memory_units: budget
                .memory_units
                .map(|limit| limit.saturating_sub(self.memory_units)),
            message_count: budget
                .message_count
                .map(|limit| limit.saturating_sub(self.message_count)),
            storage_ops: budget
                .storage_ops
                .map(|limit| limit.saturating_sub(self.storage_ops)),
            pipeline_stages: budget
                .pipeline_stages
                .map(|limit| limit.saturating_sub(self.pipeline_stages)),
        }
    }
}

impl Default for ResourceUsage {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Display for ResourceUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ResourceUsage[cpu={}, mem={}, msg={}, storage={}, stages={}]",
            self.cpu_ticks.0,
            self.memory_units.0,
            self.message_count.0,
            self.storage_ops.0,
            self.pipeline_stages.0
        )
    }
}

/// Resource exceeded information
///
/// Indicates which resource exceeded its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceExceeded {
    CpuTicks { limit: CpuTicks, usage: CpuTicks },
    MemoryUnits { limit: MemoryUnits, usage: MemoryUnits },
    MessageCount { limit: MessageCount, usage: MessageCount },
    StorageOps { limit: StorageOps, usage: StorageOps },
    PipelineStages { limit: PipelineStages, usage: PipelineStages },
}

impl fmt::Display for ResourceExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CpuTicks { limit, usage } => {
                write!(f, "CPU ticks exceeded: limit={}, usage={}", limit.0, usage.0)
            }
            Self::MemoryUnits { limit, usage } => {
                write!(f, "Memory units exceeded: limit={}, usage={}", limit.0, usage.0)
            }
            Self::MessageCount { limit, usage } => {
                write!(f, "Message count exceeded: limit={}, usage={}", limit.0, usage.0)
            }
            Self::StorageOps { limit, usage } => {
                write!(f, "Storage ops exceeded: limit={}, usage={}", limit.0, usage.0)
            }
            Self::PipelineStages { limit, usage } => {
                write!(f, "Pipeline stages exceeded: limit={}, usage={}", limit.0, usage.0)
            }
        }
    }
}

/// Resource delta
///
/// Describes changes to resource consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDelta {
    pub cpu_ticks: CpuTicks,
    pub memory_units: MemoryUnits,
    pub message_count: MessageCount,
    pub storage_ops: StorageOps,
    pub pipeline_stages: PipelineStages,
}

impl ResourceDelta {
    /// Creates a new delta with all values at zero
    pub fn zero() -> Self {
        Self {
            cpu_ticks: CpuTicks::zero(),
            memory_units: MemoryUnits::zero(),
            message_count: MessageCount::zero(),
            storage_ops: StorageOps::zero(),
            pipeline_stages: PipelineStages::zero(),
        }
    }

    /// Computes delta between two usage snapshots
    pub fn from(before: &ResourceUsage, after: &ResourceUsage) -> Self {
        Self {
            cpu_ticks: after.cpu_ticks.saturating_sub(before.cpu_ticks),
            memory_units: after.memory_units.saturating_sub(before.memory_units),
            message_count: after.message_count.saturating_sub(before.message_count),
            storage_ops: after.storage_ops.saturating_sub(before.storage_ops),
            pipeline_stages: after.pipeline_stages.saturating_sub(before.pipeline_stages),
        }
    }
}

impl Default for ResourceDelta {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Display for ResourceDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ResourceDelta[cpu=+{}, mem=+{}, msg=+{}, storage=+{}, stages=+{}]",
            self.cpu_ticks.0,
            self.memory_units.0,
            self.message_count.0,
            self.storage_ops.0,
            self.pipeline_stages.0
        )
    }
}

/// Resource-related errors
#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("Resource budget exceeded: {0}")]
    BudgetExceeded(ResourceExceeded),

    #[error("Resource budget missing: required for {operation}")]
    BudgetMissing { operation: String },

    #[error("Invalid budget derivation: {reason}")]
    InvalidBudgetDerivation { reason: String },

    #[error("Budget inheritance violation: child budget exceeds parent")]
    BudgetInheritanceViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_ticks_arithmetic() {
        let a = CpuTicks::new(100);
        let b = CpuTicks::new(50);

        assert_eq!(a.checked_add(b), Some(CpuTicks::new(150)));
        assert_eq!(a.checked_sub(b), Some(CpuTicks::new(50)));
        assert_eq!(b.checked_sub(a), None); // Would underflow

        assert_eq!(a.saturating_add(b), CpuTicks::new(150));
        assert_eq!(b.saturating_sub(a), CpuTicks::zero());
    }

    #[test]
    fn test_cpu_ticks_overflow() {
        let max = CpuTicks::new(u64::MAX);
        let one = CpuTicks::new(1);

        assert_eq!(max.checked_add(one), None);
        assert_eq!(max.saturating_add(one), max);
    }

    #[test]
    fn test_resource_budget_unlimited() {
        let budget = ResourceBudget::unlimited();
        assert_eq!(budget.cpu_ticks, None);
        assert_eq!(budget.message_count, None);
    }

    #[test]
    fn test_resource_budget_zero() {
        let budget = ResourceBudget::zero();
        assert_eq!(budget.cpu_ticks, Some(CpuTicks::zero()));
        assert_eq!(budget.message_count, Some(MessageCount::zero()));
    }

    #[test]
    fn test_resource_budget_builder() {
        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000))
            .with_message_count(MessageCount::new(50));

        assert_eq!(budget.cpu_ticks, Some(CpuTicks::new(1000)));
        assert_eq!(budget.message_count, Some(MessageCount::new(50)));
        assert_eq!(budget.memory_units, None);
    }

    #[test]
    fn test_resource_budget_subset_all_unlimited() {
        let budget1 = ResourceBudget::unlimited();
        let budget2 = ResourceBudget::unlimited();
        assert!(budget1.is_subset_of(&budget2));
        assert!(budget2.is_subset_of(&budget1));
    }

    #[test]
    fn test_resource_budget_subset_child_limited() {
        let parent = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000));
        let child = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(500));

        assert!(child.is_subset_of(&parent));
        assert!(!parent.is_subset_of(&child));
    }

    #[test]
    fn test_resource_budget_subset_equal() {
        let budget1 = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000))
            .with_message_count(MessageCount::new(100));
        let budget2 = budget1;

        assert!(budget1.is_subset_of(&budget2));
        assert!(budget2.is_subset_of(&budget1));
    }

    #[test]
    fn test_resource_budget_subset_violates() {
        let parent = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(500));
        let child = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000)); // Exceeds parent

        assert!(!child.is_subset_of(&parent));
    }

    #[test]
    fn test_resource_budget_min() {
        let budget1 = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000))
            .with_message_count(MessageCount::new(50));

        let budget2 = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(500))
            .with_message_count(MessageCount::new(100));

        let min = budget1.min(&budget2);
        assert_eq!(min.cpu_ticks, Some(CpuTicks::new(500)));
        assert_eq!(min.message_count, Some(MessageCount::new(50)));
    }

    #[test]
    fn test_resource_usage_zero() {
        let usage = ResourceUsage::zero();
        assert!(usage.cpu_ticks.is_zero());
        assert!(usage.message_count.is_zero());
    }

    #[test]
    fn test_resource_usage_consume() {
        let mut usage = ResourceUsage::zero();
        usage.consume_cpu_ticks(CpuTicks::new(10));
        usage.consume_message();
        usage.consume_message();

        assert_eq!(usage.cpu_ticks, CpuTicks::new(10));
        assert_eq!(usage.message_count, MessageCount::new(2));
    }

    #[test]
    fn test_resource_usage_exceeds_none() {
        let usage = ResourceUsage::zero();
        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000));

        assert_eq!(usage.exceeds(&budget), None);
    }

    #[test]
    fn test_resource_usage_exceeds_cpu() {
        let mut usage = ResourceUsage::zero();
        usage.consume_cpu_ticks(CpuTicks::new(1001));

        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000));

        let exceeded = usage.exceeds(&budget);
        assert!(exceeded.is_some());
        match exceeded.unwrap() {
            ResourceExceeded::CpuTicks { limit, usage: u } => {
                assert_eq!(limit, CpuTicks::new(1000));
                assert_eq!(u, CpuTicks::new(1001));
            }
            _ => panic!("Expected CpuTicks exceeded"),
        }
    }

    #[test]
    fn test_resource_usage_exceeds_messages() {
        let mut usage = ResourceUsage::zero();
        for _ in 0..11 {
            usage.consume_message();
        }

        let budget = ResourceBudget::unlimited()
            .with_message_count(MessageCount::new(10));

        let exceeded = usage.exceeds(&budget);
        assert!(exceeded.is_some());
        match exceeded.unwrap() {
            ResourceExceeded::MessageCount { limit, usage: u } => {
                assert_eq!(limit, MessageCount::new(10));
                assert_eq!(u, MessageCount::new(11));
            }
            _ => panic!("Expected MessageCount exceeded"),
        }
    }

    #[test]
    fn test_resource_usage_remaining() {
        let mut usage = ResourceUsage::zero();
        usage.consume_cpu_ticks(CpuTicks::new(300));
        usage.consume_message();

        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000))
            .with_message_count(MessageCount::new(10));

        let remaining = usage.remaining(&budget);
        assert_eq!(remaining.cpu_ticks, Some(CpuTicks::new(700)));
        assert_eq!(remaining.message_count, Some(MessageCount::new(9)));
    }

    #[test]
    fn test_resource_usage_remaining_saturates() {
        let mut usage = ResourceUsage::zero();
        usage.consume_cpu_ticks(CpuTicks::new(1500));

        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000));

        let remaining = usage.remaining(&budget);
        assert_eq!(remaining.cpu_ticks, Some(CpuTicks::zero()));
    }

    #[test]
    fn test_resource_delta_zero() {
        let delta = ResourceDelta::zero();
        assert!(delta.cpu_ticks.is_zero());
        assert!(delta.message_count.is_zero());
    }

    #[test]
    fn test_resource_delta_from() {
        let mut before = ResourceUsage::zero();
        before.consume_cpu_ticks(CpuTicks::new(100));
        before.consume_message();

        let mut after = before;
        after.consume_cpu_ticks(CpuTicks::new(50));
        after.consume_message();

        let delta = ResourceDelta::from(&before, &after);
        assert_eq!(delta.cpu_ticks, CpuTicks::new(50));
        assert_eq!(delta.message_count, MessageCount::new(1));
    }

    #[test]
    fn test_resource_types_display() {
        assert_eq!(CpuTicks::new(100).to_string(), "100 CPU ticks");
        assert_eq!(MemoryUnits::new(200).to_string(), "200 memory units");
        assert_eq!(MessageCount::new(10).to_string(), "10 messages");
        assert_eq!(StorageOps::new(5).to_string(), "5 storage ops");
        assert_eq!(PipelineStages::new(3).to_string(), "3 pipeline stages");
    }

    #[test]
    fn test_resource_budget_display() {
        let budget = ResourceBudget::unlimited()
            .with_cpu_ticks(CpuTicks::new(1000))
            .with_message_count(MessageCount::new(50));

        let display = budget.to_string();
        assert!(display.contains("cpu=1000"));
        assert!(display.contains("msg=50"));
    }

    #[test]
    fn test_resource_exceeded_display() {
        let exceeded = ResourceExceeded::CpuTicks {
            limit: CpuTicks::new(1000),
            usage: CpuTicks::new(1001),
        };

        let display = exceeded.to_string();
        assert!(display.contains("CPU ticks exceeded"));
        assert!(display.contains("limit=1000"));
        assert!(display.contains("usage=1001"));
    }
}
