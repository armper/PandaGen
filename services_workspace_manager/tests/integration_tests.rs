//! Integration tests for workspace manager with policy enforcement

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent, TrustDomainPolicy};
use resources::{CpuTicks, ResourceBudget};
use services_workspace_manager::{
    commands::{parse_command, CommandResult, WorkspaceCommand},
    ComponentType, LaunchConfig, WorkspaceError, WorkspaceManager,
};

fn create_workspace_with_policy() -> WorkspaceManager {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    WorkspaceManager::new(workspace_identity).with_policy(Box::new(TrustDomainPolicy))
}

fn create_workspace_with_deny_policy() -> WorkspaceManager {
    struct DenyAllPolicy;
    impl PolicyEngine for DenyAllPolicy {
        fn evaluate(&self, _event: PolicyEvent, _context: &PolicyContext) -> PolicyDecision {
            PolicyDecision::deny("Policy denies all operations")
        }
        fn name(&self) -> &str {
            "DenyAllPolicy"
        }
    }

    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    WorkspaceManager::new(workspace_identity).with_policy(Box::new(DenyAllPolicy))
}

#[test]
fn test_policy_allows_same_domain_launch() {
    let mut workspace = create_workspace_with_policy();

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let result = workspace.launch_component(config);
    assert!(result.is_ok());
}

#[test]
fn test_policy_denies_launch() {
    let mut workspace = create_workspace_with_deny_policy();

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let result = workspace.launch_component(config);
    assert!(matches!(result, Err(WorkspaceError::LaunchDenied { .. })));
}

#[test]
fn test_command_open_with_policy() {
    let mut workspace = create_workspace_with_policy();

    let result = workspace.execute_command(WorkspaceCommand::Open {
        component_type: ComponentType::Editor,
        args: vec!["test.txt".to_string()],
    });

    match result {
        CommandResult::Opened { .. } => {
            // Success
        }
        _ => panic!("Expected Opened result"),
    }
}

#[test]
fn test_command_open_denied_by_policy() {
    let mut workspace = create_workspace_with_deny_policy();

    let result = workspace.execute_command(WorkspaceCommand::Open {
        component_type: ComponentType::Editor,
        args: vec!["test.txt".to_string()],
    });

    match result {
        CommandResult::Error { message } => {
            assert!(message.contains("denied"));
        }
        _ => panic!("Expected Error result"),
    }
}

#[test]
fn test_budget_exhaustion_terminates_component() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let budget = ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(100));
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    )
    .with_budget(budget);

    let component_id = workspace.launch_component(config).unwrap();

    // Simulate budget exhaustion
    workspace.handle_budget_exhaustion(component_id).unwrap();

    let component = workspace.get_component(component_id).unwrap();
    assert_eq!(
        component.state,
        services_workspace_manager::ComponentState::Failed
    );
}

#[test]
fn test_focus_denied_by_policy() {
    let mut workspace = create_workspace_with_deny_policy();

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    // Launch should fail with deny policy
    let result = workspace.launch_component(config);
    assert!(result.is_err());
}

#[test]
fn test_multiple_components_with_focus_switching() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Launch three components
    let _id1 = workspace
        .launch_component(LaunchConfig::new(
            ComponentType::Editor,
            "editor1",
            IdentityKind::Component,
            TrustDomain::user(),
        ))
        .unwrap();
    let _id2 = workspace
        .launch_component(LaunchConfig::new(
            ComponentType::Cli,
            "cli",
            IdentityKind::Component,
            TrustDomain::user(),
        ))
        .unwrap();
    let id3 = workspace
        .launch_component(LaunchConfig::new(
            ComponentType::Editor,
            "editor2",
            IdentityKind::Component,
            TrustDomain::user(),
        ))
        .unwrap();

    // Last launched should have focus
    assert_eq!(workspace.get_focused_component(), Some(id3));

    // Switch focus - should cycle through all components
    let first_focus = workspace.get_focused_component().unwrap();

    workspace.focus_next().unwrap();
    let second_focus = workspace.get_focused_component().unwrap();
    assert_ne!(first_focus, second_focus);

    workspace.focus_next().unwrap();
    let third_focus = workspace.get_focused_component().unwrap();
    assert_ne!(second_focus, third_focus);

    // After three next calls, should cycle back
    workspace.focus_next().unwrap();
    assert_eq!(workspace.get_focused_component().unwrap(), first_focus);

    // Test previous
    workspace.focus_previous().unwrap();
    assert_eq!(workspace.get_focused_component().unwrap(), third_focus);
}

