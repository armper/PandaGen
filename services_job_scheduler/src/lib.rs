#![no_std]

//! # Job Scheduler Service
//!
//! A deterministic, cooperative task queue for background jobs.
//!
//! ## Philosophy
//!
//! - **Deterministic**: All jobs are explicitly ticked, no hidden threads
//! - **Cooperative**: Jobs yield control back to the scheduler
//! - **Testable**: All logic runs under `cargo test`
//! - **Bare-metal compatible**: Same code path on simulator and hardware
//! - **Explicit ticks**: No ambient execution, all progress is explicit
//!
//! ## Features
//!
//! - Cooperative task queue with explicit ticks
//! - Job priorities and dependencies
//! - Progress tracking
//! - Deterministic execution order
//!
//! ## Example Jobs
//!
//! - "index workspace" job
//! - "format document" job
//! - "sync settings" job
//!
//! ## Example
//!
//! ```ignore
//! use services_job_scheduler::{JobScheduler, Job, JobStatus};
//!
//! let mut scheduler = JobScheduler::new();
//!
//! // Create a job
//! let job_id = scheduler.schedule_job(Job::new(
//!     "index_workspace",
//!     JobPriority::Normal,
//!     Box::new(|ctx| {
//!         // Do work...
//!         JobStatus::Completed
//!     }),
//! ));
//!
//! // Tick the scheduler to make progress
//! scheduler.tick();
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Job identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(Uuid);

impl JobId {
    /// Creates a new job ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a JobId from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job:{}", self.0)
    }
}

/// Job priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum JobPriority {
    /// Low priority
    Low,
    /// Normal priority
    Normal,
    /// High priority
    High,
}

/// Job status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is pending execution
    Pending,
    /// Job is currently running
    Running,
    /// Job yielded and will continue on next tick
    Yielded,
    /// Job completed successfully
    Completed,
    /// Job failed
    Failed,
    /// Job was cancelled
    Cancelled,
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "Pending"),
            JobStatus::Running => write!(f, "Running"),
            JobStatus::Yielded => write!(f, "Yielded"),
            JobStatus::Completed => write!(f, "Completed"),
            JobStatus::Failed => write!(f, "Failed"),
            JobStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Job execution context
pub struct JobContext {
    /// Current tick count
    pub tick_count: u64,
    /// Job's own tick count
    pub job_ticks: u64,
}

impl JobContext {
    fn new(tick_count: u64, job_ticks: u64) -> Self {
        Self {
            tick_count,
            job_ticks,
        }
    }
}

/// Job execution result
pub enum JobResult {
    /// Job completed successfully
    Completed,
    /// Job yielded and will continue on next tick
    Yielded,
    /// Job failed with an error message
    Failed(String),
}

/// Job execution function
pub type JobFn = Box<dyn FnMut(&mut JobContext) -> JobResult + Send>;

/// A job descriptor
pub struct JobDescriptor {
    /// Unique job identifier
    pub id: JobId,
    /// Human-readable job name
    pub name: String,
    /// Job priority
    pub priority: JobPriority,
    /// Job status
    pub status: JobStatus,
    /// Number of ticks this job has executed
    pub ticks_executed: u64,
    /// Progress (0-100)
    pub progress: u8,
    /// Optional error message (if failed)
    pub error: Option<String>,
    /// Job execution function
    executor: Option<JobFn>,
}

impl JobDescriptor {
    /// Creates a new job descriptor
    pub fn new(name: impl Into<String>, priority: JobPriority, executor: JobFn) -> Self {
        Self {
            id: JobId::new(),
            name: name.into(),
            priority,
            status: JobStatus::Pending,
            ticks_executed: 0,
            progress: 0,
            error: None,
            executor: Some(executor),
        }
    }

    /// Sets the progress (0-100)
    pub fn set_progress(&mut self, progress: u8) {
        self.progress = progress.min(100);
    }
}

/// Job scheduler
pub struct JobScheduler {
    /// Pending jobs (not yet started)
    pending_jobs: VecDeque<JobDescriptor>,
    /// Running job (if any)
    running_job: Option<JobDescriptor>,
    /// Completed jobs (kept for history)
    completed_jobs: Vec<JobDescriptor>,
    /// Current tick count
    tick_count: u64,
}

