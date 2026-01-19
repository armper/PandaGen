//! # Pipeline
//!
//! Typed Intent Pipeline system for composing handlers safely.
//!
//! ## Philosophy
//!
//! - **Typed composition**: Outputs become inputs, not byte streams
//! - **Explicit capability flow**: No ambient authority through pipelines
//! - **Bounded failure semantics**: Explicit retry policies, no infinite loops
//! - **Deterministic under faults**: Works with SimKernel + fault injection
//!
//! ## Core Concepts
//!
//! - `PipelineSpec`: Describes a sequence of stages
//! - `Stage`: A single step that processes typed input
//! - `StageResult`: Success, Failure, or Retryable
//! - `RetryPolicy`: Bounded retry behavior with backoff

use core_types::ServiceId;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for a pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PipelineId(Uuid);

impl PipelineId {
    /// Creates a new random pipeline ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PipelineId {
    fn default() -> Self {
        Self::new()
    }
}

/// Unique identifier for a stage within a pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StageId(Uuid);

impl StageId {
    /// Creates a new random stage ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for StageId {
    fn default() -> Self {
        Self::new()
    }
}

/// Schema ID for typed intent payloads
///
/// This is used to ensure type safety across pipeline stages.
/// Each payload type has a unique schema ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PayloadSchemaId(String);

impl PayloadSchemaId {
    /// Creates a new payload schema ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the schema ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Version for payload schemas
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadSchemaVersion {
    pub major: u32,
    pub minor: u32,
}

impl PayloadSchemaVersion {
    /// Creates a new schema version
    pub fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Typed payload for pipeline stages
///
/// Uses schema IDs + versioning for type-safe transport.
/// Data is serialized but tagged with type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedPayload {
    pub schema_id: PayloadSchemaId,
    pub schema_version: PayloadSchemaVersion,
    pub data: Vec<u8>,
}

impl TypedPayload {
    /// Creates a new typed payload
    pub fn new(
        schema_id: PayloadSchemaId,
        schema_version: PayloadSchemaVersion,
        data: Vec<u8>,
    ) -> Self {
        Self {
            schema_id,
            schema_version,
            data,
        }
    }
}

/// Result of a stage execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StageResult {
    /// Stage succeeded with output and optional capabilities
    Success {
        output: TypedPayload,
        capabilities: Vec<u64>, // Cap IDs produced
    },
    /// Stage failed permanently
    Failure { error: String },
    /// Stage failed but can be retried
    Retryable { error: String },
}

/// Retry policy for a stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 = no retries)
    pub max_retries: u32,
    /// Initial backoff duration in milliseconds
    pub initial_backoff_ms: u64,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
}

impl RetryPolicy {
    /// No retries - fail immediately
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            initial_backoff_ms: 0,
            backoff_multiplier: 1.0,
        }
    }

    /// Simple retry policy with fixed attempts
    pub fn fixed_retries(max_retries: u32, backoff_ms: u64) -> Self {
        Self {
            max_retries,
            initial_backoff_ms: backoff_ms,
            backoff_multiplier: 1.0,
        }
    }

    /// Exponential backoff retry policy
    pub fn exponential_backoff(max_retries: u32, initial_backoff_ms: u64) -> Self {
        Self {
            max_retries,
            initial_backoff_ms,
            backoff_multiplier: 2.0,
        }
    }

    /// Calculates backoff duration for a given retry attempt
    ///
    /// Note: attempt 0 is the first execution (not a retry), so backoff is 0.
    /// Backoff starts from attempt 1 (first retry).
    pub fn backoff_duration(&self, attempt: u32) -> u64 {
        if attempt == 0 {
            0 // First attempt has no backoff
        } else {
            let multiplier = self.backoff_multiplier.powi((attempt - 1) as i32);
            (self.initial_backoff_ms as f64 * multiplier) as u64
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::none()
    }
}

/// A stage in a pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageSpec {
    /// Unique identifier for this stage
    pub id: StageId,
    /// Human-readable name for debugging
    pub name: String,
    /// Service that handles this stage
    pub handler: ServiceId,
    /// Action to invoke on the handler
    pub action: String,
    /// Expected input schema
    pub input_schema: PayloadSchemaId,
    /// Expected output schema
    pub output_schema: PayloadSchemaId,
    /// Retry policy for this stage
    pub retry_policy: RetryPolicy,
    /// Required capability IDs (must be available at stage start)
    pub required_capabilities: Vec<u64>,
}

