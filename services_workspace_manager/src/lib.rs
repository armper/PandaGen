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

pub mod boot_profile;
pub mod command_registry;
pub mod commands;
pub mod help;
pub mod keybindings;
pub mod workspace_status;


use core_types::TaskId;
use identity::{ExecutionId, ExitReason, IdentityKind, IdentityMetadata, TrustDomain};
use input_types::InputEvent;
use keybindings::KeyBindingManager;
use lifecycle::{CancellationReason, CancellationSource, CancellationToken};
use packages::{ComponentLoader, PackageComponentType, PackageManifest};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent};
use resources::ResourceBudget;
use serde::{Deserialize, Serialize};
use fs_view::DirectoryView;
use services_editor_vi::{Editor, OpenOptions, StorageEditorIo};
use services_fs_view::FileSystemViewService;
use services_storage::JournaledStorage;
use services_focus_manager::{FocusError, FocusManager};
use services_input::InputSubscriptionCap;
use services_view_host::{ViewHandleCap, ViewHost, ViewSubscriptionCap};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;
use view_types::{ViewFrame, ViewId, ViewKind};
use workspace_status::{ContextBreadcrumbs, RecentHistory, WorkspaceStatus};

// Re-export public types from modules
pub use help::HelpCategory;
pub use workspace_status::{
    ActionableError, CommandSuggestion, FsStatus, PromptValidation, generate_suggestions, validate_command,
};

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

/// Editor I/O context for capability-scoped storage access
#[derive(Debug, Clone)]
pub struct EditorIoContext {
    pub storage: JournaledStorage,
    pub fs_view: Option<FileSystemViewService>,
    pub root: Option<DirectoryView>,
}

impl EditorIoContext {
    pub fn new(storage: JournaledStorage) -> Self {
        Self {
            storage,
            fs_view: None,
            root: None,
        }
    }

    pub fn with_fs_view(
        storage: JournaledStorage,
        fs_view: FileSystemViewService,
        root: DirectoryView,
    ) -> Self {
        Self {
            storage,
            fs_view: Some(fs_view),
            root: Some(root),
        }
    }
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
    /// Main view handle (TextBuffer)
    pub main_view: Option<ViewHandleCap>,
    /// Status view handle (StatusLine)
    pub status_view: Option<ViewHandleCap>,
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
            main_view: None,
            status_view: None,
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

    /// Sets the main view handle
    pub fn with_main_view(mut self, handle: ViewHandleCap) -> Self {
        self.main_view = Some(handle);
        self
    }

