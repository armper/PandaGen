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
fn test_editor_launch_via_command() {
    // This test reproduces the user's scenario: using "open editor" command
    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: None,
        max_steps: 0,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Execute "open editor" command
    runtime.execute_command("open editor").unwrap();

    // Verify editor is launched and has focus
    assert_eq!(runtime.workspace().list_components().len(), 1);
    let focused = runtime.workspace().get_focused_component();
    assert!(focused.is_some(), "Editor should be focused after launch");

    // Get the component and verify it's an editor
    let component_id = focused.unwrap();
    let component = runtime.workspace().get_component(component_id).unwrap();
    assert_eq!(component.component_type, ComponentType::Editor);
    assert!(component.focusable, "Editor should be focusable");

    // Verify the component has focus in the focus manager
    assert_eq!(
        runtime.workspace().get_focused_component(),
        Some(component_id),
        "Editor should have focus"
    );

    // Now test that keyboard events are routed to the editor
    let routed_to = runtime.workspace_mut().route_input(&press_key(KeyCode::I));
    assert_eq!(
        routed_to,
        Some(component_id),
        "Key event should be routed to editor"
    );

    // Type some text
    runtime.workspace_mut().route_input(&press_key(KeyCode::H));
    runtime.workspace_mut().route_input(&press_key(KeyCode::I));

    // Exit insert mode
    runtime
        .workspace_mut()
        .route_input(&press_key(KeyCode::Escape));

    // Verify content
    let snapshot = runtime.snapshot();
    assert!(snapshot.main_view.is_some(), "Editor should have a main view");

    if let Some(view) = &snapshot.main_view {
        if let view_types::ViewContent::TextBuffer { lines } = &view.content {
            assert!(!lines.is_empty(), "Editor buffer should have content");
            assert!(
                lines[0].contains("hi"),
                "Editor should contain 'hi', got: {:?}",
                lines
            );
        }
    }

    // Check debug info in snapshot
    #[cfg(debug_assertions)]
    {
        assert!(
            snapshot.debug_info.is_some(),
            "Debug info should be present in debug builds"
        );
        let debug = snapshot.debug_info.as_ref().unwrap();
        assert_eq!(
            debug.focused_component_type,
            Some(ComponentType::Editor),
            "Debug info should show editor as focused"
        );
        assert_eq!(
            debug.last_routed_to,
            Some(component_id),
            "Debug info should show events routed to editor"
        );
        assert!(
            !debug.consumed_by_global,
            "Keys should not be consumed by global keybindings"
        );
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

#[test]
fn test_open_editor_from_cli() {
    // This test reproduces the actual bug scenario:
    // 1. Launch CLI first (like at boot)
    // 2. Execute "open editor" command
    // 3. Verify editor gets focus (NOT CLI)
    // 4. Verify keys route to editor

    let config = HostRuntimeConfig {
        mode: HostMode::Sim,
        script: None,
        max_steps: 0,
        exit_on_idle: false,
    };

    let mut runtime = HostRuntime::new(config).unwrap();

    // Launch CLI first (like in boot)
    let cli_config = LaunchConfig::new(
        ComponentType::Cli,
        "test-cli",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    let cli_id = runtime.workspace_mut().launch_component(cli_config).unwrap();
    
    // CLI should have focus initially
    assert_eq!(runtime.workspace().get_focused_component(), Some(cli_id));

    // Now execute "open editor" command
    runtime.execute_command("open editor").unwrap();

    // Now we should have 2 components
    assert_eq!(runtime.workspace().list_components().len(), 2);

    // Check which component has focus
    let focused = runtime.workspace().get_focused_component();
    
    // Get the editor component
    let components = runtime.workspace().list_components();
    let editor_comp = components.iter().find(|c| c.component_type == ComponentType::Editor);
    assert!(editor_comp.is_some(), "Editor should exist");
    let editor_id = editor_comp.unwrap().id;
    
    // THE KEY ASSERTION: Editor should have focus after 'open editor'
    assert_eq!(
        focused,
        Some(editor_id),
        "Editor should have focus after 'open editor', but component {:?} has focus instead. CLI={:?}, Editor={:?}",
        focused, cli_id, editor_id
    );

    // Verify keys route to editor
    let routed_to = runtime.workspace_mut().route_input(&press_key(KeyCode::I));
    assert_eq!(
        routed_to,
        Some(editor_id),
        "Key should route to editor, but routed to {:?}",
        routed_to
    );

    // Type some more to make sure it works
    runtime.workspace_mut().route_input(&press_key(KeyCode::H));
    runtime.workspace_mut().route_input(&press_key(KeyCode::I));
    runtime.workspace_mut().route_input(&press_key(KeyCode::Escape));

    // Verify content
    let snapshot = runtime.snapshot();
    if let Some(view) = &snapshot.main_view {
        if let view_types::ViewContent::TextBuffer { lines } = &view.content {
            assert!(
                lines[0].contains("hi"),
                "Editor should contain 'hi', got: {:?}",
                lines
            );
        }
    }
}

#[test]
fn test_editor_escape_routing() {
    // Test that Escape is routed to Editor and not consumed globally
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
    let editor_id = runtime.workspace_mut().launch_component(editor_config).unwrap();

    // Verify initial focus
    assert_eq!(runtime.workspace().get_focused_component(), Some(editor_id));

    // Send Escape
    let esc_event = press_key(KeyCode::Escape);
    let routed_to = runtime.workspace_mut().route_input(&esc_event);

    // Should be routed to editor
    assert_eq!(
        routed_to,
        Some(editor_id),
        "Escape should be routed to editor, but was routed to {:?} (None means global consumption)",
        routed_to
    );
}
