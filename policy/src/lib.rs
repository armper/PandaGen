//! # Policy Engine Framework
//!
//! This crate provides pluggable policy engines for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Mechanism not policy**: Core provides primitives, services decide policy
//! - **Policy observes; it does not own**: Authority comes from capabilities
//! - **Explicit and testable**: All policy logic runs under SimKernel
//! - **Advisory + enforceable**: Policies make decisions, enforcement points apply them
//! - **Pluggable and removable**: System works without policy engines
//!
//! ## Core Concepts
//!
//! - `PolicyEngine`: Trait for evaluating policy decisions
//! - `PolicyDecision`: Allow, Deny, or Require
//! - `PolicyContext`: Context information for policy evaluation
//! - `PolicyEvent`: System events that trigger policy evaluation
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - A global permissions system
//! - POSIX users/groups/ACLs
//! - Authentication or cryptography
//! - A hard-coded rules engine
//!
//! Policy provides governance, not control. Authority comes from capabilities.

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use pipeline::{PipelineId, StageId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Policy decision returned by policy engines
///
/// Decisions are explicit: allow, deny, or require additional action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// Operation is allowed to proceed
    Allow,
    /// Operation is denied with a specific reason
    Deny { reason: String },
    /// Operation requires additional action before proceeding
    Require { action: String },
}

impl PolicyDecision {
    /// Creates an Allow decision
    pub fn allow() -> Self {
        Self::Allow
    }

    /// Creates a Deny decision with a reason
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    /// Creates a Require decision with an action
    pub fn require(action: impl Into<String>) -> Self {
        Self::Require {
            action: action.into(),
        }
    }

    /// Checks if decision is Allow
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Checks if decision is Deny
    pub fn is_deny(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }

    /// Checks if decision is Require
    pub fn is_require(&self) -> bool {
        matches!(self, Self::Require { .. })
    }
}

impl fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => write!(f, "Allow"),
            Self::Deny { reason } => write!(f, "Deny: {}", reason),
            Self::Require { action } => write!(f, "Require: {}", action),
        }
    }
}

/// Context information for policy evaluation
///
/// Contains all relevant information about an operation for policy engines
/// to make informed decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyContext {
    /// Execution identity performing the operation
    pub actor_identity: IdentityMetadata,
    /// Target identity (if applicable)
    pub target_identity: Option<IdentityMetadata>,
    /// Capability involved (if any)
    pub capability_id: Option<u64>,
    /// Pipeline ID (if applicable)
    pub pipeline_id: Option<PipelineId>,
    /// Stage ID (if applicable)
    pub stage_id: Option<StageId>,
    /// Additional context-specific data
    pub metadata: Vec<(String, String)>,
}

impl PolicyContext {
    /// Creates a new policy context for a spawn operation
    pub fn for_spawn(actor_identity: IdentityMetadata, target_identity: IdentityMetadata) -> Self {
        Self {
            actor_identity,
            target_identity: Some(target_identity),
            capability_id: None,
            pipeline_id: None,
            stage_id: None,
            metadata: Vec::new(),
        }
    }

    /// Creates a new policy context for a capability delegation
    pub fn for_capability_delegation(
        from_identity: IdentityMetadata,
        to_identity: IdentityMetadata,
        cap_id: u64,
    ) -> Self {
        Self {
            actor_identity: from_identity,
            target_identity: Some(to_identity),
            capability_id: Some(cap_id),
            pipeline_id: None,
            stage_id: None,
            metadata: Vec::new(),
        }
    }

    /// Creates a new policy context for a pipeline execution
    pub fn for_pipeline(actor_identity: IdentityMetadata, pipeline_id: PipelineId) -> Self {
        Self {
            actor_identity,
            target_identity: None,
            capability_id: None,
            pipeline_id: Some(pipeline_id),
            stage_id: None,
            metadata: Vec::new(),
        }
    }

    /// Adds metadata to the context
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.push((key.into(), value.into()));
        self
    }

    /// Checks if actor and target are in the same trust domain
    pub fn is_same_domain(&self) -> bool {
        if let Some(target) = &self.target_identity {
            self.actor_identity.same_domain(target)
        } else {
            true
        }
    }

    /// Checks if actor and target are in different trust domains
    pub fn is_cross_domain(&self) -> bool {
        !self.is_same_domain()
    }
}