    /// Sets the status view handle
    pub fn with_status_view(mut self, handle: ViewHandleCap) -> Self {
        self.status_view = Some(handle);
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

impl WorkspaceError {
    /// Returns actionable suggestions for recovering from this error
    /// Returns (error_message, suggested_actions)
    pub fn actionable_message(&self) -> (String, Vec<String>) {
        match self {
            WorkspaceError::ComponentNotFound(id) => (
                format!("Component not found: {}", id),
                vec!["list".to_string(), "help workspace".to_string()],
            ),
            WorkspaceError::LaunchDenied { reason } => (
                format!("Component launch denied: {}", reason),
                vec!["help workspace".to_string()],
            ),
            WorkspaceError::FocusDenied { reason } => (
                format!("Focus denied: {}", reason),
                vec!["list".to_string()],
            ),
            WorkspaceError::NotFocusable(id) => (
                format!("Component not focusable: {}", id),
                vec!["list".to_string(), "next".to_string()],
            ),
            WorkspaceError::NoComponents => (
                "No components available".to_string(),
                vec!["open editor <path>".to_string(), "help".to_string()],
            ),
            WorkspaceError::InvalidCommand(cmd) => (
                format!("Invalid command: {}", cmd),
                vec!["help".to_string()],
            ),
            WorkspaceError::PolicyError(msg) => (
                format!("Policy error: {}", msg),
                vec!["help system".to_string()],
            ),
            WorkspaceError::BudgetExhausted(id) => (
                format!("Budget exhausted for component: {}", id),
                vec!["close {}".to_string(), "list".to_string()],
            ),
            WorkspaceError::FocusError(msg) => (
                format!("Focus error: {}", msg),
                vec!["list".to_string(), "next".to_string()],
            ),
        }
    }

    /// Formats error with actions for display
    /// Example: "Component not found — Try: list | help workspace"
    pub fn format_with_actions(&self) -> String {
        let (message, actions) = self.actionable_message();
        if actions.is_empty() {
            message
        } else {
            format!("{} — Try: {}", message, actions.join(" | "))
        }
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

/// Component instance holder
///
/// Stores the actual running component instance
enum ComponentInstance {
    /// Editor component
    Editor(Editor),
    /// No instance (placeholder for components not yet implemented)
    None,
}

/// Debug info for keyboard routing (gated behind debug_assertions)
#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
pub struct KeyRoutingDebug {
    /// Last key event received
    pub last_key_event: Option<input_types::KeyEvent>,
    /// Last routed component ID
    pub last_routed_to: Option<ComponentId>,
    /// Whether last event was consumed by global keybinding
    pub consumed_by_global: bool,
}

#[cfg(debug_assertions)]
impl KeyRoutingDebug {
    fn new() -> Self {
        Self {
            last_key_event: None,
            last_routed_to: None,
            consumed_by_global: false,
        }
    }
}

/// Workspace Manager
///
/// Manages component lifecycle, focus, and orchestration.
/// This is NOT a shell - it's a component orchestrator.
pub struct WorkspaceManager {
    /// Component registry
    components: HashMap<ComponentId, ComponentInfo>,
    /// Component instances (actual running components)
    component_instances: HashMap<ComponentId, ComponentInstance>,
    /// Focus manager
    focus_manager: FocusManager,
    /// View host for managing component views
    view_host: ViewHost,
    /// View subscriptions for workspace (to receive updates)
    view_subscriptions: HashMap<ViewId, ViewSubscriptionCap>,
    /// Key binding manager
    key_binding_manager: KeyBindingManager,
    /// Policy engine (optional)
    policy: Option<Box<dyn PolicyEngine>>,
    /// Audit trail of workspace events
    audit_trail: Vec<WorkspaceEvent>,
    /// Next timestamp for events
    next_timestamp: u64,
    /// Workspace identity (for policy evaluation)
    workspace_identity: IdentityMetadata,
    /// Optional editor I/O context for capability-scoped storage
    editor_io_context: Option<EditorIoContext>,
    /// Debug info for keyboard routing (gated behind debug_assertions)
    #[cfg(debug_assertions)]
    key_routing_debug: KeyRoutingDebug,
    /// Workspace status for status strip
    workspace_status: WorkspaceStatus,
    /// Recent history (files, commands, errors)
    recent_history: RecentHistory,
    /// Context breadcrumbs
    breadcrumbs: ContextBreadcrumbs,
}

impl WorkspaceManager {
    /// Creates a new workspace manager
    pub fn new(workspace_identity: IdentityMetadata) -> Self {
        Self {
            components: HashMap::new(),
            component_instances: HashMap::new(),
            focus_manager: FocusManager::new(),
            view_host: ViewHost::new(),
            view_subscriptions: HashMap::new(),
            key_binding_manager: KeyBindingManager::new(),
            policy: None,
            audit_trail: Vec::new(),
            next_timestamp: 0,
            workspace_identity,
            editor_io_context: None,
            #[cfg(debug_assertions)]
            key_routing_debug: KeyRoutingDebug::new(),
            workspace_status: WorkspaceStatus::new(),
            recent_history: RecentHistory::new(),
            breadcrumbs: ContextBreadcrumbs::new(),
        }
    }

    /// Sets the editor I/O context used to configure new editor components.
    pub fn set_editor_io_context(&mut self, context: EditorIoContext) {
        self.editor_io_context = Some(context);
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

        // Create views for the component
        // Component task ID for view ownership
        let component_task_id = TaskId::new();
        // Workspace task ID for subscriptions (could be same but kept separate for clarity)
        let workspace_task_id = TaskId::new();

        // Main view (TextBuffer)
        let main_view = self
            .view_host
            .create_view(
                ViewKind::TextBuffer,
                Some(config.name.clone()),
                component_task_id,
                ipc::ChannelId::new(),
            )
            .map_err(|e| {
                WorkspaceError::InvalidCommand(format!("Failed to create main view: {}", e))
            })?;
        component = component.with_main_view(main_view);

        // Subscribe workspace to main view
        let main_sub = self
            .view_host
            .subscribe(main_view.view_id, workspace_task_id, ipc::ChannelId::new())
            .map_err(|e| {
                WorkspaceError::InvalidCommand(format!("Failed to subscribe to main view: {}", e))
            })?;
        self.view_subscriptions.insert(main_view.view_id, main_sub);

        // Status view (StatusLine)
        let status_view = self
            .view_host
            .create_view(
                ViewKind::StatusLine,
                Some(format!("{} - status", config.name)),
                component_task_id,
                ipc::ChannelId::new(),
            )
            .map_err(|e| {
                WorkspaceError::InvalidCommand(format!("Failed to create status view: {}", e))
            })?;
        component = component.with_status_view(status_view);

        // Subscribe workspace to status view
        let status_sub = self
            .view_host
            .subscribe(
                status_view.view_id,
                workspace_task_id,
                ipc::ChannelId::new(),
            )
            .map_err(|e| {
                WorkspaceError::InvalidCommand(format!("Failed to subscribe to status view: {}", e))
            })?;
        self.view_subscriptions
            .insert(status_view.view_id, status_sub);

        let component_id = component.id;

        // Create component instance
        let instance = match config.component_type {
            ComponentType::Editor => {
                let mut editor = Editor::new();
                // Wire view handles
                if let (Some(main_view), Some(status_view)) =
                    (&component.main_view, &component.status_view)
                {
                    editor.set_view_handles(main_view.clone(), status_view.clone());
                }
                // Configure editor I/O context if available
                if let Some(context) = &self.editor_io_context {
                    let io = match (&context.fs_view, &context.root) {
                        (Some(fs_view), Some(root)) => StorageEditorIo::with_fs_view(
                            context.storage.clone(),
                            fs_view.clone(),
                            root.clone(),
                        ),
                        _ => StorageEditorIo::new(context.storage.clone()),
                    };
                    editor.set_io(Box::new(io));

                    // Open requested path if present
                    let path = component
                        .metadata
                        .get("path")
                        .or_else(|| component.metadata.get("arg0"));
                    if let Some(path) = path {
                        if !path.is_empty() {
                            let _ = editor.open_with(OpenOptions::new().with_path(path.clone()));
                        }
                    }
                }
                ComponentInstance::Editor(editor)
            }
            _ => ComponentInstance::None,
        };

        // Record event
        self.audit_trail.push(WorkspaceEvent::ComponentLaunched {
            component_id,
            component_type: config.component_type,
            execution_id: identity.execution_id,
            timestamp_ns: timestamp,
        });

        // Store component info
        self.components.insert(component_id, component);

        // Store component instance
        self.component_instances.insert(component_id, instance);

        // Grant focus if focusable and no other component has focus
        if config.focusable {
            let _ = self.focus_component(component_id);
        }

        Ok(component_id)
    }

    /// Launches all components described in a package manifest.
    ///
    /// Returns the list of created component IDs in manifest order.
    pub fn launch_package(
        &mut self,
        manifest: &PackageManifest,
    ) -> Result<Vec<ComponentId>, WorkspaceError> {
        let launch_plan = ComponentLoader::build_launch_plan(manifest).map_err(|err| {
            WorkspaceError::InvalidCommand(format!("Package manifest error: {}", err))
        })?;

        let mut created = Vec::with_capacity(launch_plan.len());
        for spec in launch_plan {
            let component_type = match spec.component_type {
                PackageComponentType::Editor => ComponentType::Editor,
                PackageComponentType::Cli => ComponentType::Cli,
                PackageComponentType::PipelineExecutor => ComponentType::PipelineExecutor,
                PackageComponentType::Custom => ComponentType::Custom,
            };

            let mut config = LaunchConfig::new(
                component_type,
                spec.name,
                IdentityKind::Component,
                TrustDomain::user(),
            )
            .with_focusable(spec.focusable)
            .with_metadata("package.name", manifest.name.clone())
            .with_metadata("package.version", manifest.version.clone())
            .with_metadata("package.entry", spec.entry);

            if let Some(budget) = spec.budget {
                config = config.with_budget(budget);
            }

            for (key, value) in spec.metadata {
                config = config.with_metadata(key, value);
            }

            created.push(self.launch_component(config)?);
        }

        Ok(created)
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

        // Clean up views
        if let Some(main_view) = &component.main_view {
            let _ = self.view_host.remove_view(main_view);
            self.view_subscriptions.remove(&main_view.view_id);
        }
        if let Some(status_view) = &component.status_view {
            let _ = self.view_host.remove_view(status_view);
            self.view_subscriptions.remove(&status_view.view_id);
        }

        // Clean up component instance
        self.component_instances.remove(&component_id);

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

    /// Routes an input event to the focused component and processes it
    pub fn route_input(&mut self, event: &InputEvent) -> Option<ComponentId> {
        let key_event = match event.as_key() {
            Some(key_event) => key_event,
            None => return None,
        };

        // Track debug info (only in debug builds)
        #[cfg(debug_assertions)]
        {
            self.key_routing_debug.last_key_event = Some(key_event.clone());
            self.key_routing_debug.consumed_by_global = false;
        }

        // Check global keybindings first
        let global_consumed = if let Some(_action) = self.key_binding_manager.get_action(key_event)
        {
            // TODO: Execute action (switch tile, etc)
            // For now, we just consume it to respect the "consumed" contract
            true
        } else {
            false
        };

        #[cfg(debug_assertions)]
        {
            self.key_routing_debug.consumed_by_global = global_consumed;
        }

        // --- Logging ---
        if key_event.state == input_types::KeyState::Pressed {
            let focus_comp_id = self.get_focused_component();
            let focus_comp = focus_comp_id.and_then(|id| self.components.get(&id));

            let focus_comp_str = if let Some(c) = focus_comp {
                format!("{{id={},type={},name={}}}", c.id, c.component_type, c.name)
            } else {
                "None".to_string()
            };

            let delivered_to = if global_consumed {
                "None".to_string()
            } else if let Ok(Some(focused_sub)) = self.focus_manager.route_event(event) {
                self.components
                    .values()
                    .find(|c| {
                        c.subscription
                            .as_ref()
                            .map(|s| s.id == focused_sub.id)
                            .unwrap_or(false)
                    })
                    .map(|c| format!("{{id={},type={},name={}}}", c.id, c.component_type, c.name))
                    .unwrap_or("None".to_string())
            } else {
                "None".to_string()
            };

            let consumed_by = if global_consumed {
                "global"
            } else if delivered_to != "None" {
                "component"
            } else {
                "none"
            };

            println!("route_input:\n  key={{code={:?}, mods={:?}, state={:?}}}\n  focus_tile={{TODO}}\n  focus_component={}\n  global_consumed={}\n  delivered_to={}\n  consumed_by={} ",
                key_event.code,
                key_event.modifiers,
                key_event.state,
                focus_comp_str,
                global_consumed,
                delivered_to,
                consumed_by,
            );
        }
        // --- End Logging ---

        if global_consumed {
            return None;
        }

        let focused_sub = self.focus_manager.route_event(event).ok()??;

        // Find component with matching subscription
        let component_id = self
            .components
            .values()
            .find(|c| {
                c.subscription
                    .as_ref()
                    .map(|s| s.id == focused_sub.id)
                    .unwrap_or(false)
            })
            .map(|c| c.id)?;

        // Update debug info with routed component
        #[cfg(debug_assertions)]
        {
            self.key_routing_debug.last_routed_to = Some(component_id);
        }

        // Get timestamp before borrowing instances mutably
        let timestamp = self.next_timestamp();

        // Process input in the component instance
        if let Some(instance) = self.component_instances.get_mut(&component_id) {
            match instance {
                ComponentInstance::Editor(editor) => {
                    // Process input
                    match editor.process_input(event.clone()) {
                        Ok(_action) => {
                            // Publish updated views
                            let _ = editor.publish_views(&mut self.view_host, timestamp);
                        }
                        Err(_e) => {
                            // Error processing input - could log here
                        }
                    }
                }
                ComponentInstance::None => {
                    // No instance to process
                }
            }
        }

        Some(component_id)
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

    // ========== Workspace Status and History Methods ==========

    /// Gets the current workspace status
    pub fn workspace_status(&self) -> &WorkspaceStatus {
        &self.workspace_status
    }

    /// Gets the mutable workspace status
    pub fn workspace_status_mut(&mut self) -> &mut WorkspaceStatus {
        &mut self.workspace_status
    }

    /// Gets the recent history
    pub fn recent_history(&self) -> &RecentHistory {
        &self.recent_history
    }

    /// Gets the mutable recent history
    pub fn recent_history_mut(&mut self) -> &mut RecentHistory {
        &mut self.recent_history
    }

    /// Gets the context breadcrumbs
    pub fn breadcrumbs(&self) -> &ContextBreadcrumbs {
        &self.breadcrumbs
    }

    /// Gets the mutable context breadcrumbs
    pub fn breadcrumbs_mut(&mut self) -> &mut ContextBreadcrumbs {
        &mut self.breadcrumbs
    }

    /// Updates workspace status based on current state (deterministic)
    pub fn update_workspace_status(&mut self) {
        // Get active editor name from focused component
        let focused_id = self.get_focused_component();
        
        let (active_editor, has_unsaved) = if let Some(id) = focused_id {
            if let Some(component) = self.get_component(id) {
                if component.component_type == ComponentType::Editor {
                    // Extract filename from component metadata or name
                    let filename = component
                        .metadata
                        .get("filename")
                        .or_else(|| component.metadata.get("arg0"))
                        .cloned()
                        .unwrap_or_else(|| component.name.clone());
                    
                    // Check for dirty state from metadata
                    let dirty = component
                        .metadata
                        .get("dirty")
                        .map(|v| v == "true")
                        .unwrap_or(false);
                    
                    (Some(filename), dirty)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        self.workspace_status.active_editor = active_editor;
        self.workspace_status.has_unsaved_changes = has_unsaved;

        // Update job count (just count running components for now)
        self.workspace_status.active_jobs = self
            .components
            .values()
            .filter(|c| c.is_running())
            .count();

        // Update breadcrumbs based on focused component
        self.update_breadcrumbs();
    }

    /// Updates context breadcrumbs based on focused component
    fn update_breadcrumbs(&mut self) {
        let focused_id = self.get_focused_component();
        
        if let Some(id) = focused_id {
            if let Some(component) = self.get_component(id) {
                let mut parts = vec!["PANDA".to_string(), "ROOT".to_string()];
                
                match component.component_type {
                    ComponentType::Editor => {
                        let filename = component
                            .metadata
                            .get("filename")
                            .or_else(|| component.metadata.get("arg0"))
                            .cloned()
                            .unwrap_or_else(|| "untitled".to_string());
                        parts.push(format!("EDITOR({})", filename));
                    }
                    ComponentType::Cli => {
                        parts.push("CLI".to_string());
                    }
                    ComponentType::PipelineExecutor => {
                        parts.push("PIPELINE".to_string());
                    }
                    ComponentType::Custom => {
                        parts.push("CUSTOM".to_string());
                    }
                }
                
                self.breadcrumbs.set_parts(parts);
            }
        } else {
            // No focused component, reset to root
            self.breadcrumbs.set_parts(vec!["PANDA".to_string(), "ROOT".to_string()]);
        }
    }

    /// Renders the current workspace state
    ///
    /// Returns a snapshot of the focused component's views and status.
    pub fn render_snapshot(&self) -> WorkspaceRenderSnapshot {
        let focused_component_id = self.get_focused_component();

        let focused_component = focused_component_id.and_then(|id| self.get_component(id));

        let main_view_frame = focused_component
            .and_then(|c| c.main_view.as_ref())
            .and_then(|handle| self.view_host.get_latest(handle.view_id).ok())
            .flatten();

        let status_view_frame = focused_component
            .and_then(|c| c.status_view.as_ref())
            .and_then(|handle| self.view_host.get_latest(handle.view_id).ok())
            .flatten();

        WorkspaceRenderSnapshot {
            focused_component: focused_component_id,
            main_view: main_view_frame,
            status_view: status_view_frame,
            component_count: self.components.len(),
            running_count: self.components.values().filter(|c| c.is_running()).count(),
            status_strip: self.workspace_status.format_status_strip_with_action(),
            breadcrumbs: self.breadcrumbs.format(),
            #[cfg(debug_assertions)]
            debug_info: Some(DebugInfo {
                focused_component_name: focused_component.map(|c| c.name.clone()),
                focused_component_type: focused_component.map(|c| c.component_type),
                last_key_event: self.key_routing_debug.last_key_event.as_ref().map(|ke| {
                    // Use Debug format for robustness
                    format!("{:?}", ke)
                }),
                last_routed_to: self.key_routing_debug.last_routed_to,
                consumed_by_global: self.key_routing_debug.consumed_by_global,
            }),
        }
    }

    /// Captures a persistent snapshot of the workspace session.
    pub fn save_session(&self) -> WorkspaceSessionSnapshot {
        let focused_component = self.get_focused_component();

        let mut components: Vec<WorkspaceComponentSnapshot> = self
            .components
            .values()
            .map(|component| {
                let main_view = component
                    .main_view
                    .as_ref()
                    .and_then(|handle| self.view_host.get_latest(handle.view_id).ok())
                    .flatten();
                let status_view = component
                    .status_view
                    .as_ref()
                    .and_then(|handle| self.view_host.get_latest(handle.view_id).ok())
                    .flatten();

                WorkspaceComponentSnapshot {
                    component_id: component.id,
                    component_type: component.component_type,
                    name: component.name.clone(),
                    focusable: component.focusable,
                    identity_kind: component.identity.kind,
                    trust_domain: component.identity.trust_domain.clone(),
                    budget: component.identity.budget,
                    metadata: component.metadata.clone(),
                    state: component.state,
                    exit_reason: component.exit_reason.clone(),
                    main_view,
                    status_view,
                }
            })
            .collect();

        components.sort_by_key(|snapshot| snapshot.component_id.as_uuid());

        WorkspaceSessionSnapshot {
            format_version: WorkspaceSessionFormat::new(1, 0),
            next_timestamp: self.next_timestamp,
            focused_component,
            components,
        }
    }

    /// Restores workspace state from a snapshot.
    pub fn restore_session(
        &mut self,
        snapshot: WorkspaceSessionSnapshot,
    ) -> Result<(), WorkspaceError> {
        self.components.clear();
        self.focus_manager = FocusManager::new();
        self.view_host = ViewHost::new();
        self.view_subscriptions.clear();
        self.audit_trail.clear();
        self.next_timestamp = snapshot.next_timestamp;

        for component_snapshot in snapshot.components {
            let WorkspaceComponentSnapshot {
                component_id,
                component_type,
                name,
                focusable,
                identity_kind,
                trust_domain,
                budget,
                metadata,
                state,
                exit_reason,
                main_view: main_frame,
                status_view: status_frame,
            } = component_snapshot;

            let timestamp = self.next_timestamp();
            let mut identity =
                IdentityMetadata::new(identity_kind, trust_domain.clone(), name.clone(), timestamp)
                    .with_parent(self.workspace_identity.execution_id);

            if let Some(budget) = budget {
                identity = identity.with_budget(budget);
            }

            let mut component =
                ComponentInfo::new(component_type, identity, focusable, name.clone());

            component.id = component_id;
            component.state = state;
            component.exit_reason = exit_reason;

            for (k, v) in metadata {
                component = component.with_metadata(k, v);
            }

            if component.focusable {
                let subscription = InputSubscriptionCap::new(
                    self.next_timestamp(),
                    TaskId::new(),
                    ipc::ChannelId::new(),
                );
                component = component.with_subscription(subscription);
            }

            let component_task_id = TaskId::new();
            let workspace_task_id = TaskId::new();

            let main_view = self
                .view_host
                .create_view(
                    ViewKind::TextBuffer,
                    Some(name.clone()),
                    component_task_id,
                    ipc::ChannelId::new(),
                )
                .map_err(|e| {
                    WorkspaceError::InvalidCommand(format!("Failed to create main view: {}", e))
                })?;
            component = component.with_main_view(main_view);

            let main_sub = self
                .view_host
                .subscribe(main_view.view_id, workspace_task_id, ipc::ChannelId::new())
                .map_err(|e| {
                    WorkspaceError::InvalidCommand(format!(
                        "Failed to subscribe to main view: {}",
                        e
                    ))
                })?;
            self.view_subscriptions.insert(main_view.view_id, main_sub);

            if let Some(frame) = main_frame {
                let remapped = remap_frame(frame, main_view.view_id, component.id);
                let _ = self.view_host.publish_frame(&main_view, remapped);
            }

            let status_view = self
                .view_host
                .create_view(
                    ViewKind::StatusLine,
                    Some(format!("{} - status", name)),
                    component_task_id,
                    ipc::ChannelId::new(),
                )
                .map_err(|e| {
                    WorkspaceError::InvalidCommand(format!("Failed to create status view: {}", e))
                })?;
            component = component.with_status_view(status_view);

            let status_sub = self
                .view_host
                .subscribe(
                    status_view.view_id,
                    workspace_task_id,
                    ipc::ChannelId::new(),
                )
                .map_err(|e| {
                    WorkspaceError::InvalidCommand(format!(
                        "Failed to subscribe to status view: {}",
                        e
                    ))
                })?;
            self.view_subscriptions
                .insert(status_view.view_id, status_sub);

            if let Some(frame) = status_frame {
                let remapped = remap_frame(frame, status_view.view_id, component.id);
                let _ = self.view_host.publish_frame(&status_view, remapped);
            }

            self.components.insert(component.id, component);
        }

        if let Some(focused) = snapshot.focused_component {
            let _ = self.focus_component(focused);
        }

        Ok(())
    }

    /// Gets all view frames for all components
    ///
    /// Returns a map of component ID to (main_view, status_view) frames.
    /// Useful for debugging and deterministic replay.
    pub fn get_all_views(&self) -> HashMap<ComponentId, (Option<ViewFrame>, Option<ViewFrame>)> {
        self.components
            .iter()
            .map(|(id, component)| {
                let main_view = component
                    .main_view
                    .as_ref()
                    .and_then(|handle| self.view_host.get_latest(handle.view_id).ok())
                    .flatten();
                let status_view = component
                    .status_view
                    .as_ref()
                    .and_then(|handle| self.view_host.get_latest(handle.view_id).ok())
                    .flatten();
                (*id, (main_view, status_view))
            })
            .collect()
    }

    /// Gets the view host (for testing or advanced operations)
    pub fn view_host(&self) -> &ViewHost {
        &self.view_host
    }

    /// Gets a mutable reference to the view host (for testing)
    pub fn view_host_mut(&mut self) -> &mut ViewHost {
        &mut self.view_host
    }
}

/// Snapshot of the workspace render state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRenderSnapshot {
    /// ID of focused component (if any)
    pub focused_component: Option<ComponentId>,
    /// Main view frame of focused component
    pub main_view: Option<ViewFrame>,
    /// Status view frame of focused component
    pub status_view: Option<ViewFrame>,
    /// Total number of components
    pub component_count: usize,
    /// Number of running components
    pub running_count: usize,
    /// Workspace status strip content
    pub status_strip: String,
    /// Context breadcrumbs
    pub breadcrumbs: String,
    /// Debug info (only in debug builds)
    #[cfg(debug_assertions)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_info: Option<DebugInfo>,
}

/// Debug information for troubleshooting (gated behind debug_assertions)
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugInfo {
    /// Name of focused component
    pub focused_component_name: Option<String>,
    /// Type of focused component
    pub focused_component_type: Option<ComponentType>,
    /// Last key event decoded
    pub last_key_event: Option<String>,
    /// Last routed component ID
    pub last_routed_to: Option<ComponentId>,
    /// Whether last event was consumed by global keybinding
    pub consumed_by_global: bool,
}

/// Workspace session snapshot format version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSessionFormat {
    pub major: u32,
    pub minor: u32,
}

impl WorkspaceSessionFormat {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Snapshot of a single component within a workspace session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceComponentSnapshot {
    pub component_id: ComponentId,
    pub component_type: ComponentType,
    pub name: String,
    pub focusable: bool,
    pub identity_kind: IdentityKind,
    pub trust_domain: TrustDomain,
    pub budget: Option<ResourceBudget>,
    pub metadata: HashMap<String, String>,
    pub state: ComponentState,
    pub exit_reason: Option<ExitReason>,
    pub main_view: Option<ViewFrame>,
    pub status_view: Option<ViewFrame>,
}

/// Persistent snapshot of the workspace component graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSessionSnapshot {
    pub format_version: WorkspaceSessionFormat,
    pub next_timestamp: u64,
    pub focused_component: Option<ComponentId>,
    pub components: Vec<WorkspaceComponentSnapshot>,
}

fn remap_frame(mut frame: ViewFrame, new_view_id: ViewId, component_id: ComponentId) -> ViewFrame {
    frame.view_id = new_view_id;
    if frame.revision == 0 {
        frame.revision = 1;
    }
    frame.component_id = Some(component_id.to_string());
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use packages::{ComponentSpec, PackageComponentType, PackageFormatVersion, PackageManifest};

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
    fn test_launch_package_components() {
        let mut workspace = create_test_workspace();

        let manifest = PackageManifest {
            format_version: PackageFormatVersion::new(1, 0),
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            components: vec![
                ComponentSpec {
                    id: "editor".to_string(),
                    name: "Editor".to_string(),
                    component_type: PackageComponentType::Editor,
                    entry: "services_editor_vi".to_string(),
                    focusable: true,
                    metadata: HashMap::new(),
                    budget: None,
                },
                ComponentSpec {
                    id: "cli".to_string(),
                    name: "CLI".to_string(),
                    component_type: PackageComponentType::Cli,
                    entry: "cli_console".to_string(),
                    focusable: true,
                    metadata: HashMap::new(),
                    budget: None,
                },
            ],
        };

        let ids = workspace.launch_package(&manifest).unwrap();
        assert_eq!(ids.len(), 2);

        let first = workspace.get_component(ids[0]).unwrap();
        assert_eq!(
            first.metadata.get("package.name"),
            Some(&"demo".to_string())
        );
        assert_eq!(
            first.metadata.get("package.entry"),
            Some(&"services_editor_vi".to_string())
        );
    }

    #[test]
    fn test_save_restore_session() {
        let mut workspace = create_test_workspace();

        let config = LaunchConfig::new(
            ComponentType::Editor,
            "session-editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );
        let component_id = workspace.launch_component(config).unwrap();

        let component = workspace.get_component(component_id).unwrap();
        let main_handle = component.main_view.unwrap();
        let status_handle = component.status_view.unwrap();

        let main_frame = ViewFrame::new(
            main_handle.view_id,
            ViewKind::TextBuffer,
            1,
            view_types::ViewContent::text_buffer(vec!["hello".to_string()]),
            10,
        );
        workspace
            .view_host_mut()
            .publish_frame(&main_handle, main_frame)
            .unwrap();

        let status_frame = ViewFrame::new(
            status_handle.view_id,
            ViewKind::StatusLine,
            1,
            view_types::ViewContent::status_line("ready"),
            11,
        );
        workspace
            .view_host_mut()
            .publish_frame(&status_handle, status_frame)
            .unwrap();

        let snapshot = workspace.save_session();

        let mut restored = create_test_workspace();
        restored.restore_session(snapshot).unwrap();

        assert_eq!(restored.list_components().len(), 1);
        let restored_component = restored.get_component(component_id).unwrap();
        assert_eq!(restored_component.name, "session-editor");

        let render = restored.render_snapshot();
        assert!(render.main_view.is_some());
        assert!(render.status_view.is_some());
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

    #[test]
    fn test_editor_receives_keyevents_after_launch() {
        use input_types::{KeyCode, KeyEvent, Modifiers};

        let mut workspace = create_test_workspace();

        // Launch editor
        let config = LaunchConfig::new(
            ComponentType::Editor,
            "test-editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );

        let editor_id = workspace.launch_component(config).unwrap();

        // Verify editor has focus
        assert_eq!(workspace.get_focused_component(), Some(editor_id));

        // Send 'i' key to enter insert mode
        let i_event = InputEvent::key(KeyEvent::pressed(KeyCode::I, Modifiers::none()));
        let routed_to = workspace.route_input(&i_event);
        
        // Verify event was routed to the editor
        assert_eq!(routed_to, Some(editor_id), "KeyEvent should be routed to editor");

        // Send 'a' key to type character
        let a_event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
        let routed_to2 = workspace.route_input(&a_event);
        
        // Verify event was routed to the editor
        assert_eq!(routed_to2, Some(editor_id), "KeyEvent should be routed to editor");
    }

    #[test]
    fn test_global_binding_consumption() {
        use input_types::{KeyCode, KeyEvent, Modifiers};

        let mut workspace = create_test_workspace();

        // Launch editor to have a focused component
        let config = LaunchConfig::new(
            ComponentType::Editor,
            "test-editor",
            IdentityKind::Component,
            TrustDomain::user(),
        );
        let editor_id = workspace.launch_component(config).unwrap();

        // 1. Test non-bound key ('a') - should be routed to editor
        let a_event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
        let routed_to = workspace.route_input(&a_event);
        assert_eq!(routed_to, Some(editor_id), "'a' should be routed to editor");

        // 2. Test globally bound key (Ctrl+S for Save) - should be consumed globally
        let save_event = InputEvent::key(KeyEvent::pressed(KeyCode::S, Modifiers::CTRL));
        let routed_to_save = workspace.route_input(&save_event);
        assert_eq!(routed_to_save, None, "Ctrl+S should be consumed globally");
    }

    #[test]
    fn test_actionable_error_no_components() {
        let err = WorkspaceError::NoComponents;
        let (message, actions) = err.actionable_message();
        
        assert!(message.contains("No components"));
        assert!(actions.len() > 0);
        assert!(actions.iter().any(|a| a.contains("open")));
    }

    #[test]
    fn test_actionable_error_invalid_command() {
        let err = WorkspaceError::InvalidCommand("unknown".to_string());
        let (message, actions) = err.actionable_message();
        
        assert!(message.contains("Invalid command"));
        assert!(actions.iter().any(|a| a.contains("help")));
    }

    #[test]
    fn test_actionable_error_format() {
        let err = WorkspaceError::NoComponents;
        let formatted = err.format_with_actions();
        
        assert!(formatted.contains("—"));
        assert!(formatted.contains("Try:"));
    }

    #[test]
    fn test_actionable_error_component_not_found() {
        let id = ComponentId::new();
        let err = WorkspaceError::ComponentNotFound(id);
        let (message, actions) = err.actionable_message();
        
        assert!(message.contains("not found"));
        assert!(actions.iter().any(|a| a == "list"));
    }
}
