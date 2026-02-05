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

/// Real-time scheduling model input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealTimeScheduleModel {
    pub utilization_ppm: u64,
    pub deadline_misses: u64,
}

/// Consensus log entry model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusLogEntryModel {
    pub index: u64,
    pub term: u64,
}

/// Consensus log model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusLogModel {
    pub entries: Vec<ConsensusLogEntryModel>,
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

/// Real-time invariant: utilization must be <= 100% (1_000_000 ppm).
pub fn verify_real_time_utilization(model: &RealTimeScheduleModel) -> VerificationResult {
    if model.utilization_ppm > 1_000_000 {
        return VerificationResult {
            name: "scheduler.realtime.utilization".to_string(),
            passed: false,
            details: Some(format!(
                "utilization_ppm {} exceeds 1_000_000",
                model.utilization_ppm
            )),
        };
    }
    VerificationResult {
        name: "scheduler.realtime.utilization".to_string(),
        passed: true,
        details: None,
    }
}

/// Real-time invariant: admitted schedules should not miss deadlines.
pub fn verify_real_time_deadlines(model: &RealTimeScheduleModel) -> VerificationResult {
    if model.utilization_ppm <= 1_000_000 && model.deadline_misses > 0 {
        return VerificationResult {
            name: "scheduler.realtime.deadlines".to_string(),
            passed: false,
            details: Some(format!(
                "deadline_misses {} despite admissible utilization",
                model.deadline_misses
            )),
        };
    }
    VerificationResult {
        name: "scheduler.realtime.deadlines".to_string(),
        passed: true,
        details: None,
    }
}

/// Consensus invariant: log indices strictly increase and terms are non-decreasing.
pub fn verify_consensus_log(model: &ConsensusLogModel) -> VerificationResult {
    let mut last_index = 0;
    let mut last_term = 0;
    for entry in &model.entries {
        if entry.index <= last_index {
            return VerificationResult {
                name: "consensus.log.monotonic".to_string(),
                passed: false,
                details: Some(format!("non-monotonic index {}", entry.index)),
            };
        }
        if entry.term < last_term {
            return VerificationResult {
                name: "consensus.log.monotonic".to_string(),
                passed: false,
                details: Some(format!("term regression at index {}", entry.index)),
            };
        }
        last_index = entry.index;
        last_term = entry.term;
    }

    VerificationResult {
        name: "consensus.log.monotonic".to_string(),
        passed: true,
        details: None,
    }
}

/// Composite inputs for critical path verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalPathInputs {
    pub capabilities: Vec<CapabilityMetadata>,
    pub scheduler_tasks: Vec<u64>,
    pub regions: Vec<MemoryRegionModel>,
    pub realtime: RealTimeScheduleModel,
    pub consensus: ConsensusLogModel,
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

/// Runs verification across critical paths (capabilities, scheduler, memory, real-time, consensus).
pub fn run_critical_path_verification(inputs: &CriticalPathInputs) -> VerificationReport {
    VerificationReport {
        results: vec![
            verify_capabilities(&inputs.capabilities),
            verify_scheduler_tasks(&inputs.scheduler_tasks),
            verify_memory_regions(&inputs.regions),
            verify_real_time_utilization(&inputs.realtime),
            verify_real_time_deadlines(&inputs.realtime),
            verify_consensus_log(&inputs.consensus),
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

    #[test]
    fn test_real_time_verification() {
        let model = RealTimeScheduleModel {
            utilization_ppm: 800_000,
            deadline_misses: 0,
        };
        assert!(verify_real_time_utilization(&model).passed);
        assert!(verify_real_time_deadlines(&model).passed);

        let failing = RealTimeScheduleModel {
            utilization_ppm: 900_000,
            deadline_misses: 2,
        };
        assert!(!verify_real_time_deadlines(&failing).passed);
    }

    #[test]
    fn test_consensus_log_verification() {
        let model = ConsensusLogModel {
            entries: vec![
                ConsensusLogEntryModel { index: 1, term: 1 },
                ConsensusLogEntryModel { index: 2, term: 1 },
                ConsensusLogEntryModel { index: 3, term: 2 },
            ],
        };
        assert!(verify_consensus_log(&model).passed);

        let bad = ConsensusLogModel {
            entries: vec![
                ConsensusLogEntryModel { index: 2, term: 2 },
                ConsensusLogEntryModel { index: 1, term: 2 },
            ],
        };
        assert!(!verify_consensus_log(&bad).passed);
    }
}
