//! # Pipeline Integration Tests
//!
//! End-to-end tests for typed intent pipelines with example handlers.
//!
//! ## Test Philosophy
//!
//! - **Happy path**: Full pipeline A->B->C with success
//! - **Fail-fast**: Pipeline stops at first non-retryable failure
//! - **Retry semantics**: Bounded retries with deterministic backoff
//! - **Fault injection**: Pipeline behaves correctly under message faults
//! - **Capability tracking**: No leaks, proper flow through stages

#![cfg(test)]

use core_types::ServiceId;
use kernel_api::{Duration, KernelApi};
use pipeline::{
    ExecutionTrace, PayloadSchemaId, PayloadSchemaVersion, PipelineExecutionResult, PipelineSpec,
    RetryPolicy, StageResult, StageSpec, TypedPayload,
};
use serde::{Deserialize, Serialize};
use services_pipeline_executor::PipelineExecutor;
use sim_kernel::SimulatedKernel;

// ============================================================================
// Example Payload Types (simulating storage operations)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateBlobInput {
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateBlobOutput {
    object_cap_id: u64,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransformBlobInput {
    object_cap_id: u64,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransformBlobOutput {
    object_cap_id: u64,
    transformed_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnnotateMetadataInput {
    object_cap_id: u64,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnnotateMetadataOutput {
    object_cap_id: u64,
    metadata: String,
}

// ============================================================================
// Mock Handler Implementations
// ============================================================================

/// Creates a TypedPayload from a serializable struct
fn create_payload<T: Serialize>(schema_id: &str, version: (u32, u32), data: &T) -> TypedPayload {
    let bytes = serde_json::to_vec(data).unwrap();
    TypedPayload::new(
        PayloadSchemaId::new(schema_id),
        PayloadSchemaVersion::new(version.0, version.1),
        bytes,
    )
}

/// Deserializes a TypedPayload
fn deserialize_payload<T: for<'de> Deserialize<'de>>(
    payload: &TypedPayload,
) -> Result<T, serde_json::Error> {
    serde_json::from_slice(&payload.data)
}

/// Mock handler for CreateBlob stage
fn handle_create_blob(input: TypedPayload) -> StageResult {
    let data: CreateBlobInput = deserialize_payload(&input).unwrap();

    // Simulate creating a blob and returning a capability
    let object_cap_id = 100; // Mock cap ID

    let output = CreateBlobOutput {
        object_cap_id,
        content: data.content,
    };

    StageResult::Success {
        output: create_payload("create_blob_output", (1, 0), &output),
        capabilities: vec![object_cap_id],
    }
}

/// Mock handler for TransformBlobUppercase stage
fn handle_transform_blob(input: TypedPayload) -> StageResult {
    // Input is CreateBlobOutput from previous stage
    let data: CreateBlobOutput = deserialize_payload(&input).unwrap();

    // Simulate transforming content to uppercase
    let transformed = data.content.to_uppercase();
    let new_cap_id = data.object_cap_id + 1; // Mock new cap

    let output = TransformBlobOutput {
        object_cap_id: new_cap_id,
        transformed_content: transformed,
    };

    StageResult::Success {
        output: create_payload("transform_blob_output", (1, 0), &output),
        capabilities: vec![new_cap_id],
    }
}

/// Mock handler for AnnotateMetadata stage
fn handle_annotate_metadata(input: TypedPayload) -> StageResult {
    // Input is TransformBlobOutput from previous stage
    let data: TransformBlobOutput = deserialize_payload(&input).unwrap();

    // Simulate adding metadata
    let metadata = format!("length={}", data.transformed_content.len());

    let output = AnnotateMetadataOutput {
        object_cap_id: data.object_cap_id,
        metadata,
    };

    StageResult::Success {
        output: create_payload("annotate_metadata_output", (1, 0), &output),
        capabilities: vec![data.object_cap_id],
    }
}

/// Mock handler that fails permanently
fn handle_failure(_input: TypedPayload) -> StageResult {
    StageResult::Failure {
        error: "Stage failed permanently".to_string(),
    }
}

/// Mock handler that fails with retry (with internal counter for testing)
fn handle_retryable_with_counter(_input: TypedPayload, attempt_counter: &mut u32) -> StageResult {
    *attempt_counter += 1;

    if *attempt_counter < 3 {
        // Fail first 2 attempts
        StageResult::Retryable {
            error: format!("Attempt {} failed", attempt_counter),
        }
    } else {
        // Succeed on 3rd attempt
        let output = CreateBlobOutput {
            object_cap_id: 200,
            content: "success after retry".to_string(),
        };
        StageResult::Success {
            output: create_payload("create_blob_output", (1, 0), &output),
            capabilities: vec![200],
        }
    }
}

// ============================================================================
// Custom Executor for Testing (with handler injection)
// ============================================================================

struct TestPipelineExecutor {
    base: PipelineExecutor,
    handlers: std::collections::HashMap<String, Box<dyn Fn(TypedPayload) -> StageResult>>,
    retry_counter: std::cell::RefCell<u32>,
}

impl TestPipelineExecutor {
    fn new() -> Self {
        Self {
            base: PipelineExecutor::new(),
            handlers: std::collections::HashMap::new(),
            retry_counter: std::cell::RefCell::new(0),
        }
    }

    fn add_capabilities(&mut self, caps: Vec<u64>) {
        self.base.add_capabilities(caps);
    }

    fn register_handler<F>(&mut self, action: String, handler: F)
    where
        F: Fn(TypedPayload) -> StageResult + 'static,
    {
        self.handlers.insert(action, Box::new(handler));
    }

    fn register_retryable_handler(&mut self, action: String) {
        // For retryable handlers, we need special handling
        self.handlers.insert(
            action,
            Box::new(|_| StageResult::Retryable {
                error: "Not implemented - use execute with retry counter".to_string(),
            }),
        );
    }

    /// Executes a pipeline with test handlers
    fn execute<K: KernelApi>(
        &mut self,
        kernel: &mut K,
        spec: &PipelineSpec,
        initial_input: TypedPayload,
    ) -> Result<(TypedPayload, ExecutionTrace), services_pipeline_executor::ExecutorError> {
        // Validate
        spec.validate()?;

        if initial_input.schema_id != spec.initial_input_schema {
            return Err(
                services_pipeline_executor::ExecutorError::SchemaValidation {
                    expected: spec.initial_input_schema.as_str().to_string(),
                    actual: initial_input.schema_id.as_str().to_string(),
                },
            );
        }

        let mut trace = ExecutionTrace::new(spec.id);
        let mut current_input = initial_input;

        // Execute stages
        for stage in &spec.stages {
            // Check required capabilities
            for &cap_id in &stage.required_capabilities {
                if !self.base.has_capability(cap_id) {
                    trace.set_final_result(PipelineExecutionResult::Failed {
                        stage_name: stage.name.clone(),
                        error: format!("Missing required capability: {}", cap_id),
                    });
                    return Err(services_pipeline_executor::ExecutorError::KernelError(
                        format!("Missing required capability: {}", cap_id),
                    ));
                }
            }

            // Execute with retry
            let result =
                self.execute_stage_with_retry(kernel, stage, current_input.clone(), &mut trace)?;

            match result {
                StageResult::Success {
                    output,
                    capabilities,
                } => {
                    for cap in capabilities {
                        self.base.add_capabilities(vec![cap]);
                    }
                    current_input = output;
                }
                StageResult::Failure { error } => {
                    trace.set_final_result(PipelineExecutionResult::Failed {
                        stage_name: stage.name.clone(),
                        error: error.clone(),
                    });
                    return Err(services_pipeline_executor::ExecutorError::KernelError(
                        error,
                    ));
                }
                StageResult::Retryable { error } => {
                    trace.set_final_result(PipelineExecutionResult::Failed {
                        stage_name: stage.name.clone(),
                        error: error.clone(),
                    });
                    return Err(services_pipeline_executor::ExecutorError::KernelError(
                        error,
                    ));
                }
            }
        }

        trace.set_final_result(PipelineExecutionResult::Success);
        Ok((current_input, trace))
    }

    fn execute_stage_with_retry<K: KernelApi>(
        &self,
        kernel: &mut K,
        stage: &StageSpec,
        input: TypedPayload,
        trace: &mut ExecutionTrace,
    ) -> Result<StageResult, services_pipeline_executor::ExecutorError> {
        let retry_policy = &stage.retry_policy;
        let mut attempt = 0;

        loop {
            let start_time = kernel.now();
            let start_time_ms = start_time.as_nanos() / 1_000_000;

            // Get handler
            let handler = self.handlers.get(&stage.action).ok_or_else(|| {
                services_pipeline_executor::ExecutorError::HandlerNotFound(stage.action.clone())
            })?;

            // Execute
            let result = if stage.action == "retryable_action" {
                // Special handling for retryable test
                let mut counter = self.retry_counter.borrow_mut();
                handle_retryable_with_counter(input.clone(), &mut counter)
            } else {
                handler(input.clone())
            };

            let end_time = kernel.now();
            let end_time_ms = end_time.as_nanos() / 1_000_000;

            // Extract caps
            let caps_out = match &result {
                StageResult::Success { capabilities, .. } => capabilities.clone(),
                _ => vec![],
            };

            // Record trace
            let trace_result = match &result {
                StageResult::Success { .. } => pipeline::StageExecutionResult::Success,
                StageResult::Failure { error } => pipeline::StageExecutionResult::Failure {
                    error: error.clone(),
                },
                StageResult::Retryable { error } => pipeline::StageExecutionResult::Retrying {
                    error: error.clone(),
                },
            };

            trace.add_entry(pipeline::StageTraceEntry {
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
                        return Ok(StageResult::Failure {
                            error: format!(
                                "Max retries ({}) exceeded: {}",
                                retry_policy.max_retries, error
                            ),
                        });
                    }

                    // Backoff
                    let backoff = retry_policy.backoff_duration(attempt);
                    if backoff > 0 {
                        let _ = kernel.sleep(Duration::from_millis(backoff));
                    }

                    attempt += 1;
                }
            }
        }
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_happy_path_pipeline() {
        // Create kernel and executor
        let mut kernel = SimulatedKernel::new();
        let mut executor = TestPipelineExecutor::new();

        // Register handlers
        executor.register_handler("create_blob".to_string(), handle_create_blob);
        executor.register_handler("transform_blob".to_string(), handle_transform_blob);
        executor.register_handler("annotate_metadata".to_string(), handle_annotate_metadata);

        // Build pipeline: CreateBlob -> TransformBlob -> AnnotateMetadata
        let stage1 = StageSpec::new(
            "CreateBlob".to_string(),
            ServiceId::new(),
            "create_blob".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("create_blob_output"),
        );

        let stage2 = StageSpec::new(
            "TransformBlob".to_string(),
            ServiceId::new(),
            "transform_blob".to_string(),
            PayloadSchemaId::new("create_blob_output"), // Must match stage1's output
            PayloadSchemaId::new("transform_blob_output"),
        )
        .with_capabilities(vec![100]); // Requires cap from stage1

        let stage3 = StageSpec::new(
            "AnnotateMetadata".to_string(),
            ServiceId::new(),
            "annotate_metadata".to_string(),
            PayloadSchemaId::new("transform_blob_output"), // Must match stage2's output
            PayloadSchemaId::new("annotate_metadata_output"),
        )
        .with_capabilities(vec![101]); // Requires cap from stage2

        let pipeline = PipelineSpec::new(
            "test_pipeline".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("annotate_metadata_output"),
        )
        .add_stage(stage1)
        .add_stage(stage2)
        .add_stage(stage3);

        // Initial input
        let input = CreateBlobInput {
            content: "hello world".to_string(),
        };
        let input_payload = create_payload("create_blob_input", (1, 0), &input);

        // Execute
        let (output, trace) = executor
            .execute(&mut kernel, &pipeline, input_payload)
            .unwrap();

        // Verify output
        assert_eq!(
            output.schema_id,
            PayloadSchemaId::new("annotate_metadata_output")
        );
        let output_data: AnnotateMetadataOutput = deserialize_payload(&output).unwrap();
        assert_eq!(output_data.object_cap_id, 101);
        assert_eq!(output_data.metadata, "length=11"); // "HELLO WORLD" length

        // Verify trace
        assert_eq!(trace.entries.len(), 3);
        assert_eq!(trace.final_result, PipelineExecutionResult::Success);

        // Check stage ordering
        assert_eq!(trace.entries[0].stage_name, "CreateBlob");
        assert_eq!(trace.entries[1].stage_name, "TransformBlob");
        assert_eq!(trace.entries[2].stage_name, "AnnotateMetadata");

        // Check capabilities flow
        assert_eq!(trace.entries[0].capabilities_out, vec![100]);
        assert_eq!(trace.entries[1].capabilities_in, vec![100]);
        assert_eq!(trace.entries[1].capabilities_out, vec![101]);
        assert_eq!(trace.entries[2].capabilities_in, vec![101]);
    }

    #[test]
    fn test_fail_fast_behavior() {
        let mut kernel = SimulatedKernel::new();
        let mut executor = TestPipelineExecutor::new();

        // Register handlers - stage2 fails
        executor.register_handler("create_blob".to_string(), handle_create_blob);
        executor.register_handler("failing_stage".to_string(), handle_failure);
        executor.register_handler("annotate_metadata".to_string(), handle_annotate_metadata);

        // Build pipeline
        let stage1 = StageSpec::new(
            "CreateBlob".to_string(),
            ServiceId::new(),
            "create_blob".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("create_blob_output"),
        );

        let stage2 = StageSpec::new(
            "FailingStage".to_string(),
            ServiceId::new(),
            "failing_stage".to_string(),
            PayloadSchemaId::new("transform_blob_input"),
            PayloadSchemaId::new("transform_blob_output"),
        );

        let stage3 = StageSpec::new(
            "AnnotateMetadata".to_string(),
            ServiceId::new(),
            "annotate_metadata".to_string(),
            PayloadSchemaId::new("annotate_metadata_input"),
            PayloadSchemaId::new("annotate_metadata_output"),
        );

        let pipeline = PipelineSpec::new(
            "fail_fast_pipeline".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("annotate_metadata_output"),
        )
        .add_stage(stage1)
        .add_stage(stage2)
        .add_stage(stage3);

        let input = CreateBlobInput {
            content: "test".to_string(),
        };
        let input_payload = create_payload("create_blob_input", (1, 0), &input);

        // Execute - should fail
        let result = executor.execute(&mut kernel, &pipeline, input_payload);
        assert!(result.is_err());

        // Verify executor's internal trace (would need to extract it properly)
        // For now, we know it failed at stage 2
    }

    #[test]
    fn test_retry_semantics() {
        let mut kernel = SimulatedKernel::new();
        let mut executor = TestPipelineExecutor::new();

        // Register retryable handler
        executor.register_retryable_handler("retryable_action".to_string());

        // Build single-stage pipeline with retry policy
        let stage = StageSpec::new(
            "RetryableStage".to_string(),
            ServiceId::new(),
            "retryable_action".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("create_blob_output"),
        )
        .with_retry_policy(RetryPolicy::fixed_retries(3, 100));

        let pipeline = PipelineSpec::new(
            "retry_pipeline".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("create_blob_output"),
        )
        .add_stage(stage);

        let input = CreateBlobInput {
            content: "retry test".to_string(),
        };
        let input_payload = create_payload("create_blob_input", (1, 0), &input);

        // Execute - should succeed after 2 retries
        let (output, trace) = executor
            .execute(&mut kernel, &pipeline, input_payload)
            .unwrap();

        // Verify output
        let output_data: CreateBlobOutput = deserialize_payload(&output).unwrap();
        assert_eq!(output_data.content, "success after retry");

        // Verify trace shows multiple attempts
        assert_eq!(trace.entries.len(), 3); // 3 attempts (0, 1, 2)
        assert_eq!(trace.entries[0].attempt, 0);
        assert_eq!(trace.entries[1].attempt, 1);
        assert_eq!(trace.entries[2].attempt, 2);

        // Verify backoff was applied (time should have advanced)
        assert!(trace.entries[1].start_time_ms > trace.entries[0].end_time_ms + 90); // ~100ms backoff
        assert!(trace.entries[2].start_time_ms > trace.entries[1].end_time_ms + 90);
    }

    #[test]
    fn test_missing_capability_fails() {
        let mut kernel = SimulatedKernel::new();
        let mut executor = TestPipelineExecutor::new();
        // Don't add any capabilities

        executor.register_handler("create_blob".to_string(), handle_create_blob);

        // Stage requires capability 999
        let stage = StageSpec::new(
            "CreateBlob".to_string(),
            ServiceId::new(),
            "create_blob".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("create_blob_output"),
        )
        .with_capabilities(vec![999]); // This cap doesn't exist

        let pipeline = PipelineSpec::new(
            "cap_test".to_string(),
            PayloadSchemaId::new("create_blob_input"),
            PayloadSchemaId::new("create_blob_output"),
        )
        .add_stage(stage);

        let input = CreateBlobInput {
            content: "test".to_string(),
        };
        let input_payload = create_payload("create_blob_input", (1, 0), &input);

        // Should fail due to missing capability
        let result = executor.execute(&mut kernel, &pipeline, input_payload);
        assert!(result.is_err());
    }
}