/// Policy events that trigger evaluation
///
/// Events represent specific system operations that may require policy checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyEvent {
    /// Task/service spawn
    OnSpawn,
    /// Task/service termination
    OnTerminate,
    /// Capability delegation between tasks
    OnCapabilityDelegate,
    /// Pipeline execution start
    OnPipelineStart,
    /// Pipeline stage start
    OnPipelineStageStart,
    /// Pipeline stage end
    OnPipelineStageEnd,
}

impl fmt::Display for PolicyEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnSpawn => write!(f, "OnSpawn"),
            Self::OnTerminate => write!(f, "OnTerminate"),
            Self::OnCapabilityDelegate => write!(f, "OnCapabilityDelegate"),
            Self::OnPipelineStart => write!(f, "OnPipelineStart"),
            Self::OnPipelineStageStart => write!(f, "OnPipelineStageStart"),
            Self::OnPipelineStageEnd => write!(f, "OnPipelineStageEnd"),
        }
    }
}

/// Policy engine trait
///
/// Policy engines evaluate operations and return decisions.
/// Engines must be deterministic and side-effect free.
pub trait PolicyEngine: Send + Sync {
    /// Evaluates a policy for the given event and context
    ///
    /// Must be deterministic: same inputs always produce same outputs.
    /// Must be side-effect free: does not modify system state.
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision;

    /// Returns the name of this policy engine (for logging/audit)
    fn name(&self) -> &str;
}

/// Reference implementation: No-op policy
///
/// Always allows all operations. Used to prove system works without policy.
#[derive(Debug, Clone)]
pub struct NoOpPolicy;

impl PolicyEngine for NoOpPolicy {
    fn evaluate(&self, _event: PolicyEvent, _context: &PolicyContext) -> PolicyDecision {
        PolicyDecision::Allow
    }

    fn name(&self) -> &str {
        "NoOpPolicy"
    }
}

/// Reference implementation: Trust domain policy
///
/// Enforces trust boundary rules:
/// - Sandbox domain cannot spawn System services
/// - Cross-domain capability delegation requires explicit opt-in
#[derive(Debug, Clone)]
pub struct TrustDomainPolicy;

impl PolicyEngine for TrustDomainPolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnSpawn => {
                // Sandbox cannot spawn System services
                if context.actor_identity.trust_domain == TrustDomain::sandbox()
                    && context
                        .target_identity
                        .as_ref()
                        .is_some_and(|t| t.kind == IdentityKind::System)
                {
                    return PolicyDecision::deny("Sandbox domain cannot spawn System services");
                }

                // Sandbox cannot spawn core services
                if context.actor_identity.trust_domain == TrustDomain::sandbox()
                    && context
                        .target_identity
                        .as_ref()
                        .is_some_and(|t| t.trust_domain == TrustDomain::core())
                {
                    return PolicyDecision::deny(
                        "Sandbox domain cannot spawn services in core domain",
                    );
                }

                PolicyDecision::Allow
            }
            PolicyEvent::OnCapabilityDelegate => {
                // Cross-domain delegation requires explicit opt-in
                if context.is_cross_domain() {
                    return PolicyDecision::require(
                        "Cross-domain capability delegation requires explicit approval",
                    );
                }
                PolicyDecision::Allow
            }
            _ => PolicyDecision::Allow,
        }
    }

    fn name(&self) -> &str {
        "TrustDomainPolicy"
    }
}

/// Reference implementation: Pipeline safety policy
///
/// Enforces pipeline safety rules:
/// - Pipelines in user domain must have a timeout
/// - Pipelines longer than N stages must be supervised
#[derive(Debug, Clone)]
pub struct PipelineSafetyPolicy {
    /// Maximum stages allowed without supervision
    pub max_stages_unsupervised: usize,
}

impl PipelineSafetyPolicy {
    /// Creates a new pipeline safety policy with default limits
    pub fn new() -> Self {
        Self {
            max_stages_unsupervised: 5,
        }
    }

