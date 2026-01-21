//! Integration test for editor end-to-end functionality

use identity::{IdentityKind, TrustDomain};
use input_types::{InputEvent, KeyCode, KeyEvent, Modifiers};
use pandagend::{HostMode, HostRuntime, HostRuntimeConfig};
use services_workspace_manager::{ComponentType, LaunchConfig};

fn press_key(code: KeyCode) -> InputEvent {
    InputEvent::key(KeyEvent::pressed(code, Modifiers::none()))
}

#[test]
fn test_editor_launches_and_accepts_input() {
    // Create runtime
    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: None,
        max_steps: 0,
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

    let component_id = runtime
        .workspace_mut()
        .launch_component(editor_config)
        .unwrap();

    // Verify editor is launched
    assert_eq!(runtime.workspace().list_components().len(), 1);
    assert_eq!(
        runtime.workspace().get_focused_component(),
        Some(component_id)
    );

    // Simulate typing: enter insert mode and type "hello"
    runtime.workspace_mut().route_input(&press_key(KeyCode::I));
    runtime.workspace_mut().route_input(&press_key(KeyCode::H));
    runtime.workspace_mut().route_input(&press_key(KeyCode::E));
    runtime.workspace_mut().route_input(&press_key(KeyCode::L));
    runtime.workspace_mut().route_input(&press_key(KeyCode::L));
    runtime.workspace_mut().route_input(&press_key(KeyCode::O));

    // Exit insert mode
    runtime
        .workspace_mut()
        .route_input(&press_key(KeyCode::Escape));

    // Get snapshot and verify editor shows content
    let snapshot = runtime.snapshot();
    assert!(snapshot.main_view.is_some());
    assert!(snapshot.status_view.is_some());

    // Verify the main view contains text
    if let Some(view) = &snapshot.main_view {
        if let view_types::ViewContent::TextBuffer { lines } = &view.content {
            assert!(!lines.is_empty(), "Editor buffer should have content");
            assert!(
                lines[0].contains("hello"),
                "Editor should contain 'hello', got: {:?}",
                lines[0]
            );
        } else {
            panic!("Expected TextBuffer view content");
        }
    }
}

#[test]
fn test_editor_with_scripted_input() {
    // Create runtime with script
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

    // Get final snapshot
    let snapshot = runtime.snapshot();
    assert!(snapshot.main_view.is_some());

    // Verify content was written
    if let Some(view) = &snapshot.main_view {
        if let view_types::ViewContent::TextBuffer { lines } = &view.content {
            assert!(!lines.is_empty(), "Editor buffer should have content");
        }
    }
}
