//! Integration tests for Phase 60 - Unified Output Model
//!
//! These tests demonstrate that the same "views → snapshot → renderer" model
//! works in both simulation and bare-metal contexts.

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use services_view_host::{ViewHost, ViewHostError};
use services_workspace_manager::{ComponentType, LaunchConfig, WorkspaceManager};
use text_renderer_host::TextRenderer;
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

#[test]
fn test_view_publishing_basic() {
    // Create a view host
    let mut view_host = ViewHost::new();

    // Create a view (simulating what a component would do)
    let task_id = core_types::TaskId::new();
    let channel_id = ipc::ChannelId::new();

    let handle = view_host
        .create_view(
            ViewKind::TextBuffer,
            Some("Test View".to_string()),
            task_id,
            channel_id,
        )
        .expect("Failed to create view");

    // Publish a frame
    let content = ViewContent::text_buffer(vec!["Hello".to_string(), "World".to_string()]);
    let frame = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content, 1000);

    view_host
        .publish_frame(&handle, frame.clone())
        .expect("Failed to publish frame");

    // Verify frame was stored
    let latest = view_host
        .get_latest(handle.view_id)
        .expect("Failed to get latest");
    assert!(latest.is_some());
    assert_eq!(latest.unwrap(), frame);
}

#[test]
fn test_workspace_render_snapshot() {
    // Create workspace
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Launch a component
    let config = LaunchConfig::new(
        ComponentType::Editor,
        "test-editor",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let component_id = workspace
        .launch_component(config)
        .expect("Failed to launch component");

    // Get the component's view handles
    let component = workspace.get_component(component_id).expect("Component not found");
    let main_view_handle = component.main_view.expect("No main view");
    let status_view_handle = component.status_view.expect("No status view");

    // Publish content to the views
    let main_content = ViewContent::text_buffer(vec!["Test line 1".to_string()]);
    let main_frame = ViewFrame::new(
        main_view_handle.view_id,
        ViewKind::TextBuffer,
        1,
        main_content,
        1000,
    )
    .with_cursor(CursorPosition::new(0, 4));

    workspace
        .view_host_mut()
        .publish_frame(&main_view_handle, main_frame)
        .expect("Failed to publish main frame");

    let status_content = ViewContent::status_line("INSERT MODE");
    let status_frame = ViewFrame::new(
        status_view_handle.view_id,
        ViewKind::StatusLine,
        1,
        status_content,
        1001,
    );

    workspace
        .view_host_mut()
        .publish_frame(&status_view_handle, status_frame)
        .expect("Failed to publish status frame");

    // Render the snapshot
    let snapshot = workspace.render_snapshot();

    // Verify snapshot contains the focused component's views
    assert_eq!(snapshot.focused_component, Some(component_id));
    assert!(snapshot.main_view.is_some());
    assert!(snapshot.status_view.is_some());

    // Verify main view content
    let main_view = snapshot.main_view.unwrap();
    match main_view.content {
        ViewContent::TextBuffer { ref lines } => {
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0], "Test line 1");
        }
        _ => panic!("Expected TextBuffer content"),
    }
    assert_eq!(main_view.cursor, Some(CursorPosition::new(0, 4)));

    // Verify status view content
    let status_view = snapshot.status_view.unwrap();
    match status_view.content {
        ViewContent::StatusLine { ref text } => {
            assert_eq!(text, "INSERT MODE");
        }
        _ => panic!("Expected StatusLine content"),
    }
}

#[test]
fn test_text_renderer_processes_snapshot() {
    // Create a renderer
    let mut renderer = TextRenderer::new();

    // Create view frames to render
    let main_content = ViewContent::text_buffer(vec!["Line 1".to_string(), "Line 2".to_string()]);
    let main_frame = ViewFrame::new(ViewId::new(), ViewKind::TextBuffer, 1, main_content, 1000)
        .with_cursor(CursorPosition::new(1, 3));

    let status_content = ViewContent::status_line("Ready");
    let status_frame =
        ViewFrame::new(ViewId::new(), ViewKind::StatusLine, 1, status_content, 1001);

    // Render the snapshot
    let output = renderer.render_snapshot(Some(&main_frame), Some(&status_frame));

    // Debug: print output to understand what's being rendered
    println!("Rendered output:\n{}", output);

    // Verify output contains expected content
    assert!(output.contains("Line 1"), "Output missing 'Line 1'");
    assert!(output.contains("Lin|e 2"), "Output missing cursor marker on 'Line 2'");
    assert!(output.contains("Ready"), "Output missing 'Ready'");
    assert!(output.contains("─"), "Output missing separator");

    // Verify cursor marker is present
    assert!(output.contains("|"), "Output missing cursor marker");
}

#[test]
fn test_renderer_revision_tracking() {
    let mut renderer = TextRenderer::new();

    // Create a frame
    let content = ViewContent::text_buffer(vec!["Test".to_string()]);
    let frame1 = ViewFrame::new(ViewId::new(), ViewKind::TextBuffer, 1, content.clone(), 1000);

    // First render should be needed
    assert!(renderer.needs_redraw(Some(&frame1), None));

    // Render it
    let _output = renderer.render_snapshot(Some(&frame1), None);

    // Same revision should not need redraw
    assert!(!renderer.needs_redraw(Some(&frame1), None));

    // New revision should need redraw
    let frame2 = ViewFrame::new(frame1.view_id, ViewKind::TextBuffer, 2, content, 2000);
    assert!(renderer.needs_redraw(Some(&frame2), None));
}