    /// Creates a policy with custom stage limit
    pub fn with_max_stages(max_stages: usize) -> Self {
        Self {
            max_stages_unsupervised: max_stages,
        }
    }
}

impl Default for PipelineSafetyPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for PipelineSafetyPolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnPipelineStart => {
                // Pipelines in user domain must have timeout
                if context.actor_identity.trust_domain == TrustDomain::user() {
                    // Check if timeout metadata is present
                    let has_timeout = context.metadata.iter().any(|(k, _)| k == "timeout_ms");

                    if !has_timeout {
                        return PolicyDecision::require(
                            "Pipelines in user domain must specify a timeout",
                        );
                    }
                }

                // Check stage count if provided
                if let Some((_, stage_count_str)) =
                    context.metadata.iter().find(|(k, _)| k == "stage_count")
                {
                    if let Ok(stage_count) = stage_count_str.parse::<usize>() {
                        if stage_count > self.max_stages_unsupervised {
                            return PolicyDecision::require(
                                format!(
                                    "Pipelines with {} stages require supervision (max unsupervised: {})",
                                    stage_count, self.max_stages_unsupervised
                                ),
                            );
                        }
                    }
                }

                PolicyDecision::Allow
            }
            _ => PolicyDecision::Allow,
        }
    }

    fn name(&self) -> &str {
        "PipelineSafetyPolicy"
    }
}

/// Composed policy engine
///
/// Evaluates multiple policies in order:
/// - First Deny wins
/// - All Require decisions must be satisfied
/// - Allow only if no Deny and all Requires handled
pub struct ComposedPolicy {
    policies: Vec<Box<dyn PolicyEngine>>,
}

impl ComposedPolicy {
    /// Creates a new composed policy
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Adds a policy to the composition
    pub fn add_policy(mut self, policy: Box<dyn PolicyEngine>) -> Self {
        self.policies.push(policy);
        self
    }

    /// Evaluates all policies and combines decisions
    ///
    /// Rules:
    /// - First Deny wins (short-circuit)
    /// - Collect all Require decisions
    /// - Return Allow if no Deny and no Require
    pub fn evaluate_all(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        let mut requires = Vec::new();

        for policy in &self.policies {
            match policy.evaluate(event.clone(), context) {
                PolicyDecision::Deny { reason } => {
                    // First deny wins
                    return PolicyDecision::Deny { reason };
                }
                PolicyDecision::Require { action } => {
                    requires.push(action);
                }
                PolicyDecision::Allow => {
                    // Continue evaluating
                }
            }
        }

        if !requires.is_empty() {
            // Return combined Require
            PolicyDecision::Require {
                action: requires.join("; "),
            }
        } else {
            PolicyDecision::Allow
        }
    }
}

