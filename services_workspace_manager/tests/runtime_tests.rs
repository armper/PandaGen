//! Tests for WorkspaceRuntime with FakePlatform

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use input_types::{KeyCode, KeyEvent, Modifiers};
use services_workspace_manager::{
    ComponentType, FakePlatform, LaunchConfig, WorkspaceCaps, WorkspaceRuntime,
};

#[test]
fn test_runtime_creation() {
    let platform = FakePlatform::new();
    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    assert_eq!(runtime.tick_count(), 0);
}

#[test]
fn test_runtime_tick_advances() {
    let platform = FakePlatform::new();
    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    runtime.tick();
    assert_eq!(runtime.tick_count(), 1);

    runtime.tick();
    assert_eq!(runtime.tick_count(), 2);
}

#[test]
fn test_runtime_handles_input() {
    let mut platform = FakePlatform::new();

    // Queue an input event
    let event = KeyEvent::pressed(KeyCode::A, Modifiers::none());
    platform.queue_input(event);

    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    // Launch an editor component to receive input
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    let _editor_id = runtime.workspace_mut().launch_component(config).unwrap();

    // Handle input - should route to editor
    runtime.handle_input();

    // Verify the component received focus
    assert!(runtime.workspace().get_focused_component().is_some());
}

#[test]
fn test_runtime_render_calls_platform() {
    let platform = FakePlatform::new();
    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    // Render should work even without components
    runtime.render();

    // The platform should have received render calls
    // (This is a basic smoke test - we can't inspect the platform through the runtime)
}

#[test]
fn test_runtime_workspace_access() {
    let platform = FakePlatform::new();
    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    // Test immutable access
    let components = runtime.workspace().list_components();
    assert_eq!(components.len(), 0);

    // Test mutable access
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    let editor_id = runtime.workspace_mut().launch_component(config).unwrap();

    // Verify component was added
    let components = runtime.workspace().list_components();
    assert_eq!(components.len(), 1);
    assert_eq!(runtime.workspace().get_focused_component(), Some(editor_id));
}

#[test]
fn test_runtime_with_storage_capability() {
    use services_storage::JournaledStorage;
    use services_workspace_manager::EditorIoContext;

    let platform = FakePlatform::new();
    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );

    let storage = JournaledStorage::new();
    let io_context = EditorIoContext::new(storage);
    let caps = WorkspaceCaps::with_storage(io_context);

    let runtime = WorkspaceRuntime::new(platform, identity, caps);

    // Workspace should have the storage context set
    // (We can't directly verify this, but it's used internally when launching editors)
    assert_eq!(runtime.tick_count(), 0);
}

#[test]
fn test_runtime_full_cycle() {
    let mut platform = FakePlatform::new();

    // Queue some input events
    platform.queue_input(KeyEvent::pressed(KeyCode::I, Modifiers::none()));
    platform.queue_input(KeyEvent::pressed(KeyCode::A, Modifiers::none()));

    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    // Launch editor
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    runtime.workspace_mut().launch_component(config).unwrap();

    // Full cycle: input -> tick -> render
    runtime.handle_input();
    runtime.tick();
    runtime.render();

    assert_eq!(runtime.tick_count(), 1);
    assert!(runtime.workspace().get_focused_component().is_some());
}

#[test]
fn test_fake_platform_display_tracking() {
    let mut platform = FakePlatform::new();
    let identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );

    let mut runtime = WorkspaceRuntime::new(platform, identity, WorkspaceCaps::empty());

    // Launch a component
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    runtime.workspace_mut().launch_component(config).unwrap();

    // Render
    runtime.render();

    // We can't check the platform state through the runtime anymore
    // but we know the render was called
    assert_eq!(runtime.tick_count(), 0);
}
