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

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use pipeline::{PipelineId, StageId};
use serde::{Deserialize, Serialize};

// ============================================================================
// Phase 10: Capability Set and Derived Authority Types
// ============================================================================

/// A set of capabilities
///
/// Represents a collection of capabilities available to an execution context.
/// Used for capability derivation and subset validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Set of capability IDs
    pub capabilities: BTreeSet<u64>,
}

impl CapabilitySet {
    /// Creates a new empty capability set
    pub fn new() -> Self {
        Self {
            capabilities: BTreeSet::new(),
        }
    }

    /// Creates a capability set from a vector of capability IDs
    pub fn from_capabilities(caps: Vec<u64>) -> Self {
        Self {
            capabilities: caps.into_iter().collect(),
        }
    }

    /// Checks if this set is a subset of another set
    ///
    /// Returns true if all capabilities in this set are present in the other set.
    pub fn is_subset_of(&self, other: &CapabilitySet) -> bool {
        self.capabilities.is_subset(&other.capabilities)
    }

    /// Returns the intersection of this set with another
    pub fn intersection(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self
                .capabilities
                .intersection(&other.capabilities)
                .copied()
                .collect(),
        }
    }

    /// Returns the difference of this set with another (elements in self but not in other)
    pub fn difference(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self
                .capabilities
                .difference(&other.capabilities)
                .copied()
                .collect(),
        }
    }

    /// Returns true if the set is empty
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Returns the number of capabilities in the set
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Returns a vector of capability IDs
    pub fn to_vec(&self) -> Vec<u64> {
        let mut caps: Vec<u64> = self.capabilities.iter().copied().collect();
        caps.sort_unstable();
        caps
    }
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a derived (restricted) authority
///
/// Contains capabilities that have been restricted from the original authority.
/// Must always be a subset of or equal to the original authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedAuthority {
    /// The restricted set of capabilities
    pub capabilities: CapabilitySet,
    /// Optional constraints (for future use, currently unused)
    #[serde(default)]
    pub constraints: Vec<String>,
}

impl DerivedAuthority {
    /// Creates a new derived authority with the given capabilities
    pub fn new(capabilities: CapabilitySet) -> Self {
        Self {
            capabilities,
            constraints: Vec::new(),
        }
    }

    /// Creates a derived authority from a vector of capability IDs
    pub fn from_capabilities(caps: Vec<u64>) -> Self {
        Self::new(CapabilitySet::from_capabilities(caps))
    }

    /// Adds a constraint to this derived authority
    pub fn with_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.constraints.push(constraint.into());
        self
    }
}

/// Describes changes made to capabilities
///
/// Used to explain what changed when deriving authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDelta {
    /// Capabilities that were removed
    pub removed: Vec<u64>,
    /// Capabilities that were restricted (for future use)
    #[serde(default)]
    pub restricted: Vec<String>,
    /// Capabilities that were added (should be empty for now - no escalation)
    #[serde(default)]
    pub added: Vec<u64>,
}

impl CapabilityDelta {
    /// Creates a new empty capability delta
    pub fn new() -> Self {
        Self {
            removed: Vec::new(),
            restricted: Vec::new(),
            added: Vec::new(),
        }
    }

    /// Computes the delta between before and after capability sets
    ///
    /// - `removed`: capabilities in `before` but not in `after`
    /// - `added`: capabilities in `after` but not in `before`
    pub fn from(before: &CapabilitySet, after: &CapabilitySet) -> Self {
        let removed = before.difference(after).to_vec();
        let added = after.difference(before).to_vec();

        Self {
            removed,
            restricted: Vec::new(),
            added,
        }
    }

    /// Returns true if there are no changes
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.restricted.is_empty() && self.added.is_empty()
    }
}