impl StageSpec {
    /// Creates a new stage specification
    pub fn new(
        name: String,
        handler: ServiceId,
        action: String,
        input_schema: PayloadSchemaId,
        output_schema: PayloadSchemaId,
    ) -> Self {
        Self {
            id: StageId::new(),
            name,
            handler,
            action,
            input_schema,
            output_schema,
            retry_policy: RetryPolicy::none(),
            required_capabilities: Vec::new(),
        }
    }

    /// Sets the retry policy for this stage
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Adds required capabilities for this stage
    pub fn with_capabilities(mut self, cap_ids: Vec<u64>) -> Self {
        self.required_capabilities = cap_ids;
        self
    }
}

/// Pipeline specification - describes a linear sequence of stages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSpec {
    /// Unique identifier for this pipeline
    pub id: PipelineId,
    /// Human-readable name
    pub name: String,
    /// Ordered sequence of stages
    pub stages: Vec<StageSpec>,
    /// Initial input schema expected by first stage
    pub initial_input_schema: PayloadSchemaId,
    /// Final output schema produced by last stage
    pub final_output_schema: PayloadSchemaId,
}

impl PipelineSpec {
    /// Creates a new pipeline specification
    pub fn new(
        name: String,
        initial_input_schema: PayloadSchemaId,
        final_output_schema: PayloadSchemaId,
    ) -> Self {
        Self {
            id: PipelineId::new(),
            name,
            stages: Vec::new(),
            initial_input_schema,
            final_output_schema,
        }
    }

    /// Adds a stage to the pipeline
    pub fn add_stage(mut self, stage: StageSpec) -> Self {
        self.stages.push(stage);
        self
    }

    /// Validates that the pipeline is well-formed
    ///
    /// Checks:
    /// - At least one stage
    /// - Stage input/output schemas chain correctly
    /// - First stage input matches pipeline input
    /// - Last stage output matches pipeline output
    pub fn validate(&self) -> Result<(), PipelineError> {
        if self.stages.is_empty() {
            return Err(PipelineError::EmptyPipeline);
        }

        // Check first stage input matches pipeline input
        if self.stages[0].input_schema != self.initial_input_schema {
            return Err(PipelineError::SchemaMismatch {
                stage_name: self.stages[0].name.clone(),
                expected: self.initial_input_schema.clone(),
                actual: self.stages[0].input_schema.clone(),
            });
        }

        // Check stage chaining
        for i in 0..self.stages.len() - 1 {
            let current_output = &self.stages[i].output_schema;
            let next_input = &self.stages[i + 1].input_schema;
            if current_output != next_input {
                return Err(PipelineError::SchemaMismatch {
                    stage_name: self.stages[i + 1].name.clone(),
                    expected: current_output.clone(),
                    actual: next_input.clone(),
                });
            }
        }

        // Check last stage output matches pipeline output
        let last_stage = self.stages.last().unwrap();
        if last_stage.output_schema != self.final_output_schema {
            return Err(PipelineError::SchemaMismatch {
                stage_name: last_stage.name.clone(),
                expected: self.final_output_schema.clone(),
                actual: last_stage.output_schema.clone(),
            });
        }

        Ok(())
    }
}

/// Errors that can occur in pipeline operations
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("Pipeline has no stages")]
    EmptyPipeline,

    #[error("Schema mismatch in stage '{stage_name}': expected {expected:?}, got {actual:?}")]
    SchemaMismatch {
        stage_name: String,
        expected: PayloadSchemaId,
        actual: PayloadSchemaId,
    },

    #[error("Stage execution failed: {0}")]
    StageExecutionFailed(String),

    #[error("Missing required capability: {0}")]
    MissingCapability(u64),

    #[error("Invalid payload schema: {0}")]
    InvalidSchema(String),
}

/// Execution trace entry for a stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTraceEntry {
    /// Stage identifier
    pub stage_id: StageId,
    /// Stage name
    pub stage_name: String,
    /// Start timestamp (simulated time in ms)
    pub start_time_ms: u64,
    /// End timestamp (simulated time in ms)
    pub end_time_ms: u64,
    /// Attempt number (0 for first attempt, increments on retry)
    pub attempt: u32,
    /// Result of execution
    pub result: StageExecutionResult,
    /// Capability IDs passed to this stage
    pub capabilities_in: Vec<u64>,
    /// Capability IDs produced by this stage
    pub capabilities_out: Vec<u64>,
}

/// Result of a stage execution in the trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StageExecutionResult {
    Success,
    Failure { error: String },
    Retrying { error: String },
}

