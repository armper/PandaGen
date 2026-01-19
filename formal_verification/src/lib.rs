//! Formal verification pass scaffolding for core invariants.

use core_types::CapabilityMetadata;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub name: String,
    pub passed: bool,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub results: Vec<VerificationResult>,
}

impl VerificationReport {
    pub fn passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }
}

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("Invariant failed: {0}")]
    Failed(String),
}

/// Capability invariants: cap IDs are unique and owned.
pub fn verify_capabilities(metadata: &[CapabilityMetadata]) -> VerificationResult {
    let mut seen = HashSet::new();
    for cap in metadata {
        if !seen.insert(cap.cap_id) {
            return VerificationResult {
                name: "capability.unique_ids".to_string(),
                passed: false,
                details: Some(format!("duplicate cap_id {}", cap.cap_id)),
            };
        }
    }
    VerificationResult {
        name: "capability.unique_ids".to_string(),
        passed: true,
        details: None,
    }
}

/// Scheduler invariant: no duplicate tasks in runnable set.
pub fn verify_scheduler_tasks(tasks: &[u64]) -> VerificationResult {
    let mut seen = HashSet::new();
    for task in tasks {
        if !seen.insert(*task) {
            return VerificationResult {
                name: "scheduler.no_duplicates".to_string(),
                passed: false,
                details: Some(format!("duplicate task {}", task)),
            };
        }
    }
    VerificationResult {
        name: "scheduler.no_duplicates".to_string(),
        passed: true,
        details: None,
    }
}

/// Memory model invariant: regions must be non-overlapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRegionModel {
    pub base: u64,
    pub len: u64,
}

pub fn verify_memory_regions(regions: &[MemoryRegionModel]) -> VerificationResult {
    for (i, a) in regions.iter().enumerate() {
        for b in regions.iter().skip(i + 1) {
            let a_end = a.base.saturating_add(a.len);
            let b_end = b.base.saturating_add(b.len);
            let overlap = a.base < b_end && b.base < a_end;
            if overlap {
                return VerificationResult {
                    name: "memory.non_overlapping".to_string(),
                    passed: false,
                    details: Some(format!("overlap between {} and {}", a.base, b.base)),
                };
            }
        }
    }
    VerificationResult {
        name: "memory.non_overlapping".to_string(),
        passed: true,
        details: None,
    }
}

pub fn run_verification(
    capabilities: &[CapabilityMetadata],
    scheduler_tasks: &[u64],
    regions: &[MemoryRegionModel],
) -> VerificationReport {
    VerificationReport {
        results: vec![
            verify_capabilities(capabilities),
            verify_scheduler_tasks(scheduler_tasks),
            verify_memory_regions(regions),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::{CapabilityMetadata, CapabilityStatus, TaskId};

    #[test]
    fn test_verification_report_passes() {
        let meta = CapabilityMetadata {
            cap_id: 1,
            owner: TaskId::new(),
            cap_type: "test".to_string(),
            status: CapabilityStatus::Valid,
            grantor: None,
            revoked: false,
            lease_expires_at_nanos: None,
        };

        let report = run_verification(
            &[meta],
            &[1, 2, 3],
            &[
                MemoryRegionModel { base: 0, len: 10 },
                MemoryRegionModel { base: 20, len: 5 },
            ],
        );
        assert!(report.passed());
    }

    #[test]
    fn test_verification_detects_overlap() {
        let report = verify_memory_regions(&[
            MemoryRegionModel { base: 0, len: 10 },
            MemoryRegionModel { base: 5, len: 10 },
        ]);
        assert!(!report.passed);
    }
}