impl Default for CapabilityDelta {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy decision returned by policy engines
///
/// Decisions are explicit: allow, deny, or require additional action.
/// The Allow variant can optionally include derived (restricted) authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// Operation is allowed to proceed, optionally with derived authority
    Allow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        derived: Option<DerivedAuthority>,
    },
    /// Operation is denied with a specific reason
    Deny { reason: String },
    /// Operation requires additional action before proceeding
    Require { action: String },
}

impl PolicyDecision {
    /// Creates an Allow decision without derived authority
    pub fn allow() -> Self {
        Self::Allow { derived: None }
    }

    /// Creates an Allow decision with derived authority
    pub fn allow_with_derived(derived: DerivedAuthority) -> Self {
        Self::Allow {
            derived: Some(derived),
        }
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

    /// Checks if decision is Allow (with or without derived authority)
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    /// Checks if decision is Deny
    pub fn is_deny(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }

    /// Checks if decision is Require
    pub fn is_require(&self) -> bool {
        matches!(self, Self::Require { .. })
    }

    /// Returns the derived authority if present
    pub fn derived_authority(&self) -> Option<&DerivedAuthority> {
        match self {
            Self::Allow { derived } => derived.as_ref(),
            _ => None,
        }
    }
}

impl fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow { derived } => {
                if derived.is_some() {
                    write!(f, "Allow (with derived authority)")
                } else {
                    write!(f, "Allow")
                }
            }
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
        PolicyDecision::Allow { derived: None }
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

                PolicyDecision::Allow { derived: None }
            }
            PolicyEvent::OnCapabilityDelegate => {
                // Cross-domain delegation requires explicit opt-in
                if context.is_cross_domain() {
                    return PolicyDecision::require(
                        "Cross-domain capability delegation requires explicit approval",
                    );
                }
                PolicyDecision::Allow { derived: None }
            }
            _ => PolicyDecision::Allow { derived: None },
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

                PolicyDecision::Allow { derived: None }
            }
            _ => PolicyDecision::Allow { derived: None },
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
    /// - Phase 10: Derived authority from the most restrictive policy is used
    pub fn evaluate_all(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        let mut requires = Vec::new();
        let mut most_restrictive_derived: Option<DerivedAuthority> = None;

        for policy in &self.policies {
            match policy.evaluate(event.clone(), context) {
                PolicyDecision::Deny { reason } => {
                    // First deny wins
                    return PolicyDecision::Deny { reason };
                }
                PolicyDecision::Require { action } => {
                    requires.push(action);
                }
                PolicyDecision::Allow { derived } => {
                    // Track the most restrictive derived authority
                    if let Some(new_derived) = derived {
                        most_restrictive_derived = Some(match most_restrictive_derived {
                            None => new_derived,
                            Some(existing) => {
                                // Take the intersection of both capability sets
                                let intersection = existing
                                    .capabilities
                                    .intersection(&new_derived.capabilities);
                                DerivedAuthority::new(intersection)
                            }
                        });
                    }
                }
            }
        }

        if !requires.is_empty() {
            // Return combined Require
            PolicyDecision::Require {
                action: requires.join("; "),
            }
        } else {
            PolicyDecision::Allow {
                derived: most_restrictive_derived,
            }
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

/// Policy decision report with full explanation
///
/// Provides detailed information about policy evaluation including
/// which policies were checked and what decisions they made.
/// Phase 10: Now includes capability derivation information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecisionReport {
    /// Final aggregated decision
    pub decision: PolicyDecision,
    /// Individual policy evaluations
    pub evaluated_policies: Vec<PolicyEvaluation>,
    /// Final deny reason (if decision is Deny)
    pub deny_reason: Option<String>,
    /// Required actions (if decision is Require)
    pub required_actions: Vec<String>,
    /// Input capabilities (before policy evaluation)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_capabilities: Option<CapabilitySet>,
    /// Output capabilities (after policy evaluation, if derived)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_capabilities: Option<CapabilitySet>,
    /// Capability delta (changes made by policy)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_delta: Option<CapabilityDelta>,
}

/// Single policy evaluation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    /// Name of the policy engine
    pub policy_name: String,
    /// Decision made by this policy
    pub decision: PolicyDecision,
}

