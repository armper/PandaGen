//! # Text Renderer Demo
//!
//! This demo shows the text renderer host in action with:
//! - Workspace manager
//! - Editor component
//! - Simulated input
//! - Text rendering to console
//!
//! This is a HOST application, so it is allowed to print.
//! Components never print - they publish views that the host renders.

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use input_types::{InputEvent, KeyCode, KeyEvent, Modifiers};
use services_editor_vi::Editor;
use services_workspace_manager::{ComponentType, LaunchConfig, WorkspaceManager};
use text_renderer_host::TextRenderer;

fn main() {
    println!("=== PandaGen Text Renderer Demo ===\n");
    println!("This demo shows how the text renderer host works:");
    println!("- Workspace manages components");
    println!("- Editor publishes views");
    println!("- Renderer displays views");
    println!();

    // Create workspace
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "demo-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Launch editor component
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "demo-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let component_id = workspace
        .launch_component(config)
        .expect("Failed to launch editor");

    println!("Launched editor component: {}\n", component_id);

    // Get view handles from the component
    let component = workspace.get_component(component_id).expect("Component not found");
    let main_view_handle = component.main_view.clone().expect("No main view");
    let status_view_handle = component.status_view.clone().expect("No status view");

    // Create an editor instance (simulated)
    let mut editor = Editor::new();
    editor.set_view_handles(main_view_handle.clone(), status_view_handle.clone());

    // Simulate some input events - entering insert mode and typing
    let test_inputs = vec![
        // Enter insert mode
        InputEvent::Key(KeyEvent::pressed(KeyCode::I, Modifiers::none())),
        // Type "Hello Panda"
        InputEvent::Key(KeyEvent::pressed(KeyCode::H, Modifiers::SHIFT)),
        InputEvent::Key(KeyEvent::pressed(KeyCode::E, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::L, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::L, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::O, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::Space, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::P, Modifiers::SHIFT)),
        InputEvent::Key(KeyEvent::pressed(KeyCode::A, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::N, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::D, Modifiers::none())),
        InputEvent::Key(KeyEvent::pressed(KeyCode::A, Modifiers::none())),
        // Press Enter for newline
        InputEvent::Key(KeyEvent::pressed(KeyCode::Enter, Modifiers::none())),
    ];

    // Create renderer
    let mut renderer = TextRenderer::new();

    println!("Simulating typing: 'Hello Panda'");
    println!("{}", "─".repeat(80));
    println!();

    // Process each input and render
    let timestamp = 1000; // Simulated timestamp
    for (i, event) in test_inputs.iter().enumerate() {
        // Process input in editor
        editor.process_input(event.clone()).expect("Failed to process input");

        // Publish views from editor
        editor
            .publish_views(workspace.view_host_mut(), timestamp + i as u64)
            .expect("Failed to publish views");

        // Render the workspace snapshot
        let snapshot = workspace.render_snapshot();

        if renderer.needs_redraw(snapshot.main_view.as_ref(), snapshot.status_view.as_ref()) {
            let output = renderer.render_snapshot(snapshot.main_view.as_ref(), snapshot.status_view.as_ref());
            
            // This is a host, so it is allowed to print
            println!("\n{}", "=".repeat(80));
            println!("After input #{}: {:?}", i + 1, event);
            println!("{}", "=".repeat(80));
            print!("{}", output);
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("Demo complete!");
    println!("{}", "=".repeat(80));
    println!();
    println!("Key points:");
    println!("✓ Workspace manages component lifecycle");
    println!("✓ Editor publishes views (never prints)");
    println!("✓ Renderer consumes views and displays them");
    println!("✓ Host is allowed to print (because it's a host, not a component)");
    println!("✓ Rendering is separated from component logic");
}
