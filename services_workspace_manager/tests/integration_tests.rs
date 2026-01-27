//! Integration tests for workspace manager with policy enforcement

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent, TrustDomainPolicy};
use resources::{CpuTicks, ResourceBudget};
use services_workspace_manager::{
    commands::{parse_command, CommandResult, WorkspaceCommand},
    ComponentType, LaunchConfig, WorkspaceError, WorkspaceManager,
};
use uuid::Uuid;

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

#[test]
fn test_component_has_views_on_launch() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let component_id = workspace.launch_component(config).unwrap();

    let component = workspace.get_component(component_id).unwrap();
    assert!(
        component.main_view.is_some(),
        "Component should have main view"
    );
    assert!(
        component.status_view.is_some(),
        "Component should have status view"
    );
}

#[test]
fn test_workspace_render_focused_component() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let component_id = workspace.launch_component(config).unwrap();

    // Render workspace
    let output = workspace.render_snapshot();
    assert_eq!(output.focused_component, Some(component_id));
    assert!(
        output.main_view.is_some(),
        "Should have main view in render output"
    );
    assert!(
        output.status_view.is_some(),
        "Should have status view in render output"
    );
}

#[test]
fn test_workspace_render_switches_with_focus() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let config1 = LaunchConfig::new(
        ComponentType::Editor,
        "editor1",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    let config2 = LaunchConfig::new(
        ComponentType::Cli,
        "cli1",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let id1 = workspace.launch_component(config1).unwrap();
    let id2 = workspace.launch_component(config2).unwrap();

    // Second component should be focused
    let output1 = workspace.render_snapshot();
    assert_eq!(output1.focused_component, Some(id2));

    // Switch focus to first
    workspace.focus_component(id1).unwrap();
    let output2 = workspace.render_snapshot();
    assert_eq!(output2.focused_component, Some(id1));
}

#[test]
fn test_views_cleaned_up_on_terminate() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let component_id = workspace.launch_component(config).unwrap();
    let component = workspace.get_component(component_id).unwrap();
    let main_view_id = component.main_view.as_ref().unwrap().view_id;
    let status_view_id = component.status_view.as_ref().unwrap().view_id;

    // Verify views exist
    assert!(workspace.view_host().get_latest(main_view_id).is_ok());
    assert!(workspace.view_host().get_latest(status_view_id).is_ok());

    // Terminate component
    workspace
        .terminate_component(component_id, identity::ExitReason::Normal)
        .unwrap();

    // Views should be cleaned up
    assert!(workspace.view_host().get_latest(main_view_id).is_err());
    assert!(workspace.view_host().get_latest(status_view_id).is_err());
}

#[test]
fn test_get_all_views() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    let config1 = LaunchConfig::new(
        ComponentType::Editor,
        "editor1",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    let config2 = LaunchConfig::new(
        ComponentType::Cli,
        "cli1",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let id1 = workspace.launch_component(config1).unwrap();
    let id2 = workspace.launch_component(config2).unwrap();

    // Get all views
    let all_views = workspace.get_all_views();
    assert_eq!(all_views.len(), 2);
    assert!(all_views.contains_key(&id1));
    assert!(all_views.contains_key(&id2));

    // Each component should have views
    let (main1, status1) = &all_views[&id1];
    let (main2, status2) = &all_views[&id2];
    assert!(main1.is_some());
    assert!(status1.is_some());
    assert!(main2.is_some());
    assert!(status2.is_some());
}

// ========== Workspace Status and History Integration Tests ==========

#[test]
fn test_command_tracking_in_history() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Execute some commands
    workspace.execute_command(WorkspaceCommand::List);
    workspace.execute_command(WorkspaceCommand::FocusNext);

    // Check history
    let history = workspace.recent_history();
    let commands = history.get_recent_commands();

    assert!(commands.len() >= 2);
    assert!(commands.contains(&"list".to_string()));
    assert!(commands.contains(&"next".to_string()));
}

#[test]
fn test_file_tracking_on_open() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Open editor with file
    workspace.execute_command(WorkspaceCommand::Open {
        component_type: ComponentType::Editor,
        args: vec!["test.txt".to_string()],
    });

    // Check recent files
    let history = workspace.recent_history();
    let files = history.get_recent_files();

    assert!(files.contains(&"test.txt".to_string()));
}

#[test]
fn test_status_updates_on_command() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Open editor
    let result = workspace.execute_command(WorkspaceCommand::Open {
        component_type: ComponentType::Editor,
        args: vec!["test.txt".to_string()],
    });

    // Check last action was set
    let status = workspace.workspace_status();
    assert!(status.last_action.is_some());

    // Clear it
    workspace.workspace_status_mut().clear_last_action();
    assert!(workspace.workspace_status().last_action.is_none());
}

#[test]
fn test_error_tracking_in_history() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Try to focus non-existent component (will error)
    let fake_id = services_workspace_manager::ComponentId::from_uuid(core_types::uuid_tools::new_uuid());
    workspace.execute_command(WorkspaceCommand::Focus {
        component_id: fake_id,
    });

    // Check error history
    let history = workspace.recent_history();
    let errors = history.get_recent_errors();

    assert!(errors.len() > 0);
}

#[test]
fn test_command_palette_accessible() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let workspace = WorkspaceManager::new(workspace_identity);

    // Should have command palette
    let palette = workspace.command_palette();
    let commands = palette.list_commands();

    assert!(commands.len() > 0);
    // Should have some key commands
    assert!(commands.iter().any(|c| c.id.as_str() == "open_editor"));
    assert!(commands.iter().any(|c| c.id.as_str() == "list"));
}

#[test]
fn test_breadcrumbs_accessible() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Initial breadcrumbs
    assert_eq!(workspace.breadcrumbs().format(), "PANDA > ROOT");

    // Can update
    workspace
        .breadcrumbs_mut()
        .push("EDITOR(test.txt)".to_string());
    assert!(workspace
        .breadcrumbs()
        .format()
        .contains("EDITOR(test.txt)"));
}
