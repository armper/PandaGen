//! # Pipeline Executor Service
//!
//! Orchestrates pipeline execution without adding kernel complexity.
//!
//! ## Philosophy
//!
//! - **User-space orchestration**: Keep kernel API primitive
//! - **Explicit capability flow**: Track caps through stages
//! - **Deterministic execution**: Works with SimKernel time
//! - **Bounded retries**: No infinite loops
//! - **Testable traces**: Minimal execution recording
//!
//! ## Design
//!
//! - Accepts PipelineSpec + initial input + caps
//! - Calls stage handlers (not kernel responsibility)
//! - Tracks correlation and stage boundaries
//! - Records execution trace for tests
//! - Enforces retry policies with backoff

use kernel_api::{Duration, KernelApi};
use pipeline::{
    ExecutionTrace, PayloadSchemaId, PipelineError, PipelineExecutionResult, PipelineSpec,
    StageExecutionResult, StageResult, StageTraceEntry, TypedPayload,
};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during pipeline execution
#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("Pipeline validation failed: {0}")]
    ValidationFailed(#[from] PipelineError),

    #[error("Kernel error: {0}")]
    KernelError(String),

    #[error("Stage handler not found: {0}")]
    HandlerNotFound(String),

    #[error("Schema validation failed: expected {expected}, got {actual}")]
    SchemaValidation { expected: String, actual: String },

    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Pipeline executor orchestrates pipeline execution
pub struct PipelineExecutor {
    /// Available capability pool (cap_id -> exists)
    available_capabilities: HashMap<u64, bool>,
}

impl PipelineExecutor {
    /// Creates a new pipeline executor
    pub fn new() -> Self {
        Self {
            available_capabilities: HashMap::new(),
        }
    }

    /// Adds initial capabilities to the pool
    pub fn add_capabilities(&mut self, cap_ids: Vec<u64>) {
        for cap_id in cap_ids {
            self.available_capabilities.insert(cap_id, true);
        }
    }

    /// Checks if a capability is available
    pub fn has_capability(&self, cap_id: u64) -> bool {
        self.available_capabilities
            .get(&cap_id)
            .copied()
            .unwrap_or(false)
    }

    /// Executes a pipeline with the given input
    ///
    /// This is the main orchestration logic:
    /// 1. Validate pipeline
    /// 2. For each stage:
    ///    - Check required capabilities
    ///    - Invoke handler (with retry)
    ///    - Update capability pool
    ///    - Record trace
    /// 3. Return final result + trace
    pub fn execute<K: KernelApi>(
        &mut self,
        kernel: &mut K,
        spec: &PipelineSpec,
        initial_input: TypedPayload,
    ) -> Result<(TypedPayload, ExecutionTrace), ExecutorError> {
        // Validate pipeline
        spec.validate()?;

        // Validate initial input schema
        if initial_input.schema_id != spec.initial_input_schema {
            return Err(ExecutorError::SchemaValidation {
                expected: spec.initial_input_schema.as_str().to_string(),
                actual: initial_input.schema_id.as_str().to_string(),
            });
        }

        let mut trace = ExecutionTrace::new(spec.id);
        let mut current_input = initial_input;

        // Execute each stage in sequence
        for stage in &spec.stages {
            // Check required capabilities
            for &cap_id in &stage.required_capabilities {
                if !self.has_capability(cap_id) {
                    trace.set_final_result(PipelineExecutionResult::Failed {
                        stage_name: stage.name.clone(),
                        error: format!("Missing required capability: {}", cap_id),
                    });
                    return Err(ExecutorError::KernelError(format!(
                        "Missing required capability: {}",
                        cap_id
                    )));
                }
            }

            // Execute stage with retry
            let stage_result =
                self.execute_stage_with_retry(kernel, stage, current_input.clone(), &mut trace)?;

            match stage_result {
                StageResult::Success {
                    output,
                    capabilities,
                } => {
                    // Update capability pool
                    for cap_id in capabilities {
                        self.available_capabilities.insert(cap_id, true);
                    }
                    current_input = output;
                }
                StageResult::Failure { error } => {
                    // Fail-fast: stop at first non-retryable failure
                    trace.set_final_result(PipelineExecutionResult::Failed {
                        stage_name: stage.name.clone(),
                        error: error.clone(),
                    });
                    return Err(ExecutorError::KernelError(error));
                }
                StageResult::Retryable { error } => {
                    // Should not happen - retry logic handles this
                    trace.set_final_result(PipelineExecutionResult::Failed {
                        stage_name: stage.name.clone(),
                        error: error.clone(),
                    });
                    return Err(ExecutorError::KernelError(format!(
                        "Retryable error not handled: {}",
                        error
                    )));
                }
            }
        }

        // Validate final output schema
        if current_input.schema_id != spec.final_output_schema {
            return Err(ExecutorError::SchemaValidation {
                expected: spec.final_output_schema.as_str().to_string(),
                actual: current_input.schema_id.as_str().to_string(),
            });
        }

        trace.set_final_result(PipelineExecutionResult::Success);
        Ok((current_input, trace))
    }

    /// Executes a single stage with retry logic
    fn execute_stage_with_retry<K: KernelApi>(
        &self,
        kernel: &mut K,
        stage: &pipeline::StageSpec,
        input: TypedPayload,
        trace: &mut ExecutionTrace,
    ) -> Result<StageResult, ExecutorError> {
        let retry_policy = &stage.retry_policy;
        let mut attempt = 0;

        loop {
            let start_time = kernel.now();
            let start_time_ms = start_time.as_nanos() / 1_000_000;

            // Execute stage (simplified - actual implementation would use IPC)
            let result = self.execute_stage_once(kernel, stage, input.clone())?;

            let end_time = kernel.now();
            let end_time_ms = end_time.as_nanos() / 1_000_000;

            // Extract caps for trace
            let caps_out = match &result {
                StageResult::Success { capabilities, .. } => capabilities.clone(),
                _ => vec![],
            };

            // Record trace entry
            let trace_result = match &result {
                StageResult::Success { .. } => StageExecutionResult::Success,
                StageResult::Failure { error } => StageExecutionResult::Failure {
                    error: error.clone(),
                },
                StageResult::Retryable { error } => StageExecutionResult::Retrying {
                    error: error.clone(),
                },
            };

            trace.add_entry(StageTraceEntry {
                stage_id: stage.id,
                stage_name: stage.name.clone(),
                start_time_ms,
                end_time_ms,
                attempt,
                result: trace_result,
                capabilities_in: stage.required_capabilities.clone(),
                capabilities_out: caps_out,
            });

            // Check result
            match result {
                StageResult::Success { .. } => return Ok(result),
                StageResult::Failure { .. } => return Ok(result),
                StageResult::Retryable { error } => {
                    if attempt >= retry_policy.max_retries {
                        // Max retries exceeded - convert to permanent failure
                        return Ok(StageResult::Failure {
                            error: format!(
                                "Max retries ({}) exceeded: {}",
                                retry_policy.max_retries, error
                            ),
                        });
                    }

                    // Wait before retry (using simulated kernel time)
                    let backoff = retry_policy.backoff_duration(attempt);
                    if backoff > 0 {
                        let _ = kernel.sleep(Duration::from_millis(backoff));
                    }

                    attempt += 1;
                }
            }
        }
    }

    /// Executes a stage once (no retry logic)
    ///
    /// In a real implementation, this would:
    /// 1. Look up handler service in registry
    /// 2. Send message with input payload
    /// 3. Wait for response
    /// 4. Deserialize result
    ///
    /// For now, this is a stub that handlers will override via dependency injection.
    fn execute_stage_once<K: KernelApi>(
        &self,
        _kernel: &mut K,
        _stage: &pipeline::StageSpec,
        _input: TypedPayload,
    ) -> Result<StageResult, ExecutorError> {
        // Stub implementation - real version would use IPC
        // This will be overridden by test implementations
        Ok(StageResult::Success {
            output: TypedPayload::new(
                PayloadSchemaId::new("stub"),
                pipeline::PayloadSchemaVersion::new(1, 0),
                vec![],
            ),
            capabilities: vec![],
        })
    }
}

impl Default for PipelineExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc::{ChannelId, MessageEnvelope};
    use pipeline::{PayloadSchemaId, PayloadSchemaVersion};

    #[test]
    fn test_executor_creation() {
        let executor = PipelineExecutor::new();
        assert!(!executor.has_capability(1));
    }

    #[test]
    fn test_add_capabilities() {
        let mut executor = PipelineExecutor::new();
        executor.add_capabilities(vec![1, 2, 3]);
        assert!(executor.has_capability(1));
        assert!(executor.has_capability(2));
        assert!(executor.has_capability(3));
        assert!(!executor.has_capability(4));
    }

    #[test]
    fn test_pipeline_validation_error() {
        let mut executor = PipelineExecutor::new();
        let mut mock_kernel = MockKernel::new();

        // Empty pipeline should fail validation
        let spec = PipelineSpec::new(
            "test".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        );

        let input = TypedPayload::new(
            PayloadSchemaId::new("in"),
            PayloadSchemaVersion::new(1, 0),
            vec![],
        );

        let result = executor.execute(&mut mock_kernel, &spec, input);
        assert!(result.is_err());
    }

    // Mock kernel for testing
    struct MockKernel {
        time_ms: u64,
    }

    impl MockKernel {
        fn new() -> Self {
            Self { time_ms: 0 }
        }
    }

    impl KernelApi for MockKernel {
        fn spawn_task(
            &mut self,
            _descriptor: kernel_api::TaskDescriptor,
        ) -> Result<kernel_api::TaskHandle, kernel_api::KernelError> {
            unimplemented!()
        }

        fn create_channel(&mut self) -> Result<ChannelId, kernel_api::KernelError> {
            unimplemented!()
        }

        fn send_message(
            &mut self,
            _channel: ChannelId,
            _message: MessageEnvelope,
        ) -> Result<(), kernel_api::KernelError> {
            unimplemented!()
        }

        fn receive_message(
            &mut self,
            _channel: ChannelId,
            _timeout: Option<Duration>,
        ) -> Result<MessageEnvelope, kernel_api::KernelError> {
            unimplemented!()
        }

        fn now(&self) -> kernel_api::Instant {
            // Mock instant based on our counter
            kernel_api::Instant::from_nanos(self.time_ms * 1_000_000)
        }

        fn sleep(&mut self, duration: Duration) -> Result<(), kernel_api::KernelError> {
            self.time_ms += duration.as_millis();
            Ok(())
        }

        fn grant_capability(
            &mut self,
            _task: core_types::TaskId,
            _capability: core_types::Cap<()>,
        ) -> Result<(), kernel_api::KernelError> {
            unimplemented!()
        }

        fn register_service(
            &mut self,
            _service_id: core_types::ServiceId,
            _channel: ChannelId,
        ) -> Result<(), kernel_api::KernelError> {
            unimplemented!()
        }

        fn lookup_service(
            &self,
            _service_id: core_types::ServiceId,
        ) -> Result<ChannelId, kernel_api::KernelError> {
            unimplemented!()
        }
    }
}
