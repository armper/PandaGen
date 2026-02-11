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

use identity::IdentityMetadata;
use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{Duration, KernelApi};
use lifecycle::{CancellationToken, Deadline};
use pipeline::{
    ExecutionTrace, PipelineError, PipelineExecutionResult, PipelineSpec, StageExecutionResult,
    StageResult, StageTraceEntry, TypedPayload,
};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent};
use serde::{Deserialize, Serialize};
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

    #[error("Policy denied {event} by {policy}: {reason}")]
    PolicyDenied {
        policy: String,
        event: String,
        reason: String,
        pipeline_id: Option<String>,
    },

    #[error("Policy requires action for {event} by {policy}: {action}")]
    PolicyRequire {
        policy: String,
        event: String,
        action: String,
        pipeline_id: Option<String>,
    },

    #[error("Policy derived authority invalid for {event} by {policy}: {reason}. Delta: {delta}")]
    PolicyDerivedAuthorityInvalid {
        policy: String,
        event: String,
        reason: String,
        delta: String,
        pipeline_id: Option<String>,
    },
}

/// Pipeline executor orchestrates pipeline execution
pub struct PipelineExecutor {
    /// Available capability pool (cap_id -> exists)
    available_capabilities: HashMap<u64, bool>,
    /// Optional policy engine for enforcement
    policy_engine: Option<Box<dyn PolicyEngine>>,
    /// Execution identity (for policy context)
    identity: Option<IdentityMetadata>,
}

/// Request payload sent to a stage handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageInvokeRequest {
    input: TypedPayload,
}

/// Response payload expected from a stage handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageInvokeResponse {
    result: StageResult,
}

impl PipelineExecutor {
    /// Creates a new pipeline executor
    pub fn new() -> Self {
        Self {
            available_capabilities: HashMap::new(),
            policy_engine: None,
            identity: None,
        }
    }

    /// Sets the policy engine for this executor
    pub fn with_policy_engine(mut self, engine: Box<dyn PolicyEngine>) -> Self {
        self.policy_engine = Some(engine);
        self
    }