impl PolicyDecisionReport {
    /// Creates a new report with a single decision
    pub fn new(policy_name: impl Into<String>, decision: PolicyDecision) -> Self {
        let policy_name = policy_name.into();
        let (deny_reason, required_actions) = match &decision {
            PolicyDecision::Deny { reason } => (Some(reason.clone()), Vec::new()),
            PolicyDecision::Require { action } => (None, vec![action.clone()]),
            PolicyDecision::Allow { .. } => (None, Vec::new()),
        };

        Self {
            decision: decision.clone(),
            evaluated_policies: vec![PolicyEvaluation {
                policy_name,
                decision,
            }],
            deny_reason,
            required_actions,
            input_capabilities: None,
            output_capabilities: None,
            capability_delta: None,
        }
    }

    /// Creates a report with capability information
    pub fn with_capabilities(
        mut self,
        input: CapabilitySet,
        output: Option<CapabilitySet>,
    ) -> Self {
        let delta = output
            .as_ref()
            .map(|out| CapabilityDelta::from(&input, out));

        self.input_capabilities = Some(input);
        self.output_capabilities = output;
        self.capability_delta = delta;
        self
    }

    /// Creates a report from composed policy evaluation
    pub fn from_composed(
        evaluations: Vec<(String, PolicyDecision)>,
        final_decision: PolicyDecision,
    ) -> Self {
        let mut deny_reason = None;
        let mut required_actions = Vec::new();

        for (_, decision) in &evaluations {
            match decision {
                PolicyDecision::Deny { reason } => {
                    if deny_reason.is_none() {
                        deny_reason = Some(reason.clone());
                    }
                }
                PolicyDecision::Require { action } => {
                    required_actions.push(action.clone());
                }
                PolicyDecision::Allow { .. } => {}
            }
        }

        Self {
            decision: final_decision,
            evaluated_policies: evaluations
                .into_iter()
                .map(|(policy_name, decision)| PolicyEvaluation {
                    policy_name,
                    decision,
                })
                .collect(),
            deny_reason,
            required_actions,
            input_capabilities: None,
            output_capabilities: None,
            capability_delta: None,
        }
    }

    /// Returns true if the final decision is Allow
    pub fn is_allow(&self) -> bool {
        self.decision.is_allow()
    }

    /// Returns true if the final decision is Deny
    pub fn is_deny(&self) -> bool {
        self.decision.is_deny()
    }

    /// Returns true if the final decision is Require
    pub fn is_require(&self) -> bool {
        self.decision.is_require()
    }
}