impl JobScheduler {
    /// Creates a new job scheduler
    pub fn new() -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            running_job: None,
            completed_jobs: Vec::new(),
            tick_count: 0,
        }
    }

    /// Schedules a new job
    pub fn schedule_job(&mut self, job: JobDescriptor) -> JobId {
        let id = job.id;

        // Insert based on priority (higher priority first)
        let insert_pos = self
            .pending_jobs
            .iter()
            .position(|j| j.priority < job.priority)
            .unwrap_or(self.pending_jobs.len());

        self.pending_jobs.insert(insert_pos, job);

        id
    }

    /// Ticks the scheduler to make progress on jobs
    pub fn tick(&mut self) {
        self.tick_count += 1;

        // If there's a running job, continue it
        if let Some(job) = self.running_job.take() {
            self.execute_and_handle_job(job, true);
            return;
        }

        // Start the next pending job
        if let Some(job) = self.pending_jobs.pop_front() {
            self.execute_and_handle_job(job, false);
        }
    }

    /// Executes one tick of a job and handles the result
    fn execute_and_handle_job(&mut self, mut job: JobDescriptor, is_continuing: bool) {
        if !is_continuing {
            job.status = JobStatus::Running;
        }
        job.ticks_executed += 1;

        let mut ctx = JobContext::new(self.tick_count, job.ticks_executed);

        if let Some(ref mut executor) = job.executor {
            match executor(&mut ctx) {
                JobResult::Completed => {
                    job.status = JobStatus::Completed;
                    job.progress = 100;
                    job.executor = None;
                    self.completed_jobs.push(job);
                }
                JobResult::Yielded => {
                    job.status = JobStatus::Yielded;
                    self.running_job = Some(job);
                }
                JobResult::Failed(error) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(error);
                    job.executor = None;
                    self.completed_jobs.push(job);
                }
            }
        }
    }

    /// Returns the current tick count
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Returns the number of pending jobs
    pub fn pending_count(&self) -> usize {
        self.pending_jobs.len()
    }

    /// Returns the number of completed jobs
    pub fn completed_count(&self) -> usize {
        self.completed_jobs.len()
    }

    /// Checks if there's a running job
    pub fn has_running_job(&self) -> bool {
        self.running_job.is_some()
    }

    /// Gets the status of a job by ID
    pub fn get_job_status(&self, id: JobId) -> Option<JobStatus> {
        // Check running job
        if let Some(ref job) = self.running_job {
            if job.id == id {
                return Some(job.status);
            }
        }

        // Check pending jobs
        for job in &self.pending_jobs {
            if job.id == id {
                return Some(job.status);
            }
        }

        // Check completed jobs
        for job in &self.completed_jobs {
            if job.id == id {
                return Some(job.status);
            }
        }

        None
    }

    /// Gets the progress of a job by ID (0-100)
    pub fn get_job_progress(&self, id: JobId) -> Option<u8> {
        // Check running job
        if let Some(ref job) = self.running_job {
            if job.id == id {
                return Some(job.progress);
            }
        }

        // Check pending jobs
        for job in &self.pending_jobs {
            if job.id == id {
                return Some(job.progress);
            }
        }

        // Check completed jobs
        for job in &self.completed_jobs {
            if job.id == id {
                return Some(job.progress);
            }
        }

        None
    }

    /// Cancels a pending job
    pub fn cancel_job(&mut self, id: JobId) -> bool {
        // Can only cancel pending jobs
        if let Some(pos) = self.pending_jobs.iter().position(|j| j.id == id) {
            let mut job = self.pending_jobs.remove(pos).unwrap();
            job.status = JobStatus::Cancelled;
            job.executor = None;
            self.completed_jobs.push(job);
            true
        } else {
            false
        }
    }

    /// Returns all pending job IDs
    pub fn list_pending_jobs(&self) -> Vec<(JobId, String)> {
        self.pending_jobs
            .iter()
            .map(|j| (j.id, j.name.clone()))
            .collect()
    }

    /// Returns all completed job IDs
    pub fn list_completed_jobs(&self) -> Vec<(JobId, String, JobStatus)> {
        self.completed_jobs
            .iter()
            .map(|j| (j.id, j.name.clone(), j.status))
            .collect()
    }
}

impl Default for JobScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn test_job_id_creation() {
        let id1 = JobId::new();
        let id2 = JobId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_job_priority_ordering() {
        assert!(JobPriority::Low < JobPriority::Normal);
        assert!(JobPriority::Normal < JobPriority::High);
    }

    #[test]
    fn test_scheduler_creation() {
        let scheduler = JobScheduler::new();
        assert_eq!(scheduler.tick_count(), 0);
        assert_eq!(scheduler.pending_count(), 0);
        assert_eq!(scheduler.completed_count(), 0);
        assert!(!scheduler.has_running_job());
    }

