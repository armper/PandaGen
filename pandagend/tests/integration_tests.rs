//! Integration tests for pandagend host runtime

use identity::{IdentityKind, TrustDomain};
use pandagend::{HostMode, HostRuntime, HostRuntimeConfig};
use services_workspace_manager::{ComponentType, LaunchConfig};

#[test]
fn test_scripted_editor_session() {
    // Script that opens editor, types text, and saves
    let script = r#"
        i
        "Hello Panda"
        Escape
        ":w"
        Enter
    "#;

    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: Some(script.to_string()),
        max_steps: 20,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Launch editor component
    let editor_config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    runtime
        .workspace_mut()
        .launch_component(editor_config)
        .unwrap();

    // Run the script
    runtime.run().unwrap();

    // Get final snapshot
    let snapshot = runtime.snapshot();

    // Verify that we have a main view
    assert!(snapshot.main_view.is_some());

    // Verify some steps were executed
    assert!(runtime.step_count() > 0);
}

#[test]
fn test_focus_switching() {
    // Script that switches focus between components
    let script = r#"
        wait 10ms
    "#;

    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: Some(script.to_string()),
        max_steps: 5,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Launch two components
    let editor_config = LaunchConfig::new(
        ComponentType::Editor,
        "editor-1",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let cli_config = LaunchConfig::new(
        ComponentType::Cli,
        "cli-1",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let editor_id = runtime
        .workspace_mut()
        .launch_component(editor_config)
        .unwrap();
    let cli_id = runtime
        .workspace_mut()
        .launch_component(cli_config)
        .unwrap();

    // Initially, CLI should have focus (last launched)
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.focused_component, Some(cli_id));

    // Switch to next (wraps to editor)
    runtime.workspace_mut().focus_next().unwrap();
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.focused_component, Some(editor_id));

    // Switch to previous (wraps back to CLI)
    runtime.workspace_mut().focus_previous().unwrap();
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.focused_component, Some(cli_id));
}

#[test]
fn test_host_command_execution() {
    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: None,
        max_steps: 0,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Execute open editor command
    runtime.execute_command("open editor").unwrap();
    assert_eq!(runtime.workspace().list_components().len(), 1);

    // Execute list command (should not error)
    runtime.execute_command("list").unwrap();

    // Execute quit command
    runtime.execute_command("quit").unwrap();
}

#[test]
fn test_no_ansi_escape_codes() {
    // Simple script
    let script = r#"
        i
        "Test"
        Escape
    "#;

    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: Some(script.to_string()),
        max_steps: 10,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Launch editor
    let editor_config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    runtime
        .workspace_mut()
        .launch_component(editor_config)
        .unwrap();

    // Run the script
    runtime.run().unwrap();

    // Get final snapshot and render it
    let snapshot = runtime.snapshot();
    let mut renderer = text_renderer_host::TextRenderer::new();
    let output =
        renderer.render_snapshot(snapshot.main_view.as_ref(), snapshot.status_view.as_ref());

    // Verify no ANSI escape sequences
    assert!(
        !output.contains('\x1b'),
        "Output should not contain ANSI escape sequences"
    );

    // Verify we got some actual output
    assert!(!output.is_empty(), "Output should not be empty");
}

#[test]
fn test_empty_workspace_handling() {
    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: Some("wait 10ms".to_string()),
        max_steps: 1,
        exit_on_idle: true,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Run with no components - should exit cleanly due to exit_on_idle
    let result = runtime.run();
    assert!(result.is_ok());
}

#[test]
fn test_max_steps_limit() {
    let script = r#"
        i
        i
        i
        i
        i
    "#;

    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: Some(script.to_string()),
        max_steps: 3,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();
    runtime.run().unwrap();

    // Should stop at max_steps
    assert_eq!(runtime.step_count(), 3);
}

#[test]
fn test_script_exhaustion() {
    let script = "i\nEscape";

    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: Some(script.to_string()),
        max_steps: 100, // Large number, but script will exhaust first
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();
    runtime.run().unwrap();

    // Should stop when script exhausts (2 steps)
    assert_eq!(runtime.step_count(), 2);
}