impl ComposedPolicy {
    /// Evaluates all policies and returns a detailed report
    pub fn evaluate_with_report(
        &self,
        event: PolicyEvent,
        context: &PolicyContext,
    ) -> PolicyDecisionReport {
        let mut evaluations = Vec::new();

        for policy in &self.policies {
            let decision = policy.evaluate(event.clone(), context);
            evaluations.push((policy.name().to_string(), decision));
        }

        let final_decision = self.evaluate_all(event, context);
        PolicyDecisionReport::from_composed(evaluations, final_decision)
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

    #[test]
    fn test_policy_decision_report_allow() {
        let report = PolicyDecisionReport::new("TestPolicy", PolicyDecision::allow());
        assert!(report.is_allow());
        assert!(!report.is_deny());
        assert!(!report.is_require());
        assert_eq!(report.evaluated_policies.len(), 1);
        assert_eq!(report.evaluated_policies[0].policy_name, "TestPolicy");
        assert!(report.deny_reason.is_none());
        assert!(report.required_actions.is_empty());
    }

    #[test]
    fn test_policy_decision_report_deny() {
        let report = PolicyDecisionReport::new("TestPolicy", PolicyDecision::deny("access denied"));
        assert!(!report.is_allow());
        assert!(report.is_deny());
        assert!(!report.is_require());
        assert_eq!(report.deny_reason, Some("access denied".to_string()));
        assert!(report.required_actions.is_empty());
    }

    #[test]
    fn test_policy_decision_report_require() {
        let report =
            PolicyDecisionReport::new("TestPolicy", PolicyDecision::require("add timeout"));
        assert!(!report.is_allow());
        assert!(!report.is_deny());
        assert!(report.is_require());
        assert!(report.deny_reason.is_none());
        assert_eq!(report.required_actions, vec!["add timeout"]);
    }

    #[test]
    fn test_composed_policy_report_with_deny() {
        let mut composed = ComposedPolicy::new();
        composed = composed.add_policy(Box::new(NoOpPolicy));
        composed = composed.add_policy(Box::new(TrustDomainPolicy));

        let context = PolicyContext::for_spawn(
            IdentityMetadata::new(
                IdentityKind::Component,
                TrustDomain::sandbox(),
                "sandboxed",
                0,
            ),
            IdentityMetadata::new(IdentityKind::System, TrustDomain::core(), "system", 0),
        );

        let report = composed.evaluate_with_report(PolicyEvent::OnSpawn, &context);
        assert!(report.is_deny());
        assert_eq!(report.evaluated_policies.len(), 2);
        assert_eq!(report.evaluated_policies[0].policy_name, "NoOpPolicy");
        assert!(report.evaluated_policies[0].decision.is_allow());
        assert_eq!(
            report.evaluated_policies[1].policy_name,
            "TrustDomainPolicy"
        );
        assert!(report.evaluated_policies[1].decision.is_deny());
        assert!(report.deny_reason.is_some());
    }

    #[test]
    fn test_composed_policy_report_with_require() {
        let mut composed = ComposedPolicy::new();
        composed = composed.add_policy(Box::new(TrustDomainPolicy));
        composed = composed.add_policy(Box::new(PipelineSafetyPolicy::new()));

        let context = PolicyContext::for_pipeline(
            IdentityMetadata::new(IdentityKind::Component, TrustDomain::user(), "pipeline", 0),
            PipelineId::new(),
        );

        let report = composed.evaluate_with_report(PolicyEvent::OnPipelineStart, &context);
        assert!(report.is_require());
        assert_eq!(report.evaluated_policies.len(), 2);
        assert!(!report.required_actions.is_empty());
    }

    // ============================================================================
    // Phase 10: Capability Set and Derived Authority Tests
    // ============================================================================

    #[test]
    fn test_capability_set_creation() {
        let set = CapabilitySet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);

        let set2 = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        assert!(!set2.is_empty());
        assert_eq!(set2.len(), 3);
    }

    #[test]
    fn test_capability_set_subset() {
        let full = CapabilitySet::from_capabilities(vec![1, 2, 3, 4]);
        let subset = CapabilitySet::from_capabilities(vec![1, 2]);
        let not_subset = CapabilitySet::from_capabilities(vec![1, 5]);

        assert!(subset.is_subset_of(&full));
        assert!(!not_subset.is_subset_of(&full));
        assert!(full.is_subset_of(&full)); // Set is subset of itself
    }

    #[test]
    fn test_capability_set_intersection() {
        let set1 = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let set2 = CapabilitySet::from_capabilities(vec![2, 3, 4]);

        let intersection = set1.intersection(&set2);
        let caps = intersection.to_vec();
        assert_eq!(caps, vec![2, 3]);
    }

    #[test]
    fn test_capability_set_difference() {
        let set1 = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let set2 = CapabilitySet::from_capabilities(vec![2, 3, 4]);

        let diff = set1.difference(&set2);
        assert_eq!(diff.to_vec(), vec![1]);
    }

    #[test]
    fn test_derived_authority_creation() {
        let caps = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let derived = DerivedAuthority::new(caps.clone());

        assert_eq!(derived.capabilities, caps);
        assert!(derived.constraints.is_empty());
    }