    /// Sets the execution identity for policy context
    pub fn with_identity(mut self, identity: IdentityMetadata) -> Self {
        self.identity = Some(identity);
        self
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

    /// Gets the current capability set
    /// Phase 10: Helper for capability derivation
    fn get_capability_set(&self) -> policy::CapabilitySet {
        let caps: Vec<u64> = self
            .available_capabilities
            .iter()
            .filter_map(|(id, &present)| if present { Some(*id) } else { None })
            .collect();
        policy::CapabilitySet::from_capabilities(caps)
    }

    /// Checks if a capability is available, considering execution authority
    /// Phase 10: Authority-aware capability checking
    fn has_capability_with_authority(
        &self,
        cap_id: u64,
        authority: &Option<policy::CapabilitySet>,
    ) -> bool {
        // First check if we have it at all
        if !self.has_capability(cap_id) {
            return false;
        }

        // If authority is restricted, check against that
        if let Some(auth) = authority {
            auth.capabilities.contains(&cap_id)
        } else {
            true
        }
    }

    /// Executes a pipeline with the given input
    ///
    /// This is the main orchestration logic:
    /// 1. Validate pipeline
    /// 2. Check policy for pipeline start
    /// 3. For each stage:
    ///    - Check cancellation and timeout
    ///    - Check policy for stage start
    ///    - Check required capabilities
    ///    - Invoke handler (with retry)
    ///    - Update capability pool
    ///    - Record trace
    ///    - Emit policy event for stage end
    /// 4. Return final result + trace
    pub fn execute<K: KernelApi>(
        &mut self,
        kernel: &mut K,
        spec: &PipelineSpec,
        initial_input: TypedPayload,
        cancellation_token: CancellationToken,
    ) -> Result<(TypedPayload, ExecutionTrace), ExecutorError> {
        // Validate pipeline
        spec.validate()?;

        // Check cancellation before starting
        if cancellation_token.is_cancelled() {
            let mut trace = ExecutionTrace::new(spec.id);
            let reason = cancellation_token
                .reason()
                .map(|r| r.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            trace.set_final_result(PipelineExecutionResult::Cancelled {
                stage_name: "before_start".to_string(),
                reason,
            });
            return Err(ExecutorError::KernelError(
                "Pipeline cancelled before start".to_string(),
            ));
        }

        // Validate initial input schema
        if initial_input.schema_id != spec.initial_input_schema {
            return Err(ExecutorError::SchemaValidation {
                expected: spec.initial_input_schema.as_str().to_string(),
                actual: initial_input.schema_id.as_str().to_string(),
            });
        }

        // Check policy for pipeline start
        // Phase 10: Apply derived authority if present
        let mut execution_authority: Option<policy::CapabilitySet> = None;

        if let Some(policy) = &self.policy_engine {
            let context = self.build_pipeline_context(spec, kernel);
            let decision = policy.evaluate(PolicyEvent::OnPipelineStart, &context);

            match decision {
                PolicyDecision::Deny { reason } => {
                    return Err(ExecutorError::PolicyDenied {
                        policy: policy.name().to_string(),
                        event: "OnPipelineStart".to_string(),
                        reason,
                        pipeline_id: Some(spec.id.to_string()),
                    });
                }
                PolicyDecision::Require { action } => {
                    return Err(ExecutorError::PolicyRequire {
                        policy: policy.name().to_string(),
                        event: "OnPipelineStart".to_string(),
                        action,
                        pipeline_id: Some(spec.id.to_string()),
                    });
                }
                PolicyDecision::Allow { derived } => {
                    // Phase 10: Apply derived authority if present
                    if let Some(derived_auth) = derived {
                        // Validate that derived authority is a subset
                        let current_caps = self.get_capability_set();
                        if !derived_auth.capabilities.is_subset_of(&current_caps) {
                            let delta = policy::CapabilityDelta::from(
                                &current_caps,
                                &derived_auth.capabilities,
                            );
                            return Err(ExecutorError::PolicyDerivedAuthorityInvalid {
                                policy: policy.name().to_string(),
                                event: "OnPipelineStart".to_string(),
                                reason: "Derived authority grants more capabilities than available"
                                    .to_string(),
                                delta: format!(
                                    "removed: {:?}, added: {:?}",
                                    delta.removed, delta.added
                                ),
                                pipeline_id: Some(spec.id.to_string()),
                            });
                        }

                        // Apply the derived authority
                        execution_authority = Some(derived_auth.capabilities);
                    }
                }
            }
        }

        // Calculate pipeline deadline if timeout is specified
        let pipeline_deadline = spec
            .timeout_ms
            .map(|ms| Deadline::at(kernel.now() + Duration::from_millis(ms)));

        let mut trace = ExecutionTrace::new(spec.id);
        let mut current_input = initial_input;

        // Execute each stage in sequence
        for stage in &spec.stages {
            // Check cancellation before each stage
            if cancellation_token.is_cancelled() {
                let reason = cancellation_token
                    .reason()
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                trace.set_final_result(PipelineExecutionResult::Cancelled {
                    stage_name: stage.name.clone(),
                    reason: reason.clone(),
                });
                return Err(ExecutorError::KernelError(format!(
                    "Pipeline cancelled at stage {}: {}",
                    stage.name, reason
                )));
            }

            // Check pipeline deadline
            if let Some(deadline) = &pipeline_deadline {
                if deadline.has_passed(kernel.now()) {
                    trace.set_final_result(PipelineExecutionResult::Cancelled {
                        stage_name: stage.name.clone(),
                        reason: "pipeline timeout".to_string(),
                    });
                    return Err(ExecutorError::KernelError(format!(
                        "Pipeline timed out at stage {}",
                        stage.name
                    )));
                }
            }

            // Check policy for stage start
            // Phase 10: Can derive stage-scoped authority
            let mut stage_authority = execution_authority.clone();

            if let Some(policy) = &self.policy_engine {
                let context = self.build_stage_context(spec, stage, kernel);
                let decision = policy.evaluate(PolicyEvent::OnPipelineStageStart, &context);

                match decision {
                    PolicyDecision::Deny { reason } => {
                        trace.set_final_result(PipelineExecutionResult::Failed {
                            stage_name: stage.name.clone(),
                            error: format!("Policy denied: {}", reason),
                        });
                        return Err(ExecutorError::PolicyDenied {
                            policy: policy.name().to_string(),
                            event: "OnPipelineStageStart".to_string(),
                            reason,
                            pipeline_id: Some(spec.id.to_string()),
                        });
                    }
                    PolicyDecision::Require { action } => {
                        trace.set_final_result(PipelineExecutionResult::Failed {
                            stage_name: stage.name.clone(),
                            error: format!("Policy requires: {}", action),
                        });
                        return Err(ExecutorError::PolicyRequire {
                            policy: policy.name().to_string(),
                            event: "OnPipelineStageStart".to_string(),
                            action,
                            pipeline_id: Some(spec.id.to_string()),
                        });
                    }
                    PolicyDecision::Allow { derived } => {
                        // Phase 10: Apply stage-scoped derived authority
                        if let Some(derived_auth) = derived {
                            let current_authority = stage_authority
                                .clone()
                                .unwrap_or_else(|| self.get_capability_set());

                            // Validate subset
                            if !derived_auth.capabilities.is_subset_of(&current_authority) {
                                let delta = policy::CapabilityDelta::from(
                                    &current_authority,
                                    &derived_auth.capabilities,
                                );
                                trace.set_final_result(PipelineExecutionResult::Failed {
                                    stage_name: stage.name.clone(),
                                    error: "Policy derived authority invalid: not a subset"
                                        .to_string(),
                                });
                                return Err(ExecutorError::PolicyDerivedAuthorityInvalid {
                                    policy: policy.name().to_string(),
                                    event: "OnPipelineStageStart".to_string(),
                                    reason:
                                        "Derived authority grants more capabilities than available"
                                            .to_string(),
                                    delta: format!(
                                        "removed: {:?}, added: {:?}",
                                        delta.removed, delta.added
                                    ),
                                    pipeline_id: Some(spec.id.to_string()),
                                });
                            }

                            // Apply stage-scoped authority (doesn't affect pipeline authority)
                            stage_authority = Some(derived_auth.capabilities);
                        }
                    }
                }
            }

            // Check required capabilities (against stage authority)
            for &cap_id in &stage.required_capabilities {
                if !self.has_capability_with_authority(cap_id, &stage_authority) {
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
            let stage_result = self.execute_stage_with_retry(
                kernel,
                stage,
                current_input.clone(),
                &mut trace,
                &cancellation_token,
            )?;

            // Emit policy event for stage end (audit only)
            if let Some(policy) = &self.policy_engine {
                let context = self.build_stage_context(spec, stage, kernel);
                let _ = policy.evaluate(PolicyEvent::OnPipelineStageEnd, &context);
                // We don't act on this decision - it's audit only
            }

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
                StageResult::Cancelled { reason } => {
                    // Stage was cancelled
                    trace.set_final_result(PipelineExecutionResult::Cancelled {
                        stage_name: stage.name.clone(),
                        reason: reason.clone(),
                    });
                    return Err(ExecutorError::KernelError(format!(
                        "Stage cancelled: {}",
                        reason
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
        cancellation_token: &CancellationToken,
    ) -> Result<StageResult, ExecutorError> {
        let retry_policy = &stage.retry_policy;
        let mut attempt = 0;

        // Calculate stage deadline if timeout is specified
        let stage_deadline = stage
            .timeout_ms
            .map(|ms| Deadline::at(kernel.now() + Duration::from_millis(ms)));

        loop {
            // Check cancellation before each attempt
            if cancellation_token.is_cancelled() {
                let reason = cancellation_token
                    .reason()
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                return Ok(StageResult::Cancelled { reason });
            }

            // Check stage deadline
            if let Some(deadline) = &stage_deadline {
                if deadline.has_passed(kernel.now()) {
                    return Ok(StageResult::Cancelled {
                        reason: "stage timeout".to_string(),
                    });
                }
            }

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
                StageResult::Cancelled { reason } => StageExecutionResult::Cancelled {
                    reason: reason.clone(),
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
                StageResult::Cancelled { .. } => return Ok(result),
                StageResult::Retryable { error } => {
                    // Check if we've exhausted retries
                    // attempt 0 = first try, attempt 1 = first retry, etc.
                    // With max_retries=3, we allow attempts 0,1,2,3 (4 total)
                    // So we check: attempt >= max_retries means we've used all retries
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
                    // Increment attempt first so backoff is calculated correctly
                    attempt += 1;
                    let backoff = retry_policy.backoff_duration(attempt);
                    if backoff > 0 {
                        let _ = kernel.sleep(Duration::from_millis(backoff));
                    }
                }
            }
        }
    }

    /// Executes a stage once (no retry logic)
    ///
    /// This implementation:
    /// 1. Looks up the handler service in registry
    /// 2. Sends a stage invocation request with typed input
    /// 3. Waits for a correlated response
    /// 4. Deserializes the returned stage result
    ///
    /// Contract:
    /// - Request payload: `StageInvokeRequest`
    /// - Response payload: `StageInvokeResponse`
    fn execute_stage_once<K: KernelApi>(
        &self,
        kernel: &mut K,
        stage: &pipeline::StageSpec,
        input: TypedPayload,
    ) -> Result<StageResult, ExecutorError> {
        let channel = kernel
            .lookup_service(stage.handler)
            .map_err(|err| match err {
                kernel_api::KernelError::ServiceNotFound(_) => {
                    ExecutorError::HandlerNotFound(stage.handler.to_string())
                }
                _ => ExecutorError::KernelError(format!(
                    "Failed to lookup stage handler {}: {}",
                    stage.handler, err
                )),
            })?;

        let input_schema_version = input.schema_version;
        let request_payload = StageInvokeRequest { input };
        let message_payload = MessagePayload::new(&request_payload).map_err(|e| {
            ExecutorError::Serialization(format!("Failed to serialize request: {}", e))
        })?;

        let schema_version =
            SchemaVersion::new(input_schema_version.major, input_schema_version.minor);
        let request = MessageEnvelope::new(
            stage.handler,
            stage.action.clone(),
            schema_version,
            message_payload,
        );
        let request_id = request.id;

        kernel.send_message(channel, request).map_err(|e| {
            ExecutorError::KernelError(format!("Failed to send stage request: {}", e))
        })?;

        let timeout = stage.timeout_ms.map(Duration::from_millis);
        let response = kernel.receive_message(channel, timeout).map_err(|e| {
            ExecutorError::KernelError(format!("Failed to receive stage response: {}", e))
        })?;

        if response.correlation_id != Some(request_id) {
            return Err(ExecutorError::KernelError(format!(
                "Stage response correlation mismatch: expected {}, got {:?}",
                request_id, response.correlation_id
            )));
        }

        let decoded: StageInvokeResponse = response.payload.deserialize().map_err(|e| {
            ExecutorError::Serialization(format!("Failed to deserialize stage response: {}", e))
        })?;
        Ok(decoded.result)
    }

    /// Builds policy context for pipeline-level events
    fn build_pipeline_context<K: KernelApi>(
        &self,
        spec: &PipelineSpec,
        kernel: &K,
    ) -> PolicyContext {
        let actor_identity = self.identity.clone().unwrap_or_else(|| {
            use identity::{IdentityKind, TrustDomain};
            IdentityMetadata::new(
                IdentityKind::Service,
                TrustDomain::core(),
                "pipeline-executor",
                kernel.now().as_nanos(),
            )
        });

        let mut context = PolicyContext::for_pipeline(actor_identity, spec.id);

        // Add metadata
        if let Some(timeout_ms) = spec.timeout_ms {
            context = context.with_metadata("timeout_ms", timeout_ms.to_string());
        }
        context = context.with_metadata("stage_count", spec.stages.len().to_string());

        context
    }

    /// Builds policy context for stage-level events
    fn build_stage_context<K: KernelApi>(
        &self,
        spec: &PipelineSpec,
        stage: &pipeline::StageSpec,
        kernel: &K,
    ) -> PolicyContext {
        let actor_identity = self.identity.clone().unwrap_or_else(|| {
            use identity::{IdentityKind, TrustDomain};
            IdentityMetadata::new(
                IdentityKind::Service,
                TrustDomain::core(),
                "pipeline-executor",
                kernel.now().as_nanos(),
            )
        });

        let mut context = PolicyContext::for_pipeline(actor_identity, spec.id);
        context.stage_id = Some(stage.id);

        // Add stage metadata
        if let Some(timeout_ms) = stage.timeout_ms {
            context = context.with_metadata("stage_timeout_ms", timeout_ms.to_string());
        }
        context = context.with_metadata("retry_max", stage.retry_policy.max_retries.to_string());
        context = context.with_metadata(
            "required_capabilities",
            format!("{:?}", stage.required_capabilities),
        );

        context
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
    use core_types::ServiceId;
    use ipc::{ChannelId, MessageEnvelope, MessagePayload, SchemaVersion};
    use pipeline::{PayloadSchemaId, PayloadSchemaVersion};
    use std::collections::HashMap;

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

        let token = CancellationToken::none();
        let result = executor.execute(&mut mock_kernel, &spec, input, token);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_stage_once_invokes_handler_over_ipc() {
        let executor = PipelineExecutor::new();
        let mut mock_kernel = MockKernel::new();
        let handler_id = ServiceId::from_u128(0xA11CE);
        let channel = ChannelId::new();
        mock_kernel.register_handler(handler_id, channel);

        let output = TypedPayload::new(
            PayloadSchemaId::new("out"),
            PayloadSchemaVersion::new(1, 0),
            b"{\"ok\":true}".to_vec(),
        );
        mock_kernel.set_next_stage_result(StageResult::Success {
            output: output.clone(),
            capabilities: vec![42],
        });

        let stage = pipeline::StageSpec::new(
            "test-stage".to_string(),
            handler_id,
            "transform".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        );
        let input = TypedPayload::new(
            PayloadSchemaId::new("in"),
            PayloadSchemaVersion::new(1, 0),
            b"{\"value\":1}".to_vec(),
        );

        let result = executor
            .execute_stage_once(&mut mock_kernel, &stage, input.clone())
            .unwrap();

        match result {
            StageResult::Success {
                output: returned_output,
                capabilities,
            } => {
                assert_eq!(returned_output.schema_id, output.schema_id);
                assert_eq!(capabilities, vec![42]);
            }
            other => panic!("Expected success response, got {:?}", other),
        }

        let (sent_channel, sent_message) = mock_kernel.last_sent().expect("message was sent");
        assert_eq!(sent_channel, channel);
        assert_eq!(sent_message.destination, handler_id);
        assert_eq!(sent_message.action, "transform");
        assert_eq!(sent_message.schema_version, SchemaVersion::new(1, 0));

        let request: StageInvokeRequest = sent_message.payload.deserialize().unwrap();
        assert_eq!(request.input.schema_id.as_str(), "in");
    }

    #[test]
    fn test_execute_stage_once_returns_handler_not_found() {
        let executor = PipelineExecutor::new();
        let mut mock_kernel = MockKernel::new();
        let missing_handler = ServiceId::from_u128(0xDEAD);

        let stage = pipeline::StageSpec::new(
            "missing".to_string(),
            missing_handler,
            "do_work".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        );
        let input = TypedPayload::new(
            PayloadSchemaId::new("in"),
            PayloadSchemaVersion::new(1, 0),
            vec![],
        );

        let result = executor.execute_stage_once(&mut mock_kernel, &stage, input);
        match result {
            Err(ExecutorError::HandlerNotFound(handler)) => {
                assert_eq!(handler, missing_handler.to_string());
            }
            other => panic!("Expected HandlerNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_stage_once_rejects_mismatched_correlation() {
        let executor = PipelineExecutor::new();
        let mut mock_kernel = MockKernel::new();
        let handler_id = ServiceId::from_u128(0xBEEF);
        let channel = ChannelId::new();
        mock_kernel.register_handler(handler_id, channel);
        mock_kernel.set_force_bad_correlation(true);
        mock_kernel.set_next_stage_result(StageResult::Failure {
            error: "should not decode".to_string(),
        });

        let stage = pipeline::StageSpec::new(
            "correlation".to_string(),
            handler_id,
            "check".to_string(),
            PayloadSchemaId::new("in"),
            PayloadSchemaId::new("out"),
        );
        let input = TypedPayload::new(
            PayloadSchemaId::new("in"),
            PayloadSchemaVersion::new(1, 0),
            vec![],
        );

        let result = executor.execute_stage_once(&mut mock_kernel, &stage, input);
        match result {
            Err(ExecutorError::KernelError(msg)) => {
                assert!(msg.contains("correlation mismatch"));
            }
            other => panic!("Expected correlation mismatch KernelError, got {:?}", other),
        }
    }

    // Mock kernel for testing
    struct MockKernel {
        time_ms: u64,
        handlers: HashMap<ServiceId, ChannelId>,
        sent: Vec<(ChannelId, MessageEnvelope)>,
        next_stage_result: Option<StageResult>,
        force_bad_correlation: bool,
    }

    impl MockKernel {
        fn new() -> Self {
            Self {
                time_ms: 0,
                handlers: HashMap::new(),
                sent: Vec::new(),
                next_stage_result: None,
                force_bad_correlation: false,
            }
        }

        fn register_handler(&mut self, service_id: ServiceId, channel: ChannelId) {
            self.handlers.insert(service_id, channel);
        }

        fn set_next_stage_result(&mut self, result: StageResult) {
            self.next_stage_result = Some(result);
        }

        fn set_force_bad_correlation(&mut self, enabled: bool) {
            self.force_bad_correlation = enabled;
        }

        fn last_sent(&self) -> Option<(ChannelId, MessageEnvelope)> {
            self.sent.last().cloned()
        }
    }

    impl KernelApi for MockKernel {
        fn spawn_task(
            &mut self,
            _descriptor: kernel_api::TaskDescriptor,
        ) -> Result<kernel_api::TaskHandle, kernel_api::KernelError> {
            Err(kernel_api::KernelError::SpawnFailed(
                "not implemented in test mock".to_string(),
            ))
        }

        fn create_channel(&mut self) -> Result<ChannelId, kernel_api::KernelError> {
            Ok(ChannelId::new())
        }

        fn send_message(
            &mut self,
            channel: ChannelId,
            message: MessageEnvelope,
        ) -> Result<(), kernel_api::KernelError> {
            self.sent.push((channel, message));
            Ok(())
        }

        fn receive_message(
            &mut self,
            channel: ChannelId,
            _timeout: Option<Duration>,
        ) -> Result<MessageEnvelope, kernel_api::KernelError> {
            let (_, last_request) = self
                .sent
                .iter()
                .rev()
                .find(|(sent_channel, _)| *sent_channel == channel)
                .ok_or_else(|| {
                    kernel_api::KernelError::ReceiveFailed("no request sent".to_string())
                })?;

            let result = self.next_stage_result.clone().ok_or_else(|| {
                kernel_api::KernelError::ReceiveFailed("no staged response configured".to_string())
            })?;
            let response_payload = StageInvokeResponse { result };
            let payload = MessagePayload::new(&response_payload).map_err(|e| {
                kernel_api::KernelError::ReceiveFailed(format!(
                    "failed to encode test response payload: {}",
                    e
                ))
            })?;

            let correlation_id = if self.force_bad_correlation {
                Some(ipc::MessageId::new())
            } else {
                Some(last_request.id)
            };

            let mut response = MessageEnvelope::new(
                last_request.destination,
                last_request.action.clone(),
                last_request.schema_version,
                payload,
            );
            response.correlation_id = correlation_id;
            Ok(response)
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
            Ok(())
        }

        fn register_service(
            &mut self,
            service_id: core_types::ServiceId,
            channel: ChannelId,
        ) -> Result<(), kernel_api::KernelError> {
            self.handlers.insert(service_id, channel);
            Ok(())
        }

        fn lookup_service(
            &self,
            service_id: core_types::ServiceId,
        ) -> Result<ChannelId, kernel_api::KernelError> {
            self.handlers
                .get(&service_id)
                .copied()
                .ok_or_else(|| kernel_api::KernelError::ServiceNotFound(service_id.to_string()))
        }
    }
}