/// Complete execution trace for a pipeline run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Pipeline identifier
    pub pipeline_id: PipelineId,
    /// Ordered trace entries for each stage execution
    pub entries: Vec<StageTraceEntry>,
    /// Overall pipeline result
    pub final_result: PipelineExecutionResult,
}

impl ExecutionTrace {
    /// Creates a new empty execution trace
    pub fn new(pipeline_id: PipelineId) -> Self {
        Self {
            pipeline_id,
            entries: Vec::new(),
            final_result: PipelineExecutionResult::InProgress,
        }
    }

    /// Adds a trace entry
    pub fn add_entry(&mut self, entry: StageTraceEntry) {
        self.entries.push(entry);
    }

    /// Sets the final result
    pub fn set_final_result(&mut self, result: PipelineExecutionResult) {
        self.final_result = result;
    }

    /// Returns entries for a specific stage
    pub fn entries_for_stage(&self, stage_id: StageId) -> Vec<&StageTraceEntry> {
        self.entries
            .iter()
            .filter(|e| e.stage_id == stage_id)
            .collect()
    }
}

/// Overall result of pipeline execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PipelineExecutionResult {
    InProgress,
    Success,
    Failed { stage_name: String, error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_id_uniqueness() {
        let id1 = PipelineId::new();
        let id2 = PipelineId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_stage_id_uniqueness() {
        let id1 = StageId::new();
        let id2 = StageId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_retry_policy_none() {
        let policy = RetryPolicy::none();
        assert_eq!(policy.max_retries, 0);
    }

    #[test]
    fn test_retry_policy_fixed() {
        let policy = RetryPolicy::fixed_retries(3, 100);
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.backoff_duration(0), 0); // First attempt, no backoff
        assert_eq!(policy.backoff_duration(1), 100); // First retry
        assert_eq!(policy.backoff_duration(2), 100); // Second retry
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy = RetryPolicy::exponential_backoff(3, 100);
        assert_eq!(policy.backoff_duration(0), 0); // First attempt, no backoff
        assert_eq!(policy.backoff_duration(1), 100); // First retry: 100 * 2^0
        assert_eq!(policy.backoff_duration(2), 200); // Second retry: 100 * 2^1
        assert_eq!(policy.backoff_duration(3), 400); // Third retry: 100 * 2^2
    }

    #[test]
    fn test_pipeline_validation_empty() {
        let pipeline = PipelineSpec::new(
            "test".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        );
        assert!(pipeline.validate().is_err());
    }

    #[test]
    fn test_pipeline_validation_single_stage() {
        let stage = StageSpec::new(
            "stage1".to_string(),
            ServiceId::new(),
            "action".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        );

        let pipeline = PipelineSpec::new(
            "test".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        )
        .add_stage(stage);

        assert!(pipeline.validate().is_ok());
    }

    #[test]
    fn test_pipeline_validation_chained_stages() {
        let stage1 = StageSpec::new(
            "stage1".to_string(),
            ServiceId::new(),
            "action1".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("mid"),
        );

        let stage2 = StageSpec::new(
            "stage2".to_string(),
            ServiceId::new(),
            "action2".to_string(),
            PayloadSchemaId::new("mid"),
            PayloadSchemaId::new("out"),
        );

        let pipeline = PipelineSpec::new(
            "test".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        )
        .add_stage(stage1)
        .add_stage(stage2);

        assert!(pipeline.validate().is_ok());
    }

    #[test]
    fn test_pipeline_validation_schema_mismatch() {
        let stage1 = StageSpec::new(
            "stage1".to_string(),
            ServiceId::new(),
            "action1".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("mid1"),
        );

        let stage2 = StageSpec::new(
            "stage2".to_string(),
            ServiceId::new(),
            "action2".to_string(),
            PayloadSchemaId::new("mid2"), // Wrong! Should be mid1
            PayloadSchemaId::new("out"),
        );

        let pipeline = PipelineSpec::new(
            "test".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        )
        .add_stage(stage1)
        .add_stage(stage2);

        assert!(pipeline.validate().is_err());
    }

    #[test]
    fn test_execution_trace() {
        let mut trace = ExecutionTrace::new(PipelineId::new());
        let stage_id = StageId::new();

        trace.add_entry(StageTraceEntry {
            stage_id,
            stage_name: "test_stage".to_string(),
            start_time_ms: 0,
            end_time_ms: 100,
            attempt: 0,
            result: StageExecutionResult::Success,
            capabilities_in: vec![1],
            capabilities_out: vec![2],
        });

        assert_eq!(trace.entries.len(), 1);
        assert_eq!(trace.entries_for_stage(stage_id).len(), 1);
    }
}
