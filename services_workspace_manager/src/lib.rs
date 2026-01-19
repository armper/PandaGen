//! # Workspace Manager Service
//!
//! This crate implements the workspace manager for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Workspace manages components, not processes**
//! - **Focus is explicit and capability-driven**
//! - **Lifecycle is observable and auditable**
//! - **Commands launch components; components own interaction**
//! - **No ambient authority, no global state**
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - A POSIX shell
//! - A job control system
//! - A global stdin/stdout router
//! - A terminal multiplexer
//! - A monolithic "god shell"

pub mod commands;

use core_types::TaskId;
use identity::{ExecutionId, ExitReason, IdentityKind, IdentityMetadata, TrustDomain};
use input_types::InputEvent;
use lifecycle::{CancellationReason, CancellationSource, CancellationToken};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent};
use resources::ResourceBudget;
use serde::{Deserialize, Serialize};
use services_focus_manager::{FocusError, FocusManager};
use services_input::InputSubscriptionCap;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for a component in the workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentId(Uuid);

impl ComponentId {
    /// Creates a new unique component ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a ComponentId from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID value
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ComponentId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "comp:{}", self.0)
    }
}

/// Type of component managed by workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentType {
    /// Editor component (vi-like)
    Editor,
    /// CLI/console component
    Cli,
    /// Pipeline executor component
    PipelineExecutor,
    /// Custom component
    Custom,
}

impl std::fmt::Display for ComponentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentType::Editor => write!(f, "Editor"),
            ComponentType::Cli => write!(f, "CLI"),
            ComponentType::PipelineExecutor => write!(f, "PipelineExecutor"),
            ComponentType::Custom => write!(f, "Custom"),
        }
    }
}

/// State of a component
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentState {
    /// Component is running
    Running,
    /// Component exited normally
    Exited,
    /// Component was cancelled
    Cancelled,
    /// Component failed
    Failed,
}

impl std::fmt::Display for ComponentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentState::Running => write!(f, "Running"),
            ComponentState::Exited => write!(f, "Exited"),
            ComponentState::Cancelled => write!(f, "Cancelled"),
            ComponentState::Failed => write!(f, "Failed"),
        }
    }
}

/// Information about a component in the workspace
#[derive(Debug, Clone)]
pub struct ComponentInfo {
    /// Unique component identifier
    pub id: ComponentId,
    /// Type of component
    pub component_type: ComponentType,
    /// Execution identity
    pub identity: IdentityMetadata,
    /// Current state
    pub state: ComponentState,
    /// Whether component can receive focus
    pub focusable: bool,
    /// Input subscription capability (if focusable)
    pub subscription: Option<InputSubscriptionCap>,
    /// Cancellation source for the component
    pub cancellation: CancellationSource,
    /// Exit reason (if not running)
    pub exit_reason: Option<ExitReason>,
    /// Human-readable name
    pub name: String,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl ComponentInfo {
    /// Creates a new component info
    pub fn new(
        component_type: ComponentType,
        identity: IdentityMetadata,
        focusable: bool,
        name: impl Into<String>,
    ) -> Self {
        Self {
            id: ComponentId::new(),
            component_type,
            identity,
            state: ComponentState::Running,
            focusable,
            subscription: None,
            cancellation: CancellationSource::new(),
            exit_reason: None,
            name: name.into(),
            metadata: HashMap::new(),
        }
    }

    /// Adds metadata to the component
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Sets the input subscription capability
    pub fn with_subscription(mut self, subscription: InputSubscriptionCap) -> Self {
        self.subscription = Some(subscription);
        self
    }

    /// Returns the cancellation token for this component
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.token()
    }

    /// Checks if the component is running
    pub fn is_running(&self) -> bool {
        self.state == ComponentState::Running
    }

    /// Checks if the component has focus
    pub fn has_focus(&self, focus_manager: &FocusManager) -> bool {
        if let Some(sub) = &self.subscription {
            focus_manager.has_focus(sub)
        } else {
            false
        }
    }
}