#[test]
fn test_multiple_components_focus_switching() {
    // Create workspace
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "test-workspace",
        0,
    );
    let mut workspace = WorkspaceManager::new(workspace_identity);

    // Launch two components
    let config1 = LaunchConfig::new(
        ComponentType::Editor,
        "editor-1",
        IdentityKind::Component,
        TrustDomain::user(),
    );
    let config2 = LaunchConfig::new(
        ComponentType::Editor,
        "editor-2",
        IdentityKind::Component,
        TrustDomain::user(),
    );

    let id1 = workspace
        .launch_component(config1)
        .expect("Failed to launch component 1");
    let id2 = workspace
        .launch_component(config2)
        .expect("Failed to launch component 2");

    // Second component should have focus initially
    assert_eq!(workspace.get_focused_component(), Some(id2));

    // Switch focus to first component
    workspace
        .focus_component(id1)
        .expect("Failed to focus component 1");
    assert_eq!(workspace.get_focused_component(), Some(id1));

    // Render snapshot should show first component
    let snapshot = workspace.render_snapshot();
    assert_eq!(snapshot.focused_component, Some(id1));
}

#[test]
fn test_view_revision_monotonic_enforcement() {
    let mut view_host = ViewHost::new();

    let task_id = core_types::TaskId::new();
    let channel_id = ipc::ChannelId::new();

    let handle = view_host
        .create_view(ViewKind::TextBuffer, None, task_id, channel_id)
        .expect("Failed to create view");

    // Publish frame with revision 1
    let content1 = ViewContent::text_buffer(vec!["First".to_string()]);
    let frame1 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content1, 1000);
    view_host
        .publish_frame(&handle, frame1)
        .expect("Failed to publish frame 1");

    // Publish frame with revision 2 (OK)
    let content2 = ViewContent::text_buffer(vec!["Second".to_string()]);
    let frame2 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 2, content2, 2000);
    assert!(view_host.publish_frame(&handle, frame2).is_ok());

    // Try to publish frame with revision 1 (should fail - not monotonic)
    let content3 = ViewContent::text_buffer(vec!["Third".to_string()]);
    let frame3 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content3, 3000);
    let result = view_host.publish_frame(&handle, frame3);

    match result {
        Err(ViewHostError::RevisionNotMonotonic { expected, actual }) => {
            assert_eq!(expected, 3);
            assert_eq!(actual, 1);
        }
        _ => panic!("Expected RevisionNotMonotonic error"),
    }
}

#[test]
fn test_session_snapshot_preserves_views() {
    // Create workspace with components
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

    let component_id = workspace
        .launch_component(config)
        .expect("Failed to launch component");

    // Publish content
    let component = workspace.get_component(component_id).expect("Component not found");
    let main_view_handle = component.main_view.expect("No main view");

    let content = ViewContent::text_buffer(vec!["Saved content".to_string()]);
    let frame = ViewFrame::new(main_view_handle.view_id, ViewKind::TextBuffer, 1, content, 1000);

    workspace
        .view_host_mut()
        .publish_frame(&main_view_handle, frame)
        .expect("Failed to publish frame");

    // Save session
    let snapshot = workspace.save_session();

    // Restore to new workspace
    let workspace_identity2 = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "restored-workspace",
        0,
    );
    let mut restored_workspace = WorkspaceManager::new(workspace_identity2);
    restored_workspace
        .restore_session(snapshot)
        .expect("Failed to restore session");

    // Verify content was preserved
    let render = restored_workspace.render_snapshot();
    assert!(render.main_view.is_some());
    let main_view = render.main_view.unwrap();
    match main_view.content {
        ViewContent::TextBuffer { ref lines } => {
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0], "Saved content");
        }
        _ => panic!("Expected TextBuffer content"),
    }
}

#[test]
fn test_view_serialization_roundtrip() {
    // Create a complex view frame
    let view_id = ViewId::new();
    let content = ViewContent::text_buffer(vec![
        "Line 1".to_string(),
        "Line 2".to_string(),
        "Line 3".to_string(),
    ]);
    let frame = ViewFrame::new(view_id, ViewKind::TextBuffer, 42, content, 12345)
        .with_cursor(CursorPosition::new(1, 5))
        .with_title("Test View")
        .with_component_id("comp:test");

    // Serialize to JSON
    let json = serde_json::to_string(&frame).expect("Failed to serialize");

    // Deserialize back
    let deserialized: ViewFrame = serde_json::from_str(&json).expect("Failed to deserialize");

    // Verify all fields match
    assert_eq!(deserialized.view_id, frame.view_id);
    assert_eq!(deserialized.kind, frame.kind);
    assert_eq!(deserialized.revision, frame.revision);
    assert_eq!(deserialized.content, frame.content);
    assert_eq!(deserialized.cursor, frame.cursor);
    assert_eq!(deserialized.title, frame.title);
    assert_eq!(deserialized.component_id, frame.component_id);
    assert_eq!(deserialized.timestamp_ns, frame.timestamp_ns);
}

#[test]
fn test_empty_workspace_renders_placeholder() {
    let workspace_identity = IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "empty-workspace",
        0,
    );
    let workspace = WorkspaceManager::new(workspace_identity);

    // Render empty workspace
    let snapshot = workspace.render_snapshot();

    assert_eq!(snapshot.focused_component, None);
    assert!(snapshot.main_view.is_none());
    assert!(snapshot.status_view.is_none());
    assert_eq!(snapshot.component_count, 0);
    assert_eq!(snapshot.running_count, 0);

    // Renderer should handle None views gracefully
    let mut renderer = TextRenderer::new();
    let output = renderer.render_snapshot(None, None);

    assert!(output.contains("(no view)"));
    assert!(output.contains("(no status)"));
}