impl Default for ComposedPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for ComposedPolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        self.evaluate_all(event, context)
    }

    fn name(&self) -> &str {
        "ComposedPolicy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_decision_allow() {
        let decision = PolicyDecision::allow();
        assert!(decision.is_allow());
        assert!(!decision.is_deny());
        assert!(!decision.is_require());
    }

    #[test]
    fn test_policy_decision_deny() {
        let decision = PolicyDecision::deny("test reason");
        assert!(!decision.is_allow());
        assert!(decision.is_deny());
        assert!(!decision.is_require());
        assert_eq!(decision.to_string(), "Deny: test reason");
    }

    #[test]
    fn test_policy_decision_require() {
        let decision = PolicyDecision::require("test action");
        assert!(!decision.is_allow());
        assert!(!decision.is_deny());
        assert!(decision.is_require());
        assert_eq!(decision.to_string(), "Require: test action");
    }

    #[test]
    fn test_noop_policy() {
        let policy = NoOpPolicy;
        let context = PolicyContext::for_spawn(
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "test", 0),
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "target", 0),
        );

        let decision = policy.evaluate(PolicyEvent::OnSpawn, &context);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_trust_domain_policy_sandbox_cannot_spawn_system() {
        let policy = TrustDomainPolicy;
        let context = PolicyContext::for_spawn(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::sandbox(),
                "sandboxed",
                0,
            ),
            IdentityMetadata::new(IdentityKind::System, TrustDomain::core(), "system", 0),
        );

        let decision = policy.evaluate(PolicyEvent::OnSpawn, &context);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_trust_domain_policy_sandbox_cannot_spawn_core() {
        let policy = TrustDomainPolicy;
        let context = PolicyContext::for_spawn(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::sandbox(),
                "sandboxed",
                0,
            ),
            IdentityMetadata::new(
                IdentityKind::Service,
                TrustDomain::core(),
                "core-service",
                0,
            ),
        );

        let decision = policy.evaluate(PolicyEvent::OnSpawn, &context);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_trust_domain_policy_cross_domain_delegation_requires() {
        let policy = TrustDomainPolicy;
        let context = PolicyContext::for_capability_delegation(
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service1", 0),
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::user(),
                "user-component",
                0,
            ),
            42,
        );

        let decision = policy.evaluate(PolicyEvent::OnCapabilityDelegate, &context);
        assert!(decision.is_require());
    }

    #[test]
    fn test_trust_domain_policy_same_domain_delegation_allowed() {
        let policy = TrustDomainPolicy;
        let context = PolicyContext::for_capability_delegation(
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service1", 0),
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service2", 0),
            42,
        );

        let decision = policy.evaluate(PolicyEvent::OnCapabilityDelegate, &context);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_pipeline_safety_policy_user_domain_requires_timeout() {
        let policy = PipelineSafetyPolicy::new();
        let context = PolicyContext::for_pipeline(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::user(),
                "user-pipeline",
                0,
            ),
            PipelineId::new(),
        );

        let decision = policy.evaluate(PolicyEvent::OnPipelineStart, &context);
        assert!(decision.is_require());
    }

    #[test]
    fn test_pipeline_safety_policy_user_domain_with_timeout_allowed() {
        let policy = PipelineSafetyPolicy::new();
        let context = PolicyContext::for_pipeline(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::user(),
                "user-pipeline",
                0,
            ),
            PipelineId::new(),
        )
        .with_metadata("timeout_ms", "5000");

        let decision = policy.evaluate(PolicyEvent::OnPipelineStart, &context);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_pipeline_safety_policy_too_many_stages() {
        let policy = PipelineSafetyPolicy::with_max_stages(3);
        let context = PolicyContext::for_pipeline(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::core(),
                "large-pipeline",
                0,
            ),
            PipelineId::new(),
        )
        .with_metadata("stage_count", "5")
        .with_metadata("timeout_ms", "10000");

        let decision = policy.evaluate(PolicyEvent::OnPipelineStart, &context);
        assert!(decision.is_require());
    }

    #[test]
    fn test_composed_policy_first_deny_wins() {
        let mut composed = ComposedPolicy::new();
        composed = composed.add_policy(Box::new(NoOpPolicy)); // Allow
        composed = composed.add_policy(Box::new(TrustDomainPolicy)); // Will deny

        let context = PolicyContext::for_spawn(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::sandbox(),
                "sandboxed",
                0,
            ),
            IdentityMetadata::new(IdentityKind::System, TrustDomain::core(), "system", 0),
        );

        let decision = composed.evaluate(PolicyEvent::OnSpawn, &context);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_composed_policy_collect_requires() {
        let mut composed = ComposedPolicy::new();
        composed = composed.add_policy(Box::new(TrustDomainPolicy)); // Will require
        composed = composed.add_policy(Box::new(PipelineSafetyPolicy::new())); // Will require

        let context = PolicyContext::for_pipeline(
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::user(), "pipeline", 0),
            PipelineId::new(),
        );

        // TrustDomainPolicy allows pipeline start, PipelineSafetyPolicy requires timeout
        let decision = composed.evaluate(PolicyEvent::OnPipelineStart, &context);
        assert!(decision.is_require());
    }

    #[test]
    fn test_composed_policy_all_allow() {
        let mut composed = ComposedPolicy::new();
        composed = composed.add_policy(Box::new(NoOpPolicy));
        composed = composed.add_policy(Box::new(NoOpPolicy));

        let context = PolicyContext::for_spawn(
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service1", 0),
            IdentityMetadata::new(IdentityKind::Service, TrustDomain::core(), "service2", 0),
        );

        let decision = composed.evaluate(PolicyEvent::OnSpawn, &context);
        assert!(decision.is_allow());
    }
}