/// Workspace lifecycle event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceEvent {
    /// Component was launched
    ComponentLaunched {
        component_id: ComponentId,
        component_type: ComponentType,
        execution_id: ExecutionId,
        timestamp_ns: u64,
    },
    /// Component state changed
    ComponentStateChanged {
        component_id: ComponentId,
        old_state: ComponentState,
        new_state: ComponentState,
        timestamp_ns: u64,
    },
    /// Component received focus
    ComponentFocused {
        component_id: ComponentId,
        timestamp_ns: u64,
    },
    /// Component lost focus
    ComponentUnfocused {
        component_id: ComponentId,
        timestamp_ns: u64,
    },
    /// Component terminated
    ComponentTerminated {
        component_id: ComponentId,
        reason: ExitReason,
        timestamp_ns: u64,
    },
}

/// Workspace manager errors
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkspaceError {
    #[error("Component not found: {0}")]
    ComponentNotFound(ComponentId),

    #[error("Component launch denied: {reason}")]
    LaunchDenied { reason: String },

    #[error("Focus denied: {reason}")]
    FocusDenied { reason: String },

    #[error("Component not focusable: {0}")]
    NotFocusable(ComponentId),

    #[error("No components available")]
    NoComponents,

    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Policy error: {0}")]
    PolicyError(String),

    #[error("Budget exhausted for component: {0}")]
    BudgetExhausted(ComponentId),

    #[error("Focus error: {0}")]
    FocusError(String),
}

impl From<FocusError> for WorkspaceError {
    fn from(err: FocusError) -> Self {
        WorkspaceError::FocusError(format!("{}", err))
    }
}