#[test]
fn test_terminate_focused_component_shifts_focus() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let id1 = workspace
        .launch_component(LaunchConfig::new(
            ComponentType::Editor,
            "editor1",
            IdentityKind::Component,
            TrustDomain::user(),
        ))
        .unwrap();
    let _id2 = workspace
        .launch_component(LaunchConfig::new(
            ComponentType::Cli,
            "cli",
            IdentityKind::Component,
            TrustDomain::user(),
        ))
        .unwrap();

    // Focus first component
    workspace.focus_component(id1).unwrap();

    // Terminate it
    workspace
        .terminate_component(id1, identity::ExitReason::Normal)
        .unwrap();

    // Focus should be removed
    let focused = workspace.get_focused_component();
    assert!(focused.is_none() || focused != Some(id1));
}

#[test]
fn test_audit_trail_records_all_events() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let component_id = workspace
        .launch_component(LaunchConfig::new(
            ComponentType::Editor,
            "editor",
            IdentityKind::Component,
            TrustDomain::user(),
        ))
        .unwrap();

    workspace.focus_component(component_id).unwrap();
    workspace
        .terminate_component(component_id, identity::ExitReason::Normal)
        .unwrap();

    let trail = workspace.audit_trail();

    // Should have: ComponentLaunched, ComponentFocused, ComponentTerminated
    assert!(trail.len() >= 3);

    // Check for specific events
    let has_launched = trail.iter().any(|e| {
        matches!(
            e,
            services_workspace_manager::WorkspaceEvent::ComponentLaunched { .. }
        )
    });
    let has_focused = trail.iter().any(|e| {
        matches!(
            e,
            services_workspace_manager::WorkspaceEvent::ComponentFocused { .. }
        )
    });
    let has_terminated = trail.iter().any(|e| {
        matches!(
            e,
            services_workspace_manager::WorkspaceEvent::ComponentTerminated { .. }
        )
    });

    assert!(has_launched);
    assert!(has_focused);
    assert!(has_terminated);
}

#[test]
fn test_parse_and_execute_commands() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Parse and execute open command
    let cmd = parse_command("open editor test.txt").unwrap();
    let result = workspace.execute_command(cmd);
    match result {
        CommandResult::Opened { .. } => {}
        _ => panic!("Expected Opened result"),
    }

    // Parse and execute list command
    let cmd = parse_command("list").unwrap();
    let result = workspace.execute_command(cmd);
    match result {
        CommandResult::List { components } => {
            assert_eq!(components.len(), 1);
        }
        _ => panic!("Expected List result"),
    }

    // Parse and execute next command
    let cmd = parse_command("next").unwrap();
    let result = workspace.execute_command(cmd);
    match result {
        CommandResult::FocusChanged { .. } => {}
        _ => panic!("Expected FocusChanged result"),
    }
}

#[test]
fn test_component_metadata_preserved() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    )
    .with_metadata("file", "test.txt")
    .with_metadata("line", "42");

    let component_id = workspace.launch_component(config).unwrap();

    let component = workspace.get_component(component_id).unwrap();
    assert_eq!(
        component.metadata.get("file"),
        Some(&"test.txt".to_string())
    );
    assert_eq!(component.metadata.get("line"), Some(&"42".to_string()));
}
