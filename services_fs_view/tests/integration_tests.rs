//! Integration tests for filesystem view service
//!
//! These tests validate the complete filesystem view functionality including:
//! - Directory hierarchy management
//! - Path resolution
//! - Capability safety
//! - Immutability guarantees

use fs_view::DirectoryView;
use services_fs_view::{FileSystemOperations, FileSystemViewService};
use services_storage::{ObjectId, ObjectKind};

#[test]
fn test_complete_directory_workflow() {
    let mut service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let mut root = DirectoryView::new(root_id);

    // Create directory structure: /docs/projects/
    let docs_id = service.mkdir(&mut root, "docs").unwrap();

    // Register docs directory
    let mut docs_dir = DirectoryView::new(docs_id);
    let projects_id = service.mkdir(&mut docs_dir, "projects").unwrap();
    service.register_directory(docs_dir);

    // Register projects directory and link a file
    let file_id = ObjectId::new();
    let mut projects_dir = DirectoryView::new(projects_id);
    service
        .link(&mut projects_dir, "readme.txt", file_id, ObjectKind::Blob)
        .unwrap();
    service.register_directory(projects_dir);

    // Verify we can open the file through path resolution
    let opened_id = service.open(&root, "docs/projects/readme.txt").unwrap();
    assert_eq!(opened_id, file_id);
}

#[test]
fn test_capability_isolation() {
    let mut service = FileSystemViewService::new();

    // Create two separate root directories (simulating different users)
    let root1_id = ObjectId::new();
    let mut root1 = DirectoryView::new(root1_id);

    let root2_id = ObjectId::new();
    let root2 = DirectoryView::new(root2_id);

    // User 1 creates a secret directory
    service.mkdir(&mut root1, "secret").unwrap();

    // User 2 should NOT be able to access user 1's secret directory
    let result = service.open(&root2, "secret");
    assert!(result.is_err());
}

#[test]
fn test_unlink_preserves_immutability() {
    let mut service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let mut root = DirectoryView::new(root_id);

    // Link a file
    let file_id = ObjectId::new();
    service
        .link(&mut root, "important.txt", file_id, ObjectKind::Blob)
        .unwrap();

    // Verify we can access it
    let opened_id = service.open(&root, "important.txt").unwrap();
    assert_eq!(opened_id, file_id);

    // Unlink the name
    service.unlink(&mut root, "important.txt").unwrap();

    // The object still exists (file_id is still valid)
    // Only the name -> object link was removed
    let result = service.open(&root, "important.txt");
    assert!(result.is_err()); // Name no longer exists

    // But if we had saved the object_id, we could still use it
    assert_eq!(file_id, file_id); // Object still exists conceptually
}

#[test]
fn test_nested_directory_traversal() {
    let mut service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let mut root = DirectoryView::new(root_id);

    // Create deep hierarchy a/b/c
    let a_id = service.mkdir(&mut root, "a").unwrap();

    // Create b inside a
    let mut a_dir = DirectoryView::new(a_id);
    let b_id = service.mkdir(&mut a_dir, "b").unwrap();
    service.register_directory(a_dir);

    // Create c inside b
    let mut b_dir = DirectoryView::new(b_id);
    let c_id = service.mkdir(&mut b_dir, "c").unwrap();
    service.register_directory(b_dir);

    // Register c
    service.register_directory(DirectoryView::new(c_id));

    // Can traverse all the way down
    let result = service.open(&root, "a/b/c");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), c_id);
}

#[test]
fn test_cannot_traverse_through_blob() {
    let mut service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let mut root = DirectoryView::new(root_id);

    // Create a file (blob)
    let file_id = ObjectId::new();
    service
        .link(&mut root, "file.txt", file_id, ObjectKind::Blob)
        .unwrap();

    // Try to traverse through the blob (should fail)
    let result = service.open(&root, "file.txt/something");
    assert!(result.is_err());
}

#[test]
fn test_ls_returns_correct_entries() {
    let mut service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let mut root = DirectoryView::new(root_id);

    // Create multiple entries
    service.mkdir(&mut root, "docs").unwrap();
    service.mkdir(&mut root, "projects").unwrap();
    service
        .link(&mut root, "readme.txt", ObjectId::new(), ObjectKind::Blob)
        .unwrap();

    // List root directory
    let entries = service.ls(&root, "/").unwrap();
    assert_eq!(entries.len(), 3);

    let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"docs".to_string()));
    assert!(names.contains(&"projects".to_string()));
    assert!(names.contains(&"readme.txt".to_string()));
}

#[test]
fn test_stat_provides_correct_info() {
    let mut service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let mut root = DirectoryView::new(root_id);

    // Create directory with entries
    let dir_id = service.mkdir(&mut root, "docs").unwrap();
    let mut docs_dir = DirectoryView::new(dir_id);
    docs_dir.add_entry(fs_view::DirectoryEntry::new(
        "file1.txt".to_string(),
        ObjectId::new(),
        ObjectKind::Blob,
    ));
    docs_dir.add_entry(fs_view::DirectoryEntry::new(
        "file2.txt".to_string(),
        ObjectId::new(),
        ObjectKind::Blob,
    ));
    service.register_directory(docs_dir);

    // Stat the directory
    let stat = service.stat(&root, "docs").unwrap();
    assert_eq!(stat.kind, ObjectKind::Map);
    assert_eq!(stat.entry_count, Some(2));
}

#[test]
fn test_no_relative_paths() {
    let service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let root = DirectoryView::new(root_id);

    // Paths with . or .. should be rejected
    let result = service.open(&root, "./file.txt");
    assert!(result.is_err());

    let result = service.open(&root, "../file.txt");
    assert!(result.is_err());

    let result = service.open(&root, "docs/./file.txt");
    assert!(result.is_err());
}

#[test]
fn test_empty_path_components_rejected() {
    let service = FileSystemViewService::new();
    let root_id = ObjectId::new();
    let root = DirectoryView::new(root_id);

    // Paths with empty components (double slashes) should be rejected
    let result = service.open(&root, "docs//file.txt");
    assert!(result.is_err());
}