/// Configuration for launching a component
pub struct LaunchConfig {
    /// Type of component to launch
    pub component_type: ComponentType,
    /// Name for the component
    pub name: String,
    /// Identity kind
    pub identity_kind: IdentityKind,
    /// Trust domain
    pub trust_domain: TrustDomain,
    /// Whether component should be focusable
    pub focusable: bool,
    /// Optional resource budget
    pub budget: Option<ResourceBudget>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl LaunchConfig {
    /// Creates a new launch configuration
    pub fn new(
        component_type: ComponentType,
        name: impl Into<String>,
        identity_kind: IdentityKind,
        trust_domain: TrustDomain,
    ) -> Self {
        Self {
            component_type,
            name: name.into(),
            identity_kind,
            trust_domain,
            focusable: true,
            budget: None,
            metadata: HashMap::new(),
        }
    }

    /// Sets whether the component is focusable
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Sets the resource budget
    pub fn with_budget(mut self, budget: ResourceBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Adds metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Workspace Manager
///
/// Manages component lifecycle, focus, and orchestration.
/// This is NOT a shell - it's a component orchestrator.
pub struct WorkspaceManager {
    /// Component registry
    components: HashMap<ComponentId, ComponentInfo>,
    /// Focus manager
    focus_manager: FocusManager,
    /// Policy engine (optional)
    policy: Option<Box<dyn PolicyEngine>>,
    /// Audit trail of workspace events
    audit_trail: Vec<WorkspaceEvent>,
    /// Next timestamp for events
    next_timestamp: u64,
    /// Workspace identity (for policy evaluation)
    workspace_identity: IdentityMetadata,
}

impl WorkspaceManager {
    /// Creates a new workspace manager
    pub fn new(workspace_identity: IdentityMetadata) -> Self {
        Self {
            components: HashMap::new(),
            focus_manager: FocusManager::new(),
            policy: None,
            audit_trail: Vec::new(),
            next_timestamp: 0,
            workspace_identity,
        }
    }

    /// Sets the policy engine
    pub fn with_policy(mut self, policy: Box<dyn PolicyEngine>) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Launches a new component
    ///
    /// Creates a component with the specified configuration and optionally
    /// grants it focus if focusable.
    pub fn launch_component(
        &mut self,
        config: LaunchConfig,
    ) -> Result<ComponentId, WorkspaceError> {
        let timestamp = self.next_timestamp();

        // Create identity for the component
        let mut identity = IdentityMetadata::new(
            config.identity_kind,
            config.trust_domain.clone(),
            config.name.clone(),
            timestamp,
        )
        .with_parent(self.workspace_identity.execution_id);

        if let Some(budget) = config.budget {
            identity = identity.with_budget(budget);
        }

        // Check policy if configured
        if let Some(policy) = &self.policy {
            let context =
                PolicyContext::for_spawn(self.workspace_identity.clone(), identity.clone());
            let decision = policy.evaluate(PolicyEvent::OnSpawn, &context);

            if let PolicyDecision::Deny { reason } = decision {
                return Err(WorkspaceError::LaunchDenied { reason });
            }
        }

        // Create component info
        let mut component = ComponentInfo::new(
            config.component_type,
            identity.clone(),
            config.focusable,
            config.name.clone(),
        );

        for (k, v) in config.metadata {
            component = component.with_metadata(k, v);
        }

        // If focusable, create input subscription
        if config.focusable {
            let subscription = InputSubscriptionCap::new(
                self.next_timestamp(),
                TaskId::new(),
                ipc::ChannelId::new(),
            );
            component = component.with_subscription(subscription);
        }

        let component_id = component.id;

        // Record event
        self.audit_trail.push(WorkspaceEvent::ComponentLaunched {
            component_id,
            component_type: config.component_type,
            execution_id: identity.execution_id,
            timestamp_ns: timestamp,
        });

        // Store component
        self.components.insert(component_id, component);

        // Grant focus if focusable and no other component has focus
        if config.focusable {
            let _ = self.focus_component(component_id);
        }

        Ok(component_id)
    }

    /// Grants focus to a component
    pub fn focus_component(&mut self, component_id: ComponentId) -> Result<(), WorkspaceError> {
        let component = self
            .components
            .get(&component_id)
            .ok_or(WorkspaceError::ComponentNotFound(component_id))?;

        if !component.focusable {
            return Err(WorkspaceError::NotFocusable(component_id));
        }

        let subscription = component
            .subscription
            .as_ref()
            .ok_or(WorkspaceError::NotFocusable(component_id))?;

        // Check policy if configured
        if let Some(policy) = &self.policy {
            let context = PolicyContext::for_capability_delegation(
                self.workspace_identity.clone(),
                component.identity.clone(),
                subscription.id,
            );
            let decision = policy.evaluate(PolicyEvent::OnCapabilityDelegate, &context);

            if let PolicyDecision::Deny { reason } = decision {
                return Err(WorkspaceError::FocusDenied { reason });
            }
        }

        // Request focus
        self.focus_manager.request_focus(*subscription)?;

        // Record event
        let timestamp = self.next_timestamp();
        self.audit_trail.push(WorkspaceEvent::ComponentFocused {
            component_id,
            timestamp_ns: timestamp,
        });

        Ok(())
    }

    /// Switches focus to the next component
    pub fn focus_next(&mut self) -> Result<(), WorkspaceError> {
        let running_focusable: Vec<ComponentId> = self
            .components
            .values()
            .filter(|c| c.is_running() && c.focusable)
            .map(|c| c.id)
            .collect();

        if running_focusable.is_empty() {
            return Err(WorkspaceError::NoComponents);
        }

        // Find currently focused component
        let current_focus = self.get_focused_component();

        let next_id = if let Some(current_id) = current_focus {
            // Find next component after current
            let current_pos = running_focusable
                .iter()
                .position(|&id| id == current_id)
                .unwrap_or(0);
            running_focusable[(current_pos + 1) % running_focusable.len()]
        } else {
            // No focus, take first
            running_focusable[0]
        };

        self.focus_component(next_id)
    }

    /// Switches focus to the previous component
    pub fn focus_previous(&mut self) -> Result<(), WorkspaceError> {
        let running_focusable: Vec<ComponentId> = self
            .components
            .values()
            .filter(|c| c.is_running() && c.focusable)
            .map(|c| c.id)
            .collect();

        if running_focusable.is_empty() {
            return Err(WorkspaceError::NoComponents);
        }

        // Find currently focused component
        let current_focus = self.get_focused_component();

        let prev_id = if let Some(current_id) = current_focus {
            // Find previous component before current
            let current_pos = running_focusable
                .iter()
                .position(|&id| id == current_id)
                .unwrap_or(0);
            let prev_pos = if current_pos == 0 {
                running_focusable.len() - 1
            } else {
                current_pos - 1
            };
            running_focusable[prev_pos]
        } else {
            // No focus, take last
            running_focusable[running_focusable.len() - 1]
        };

        self.focus_component(prev_id)
    }

    /// Terminates a component
    pub fn terminate_component(
        &mut self,
        component_id: ComponentId,
        reason: ExitReason,
    ) -> Result<(), WorkspaceError> {
        let component = self
            .components
            .get_mut(&component_id)
            .ok_or(WorkspaceError::ComponentNotFound(component_id))?;

        // Update state based on reason
        component.state = match reason {
            ExitReason::Normal => ComponentState::Exited,
            ExitReason::Cancelled { .. } => ComponentState::Cancelled,
            ExitReason::Failure { .. } | ExitReason::Timeout => ComponentState::Failed,
        };
        component.exit_reason = Some(reason.clone());

        // Cancel the component
        let cancel_reason = match &reason {
            ExitReason::Normal => CancellationReason::UserCancel,
            ExitReason::Cancelled { .. } => CancellationReason::SupervisorCancel,
            ExitReason::Timeout => CancellationReason::Timeout,
            ExitReason::Failure { .. } => CancellationReason::DependencyFailed,
        };
        component.cancellation.cancel(cancel_reason);

        // Remove from focus if it has focus
        if let Some(subscription) = &component.subscription {
            let _ = self.focus_manager.remove_subscription(subscription);
        }

        // Record event
        let timestamp = self.next_timestamp();
        self.audit_trail.push(WorkspaceEvent::ComponentTerminated {
            component_id,
            reason,
            timestamp_ns: timestamp,
        });

        Ok(())
    }

    /// Lists all components
    pub fn list_components(&self) -> Vec<&ComponentInfo> {
        self.components.values().collect()
    }

    /// Gets a component by ID
    pub fn get_component(&self, component_id: ComponentId) -> Option<&ComponentInfo> {
        self.components.get(&component_id)
    }

    /// Returns the currently focused component ID
    pub fn get_focused_component(&self) -> Option<ComponentId> {
        let focus_sub = self.focus_manager.current_focus()?;

        // Find component with matching subscription
        self.components
            .values()
            .find(|c| {
                c.subscription
                    .as_ref()
                    .map(|s| s.id == focus_sub.id)
                    .unwrap_or(false)
            })
            .map(|c| c.id)
    }

    /// Routes an input event to the focused component
    pub fn route_input(&self, event: &InputEvent) -> Option<ComponentId> {
        let focused_sub = self.focus_manager.route_event(event).ok()??;

        // Find component with matching subscription
        self.components
            .values()
            .find(|c| {
                c.subscription
                    .as_ref()
                    .map(|s| s.id == focused_sub.id)
                    .unwrap_or(false)
            })
            .map(|c| c.id)
    }

    /// Returns the audit trail
    pub fn audit_trail(&self) -> &[WorkspaceEvent] {
        &self.audit_trail
    }

    /// Handles budget exhaustion for a component
    pub fn handle_budget_exhaustion(
        &mut self,
        component_id: ComponentId,
    ) -> Result<(), WorkspaceError> {
        self.terminate_component(
            component_id,
            ExitReason::Failure {
                error: "Resource budget exhausted".to_string(),
            },
        )?;

        Ok(())
    }

    /// Gets next timestamp and increments counter
    fn next_timestamp(&mut self) -> u64 {
        let ts = self.next_timestamp;
        self.next_timestamp += 1;
        ts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_workspace() -> WorkspaceManager {
        let workspace_identity = IdentityMetadata::new(
            IdentityKind::Service,
            TrustDomain::core(),
            "test-workspace",
            0,
        );
        WorkspaceManager::new(workspace_identity)
    }

    #[test]
    fn test_workspace_creation() {
        let workspace = create_test_workspace();
        assert_eq!(workspace.components.len(), 0);
        assert_eq!(workspace.audit_trail.len(), 0);
    }

    #[test]
    fn test_launch_component() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "test-editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let component_id = workspace.launch_component(config).unwrap();

        assert_eq!(workspace.components.len(), 1);
        assert!(workspace.get_component(component_id).is_some());

        let component = workspace.get_component(component_id).unwrap();
        assert_eq!(component.component_type, ComponentType::Editor);
        assert_eq!(component.state, ComponentState::Running);
        assert!(component.focusable);
    }

    #[test]
    fn test_launch_multiple_components() {
        let mut workspace = create_test_workspace();

        let editor_config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );
        let cli_config = LaunchConfig::new(
            ComponentType::Cli,
            "cli",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        workspace.launch_component(editor_config).unwrap();
        workspace.launch_component(cli_config).unwrap();

        assert_eq!(workspace.components.len(), 2);
        assert_eq!(workspace.list_components().len(), 2);
    }

    #[test]
    fn test_focus_component() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let component_id = workspace.launch_component(config).unwrap();

        // Component should have focus automatically
        let focused = workspace.get_focused_component();
        assert_eq!(focused, Some(component_id));
    }

    #[test]
    fn test_focus_switching() {
        let mut workspace = create_test_workspace();

        let config1 = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );
        let config2 = LaunchConfig::new(
            ComponentType::Cli,
            "cli",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let id1 = workspace.launch_component(config1).unwrap();
        let id2 = workspace.launch_component(config2).unwrap();

        // Second component should have focus
        assert_eq!(workspace.get_focused_component(), Some(id2));

        // Switch to next (wraps to first)
        workspace.focus_next().unwrap();
        assert_eq!(workspace.get_focused_component(), Some(id1));

        // Switch to previous
        workspace.focus_previous().unwrap();
        assert_eq!(workspace.get_focused_component(), Some(id2));
    }

    #[test]
    fn test_terminate_component() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let component_id = workspace.launch_component(config).unwrap();

        workspace
            .terminate_component(component_id, ExitReason::Normal)
            .unwrap();

        let component = workspace.get_component(component_id).unwrap();
        assert_eq!(component.state, ComponentState::Exited);
        assert!(component.exit_reason.is_some());

        // Should no longer have focus
        assert_eq!(workspace.get_focused_component(), None);
    }

    #[test]
    fn test_terminate_removes_focus() {
        let mut workspace = create_test_workspace();

        let config1 = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );
        let config2 = LaunchConfig::new(
            ComponentType::Cli,
            "cli",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let id1 = workspace.launch_component(config1).unwrap();
        let id2 = workspace.launch_component(config2).unwrap();

        // Focus first component
        workspace.focus_component(id1).unwrap();
        assert_eq!(workspace.get_focused_component(), Some(id1));

        // Terminate focused component
        workspace
            .terminate_component(id1, ExitReason::Normal)
            .unwrap();

        // Focus should be removed, but id2 is still available
        let focused = workspace.get_focused_component();
        assert!(focused.is_none() || focused == Some(id2));
    }

    #[test]
    fn test_not_focusable_component() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Custom,
            "background-task",
            IdentityKind::Component,
            TrustDomain::user(),
        )
        .with_focusable(false);