    #[test]
    fn test_schedule_single_job() {
        let mut scheduler = JobScheduler::new();

        let job_id = scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        assert_eq!(scheduler.pending_count(), 1);
        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Pending));
    }

    #[test]
    fn test_tick_completes_job() {
        let mut scheduler = JobScheduler::new();

        let job_id = scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        scheduler.tick();

        assert_eq!(scheduler.pending_count(), 0);
        assert_eq!(scheduler.completed_count(), 1);
        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Completed));
        assert_eq!(scheduler.get_job_progress(job_id), Some(100));
    }

    #[test]
    fn test_tick_yields_job() {
        let mut scheduler = JobScheduler::new();
        let mut tick_count = 0;

        let job_id = scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(move |_| {
                tick_count += 1;
                if tick_count < 3 {
                    JobResult::Yielded
                } else {
                    JobResult::Completed
                }
            }),
        ));

        // First tick: should yield
        scheduler.tick();
        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Yielded));
        assert!(scheduler.has_running_job());

        // Second tick: should yield again
        scheduler.tick();
        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Yielded));

        // Third tick: should complete
        scheduler.tick();
        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Completed));
        assert!(!scheduler.has_running_job());
    }

    #[test]
    fn test_job_failure() {
        let mut scheduler = JobScheduler::new();

        let job_id = scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(|_| JobResult::Failed("Test error".to_string())),
        ));

        scheduler.tick();

        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Failed));
        assert_eq!(scheduler.completed_count(), 1);
    }

    #[test]
    fn test_scheduler_priority_ordering() {
        let mut scheduler = JobScheduler::new();

        let low_job = scheduler.schedule_job(JobDescriptor::new(
            "low",
            JobPriority::Low,
            Box::new(|_| JobResult::Completed),
        ));

        let high_job = scheduler.schedule_job(JobDescriptor::new(
            "high",
            JobPriority::High,
            Box::new(|_| JobResult::Completed),
        ));

        let normal_job = scheduler.schedule_job(JobDescriptor::new(
            "normal",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        // High priority should run first
        scheduler.tick();
        assert_eq!(
            scheduler.get_job_status(high_job),
            Some(JobStatus::Completed)
        );

        // Normal priority should run second
        scheduler.tick();
        assert_eq!(
            scheduler.get_job_status(normal_job),
            Some(JobStatus::Completed)
        );

        // Low priority should run last
        scheduler.tick();
        assert_eq!(
            scheduler.get_job_status(low_job),
            Some(JobStatus::Completed)
        );
    }

    #[test]
    fn test_cancel_pending_job() {
        let mut scheduler = JobScheduler::new();

        let job_id = scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        let cancelled = scheduler.cancel_job(job_id);
        assert!(cancelled);

        assert_eq!(scheduler.pending_count(), 0);
        assert_eq!(scheduler.get_job_status(job_id), Some(JobStatus::Cancelled));
    }

    #[test]
    fn test_cannot_cancel_running_job() {
        let mut scheduler = JobScheduler::new();

        let job_id = scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(|_| JobResult::Yielded),
        ));

        scheduler.tick();

        // Job is now running (yielded)
        let cancelled = scheduler.cancel_job(job_id);
        assert!(!cancelled);
    }

    #[test]
    fn test_job_context() {
        let mut scheduler = JobScheduler::new();

        // Just verify that job_ticks increment and eventually complete
        let mut ticks = 0;
        scheduler.schedule_job(JobDescriptor::new(
            "test_job",
            JobPriority::Normal,
            Box::new(move |ctx| {
                // Verify tick_count increases
                assert!(ctx.tick_count > 0);
                // Verify job_ticks is reasonable
                assert_eq!(ctx.job_ticks, ticks + 1);
                ticks += 1;

                if ctx.job_ticks < 3 {
                    JobResult::Yielded
                } else {
                    JobResult::Completed
                }
            }),
        ));

        // Run the job to completion
        scheduler.tick();
        scheduler.tick();
        scheduler.tick();

        assert_eq!(scheduler.completed_count(), 1);
    }

    #[test]
    fn test_list_pending_jobs() {
        let mut scheduler = JobScheduler::new();

        scheduler.schedule_job(JobDescriptor::new(
            "job1",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        scheduler.schedule_job(JobDescriptor::new(
            "job2",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        let pending = scheduler.list_pending_jobs();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_list_completed_jobs() {
        let mut scheduler = JobScheduler::new();

        scheduler.schedule_job(JobDescriptor::new(
            "job1",
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));

        scheduler.schedule_job(JobDescriptor::new(
            "job2",
            JobPriority::Normal,
            Box::new(|_| JobResult::Failed("error".to_string())),
        ));

        scheduler.tick();
        scheduler.tick();

        let completed = scheduler.list_completed_jobs();
        assert_eq!(completed.len(), 2);
        assert_eq!(completed[0].2, JobStatus::Completed);
        assert_eq!(completed[1].2, JobStatus::Failed);
    }

    #[test]
    fn test_get_nonexistent_job() {
        let scheduler = JobScheduler::new();
        let status = scheduler.get_job_status(JobId::new());
        assert_eq!(status, None);
    }
}