    #[test]
    fn test_derived_authority_with_constraints() {
        let derived = DerivedAuthority::from_capabilities(vec![1, 2])
            .with_constraint("read-only")
            .with_constraint("no-network");

        assert_eq!(derived.constraints.len(), 2);
        assert_eq!(derived.constraints[0], "read-only");
        assert_eq!(derived.constraints[1], "no-network");
    }

    #[test]
    fn test_capability_delta_empty() {
        let before = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let after = before.clone();

        let delta = CapabilityDelta::from(&before, &after);
        assert!(delta.is_empty());
        assert!(delta.removed.is_empty());
        assert!(delta.added.is_empty());
    }

    #[test]
    fn test_capability_delta_removed() {
        let before = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let after = CapabilitySet::from_capabilities(vec![1, 2]);

        let delta = CapabilityDelta::from(&before, &after);
        assert!(!delta.is_empty());
        assert_eq!(delta.removed, vec![3]);
        assert!(delta.added.is_empty());
    }

    #[test]
    fn test_capability_delta_added() {
        let before = CapabilitySet::from_capabilities(vec![1, 2]);
        let after = CapabilitySet::from_capabilities(vec![1, 2, 3]);

        let delta = CapabilityDelta::from(&before, &after);
        assert!(!delta.is_empty());
        assert!(delta.removed.is_empty());
        assert_eq!(delta.added, vec![3]);
    }

    #[test]
    fn test_capability_delta_both() {
        let before = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let after = CapabilitySet::from_capabilities(vec![2, 3, 4]);

        let delta = CapabilityDelta::from(&before, &after);
        assert!(!delta.is_empty());
        assert_eq!(delta.removed, vec![1]);
        assert_eq!(delta.added, vec![4]);
    }

    #[test]
    fn test_policy_decision_allow_with_derived() {
        let derived = DerivedAuthority::from_capabilities(vec![1, 2]);
        let decision = PolicyDecision::allow_with_derived(derived.clone());

        assert!(decision.is_allow());
        assert_eq!(decision.derived_authority(), Some(&derived));
    }

    #[test]
    fn test_policy_decision_allow_without_derived() {
        let decision = PolicyDecision::allow();

        assert!(decision.is_allow());
        assert_eq!(decision.derived_authority(), None);
    }

    #[test]
    fn test_policy_decision_report_with_capabilities() {
        let input = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let output = CapabilitySet::from_capabilities(vec![1, 2]);

        let report = PolicyDecisionReport::new("TestPolicy", PolicyDecision::allow())
            .with_capabilities(input.clone(), Some(output.clone()));

        assert_eq!(report.input_capabilities, Some(input));
        assert_eq!(report.output_capabilities, Some(output));
        assert!(report.capability_delta.is_some());

        let delta = report.capability_delta.unwrap();
        assert_eq!(delta.removed, vec![3]);
        assert!(delta.added.is_empty());
    }

    #[test]
    fn test_capability_set_serialization() {
        let set = CapabilitySet::from_capabilities(vec![1, 2, 3]);
        let json = serde_json::to_string(&set).unwrap();
        let deserialized: CapabilitySet = serde_json::from_str(&json).unwrap();

        assert_eq!(set, deserialized);
    }

    #[test]
    fn test_derived_authority_serialization() {
        let derived =
            DerivedAuthority::from_capabilities(vec![1, 2, 3]).with_constraint("read-only");

        let json = serde_json::to_string(&derived).unwrap();
        let deserialized: DerivedAuthority = serde_json::from_str(&json).unwrap();

        assert_eq!(derived, deserialized);
    }

    #[test]
    fn test_capability_delta_serialization() {
        let delta = CapabilityDelta {
            removed: vec![1, 2],
            restricted: vec!["read-only".to_string()],
            added: vec![3],
        };

        let json = serde_json::to_string(&delta).unwrap();
        let deserialized: CapabilityDelta = serde_json::from_str(&json).unwrap();

        assert_eq!(delta, deserialized);
    }
}