        let component_id = workspace.launch_component(config).unwrap();

        // Try to focus non-focusable component
        let result = workspace.focus_component(component_id);
        assert_eq!(result, Err(WorkspaceError::NotFocusable(component_id)));
    }

    #[test]
    fn test_component_with_budget() {
        let mut workspace = create_test_workspace();

        let budget = ResourceBudget::unlimited();
        let config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        )
        .with_budget(budget);

        let component_id = workspace.launch_component(config).unwrap();

        let component = workspace.get_component(component_id).unwrap();
        assert!(component.identity.has_budget());
    }

    #[test]
    fn test_budget_exhaustion() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let component_id = workspace.launch_component(config).unwrap();

        workspace.handle_budget_exhaustion(component_id).unwrap();

        let component = workspace.get_component(component_id).unwrap();
        assert_eq!(component.state, ComponentState::Failed);
    }

    #[test]
    fn test_audit_trail() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let component_id = workspace.launch_component(config).unwrap();

        // Should have launch event
        let trail = workspace.audit_trail();
        assert!(!trail.is_empty());

        match &trail[0] {
            WorkspaceEvent::ComponentLaunched {
                component_id: id, ..
            } => {
                assert_eq!(*id, component_id);
            }
            _ => panic!("Expected ComponentLaunched event"),
        }
    }

    #[test]
    fn test_component_metadata() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        )
        .with_metadata("file", "test.txt")
        .with_metadata("mode", "edit");

        let component_id = workspace.launch_component(config).unwrap();

        let component = workspace.get_component(component_id).unwrap();
        assert_eq!(
            component.metadata.get("file"),
            Some(&"test.txt".to_string())
        );
        assert_eq!(component.metadata.get("mode"), Some(&"edit".to_string()));
    }
}
